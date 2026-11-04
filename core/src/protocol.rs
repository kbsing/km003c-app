//! KM003C wire format. Pure parsing, no I/O.
//!
//! Everything here is validated against real hardware captures. The three
//! places that are easy to get wrong, and were:
//!
//! 1. The control header's `attribute` starts at bit 17, so it appears shifted
//!    left by one in raw bytes. Build a u32 and let `to_le_bytes` place it.
//! 2. Logical-packet boundaries come from each extended header's own
//!    `size`/`next`. The main header's `obj_count_words` is opaque.
//! 3. A PD message header's bit 15 is the *extended* flag and must be checked
//!    BEFORE reading bits 12-14 as an object count. Extended messages carry a
//!    2-byte extended header plus raw bytes, not 4-byte data objects.

pub const VID: u16 = 0x5FC9;
pub const PID: u16 = 0x0063;

pub const IFACE: u8 = 0; // vendor bulk, WinUSB-bound on Windows
pub const EP_OUT: u8 = 0x01;
pub const EP_IN: u8 = 0x81;

pub const CMD_CONNECT: u8 = 0x02;
pub const CMD_DISCONNECT: u8 = 0x03;
pub const CMD_ACCEPT: u8 = 0x05;
pub const CMD_GET_DATA: u8 = 0x0C;
pub const CMD_PUT_DATA: u8 = 0x41;

pub const ATT_ADC: u16 = 0x0001;
pub const ATT_ADC_QUEUE: u16 = 0x0002;
pub const ATT_PD_PACKET: u16 = 0x0010;

// ---------------------------------------------------------------- header

/// Build a 4-byte control header.
///
/// Wire layout as a 32-bit little-endian word:
///   `type:7 | reserved:1 | id:8 | unused:1 | attribute:15`
pub fn control_header(packet_type: u8, id: u8, attribute: u16) -> [u8; 4] {
    let word =
        (packet_type as u32 & 0x7F) | ((id as u32) << 8) | ((attribute as u32 & 0x7FFF) << 17);
    word.to_le_bytes()
}

// ---------------------------------------------------------------- ADC

/// 44-byte ADC payload (attribute 0x0001). High resolution, but the device only
/// refreshes it about every 500 ms -- measured, see README notes.
#[derive(Debug, Clone, Copy)]
pub struct Adc {
    pub vbus_uv: i32,
    pub ibus_ua: i32,
    pub vbus_v: f64,
    pub ibus_a: f64,
    pub temp_c: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
}

pub fn parse_adc(p: &[u8]) -> Option<Adc> {
    if p.len() < 44 {
        return None;
    }
    let i32_at = |o: usize| i32::from_le_bytes(p[o..o + 4].try_into().unwrap());
    let u16_at = |o: usize| u16::from_le_bytes(p[o..o + 2].try_into().unwrap());
    let i16_at = |o: usize| i16::from_le_bytes(p[o..o + 2].try_into().unwrap());

    let vbus_uv = i32_at(0);
    let ibus_ua = i32_at(4);
    Some(Adc {
        vbus_uv,
        ibus_ua,
        vbus_v: vbus_uv as f64 / 1_000_000.0,
        ibus_a: ibus_ua as f64 / 1_000_000.0,
        temp_c: i16_at(24) as f64 / 128.0,
        cc1_v: u16_at(26) as f64 / 10_000.0, // 0.1 mV units
        cc2_v: u16_at(28) as f64 / 10_000.0,
    })
}

// ---------------------------------------------------------------- PD

/// 12-byte PD measurement block. Same layout for standalone status, the PD part
/// of a chained ADC+PD response, and the preamble of an event stream.
///
/// Measured to refresh on essentially every poll (~21 ms median), which makes it
/// the fast unauthenticated path for volts/amps at 1 mV / 1 mA resolution.
#[derive(Debug, Clone, Copy)]
pub struct PdMeasurement {
    pub timestamp_ms: u32,
    pub vbus_mv: u16,
    pub ibus_ma: i16,
    pub cc1_mv: u16,
    pub cc2_mv: u16,
}

pub fn parse_pd_measurement(p: &[u8]) -> Option<PdMeasurement> {
    if p.len() < 12 {
        return None;
    }
    Some(PdMeasurement {
        timestamp_ms: u32::from_le_bytes(p[0..4].try_into().unwrap()),
        vbus_mv: u16::from_le_bytes(p[4..6].try_into().unwrap()),
        ibus_ma: i16::from_le_bytes(p[6..8].try_into().unwrap()),
        cc1_mv: u16::from_le_bytes(p[8..10].try_into().unwrap()),
        cc2_mv: u16::from_le_bytes(p[10..12].try_into().unwrap()),
    })
}

/// One record inside a PD event stream.
#[derive(Debug, Clone)]
pub enum PdEvent {
    /// 6-byte device-event record. Marker 0x45 is Type-C attach/detach;
    /// marker 0x05 is another V1.9.9 device-event class with an undocumented
    /// code byte.
    ///
    /// Observed codes on this firmware are 0x11 (attach) and 0x12 (detach).
    /// The repo docs list 0x21/0x22; the low nibble agrees, the high nibble
    /// does not. Codes are preserved verbatim rather than normalized.
    Connection {
        timestamp_ms: u32,
        code: u8,
        raw: Vec<u8>,
    },
    /// A non-PD device event emitted by the instrument. Its marker and code
    /// are preserved because their meaning has not yet been documented.
    Device {
        timestamp_ms: u32,
        marker: u8,
        code: u8,
        raw: Vec<u8>,
    },
    /// Wrapped PD message: 6-byte vendor header + PD wire bytes.
    Message {
        timestamp_ms: u32,
        sop: u8,
        wire: Vec<u8>,
        raw: Vec<u8>,
    },
    /// Could not classify the leading byte; stream parsing stops here.
    Unknown { offset: usize, rest: Vec<u8> },
}

/// Interpret a connection event code. Returns None for codes we have not
/// confirmed, so callers surface the raw value instead of guessing.
pub fn connection_label(code: u8) -> Option<&'static str> {
    match code {
        0x11 => Some("ATTACH"),
        0x12 => Some("DETACH"),
        _ => None,
    }
}

/// Split a PD event stream payload into preamble + events.
///
/// Layout: `[preamble 12B][event][event]...`
///
/// Event records are either 6-byte device events (`0x45` and observed `0x05`)
/// or PD messages with a 6-byte vendor header followed by PD wire bytes.
/// The PD record's low six bits carry the record size (including the five
/// bytes after the size byte).
pub fn parse_pd_event_stream(p: &[u8]) -> (Option<PdMeasurement>, Vec<PdEvent>) {
    let preamble = parse_pd_measurement(p);
    let mut events = Vec::new();
    if p.len() <= 12 {
        return (preamble, events);
    }

    let mut i = 12;
    while i < p.len() {
        let lead = p[i];

        if lead == 0x45 || lead == 0x05 {
            if i + 6 > p.len() {
                events.push(PdEvent::Unknown {
                    offset: i,
                    rest: p[i..].to_vec(),
                });
                break;
            }
            let raw = p[i..i + 6].to_vec();
            let ts = u32::from(p[i + 1]) | (u32::from(p[i + 2]) << 8) | (u32::from(p[i + 3]) << 16);
            if lead == 0x45 {
                events.push(PdEvent::Connection {
                    timestamp_ms: ts,
                    code: p[i + 5],
                    raw,
                });
            } else {
                events.push(PdEvent::Device {
                    timestamp_ms: ts,
                    marker: lead,
                    code: p[i + 5],
                    raw,
                });
            }
            i += 6;
        } else if (lead & 0x3F) >= 5 {
            // The low six bits carry the record size including the five bytes
            // after the size byte, so size_flag is always >= 5 here.
            let size_flag = (lead & 0x3F) as usize;
            let wire_len = size_flag - 5;
            let total = 6 + wire_len;
            if i + total > p.len() {
                events.push(PdEvent::Unknown {
                    offset: i,
                    rest: p[i..].to_vec(),
                });
                break;
            }
            let payload = &p[i + 6..i + total];
            // Some V1.9.9 replies use a 0x7B container whose payload is a
            // sequence of normal wrapped records (0x97, 0x87, ...). Decode
            // those inner records instead of treating the container as PD
            // wire bytes.
            if !(0x80..=0x9F).contains(&lead)
                && payload
                    .first()
                    .is_some_and(|b| *b == 0x45 || *b == 0x05 || (0x80..=0x9F).contains(b))
            {
                let (_, mut nested) = parse_pd_event_stream(&[&[0u8; 12], payload].concat());
                // Container payloads are padded to the transport size.
                nested.retain(|event| {
                    !matches!(event, PdEvent::Unknown { rest, .. } if rest.iter().all(|byte| *byte == 0))
                });
                events.extend(nested);
                i += total;
                continue;
            }
            let raw = p[i..i + total].to_vec();
            let ts = u32::from_le_bytes(p[i + 1..i + 5].try_into().unwrap());
            events.push(PdEvent::Message {
                timestamp_ms: ts,
                sop: p[i + 5],
                wire: p[i + 6..i + 6 + wire_len].to_vec(),
                raw,
            });
            i += total;
        } else {
            events.push(PdEvent::Unknown {
                offset: i,
                rest: p[i..].to_vec(),
            });
            break;
        }
    }

    (preamble, events)
}

// ------------------------------------------------- standard USB PD decoding

pub fn pd_control_name(t: u8) -> &'static str {
    match t {
        0x01 => "GoodCRC",
        0x02 => "GotoMin",
        0x03 => "Accept",
        0x04 => "Reject",
        0x05 => "Ping",
        0x06 => "PS_RDY",
        0x07 => "Get_Source_Cap",
        0x08 => "Get_Sink_Cap",
        0x09 => "DR_Swap",
        0x0A => "PR_Swap",
        0x0B => "VCONN_Swap",
        0x0C => "Wait",
        0x0D => "Soft_Reset",
        0x0E => "Data_Reset",
        0x0F => "Data_Reset_Complete",
        0x10 => "Not_Supported",
        0x11 => "Get_Source_Cap_Extended",
        0x12 => "Get_Status",
        0x13 => "FR_Swap",
        0x14 => "Get_PPS_Status",
        0x15 => "Get_Country_Codes",
        0x16 => "Get_Sink_Cap_Extended",
        0x17 => "Get_Source_Info",
        0x18 => "Get_Revision",
        _ => "Control?",
    }
}

pub fn pd_data_name(t: u8) -> &'static str {
    match t {
        0x01 => "Source_Capabilities",
        0x02 => "Request",
        0x03 => "BIST",
        0x04 => "Sink_Capabilities",
        0x05 => "Battery_Status",
        0x06 => "Alert",
        0x07 => "Get_Country_Info",
        0x08 => "Enter_USB",
        0x09 => "EPR_Request",
        0x0A => "EPR_Mode",
        0x0B => "Source_Info",
        0x0C => "Revision",
        0x0F => "Vendor_Defined",
        _ => "Data?",
    }
}

pub fn pd_ext_name(t: u8) -> &'static str {
    match t {
        0x01 => "Source_Capabilities_Extended",
        0x02 => "Status",
        0x03 => "Get_Battery_Cap",
        0x04 => "Get_Battery_Status",
        0x05 => "Battery_Capabilities",
        0x06 => "Get_Manufacturer_Info",
        0x07 => "Manufacturer_Info",
        0x08 => "Security_Request",
        0x09 => "Security_Response",
        0x0A => "Firmware_Update_Request",
        0x0B => "Firmware_Update_Response",
        0x0C => "PPS_Status",
        0x0D => "Country_Info",
        0x0E => "Country_Codes",
        0x0F => "Sink_Capabilities_Extended",
        0x10 => "Extended_Control",
        0x11 => "EPR_Source_Capabilities",
        0x12 => "EPR_Sink_Capabilities",
        0x14 => "Vendor_Defined_Extended",
        _ => "Ext?",
    }
}

/// Describe the standardized header of a Vendor Defined Message (VDM).
///
/// The payload after this header belongs to the SVID owner. We intentionally
/// keep that payload opaque until a capture or public specification validates
/// its meaning.
pub fn describe_vdm(objects: &[u32]) -> Vec<String> {
    let Some(&header) = objects.first() else {
        return vec!["VDM has no header object".to_string()];
    };

    let svid = (header >> 16) as u16;
    if (header >> 15) & 1 == 0 {
        return vec![
            format!("Unstructured VDM: SVID 0x{svid:04X}"),
            format!(
                "Vendor payload: {} VDO(s), kept opaque",
                objects.len().saturating_sub(1)
            ),
        ];
    }

    let version = (header >> 13) & 0x3;
    let object_position = (header >> 8) & 0x7;
    let command_type = match (header >> 6) & 0x3 {
        0 => "Initiator",
        1 => "ACK",
        2 => "NAK",
        _ => "BUSY",
    };
    let command = match header & 0x1F {
        1 => "Discover Identity",
        2 => "Discover SVIDs",
        3 => "Discover Modes",
        4 => "Enter Mode",
        5 => "Exit Mode",
        6 => "Attention",
        value => {
            return vec![
                format!("Structured VDM: SVID 0x{svid:04X}"),
                format!(
                    "Command 0x{value:02X} ({command_type}), version field {version}, mode position {object_position}"
                ),
                format!(
                    "Vendor payload: {} VDO(s), kept opaque",
                    objects.len().saturating_sub(1)
                ),
            ];
        }
    };

    vec![
        format!("Structured VDM: SVID 0x{svid:04X}"),
        format!(
            "{command} ({command_type}), version field {version}, mode position {object_position}"
        ),
        format!(
            "Vendor payload: {} VDO(s), kept opaque",
            objects.len().saturating_sub(1)
        ),
    ]
}

/// Describe a PDO from Source_Capabilities.
pub fn describe_pdo(index: usize, pdo: u32) -> String {
    let kind = pdo >> 30;
    match kind {
        0b00 => {
            let mv = ((pdo >> 10) & 0x3FF) * 50;
            let ma = (pdo & 0x3FF) * 10;
            format!(
                "#{}: Fixed {:.2}V {:.2}A",
                index + 1,
                mv as f64 / 1000.0,
                ma as f64 / 1000.0
            )
        }
        0b01 => {
            let max_mv = ((pdo >> 20) & 0x3FF) * 50;
            let min_mv = ((pdo >> 10) & 0x3FF) * 50;
            let mw = (pdo & 0x3FF) * 250;
            format!(
                "#{}: Battery {:.2}-{:.2}V {:.2}W",
                index + 1,
                min_mv as f64 / 1000.0,
                max_mv as f64 / 1000.0,
                mw as f64 / 1000.0
            )
        }
        0b10 => {
            let max_mv = ((pdo >> 20) & 0x3FF) * 50;
            let min_mv = ((pdo >> 10) & 0x3FF) * 50;
            let ma = (pdo & 0x3FF) * 10;
            format!(
                "#{}: Variable {:.2}-{:.2}V {:.2}A",
                index + 1,
                min_mv as f64 / 1000.0,
                max_mv as f64 / 1000.0,
                ma as f64 / 1000.0
            )
        }
        _ => {
            // Augmented (APDO). Subtype in bits 28-29; 00 = SPR PPS.
            let subtype = (pdo >> 28) & 0x3;
            if subtype == 0 {
                let max_mv = ((pdo >> 17) & 0xFF) * 100;
                let min_mv = ((pdo >> 8) & 0xFF) * 100;
                let ma = (pdo & 0x7F) * 50;
                format!(
                    "#{}: PPS {:.2}-{:.2}V {:.2}A",
                    index + 1,
                    min_mv as f64 / 1000.0,
                    max_mv as f64 / 1000.0,
                    ma as f64 / 1000.0
                )
            } else {
                format!("#{}: APDO subtype={} raw=0x{:08X}", index + 1, subtype, pdo)
            }
        }
    }
}

/// Describe an RDO from a Request.
///
/// A PPS RDO has a completely different field layout from a Fixed/Variable RDO,
/// so the selected PDO must be known first. When it is not (Request seen before
/// Source_Capabilities, e.g. replayed from the device buffer), say so rather
/// than silently applying the Fixed layout and printing a plausible-but-wrong
/// number.
pub fn describe_rdo(rdo: u32, source_caps: &[u32]) -> String {
    let pos = ((rdo >> 28) & 0x7) as usize;

    let Some(&selected) = source_caps.get(pos.wrapping_sub(1)) else {
        return format!("PDO#{pos} <layout unknown: no Source_Capabilities yet> raw=0x{rdo:08X}");
    };

    let is_pps = (selected >> 30) == 0b11 && ((selected >> 28) & 0x3) == 0;
    if is_pps {
        // PPS RDO: bits 19-9 = output voltage in 20 mV, bits 6-0 = current in 50 mA.
        let mv = ((rdo >> 9) & 0x7FF) * 20;
        let ma = (rdo & 0x7F) * 50;
        format!(
            "PDO#{} PPS req {:.2}V {:.2}A",
            pos,
            mv as f64 / 1000.0,
            ma as f64 / 1000.0
        )
    } else {
        // Fixed/Variable RDO: bits 19-10 = operating current, bits 9-0 = max, in 10 mA.
        let op_ma = ((rdo >> 10) & 0x3FF) * 10;
        let max_ma = (rdo & 0x3FF) * 10;
        format!(
            "PDO#{} op={:.2}A max={:.2}A",
            pos,
            op_ma as f64 / 1000.0,
            max_ma as f64 / 1000.0
        )
    }
}

/// A request decoded only where the selected PDO defines an unambiguous layout.
/// Variable and battery PDO requests do not carry one requested voltage, so their
/// voltage/current fields remain `None` instead of being guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdRequestInfo {
    pub pdo_position: usize,
    pub kind: &'static str,
    pub voltage_mv: Option<u32>,
    pub current_ma: Option<u32>,
}

pub fn decode_pd_request(rdo: u32, source_caps: &[u32]) -> PdRequestInfo {
    let pdo_position = ((rdo >> 28) & 0x7) as usize;
    let Some(&selected) = source_caps.get(pdo_position.wrapping_sub(1)) else {
        return PdRequestInfo {
            pdo_position,
            kind: "unknown",
            voltage_mv: None,
            current_ma: None,
        };
    };

    if (selected >> 30) == 0b00 {
        return PdRequestInfo {
            pdo_position,
            kind: "fixed",
            voltage_mv: Some(((selected >> 10) & 0x3FF) * 50),
            current_ma: Some(((rdo >> 10) & 0x3FF) * 10),
        };
    }

    if (selected >> 30) == 0b11 && ((selected >> 28) & 0x3) == 0 {
        return PdRequestInfo {
            pdo_position,
            kind: "pps",
            voltage_mv: Some(((rdo >> 9) & 0x7FF) * 20),
            current_ma: Some((rdo & 0x7F) * 50),
        };
    }

    PdRequestInfo {
        pdo_position,
        kind: "other",
        voltage_mv: None,
        current_ma: None,
    }
}

/// Payload of an extended message, when we can interpret it.
#[derive(Debug, Clone)]
pub enum ExtBody {
    /// PPS_Status Data Block: output V/I plus real-time flags.
    PpsStatus {
        mv: u32,
        ma: u32,
        flags: u8,
    },
    /// One transport chunk of a chunked extended message. The full payload is
    /// assembled by `PdNegotiation`, which has the cross-message context.
    Chunk {
        message_type: u8,
        data_size: usize,
        chunk_number: u8,
        request_chunk: bool,
        data: Vec<u8>,
    },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct PdMessage {
    pub name: String,
    pub msg_id: u8,
    pub num_objects: usize,
    pub power_role: &'static str,
    pub data_role: &'static str,
    pub spec_rev: &'static str,
    pub objects: Vec<u32>,
    pub is_source_caps: bool,
    pub is_request: bool,
    /// `Some` when header bit 15 is set (extended message).
    pub ext: Option<ExtBody>,
}

/// The observed state of the primary (SOP) power negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationUpdate {
    SourceCapabilities,
    RequestPending,
    RequestWithoutCapabilities,
    RequestAccepted,
    RequestRejected,
    RequestDeferred,
    UnexpectedAccept,
    Complete,
    UnexpectedPsRdy,
    ControlPending(&'static str),
    ControlAccepted(&'static str),
    ControlComplete(&'static str),
    SoftResetPending,
    SoftResetAccepted,
}

impl NegotiationUpdate {
    pub fn label(self) -> String {
        match self {
            Self::SourceCapabilities => "negotiation: Source_Capabilities received".into(),
            Self::RequestPending => "negotiation: Request pending Accept".into(),
            Self::RequestWithoutCapabilities => {
                "negotiation: Request seen before Source_Capabilities".into()
            }
            Self::RequestAccepted => "negotiation: Request accepted".into(),
            Self::RequestRejected => "negotiation: Request rejected".into(),
            Self::RequestDeferred => "negotiation: Request deferred (Wait)".into(),
            Self::UnexpectedAccept => "negotiation: Accept without a tracked Request".into(),
            Self::Complete => "negotiation: complete (PS_RDY)".into(),
            Self::UnexpectedPsRdy => "negotiation: PS_RDY without a tracked Accept".into(),
            Self::ControlPending(name) => format!("negotiation: {name} pending Accept"),
            Self::ControlAccepted(name) => format!("negotiation: {name} accepted"),
            Self::ControlComplete(name) => format!("negotiation: {name} complete (PS_RDY)"),
            Self::SoftResetPending => "negotiation: Soft Reset pending Accept".into(),
            Self::SoftResetAccepted => {
                "negotiation: Soft Reset accepted; waiting for capabilities".into()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NegotiationPhase {
    Idle,
    Offered,
    Requested,
    Accepted,
    ControlPending(&'static str),
    ControlAccepted(&'static str),
    SoftResetPending,
}

/// PDO context and state for one primary-SOP negotiation.
///
/// SOP' traffic is intentionally excluded: cable messages are useful to show,
/// but must not replace the partner's Source_Capabilities used for an RDO.
#[derive(Debug, Default)]
pub struct PdNegotiation {
    source_caps: Vec<u32>,
    selected_pdo: Option<usize>,
    pending_rdo: Option<u32>,
    active_rdo: Option<u32>,
    phase: NegotiationPhase,
    extended: Vec<ExtendedAssembly>,
}

#[derive(Debug)]
struct ExtendedAssembly {
    sop: u8,
    message_type: u8,
    data_size: usize,
    next_chunk: u8,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum ExtendedUpdate {
    Pending { chunk_number: u8, data_size: usize },
    Complete(ExtBody),
    RequestChunk { chunk_number: u8 },
    OutOfSequence { chunk_number: u8 },
}

impl Default for NegotiationPhase {
    fn default() -> Self {
        Self::Idle
    }
}

impl PdNegotiation {
    pub fn reset(&mut self) {
        self.source_caps.clear();
        self.selected_pdo = None;
        self.pending_rdo = None;
        self.active_rdo = None;
        self.phase = NegotiationPhase::Idle;
        self.extended.clear();
    }

    pub fn source_caps(&self) -> &[u32] {
        &self.source_caps
    }

    pub fn selected_pdo(&self) -> Option<usize> {
        self.selected_pdo
    }

    pub fn pending_rdo(&self) -> Option<u32> {
        self.pending_rdo
    }

    pub fn active_rdo(&self) -> Option<u32> {
        self.active_rdo
    }

    pub fn phase_label(&self) -> &'static str {
        match self.phase {
            NegotiationPhase::Idle => "idle",
            NegotiationPhase::Offered => "offered",
            NegotiationPhase::Requested => "request-pending",
            NegotiationPhase::Accepted => "accepted-waiting-ps-rdy",
            NegotiationPhase::ControlPending(_) => "control-pending",
            NegotiationPhase::ControlAccepted(_) => "control-accepted",
            NegotiationPhase::SoftResetPending => "soft-reset",
        }
    }

    /// Feed one chunked extended message into the reassembler. Unchunked
    /// extended messages are already complete and return `None` here.
    pub fn observe_extended(&mut self, sop: u8, msg: &PdMessage) -> Option<ExtendedUpdate> {
        let ExtBody::Chunk {
            message_type,
            data_size,
            chunk_number,
            request_chunk,
            data,
        } = msg.ext.as_ref()?
        else {
            return None;
        };

        if *request_chunk {
            return Some(ExtendedUpdate::RequestChunk {
                chunk_number: *chunk_number,
            });
        }

        if *chunk_number == 0 {
            self.extended
                .retain(|assembly| assembly.sop != sop || assembly.message_type != *message_type);
            self.extended.push(ExtendedAssembly {
                sop,
                message_type: *message_type,
                data_size: *data_size,
                next_chunk: 1,
                data: data.clone(),
            });
        } else {
            let Some(assembly) = self.extended.iter_mut().find(|assembly| {
                assembly.sop == sop
                    && assembly.message_type == *message_type
                    && assembly.data_size == *data_size
            }) else {
                return Some(ExtendedUpdate::OutOfSequence {
                    chunk_number: *chunk_number,
                });
            };
            if assembly.next_chunk != *chunk_number {
                return Some(ExtendedUpdate::OutOfSequence {
                    chunk_number: *chunk_number,
                });
            }
            assembly.data.extend_from_slice(data);
            assembly.next_chunk += 1;
        }

        let position = self.extended.iter().position(|assembly| {
            assembly.sop == sop
                && assembly.message_type == *message_type
                && assembly.data_size == *data_size
                && assembly.data.len() >= assembly.data_size
        });
        if let Some(position) = position {
            let assembly = self.extended.remove(position);
            return Some(ExtendedUpdate::Complete(decode_ext_body(
                assembly.message_type,
                &assembly.data[..assembly.data_size],
            )));
        }

        Some(ExtendedUpdate::Pending {
            chunk_number: *chunk_number,
            data_size: *data_size,
        })
    }

    pub fn observe(&mut self, sop: u8, msg: &PdMessage) -> Option<NegotiationUpdate> {
        if sop != 0 {
            return None;
        }

        if msg.is_source_caps {
            self.source_caps.clear();
            self.source_caps.extend_from_slice(&msg.objects);
            self.selected_pdo = None;
            self.pending_rdo = None;
            self.active_rdo = None;
            self.phase = NegotiationPhase::Offered;
            return Some(NegotiationUpdate::SourceCapabilities);
        }

        if msg.is_request {
            self.pending_rdo = msg.objects.first().copied();
            self.selected_pdo = self
                .pending_rdo
                .map(|rdo| ((rdo >> 28) & 0x7) as usize)
                .filter(|position| *position > 0);
            self.phase = NegotiationPhase::Requested;
            return Some(if self.source_caps.is_empty() {
                NegotiationUpdate::RequestWithoutCapabilities
            } else {
                NegotiationUpdate::RequestPending
            });
        }

        if msg.name == "Soft_Reset" {
            self.source_caps.clear();
            self.selected_pdo = None;
            self.pending_rdo = None;
            self.active_rdo = None;
            self.extended.clear();
            self.phase = NegotiationPhase::SoftResetPending;
            return Some(NegotiationUpdate::SoftResetPending);
        }

        if let Some(name) = control_swap_name(&msg.name) {
            self.phase = NegotiationPhase::ControlPending(name);
            return Some(NegotiationUpdate::ControlPending(name));
        }

        if msg.name == "Accept" {
            return Some(match self.phase {
                NegotiationPhase::Requested => {
                    self.phase = NegotiationPhase::Accepted;
                    NegotiationUpdate::RequestAccepted
                }
                NegotiationPhase::ControlPending(name) => {
                    self.phase = NegotiationPhase::ControlAccepted(name);
                    NegotiationUpdate::ControlAccepted(name)
                }
                NegotiationPhase::SoftResetPending => {
                    self.phase = NegotiationPhase::Idle;
                    NegotiationUpdate::SoftResetAccepted
                }
                _ => NegotiationUpdate::UnexpectedAccept,
            });
        }

        if msg.name == "Reject" || msg.name == "Wait" {
            let deferred = msg.name == "Wait";
            let update = match self.phase {
                NegotiationPhase::Requested => Some(if deferred {
                    NegotiationUpdate::RequestDeferred
                } else {
                    NegotiationUpdate::RequestRejected
                }),
                _ => None,
            };
            self.phase = NegotiationPhase::Idle;
            self.pending_rdo = None;
            return update;
        }

        if msg.name == "PS_RDY" {
            return Some(match self.phase {
                NegotiationPhase::Accepted => {
                    self.phase = NegotiationPhase::Idle;
                    self.active_rdo = self.pending_rdo.take();
                    NegotiationUpdate::Complete
                }
                NegotiationPhase::ControlAccepted(name) => {
                    self.phase = NegotiationPhase::Idle;
                    NegotiationUpdate::ControlComplete(name)
                }
                _ => NegotiationUpdate::UnexpectedPsRdy,
            });
        }

        None
    }
}

fn control_swap_name(name: &str) -> Option<&'static str> {
    match name {
        "DR_Swap" => Some("DR_Swap"),
        "PR_Swap" => Some("PR_Swap"),
        "VCONN_Swap" => Some("VCONN_Swap"),
        "FR_Swap" => Some("FR_Swap"),
        _ => None,
    }
}

/// Decode a USB PD message from its wire bytes.
///
/// Header is 2 bytes LE. **Bit 15 is the extended flag and is checked first.**
/// An extended message is followed by a 2-byte extended header
/// (`chunked:1 | chunk_number:4 | request_chunk:1 | reserved:1 | data_size:9`)
/// and then raw bytes -- not by 4-byte data objects. Reading bits 12-14 as an
/// object count on an extended message yields a bogus type and object list;
/// that bug made every PPS_Status look like a 2-object "Revision".
pub fn decode_pd_wire(wire: &[u8]) -> Result<PdMessage, String> {
    if wire.len() < 2 {
        return Err(format!("header needs 2 bytes, got {}", wire.len()));
    }
    let hdr = u16::from_le_bytes([wire[0], wire[1]]);
    let msg_type = (hdr & 0x1F) as u8;
    let data_role = if (hdr >> 5) & 1 == 1 { "DFP" } else { "UFP" };
    let spec_rev = match (hdr >> 6) & 0x3 {
        0 => "1.0",
        1 => "2.0",
        2 => "3.0",
        _ => "3.1?",
    };
    let power_role = if (hdr >> 8) & 1 == 1 { "Src" } else { "Snk" };
    let msg_id = ((hdr >> 9) & 0x7) as u8;
    let num_objects = ((hdr >> 12) & 0x7) as usize;
    let is_extended = (hdr >> 15) & 1 == 1;

    if is_extended {
        if wire.len() < 4 {
            return Err(format!("extended header needs 4 bytes, got {}", wire.len()));
        }
        let expected_wire_len = 2 + num_objects * 4;
        if wire.len() != expected_wire_len {
            return Err(format!(
                "extended header declares {num_objects} data objects, expected {expected_wire_len} wire bytes, got {}",
                wire.len()
            ));
        }
        let ext_hdr = u16::from_le_bytes([wire[2], wire[3]]);
        let data_size = (ext_hdr & 0x1FF) as usize;
        let chunked = (ext_hdr >> 15) & 1 == 1;
        let chunk_number = ((ext_hdr >> 11) & 0xF) as u8;
        let request_chunk = (ext_hdr >> 10) & 1 == 1;

        // Some KM003C captures set the high reserved bit on short, complete
        // messages (notably PPS_Status). Treat those as complete unless the
        // fields actually require chunking.
        let chunked = chunked && (data_size > 26 || chunk_number > 0 || request_chunk);
        if chunked {
            let chunk_start = chunk_number as usize * 26;
            if !request_chunk && chunk_start >= data_size {
                return Err(format!(
                    "extended chunk {chunk_number} starts beyond declared {data_size}-byte payload"
                ));
            }
            let chunk_len = if request_chunk {
                0
            } else {
                (data_size - chunk_start).min(26)
            };
            let payload = &wire[4..];
            if payload.len() < chunk_len {
                return Err(format!(
                    "extended chunk {chunk_number} needs {chunk_len} data bytes, got {}",
                    payload.len()
                ));
            }
            return Ok(PdMessage {
                name: pd_ext_name(msg_type).to_string(),
                msg_id,
                num_objects: 0,
                power_role,
                data_role,
                spec_rev,
                objects: Vec::new(),
                is_source_caps: false,
                is_request: false,
                ext: Some(ExtBody::Chunk {
                    message_type: msg_type,
                    data_size,
                    chunk_number,
                    request_chunk,
                    data: payload[..chunk_len].to_vec(),
                }),
            });
        }

        // The extended header plus data is padded to a 4-byte data-object boundary.
        let expected_len = 2 + (2 + data_size).next_multiple_of(4);
        if wire.len() != expected_len {
            return Err(format!(
                "extended message declares {data_size} data bytes, expected {expected_len} wire bytes, got {}",
                wire.len()
            ));
        }
        let data = &wire[4..4 + data_size];
        let body = decode_ext_body(msg_type, data);

        return Ok(PdMessage {
            name: pd_ext_name(msg_type).to_string(),
            msg_id,
            num_objects: 0,
            power_role,
            data_role,
            spec_rev,
            objects: Vec::new(),
            is_source_caps: false,
            is_request: false,
            ext: Some(body),
        });
    }

    let is_data = num_objects > 0;
    let name = if is_data {
        pd_data_name(msg_type).to_string()
    } else {
        pd_control_name(msg_type).to_string()
    };

    let expected_len = 2 + num_objects * 4;
    if wire.len() != expected_len {
        return Err(format!(
            "header declares {num_objects} data objects, expected {expected_len} wire bytes, got {}",
            wire.len()
        ));
    }

    let mut objects = Vec::with_capacity(num_objects);
    for k in 0..num_objects {
        let off = 2 + k * 4;
        objects.push(u32::from_le_bytes(wire[off..off + 4].try_into().unwrap()));
    }

    Ok(PdMessage {
        name,
        msg_id,
        num_objects,
        power_role,
        data_role,
        spec_rev,
        objects,
        is_source_caps: is_data && msg_type == 0x01,
        is_request: is_data && msg_type == 0x02,
        ext: None,
    })
}

fn decode_ext_body(msg_type: u8, data: &[u8]) -> ExtBody {
    match (msg_type, data.len()) {
        // PPS Status Data Block: u16 mV/20, u8 mA/50, u8 flags
        (0x0C, n) if n >= 4 => ExtBody::PpsStatus {
            mv: u16::from_le_bytes([data[0], data[1]]) as u32 * 20,
            ma: data[2] as u32 * 50,
            flags: data[3],
        },
        _ => ExtBody::Raw(data.to_vec()),
    }
}

// ---------------------------------------------------------------- framing

pub struct LogicalPacket {
    pub attribute: u16,
    pub payload: Vec<u8>,
}

/// Walk the chained logical packets of a PutData response.
///
/// Extended header per logical packet (32-bit LE):
///   `attribute:15 | next:1 | chunk:6 | size:10`
///
/// Boundaries come from each header's own `size`/`next`. The main header's
/// `obj_count_words` is opaque -- do not use it to delimit.
///
/// Returns `Ok(empty vec)` for a well-formed response that carries no logical
/// packets. Callers must distinguish that from an error: an empty PutData is a
/// normal "nothing buffered" reply, not a fault.
pub fn parse_put_data(resp: &[u8]) -> Result<Vec<LogicalPacket>, String> {
    if resp.len() < 4 {
        return Err(format!("response too short: {} bytes", resp.len()));
    }
    let ptype = resp[0] & 0x7F;
    if ptype != CMD_PUT_DATA {
        return Err(format!("expected PutData (0x41), got 0x{ptype:02X}"));
    }

    let mut out = Vec::new();
    let mut off = 4;

    loop {
        if off + 4 > resp.len() {
            break;
        }
        let ext = u32::from_le_bytes(resp[off..off + 4].try_into().unwrap());
        off += 4;

        let attribute = (ext & 0x7FFF) as u16;
        let next = (ext >> 15) & 0x1;
        let chunk = ((ext >> 16) & 0x3F) as usize;
        let size = ((ext >> 22) & 0x3FF) as usize;

        // AdcQueue reports size as per-sample stride with chunk = sample count.
        let take = if attribute == ATT_ADC_QUEUE {
            chunk * size
        } else {
            size
        };

        if off + take > resp.len() {
            return Err(format!(
                "logical packet attr=0x{attribute:04X} wants {take} bytes, only {} left",
                resp.len() - off
            ));
        }

        out.push(LogicalPacket {
            attribute,
            payload: resp[off..off + take].to_vec(),
        });
        off += take;

        if next == 0 {
            break;
        }
    }

    Ok(out)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_lands_at_bit_17() {
        // ADC (att=1) must appear as 0x02 in wire byte 2, not 0x01.
        assert_eq!(
            control_header(CMD_GET_DATA, 0x05, ATT_ADC),
            [0x0C, 0x05, 0x02, 0x00]
        );
        // PdPacket (att=0x10) -> 0x20.
        assert_eq!(
            control_header(CMD_GET_DATA, 0x05, ATT_PD_PACKET),
            [0x0C, 0x05, 0x20, 0x00]
        );
        // ADC | PdPacket = 0x11 -> 0x22.
        assert_eq!(
            control_header(CMD_GET_DATA, 0x01, ATT_ADC | ATT_PD_PACKET),
            [0x0C, 0x01, 0x22, 0x00]
        );
    }

    #[test]
    fn pps_status_is_not_a_revision_message() {
        // Captured from hardware. Previously mis-decoded as "Revision objs=2".
        let wire = [0xac, 0xa5, 0x04, 0x80, 0xe1, 0x01, 0x40, 0x02, 0x00, 0x00];
        let m = decode_pd_wire(&wire).expect("decodes");
        assert_eq!(m.name, "PPS_Status");
        assert_eq!(m.num_objects, 0);
        match m.ext {
            Some(ExtBody::PpsStatus { mv, ma, flags }) => {
                assert_eq!(mv, 9620); // 481 * 20 mV
                assert_eq!(ma, 3200); // 64 * 50 mA
                assert_eq!(flags, 0x02);
            }
            other => panic!("expected PpsStatus, got {other:?}"),
        }
    }

    #[test]
    fn pps_rdo_uses_pps_layout_when_caps_known() {
        // Captured Request selecting PDO#5, which is a PPS APDO.
        let wire = [0x82, 0x12, 0x42, 0xca, 0x03, 0x53];
        let m = decode_pd_wire(&wire).expect("decodes");
        assert!(m.is_request);
        let rdo = m.objects[0];

        // PDO#5 from the same capture: PPS 3.30-11.00V 5.00A.
        let caps = vec![
            0x0801912C, 0x0002D12C, 0x0003C12C, 0x0004B12C, 0xC0DC2164, 0xC1402138,
        ];
        assert_eq!(describe_pdo(4, caps[4]), "#5: PPS 3.30-11.00V 5.00A");

        // With caps known: real request was 9.70 V / 3.30 A.
        let s = describe_rdo(rdo, &caps);
        assert!(s.contains("PPS req 9.70V 3.30A"), "got {s}");

        // With caps unknown it must refuse rather than print the Fixed reading
        // (which would be a plausible-looking but wrong "op=2.42A max=5.78A").
        let s = describe_rdo(rdo, &[]);
        assert!(s.contains("layout unknown"), "got {s}");
        assert!(!s.contains("2.42"), "must not apply Fixed layout: {s}");
    }

    #[test]
    fn fixed_rdo_decodes_operating_current() {
        // Captured Request selecting PDO#1 (Fixed 5V) at 2.00 A.
        let wire = [0x82, 0x18, 0xc8, 0x20, 0x03, 0x13];
        let m = decode_pd_wire(&wire).expect("decodes");
        let caps = vec![0x0801912C];
        let s = describe_rdo(m.objects[0], &caps);
        assert!(s.contains("op=2.00A max=2.00A"), "got {s}");

        let request = decode_pd_request(m.objects[0], &caps);
        assert_eq!(request.pdo_position, 1);
        assert_eq!(request.kind, "fixed");
        assert_eq!(request.voltage_mv, Some(5_000));
        assert_eq!(request.current_ma, Some(2_000));
    }

    #[test]
    fn pps_request_decodes_requested_voltage_and_current() {
        let request = decode_pd_wire(&[0x82, 0x12, 0x42, 0xca, 0x03, 0x53]).expect("request");
        let caps = vec![
            0x0801912C, 0x0002D12C, 0x0003C12C, 0x0004B12C, 0xC0DC2164, 0xC1402138,
        ];

        let decoded = decode_pd_request(request.objects[0], &caps);
        assert_eq!(decoded.pdo_position, 5);
        assert_eq!(decoded.kind, "pps");
        assert_eq!(decoded.voltage_mv, Some(9_700));
        assert_eq!(decoded.current_ma, Some(3_300));
    }

    #[test]
    fn structured_vdm_header_is_decoded_without_guessing_payload() {
        // DisplayPort SVID, structured VDM, ACK to Discover Identity, one payload VDO.
        let header = 0xFF01_A041;
        let detail = describe_vdm(&[header, 0x1122_3344]);
        assert_eq!(detail[0], "Structured VDM: SVID 0xFF01");
        assert!(
            detail[1].contains("Discover Identity (ACK)"),
            "got {detail:?}"
        );
        assert_eq!(detail[2], "Vendor payload: 1 VDO(s), kept opaque");
    }

    #[test]
    fn unstructured_vdm_is_kept_opaque() {
        let detail = describe_vdm(&[0x1234_0000, 0xCAFE_BABE]);
        assert_eq!(detail[0], "Unstructured VDM: SVID 0x1234");
        assert_eq!(detail[1], "Vendor payload: 1 VDO(s), kept opaque");
    }

    #[test]
    fn source_caps_pdos_decode() {
        let wire = [
            0xa1, 0x6f, 0x2c, 0x91, 0x01, 0x08, 0x2c, 0xd1, 0x02, 0x00, 0x2c, 0xc1, 0x03, 0x00,
            0x2c, 0xb1, 0x04, 0x00, 0x64, 0x21, 0xdc, 0xc0, 0x38, 0x21, 0x40, 0xc1,
        ];
        let m = decode_pd_wire(&wire).expect("decodes");
        assert!(m.is_source_caps);
        assert_eq!(m.num_objects, 6);
        assert_eq!(describe_pdo(0, m.objects[0]), "#1: Fixed 5.00V 3.00A");
        assert_eq!(describe_pdo(1, m.objects[1]), "#2: Fixed 9.00V 3.00A");
        assert_eq!(describe_pdo(3, m.objects[3]), "#4: Fixed 15.00V 3.00A");
        assert_eq!(describe_pdo(5, m.objects[5]), "#6: PPS 3.30-16.00V 2.80A");
    }

    #[test]
    fn chained_adc_plus_pd_splits_into_two_packets() {
        // 68-byte shape: main(4) + ext(4) + adc(44) + ext(4) + pd(12).
        let mut resp = vec![0u8; 68];
        resp[0] = CMD_PUT_DATA;
        // ext header 1: attr=ADC, next=1, size=44
        let e1: u32 = (ATT_ADC as u32) | (1 << 15) | (44 << 22);
        resp[4..8].copy_from_slice(&e1.to_le_bytes());
        // ext header 2: attr=PdPacket, next=0, size=12
        let e2: u32 = (ATT_PD_PACKET as u32) | (12 << 22);
        resp[52..56].copy_from_slice(&e2.to_le_bytes());

        let packets = parse_put_data(&resp).expect("parses");
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].attribute, ATT_ADC);
        assert_eq!(packets[0].payload.len(), 44);
        assert_eq!(packets[1].attribute, ATT_PD_PACKET);
        assert_eq!(packets[1].payload.len(), 12);
    }

    #[test]
    fn empty_put_data_is_ok_not_an_error() {
        // A 4-byte PutData with no logical packets is a valid "nothing buffered"
        // reply. It must not be reported as a parse failure.
        let resp = [CMD_PUT_DATA, 0x07, 0x00, 0x00];
        let packets = parse_put_data(&resp).expect("must not error");
        assert!(packets.is_empty());
    }

    #[test]
    fn connection_codes_are_not_guessed() {
        assert_eq!(connection_label(0x11), Some("ATTACH"));
        assert_eq!(connection_label(0x12), Some("DETACH"));
        // Documented-but-unobserved codes stay unnamed.
        assert_eq!(connection_label(0x21), None);
    }

    #[test]
    fn event_stream_splits_connection_and_message() {
        let mut p = vec![0u8; 12]; // preamble
        // Connection event: marker, ts LE24 = 0x001234, reserved, code 0x11
        p.extend_from_slice(&[0x45, 0x34, 0x12, 0x00, 0x00, 0x11]);
        // Wrapped message: size_flag 0x87 -> wire_len = 7-5 = 2
        p.extend_from_slice(&[0x87, 0x01, 0x00, 0x00, 0x00, 0x00, 0x41, 0x02]);

        let (pre, events) = parse_pd_event_stream(&p);
        assert!(pre.is_some());
        assert_eq!(events.len(), 2);
        match &events[0] {
            PdEvent::Connection {
                timestamp_ms, code, ..
            } => {
                assert_eq!(*timestamp_ms, 0x1234);
                assert_eq!(*code, 0x11);
            }
            other => panic!("expected Connection, got {other:?}"),
        }
        match &events[1] {
            PdEvent::Message { sop, wire, .. } => {
                assert_eq!(*sop, 0x00);
                assert_eq!(wire, &[0x41, 0x02]);
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn event_stream_recognizes_v199_device_event_records() {
        let mut p = vec![0u8; 12];
        // A six-byte device-event record observed on KM003C V1.9.9.
        p.extend_from_slice(&[0x05, 0xc5, 0x57, 0xaf, 0x00, 0x04]);
        // Followed by a valid two-byte PD record.
        p.extend_from_slice(&[0x87, 0x01, 0x00, 0x00, 0x00, 0x00, 0x41, 0x02]);

        let (_, events) = parse_pd_event_stream(&p);
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], PdEvent::Device { timestamp_ms, marker, code, .. } if *timestamp_ms == 0x00AF57C5 && *marker == 0x05 && *code == 0x04)
        );
        assert!(matches!(&events[1], PdEvent::Message { wire, .. } if wire == &[0x41, 0x02]));
    }

    #[test]
    fn event_stream_unwraps_v199_nested_container() {
        let mut p = vec![0u8; 12];
        // 0x7B container: 6-byte header plus one inner 0x87 record.
        p.extend_from_slice(&[0x7B, 0x02, 0xAF, 0x00, 0x04, 0x00]);
        p.extend_from_slice(&[0x87, 0x01, 0x00, 0x00, 0x00, 0x00, 0x41, 0x02]);
        p.resize(12 + 60, 0);

        let (_, events) = parse_pd_event_stream(&p);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], PdEvent::Message { wire, .. } if wire == &[0x41, 0x02]));
    }

    #[test]
    fn rejects_truncated_pd_wire_messages() {
        // Header says one data object, but only one object byte follows.
        let err = decode_pd_wire(&[0x82, 0x12, 0x42]).expect_err("must reject truncation");
        assert!(err.contains("declares 1 data objects"), "got {err}");

        // PPS_Status declares four data bytes; its 2-byte extended header plus
        // data must be padded to a 4-byte data-object boundary.
        let err = decode_pd_wire(&[0xac, 0xa5, 0x04, 0x80, 0xe1, 0x01, 0x40, 0x02])
            .expect_err("must reject missing padding");
        assert!(err.contains("expected 10 wire bytes"), "got {err}");
    }

    #[test]
    fn primary_sop_tracks_a_complete_negotiation() {
        // Source_Capabilities and Request are capture-derived samples also used
        // by the PDO/RDO decoder tests above.
        let source = decode_pd_wire(&[
            0xa1, 0x6f, 0x2c, 0x91, 0x01, 0x08, 0x2c, 0xd1, 0x02, 0x00, 0x2c, 0xc1, 0x03, 0x00,
            0x2c, 0xb1, 0x04, 0x00, 0x64, 0x21, 0xdc, 0xc0, 0x38, 0x21, 0x40, 0xc1,
        ])
        .expect("source caps decode");
        let request =
            decode_pd_wire(&[0x82, 0x12, 0x42, 0xca, 0x03, 0x53]).expect("request decode");
        let accept = decode_pd_wire(&[0x03, 0x00]).expect("accept decode");
        let ps_rdy = decode_pd_wire(&[0x06, 0x00]).expect("ps_rdy decode");

        let mut negotiation = PdNegotiation::default();
        assert_eq!(
            negotiation.observe(0, &source),
            Some(NegotiationUpdate::SourceCapabilities)
        );
        assert_eq!(negotiation.source_caps().len(), 6);
        assert_eq!(
            negotiation.observe(0, &request),
            Some(NegotiationUpdate::RequestPending)
        );
        assert_eq!(negotiation.pending_rdo(), Some(request.objects[0]));
        assert_eq!(negotiation.active_rdo(), None);
        assert_eq!(
            negotiation.observe(0, &accept),
            Some(NegotiationUpdate::RequestAccepted)
        );
        assert_eq!(negotiation.pending_rdo(), Some(request.objects[0]));
        assert_eq!(negotiation.active_rdo(), None);
        assert_eq!(
            negotiation.observe(0, &ps_rdy),
            Some(NegotiationUpdate::Complete)
        );
        assert_eq!(negotiation.pending_rdo(), None);
        assert_eq!(negotiation.active_rdo(), Some(request.objects[0]));

        assert_eq!(
            negotiation.observe(0, &source),
            Some(NegotiationUpdate::SourceCapabilities)
        );
        assert_eq!(negotiation.active_rdo(), None);

        // Cable SOP' traffic must not overwrite the primary partner's PDO list.
        assert_eq!(negotiation.observe(1, &source), None);
        assert_eq!(negotiation.source_caps().len(), 6);
    }

    #[test]
    fn rejected_or_deferred_request_does_not_create_a_contract() {
        let source = decode_pd_wire(&[0xa1, 0x10, 0x2c, 0x91, 0x01, 0x08]).expect("source");
        let request = decode_pd_wire(&[0x82, 0x18, 0xc8, 0x20, 0x03, 0x13]).expect("request");
        let reject = decode_pd_wire(&[0x04, 0x00]).expect("reject");
        let wait = decode_pd_wire(&[0x0c, 0x00]).expect("wait");
        let mut negotiation = PdNegotiation::default();

        negotiation.observe(0, &source);
        negotiation.observe(0, &request);
        assert_eq!(
            negotiation.observe(0, &reject),
            Some(NegotiationUpdate::RequestRejected)
        );
        assert_eq!(negotiation.pending_rdo(), None);
        assert_eq!(negotiation.active_rdo(), None);

        negotiation.observe(0, &request);
        assert_eq!(
            negotiation.observe(0, &wait),
            Some(NegotiationUpdate::RequestDeferred)
        );
        assert_eq!(negotiation.pending_rdo(), None);
        assert_eq!(negotiation.active_rdo(), None);
    }

    #[test]
    fn swap_accept_ps_rdy_is_a_control_exchange() {
        let swap = decode_pd_wire(&[0x0b, 0x00]).expect("vconn swap decode");
        let accept = decode_pd_wire(&[0x03, 0x00]).expect("accept decode");
        let ps_rdy = decode_pd_wire(&[0x06, 0x00]).expect("ps_rdy decode");
        let mut negotiation = PdNegotiation::default();

        assert_eq!(
            negotiation.observe(0, &swap),
            Some(NegotiationUpdate::ControlPending("VCONN_Swap"))
        );
        assert_eq!(
            negotiation.observe(0, &accept),
            Some(NegotiationUpdate::ControlAccepted("VCONN_Swap"))
        );
        assert_eq!(
            negotiation.observe(0, &ps_rdy),
            Some(NegotiationUpdate::ControlComplete("VCONN_Swap"))
        );
    }
}
