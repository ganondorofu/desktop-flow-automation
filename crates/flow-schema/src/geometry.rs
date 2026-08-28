use serde::{Deserialize, Serialize};

/// A point expressed as "which monitor" + "logical (DPI-independent)
/// pixel offset within that monitor", per the coordinate design in
/// docs/architecture.md. Never store raw physical/virtual-screen
/// pixels directly — DPI and monitor-layout changes would silently
/// invalidate them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorPoint {
    pub monitor_id: String,
    pub x: i32,
    pub y: i32,
}

/// A rectangle in logical (DPI-independent) coordinates, relative to
/// the top-left of the captured virtual-desktop screenshot it was
/// picked from (so it can cover any connected monitor, not just the
/// primary one) — `find_text_ocr`'s optional scan region, so OCR can
/// be limited to part of the screen instead of always reading the
/// whole thing (faster, and avoids false-positive matches from
/// unrelated text elsewhere on screen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Where a `find_image` step's match actually was, once found — the
/// center of the matched region, in the same logical (DPI-independent)
/// coordinates as `MonitorPoint`, plus the match's own confidence
/// score. What a `Click`/`MoveMouse` step targeting `LastMatch`
/// actually resolves to.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageMatch {
    pub point: MonitorPoint,
    pub score: f64,
}
