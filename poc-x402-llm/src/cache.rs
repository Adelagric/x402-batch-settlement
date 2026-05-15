//! Tiny in-memory TTL set, keyed by payer address. Used by the
//! `cached` verify mode: a payer whose on-chain verification succeeded
//! is trusted for a short TTL without re-hitting the facilitator.
//!
//! Risk this encodes (intentional, measured): within the TTL a payer's
//! on-chain solvency could change, or a structurally-valid but
//! underfunded payment from the same payer would be accepted. Bounded
//! by the TTL; only acceptable for micro-amounts. A production system
//! would pair this with per-payer spend caps.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct TtlCache {
    inner: Mutex<HashMap<String, Instant>>,
}

impl TtlCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if `key` was recorded within `ttl`. Read-only.
    pub fn fresh(&self, key: &str, ttl: Duration) -> bool {
        let now = Instant::now();
        let map = self.inner.lock().unwrap();
        matches!(map.get(key), Some(&t) if now.duration_since(t) < ttl)
    }

    /// Record `key` as verified now. Opportunistically prunes.
    pub fn record(&self, key: &str, ttl: Duration) {
        let now = Instant::now();
        let mut map = self.inner.lock().unwrap();
        map.insert(key.to_string(), now);
        if map.len() > 4096 {
            map.retain(|_, &mut t| now.duration_since(t) < ttl);
        }
    }
}
