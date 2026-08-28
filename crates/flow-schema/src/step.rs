use crate::Action;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(flatten)]
    pub action: Action,
    #[serde(default)]
    pub retry: RetryPolicy,
    /// A disabled step is skipped entirely at run time (no execution,
    /// no observer events) but stays wired in place — a lighter-weight
    /// "mute this one step" affordance distinct from disconnecting it.
    /// Omitted from YAML when true so ordinary flows stay uncluttered.
    #[serde(default = "default_enabled", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Step-through debugging pauses the run just before this step
    /// executes (see `crates/engine`'s pause/resume handling). Omitted
    /// from YAML when false so ordinary flows stay uncluttered.
    #[serde(default, skip_serializing_if = "is_false")]
    pub breakpoint: bool,
}

fn default_enabled() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_fail(value: &FailureBehavior) -> bool {
    *value == FailureBehavior::Fail
}

/// What to do once a step has exhausted its `retry` policy and still
/// failed. `Skip` logs the failure (the observer still sees it) but
/// lets the flow carry on to the next step anyway — for a step whose
/// absence shouldn't sink the rest of the flow (an optional click, a
/// best-effort read).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FailureBehavior {
    #[default]
    Fail,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RetryPolicy {
    #[serde(default)]
    pub max_attempts: u32,
    #[serde(default)]
    pub interval_ms: u64,
    /// What happens once `max_attempts` is exhausted and the step has
    /// still failed. Omitted from YAML at the default (`Fail`) so
    /// ordinary flows stay uncluttered.
    #[serde(default, skip_serializing_if = "is_fail")]
    pub on_failure: FailureBehavior,
}
