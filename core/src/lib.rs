//! KM003C protocol, transport, and acquisition.
//!
//! Everything here runs at auth level 0: no AES, no MemoryRead, no AdcQueue.
//! Some decoder paths are hex-validated only and still need live-traffic
//! coverage; conclusions marked "measured" below come from a real KM003C.
//!
//! Layering:
//!   protocol    -- pure parsing/encoding, no I/O, unit-testable
//!   device      -- `Device` trait + `list_devices`/`open_device`; transport
//!                  backends (KM003C bulk, WITRN HID, Power-Pico CDC) behind
//!                  one interface
//!   acquisition -- background polling thread, bounded history, batched output
//!
//! Measured facts that shaped this design:
//!   - The PD 12-byte measurement block refreshes per poll (~50 Hz achievable).
//!   - The ADC block only refreshes about every 500 ms (~2 Hz), so V/I for a
//!     live graph must come from the PD block, not the ADC block.
//!   - Both are reachable without authentication.
//!
//! The standard USB PD decoding in `protocol` is device-agnostic: every backend
//! feeds it the same `PdEvent`/wire-byte stream.

pub mod acquisition;
pub mod device;
pub mod powerpico;
pub mod protocol;
pub mod witrn;
