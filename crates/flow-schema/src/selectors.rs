use crate::MonitorPoint;
use serde::{Deserialize, Serialize};

/// `Click` never carries its own position — positioning is entirely
/// `MoveMouse`'s job (a fixed coordinate, or wherever the last
/// `find_image` match was, via `PointTarget::LastMatch`). A flow that
/// wants to click somewhere specific is a `MoveMouse` step followed
/// by a plain `Cursor` click, not one step doing both — one thing to
/// learn ("this positions the cursor", "this presses a button")
/// instead of every coordinate-shaped action re-implementing its own
/// notion of "where".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickTarget {
    /// Clicks wherever the cursor already is.
    Cursor,
    /// Locates and clicks a UI element directly (via UI Automation),
    /// which doesn't involve moving the physical cursor at all —
    /// unlike a coordinate, "where an element is" isn't something a
    /// separate `MoveMouse` step could stand in for.
    Element(ElementSelector),
}

/// Where a `MoveMouse` step should move the cursor to — a fixed
/// coordinate, or `LastMatch` (see `ClickTarget::LastMatch`). Doesn't
/// support targeting a UI element directly the way `ClickTarget`
/// does — moving to "wherever an element is" without clicking it
/// isn't a need this project has hit yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointTarget {
    Coordinate(MonitorPoint),
    LastMatch,
}

/// Locates a UI Automation element instead of a raw coordinate, so the
/// click survives window moves, DPI changes, and (for standard
/// Windows controls) resizes. At least one of `automation_id` / `name`
/// / `control_type` should be set; `window_title` scopes the search to
/// one top-level window instead of the whole desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ElementSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_type: Option<String>,
}

/// How to find an element in the active browser tab — plain CSS is
/// the common case (and the only thing the on-page picker produces),
/// but a page redesign can change class names out from under a CSS
/// selector while the element's own visible text or a semantic
/// attribute (`placeholder`, `aria-label`, `name`, ...) stays put, so
/// matching by those is offered as an alternative, the way PAD's own
/// element descriptor lets you match on more than one property.
///
/// A bare YAML string (`selector: "#submit"`) still deserializes
/// straight into `Css` — every flow saved before this existed keeps
/// working unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BrowserSelector {
    Css(String),
    Other(BrowserSelectorSpec),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserSelectorSpec {
    /// Matches the element whose own trimmed visible text equals
    /// `value` exactly.
    Text { value: String },
    /// Matches the first element whose `name` attribute equals
    /// `value` (e.g. `name: "placeholder", value: "Search"`).
    Attribute { name: String, value: String },
}

impl From<&str> for BrowserSelector {
    fn from(value: &str) -> Self {
        BrowserSelector::Css(value.to_string())
    }
}

impl From<String> for BrowserSelector {
    fn from(value: String) -> Self {
        BrowserSelector::Css(value)
    }
}

/// Where a `find_image` step's reference image actually comes from.
///
/// A bare YAML string (`image: "target.png"`) still deserializes
/// straight into `Path` — every flow saved before embedding existed
/// keeps working unchanged, and referencing a file on disk is still
/// useful when the user would rather keep large images out of the
/// flow file and manage them as ordinary files instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageSource {
    Path(String),
    /// The reference image's raw bytes (any format the `image` crate
    /// reads — PNG is what the built-in region capture produces),
    /// base64-encoded directly into the flow file. Makes the `.relay`
    /// file self-contained: copy or send just the one file and the
    /// reference image comes with it, no separate asset to keep track
    /// of or lose.
    Embedded { data: String },
}

impl From<&str> for ImageSource {
    fn from(value: &str) -> Self {
        ImageSource::Path(value.to_string())
    }
}

impl From<String> for ImageSource {
    fn from(value: String) -> Self {
        ImageSource::Path(value)
    }
}
