//! WITRN USB-HID backend.
//!
//! The WITRN (维简) instrument family -- K2, K2C, K3, KM20, ... -- shares one
//! HID report protocol and one vendor ID (`0x0716`), so this backend matches
//! the whole family, not a single model. This is a different instrument family
//! from the KM003C: instead of a request/response bulk protocol it streams
//! fixed 64-byte HID reports as soon as the interface is opened. The first
//! byte discriminates the payload:
//!
//! - `0xFF` (general): live V/I/T/CC measurement plus accumulated Ah/Wh etc.
//! - `0xFE` (pd): one captured USB PD packet (SOP byte + PD wire, optionally CRC).
//!
//! Field offsets below are transcribed from `WITRN_HID_API` (`witrnhid/core.py`,
//! `general_msg`), which is the reference implementation for this device. The
//! PD framing (length byte at report[1], SOP at report[2]) follows the
//! reference `witrnhid` implementation: `report[3..len+2]` is passed to the
//! PD parser as-is. Some firmware/export formats may append a four-byte CRC,
//! so `parse_pd` removes it only when the PD header proves that the report is
//! exactly four bytes longer than the message itself.
//!
//! Not yet verified on hardware (no WITRN device available to this project):
//! the general report's push rate, and whether extended/PD messages always
//! arrive unchunked in a single report. The parser is deliberately
//! conservative -- malformed reports become `PollOutcome::Empty` or a `PdEvent`
//! whose wire fails `decode_pd_wire`, never a silent wrong reading.

use std::time::Instant;

use hidapi::{HidApi, HidDevice};

use crate::device::{Device, Poll, PollError, PollOutcome, Sample};
use crate::protocol::PdEvent;

/// WITRN vendor ID. The K2 is `0x0716:0x5060`; other models use the same VID,
/// so the picker enumerates every device under it.
pub const VID: u16 = 0x0716;

/// WITRN HID report payload length (no report id byte). Parsing assumes this
/// layout; `READ_BUF` is larger so a device with a report id or a slightly
/// different report size does not trip the transport read.
const REPORT_LEN: usize = 64;
/// Size of the buffer handed to hidapi for one read.
const READ_BUF: usize = 256;
/// Read timeout. When the device is idle a poll blocks for this long and
/// returns `Empty`, letting the acquisition loop check its stop flag.
const READ_TIMEOUT_MS: i32 = 200;
/// Drain timeout used to discard stale reports on open.
const DRAIN_TIMEOUT_MS: i32 = 50;
/// Upper bound on how many reports `drain` will read. The WITRN family pushes
/// reports continuously, so "read until empty" would never terminate.
const DRAIN_MAX_READS: usize = 20;

const REPORT_GENERAL: u8 = 0xFF;
const REPORT_PD: u8 = 0xFE;

/// K2 SOP bytes (per `WITRN_HID_API`) mapped onto the convention the rest of
/// the app uses (KM003C: 0 = SOP, 1 = SOP', ...). Without this normalization
/// the primary-SOP negotiation state machine would treat every K2 message as
/// cable (SOP') traffic.
fn normalize_sop(sop: u8) -> u8 {
    match sop {
        224 => 0, // SOP
        192 => 1, // SOP'
        160 => 2, // SOP''
        128 => 3, // SOP'_DEBUG
        96 => 4,  // SOP''_DEBUG
        other => other,
    }
}

/// Build a `Sample` from a 0xFF general report. Returns `None` if the report
/// is truncated or the measured values are not finite (garbage report).
///
/// D+/D- (volts) and accumulated Ah/Wh come from the same report and are kept
/// as `Option`s (individual non-finite values become `None` rather than
/// dropping the whole sample).
fn parse_general(report: &[u8], device_ms: u32) -> Option<Sample> {
    if report.len() < 57 {
        return None;
    }
    let f32_at = |o: usize| f32::from_le_bytes(report[o..o + 4].try_into().unwrap()) as f64;
    let vbus_v = f32_at(46);
    let ibus_a = f32_at(50);
    let temp_c = f32_at(42);
    let cc1_v = report[55] as f64 / 10.0;
    let cc2_v = report[56] as f64 / 10.0;
    let d_plus_v = f32_at(30);
    let d_minus_v = f32_at(34);
    let amp_hours = f32_at(14);
    let watt_hours = f32_at(18);

    if !vbus_v.is_finite() || !ibus_a.is_finite() || !temp_c.is_finite() {
        return None;
    }
    let fin = |v: f64| v.is_finite().then_some(v);
    Some(Sample {
        device_ms,
        vbus_v,
        ibus_a,
        cc1_v,
        cc2_v,
        temp_c: Some(temp_c),
        d_plus_v: fin(d_plus_v),
        d_minus_v: fin(d_minus_v),
        amp_hours: fin(amp_hours),
        watt_hours: fin(watt_hours),
    })
}

/// Build PD events from a 0xFE report.
///
/// Layout (index within the report):
///   [0]  = 0xFE marker
///   [1]  = message length, counting the SOP byte (i.e. PD packet + 1)
///   [2]  = SOP byte
///   [3..] = PD packet: header + data objects (and, on some firmware, CRC)
///
/// The reference parser does not require a CRC and the observed WITRN frame
/// length is the PD header plus its data objects. Only an unambiguous trailing
/// four-byte CRC is removed; otherwise the packet is kept intact so a data
/// object is not mistaken for a checksum.
fn parse_pd(report: &[u8], device_ms: u32) -> Vec<PdEvent> {
    let Some(&len_byte) = report.get(1) else {
        return Vec::new();
    };
    let end = (len_byte as usize + 2).min(report.len());
    // Need at least marker + length + SOP + the 2-byte PD header. WITRN's
    // reference format commonly omits CRC, including for control messages.
    if end < 5 {
        return Vec::new();
    }
    let sop = normalize_sop(report[2]);
    let packet = &report[3..end];
    let wire_len = pd_wire_len(packet);
    let wire = match wire_len {
        // A few captures include CRC32 after the message. Strip it only when
        // the header-derived length makes that interpretation exact.
        Some(expected) if packet.len() == expected + 4 => &packet[..expected],
        _ => packet,
    };
    vec![PdEvent::Message {
        timestamp_ms: device_ms,
        sop,
        wire: wire.to_vec(),
        raw: report[..end].to_vec(),
    }]
}

/// Return the USB-PD message length implied by its header, excluding any CRC.
/// This mirrors the length rules in `decode_pd_wire` without decoding the
/// payload. `None` means the packet is too short to inspect safely.
fn pd_wire_len(packet: &[u8]) -> Option<usize> {
    if packet.len() < 2 {
        return None;
    }
    let hdr = u16::from_le_bytes([packet[0], packet[1]]);
    let num_objects = ((hdr >> 12) & 0x7) as usize;
    if (hdr >> 15) & 1 == 0 {
        return Some(2 + num_objects * 4);
    }
    if packet.len() < 4 {
        return None;
    }
    let ext_hdr = u16::from_le_bytes([packet[2], packet[3]]);
    let data_size = (ext_hdr & 0x1FF) as usize;
    let chunked = ((ext_hdr >> 15) & 1) == 1;
    if chunked && (data_size > 26 || ((ext_hdr >> 11) & 0xF) > 0 || ((ext_hdr >> 10) & 1) == 1) {
        // Chunked extended messages occupy the number of 32-bit objects from
        // the PD header, including padding in the final object.
        return Some(2 + num_objects * 4);
    }
    Some(2 + (2 + data_size).next_multiple_of(4))
}

pub struct WitrnHid {
    dev: HidDevice,
    product: String,
    serial: String,
    id: String,
    started: Instant,
}

impl WitrnHid {
    /// Open a WITRN device. `path` comes from `list_devices` (the `witrn:`
    /// suffix) and may be any model under VID `0x0716`; a `None` path opens the
    /// first WITRN device found.
    pub fn open(path: Option<&str>) -> Result<Self, String> {
        let api = HidApi::new().map_err(|e| format!("hidapi init failed: {e}"))?;
        let dev = match path {
            Some(path) => {
                let c_path = std::ffi::CString::new(path)
                    .map_err(|_| "device path contains a NUL byte".to_string())?;
                api.open_path(&c_path)
                    .map_err(|e| format!("open WITRN device at {path} failed: {e}"))?
            }
            None => {
                let info = api
                    .device_list()
                    .find(|info| info.vendor_id() == VID)
                    .ok_or_else(|| format!("WITRN device not found (VID={VID:04X})"))?;
                api.open_path(info.path())
                    .map_err(|e| format!("open WITRN device failed: {e}"))?
            }
        };

        let product = dev
            .get_product_string()
            .ok()
            .flatten()
            .unwrap_or_else(|| "WITRN".to_string());
        let serial = dev
            .get_serial_number_string()
            .ok()
            .flatten()
            .unwrap_or_else(|| "<none>".to_string());
        let id = path
            .map(|p| format!("witrn:{p}"))
            .unwrap_or_else(|| "witrn".to_string());

        Ok(Self {
            dev,
            product,
            serial,
            id,
            started: Instant::now(),
        })
    }

    fn read_report(&mut self) -> Result<PollOutcome, PollError> {
        let mut buf = [0u8; READ_BUF];
        let n = self
            .dev
            .read_timeout(&mut buf, READ_TIMEOUT_MS)
            .map_err(|_| PollError::Disconnected)?;
        if n == 0 {
            return Ok(PollOutcome::Empty);
        }
        let report = &buf[..n.min(REPORT_LEN)];
        let device_ms = self.started.elapsed().as_millis() as u32;

        match report.first().copied() {
            Some(REPORT_GENERAL) => Ok(PollOutcome::Data(Poll {
                sample: parse_general(report, device_ms),
                events: Vec::new(),
                raw_len: report.len(),
            })),
            Some(REPORT_PD) => Ok(PollOutcome::Data(Poll {
                sample: None,
                events: parse_pd(report, device_ms),
                raw_len: report.len(),
            })),
            _ => Ok(PollOutcome::Empty),
        }
    }
}

impl Device for WitrnHid {
    fn product(&self) -> &str {
        &self.product
    }

    fn serial(&self) -> &str {
        &self.serial
    }

    fn kind(&self) -> &'static str {
        "WITRN"
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn connect(&mut self) -> Result<(), String> {
        // The K2 streams HID reports as soon as the interface is opened; there
        // is no separate connect handshake (WITRN_HID_API opens and reads).
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn poll(&mut self) -> Result<PollOutcome, PollError> {
        self.read_report()
    }

    fn poll_slow(&mut self) -> Result<PollOutcome, PollError> {
        self.read_report()
    }

    /// Discard buffered reports. Must be bounded: the WITRN family pushes
    /// reports continuously, so a "read until empty" loop would never return.
    fn drain(&mut self) -> usize {
        let mut total = 0;
        for _ in 0..DRAIN_MAX_READS {
            let mut buf = [0u8; READ_BUF];
            match self.dev.read_timeout(&mut buf, DRAIN_TIMEOUT_MS) {
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

    fn general_report(vbus: f32, current: f32, temp: f32, cc1: u8, cc2: u8) -> Vec<u8> {
        let mut report = vec![0u8; REPORT_LEN];
        report[0] = REPORT_GENERAL;
        // Ah / Wh at [14..22], D+ / D- at [30..38], temp at [42..46].
        report[14..18].copy_from_slice(&1.25f32.to_le_bytes());
        report[18..22].copy_from_slice(&6.5f32.to_le_bytes());
        report[30..34].copy_from_slice(&0.9f32.to_le_bytes());
        report[34..38].copy_from_slice(&0.1f32.to_le_bytes());
        report[42..46].copy_from_slice(&temp.to_le_bytes());
        report[46..50].copy_from_slice(&vbus.to_le_bytes());
        report[50..54].copy_from_slice(&current.to_le_bytes());
        report[55] = cc1;
        report[56] = cc2;
        report
    }

    #[test]
    fn general_report_parses_into_sample() {
        let report = general_report(9.7, -3.2, 27.8, 12, 0);
        let sample = parse_general(&report, 1234).expect("parses");
        assert_eq!(sample.device_ms, 1234);
        assert!((sample.vbus_v - 9.7).abs() < 1e-4);
        assert!((sample.ibus_a - -3.2).abs() < 1e-4);
        assert!((sample.temp_c.unwrap() - 27.8).abs() < 1e-4);
        assert!((sample.cc1_v - 1.2).abs() < 1e-9);
        assert_eq!(sample.cc2_v, 0.0);
        // WITRN extras ride along on the same report.
        assert!((sample.d_plus_v.unwrap() - 0.9).abs() < 1e-4);
        assert!((sample.d_minus_v.unwrap() - 0.1).abs() < 1e-4);
        assert!((sample.amp_hours.unwrap() - 1.25).abs() < 1e-4);
        assert!((sample.watt_hours.unwrap() - 6.5).abs() < 1e-4);
    }

    #[test]
    fn truncated_or_non_finite_general_is_none() {
        assert!(parse_general(&[REPORT_GENERAL; 10], 0).is_none());
        let report = general_report(f32::NAN, 1.0, 25.0, 12, 0);
        assert!(parse_general(&report, 0).is_none());
    }

    /// Build a 0xFE report carrying one PD packet: sop byte, a 2-byte control
    /// header, and a 4-byte CRC. `len` field counts the SOP byte, matching the
    /// WITRN framing.
    fn pd_report(sop: u8, header: [u8; 2], crc: [u8; 4]) -> Vec<u8> {
        let mut report = vec![0u8; REPORT_LEN];
        report[0] = REPORT_PD;
        report[1] = 1 + 2 + 4; // sop + header + crc
        report[2] = sop;
        report[3..5].copy_from_slice(&header);
        report[5..9].copy_from_slice(&crc);
        report
    }

    #[test]
    fn pd_report_strips_crc_and_normalizes_sop() {
        let report = pd_report(224, [0x06, 0x00], [0xAA, 0xBB, 0xCC, 0xDD]);
        let events = parse_pd(&report, 42);
        assert_eq!(events.len(), 1);
        match &events[0] {
            PdEvent::Message {
                timestamp_ms,
                sop,
                wire,
                raw,
            } => {
                assert_eq!(*timestamp_ms, 42);
                // SOP (224) normalizes to the primary-SOP convention (0).
                assert_eq!(*sop, 0);
                // CRC stripped; only the PD header remains.
                assert_eq!(wire, &[0x06, 0x00]);
                assert_eq!(raw.len(), 9);
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn pd_report_keeps_data_object_when_frame_has_no_crc() {
        // This is the shape emitted by the reference witrnhid parser:
        // FE 07 E0 82 18 59 FE 03 53. The final four bytes are the one
        // data object, not a CRC, so the complete six-byte PD wire is kept.
        let mut report = vec![0u8; REPORT_LEN];
        report[0] = REPORT_PD;
        report[1] = 7; // SOP + 2-byte header + one 4-byte data object.
        report[2] = 224;
        report[3..9].copy_from_slice(&[0x82, 0x18, 0x59, 0xFE, 0x03, 0x53]);
        let events = parse_pd(&report, 0);
        match &events[0] {
            PdEvent::Message { wire, .. } => {
                assert_eq!(wire, &[0x82, 0x18, 0x59, 0xFE, 0x03, 0x53]);
                let decoded = crate::protocol::decode_pd_wire(wire).expect("PD object decodes");
                assert_eq!(decoded.num_objects, 1);
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn pd_report_keeps_control_message_when_frame_has_no_crc() {
        let mut report = vec![0u8; REPORT_LEN];
        report[0] = REPORT_PD;
        report[1] = 3; // SOP + 2-byte Accept header.
        report[2] = 224;
        report[3..5].copy_from_slice(&[0x03, 0x00]);
        let events = parse_pd(&report, 0);
        match &events[0] {
            PdEvent::Message { wire, .. } => assert_eq!(wire, &[0x03, 0x00]),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn sop_prime_stays_outside_primary_negotiation() {
        let report = pd_report(192, [0x06, 0x00], [0xAA, 0xBB, 0xCC, 0xDD]);
        let events = parse_pd(&report, 0);
        match &events[0] {
            PdEvent::Message { sop, .. } => assert_eq!(*sop, 1),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn short_pd_report_is_ignored() {
        // Marker + len claiming an impossible 2-byte message.
        let report = [REPORT_PD, 2, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(parse_pd(&report, 0).is_empty());
    }
}
