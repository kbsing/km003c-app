//! Power-Pico USB CDC backend.
//!
//! Power-Pico (github.com/kingham/Power-Pico) is an STM32F411 USB current
//! meter / low-power analyzer. It presents as a USB CDC virtual COM port and
//! pushes ADC packets continuously. There is no host source in the repo (only
//! a compiled installer in `PC_Client/`), so this backend is transcribed from
//! the firmware (`User/user_AdcDataStrategy.{c,h}`), which is authoritative.
//!
//! Wire protocol (`USB_ADC_Packet_t`, packed, length 11 + data_count*7):
//!   [0..2]  = 0xAA 0x55 header
//!   [2..10] = u64 LE microsecond device timestamp
//!   [10]    = data_count (current firmware sends ADC_TIMES = 10; the 711-byte
//!             README format was data_count = 100 -- the field drives length)
//!   then data_count rows of 7 bytes:
//!     range   u8   (1 = Low, 2 = Mid, 3 = High)
//!     vol_adc u16 LE
//!     cur_adc u16 LE  (selected range's ADC code)
//!     ref_adc u16 LE
//!
//! Physical conversions (firmware defaults):
//!   V = vol_adc * (3000/4095 * 11) / 1000          (11x divider, 3.0 V ref)
//!   I(uA) = (cur_adc - ref_adc) * scale(range)     (INA190 gain 50)
//!     Low  50 ohm  -> 0.293  uA/LSB
//!     Mid  0.5 ohm -> 29.3   uA/LSB
//!     High 5 m ohm -> 2929.7 uA/LSB
//!   minus the voltage-divider loading current (V/1100 uA), matching the
//!   firmware's monitor.
//!
//! Not verified on hardware (no Power-Pico available to this project): the
//! exact default calibration values (they can be changed at runtime on-device,
//! but the host cannot query them), and the CDC descriptor is the generic ST
//! "STM32 Virtual ComPort", so enumeration matches any STM32 VCP -- the stream
//! header is verified on connect to reject non-Power-Pico devices.

use std::io::Read;
use std::time::{Duration, Instant};

use serialport::SerialPort;

use crate::device::{Device, Poll, PollError, PollOutcome, Sample};

/// ST's generic CDC VID/PID as programmed in `USB_DEVICE/App/usbd_desc.c`.
pub const VID: u16 = 0x0483;
pub const PID: u16 = 0x5740;

const PACKET_HEADER_0: u8 = 0xAA;
const PACKET_HEADER_1: u8 = 0x55;
const MAX_DATA_COUNT: usize = 100;

const BAUD: u32 = 115200;
const READ_TIMEOUT: Duration = Duration::from_millis(100);
const CONNECT_VERIFY_TIMEOUT: Duration = Duration::from_millis(1000);

/// Firmware default current scale in uA per ADC LSB: `3.0/4096/50/shunt`.
const SCALE_LOW_UA: f64 = 3.0 / 4096.0 / 50.0 / 50.0 * 1_000_000.0;
const SCALE_MID_UA: f64 = 3.0 / 4096.0 / 50.0 / 0.5 * 1_000_000.0;
const SCALE_HIGH_UA: f64 = 3.0 / 4096.0 / 50.0 / 0.005 * 1_000_000.0;

fn range_scale_ua(range: u8) -> Option<f64> {
    match range {
        1 => Some(SCALE_LOW_UA),
        2 => Some(SCALE_MID_UA),
        3 => Some(SCALE_HIGH_UA),
        _ => None,
    }
}

/// Voltage divider: `(1000k + 100k) / 100k` = 11x, 3.0 V ADC reference.
fn scale_voltage(vol_adc: u16) -> f64 {
    vol_adc as f64 * (3000.0 / 4095.0 * 11.0) / 1000.0
}

/// Convert one 7-byte sample row into a `Sample`. Unknown ranges yield `None`.
fn sample_from_row(
    range: u8,
    vol_adc: u16,
    cur_adc: u16,
    ref_adc: u16,
    device_ms: u32,
) -> Option<Sample> {
    let scale_ua = range_scale_ua(range)?;
    let delta = cur_adc as i32 - ref_adc as i32;
    let current_ua = delta as f64 * scale_ua;
    let vbus_v = scale_voltage(vol_adc);
    // Loading current of the 1.1 M ohm divider across VBUS, as the firmware
    // subtracts it from the monitor (`voltage_mv / 11` in 10 nA units).
    let loading_ua = vbus_v * 1000.0 / 1100.0;
    Some(Sample {
        device_ms,
        vbus_v,
        ibus_a: (current_ua - loading_ua) / 1_000_000.0,
        cc1_v: 0.0,
        cc2_v: 0.0,
        temp_c: None,
        d_plus_v: None,
        d_minus_v: None,
        amp_hours: None,
        watt_hours: None,
    })
}

/// Parse one complete packet and return its most recent sample.
fn parse_packet(buf: &[u8]) -> Option<Sample> {
    if buf.len() < 11 || buf[0] != PACKET_HEADER_0 || buf[1] != PACKET_HEADER_1 {
        return None;
    }
    let ts_us = u64::from_le_bytes(buf[2..10].try_into().ok()?);
    let data_count = buf[10] as usize;
    if data_count == 0 || data_count > MAX_DATA_COUNT {
        return None;
    }
    let device_ms = (ts_us / 1000) as u32;
    let end = 11 + data_count * 7;
    if buf.len() < end {
        return None;
    }
    // Validate every row's range byte, not just the converted last one: a
    // false 0xAA 0x55 inside sample data can carry a plausible data_count,
    // and arbitrary stream bytes almost never pass a 1-3 check on all rows.
    if !buf[11..end].chunks_exact(7).all(|row| matches!(row[0], 1..=3)) {
        return None;
    }
    let row = &buf[end - 7..end];
    sample_from_row(
        row[0],
        u16::from_le_bytes([row[1], row[2]]),
        u16::from_le_bytes([row[3], row[4]]),
        u16::from_le_bytes([row[5], row[6]]),
        device_ms,
    )
}

pub struct PowerPico {
    port: Box<dyn SerialPort>,
    product: String,
    serial: String,
    id: String,
    /// Serial reads can split packets arbitrarily; keep the tail across polls.
    buffer: Vec<u8>,
}

impl PowerPico {
    /// Open the CDC port by name (e.g. `COM3`).
    pub fn open(path: &str) -> Result<Self, String> {
        let port = serialport::new(path, BAUD)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|e| format!("open Power-Pico at {path} failed: {e}"))?;
        let _ = port.clear(serialport::ClearBuffer::Input);
        Ok(Self {
            port,
            product: "Power-Pico".to_string(),
            serial: path.to_string(),
            id: format!("powerpico:{path}"),
            buffer: Vec::new(),
        })
    }
}

/// Pull complete packets from the reassembly buffer, returning the most recent
/// sample. Older bytes are dropped when the tail has no header (the device
/// streams far faster than the ~50 Hz display needs).
fn parse_buffered(buffer: &mut Vec<u8>) -> Option<Sample> {
    let mut last = None;
    loop {
        let Some(off) = find_header(buffer) else {
            // Keep at most 2 bytes so a header split across reads survives.
            let keep = buffer.len().min(2);
            buffer.drain(..buffer.len() - keep);
            return last;
        };
        if off > 0 {
            buffer.drain(..off);
        }
        if buffer.len() < 11 {
            return last;
        }
        let data_count = buffer[10] as usize;
        let packet_len = 11 + data_count * 7;
        if data_count == 0 || data_count > MAX_DATA_COUNT {
            // False header (0xAA 0x55 inside sample data): drop one byte and
            // rescan.
            buffer.drain(..1);
            continue;
        }
        if buffer.len() < packet_len {
            // Incomplete packet; wait for the rest. Keep the whole tail.
            return last;
        }
        if let Some(sample) = parse_packet(&buffer[..packet_len]) {
            last = Some(sample);
        }
        buffer.drain(..packet_len);
    }
}

/// Read available bytes and return the most recent complete sample.
fn read_poll(port: &mut dyn SerialPort, buffer: &mut Vec<u8>) -> Result<PollOutcome, PollError> {
    let mut tmp = [0u8; 8192];
    match port.read(&mut tmp) {
        Ok(n) if n > 0 => buffer.extend_from_slice(&tmp[..n]),
        Ok(_) => {} // read timeout: nothing new buffered
        Err(_) => return Err(PollError::Disconnected),
    }
    match parse_buffered(buffer) {
        Some(sample) => Ok(PollOutcome::Data(Poll {
            sample: Some(sample),
            events: Vec::new(),
            raw_len: 0,
        })),
        None => Ok(PollOutcome::Empty),
    }
}

/// Scan for the 0xAA 0x55 packet header.
fn find_header(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    for i in 0..buf.len() - 1 {
        if buf[i] == PACKET_HEADER_0 && buf[i + 1] == PACKET_HEADER_1 {
            return Some(i);
        }
    }
    None
}

impl Device for PowerPico {
    fn product(&self) -> &str {
        &self.product
    }

    fn serial(&self) -> &str {
        &self.serial
    }

    fn kind(&self) -> &'static str {
        "Power-Pico"
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn connect(&mut self) -> Result<(), String> {
        // The device pushes packets as soon as the port is open. Confirm we can
        // actually see a valid 0xAA 0x55 stream so a generic STM32 VCP (same
        // VID/PID) is rejected instead of silently showing no data.
        let deadline = Instant::now() + CONNECT_VERIFY_TIMEOUT;
        while Instant::now() < deadline {
            let mut tmp = [0u8; 8192];
            match self.port.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    self.buffer.extend_from_slice(&tmp[..n]);
                    if parse_buffered(&mut self.buffer).is_some() {
                        return Ok(());
                    }
                }
                Ok(_) => {}
                Err(e) => return Err(format!("Power-Pico 读取失败：{e}")),
            }
        }
        Err(
            "Power-Pico 未返回有效数据流：请确认该端口是 Power-Pico，且未被其它程序占用"
                .to_string(),
        )
    }

    fn disconnect(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn poll(&mut self) -> Result<PollOutcome, PollError> {
        read_poll(self.port.as_mut(), &mut self.buffer)
    }

    fn poll_slow(&mut self) -> Result<PollOutcome, PollError> {
        read_poll(self.port.as_mut(), &mut self.buffer)
    }

    fn drain(&mut self) -> usize {
        self.buffer.clear();
        let mut total = 0;
        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            let mut tmp = [0u8; 8192];
            match self.port.read(&mut tmp) {
                Ok(n) if n > 0 => total += n,
                _ => break,
            }
        }
        total
    }

    fn clear_stall(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(data_count: usize, ts_us: u64, last: [u8; 7]) -> Vec<u8> {
        let mut p = Vec::with_capacity(11 + data_count * 7);
        p.push(PACKET_HEADER_0);
        p.push(PACKET_HEADER_1);
        p.extend_from_slice(&ts_us.to_le_bytes());
        p.push(data_count as u8);
        for _ in 0..data_count.saturating_sub(1) {
            p.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0]);
        }
        p.extend_from_slice(&last);
        p
    }

    #[test]
    fn parses_packet_and_converts_voltage() {
        // High range (3), vol_adc = 4095 -> 33.0 V nominal; ref==cur -> ~0 A.
        let p = packet(10, 5_000_000, [3, 0xFF, 0x0F, 0x00, 0x00, 0x00, 0x00]);
        let s = parse_packet(&p).expect("parses");
        assert_eq!(s.device_ms, 5_000);
        assert!((s.vbus_v - 33.0).abs() < 0.01, "got {}", s.vbus_v);
        assert!((s.ibus_a).abs() < 1e-4, "got {}", s.ibus_a);
    }

    #[test]
    fn parses_old_100_sample_format() {
        // README's 711-byte format is the same struct with data_count = 100.
        // vol_adc = 1280 (0x0500) -> ~10.3 V.
        let p = packet(100, 42_000_000, [2, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(p.len(), 711);
        let s = parse_packet(&p).expect("parses");
        assert_eq!(s.device_ms, 42_000);
        assert!((s.vbus_v - 10.318).abs() < 0.01, "got {}", s.vbus_v);
    }

    #[test]
    fn current_scale_depends_on_range() {
        // 1000 LSB delta on Mid (0.5 ohm, 29.3 uA/LSB) -> ~29.3 mA.
        let s = sample_from_row(2, 1280, 1000 + 2048, 2048, 1).unwrap();
        let expected_ua = 1000.0 * SCALE_MID_UA - s.vbus_v * 1000.0 / 1100.0;
        assert!(
            (s.ibus_a * 1e6 - expected_ua).abs() < 0.1,
            "got {} uA",
            s.ibus_a * 1e6
        );

        // Same delta on High (5 m ohm, ~2.93 mA/LSB) -> ~2.93 A.
        let s = sample_from_row(3, 1280, 1000 + 2048, 2048, 1).unwrap();
        let expected_ua = 1000.0 * SCALE_HIGH_UA - s.vbus_v * 1000.0 / 1100.0;
        assert!(
            (s.ibus_a * 1e6 - expected_ua).abs() < 10.0,
            "got {} uA",
            s.ibus_a * 1e6
        );
    }

    #[test]
    fn unknown_range_yields_none() {
        assert!(sample_from_row(0, 1280, 0, 0, 1).is_none());
        assert!(sample_from_row(4, 1280, 0, 0, 1).is_none());
    }

    #[test]
    fn invalid_range_in_any_row_rejects_the_packet() {
        // A false 0xAA 0x55 inside sample data can carry a plausible
        // data_count; a bad range byte in ANY row must reject the frame
        // instead of yielding a misaligned sample from the last row.
        let mut p = packet(10, 1_000_000, [3, 0, 0, 0, 0, 0, 0]);
        p[11] = 0; // first row's range byte is now invalid
        assert!(parse_packet(&p).is_none());
    }

    #[test]
    fn stream_reassembly_keeps_last_sample() {
        let mut buffer = Vec::new();
        let mut stream = Vec::new();
        // Two packets back to back, second one later in time.
        stream.extend(packet(10, 1_000_000, [1, 0, 0, 0, 0, 0, 0]));
        stream.extend(packet(10, 2_000_000, [3, 0, 0, 0, 0, 0, 0]));
        // Feed it in awkward chunks to exercise tail handling.
        for chunk in stream.chunks(13) {
            buffer.extend_from_slice(chunk);
        }
        let s = parse_buffered(&mut buffer).expect("parses");
        assert_eq!(s.device_ms, 2_000);
        assert!(buffer.is_empty());
    }

    #[test]
    fn garbage_tail_resyncs_on_header() {
        let mut buffer = Vec::new();
        // Leading garbage (may contain a false 0xAA 0x55 in the timestamp).
        let mut stream = vec![0x00, 0xAA, 0x55, 0xFF, 0xAA, 0x55];
        let good = packet(10, 3_000_000, [3, 0, 0, 0, 0, 0, 0]);
        stream.extend_from_slice(&good);
        buffer.extend_from_slice(&stream);
        let s = parse_buffered(&mut buffer).expect("parses");
        assert_eq!(s.device_ms, 3_000);
    }
}
