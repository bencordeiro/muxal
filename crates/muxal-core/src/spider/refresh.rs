//! Event-driven inventory refresh scheduling; callers supply a monotonic clock.
//! Ordinary file writes never schedule a scan. Bursts of membership changes do.
use std::time::Duration;

#[derive(Default)]
pub struct InventoryRefresh {
    first: Option<Duration>,
    latest: Duration,
    last_start: Option<Duration>,
    scanning: bool,
    inflight_changes: bool,
    failed: bool,
}
impl InventoryRefresh {
    pub fn changed(&mut self, now: Duration) {
        self.first.get_or_insert(now);
        self.latest = now;
    }
    pub fn pending(&self) -> bool {
        self.first.is_some() || self.inflight_changes
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn due(&self, now: Duration) -> bool {
        !self.scanning
            && !self.failed
            && self.first.is_some_and(|first| {
                now.saturating_sub(self.latest) >= Duration::from_secs(1)
                    || now.saturating_sub(first) >= Duration::from_secs(5)
            })
            && self
                .last_start
                .is_none_or(|at| now.saturating_sub(at) >= Duration::from_secs(5))
    }
    /// Called when a shared scan actually starts, including a manual Refresh.
    pub fn begin(&mut self, now: Duration) {
        self.inflight_changes = self.pending();
        self.first = None;
        self.scanning = true;
        self.failed = false;
        self.last_start = Some(now);
    }
    /// Changes received during a scan remain pending for a later scan. A failed
    /// scan waits for explicit retry instead of looping on an unavailable path.
    pub fn finish(&mut self, success: bool, now: Duration) {
        if !self.scanning {
            return;
        }
        self.scanning = false;
        self.inflight_changes = false;
        if !success {
            self.changed(now);
            self.failed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InventoryRefresh;
    use std::time::Duration;
    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }
    #[test]
    fn membership_bursts_coalesce_and_ordinary_ticks_do_nothing() {
        let mut refresh = InventoryRefresh::default();
        assert!(!refresh.due(at(100_000)));
        for time in [100, 200, 800] {
            refresh.changed(at(time));
        }
        assert!(!refresh.due(at(1799)));
        assert!(refresh.due(at(1800)));
        refresh.begin(at(1800));
        assert!(!refresh.due(at(10_000)));
        refresh.finish(true, at(2100));
        assert!(!refresh.pending());
        assert!(!refresh.due(at(100_000)));
    }
    #[test]
    fn sustained_changes_have_bounded_latency_and_a_minimum_scan_interval() {
        let mut refresh = InventoryRefresh::default();
        for time in (0..=5000).step_by(100) {
            refresh.changed(at(time));
        }
        assert!(refresh.due(at(5000)));
        refresh.begin(at(5000));
        refresh.changed(at(5100));
        refresh.finish(true, at(5200));
        assert!(refresh.pending());
        assert!(!refresh.due(at(9999)));
        assert!(refresh.due(at(10_000)));
        refresh.begin(at(10_000));
        refresh.finish(true, at(10_100));
        assert!(!refresh.pending());
    }
    #[test]
    fn failure_requires_manual_retry_and_reset_discards_pending_work() {
        let mut refresh = InventoryRefresh::default();
        refresh.changed(at(0));
        refresh.begin(at(1000));
        refresh.finish(false, at(2000));
        refresh.changed(at(3000));
        assert!(refresh.failed());
        assert!(!refresh.due(at(100_000)));
        refresh.begin(at(100_000));
        refresh.finish(true, at(100_100));
        assert!(!refresh.pending());
        assert!(!refresh.failed());
        refresh.changed(at(101_000));
        refresh = InventoryRefresh::default();
        assert!(!refresh.due(at(200_000)));
    }
}
