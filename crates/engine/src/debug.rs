//! Process-wide run-control state: force-stop and step-through
//! debugging (breakpoints / "Step" vs "Run"). Both are single atomics
//! rather than anything per-run, because only one flow ever runs at a
//! time from this app — see each item's doc comment for why that also
//! matters for testing (`tests.rs`'s closing comment explains why
//! neither is covered by an automated test that pokes these directly).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

/// Set by the "force stop" command (the UI's Escape-while-running
/// shortcut, or the Stop button) and polled between steps — and during
/// any `sleep_interruptible` wait — so a running flow can actually be
/// cut off instead of running to its natural end regardless of what
/// the user wants. Process-wide rather than per-run because only one
/// flow ever runs at a time from this app.
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Call before starting a new run, so a stop left over from a
/// previous one (already consumed) doesn't kill this one instantly.
pub fn clear_stop() {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn request_stop() {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn is_stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

/// True while the run should pause before *every* step, not just ones
/// with `Step.breakpoint` set — what "Step" (rather than "Run") starts
/// the flow in, and what a "step once" resume switches back into right
/// after running the one step it was asked for (see `RESUME`'s `STEP`
/// case in `run_branch`), so the run re-pauses at the very next step
/// too. A plain "Continue" resume clears it, letting the flow run
/// freely again until the next `Step.breakpoint` (if any).
pub(crate) static STEP_MODE: AtomicBool = AtomicBool::new(false);

/// What a paused run should do next, set by the debug commands and
/// consumed once by the pause loop in `run_branch`. `NONE` means
/// "still waiting" — the pause loop keeps polling.
pub(crate) const RESUME_NONE: u8 = 0;
pub(crate) const RESUME_STEP: u8 = 1;
pub(crate) const RESUME_CONTINUE: u8 = 2;
pub(crate) static RESUME: AtomicU8 = AtomicU8::new(RESUME_NONE);

/// Call before starting a new run so `STEP_MODE`/`RESUME` left over
/// from a previous run don't affect this one. `step_mode` is whether
/// this run should start paused before its very first step (the
/// "Step" command) rather than running freely until the first
/// `Step.breakpoint` (the "Run" command).
pub fn reset_debug_state(step_mode: bool) {
    STEP_MODE.store(step_mode, Ordering::SeqCst);
    RESUME.store(RESUME_NONE, Ordering::SeqCst);
}

/// Advances a paused run by exactly one step, then re-pauses before
/// the step after that.
pub fn request_step() {
    RESUME.store(RESUME_STEP, Ordering::SeqCst);
}

/// Resumes a paused run, letting it run freely until the next
/// `Step.breakpoint` or the flow ends.
pub fn request_continue() {
    RESUME.store(RESUME_CONTINUE, Ordering::SeqCst);
}

/// Whether a container's walk should keep going after the step that
/// just ran, and if not, why — `run_branch`'s own step-walking loop
/// treats every non-`Continue` value identically (stop walking this
/// container's chain, propagate the signal to the caller); it's each
/// *caller* (`Loop`'s iteration loop, `CallFunction`'s call site, or
/// `run_flow_with_backend` at the very top) that decides what a given
/// signal actually means for it — see each variant's doc comment.
#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum Signal {
    Continue,
    /// From `Action::Stop` — unwinds every enclosing `run_branch`
    /// call (including any `Loop` iterations or `CallFunction` calls
    /// in progress) all the way back to `run_flow_with_backend`,
    /// which then reports a normal success: ending early on purpose
    /// is not a failure. Unlike `BreakLoop`/`ContinueLoop`/`Return`,
    /// nothing along the way absorbs a `Stop` — it always means "end
    /// the whole run", regardless of what loops or function calls are
    /// currently in progress.
    Stop,
    /// From `Action::Break` — absorbed by the nearest enclosing
    /// `Loop`, which ends that loop (skipping any remaining
    /// iterations) and lets its own chain continue normally
    /// afterward. Also absorbed at a `CallFunction`/`FunctionDef`
    /// boundary (ending that function call early, same as `Return`)
    /// or at the top level of the flow (ending the run, same as
    /// `Stop`) if it reaches either without an enclosing `Loop` in
    /// between — lexically, there's nothing else for it to mean.
    BreakLoop,
    /// From `Action::Continue` — absorbed by the nearest enclosing
    /// `Loop`, which skips straight to its next iteration. Absorbed
    /// the same way `BreakLoop` is at a function/top-level boundary
    /// with no enclosing `Loop` — there, it's simply a no-op.
    ContinueLoop,
    /// From `Action::Return` — absorbed by the nearest enclosing
    /// `CallFunction` call, which ends that function call early and
    /// lets the caller's chain continue normally right after it.
    /// Passes straight through any `Loop` boundary in between (a
    /// `return` inside a loop inside a function still returns from
    /// the function, not just breaks the loop). Absorbed at the top
    /// level of the flow (ending the run, same as `Stop`) if it
    /// reaches there with no enclosing function call.
    Return,
}

/// Sleeps `duration`, but in small chunks that each check
/// `STOP_REQUESTED` — so a force-stop during a long `wait` step, retry
/// interval, or the flow-wide inter-step delay takes effect within
/// `POLL_INTERVAL` instead of only once the sleep finishes on its own.
pub(crate) fn sleep_interruptible(duration: Duration) -> Signal {
    const POLL_INTERVAL: Duration = Duration::from_millis(50);
    let mut remaining = duration;
    loop {
        if is_stop_requested() {
            return Signal::Stop;
        }
        if remaining.is_zero() {
            return Signal::Continue;
        }
        let chunk = remaining.min(POLL_INTERVAL);
        std::thread::sleep(chunk);
        remaining -= chunk;
    }
}
