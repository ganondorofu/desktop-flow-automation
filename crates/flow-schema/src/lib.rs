//! The `.relay` flow file format: `Flow` (a pool of `Step`s + explicit
//! `Connection`s between them, walked from `entry`) and every `Action`
//! a step can run. Parsing/serializing lives here (`parse_flow`,
//! `to_yaml`); actually running a flow is `crates/engine`'s job — this
//! crate only defines the shape of the data.

mod action;
mod flow;
mod geometry;
mod selectors;
mod step;

pub use action::{
    Action, CalcOp, ClickKind, Condition, DateTimeFormat, HttpMethod, KeyModifiers, KeyPressMode, MatchMode, MouseButton, PowerMode,
    TextOp,
};
pub use flow::{parse_flow, to_yaml, Branch, Connection, Flow, FlowParseError};
pub use geometry::{CaptureRegion, ImageMatch, MonitorPoint};
pub use selectors::{BrowserSelector, BrowserSelectorSpec, ClickTarget, ElementSelector, ImageSource, PointTarget, WindowSelector, WindowSelectorSpec};
pub use step::{FailureBehavior, RetryPolicy, Step};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
