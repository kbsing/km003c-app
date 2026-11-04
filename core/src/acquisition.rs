//! Acquisition layer: owns the polling loop, buffers samples, emits batches.
//!
//! Design constraints that drove this shape:
//!
//! 1. The poll loop runs on its own thread with its own timing. It is never
//!    driven by the UI. If a webview timer drove polling, webview scheduling
//!    jitter would land directly in the data -- which defeats the point of
//!    using the device-side timestamp.
//!
//! 2. Emission is decoupled from polling. We poll at 50 Hz but emit batches at
//!    ~12 Hz. Serializing 50 messages/s across an IPC boundary is wasteful and
//!    no display needs it.
//!
//! 3. The ring buffer is bounded. A long session must not grow memory without
//!    limit, on either side of the boundary.
//!
//! 4. Errors are classified, not just counted. A timeout wants a retry, a stall
//!    wants clear_halt, a disconnect must stop the loop. "10 errors" is not
//!    actionable; "10 empty responses" vs "10 stalls" is.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use crate::device::{Device, Poll, PollError, PollOutcome};
use crate::protocol::PdEvent;

/// One V/I sample for display.
///
/// Sourced from the 12-byte PD measurement block: ~50 Hz, 1 mV / 1 mA, with a
/// device-side millisecond timestamp. `temp_c` is carried from the most recent
/// ADC poll, which refreshes far more slowly (~2 Hz).
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Device monotonic milliseconds. Use this for the time axis, not host time.
    pub device_ms: u32,
    /// Host elapsed milliseconds since acquisition start. For diagnostics only.
    pub host_ms: u64,
    pub vbus_v: f64,
    pub ibus_a: f64,
    pub cc1_v: f64,
    pub cc2_v: f64,
    /// Last known temperature. None until the first ADC poll lands.
    pub temp_c: Option<f64>,
    /// WITRN only: D+/D- line voltage and accumulated Ah/Wh (None elsewhere).
    pub d_plus_v: Option<f64>,
    pub d_minus_v: Option<f64>,
    pub amp_hours: Option<f64>,
    pub watt_hours: Option<f64>,
}

impl Measurement {
    pub fn power_w(&self) -> f64 {
        self.vbus_v * self.ibus_a
    }
}

/// A decoded PD event with the host-relative time it was observed.
#[derive(Debug)]
pub struct TimedEvent {
    pub host_ms: u64,
    pub event: PdEvent,
}

/// Per-category counters. Sums to `polls`.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub polls: u64,
    pub ok: u64,
    /// Zero-length IN completions. Normal, not a fault.
    pub empty: u64,
    pub timeout: u64,
    pub stall: u64,
    pub fault: u64,
    pub os_error: u64,
    pub parse_error: u64,
    pub invalid_arg: u64,
    /// Device went away; the polling loop stops on this. Kept so `polls`
    /// equals `ok + empty + real_errors()` even at end of session.
    pub disconnected: u64,
    /// Samples dropped because the consumer wasn't keeping up.
    pub dropped: u64,
    pub events_seen: u64,
    pub largest_response: usize,
}

impl Stats {
    fn record(&mut self, e: &PollError) {
        match e {
            PollError::Timeout => self.timeout += 1,
            PollError::Stall => self.stall += 1,
            PollError::Disconnected => self.disconnected += 1,
            PollError::Fault => self.fault += 1,
            PollError::InvalidArgument => self.invalid_arg += 1,
            PollError::Os(_) => self.os_error += 1,
            PollError::Parse(_) => self.parse_error += 1,
        }
    }

    /// Everything that is neither a good sample nor a benign empty response.
    pub fn real_errors(&self) -> u64 {
        self.timeout
            + self.stall
            + self.fault
            + self.os_error
            + self.parse_error
            + self.invalid_arg
            + self.disconnected
    }
}

/// Bounded sample history. Oldest samples are evicted once full.
pub struct RingBuffer {
    buf: VecDeque<Measurement>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, m: Measurement) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(m);
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Measurement> {
        self.buf.iter()
    }

    pub fn latest(&self) -> Option<&Measurement> {
        self.buf.back()
    }
}

/// One emission from the acquisition thread.
pub struct Batch {
    pub measurements: Vec<Measurement>,
    pub events: Vec<TimedEvent>,
    pub stats: Stats,
}

/// Enqueue one UI batch without letting a slow consumer stall USB polling.
///
/// A full queue deliberately drops the newest batch. The next emitted batch
/// carries the updated counter, so the UI can distinguish a rendered trace
/// from a lossless one.
fn enqueue_batch(tx: &SyncSender<Batch>, batch: Batch, stats: &mut Stats) -> bool {
    match tx.try_send(batch) {
        Ok(()) => true,
        Err(TrySendError::Full(batch)) => {
            stats.dropped += batch.measurements.len() as u64;
            true
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

pub struct Config {
    /// Poll period. 20 ms matches the PD block's measured refresh rate; faster
    /// only burns transport for duplicate values.
    pub poll_period: Duration,
    /// How often to emit a batch to the consumer.
    pub emit_period: Duration,
    /// How often to poll ADC (for temperature and uV/uA). The ADC block only
    /// refreshes about every 500 ms, so polling it faster is pure waste. The
    /// chained response still carries a PD block, so this costs no V/I sample.
    pub adc_period: Duration,
    /// Consecutive unrecoverable-ish failures before giving up.
    pub max_consecutive_errors: u32,
    /// Maximum number of UI batches allowed to await consumption.
    pub batch_queue_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_period: Duration::from_millis(20),
            emit_period: Duration::from_millis(80),
            adc_period: Duration::from_millis(500),
            max_consecutive_errors: 50,
            batch_queue_capacity: 4,
        }
    }
}

/// Handle to a running acquisition thread.
pub struct Acquisition {
    pub rx: Receiver<Batch>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<Result<Stats, String>>>,
}

impl Acquisition {
    /// Take ownership of an open device and start polling.
    pub fn start(mut dev: Box<dyn Device>, cfg: Config) -> Self {
        // Keep at most a few 80 ms batches in flight. A slow IPC/UI consumer
        // must not create unbounded memory growth or slow the USB poller.
        let (tx, rx) = mpsc::sync_channel::<Batch>(cfg.batch_queue_capacity.max(1));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();

        let handle = thread::spawn(move || -> Result<Stats, String> {
            let start = Instant::now();
            let mut stats = Stats::default();
            let mut pending: Vec<Measurement> = Vec::new();
            let mut pending_events: Vec<TimedEvent> = Vec::new();
            let mut last_emit = Instant::now();
            let mut last_adc = Instant::now() - cfg.adc_period; // force one immediately
            let mut consecutive = 0u32;

            while !stop_thread.load(Ordering::Relaxed) {
                let want_adc = last_adc.elapsed() >= cfg.adc_period;
                stats.polls += 1;

                let result = if want_adc {
                    last_adc = Instant::now();
                    dev.poll_slow()
                } else {
                    dev.poll()
                };

                match result {
                    Ok(PollOutcome::Data(p)) => {
                        consecutive = 0;
                        stats.ok += 1;
                        stats.largest_response = stats.largest_response.max(p.raw_len);
                        let host_ms = start.elapsed().as_millis() as u64;

                        if let Some(s) = &p.sample {
                            pending.push(Measurement {
                                device_ms: s.device_ms,
                                host_ms,
                                vbus_v: s.vbus_v,
                                ibus_a: s.ibus_a,
                                cc1_v: s.cc1_v,
                                cc2_v: s.cc2_v,
                                temp_c: s.temp_c,
                                d_plus_v: s.d_plus_v,
                                d_minus_v: s.d_minus_v,
                                amp_hours: s.amp_hours,
                                watt_hours: s.watt_hours,
                            });
                        }
                        stats.events_seen += p.events.len() as u64;
                        for event in extract_events(p) {
                            pending_events.push(TimedEvent { host_ms, event });
                        }
                    }
                    Ok(PollOutcome::Empty) => {
                        consecutive = 0;
                        stats.empty += 1;
                    }
                    Err(e @ PollError::Disconnected) => {
                        stats.record(&e);
                        // Surface the final counters (including the disconnect)
                        // before the channel closes, so the consumer sees the
                        // real end-of-session stats rather than a stale clone.
                        let batch = Batch {
                            measurements: std::mem::take(&mut pending),
                            events: std::mem::take(&mut pending_events),
                            stats: stats.clone(),
                        };
                        let _ = enqueue_batch(&tx, batch, &mut stats);
                        return Err("device disconnected".to_string());
                    }
                    Err(e) => {
                        stats.record(&e);
                        consecutive += 1;
                        if matches!(e, PollError::Stall) {
                            let _ = dev.clear_stall();
                        }
                        if consecutive >= cfg.max_consecutive_errors {
                            return Err(format!(
                                "giving up after {consecutive} consecutive errors, last: {e}"
                            ));
                        }
                    }
                }

                if last_emit.elapsed() >= cfg.emit_period
                    && (!pending.is_empty() || !pending_events.is_empty())
                {
                    let batch = Batch {
                        measurements: std::mem::take(&mut pending),
                        events: std::mem::take(&mut pending_events),
                        stats: stats.clone(),
                    };
                    if !enqueue_batch(&tx, batch, &mut stats) {
                        // Consumer hung up; nothing left to feed.
                        return Ok(stats);
                    }
                    last_emit = Instant::now();
                }

                thread::sleep(cfg.poll_period);
            }

            let _ = dev.disconnect();
            Ok(stats)
        });

        Self {
            rx,
            stop,
            handle: Some(handle),
        }
    }

    /// Signal the thread to stop and wait for its final stats.
    pub fn stop(mut self) -> Result<Stats, String> {
        self.stop.store(true, Ordering::Relaxed);
        match self.handle.take() {
            Some(h) => h
                .join()
                .map_err(|_| "acquisition thread panicked".to_string())?,
            None => Err("already stopped".to_string()),
        }
    }
}

impl Drop for Acquisition {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn extract_events(p: Poll) -> Vec<PdEvent> {
    p.events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(samples: usize) -> Batch {
        Batch {
            measurements: (0..samples)
                .map(|_| Measurement {
                    device_ms: 0,
                    host_ms: 0,
                    vbus_v: 0.0,
                    ibus_a: 0.0,
                    cc1_v: 0.0,
                    cc2_v: 0.0,
                    temp_c: None,
                    d_plus_v: None,
                    d_minus_v: None,
                    amp_hours: None,
                    watt_hours: None,
                })
                .collect(),
            events: Vec::new(),
            stats: Stats::default(),
        }
    }

    #[test]
    fn full_batch_queue_counts_dropped_measurements() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let mut stats = Stats::default();

        assert!(enqueue_batch(&tx, batch(2), &mut stats));
        assert!(enqueue_batch(&tx, batch(3), &mut stats));
        assert_eq!(stats.dropped, 3);
    }
}
