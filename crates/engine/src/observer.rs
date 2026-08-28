use flow_schema::Step;
use std::collections::HashMap;

#[derive(Debug)]
pub struct StepFailure {
    pub step_id: String,
    pub message: String,
}

pub enum StepOutcome {
    Success,
    Failed(StepFailure),
}

/// Lets a caller (e.g. the Tauri command layer) observe per-step
/// progress without the engine depending on any UI framework.
pub trait ExecutionObserver {
    fn on_step_start(&mut self, _step: &Step) {}
    fn on_step_result(&mut self, _step: &Step, _outcome: &StepOutcome) {}
    /// The monitor layout changed mid-run (a monitor unplugged,
    /// reconnected, resized, or moved) — every coordinate the flow has
    /// is now potentially pointing at the wrong place, so the run is
    /// paused rather than clicking blind. Fires once when the pause
    /// begins; `on_monitor_restored` fires once it ends (the original
    /// layout came back) or the run is stopped instead.
    fn on_monitor_mismatch(&mut self) {}
    fn on_monitor_restored(&mut self) {}
    /// The run has paused just before `step` — either step-through
    /// mode is on, or `step.breakpoint` is set. Fires once when the
    /// pause begins; `on_resumed` fires once `request_step`/
    /// `request_continue` lets it proceed, or the run is stopped
    /// instead.
    fn on_paused(&mut self, _step: &Step) {}
    fn on_resumed(&mut self) {}
    /// Fires after every step (whether it succeeded or failed) with
    /// the run's complete current variable snapshot — the UI's
    /// "variables panel" runtime trace lives entirely off this, one
    /// full snapshot at a time rather than individual diffs, since
    /// the flat variable count in a typical flow is small enough that
    /// resending the whole map is simpler and cheap.
    fn on_variables_changed(&mut self, _variables: &HashMap<String, String>) {}
}

pub struct NullObserver;
impl ExecutionObserver for NullObserver {}
