//! Cooperative nesting guard for re-entrant native operations (M2 hardening).
//!
//! Parser, `JSON.parse` / `JSON.stringify`, RegExp compile/exec and `call_from_rust` all count
//! toward one per-context depth. Exceeding `Options::stack::max_native_depth` produces a
//! catchable `RangeError`, never `EvalError::Terminated`.

use crate::{
    eval_err,
    runtime::{Context, error::range_error_value, eval_result::EvalResult},
};

/// RAII guard incrementing the context's native call depth on creation and decrementing on drop.
pub struct NativeGuard {
    cx: Context,
}

impl NativeGuard {
    /// Enter a native operation. Returns a catchable `RangeError` if the depth budget is spent.
    pub fn new(cx: Context) -> EvalResult<Self> {
        let max = cx.options.stack.max_native_depth;
        let depth = cx.native_depth.get();
        if depth >= max {
            let error = range_error_value(cx, "Maximum native call depth exceeded")?;
            return eval_err!(error);
        }
        cx.native_depth.set(depth + 1);
        Ok(Self { cx })
    }
}

impl Drop for NativeGuard {
    fn drop(&mut self) {
        let depth = self.cx.native_depth.get();
        self.cx.native_depth.set(depth.saturating_sub(1));
    }
}
