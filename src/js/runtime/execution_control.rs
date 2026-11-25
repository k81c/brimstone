//! Cooperative execution control for embedders.
//!
//! This module is part of the M0 embed API fork. It deliberately exposes only plain Rust types
//! (`Arc<AtomicU64>`) so that a host can stop a running script from another thread without ever
//! touching a `Context`, a `Handle`, or a `HeapPtr`.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::runtime::eval_result::{EvalError, EvalResult};

/// Default number of dispatched instructions between safepoint checks.
pub const DEFAULT_SAFEPOINT_INTERVAL: u64 = 2048;

/// Why an execution was terminated, from the engine's point of view.
///
/// The host is free to map this to its own `StopReason` type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminationReason {
    /// A new epoch or an explicit stop request was observed.
    Terminated,
    /// The absolute deadline was exceeded.
    TotalTimeout,
    /// The no-progress deadline was exceeded.
    IdleTimeout,
    /// The optional dispatch budget was exhausted.
    BudgetExceeded,
}

/// Shared, thread-safe control state for one query.
///
/// All fields are `Arc<AtomicU64>` so a clone can be kept by the supervisor thread while the
/// worker thread executes JS. `ExecutionControl` itself is not stored in the JavaScript heap.
#[derive(Clone, Debug)]
pub struct ExecutionControl {
    /// Stop requests write the epoch of the query they want to stop.
    stop_epoch: Arc<AtomicU64>,
    /// The epoch of the currently running query. A stop only takes effect for `epoch <= stop_epoch`.
    active_epoch: Arc<AtomicU64>,
    /// Deadline in millis from the process clock anchor; zero disables the check.
    total_deadline_ms: Arc<AtomicU64>,
    /// No-progress deadline in millis from the process clock anchor; zero disables the check.
    idle_deadline_ms: Arc<AtomicU64>,
    /// Remaining dispatch budget; `u64::MAX` disables the check.
    dispatch_budget: Arc<AtomicU64>,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self::new(0)
    }
}

impl ExecutionControl {
    /// Create control state for a query with the given (monotonically increasing) epoch.
    pub fn new(epoch: u64) -> Self {
        Self {
            // u64::MAX is the "no stop requested" sentinel.
            stop_epoch: Arc::new(AtomicU64::new(u64::MAX)),
            active_epoch: Arc::new(AtomicU64::new(epoch)),
            total_deadline_ms: Arc::new(AtomicU64::new(0)),
            idle_deadline_ms: Arc::new(AtomicU64::new(0)),
            dispatch_budget: Arc::new(AtomicU64::new(u64::MAX)),
        }
    }

    /// Ask the running query to stop. Stale epochs are ignored by `check`.
    pub fn stop(&self, epoch: u64) {
        self.stop_epoch.store(epoch, Ordering::Relaxed);
    }

    pub fn stop_epoch(&self) -> u64 {
        self.stop_epoch.load(Ordering::Relaxed)
    }

    pub fn active_epoch(&self) -> u64 {
        self.active_epoch.load(Ordering::Relaxed)
    }

    pub fn is_stopped(&self) -> bool {
        let stop = self.stop_epoch();
        stop != u64::MAX && stop >= self.active_epoch()
    }

    /// Set the absolute deadline, relative to now.
    pub fn set_total_deadline(&self, duration: Option<Duration>) {
        let deadline = match duration {
            Some(duration) => now_ms().saturating_add(duration.as_millis() as u64),
            None => 0,
        };
        self.total_deadline_ms.store(deadline, Ordering::Relaxed);
    }

    /// Set the no-progress deadline, relative to now.
    pub fn set_idle_deadline(&self, duration: Option<Duration>) {
        let deadline = match duration {
            Some(duration) => now_ms().saturating_add(duration.as_millis() as u64),
            None => 0,
        };
        self.idle_deadline_ms.store(deadline, Ordering::Relaxed);
    }

    /// Reset the no-progress deadline because progress was made.
    pub fn note_progress(&self, duration: Duration) {
        let deadline = now_ms().saturating_add(duration.as_millis() as u64);
        self.idle_deadline_ms.store(deadline, Ordering::Relaxed);
    }

    pub fn total_deadline_ms(&self) -> u64 {
        self.total_deadline_ms.load(Ordering::Relaxed)
    }

    pub fn idle_deadline_ms(&self) -> u64 {
        self.idle_deadline_ms.load(Ordering::Relaxed)
    }

    /// Set an optional dispatch budget. `None` means unlimited.
    pub fn set_dispatch_budget(&self, budget: Option<u64>) {
        self.dispatch_budget
            .store(budget.unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub fn dispatch_budget(&self) -> Option<u64> {
        match self.dispatch_budget.load(Ordering::Relaxed) {
            u64::MAX => None,
            value => Some(value),
        }
    }

    /// Consume `steps` units of the dispatch budget, returning false if it is exhausted.
    fn consume_dispatch_budget(&self, steps: u64) -> bool {
        self.dispatch_budget
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                if remaining == u64::MAX {
                    Some(u64::MAX)
                } else if remaining > steps {
                    Some(remaining - steps)
                } else {
                    Some(0)
                }
            })
            .map(|previous| previous == u64::MAX || previous > steps)
            .unwrap_or(false)
    }

    /// Check all stop conditions. Must be called from a safepoint.
    ///
    /// This returns `EvalError::Terminated`, which is intentionally not a JS value so that it
    /// cannot be caught by `try`/`catch`, and so that `finally` and iterator close are skipped.
    pub fn check(&self, steps: u64) -> EvalResult<()> {
        if self.is_stopped() {
            return Err(EvalError::Terminated);
        }

        let now = now_ms();
        let total = self.total_deadline_ms.load(Ordering::Relaxed);
        if total != 0 && now >= total {
            return Err(EvalError::Terminated);
        }

        let idle = self.idle_deadline_ms.load(Ordering::Relaxed);
        if idle != 0 && now >= idle {
            return Err(EvalError::Terminated);
        }

        if !self.consume_dispatch_budget(steps) {
            return Err(EvalError::Terminated);
        }

        Ok(())
    }

    /// Classify the termination from the shared state, for host-side reporting.
    pub fn termination_reason(&self) -> TerminationReason {
        if self.is_stopped() {
            return TerminationReason::Terminated;
        }

        let now = now_ms();
        let total = self.total_deadline_ms.load(Ordering::Relaxed);
        if total != 0 && now >= total {
            return TerminationReason::TotalTimeout;
        }

        let idle = self.idle_deadline_ms.load(Ordering::Relaxed);
        if idle != 0 && now >= idle {
            return TerminationReason::IdleTimeout;
        }

        if self.dispatch_budget.load(Ordering::Relaxed) == 0 {
            return TerminationReason::BudgetExceeded;
        }

        TerminationReason::Terminated
    }
}

/// Deadlines are relative to a process-wide monotonic anchor so that wall-clock adjustments
/// cannot shorten or extend a running query.
static CLOCK_ANCHOR: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    CLOCK_ANCHOR.get_or_init(Instant::now).elapsed().as_millis() as u64
}
