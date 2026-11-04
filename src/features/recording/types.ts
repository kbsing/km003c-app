export interface RecordingOptions {
  current_threshold_ma: number
  hold_seconds: number
  stop_when_condition_met: boolean
  use_system_time: boolean
  edge_save: boolean
  max_buffer_mb: number
}

export interface RecordingStatus {
  active: boolean
  file_path: string
  started_unix_ms: number | null
  samples: number
  events: number
  bytes_written: number
  buffered_bytes: number
  max_buffer_bytes: number
  stop_reason: string | null
}

export const defaultRecordingOptions: RecordingOptions = {
  current_threshold_ma: 10,
  hold_seconds: 10,
  stop_when_condition_met: false,
  use_system_time: false,
  // Default on so every batch is flushed to disk — an unexpected app close
  // keeps the data recoverable on the next launch.
  edge_save: true,
  max_buffer_mb: 100,
}
