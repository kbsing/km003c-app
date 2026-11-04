// Mirrors the DTOs in src-tauri/src/main.rs. Keep the two in sync.
//
// All interpretation (PD message names, PDO/RDO decoding, extended-message
// bodies, attach/detach codes) happens in Rust. The frontend only renders
// what it is given -- it never parses protocol bytes itself.

export interface Measurement {
  /** Device monotonic milliseconds. Use this for the time axis. */
  device_ms: number
  /** Host elapsed ms since acquisition start. Diagnostics only. */
  host_ms: number
  vbus_v: number
  /** Signed instrument reading. UI charts the magnitude and exposes the sign as direction. */
  ibus_a: number
  cc1_v: number
  cc2_v: number
  /** Carried from the most recent ADC poll (~2 Hz). Null until the first lands. */
  temp_c: number | null
  /** WITRN only (null elsewhere): D+/D- line voltage and accumulated Ah/Wh. */
  d_plus_v: number | null
  d_minus_v: number | null
  amp_hours: number | null
  watt_hours: number | null
}

export type EventKind =
  | 'attach'
  | 'detach'
  | 'conn-unknown'
  | 'device-event'
  | 'message'
  | 'unparsed'

export interface PdEvent {
  kind: EventKind
  host_ms: number
  device_ms: number
  /** One-line summary, already formatted by Rust. */
  summary: string
  /** Decoded detail lines: PDO list, RDO, PPS status, or raw objects. */
  detail: string[]
  /** Complete record including the 6-byte vendor wrapper. Ground truth. */
  raw_hex: string
  /** PD wire bytes only (what the USB PD spec describes). Empty for non-messages. */
  wire_hex: string
  sop: number | null
  message: PdMessageView | null
}

export interface PdMessageView {
  title: string
  standard_name: string
  msg_id: number
  spec_rev: string
  power_role: string
  data_role: string
  object_count: number
  extended: boolean
}

export interface PdRequest {
  pdo_position: number
  kind: 'fixed' | 'pps' | 'other' | 'unknown'
  voltage_v: number | null
  current_a: number | null
}

export interface PdContract {
  phase: 'idle' | 'offered' | 'request-pending' | 'accepted-waiting-ps-rdy' | 'control-pending' | 'control-accepted' | 'soft-reset'
  visible_request: PdRequest | null
  active_contract: PdRequest | null
}

export interface PowerProfile {
  index: number
  kind: 'fixed' | 'pps' | 'battery' | 'variable' | 'apdo'
  min_v: number | null
  max_v: number
  max_a: number | null
  max_w: number | null
  selected: boolean
}

export interface Stats {
  polls: number
  ok: number
  /** Zero-length IN completions. Documented as a normal ack, not a fault. */
  empty: number
  timeout: number
  stall: number
  fault: number
  os_error: number
  parse_error: number
  invalid_arg: number
  /** Device went away; the polling loop stops on this. */
  disconnected: number
  /** Samples dropped because the consumer fell behind. Backpressure signal. */
  dropped: number
  events_seen: number
  largest_response: number
  /** Sum of everything that is neither ok nor empty. */
  real_errors: number
}

export interface Batch {
  measurements: Measurement[]
  events: PdEvent[]
  /** Current charger capabilities from the backend negotiation snapshot. */
  power_profiles: PowerProfile[]
  contract: PdContract
  stats: Stats
}

/** Returned after the Rust backend has opened the device and started polling. */
export interface StartResult {
  product: string
  serial: string
  /** Brand label: "KM003C" | "WITRN" | "Power-Pico". */
  kind: string
}

/** A discovered, connectable instrument for the device picker. */
export interface DeviceInfo {
  /** Passed back to `start_acquisition` as `device_id`. */
  id: string
  name: string
  serial: string
  /** Brand label, e.g. "KM003C" or "WITRN". */
  kind: string
}

/** UI preference type (not a Rust DTO). Colors for the live chart curves. */
export interface CurveColors {
  voltage: string
  current: string
  power: string
  temperature: string
}

/** Per-metric min/max/avg over the current session's full sample history. */
export interface MetricRange {
  min: number
  max: number
  avg: number
}

/** UI-only session statistics (not a Rust DTO). */
export interface SessionStats {
  vbus: MetricRange
  ibus: MetricRange
  power: MetricRange
  count: number
}

/** Point currently under the cursor, reported by the chart to its parent
 *  (UI only). `span` is the visible x-axis range in seconds, used to pick a
 *  compact time format for the hover readout. */
export interface ChartHover {
  t: number
  v: number | null
  a: number | null
  temp: number | null
  span: number
  /** Pointer coordinates relative to the chart viewport (UI only). */
  x: number
  y: number
}

export interface ChartRange {
  start: number
  end: number
  following: boolean
}

export interface ChartEventMarker {
  t: number
  label: string
  kind: EventKind
}

export interface ChartSelectionStats {
  duration_s: number
  count: number
  vbus: MetricRange
  ibus: MetricRange
  power: MetricRange
  capacity_mah: number
  energy_wh: number
}

/** LAN browser viewer status (mirrors RemoteStatusDto in src-tauri). */
export interface RemoteStatus {
  running: boolean
  /** http://<lan-ip>:<port>/ — empty while stopped. */
  url: string
}

/** Restored previous session, loaded from the last recording file (mirrors
 *  LastSessionDto in src-tauri). Lets the app recover data after a crash. */
export interface LastSessionResult {
  found: boolean
  file_path: string
  started_unix_ms: number | null
  measurements: Measurement[]
  events: PdEvent[]
  stats: Stats | null
}
