use flow_schema::{Branch, KeyModifiers, MonitorPoint};
use std::collections::HashMap;

#[derive(Debug)]
pub struct FlowFailure {
    pub step_id: String,
    pub message: String,
}

/// Variables set by `Action::SetVariable` and read by `Action::If`
/// conditions, shared across the whole flow run including nested
/// `If`/`Loop` bodies.
#[derive(Default)]
pub struct ExecutionContext {
    pub variables: HashMap<String, String>,
    /// A pause applied after every step, independent of any
    /// individual `wait` step — set once per run from `Flow.step_delay_ms`.
    pub step_delay_ms: u64,
    /// The monitor layout `run_flow_with_backend` captured right
    /// before this run started — `run_branch` pauses if it ever sees
    /// something different partway through.
    pub initial_monitor_signature: String,
    /// The most recent successful `find_image` match's on-screen
    /// location, shared across the whole run the same way `variables`
    /// is — what a `Click`/`MoveMouse` step targeting `LastMatch`
    /// actually resolves to. `None` until the first successful match.
    pub last_match: Option<MonitorPoint>,
    /// Every `FunctionDef` step's `body`, keyed by `name` — collected
    /// once from `flow.steps` at the start of the run (see
    /// `run_flow_with_backend`), so `CallFunction` can look one up by
    /// name regardless of where in the flow the matching `FunctionDef`
    /// step sits.
    pub functions: HashMap<String, Branch>,
    /// Names of functions currently being called, innermost last —
    /// `CallFunction` checks this before recursing into a function's
    /// body, the same "don't run forever" guard `run_branch`'s own
    /// `visited` set gives a single branch's wiring, just for the
    /// call graph instead.
    pub call_stack: Vec<String>,
    /// Keys a `KeyPress` step with `mode: Press` left held, each with
    /// the modifiers it was pressed together with — `mode: Release`
    /// removes its match here (see `runner::run_action`). Whatever's
    /// still left when the run ends gets force-released — see
    /// `runner::run_flow_with_backend` — so a flow can never leave the
    /// real keyboard's modifier state stuck down.
    pub held_keys: Vec<(String, KeyModifiers)>,
}
