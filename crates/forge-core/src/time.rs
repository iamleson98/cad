//! Cross-platform time (W-10).
//!
//! `std::time::Instant` and `std::time::SystemTime` panic on
//! `wasm32-unknown-unknown` ("time not implemented on this platform").
//! The `web-time` crate re-exports the whole `std::time` API with the
//! two clock types replaced by `performance.now()`-backed equivalents
//! on wasm and plain `std` types on native, so every crate in the
//! workspace should take `Instant`/`SystemTime` from here — a drop-in
//! import swap, no call-site changes.
//!
//! `Duration` is pure arithmetic and works everywhere; it is re-exported
//! for one-stop imports.

pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
mod tests {
    use super::Instant;

    #[test]
    fn instant_advances() {
        let a = Instant::now();
        let b = Instant::now();
        assert!(b >= a, "monotonic clock must not run backwards");
    }
}
