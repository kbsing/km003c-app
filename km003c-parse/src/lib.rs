//! Human-readable USB PD decoding as a reusable library.
//!
//! This crate is the "explain a PD frame" layer: it takes USB PD wire bytes
//! (header + objects + extended payload, **no CRC**) and returns a structured,
//! Chinese-labelled `PdDecode` that serializes to JSON. The heavy lifting
//! (`decode_pd_wire`, `describe_pdo`, `describe_rdo`, `describe_vdm`) comes
//! from `km003c-core`; this crate adds the presentation layer (Chinese names,
//! summary line, detail lines) and a stable C ABI for non-Rust consumers.
//!
//! The C ABI is the same decoder exposed as a DLL. Every function returns a
//! JSON string you must free with [`km003c_free`]:
//!
//! ```c
//! char* json = km003c_parse_pd_hex("01106421dcc0");
//! // ... use json ...
//! km003c_free(json);
//! ```
//!
//! Panics never cross the FFI boundary: they are caught and reported as
//! `{"ok": false, "error": "..."}`.
//!
//! Validation boundary: the decoder is verified against a real KM003C for the
//! main paths; some (WITRN / Power-Pico, specific extended messages) are
//! hex-verified only.

use serde::Serialize;

use km003c_core::protocol::{
    ExtBody, decode_pd_wire, describe_pdo, describe_rdo, describe_vdm, hex,
};

// ------------------------------------------------------------------ rendering

/// Chinese name for a standard USB PD message type (shared with the Tauri app).
pub fn message_name_zh(name: &str) -> &str {
    match name {
        "GoodCRC" => "确认（GoodCRC）",
        "GotoMin" => "请求降至最小功率（GotoMin）",
        "Accept" => "接受（Accept）",
        "Reject" => "拒绝（Reject）",
        "Ping" => "探测（Ping）",
        "PS_RDY" => "电源就绪（PS_RDY）",
        "Get_Source_Cap" => "请求源端能力（Get_Source_Cap）",
        "Get_Sink_Cap" => "请求受电端能力（Get_Sink_Cap）",
        "DR_Swap" => "数据角色切换（DR_Swap）",
        "PR_Swap" => "电源角色切换（PR_Swap）",
        "VCONN_Swap" => "VCONN 切换（VCONN_Swap）",
        "Wait" => "等待（Wait）",
        "Soft_Reset" => "软复位（Soft_Reset）",
        "Data_Reset" => "数据复位（Data_Reset）",
        "Data_Reset_Complete" => "数据复位完成（Data_Reset_Complete）",
        "Not_Supported" => "不支持（Not_Supported）",
        "FR_Swap" => "快速角色切换（FR_Swap）",
        "Source_Capabilities" => "源端能力（Source_Capabilities）",
        "Request" => "电源请求（Request）",
        "BIST" => "自检（BIST）",
        "Sink_Capabilities" => "受电端能力（Sink_Capabilities）",
        "Battery_Status" => "电池状态（Battery_Status）",
        "Alert" => "告警（Alert）",
        "Vendor_Defined" => "厂商自定义（Vendor_Defined）",
        "PPS_Status" => "PPS 状态（PPS_Status）",
        _ => name,
    }
}

/// Chinese label for a USB PD power/data role.
pub fn role_zh(role: &str) -> &str {
    match role {
        "Src" => "供电端",
        "Snk" => "受电端",
        "DFP" => "下行端",
        "UFP" => "上行端",
        _ => role,
    }
}

/// Translates the English decode lines produced by `km003c-core` into Chinese.
pub fn detail_zh(detail: &str) -> String {
    let mut value = detail.to_string();
    for (from, to) in [
        ("Fixed", "固定档"),
        ("Battery", "电池档"),
        ("Variable", "可变档"),
        ("PPS req", "PPS 请求"),
        ("Structured VDM", "结构化厂商消息（VDM）"),
        ("Unstructured VDM", "非结构化厂商消息（VDM）"),
        ("VDM has no header object", "厂商消息缺少 VDM 头对象"),
        ("Vendor payload", "厂商载荷"),
        ("kept opaque", "已保留原始数据，含义待确认"),
        ("Discover Identity", "发现身份"),
        ("Discover SVIDs", "发现厂商 ID"),
        ("Discover Modes", "发现模式"),
        ("Enter Mode", "进入模式"),
        ("Exit Mode", "退出模式"),
        ("Attention", "注意通知"),
        ("Initiator", "发起方"),
        ("version field", "版本字段"),
        ("mode position", "模式位置"),
        ("Command", "命令"),
        (
            "layout unknown: no Source_Capabilities yet",
            "布局未知：尚未收到源端能力",
        ),
        ("op=", "工作电流="),
        ("max=", "最大电流="),
        ("raw=", "原始值="),
        ("negotiation: ", "协商："),
        ("Source_Capabilities received", "已收到源端能力"),
        ("Request pending Accept", "等待接受请求"),
        (
            "Request seen before Source_Capabilities",
            "在源端能力之前收到请求",
        ),
        ("Request accepted", "请求已接受"),
        (
            "Accept without a tracked Request",
            "收到未匹配请求的接受消息",
        ),
        ("complete (PS_RDY)", "协商完成（PS_RDY）"),
        (
            "PS_RDY without a tracked Accept",
            "收到未匹配接受消息的 PS_RDY",
        ),
        ("pending Accept", "等待接受"),
        ("accepted", "已接受"),
        ("complete (PS_RDY)", "完成（PS_RDY）"),
    ] {
        value = value.replace(from, to);
    }
    value
}

// ------------------------------------------------------------------ API

/// Structured, Chinese-labelled interpretation of one USB PD wire frame.
#[derive(Serialize, Clone, Debug)]
pub struct PdDecode {
    pub standard_name: String,
    pub title_zh: String,
    pub summary_zh: String,
    pub msg_id: u8,
    pub spec_rev: String,
    pub power_role: String,
    pub data_role: String,
    pub object_count: usize,
    pub extended: bool,
    pub detail: Vec<String>,
    pub objects: Vec<u32>,
    pub wire_hex: String,
}

impl PdDecode {
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Decode one USB PD wire frame (header + objects + extended payload, **no
/// trailing CRC** — same input `km003c-core`'s `decode_pd_wire` expects).
pub fn parse_pd_wire(wire: &[u8]) -> Result<PdDecode, String> {
    let m = decode_pd_wire(wire).map_err(|e| e.to_string())?;
    let standard_name = m.name.clone();
    let mut detail = Vec::new();
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
    match &m.ext {
        Some(ExtBody::PpsStatus { mv, ma, flags }) => detail.push(format!(
            "PPS 状态：输出 {:.2} V / {:.2} A，标志位 0x{:02X}",
            *mv as f64 / 1000.0,
            *ma as f64 / 1000.0,
            flags
        )),
        Some(ExtBody::Raw(d)) if !d.is_empty() => detail.push(format!("扩展数据：{}", hex(d))),
        _ => {}
    }
    if m.is_source_caps {
        for (i, pdo) in m.objects.iter().enumerate() {
            detail.push(detail_zh(&describe_pdo(i, *pdo)));
        }
    } else if m.is_request {
        if let Some(rdo) = m.objects.first() {
            // No Source_Capabilities context for a stateless decode; the
            // caller can re-decode with context later if needed.
            detail.push(detail_zh(&describe_rdo(*rdo, &[])));
        }
    } else if standard_name == "Vendor_Defined" {
        detail.extend(describe_vdm(&m.objects).iter().map(|line| detail_zh(line)));
        for (i, object) in m.objects.iter().enumerate() {
            detail.push(format!("数据对象 #{}：0x{object:08X}", i + 1));
        }
    } else {
        for (i, object) in m.objects.iter().enumerate() {
            detail.push(format!("数据对象 #{}：0x{object:08X}", i + 1));
        }
    }
    Ok(PdDecode {
        standard_name: standard_name.clone(),
        title_zh: message_name_zh(&standard_name).to_string(),
        summary_zh: format!(
            "{} · 消息 ID {} · PD {} · {} · {}",
            message_name_zh(&standard_name),
            m.msg_id,
            m.spec_rev,
            role_zh(m.power_role),
            role_zh(m.data_role)
        ),
        msg_id: m.msg_id,
        spec_rev: m.spec_rev.to_string(),
        power_role: role_zh(m.power_role).to_string(),
        data_role: role_zh(m.data_role).to_string(),
        object_count: m.num_objects,
        extended: m.ext.is_some(),
        detail,
        objects: m.objects.clone(),
        wire_hex: hex(wire),
    })
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact.strip_prefix("0x").unwrap_or(&compact);
    // Work on bytes: slicing the String by byte index can panic on a char
    // boundary when the input contains multi-byte characters.
    let bytes = compact.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err("hex string length must be even".into());
    }
    (0..bytes.len())
        .step_by(2)
        .map(|i| {
            let high = hex_digit(bytes[i]).ok_or_else(|| format!("invalid hex at byte {i}"))?;
            let low = hex_digit(bytes[i + 1]).ok_or_else(|| format!("invalid hex at byte {}", i + 1))?;
            Ok(high << 4 | low)
        })
        .collect()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ------------------------------------------------------------------ C ABI

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn cjson(result: Result<PdDecode, String>) -> *mut c_char {
    let payload = match result {
        Ok(decoded) => serde_json::json!({ "ok": true, "data": decoded }).to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    };
    match CString::new(payload) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn guarded(f: impl FnOnce() -> Result<PdDecode, String>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => cjson(result),
        Err(_) => cjson(Err("panic inside decoder".into())),
    }
}

/// Parse a USB PD wire frame given as raw bytes. Returns a JSON string you must
/// free with [`km003c_free`]: `{"ok":true,"data":{...}}` or
/// `{"ok":false,"error":"..."}`.
#[unsafe(no_mangle)]
pub extern "C" fn km003c_parse_pd_wire(data: *const u8, len: usize) -> *mut c_char {
    guarded(|| {
        if data.is_null() {
            return Err("null data pointer".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        parse_pd_wire(bytes)
    })
}

/// Parse a USB PD wire frame given as a hex string (spaces/`0x` allowed).
/// Returns a JSON string you must free with [`km003c_free`].
#[unsafe(no_mangle)]
pub extern "C" fn km003c_parse_pd_hex(hex_ptr: *const c_char) -> *mut c_char {
    guarded(|| {
        if hex_ptr.is_null() {
            return Err("null string pointer".into());
        }
        let input = unsafe { CStr::from_ptr(hex_ptr) }.to_string_lossy();
        let input = input.trim().strip_prefix("0x").unwrap_or(&input);
        let bytes = hex_decode(input)?;
        parse_pd_wire(&bytes)
    })
}

/// Free a string previously returned by this library.
#[unsafe(no_mangle)]
pub extern "C" fn km003c_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// Library version, e.g. `"km003c-parse 0.1.0"` (free with [`km003c_free`]).
#[unsafe(no_mangle)]
pub extern "C" fn km003c_version() -> *mut c_char {
    let version = format!("km003c-parse {}", env!("CARGO_PKG_VERSION"));
    match CString::new(version) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_source_capabilities() {
        // header: data message Source_Capabilities, 1 object, power role Src
        let mut wire = vec![0x01u8, 0x11];
        // one Fixed PDO: 5 V / 3 A
        wire.extend_from_slice(&0x0001_912Cu32.to_le_bytes());
        let decoded = parse_pd_wire(&wire).expect("decodes");
        assert_eq!(decoded.standard_name, "Source_Capabilities");
        assert_eq!(decoded.object_count, 1);
        assert_eq!(decoded.power_role, "供电端");
        assert!(decoded.detail.iter().any(|l| l.contains("固定档")));
        let json = decoded.to_json_string();
        assert!(json.contains("Source_Capabilities"));
        assert!(json.contains("供电"));
    }

    #[test]
    fn reports_bad_wire() {
        let err = parse_pd_wire(&[0x01]).unwrap_err();
        assert!(err.contains("header"));
    }

    #[test]
    fn c_abi_hex_roundtrip() {
        let json =
            unsafe { std::ffi::CString::from_raw(km003c_parse_pd_hex(c"01106421dcc0".as_ptr())) };
        let s = json.to_string_lossy();
        assert!(s.contains("\"ok\":true"));
        assert!(s.contains("Source_Capabilities"));
    }

    #[test]
    fn hex_decode_handles_prefix_and_spaces() {
        assert_eq!(hex_decode(" 0x01 02 03 ").unwrap(), vec![1, 2, 3]);
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn hex_decode_rejects_non_ascii_instead_of_panicking() {
        // Multi-byte characters previously made byte slicing panic on a char
        // boundary; both odd and even byte lengths must be plain errors.
        assert!(hex_decode("0中0102").is_err());
        assert!(hex_decode("01中02").is_err());
    }
}
