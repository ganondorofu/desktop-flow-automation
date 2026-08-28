use crate::Step;
use serde::{Deserialize, Serialize};

/// Top-level flow definition, parsed from `flow.yaml`.
///
/// Execution order is not the order of `steps` in this array — it is
/// derived by walking `connections` starting from `entry`. A step that
/// exists in `steps` but is unreachable from `entry` (no incoming
/// connection, or its whole chain got disconnected) simply never runs,
/// the same way an unwired block sits inert in Scratch or an
/// unconnected node never fires in n8n.
///
/// Layout (node positions) is intentionally kept out of this struct —
/// the frontend writes it as a sibling `layout:` key in the same YAML
/// file instead, which this struct's `Deserialize` silently ignores
/// (no `deny_unknown_fields`), so a step definition never picks up
/// noisy diffs just because the user dragged a node around the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub name: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// A pause applied between *every* step in the run (0 = off) —
    /// independent of any individual `wait` step, for slowing a whole
    /// flow down to watch it work or to give a target app breathing
    /// room between actions.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub step_delay_ms: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// A directed wire between two steps within the same container (the
/// top-level flow, an `if`'s `then`/`otherwise`, or a `loop`'s
/// `body`). `from_port` exists for a future step with more than one
/// unnamed output — nothing currently uses anything but `None`; `if`
/// used to (a `"yes"`/`"no"`-ported wire in the enclosing container),
/// before it became a nested container like `loop`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_port: Option<String>,
    pub to: String,
}

/// A self-contained pool of steps + wires — the same graph model as
/// `Flow`, just nested one level down. Used for a `loop`'s `body` and
/// an `if`'s `then`/`otherwise`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Branch {
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

#[derive(Debug)]
pub struct FlowParseError(pub String);

impl std::fmt::Display for FlowParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for FlowParseError {}

pub fn parse_flow(yaml: &str) -> Result<Flow, FlowParseError> {
    serde_yaml::from_str(yaml).map_err(|e| FlowParseError(e.to_string()))
}

pub fn to_yaml(flow: &Flow) -> Result<String, FlowParseError> {
    serde_yaml::to_string(flow).map_err(|e| FlowParseError(e.to_string()))
}
