use crate::{Branch, BrowserSelector, CaptureRegion, ClickTarget, ElementSelector, ImageSource, PointTarget, WindowSelector};
use serde::{Deserialize, Serialize};

fn default_wait_for_window_timeout_ms() -> u32 {
    10_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// A pure marker with no side effect of its own — the canvas gives
    /// every flow a visible, explicit starting block instead of
    /// leaving "where does this begin?" as an invisible property of
    /// `entry`. What actually runs first is still whatever `entry`
    /// points to; by convention that's this step, but nothing enforces
    /// it structurally (deleting it, or repointing `entry` elsewhere,
    /// is still valid — a flow simply isn't required to start at a
    /// `Start` step, only conventionally does).
    Start,
    /// A pure marker, like `Start` — placed at most once, at the top
    /// level of a flow (the frontend enforces both; nothing here
    /// stops a hand-edited file from breaking either rule, but the
    /// engine treats "first one found in `Flow.steps`" as the answer
    /// either way rather than erroring on it). When the flow's main
    /// run fails with an uncaught error, `runner::run_flow_with_backend`
    /// looks for this step and, if found, runs whatever its own plain
    /// output is wired to instead of ending the run as failed —
    /// `caught_error`/`failed_step_id` are set in the flow's variables
    /// first, the same way `TryCatch`'s `catch` branch sets
    /// `caught_error`. Never triggered by a manual Stop (that's
    /// `Signal::Stop`, an `Ok` outcome, not a `FlowFailure`) — only a
    /// real step failure that would otherwise end the run.
    ErrorHandler,
    Click {
        target: ClickTarget,
        #[serde(default)]
        button: MouseButton,
        #[serde(default)]
        click_kind: ClickKind,
    },
    /// Moves the cursor without clicking — either instantly
    /// (`duration_ms: 0`) or smoothly interpolated over `duration_ms`
    /// to simulate a human-speed drag/hover.
    MoveMouse {
        target: PointTarget,
        #[serde(default)]
        duration_ms: u32,
    },
    TypeText {
        text: String,
    },
    /// Presses, holds down, or releases a named key (see
    /// `automation::key_from_name` for the supported names — function
    /// keys, arrows, Enter/Tab/Esc, modifiers, etc) — see
    /// `KeyPressMode`'s doc comment for what `mode` changes.
    /// `modifiers` are held alongside `key` for `Tap`/`Press` — e.g.
    /// `key: "a"`, `mode: Tap`, `modifiers.ctrl: true` sends the
    /// Ctrl+A combo in one step, rather than needing three separate
    /// `KeyPress` steps to hold Ctrl, tap A, then release Ctrl.
    KeyPress {
        key: String,
        #[serde(default)]
        mode: KeyPressMode,
        #[serde(default)]
        modifiers: KeyModifiers,
    },
    Wait {
        seconds: f64,
    },
    SetVariable {
        name: String,
        value: String,
    },
    /// Arithmetic and rounding on two operands (each resolved through
    /// `%variable%` substitution first, then parsed as a number),
    /// storing the result into `variable` — e.g. a running total,
    /// turning `%price%` and `%quantity%` into a subtotal, or rounding
    /// `%price%` to 2 decimal places. Division by zero fails the step
    /// rather than producing `inf`/`NaN`, since a silently-broken
    /// number is worse than a visible failure.
    Calculate {
        a: String,
        op: CalcOp,
        b: String,
        variable: String,
    },
    /// A nested-container branch, same shape as `Loop`: `then`/
    /// `otherwise` are each a self-contained pool of steps with their
    /// own `entry`, evaluated in isolation. Whichever one actually runs
    /// (based on `condition`), execution falls back out to this step's
    /// own single plain output once that branch's chain ends — there's
    /// no separate `"yes"`/`"no"`-ported wire in the enclosing
    /// container to keep in sync, and no way for one branch to lead
    /// somewhere the other doesn't: both always rejoin here.
    If {
        condition: Condition,
        #[serde(default)]
        then: Branch,
        #[serde(default)]
        otherwise: Branch,
    },
    Loop {
        count: u32,
        #[serde(default)]
        body: Branch,
    },
    /// A nested-container branch, same shape as `If`/`Loop`: runs
    /// `try_branch` in isolation; if any of its steps fails (and isn't
    /// itself configured to retry/skip past that failure), the error
    /// is caught here instead of aborting the whole flow — `catch`
    /// runs instead, with the failure's message available as the
    /// `caught_error` variable. If `try_branch` succeeds, `catch`
    /// never runs at all. Either way, execution falls back out to this
    /// step's own single plain output once whichever branch actually
    /// ran finishes — same reconvergence rule as `If`.
    TryCatch {
        #[serde(default)]
        try_branch: Branch,
        #[serde(default)]
        catch: Branch,
    },
    /// A named, reusable step sequence — Scratch's custom block, not
    /// n8n's "Execute Workflow" (that's a separate, external-file
    /// concept; this stays inside one `.relay` file). Structurally a
    /// single-branch nested container exactly like `Loop`'s `body` —
    /// reachable from multiple `CallFunction` steps scattered anywhere
    /// in the flow, unlike `Loop`'s body, which only one node ever
    /// owns. If a normal wire ever leads directly into this step
    /// (nothing does by convention, but nothing stops a user from
    /// trying), running it this way is a deliberate no-op — `body`
    /// only ever runs via `CallFunction`'s explicit lookup by `name`,
    /// never by being wired into like an ordinary step.
    FunctionDef {
        name: String,
        #[serde(default)]
        body: Branch,
    },
    /// Runs the `body` of whichever `FunctionDef` step (anywhere in
    /// this flow) has this `name`, then returns to whatever wire
    /// follows this step — a plain Rust call/return under the hood
    /// (`run_branch` recursing into the function's `Branch` and
    /// returning when it finishes), no separate call-stack data
    /// structure needed. Fails if no `FunctionDef` with `name`
    /// exists, or if calling it would recurse (directly or through
    /// another function) back into a function already being called.
    CallFunction {
        name: String,
    },
    /// Searches the screen for `image`. Succeeds (and stores the
    /// match into `last_match_x` / `last_match_y` / `last_match_score`
    /// variables, and into the run's "last match" for any later
    /// `Click`/`MoveMouse` step targeting `ClickTarget::LastMatch` /
    /// `PointTarget::LastMatch`) if found above threshold; fails
    /// otherwise — combine with `retry` for "wait for image" semantics.
    FindImage {
        image: ImageSource,
        #[serde(default)]
        mode: MatchMode,
        /// Used by `Similar` and `Ai` — `Exact` always requires
        /// ~perfect correlation at the original size. For `Ai` this is
        /// a cosine-similarity threshold (embedding space) rather than
        /// a normalized-cross-correlation score, but both are roughly
        /// 0..1 so the same UI slider/field is reused.
        #[serde(default = "default_threshold")]
        threshold: f64,
        /// Used by `Similar` and `Ai` — how much smaller/larger than
        /// the reference image a match is still allowed to be (1.0 =
        /// same size). `Exact` always searches at exactly 1.0. For
        /// `Ai` this bounds the candidate-localization scan that feeds
        /// the embedding rescoring, same as it bounds `Similar`'s NCC
        /// scan.
        #[serde(default = "default_min_scale")]
        min_scale: f64,
        #[serde(default = "default_max_scale")]
        max_scale: f64,
        /// Used by `Similar` and `Ai` — how many scale steps to sample
        /// across `[min_scale, max_scale]`. More steps costs roughly
        /// proportionally more time (each is close to a full
        /// coarse-then-refine search — see `vision::locate`) in
        /// exchange for a finer-grained match to the true scale;
        /// fewer steps is faster but coarser. The UI's "認識パフォーマンス"
        /// low/balanced/high presets set this together with
        /// `min_scale`/`max_scale`. `Ai` only runs embedding inference
        /// on a handful of the resulting candidates regardless of step
        /// count (see `vision::AI_CANDIDATE_COUNT`), so this affects
        /// `Ai`'s localization precision more than its wall-clock time.
        #[serde(default = "default_scale_steps")]
        scale_steps: u32,
    },
    /// Runs Windows' built-in OCR over the screen (or just `region`,
    /// if set — narrows the scan and avoids false-positive matches
    /// from unrelated text elsewhere on screen), succeeding if `text`
    /// appears anywhere in the recognized text (case-insensitive) —
    /// fails otherwise, so combine with `retry` for "wait until this
    /// text shows up" the same way `FindImage` is used, but without
    /// needing a reference image ahead of time.
    FindTextOcr {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<CaptureRegion>,
    },
    /// Polls for up to `timeout_ms` until a window matching `window`
    /// exists, checking every 500ms — genuinely waits on its own now,
    /// unlike relying on the generic per-step `retry` policy (whose
    /// `max_attempts` defaults to 0, meaning "one single check, no
    /// waiting at all" unless a flow author happens to configure it —
    /// exactly the "times out instantly" behavior this step's own name
    /// promised not to have).
    WaitForWindow {
        window: WindowSelector,
        #[serde(default = "default_wait_for_window_timeout_ms")]
        timeout_ms: u32,
    },
    /// Brings the window matching `window` to the foreground and gives
    /// it keyboard focus.
    FocusWindow {
        window: WindowSelector,
    },
    /// Shuts down or restarts the machine. `force` closes apps that
    /// would otherwise block it by prompting to save unsaved work.
    PowerAction {
        mode: PowerMode,
        #[serde(default)]
        force: bool,
    },
    /// Locks the workstation — same effect as Win+L.
    LockWorkstation,
    /// Reads the clipboard's current text into `variable` — fails if
    /// the clipboard doesn't hold text.
    ReadClipboard {
        variable: String,
    },
    /// Replaces the clipboard's contents with `text`.
    WriteClipboard {
        text: String,
    },
    /// Ends the flow run immediately and successfully — not a failure,
    /// just "stop here". Typically wired behind an `if` so a flow can
    /// bail out early once some condition is met, without needing an
    /// explicit path to the very last step.
    Stop,
    /// Starts a new process. `args` is a single space-separated
    /// string, split and passed through as plain argv — no shell
    /// involved.
    LaunchApp {
        path: String,
        #[serde(default)]
        args: String,
    },
    /// Opens `url` in the user's default browser (shell-level, like
    /// double-clicking a link). For actually driving page content,
    /// see the `Browser*` actions below.
    OpenUrl {
        url: String,
    },
    /// Shows a Windows toast notification. `message` may be empty (a
    /// title-only toast).
    Notify {
        title: String,
        #[serde(default)]
        message: String,
    },
    /// Reads `path`'s entire contents as UTF-8 text into `variable`.
    ReadFile {
        path: String,
        variable: String,
    },
    /// Writes `content` to `path` — overwrites by default, or appends
    /// when `append` is set.
    WriteFile {
        path: String,
        content: String,
        #[serde(default)]
        append: bool,
    },
    CopyFile {
        source: String,
        destination: String,
    },
    /// Moves (or, within the same directory, renames) a file — fails
    /// if `destination` already exists.
    MoveFile {
        source: String,
        destination: String,
    },
    DeleteFile {
        path: String,
    },
    /// Creates `path`, including any missing parent directories — a
    /// no-op (not a failure) if it already exists.
    CreateDirectory {
        path: String,
    },
    /// Lists `path`'s immediate entries (not recursive) into
    /// `variable` as a newline-joined, alphabetically sorted string —
    /// there's no array/list variable type, so this is the same "one
    /// string" shape `ReadFile` uses.
    ListDirectory {
        path: String,
        variable: String,
    },
    /// Sends an HTTP request and stores the response body into
    /// `variable` and the numeric status code into `status_variable` —
    /// two independently-named outputs (neither a fixed suffix on the
    /// other) so both can be renamed freely, same as any other step's
    /// outputs. `headers` is `Name: Value` pairs, one per line.
    Http {
        method: HttpMethod,
        url: String,
        #[serde(default)]
        headers: String,
        #[serde(default)]
        body: String,
        variable: String,
        status_variable: String,
    },
    /// Sends a GET request and saves the raw response body straight to
    /// `path` — for actually downloading a file (an image, a zip, ...)
    /// rather than reading text/JSON into a variable, which is what
    /// plain `Http` is for. The response is streamed straight to disk
    /// rather than buffered fully in memory first, so this scales to a
    /// large file the same way it would to a small one. `variable`
    /// receives the numeric status code; `path_variable` receives
    /// `path` after `%variable%` substitution — the actual saved
    /// location, for a later step to reference without having to
    /// rebuild/re-resolve `path` itself.
    HttpDownload {
        url: String,
        #[serde(default)]
        headers: String,
        path: String,
        variable: String,
        path_variable: String,
    },
    /// Reads a UI element's text (its editable value if it has one,
    /// otherwise its accessible name) into `variable`.
    GetElementText {
        selector: ElementSelector,
        variable: String,
    },
    /// Opens a new browser window navigated to `url` (or left blank if
    /// empty), and stores its instance id into `variable` — Relay's
    /// answer to PAD's browser-instance handle. Every `Browser*`
    /// action's `instance` field can hold `"%variable%"` to target
    /// this specific window from then on, instead of whatever the
    /// bridge's default connection happens to be. `browser` picks
    /// which installed browser to launch (an id from the
    /// browser-picker, e.g. `"chrome"`/`"edge"`; empty/unset falls back
    /// to whichever's found installed first). `profile_dir` points
    /// `--user-data-dir` at a specific folder for an isolated,
    /// dedicated profile; empty/unset uses that browser's own normal
    /// default profile instead (the same one the user already browses
    /// with — its bookmarks, logins, and an already-installed Relay
    /// Bridge extension all apply immediately), which in turn means
    /// this may open as a new window in whatever instance of that
    /// browser is already running rather than a genuinely separate
    /// process.
    LaunchBrowser {
        #[serde(default)]
        url: String,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        browser: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_dir: Option<String>,
    },
    /// Navigates a tab of whatever browser the companion extension is
    /// running in to `url` — `instance` (a `LaunchBrowser`-captured
    /// tab id, or a variable reference to one) targets a specific
    /// tab; `None`/unset falls back to whatever tab is currently
    /// active, same as before instances existed.
    BrowserNavigate {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Clicks the first element in the target tab matching `selector`.
    BrowserClick {
        selector: BrowserSelector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Reads `innerText` (or, for form fields, the current value) of
    /// the first element matching `selector` into `variable`.
    BrowserGetText {
        selector: BrowserSelector,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Sets the value of an `<input>`/`<textarea>`/`<select>` matching
    /// `selector`, dispatching the same `input`/`change` events a real
    /// keystroke would so the page's own JavaScript notices the
    /// change.
    BrowserSetValue {
        selector: BrowserSelector,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Polls the target tab until an element matching `selector`
    /// appears in the DOM, or fails after its own internal timeout —
    /// combine with `retry` the same way `WaitForWindow` is used, for
    /// pages that load content asynchronously.
    BrowserWaitForSelector {
        selector: BrowserSelector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Shows a message box with an OK button. `blocking` (default
    /// `true`, omitted from YAML at that default so ordinary flows
    /// stay uncluttered) controls whether the flow pauses until the
    /// user dismisses it or continues to the next step immediately,
    /// leaving the box open — the latter for a status note the user
    /// should see eventually but that shouldn't hold up the rest of
    /// the run.
    ShowMessage {
        title: String,
        message: String,
        #[serde(default = "default_true", skip_serializing_if = "is_true_bool")]
        blocking: bool,
    },
    /// Shows a blocking Yes/No prompt, storing `"yes"`/`"no"` into
    /// `variable` once the user picks one — combine with `If` to
    /// branch on the answer.
    ShowConfirm {
        title: String,
        message: String,
        variable: String,
    },
    /// Shows a blocking text-input prompt pre-filled with
    /// `default_value`, storing whatever the user typed (or
    /// `default_value` if they cancel) into `variable`.
    ShowInput {
        title: String,
        message: String,
        #[serde(default)]
        default_value: String,
        variable: String,
    },
    /// Immediately exits the innermost enclosing `Loop`, skipping any
    /// remaining iterations — the same idea as `break` in a for loop.
    /// Used outside any `Loop` (directly in the main flow, or inside a
    /// `FunctionDef` body that isn't itself inside a loop), it just
    /// ends the run early instead — same as `Stop` — since lexically
    /// there's no loop to break out of.
    Break,
    /// Skips the rest of the current iteration of the innermost
    /// enclosing `Loop` and moves straight to the next one. Used
    /// outside any `Loop`, it's a no-op — there's nothing to skip to.
    Continue,
    /// Immediately ends the current `FunctionDef` call, returning
    /// control to right after the `CallFunction` step that invoked it
    /// — the same idea as `return` in a function. Used outside any
    /// function call (directly in the main flow, or inside a `Loop`
    /// that isn't itself inside a function call), it just ends the run
    /// early instead — same as `Stop`.
    Return,
    /// The current date/time, formatted per `format`, into `variable`.
    GetDateTime {
        #[serde(default)]
        format: DateTimeFormat,
        variable: String,
    },
    /// A snapshot of this machine's basic state — each field is
    /// independently `Some(variable name)` to write it, or `None` to
    /// skip gathering it at all (no reason to spend ~200ms sampling
    /// CPU usage if nothing downstream reads it). Every field writes
    /// to its own independently-named variable rather than a fixed
    /// suffix on a shared prefix, so nothing about this step's output
    /// is harder to rename than any other step's.
    GetSystemInfo {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hostname: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        os_version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cpu_percent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        memory_percent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ip_address: Option<String>,
    },
    /// A single text operation — see `TextOp`'s doc comment for what
    /// each one does and how `arg1`/`arg2` are used (their meaning
    /// depends entirely on `op`; both are ignored where an operation
    /// doesn't need them).
    TextTransform {
        op: TextOp,
        text: String,
        #[serde(default)]
        arg1: String,
        #[serde(default)]
        arg2: String,
        variable: String,
    },
    /// Pings `host` (a native ICMP echo, the same mechanism the `ping`
    /// command uses), storing `"true"`/`"false"` (whether it
    /// responded within `timeout_ms`) into `variable`, plus the round
    /// trip time in milliseconds into `{variable}_latency_ms` when it
    /// did respond.
    Ping {
        host: String,
        #[serde(default = "default_ping_timeout_ms")]
        timeout_ms: u32,
        variable: String,
    },
    /// Resolves `hostname` to its IP address via the system's normal
    /// DNS resolution, into `variable` — fails if it doesn't resolve.
    DnsLookup {
        hostname: String,
        variable: String,
    },
    /// Saves a screenshot of the screen to `path` as a PNG — the
    /// whole (virtual, every connected monitor) desktop if `region` is
    /// unset, otherwise just that rectangle (logical, DPI-independent
    /// coordinates — the same region shape `FindTextOcr`'s scan area
    /// uses).
    Screenshot {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<CaptureRegion>,
        path: String,
    },
    /// Saves a screenshot of a browser tab's visible viewport to
    /// `path` as a PNG. Unlike every other `Browser*` action, this
    /// briefly steals focus to that tab if it isn't already the
    /// active one in its window — Chrome's extension API can only
    /// capture whichever tab is currently on top, there's no way to
    /// screenshot a background tab directly.
    BrowserScreenshot {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// Reads environment variable `name` into `variable` — fails if
    /// it isn't set (a missing env var and an empty one are different
    /// things; conflating them would hide a real misconfiguration).
    GetEnvVar {
        name: String,
        variable: String,
    },
    /// Whether at least one running process is named `name` (matched
    /// case-insensitively, e.g. `"notepad.exe"`), into `variable` as
    /// `"true"`/`"false"`.
    CheckProcess {
        name: String,
        variable: String,
    },
    /// Ends every running process named `name`. `force` skips asking
    /// each process to close itself first (the same difference as
    /// `PowerAction`'s `force`) — off by default so this doesn't
    /// silently discard another app's unsaved work.
    KillProcess {
        name: String,
        #[serde(default)]
        force: bool,
    },
    /// Polls until `path` exists on disk, or fails after `timeout_ms`
    /// — the same "wait for X" shape `WaitForWindow`/`FindImage` use,
    /// generalized to "a file shows up" (a download finishing, another
    /// program writing its output, ...) rather than a window or an
    /// on-screen image.
    WaitForFile {
        path: String,
        #[serde(default = "default_wait_for_file_timeout_ms")]
        timeout_ms: u32,
    },
    /// A random integer in `[min, max]` (inclusive both ends,
    /// resolved from text the same way `Calculate`'s operands are),
    /// into `variable`.
    GenerateRandom {
        min: String,
        max: String,
        variable: String,
    },
}

fn default_true() -> bool {
    true
}

fn is_true_bool(value: &bool) -> bool {
    *value
}

fn default_ping_timeout_ms() -> u32 {
    2000
}

fn default_wait_for_file_timeout_ms() -> u32 {
    30_000
}

fn default_threshold() -> f64 {
    0.85
}

fn default_min_scale() -> f64 {
    0.7
}

fn default_max_scale() -> f64 {
    1.4
}

fn default_scale_steps() -> u32 {
    12
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalcOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    /// Rounds `a` to `b` decimal places (`b: "0"` for the nearest
    /// whole number) — half-away-from-zero, same as `f64::round`.
    Round,
    /// Rounds `a` down to `b` decimal places.
    Floor,
    /// Rounds `a` up to `b` decimal places.
    Ceil,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClickKind {
    #[default]
    Single,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    Exact,
    Similar,
    /// CNN embedding similarity — robust to color/brightness shifts
    /// and noise that break `Similar`'s pixel correlation, at the cost
    /// of being noticeably slower (a forward pass per candidate).
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PowerMode {
    #[default]
    Shutdown,
    Restart,
}

/// `Tap` presses and immediately releases `KeyPress`'s `key` (and any
/// `modifiers`) — the ordinary case. `Press` holds them down without
/// releasing, for building a custom sequence across multiple steps
/// (e.g. holding Shift while several `Click` steps run to
/// multi-select something). `Release` lets go of a key a matching
/// `Press` left held.
///
/// A key still held when the flow run ends — the matching `Release`
/// step was skipped, the flow failed partway through, the user hit
/// Stop, ... — is always force-released by the engine (see
/// `engine::runner::run_flow_with_backend`), so a flow can never
/// leave the real keyboard's modifier state stuck down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeyPressMode {
    #[default]
    Tap,
    Press,
    Release,
}

/// Modifier keys to hold alongside `KeyPress`'s `key` — see its doc
/// comment. Meaningless (and ignored) for `KeyPressMode::Release`:
/// releasing lets go of whichever modifiers that key's matching
/// `Press` actually pressed, tracked by the engine rather than
/// re-specified here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct KeyModifiers {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub win: bool,
}

/// How `GetDateTime` renders the current date/time — presets rather
/// than a raw strftime pattern, so the field stays approachable to
/// someone who's never seen one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DateTimeFormat {
    /// `2026-08-27 22:48:06` — sortable, unambiguous, the sensible
    /// default for a value that's more likely to be logged/compared
    /// than read aloud.
    #[default]
    Iso8601,
    /// `2026-08-27`.
    DateOnly,
    /// `22:48:06`.
    TimeOnly,
    /// Seconds since the Unix epoch, as a plain integer string — for
    /// arithmetic (elapsed time, a cache-busting value, ...) rather
    /// than display.
    UnixSeconds,
}

/// A single text operation `TextTransform` runs on `text`, storing
/// the result into `variable`. `arg1`/`arg2`'s meaning depends
/// entirely on which one this is — see each variant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOp {
    Uppercase,
    Lowercase,
    /// Strips leading/trailing whitespace. Neither `arg1` nor `arg2`
    /// is used.
    Trim,
    /// Replaces every occurrence of `arg1` in `text` with `arg2`.
    Replace,
    /// The substring starting at `arg1` (a character index, not
    /// bytes — 0-based), `arg2` characters long. `arg2` empty means
    /// "to the end of `text`".
    Substring,
    /// `text`'s length, as a plain integer string. Neither `arg1` nor
    /// `arg2` is used.
    Length,
    /// `"true"`/`"false"`: whether `text` contains `arg1`.
    Contains,
    /// `"true"`/`"false"`: whether `text` starts with `arg1`.
    StartsWith,
    /// `"true"`/`"false"`: whether `text` ends with `arg1`.
    EndsWith,
    /// Splits `text` on `arg1` (empty `arg1` splits on any
    /// whitespace run) and takes the piece at `arg2`'s index
    /// (0-based; empty `arg2` joins every piece back with `\n`
    /// instead of picking one — the same "one string, newline-joined"
    /// shape `ListDirectory` uses for its listing).
    Split,
    Base64Encode,
    /// Fails if `text` isn't valid base64.
    Base64Decode,
    /// Hex-encoded MD5 — offered alongside `Sha256` for
    /// compatibility with older systems/APIs that still expect it,
    /// not because it's recommended for anything security-sensitive.
    Md5,
    /// Hex-encoded SHA-256.
    Sha256,
    /// Reads the value at `arg1` — a dot/bracket path like
    /// `user.name` or `items[0].id` — out of `text` (parsed as JSON).
    /// A string/number/bool comes back as its own plain text; an
    /// object/array comes back re-serialized as compact JSON text.
    /// Fails if `text` isn't valid JSON or the path doesn't exist.
    JsonGet,
    /// Escapes `text` so it's safe to splice into a JSON string
    /// literal (quotes, backslashes, control characters) — for
    /// building a JSON request body via plain string
    /// concatenation/`%variable%` substitution, the same way
    /// `Http`'s `body` field already works. Neither `arg1` nor `arg2`
    /// is used.
    JsonEscape,
    /// `"true"`/`"false"`: whether `text` matches the regular
    /// expression `arg1` anywhere in it.
    RegexTest,
    /// The first match of the regular expression `arg1` in `text` —
    /// the whole match, or capture group number `arg2` (0 is the
    /// whole match; empty `arg2` also means the whole match). Empty
    /// string if there's no match, rather than failing the step — use
    /// `RegexTest` first to tell "no match" apart from "matched an
    /// empty string".
    RegexMatch,
}

/// Equality check against a variable, evaluated by the engine at
/// runtime. Only equality is supported for now; extend as real flows
/// need it (contains, numeric comparison, etc).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub variable: String,
    pub equals: String,
}
