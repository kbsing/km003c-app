// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri backend for the KM003C monitor.
//!
//! This layer does three things and nothing else:
//!   1. Own the USB device and the acquisition thread (the webview never sees USB).
//!   2. Convert core types into JS-friendly DTOs (raw bytes -> hex, enums -> tagged
//!      structs, PD wire -> already-decoded labels).
//!   3. Forward batches to the frontend as events.
//!
//! All protocol interpretation stays in Rust. The frontend receives decoded text,
//! never raw PD bytes to parse, so there is exactly one implementation of the
//! wire format.

use std::fs::{self, File};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use km003c_core::acquisition::{Acquisition, Batch, Config, Stats, TimedEvent};
use km003c_core::device::list_devices as core_list_devices;
use km003c_core::device::open_device;
use km003c_core::protocol::{
    ExtBody, ExtendedUpdate, PdEvent, PdNegotiation, connection_label, decode_pd_request,
    decode_pd_wire, describe_pdo, describe_rdo, describe_vdm, hex,
};
use km003c_parse::{detail_zh, message_name_zh, role_zh};

mod remote;
use remote::RemoteServer;

// ------------------------------------------------------------------ DTOs

#[derive(Serialize, Clone)]
struct MeasurementDto {
    /// Device monotonic ms. Use this for the time axis.
    device_ms: u32,
    host_ms: u64,
    vbus_v: f64,
    ibus_a: f64,
    power_w: f64,
    cc1_v: f64,
    cc2_v: f64,
    temp_c: Option<f64>,
    /// WITRN only (null elsewhere): D+/D- line voltage, accumulated Ah/Wh.
    d_plus_v: Option<f64>,
    d_minus_v: Option<f64>,
    amp_hours: Option<f64>,
    watt_hours: Option<f64>,
}

#[derive(Serialize, Clone)]
struct StatsDto {
    polls: u64,
    ok: u64,
    empty: u64,
    timeout: u64,
    stall: u64,
    fault: u64,
    os_error: u64,
    parse_error: u64,
    invalid_arg: u64,
    disconnected: u64,
    dropped: u64,
    events_seen: u64,
    largest_response: usize,
    /// Sum of everything that is neither a good sample nor a benign empty reply.
    real_errors: u64,
}

impl From<&Stats> for StatsDto {
    fn from(s: &Stats) -> Self {
        Self {
            polls: s.polls,
            ok: s.ok,
            empty: s.empty,
            timeout: s.timeout,
            stall: s.stall,
            fault: s.fault,
            os_error: s.os_error,
            parse_error: s.parse_error,
            invalid_arg: s.invalid_arg,
            disconnected: s.disconnected,
            dropped: s.dropped,
            events_seen: s.events_seen,
            largest_response: s.largest_response,
            real_errors: s.real_errors(),
        }
    }
}

/// A PD event, fully interpreted on the Rust side.
///
/// `kind` discriminates: "attach" | "detach" | "conn-unknown" |
/// "device-event" | "message" | "unparsed".
#[derive(Serialize, Clone)]
struct EventDto {
    kind: &'static str,
    host_ms: u64,
    /// Device ms from the event record itself (not the poll time).
    device_ms: u32,
    /// One-line summary for the log view.
    summary: String,
    /// Extra decoded lines (PDO list, RDO, PPS status, raw objects).
    detail: Vec<String>,
    /// The complete record as hex, vendor wrapper included. Ground truth for
    /// checking framing.
    raw_hex: String,
    /// Just the USB PD wire bytes, wrapper stripped. Ground truth for checking
    /// the decode against the PD spec. Empty for non-message events.
    wire_hex: String,
    sop: Option<u8>,
    message: Option<PdMessageViewDto>,
}

#[derive(Serialize, Clone)]
struct PowerProfileDto {
    index: usize,
    kind: &'static str,
    min_v: Option<f64>,
    max_v: f64,
    max_a: Option<f64>,
    max_w: Option<f64>,
    selected: bool,
}

#[derive(Serialize, Clone)]
struct PdRequestDto {
    pdo_position: usize,
    kind: &'static str,
    voltage_v: Option<f64>,
    current_a: Option<f64>,
}

#[derive(Serialize, Clone)]
struct PdContractDto {
    phase: &'static str,
    visible_request: Option<PdRequestDto>,
    active_contract: Option<PdRequestDto>,
}

fn power_profiles(objects: &[u32]) -> Vec<PowerProfileDto> {
    objects
        .iter()
        .enumerate()
        .map(|(i, pdo)| match pdo >> 30 {
            0b00 => PowerProfileDto {
                index: i + 1,
                kind: "fixed",
                min_v: None,
                max_v: (((pdo >> 10) & 0x3ff) * 50) as f64 / 1000.0,
                max_a: Some((pdo & 0x3ff) as f64 * 0.01),
                max_w: None,
                selected: false,
            },
            0b01 => PowerProfileDto {
                index: i + 1,
                kind: "battery",
                min_v: Some((((pdo >> 10) & 0x3ff) * 50) as f64 / 1000.0),
                max_v: (((pdo >> 20) & 0x3ff) * 50) as f64 / 1000.0,
                max_a: None,
                max_w: Some((pdo & 0x3ff) as f64 * 0.25),
                selected: false,
            },
            0b10 => PowerProfileDto {
                index: i + 1,
                kind: "variable",
                min_v: Some((((pdo >> 10) & 0x3ff) * 50) as f64 / 1000.0),
                max_v: (((pdo >> 20) & 0x3ff) * 50) as f64 / 1000.0,
                max_a: Some((pdo & 0x3ff) as f64 * 0.01),
                max_w: None,
                selected: false,
            },
            _ if ((pdo >> 28) & 0x3) == 0 => PowerProfileDto {
                index: i + 1,
                kind: "pps",
                min_v: Some((((pdo >> 8) & 0xff) * 100) as f64 / 1000.0),
                max_v: (((pdo >> 17) & 0xff) * 100) as f64 / 1000.0,
                max_a: Some((pdo & 0x7f) as f64 * 0.05),
                max_w: None,
                selected: false,
            },
            _ => PowerProfileDto {
                index: i + 1,
                kind: "apdo",
                min_v: None,
                max_v: 0.0,
                max_a: None,
                max_w: None,
                selected: false,
            },
        })
        .collect()
}

fn request_to_dto(rdo: u32, source_caps: &[u32]) -> PdRequestDto {
    let request = decode_pd_request(rdo, source_caps);
    PdRequestDto {
        pdo_position: request.pdo_position,
        kind: request.kind,
        voltage_v: request.voltage_mv.map(|mv| mv as f64 / 1000.0),
        current_a: request.current_ma.map(|ma| ma as f64 / 1000.0),
    }
}

#[derive(Serialize, Clone)]
struct PdMessageViewDto {
    title: String,
    standard_name: String,
    msg_id: u8,
    spec_rev: String,
    power_role: String,
    data_role: String,
    object_count: usize,
    extended: bool,
}

fn event_to_dto(te: &TimedEvent, negotiation: &mut PdNegotiation) -> EventDto {
    match &te.event {
        PdEvent::Connection {
            timestamp_ms,
            code,
            raw,
        } => {
            // V1.9.9 hardware emits 0x11 attach / 0x12 detach. The protocol
            // reference documents 0x21/0x22; the low nibble agrees, the high
            // nibble does not. connection_label() encodes the observed values.
            let (kind, summary) = match connection_label(*code) {
                Some(label) => {
                    let kind = if label.contains("ATTACH") {
                        "attach"
                    } else {
                        "detach"
                    };
                    let summary = if kind == "attach" {
                        "设备已连接（ATTACH）".to_string()
                    } else {
                        "设备已断开（DETACH）".to_string()
                    };
                    (kind, summary)
                }
                None => ("conn-unknown", format!("未知连接事件码 0x{code:02X}")),
            };
            // A fresh attach/detach invalidates any cached Source_Capabilities.
            negotiation.reset();
            EventDto {
                kind,
                host_ms: te.host_ms,
                device_ms: *timestamp_ms,
                summary,
                detail: Vec::new(),
                raw_hex: hex(raw),
                wire_hex: String::new(),
                sop: None,
                message: None,
            }
        }

        PdEvent::Device {
            timestamp_ms,
            marker,
            code,
            raw,
        } => EventDto {
            kind: "device-event",
            host_ms: te.host_ms,
            device_ms: *timestamp_ms,
            summary: format!("设备事件（标记 0x{marker:02X}，代码 0x{code:02X}）"),
            detail: vec![
                "该事件类型来自仪器固件，暂未找到公开字段定义。原始记录已保留。".to_string(),
            ],
            raw_hex: hex(raw),
            wire_hex: String::new(),
            sop: None,
            message: None,
        },

        PdEvent::Message {
            timestamp_ms,
            sop,
            wire,
            raw,
        } => {
            let mut detail = Vec::new();
            let mut message = None;
            let summary = match decode_pd_wire(wire) {
                Ok(m) => {
                    let standard_name = m.name.clone();
                    message = Some(PdMessageViewDto {
                        title: message_name_zh(&m.name).to_string(),
                        standard_name: standard_name.clone(),
                        msg_id: m.msg_id,
                        spec_rev: m.spec_rev.to_string(),
                        power_role: role_zh(m.power_role).to_string(),
                        data_role: role_zh(m.data_role).to_string(),
                        object_count: m.num_objects,
                        extended: m.ext.is_some(),
                    });
                    // Keep a readable message-header breakdown in the
                    // expandable details area. The compact cards above are
                    // useful for scanning, but control messages have no data
                    // objects and otherwise would show only a negotiation
                    // status line. This mirrors the reference sniffer's
                    // technical-details pane without making the frontend
                    // parse raw PD bytes.
                    let header = u16::from_le_bytes([wire[0], wire[1]]);
                    detail.push(format!("消息头：0x{header:04X}"));
                    detail.push(format!(
                        "消息类型：{}；扩展消息：{}；数据对象：{}；消息 ID：{}；PD 版本：{}；电源角色：{}；数据角色：{}",
                        message_name_zh(&standard_name),
                        if m.ext.is_some() { "是" } else { "否" },
                        m.num_objects,
                        m.msg_id,
                        m.spec_rev,
                        role_zh(m.power_role),
                        role_zh(m.data_role),
                    ));
                    // Extended-message bodies (e.g. PPS_Status) are decoded here,
                    // not in the frontend.
                    match &m.ext {
                        Some(ExtBody::PpsStatus { mv, ma, flags }) => detail.push(format!(
                            "PPS 状态：输出 {:.2} V / {:.2} A，标志位 0x{:02X}",
                            *mv as f64 / 1000.0,
                            *ma as f64 / 1000.0,
                            flags
                        )),
                        Some(ExtBody::Raw(d)) if !d.is_empty() => {
                            detail.push(format!("扩展数据：{}", hex(d)))
                        }
                        _ => {}
                    }

                    if let Some(update) = negotiation.observe_extended(*sop, &m) {
                        match update {
                            ExtendedUpdate::Pending {
                                chunk_number,
                                data_size,
                            } => detail.push(format!(
                                "扩展数据分片：已收到第 {} 段，等待后续分片（共 {} 字节）",
                                chunk_number + 1,
                                data_size
                            )),
                            ExtendedUpdate::RequestChunk { chunk_number } => detail
                                .push(format!("扩展数据分片：对端请求第 {} 段", chunk_number + 1)),
                            ExtendedUpdate::OutOfSequence { chunk_number } => detail.push(format!(
                                "扩展数据分片顺序异常：收到第 {} 段，已保留原始字节",
                                chunk_number + 1
                            )),
                            ExtendedUpdate::Complete(ExtBody::PpsStatus { mv, ma, flags }) => {
                                detail.push(format!(
                                    "PPS 状态（分片重组）：输出 {:.2} V / {:.2} A，标志位 0x{:02X}",
                                    mv as f64 / 1000.0,
                                    ma as f64 / 1000.0,
                                    flags
                                ))
                            }
                            ExtendedUpdate::Complete(ExtBody::Raw(data)) => {
                                detail.push(format!("扩展数据（分片重组）：{}", hex(&data)))
                            }
                            ExtendedUpdate::Complete(ExtBody::Chunk { .. }) => {}
                        }
                    }

                    if m.is_source_caps {
                        for (i, pdo) in m.objects.iter().enumerate() {
                            detail.push(detail_zh(&describe_pdo(i, *pdo)));
                        }
                    } else if m.is_request {
                        if let Some(rdo) = m.objects.first() {
                            detail.push(detail_zh(&describe_rdo(*rdo, negotiation.source_caps())));
                        }
                    } else if standard_name == "Vendor_Defined" {
                        detail.extend(describe_vdm(&m.objects).iter().map(|line| detail_zh(line)));
                        detail.extend(
                            m.objects
                                .iter()
                                .enumerate()
                                .map(|(i, object)| format!("数据对象 #{}：0x{object:08X}", i + 1)),
                        );
                    } else {
                        for (i, o) in m.objects.iter().enumerate() {
                            detail.push(format!("数据对象 #{}：0x{:08X}", i + 1, o));
                        }
                    }

                    if let Some(update) = negotiation.observe(*sop, &m) {
                        detail.push(detail_zh(&update.label()));
                    }

                    format!(
                        "{} · 消息 ID {} · PD {} · {} · {}",
                        message_name_zh(&standard_name),
                        m.msg_id,
                        m.spec_rev,
                        role_zh(m.power_role),
                        role_zh(m.data_role)
                    )
                }
                Err(e) => format!("PD 报文格式错误：{e}"),
            };

            EventDto {
                kind: "message",
                host_ms: te.host_ms,
                device_ms: *timestamp_ms,
                summary,
                detail,
                // Full record including the 6-byte vendor wrapper, for checking
                // framing; wire_hex alone is what the PD spec describes.
                raw_hex: hex(raw),
                wire_hex: hex(wire),
                sop: Some(*sop),
                message,
            }
        }

        PdEvent::Unknown { offset, rest } => EventDto {
            kind: "unparsed",
            host_ms: te.host_ms,
            device_ms: 0,
            summary: format!("未解析事件流：偏移 {offset}，剩余 {} 字节", rest.len()),
            detail: Vec::new(),
            raw_hex: hex(rest),
            wire_hex: String::new(),
            sop: None,
            message: None,
        },
    }
}

#[derive(Serialize, Clone)]
struct BatchDto {
    measurements: Vec<MeasurementDto>,
    events: Vec<EventDto>,
    /// Current primary-SOP charger capability snapshot. Unlike event history,
    /// this persists through batches containing only status/control traffic.
    power_profiles: Vec<PowerProfileDto>,
    contract: PdContractDto,
    stats: StatsDto,
}

fn batch_to_dto(b: &Batch, negotiation: &mut PdNegotiation) -> BatchDto {
    let events = b
        .events
        .iter()
        .map(|event| event_to_dto(event, negotiation))
        .collect();
    BatchDto {
        measurements: b
            .measurements
            .iter()
            .map(|m| MeasurementDto {
                device_ms: m.device_ms,
                host_ms: m.host_ms,
                vbus_v: m.vbus_v,
                ibus_a: m.ibus_a,
                power_w: m.power_w(),
                cc1_v: m.cc1_v,
                cc2_v: m.cc2_v,
                temp_c: m.temp_c,
                d_plus_v: m.d_plus_v,
                d_minus_v: m.d_minus_v,
                amp_hours: m.amp_hours,
                watt_hours: m.watt_hours,
            })
            .collect(),
        events,
        power_profiles: power_profiles(negotiation.source_caps()),
        contract: PdContractDto {
            phase: negotiation.phase_label(),
            visible_request: negotiation
                .pending_rdo()
                .map(|rdo| request_to_dto(rdo, negotiation.source_caps())),
            active_contract: negotiation
                .active_rdo()
                .map(|rdo| request_to_dto(rdo, negotiation.source_caps())),
        },
        stats: StatsDto::from(&b.stats),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RecordingOptions {
    current_threshold_ma: f64,
    hold_seconds: u64,
    stop_when_condition_met: bool,
    use_system_time: bool,
    edge_save: bool,
    max_buffer_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
struct RecordingStatusDto {
    active: bool,
    file_path: String,
    started_unix_ms: Option<u64>,
    samples: u64,
    events: u64,
    bytes_written: u64,
    buffered_bytes: u64,
    max_buffer_bytes: u64,
    stop_reason: Option<String>,
}

struct SessionRecorder {
    writer: BufWriter<File>,
    file_path: PathBuf,
    options: RecordingOptions,
    started_unix_ms: Option<u64>,
    pending: Vec<u8>,
    bytes_written: u64,
    samples: u64,
    events: u64,
    /// Wall-clock start of the current "current below threshold" streak.
    /// `Instant` (not a per-acquisition host_ms) so the hold timing stays
    /// correct even if the host-side clock basis changes.
    condition_started: Option<Instant>,
}

impl SessionRecorder {
    fn new(file_path: PathBuf, file: File, options: RecordingOptions, device_kind: Option<String>) -> Result<Self, String> {
        let started_unix_ms = options.use_system_time.then(|| unix_ms()).transpose()?;
        let mut recorder = Self {
            writer: BufWriter::new(file),
            file_path,
            options,
            started_unix_ms,
            pending: Vec::new(),
            bytes_written: 0,
            samples: 0,
            events: 0,
            condition_started: None,
        };
        recorder.write_json_line(&json!({
            "kind": "header",
            "schema_version": 2,
            "started_unix_ms": recorder.started_unix_ms,
            "device_kind": device_kind,
            "options": recorder.options.clone(),
        }))?;
        recorder.flush_pending()?;
        Ok(recorder)
    }

    fn max_buffer_bytes(&self) -> u64 {
        self.options.max_buffer_mb.saturating_mul(1024 * 1024)
    }

    fn write_json_line(&mut self, value: &serde_json::Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        line.push(b'\n');
        self.pending.extend_from_slice(&line);
        Ok(())
    }

    fn flush_pending(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.writer
            .write_all(&self.pending)
            .map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        self.bytes_written = self.bytes_written.saturating_add(self.pending.len() as u64);
        self.pending.clear();
        Ok(())
    }

    fn record_batch(&mut self, batch: &BatchDto) -> Result<bool, String> {
        self.write_json_line(&json!({ "kind": "batch", "data": batch }))?;
        self.samples = self.samples.saturating_add(batch.measurements.len() as u64);
        self.events = self.events.saturating_add(batch.events.len() as u64);

        let mut should_stop = false;
        if self.options.stop_when_condition_met {
            let threshold_a = self.options.current_threshold_ma.max(0.0) / 1000.0;
            let now = Instant::now();
            for measurement in &batch.measurements {
                if measurement.ibus_a.abs() <= threshold_a {
                    let started = self.condition_started.get_or_insert_with(|| now);
                    if now.duration_since(*started)
                        >= Duration::from_millis(self.options.hold_seconds.saturating_mul(1000))
                    {
                        should_stop = true;
                        break;
                    }
                } else {
                    self.condition_started = None;
                }
            }
        }

        if self.options.edge_save || self.pending.len() as u64 >= self.max_buffer_bytes() {
            self.flush_pending()?;
        }
        Ok(should_stop)
    }

    fn finish(mut self, reason: Option<String>) -> Result<RecordingStatusDto, String> {
        self.flush_pending()?;
        Ok(RecordingStatusDto {
            active: false,
            file_path: self.file_path.display().to_string(),
            started_unix_ms: self.started_unix_ms,
            samples: self.samples,
            events: self.events,
            bytes_written: self.bytes_written,
            buffered_bytes: 0,
            max_buffer_bytes: self.max_buffer_bytes(),
            stop_reason: reason,
        })
    }

    fn status(&self) -> RecordingStatusDto {
        RecordingStatusDto {
            active: true,
            file_path: self.file_path.display().to_string(),
            started_unix_ms: self.started_unix_ms,
            samples: self.samples,
            events: self.events,
            bytes_written: self.bytes_written,
            buffered_bytes: self.pending.len() as u64,
            max_buffer_bytes: self.max_buffer_bytes(),
            stop_reason: None,
        }
    }
}

fn unix_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .map_err(|e| e.to_string())
}

fn finish_active_recording(
    recording: &Mutex<Option<SessionRecorder>>,
    reason: &str,
) -> Result<Option<RecordingStatusDto>, String> {
    let recorder = recording
        .lock()
        .map_err(|_| "recording state poisoned".to_string())?
        .take();
    recorder
        .map(|recorder| recorder.finish(Some(reason.to_string())))
        .transpose()
}

fn validate_recording_options(options: &RecordingOptions) -> Result<(), String> {
    if !options.current_threshold_ma.is_finite() || options.current_threshold_ma < 0.0 {
        return Err("current threshold must be a non-negative number".into());
    }
    // The hold duration only feeds the auto-stop condition; do not reject a
    // recording whose saved options happen to carry an out-of-range value
    // while auto-stop is off.
    if options.stop_when_condition_met && !(1..=86_400).contains(&options.hold_seconds) {
        return Err("hold duration must be between 1 and 86400 seconds".into());
    }
    if !(1..=1024).contains(&options.max_buffer_mb) {
        return Err("buffer depth must be between 1 and 1024 MB".into());
    }
    Ok(())
}

// ------------------------------------------------------------------ state

#[derive(Default)]
struct AppState {
    /// Set false to ask the acquisition thread to wind down.
    running: Arc<AtomicBool>,
    /// Bumped by every `start_acquisition`. A winding-down forwarding thread
    /// checks it before touching shared state, so a late wakeup (recv_timeout
    /// sleeps up to 250 ms; a recording flush can take longer) cannot clobber
    /// a session that started in the meantime.
    session_epoch: Arc<AtomicU64>,
    device_label: Mutex<Option<String>>,
    device_kind: Arc<Mutex<Option<String>>>,
    recording: Arc<Mutex<Option<SessionRecorder>>>,
    remote: RemoteServer,
}

#[derive(Serialize)]
struct StartResult {
    product: String,
    serial: String,
    /// Brand label ("KM003C" / "WITRN" / "Power-Pico") for UI display.
    kind: String,
}

#[derive(Serialize, Clone)]
struct DeviceInfoDto {
    id: String,
    name: String,
    serial: String,
    kind: String,
}

/// Enumerate every supported instrument currently attached (KM003C and WITRN
/// K2 today). The frontend uses this to render the device picker.
#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfoDto>, String> {
    Ok(core_list_devices()
        .into_iter()
        .map(|info| DeviceInfoDto {
            id: info.id,
            name: info.name,
            serial: info.serial,
            kind: info.kind.to_string(),
        })
        .collect())
}

#[tauri::command]
fn start_acquisition(
    app: AppHandle,
    state: State<AppState>,
    device_id: Option<String>,
) -> Result<StartResult, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("acquisition already running".into());
    }
    let epoch = state.session_epoch.fetch_add(1, Ordering::SeqCst) + 1;

    // Backward-compatible default: the app used to be single-device.
    let device_id = device_id.unwrap_or_else(|| "km003c".to_string());
    let mut dev = open_device(&device_id)?;
    let product = dev.product().to_string();
    let serial = dev.serial().to_string();
    let kind = dev.kind().to_string();
    let device_kind_state = state.device_kind.clone();

    // Clear anything a previous session left buffered, then open the session.
    dev.drain();
    dev.connect()?;

    let running = state.running.clone();
    let recording = state.recording.clone();
    let remote = state.remote.clone();
    let session_epoch = state.session_epoch.clone();
    running.store(true, Ordering::SeqCst);
    *state.device_label.lock().unwrap() = Some(format!("{product} ({serial})"));
    *state.device_kind.lock().unwrap() = Some(kind.clone());
    remote.set_connected(true, format!("{product} ({serial})"));

    std::thread::spawn(move || {
        let acq = Acquisition::start(dev, Config::default());
        // PDO context must persist across batches so a later Request can use
        // the correct RDO layout and negotiation phase.
        let mut negotiation = PdNegotiation::default();

        while running.load(Ordering::SeqCst) && session_epoch.load(Ordering::SeqCst) == epoch {
            match acq.rx.recv_timeout(Duration::from_millis(250)) {
                Ok(batch) => {
                    if session_epoch.load(Ordering::SeqCst) != epoch {
                        break; // a newer session owns shared state now
                    }
                    let dto = batch_to_dto(&batch, &mut negotiation);
                    let auto_stop = {
                        let mut guard = match recording.lock() {
                            Ok(guard) => guard,
                            Err(_) => {
                                let _ = app.emit("recording-error", "recording state poisoned");
                                break;
                            }
                        };
                        if let Some(recorder) = guard.as_mut() {
                            match recorder.record_batch(&dto) {
                                Ok(auto_stop) => auto_stop,
                                Err(error) => {
                                    let _ = app.emit("recording-error", error.clone());
                                    let recorder = guard.take();
                                    if let Some(recorder) = recorder {
                                        let _ = recorder.finish(Some("write error".into()));
                                    }
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    };
                    // Mirror the batch to LAN browser viewers (no-op when the
                    // remote server is stopped). Same DTO, serialized once.
                    remote.publish(&dto);
                    if app.emit("batch", dto).is_err() {
                        break; // window gone
                    }
                    if auto_stop {
                        if let Ok(Some(status)) =
                            finish_active_recording(&recording, "condition met")
                        {
                            let _ = app.emit("recording-stopped", status);
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        // Only the session that still owns the epoch may reset shared state;
        // a stale thread just stops its own acquisition quietly so it cannot
        // end a newer session's recording or flip its connection flags.
        if session_epoch.load(Ordering::SeqCst) == epoch {
            if let Ok(Some(status)) = finish_active_recording(&recording, "acquisition stopped") {
                let _ = app.emit("recording-stopped", status);
            }
            running.store(false, Ordering::SeqCst);
            if let Ok(mut current_kind) = device_kind_state.lock() {
                *current_kind = None;
            }
            remote.set_connected(false, String::new());
        }
        match acq.stop() {
            Ok(stats) => {
                if session_epoch.load(Ordering::SeqCst) == epoch {
                    let _ = app.emit("acq-stopped", StatsDto::from(&stats));
                }
            }
            Err(e) => {
                if session_epoch.load(Ordering::SeqCst) == epoch {
                    let _ = app.emit("acq-error", e);
                }
            }
        }
    });

    Ok(StartResult {
        product,
        serial,
        kind,
    })
}

#[tauri::command]
fn stop_acquisition(state: State<AppState>) {
    state.running.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn start_recording(
    app: AppHandle,
    state: State<AppState>,
    options: RecordingOptions,
) -> Result<RecordingStatusDto, String> {
    validate_recording_options(&options)?;
    if !state.running.load(Ordering::SeqCst) {
        return Err("connect the device before starting a recording".into());
    }

    let mut guard = state
        .recording
        .lock()
        .map_err(|_| "recording state poisoned".to_string())?;
    if guard.is_some() {
        return Err("recording already active".into());
    }

    let recordings_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("recordings");
    fs::create_dir_all(&recordings_dir).map_err(|e| e.to_string())?;
    let timestamp = unix_ms()?;
    let file_path = recordings_dir.join(format!("km003c-session-{timestamp}.ndjson"));
    let file = File::create(&file_path).map_err(|e| e.to_string())?;
    let device_kind = state
        .device_kind
        .lock()
        .map_err(|_| "device state poisoned".to_string())?
        .clone();
    let recorder = SessionRecorder::new(file_path, file, options, device_kind)?;
    let status = recorder.status();
    *guard = Some(recorder);
    Ok(status)
}

#[tauri::command]
fn stop_recording(state: State<AppState>) -> Result<RecordingStatusDto, String> {
    let recorder = state
        .recording
        .lock()
        .map_err(|_| "recording state poisoned".to_string())?
        .take()
        .ok_or_else(|| "no active recording".to_string())?;
    recorder.finish(Some("manual stop".into()))
}

// ----------------------------------------------------- recording convenience

fn recordings_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("recordings");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg(target_os = "windows")]
fn open_path(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map_err(|e| format!("打开目录失败：{e}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn open_path(_path: &Path) -> Result<(), String> {
    Err("此操作仅在 Windows 上可用".into())
}

fn picked_path(picked: Option<FilePath>) -> Result<Option<PathBuf>, String> {
    match picked {
        Some(FilePath::Path(path)) => Ok(Some(path)),
        Some(FilePath::Url(url)) => url
            .to_file_path()
            .map(Some)
            .map_err(|_| "无法解析目标路径".to_string()),
        None => Ok(None),
    }
}

/// Open the recordings folder in the system file manager.
#[tauri::command]
fn open_recordings_dir(app: AppHandle) -> Result<(), String> {
    let dir = recordings_dir(&app)?;
    open_path(&dir)
}

/// Copy a recording file to a user-chosen location.
#[tauri::command]
fn save_recording_as(app: AppHandle, source_path: String) -> Result<Option<String>, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_file() {
        return Err(format!("记录文件不存在：{source_path}"));
    }
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("km003c-session.ndjson")
        .to_string();
    let picked = app
        .dialog()
        .file()
        .add_filter("NDJSON 会话记录", &["ndjson"])
        .set_file_name(&file_name)
        .blocking_save_file();
    let Some(dest) = picked_path(picked)? else {
        return Ok(None);
    };
    fs::copy(&source, &dest).map_err(|e| format!("复制失败：{e}"))?;
    Ok(Some(dest.display().to_string()))
}

/// Convert a recording file's measurement batches to a CSV the user chooses.
#[tauri::command]
fn convert_recording_to_csv(
    app: AppHandle,
    source_path: String,
    range_start_host_ms: Option<u64>,
    range_end_host_ms: Option<u64>,
) -> Result<Option<String>, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_file() {
        return Err(format!("记录文件不存在：{source_path}"));
    }
    let stem = csv_default_stem(&source);
    let picked = app
        .dialog()
        .file()
        .add_filter("CSV 表格", &["csv"])
        .set_file_name(&format!("{stem}.csv"))
        .blocking_save_file();
    let Some(dest) = picked_path(picked)? else {
        return Ok(None);
    };
    let rows = ndjson_to_csv(&source, &dest, range_start_host_ms, range_end_host_ms)?;
    Ok(Some(format!("{}（{rows} 行）", dest.display())))
}

fn csv_default_stem(source: &Path) -> String {
    let source_stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("km003c-session")
        .to_string();
    let fallback = if source_stem.starts_with("km003c-session") {
        "km003c".to_string()
    } else {
        source_stem
    };
    let Ok(file) = File::open(source) else {
        return fallback;
    };
    let Some(Ok(line)) = std::io::BufReader::new(file).lines().next() else {
        return fallback;
    };
    let Ok(header) = serde_json::from_str::<serde_json::Value>(&line) else {
        return fallback;
    };
    let Some(kind) = header.get("device_kind").and_then(serde_json::Value::as_str) else {
        return fallback;
    };
    match kind.to_ascii_lowercase().as_str() {
        "km003c" => "km003c".into(),
        "witrn" => "witrn".into(),
        "power-pico" | "powerpico" => "powerpico".into(),
        _ => fallback,
    }
}

/// Read the NDJSON recording and write a measurements CSV. Line-by-line so it
/// works on multi-hundred-MB files without loading everything into memory.
fn in_export_range(value: &serde_json::Value, start: Option<u64>, end: Option<u64>) -> bool {
    let Some(host_ms) = value.get("host_ms").and_then(serde_json::Value::as_u64) else {
        return start.is_none() && end.is_none();
    };
    start.map_or(true, |bound| host_ms >= bound) && end.map_or(true, |bound| host_ms <= bound)
}

fn enrich_export_measurement(mut value: serde_json::Value) -> serde_json::Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    let current = object.get("ibus_a").and_then(serde_json::Value::as_f64);
    let power = object
        .get("power_w")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| Some(object.get("vbus_v").and_then(serde_json::Value::as_f64)? * current?));
    if let Some(value) = current {
        object.insert("ibus_abs_a".into(), json!(value.abs()));
    }
    if let Some(value) = power {
        object.insert("power_w".into(), json!(value));
        object.insert("power_abs_w".into(), json!(value.abs()));
    }
    value
}

fn ndjson_to_csv(
    source: &Path,
    dest: &Path,
    range_start_host_ms: Option<u64>,
    range_end_host_ms: Option<u64>,
) -> Result<u64, String> {
    let reader = std::io::BufReader::new(File::open(source).map_err(|e| e.to_string())?);
    let out = File::create(dest).map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(out);
    writeln!(
        writer,
        "device_ms,host_ms,elapsed_s,vbus_v,ibus_signed_a,ibus_abs_a,power_signed_w,power_abs_w,cc1_v,cc2_v,temp_c,d_plus_v,d_minus_v,amp_hours,watt_hours"
    )
    .map_err(|e| e.to_string())?;

    let mut rows = 0u64;
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if value["kind"].as_str() != Some("batch") {
            continue;
        }
        let Some(measurements) = value["data"]["measurements"].as_array() else {
            continue;
        };
        for m in measurements {
            if !in_export_range(m, range_start_host_ms, range_end_host_ms) {
                continue;
            }
            let num = |key: &str| {
                m.get(key)
                    .and_then(|v| v.as_f64())
                    .map(|x| x.to_string())
                    .unwrap_or_default()
            };
            let power = m
                .get("power_w")
                .and_then(|v| v.as_f64())
                .or_else(|| Some(m["vbus_v"].as_f64()? * m["ibus_a"].as_f64()?));
            let power_text = power.map(|x| x.to_string()).unwrap_or_default();
            let power_abs_text = power.map(|x| x.abs().to_string()).unwrap_or_default();
            let ibus_abs_text = m["ibus_a"]
                .as_f64()
                .map(|x| x.abs().to_string())
                .unwrap_or_default();
            writeln!(
                writer,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                m["device_ms"].as_u64().unwrap_or_default(),
                m["host_ms"].as_u64().unwrap_or_default(),
                m["host_ms"]
                    .as_u64()
                    .map(|x| (x as f64 / 1000.0).to_string())
                    .unwrap_or_default(),
                num("vbus_v"),
                num("ibus_a"),
                ibus_abs_text,
                power_text,
                power_abs_text,
                num("cc1_v"),
                num("cc2_v"),
                num("temp_c"),
                num("d_plus_v"),
                num("d_minus_v"),
                num("amp_hours"),
                num("watt_hours"),
            )
            .map_err(|e| e.to_string())?;
            rows += 1;
        }
    }
    writer.flush().map_err(|e| e.to_string())?;
    Ok(rows)
}

const MAX_LAST_SESSION_SAMPLES: usize = 200_000;
const MAX_LAST_SESSION_EVENTS: usize = 2000;

/// Read a session NDJSON and collect measurements, PD events, the last stats
/// and the header's started_unix_ms, as JSON values matching the DTOs the
/// frontend consumes. Line-by-line so big files are not loaded wholesale.
fn read_recording(
    source: &Path,
) -> Result<
    (
        Vec<serde_json::Value>,
        Vec<serde_json::Value>,
        Option<serde_json::Value>,
        Option<u64>,
    ),
    String,
> {
    let reader = std::io::BufReader::new(File::open(source).map_err(|e| e.to_string())?);
    let mut measurements = Vec::new();
    let mut events = Vec::new();
    let mut stats = None;
    let mut started_unix_ms = None;
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match value["kind"].as_str() {
            Some("header") => started_unix_ms = value["started_unix_ms"].as_u64(),
            Some("batch") => {
                if let Some(ms) = value["data"]["measurements"].as_array() {
                    for m in ms {
                        measurements.push(m.clone());
                    }
                }
                if let Some(es) = value["data"]["events"].as_array() {
                    for e in es {
                        events.push(e.clone());
                    }
                }
                if let Some(s) = value["data"]["stats"].as_object() {
                    stats = Some(serde_json::Value::Object(s.clone()));
                }
            }
            _ => {}
        }
    }
    Ok((measurements, events, stats, started_unix_ms))
}

fn latest_recording(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let dir = recordings_dir(app)?;
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |e| e == "ndjson"))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map_or(false, |n| n.starts_with("km003c-session-"))
        })
        .collect();
    files.sort();
    Ok(files.pop())
}

#[derive(Serialize)]
struct LastSessionDto {
    found: bool,
    file_path: String,
    started_unix_ms: Option<u64>,
    measurements: Vec<serde_json::Value>,
    events: Vec<serde_json::Value>,
    stats: Option<serde_json::Value>,
}

/// Load the most recent session recording so the app can restore the previous
/// run's data after an unexpected close. The NDJSON persists on disk because
/// edge_save (default on) flushes every batch.
#[tauri::command]
fn load_last_session(app: AppHandle) -> Result<LastSessionDto, String> {
    let empty = || LastSessionDto {
        found: false,
        file_path: String::new(),
        started_unix_ms: None,
        measurements: Vec::new(),
        events: Vec::new(),
        stats: None,
    };
    let Some(path) = latest_recording(&app)? else {
        return Ok(empty());
    };
    let (mut measurements, mut events, stats, started_unix_ms) = read_recording(&path)?;
    let overflow = measurements.len().saturating_sub(MAX_LAST_SESSION_SAMPLES);
    if overflow > 0 {
        measurements.drain(0..overflow);
    }
    let overflow = events.len().saturating_sub(MAX_LAST_SESSION_EVENTS);
    if overflow > 0 {
        events.drain(0..overflow);
    }
    Ok(LastSessionDto {
        found: !measurements.is_empty() || !events.is_empty(),
        file_path: path.display().to_string(),
        started_unix_ms,
        measurements,
        events,
        stats,
    })
}

/// Convert a recording NDJSON to a diagnostic JSON the user chooses.
#[tauri::command]
fn convert_recording_to_json(
    app: AppHandle,
    source_path: String,
    range_start_host_ms: Option<u64>,
    range_end_host_ms: Option<u64>,
) -> Result<Option<String>, String> {
    let source = PathBuf::from(&source_path);
    if !source.is_file() {
        return Err(format!("记录文件不存在：{source_path}"));
    }
    let (mut measurements, mut events, stats, started_unix_ms) = read_recording(&source)?;
    measurements = measurements
        .into_iter()
        .filter(|value| in_export_range(value, range_start_host_ms, range_end_host_ms))
        .map(enrich_export_measurement)
        .collect();
    events.retain(|value| in_export_range(value, range_start_host_ms, range_end_host_ms));
    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("km003c-session");
    let picked = app
        .dialog()
        .file()
        .add_filter("JSON 诊断", &["json"])
        .set_file_name(&format!("{stem}.json"))
        .blocking_save_file();
    let Some(dest) = picked_path(picked)? else {
        return Ok(None);
    };
    let doc = serde_json::json!({
        "schema_version": 2,
        "exported_at_unix_ms": unix_ms().ok(),
        "started_unix_ms": started_unix_ms,
        "range": {
            "kind": if range_start_host_ms.is_some() || range_end_host_ms.is_some() { "host_ms" } else { "full" },
            "start_host_ms": range_start_host_ms,
            "end_host_ms": range_end_host_ms,
        },
        "columns": {
            "ibus_a": "signed instrument reading (A)",
            "ibus_abs_a": "absolute current magnitude (A)",
            "power_w": "signed vbus_v * ibus_a (W)",
            "power_abs_w": "absolute power magnitude (W)",
            "host_ms": "elapsed host time since acquisition start (ms)",
            "device_ms": "device monotonic clock (ms, may wrap)",
        },
        "measurements": measurements,
        "pd_events": events,
        "stats": stats,
    });
    let bytes = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&dest, bytes).map_err(|e| format!("写入失败：{e}"))?;
    Ok(Some(format!("{}", dest.display())))
}

/// Open the save dialog for a chart PNG and return the chosen path.
///
/// Splitting the picker from the write lets the webview render and encode the
/// image only after a destination exists: the dialog opens instantly on click,
/// and cancelling it wastes no render work.
#[tauri::command]
fn pick_chart_png_path(app: AppHandle, default_name: String) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter("PNG 图片", &["png"])
        .set_file_name(&default_name)
        .blocking_save_file();
    Ok(picked_path(picked)?.map(|path| path.display().to_string()))
}

/// Write a chart PNG (base64 data URL) to a path picked via
/// `pick_chart_png_path`.
#[tauri::command]
fn write_chart_png(path: String, data_url: String) -> Result<(), String> {
    use base64::Engine;
    let payload = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "不是有效的 PNG 数据 URL".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("PNG 解码失败：{e}"))?;
    std::fs::write(&path, &bytes).map_err(|e| format!("写入失败：{e}"))?;
    Ok(())
}

#[tauri::command]
fn acquisition_running(state: State<AppState>) -> bool {
    state.running.load(Ordering::SeqCst)
}

#[derive(Serialize, Clone)]
struct RemoteStatusDto {
    running: bool,
    /// Browser address (http://<lan-ip>:<port>/). Empty when stopped.
    url: String,
}

/// Start the LAN browser viewer on `0.0.0.0:port`.
#[tauri::command]
fn start_remote_viewer(
    state: State<AppState>,
    port: Option<u16>,
) -> Result<RemoteStatusDto, String> {
    let port = port.unwrap_or(8765);
    if !(1..=65535).contains(&port) {
        return Err("端口必须在 1-65535 之间".into());
    }
    state.remote.start(port)?;
    Ok(RemoteStatusDto {
        running: state.remote.is_running(),
        url: state.remote.url(),
    })
}

#[tauri::command]
fn stop_remote_viewer(state: State<AppState>) -> Result<RemoteStatusDto, String> {
    state.remote.stop();
    Ok(RemoteStatusDto {
        running: false,
        url: String::new(),
    })
}

#[tauri::command]
fn remote_viewer_status(state: State<AppState>) -> RemoteStatusDto {
    RemoteStatusDto {
        running: state.remote.is_running(),
        url: state.remote.url(),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app.manage(AppState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_acquisition,
            stop_acquisition,
            acquisition_running,
            list_devices,
            start_recording,
            stop_recording,
            open_recordings_dir,
            save_recording_as,
            convert_recording_to_csv,
            convert_recording_to_json,
            load_last_session,
            pick_chart_png_path,
            write_chart_png,
            start_remote_viewer,
            stop_remote_viewer,
            remote_viewer_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
