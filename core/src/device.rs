//! Device abstraction and the KM003C bulk transport.
//!
//! Layer 1 of the acquisition pipeline. Every supported instrument presents
//! the same `Device` interface so the polling loop in `acquisition` does not
//! care which brand is plugged in:
//!
//! - `poll()`      -- fast channel (V/I for the live graph)
//! - `poll_slow()` -- slow auxiliary channel (temperature/ADC); the device may
//!                    implement it as a no-op difference from `poll()`
//!
//! KM003C specifics (bulk over WinUSB/libusb, auth level 0 only):
//! - On Windows IF0 is bound to WinUSB automatically via the device's MS OS
//!   descriptor, so no driver install is needed. On Linux the `powerz` hwmon
//!   driver claims IF0 and must be unbound first.

use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{Buffer, Bulk, In, Out, TransferError};

use crate::powerpico::{self, PowerPico};
use crate::protocol::{
    ATT_ADC, ATT_PD_PACKET, CMD_ACCEPT, CMD_CONNECT, CMD_DISCONNECT, CMD_GET_DATA, PdEvent,
    PdMeasurement, control_header, parse_adc, parse_pd_event_stream, parse_put_data,
};
use crate::witrn::{self, WitrnHid};

pub const VID: u16 = 0x5FC9;
pub const PID: u16 = 0x0063;

const IFACE: u8 = 0;
const EP_OUT: u8 = 0x01;
const EP_IN: u8 = 0x81;

/// Bulk IN buffer size.
///
/// An extended header's `size` field is 10 bits, so one logical packet can be
/// up to 1023 bytes. A chained ADC+PD response can therefore exceed 1024 total.
/// 4096 leaves ample headroom.
const IN_BUF: usize = 4096;

const TIMEOUT: Duration = Duration::from_millis(1000);

/// Why a poll produced no usable data.
///
/// `Empty` is deliberately NOT in here -- a zero-length IN completion is a
/// normal acknowledgement path per the USB transport notes, not a fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    /// Transfer timed out or was cancelled.
    Timeout,
    /// Endpoint stalled. Recoverable via clear_halt.
    Stall,
    /// Device went away. Not recoverable without re-enumeration.
    Disconnected,
    /// Hardware fault or protocol violation reported by the OS.
    Fault,
    /// Bad argument -- a bug on our side, not the device's.
    InvalidArgument,
    /// OS-specific error we don't classify.
    Os(u32),
    /// Transport succeeded but the bytes didn't parse as a PutData response.
    Parse(String),
}

impl PollError {
    /// Whether continuing to poll makes sense.
    pub fn is_recoverable(&self) -> bool {
        !matches!(self, PollError::Disconnected)
    }

    pub fn label(&self) -> &'static str {
        match self {
            PollError::Timeout => "timeout",
            PollError::Stall => "stall",
            PollError::Disconnected => "disconnected",
            PollError::Fault => "fault",
            PollError::InvalidArgument => "invalid-arg",
            PollError::Os(_) => "os-error",
            PollError::Parse(_) => "parse",
        }
    }
}

impl From<TransferError> for PollError {
    fn from(e: TransferError) -> Self {
        match e {
            TransferError::Cancelled => PollError::Timeout,
            TransferError::Stall => PollError::Stall,
            TransferError::Disconnected => PollError::Disconnected,
            TransferError::Fault => PollError::Fault,
            TransferError::InvalidArgument => PollError::InvalidArgument,
            TransferError::Unknown(c) => PollError::Os(c),
        }
    }
}

impl std::fmt::Display for PollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PollError::Os(c) => write!(f, "os-error({c})"),
            PollError::Parse(m) => write!(f, "parse: {m}"),
            other => write!(f, "{}", other.label()),
        }
    }
}

/// One normalized measurement sample, device-agnostic.
///
/// Every backend converts its native format into this: the 12-byte PD block
/// (KM003C) and the 0xFF general report (WITRN family) both end up here, so the
/// acquisition loop and the DTO layer never see vendor byte layouts. The WITRN
/// extras (D+/D- and accumulated Ah/Wh) are `Option`s, `None` on other brands.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Device monotonic milliseconds. For the time axis.
    pub device_ms: u32,
    pub vbus_v: f64,
    /// Signed instrument reading. UI charts the magnitude.
    pub ibus_a: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
    /// Last known temperature. `None` until the backend observes one.
    pub temp_c: Option<f64>,
    /// WITRN only: D+ line voltage.
    pub d_plus_v: Option<f64>,
    /// WITRN only: D- line voltage.
    pub d_minus_v: Option<f64>,
    /// WITRN only: accumulated amp-hours.
    pub amp_hours: Option<f64>,
    /// WITRN only: accumulated watt-hours.
    pub watt_hours: Option<f64>,
}

/// One decoded response from a single poll/read.
#[derive(Debug, Default)]
pub struct Poll {
    pub sample: Option<Sample>,
    pub events: Vec<PdEvent>,
    pub raw_len: usize,
}

/// Result of one poll attempt.
#[derive(Debug)]
pub enum PollOutcome {
    /// The device returned a parseable response.
    Data(Poll),
    /// The device returned nothing new. Normal when there is nothing buffered;
    /// distinct from an error.
    Empty,
}

/// A measurement instrument that the acquisition loop can poll.
pub trait Device: Send {
    fn product(&self) -> &str;
    fn serial(&self) -> &str;
    /// Brand label shown in the picker / UI ("KM003C", "WITRN", "Power-Pico").
    fn kind(&self) -> &'static str;
    /// Stable identifier for this device (see `list_devices` / `open_device`).
    fn id(&self) -> &str;

    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;

    /// Fast channel: one V/I sample (and any queued PD events).
    fn poll(&mut self) -> Result<PollOutcome, PollError>;

    /// Slow auxiliary channel (e.g. ADC for temperature). May read the same
    /// transport as `poll` -- the difference is only meaningful for backends
    /// where the extra data costs a different request.
    fn poll_slow(&mut self) -> Result<PollOutcome, PollError>;

    /// Clear stale buffered responses left by a previous session.
    fn drain(&mut self) -> usize;

    /// Recover from a stall so polling can continue.
    fn clear_stall(&mut self) -> Result<(), String>;
}

/// A discovered, connectable device (name/serial/id for the picker UI).
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub serial: String,
    pub kind: &'static str,
}

/// Enumerate every supported instrument currently attached.
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut out = Vec::new();

    if let Ok(list) = nusb::list_devices().wait() {
        for (index, info) in list
            .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
            .enumerate()
        {
            out.push(DeviceInfo {
                id: format!("km003c:{index}"),
                name: info.product_string().unwrap_or("KM003C").to_string(),
                serial: info.serial_number().unwrap_or("<none>").to_string(),
                kind: "KM003C",
            });
        }
    }

    // The whole WITRN family shares one VID and one HID report protocol
    // (K2 is just 0x0716:0x5060); any WITRN model can be driven the same way.
    if let Ok(api) = hidapi::HidApi::new() {
        for info in api.device_list().filter(|d| d.vendor_id() == witrn::VID) {
            let path = info.path().to_string_lossy().into_owned();
            out.push(DeviceInfo {
                id: format!("witrn:{path}"),
                name: info.product_string().unwrap_or("WITRN").to_string(),
                serial: info.serial_number().unwrap_or("<none>").to_string(),
                kind: "WITRN",
            });
        }
    }

    // Power-Pico is a CDC virtual COM port. It uses ST's generic descriptor
    // (VID 0x0483 / PID 0x5740 / "STM32 Virtual ComPort"), so any STM32 VCP
    // matches here; the stream is verified on connect.
    if let Ok(ports) = serialport::available_ports() {
        for info in ports {
            if let serialport::SerialPortType::UsbPort(usb) = &info.port_type {
                if usb.vid == powerpico::VID && usb.pid == powerpico::PID {
                    out.push(DeviceInfo {
                        id: format!("powerpico:{}", info.port_name),
                        name: "Power-Pico".to_string(),
                        serial: usb
                            .serial_number
                            .clone()
                            .unwrap_or_else(|| info.port_name.clone()),
                        kind: "Power-Pico",
                    });
                }
            }
        }
    }

    out
}

/// Open a device by the id produced by `list_devices`.
pub fn open_device(id: &str) -> Result<Box<dyn Device>, String> {
    if let Some(path) = id.strip_prefix("witrn:") {
        Ok(Box::new(WitrnHid::open(Some(path))?))
    } else if let Some(path) = id.strip_prefix("powerpico:") {
        Ok(Box::new(PowerPico::open(path)?))
    } else if id == "km003c" {
        // Backward-compatible default: the first KM003C.
        Ok(Box::new(Km003c::open()?))
    } else if let Some(index) = id
        .strip_prefix("km003c:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        Ok(Box::new(Km003c::open_index(index)?))
    } else {
        Err(format!("unsupported device id: {id}"))
    }
}

pub struct Km003c {
    ep_out: nusb::Endpoint<Bulk, Out>,
    ep_in: nusb::Endpoint<Bulk, In>,
    tid: u8,
    /// Kept alive so the interface stays claimed for the endpoints' lifetime.
    _interface: nusb::Interface,
    product: String,
    serial: String,
    /// Most recent ADC temperature, carried into every PD sample until the
    /// next slow poll refreshes it.
    last_temp: Option<f64>,
}

impl Km003c {
    pub fn open() -> Result<Self, String> {
        Self::open_index(0)
    }

    /// Open the Nth matching KM003C (0-based) so several identical units can be
    /// told apart in the device picker.
    pub fn open_index(index: usize) -> Result<Self, String> {
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| format!("list_devices failed: {e}"))?
            .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
            .nth(index)
            .ok_or_else(|| {
                format!("KM003C#{index} not found (VID={VID:04X} PID={PID:04X}); is it plugged in?")
            })?;

        let product = info.product_string().unwrap_or("KM003C").to_string();
        let serial = info.serial_number().unwrap_or("<none>").to_string();

        let device = info
            .open()
            .wait()
            .map_err(|e| format!("open failed: {e}"))?;

        let interface = device.claim_interface(IFACE).wait().map_err(|e| {
            format!("claim_interface({IFACE}) failed: {e} -- is another app (Mtools?) holding it?")
        })?;

        let ep_out = interface
            .endpoint::<Bulk, Out>(EP_OUT)
            .map_err(|e| format!("endpoint 0x{EP_OUT:02X} failed: {e}"))?;
        let ep_in = interface
            .endpoint::<Bulk, In>(EP_IN)
            .map_err(|e| format!("endpoint 0x{EP_IN:02X} failed: {e}"))?;

        Ok(Self {
            ep_out,
            ep_in,
            tid: 0,
            _interface: interface,
            product,
            serial,
            last_temp: None,
        })
    }

    fn next_tid(&mut self) -> u8 {
        self.tid = self.tid.wrapping_add(1);
        self.tid
    }

    fn send(&mut self, data: &[u8]) -> Result<(), PollError> {
        let mut buf = Buffer::new(data.len());
        buf.extend_from_slice(data);
        let c = self.ep_out.transfer_blocking(buf, TIMEOUT);
        c.status.map_err(PollError::from)?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Vec<u8>, PollError> {
        let c = self.ep_in.transfer_blocking(Buffer::new(IN_BUF), TIMEOUT);
        c.status.map_err(PollError::from)?;
        Ok(c.buffer[..c.actual_len].to_vec())
    }

    fn poll_attribute(&mut self, attribute: u16) -> Result<PollOutcome, PollError> {
        let tid = self.next_tid();
        self.send(&control_header(CMD_GET_DATA, tid, attribute))?;
        let resp = self.recv()?;

        // A zero-length IN completion is a normal ack, not a failure.
        if resp.is_empty() {
            return Ok(PollOutcome::Empty);
        }

        let packets = parse_put_data(&resp).map_err(PollError::Parse)?;
        let mut pd_meas: Option<PdMeasurement> = None;
        let mut events: Vec<PdEvent> = Vec::new();

        for lp in &packets {
            match lp.attribute {
                ATT_ADC => {
                    if let Some(a) = parse_adc(&lp.payload) {
                        self.last_temp = Some(a.temp_c);
                    }
                }
                ATT_PD_PACKET => {
                    let (m, ev) = parse_pd_event_stream(&lp.payload);
                    pd_meas = m;
                    events = ev;
                }
                _ => {}
            }
        }

        let sample = pd_meas.map(|m| Sample {
            device_ms: m.timestamp_ms,
            vbus_v: m.vbus_mv as f64 / 1000.0,
            ibus_a: m.ibus_ma as f64 / 1000.0,
            cc1_v: m.cc1_mv as f64 / 1000.0,
            cc2_v: m.cc2_mv as f64 / 1000.0,
            temp_c: self.last_temp,
            d_plus_v: None,
            d_minus_v: None,
            amp_hours: None,
            watt_hours: None,
        });

        Ok(PollOutcome::Data(Poll {
            sample,
            events,
            raw_len: resp.len(),
        }))
    }
}

impl Device for Km003c {
    fn product(&self) -> &str {
        &self.product
    }

    fn serial(&self) -> &str {
        &self.serial
    }

    fn kind(&self) -> &'static str {
        "KM003C"
    }

    fn id(&self) -> &str {
        "km003c"
    }

    fn connect(&mut self) -> Result<(), String> {
        let tid = self.next_tid();
        self.send(&control_header(CMD_CONNECT, tid, 0))
            .map_err(|e| format!("Connect send failed: {e}"))?;
        let resp = self
            .recv()
            .map_err(|e| format!("Connect response failed: {e}"))?;
        match resp.first().map(|b| b & 0x7F) {
            Some(CMD_ACCEPT) => Ok(()),
            _ => Err(format!(
                "Connect not accepted: got {resp:02X?} (expected type 0x05)"
            )),
        }
    }

    fn disconnect(&mut self) -> Result<(), String> {
        let tid = self.next_tid();
        self.send(&control_header(CMD_DISCONNECT, tid, 0))
            .map_err(|e| format!("Disconnect failed: {e}"))?;
        let _ = self.recv();
        Ok(())
    }

    /// Fast path: PD measurement block plus any queued PD events.
    fn poll(&mut self) -> Result<PollOutcome, PollError> {
        self.poll_attribute(ATT_PD_PACKET)
    }

    /// Slow path: ADC block (temperature, uV/uA) chained with PD.
    fn poll_slow(&mut self) -> Result<PollOutcome, PollError> {
        self.poll_attribute(ATT_ADC | ATT_PD_PACKET)
    }

    /// Clear stale buffered responses left by a previous session.
    fn drain(&mut self) -> usize {
        let mut total = 0;
        loop {
            let c = self
                .ep_in
                .transfer_blocking(Buffer::new(IN_BUF), Duration::from_millis(50));
            match c.status {
                Ok(()) if c.actual_len > 0 => total += c.actual_len,
                _ => break,
            }
        }
        total
    }

    /// Recover from a stall so polling can continue.
    fn clear_stall(&mut self) -> Result<(), String> {
        self.ep_in
            .clear_halt()
            .wait()
            .map_err(|e| format!("clear_halt failed: {e}"))
    }
}
