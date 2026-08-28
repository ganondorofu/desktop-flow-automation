//! Graph-walking flow executor. A `flow_schema::Flow` (and each `loop`
//! body nested inside it) is a pool of steps plus explicit
//! `connections` between them — execution starts at `entry` and
//! follows the wire leaving each step to find the next one. A step
//! with no incoming wire (unreachable from `entry`) simply never runs,
//! the same way an unwired Scratch block or an unconnected n8n node
//! sits inert — there is no special-cased "disabled" state needed for
//! that; `Step.enabled` is a separate, lighter-weight per-step mute.
//!
//! `if` is not a nested container: its two outputs (`"yes"`/`"no"`)
//! are just connections in the *same* pool, so both paths are free to
//! reconverge on the same downstream step — see the `port` dispatch
//! in `runner::run_branch`.
//!
//! The backend is injected rather than called directly so tests can
//! run the full retry/branch/loop logic without moving the real mouse
//! or typing into whatever window happens to have focus — see
//! `MockBackend` in `tests.rs`.
//!
//! Module map: `debug` is process-wide run-control state (force-stop,
//! step-through/breakpoints); `observer` is the `ExecutionObserver`
//! callback trait; `backend` is the `AutomationBackend` trait plus its
//! real Windows implementation; `context` is per-run state
//! (`ExecutionContext`); `runner` is the actual graph walk.

mod backend;
mod context;
mod debug;
mod observer;
mod runner;

pub use backend::{AutomationBackend, WindowsBackend};
pub use context::{ExecutionContext, FlowFailure};
pub use debug::{clear_stop, is_stop_requested, request_continue, request_step, request_stop, reset_debug_state};
pub use observer::{ExecutionObserver, NullObserver, StepFailure, StepOutcome};
pub use runner::{run_flow, run_flow_with_backend};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
