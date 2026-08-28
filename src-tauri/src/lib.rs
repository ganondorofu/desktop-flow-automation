use base64::Engine;
use engine::{ExecutionObserver, StepOutcome};
use flow_schema::{parse_flow, ClickKind, Flow, MonitorPoint, MouseButton, Step};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Parses a flow definition (YAML) and returns it as structured data,
/// proving the GUI can round-trip through the same schema the
/// execution engine will consume.
#[tauri::command]
fn parse_flow_yaml(yaml: String) -> Result<Flow, String> {
    parse_flow(&yaml).map_err(|e| e.to_string())
}

/// Clicks at a logical point on the primary monitor. This is a Phase 0
/// smoke test for the automation crate wired through Tauri — the real
/// command surface (per-step execution, retries, breakpoints) is
/// Phase 1 work per docs/roadmap.md.
#[tauri::command]
fn click_at(x: i32, y: i32) -> Result<(), String> {
    let point = MonitorPoint {
        monitor_id: "primary".into(),
        x,
        y,
    };
    automation::click_at(&point, MouseButton::Left, ClickKind::Single).map_err(|e| e.to_string())
}

/// Sleeps `delay_ms` then reads the cursor position — the backend half
/// of "click 位置を記録": the frontend shows a countdown while the user
/// moves the mouse to the target, then calls this once to capture it.
///
/// Runs on a blocking-pool thread (`spawn_blocking`) rather than
/// whatever thread Tauri dispatches a plain sync command on — on
/// Windows that dispatch thread also pumps the app's own window
/// messages, so a raw multi-second `std::thread::sleep` there froze
/// the whole window (title bar included) for the sleep's duration.
#[tauri::command]
async fn cursor_position_after_delay(delay_ms: u64) -> Result<(i32, i32), String> {
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        automation::cursor_position().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
struct PickedUiElement {
    window_title: String,
    element_name: String,
    automation_id: String,
    preview: String,
}

/// The backend half of the desktop UI-element picker: the frontend
/// shows the same countdown as "click 位置を記録", then calls this once
/// to hit-test whatever's under the cursor at that moment. Off the
/// command-dispatch thread for the same reason as
/// `cursor_position_after_delay` — see its doc comment.
#[tauri::command]
async fn pick_ui_element_after_delay(delay_ms: u64) -> Result<PickedUiElement, String> {
    tauri::async_runtime::spawn_blocking(move || {
        automation::pick_ui_element_after_delay(delay_ms)
            .map(|p| PickedUiElement {
                window_title: p.window_title,
                element_name: p.element_name,
                automation_id: p.automation_id,
                preview: p.preview,
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reads `path` and returns its raw bytes as base64 — the backend
/// half of "参照…" (browse for an existing image file), so a picked
/// file can be embedded into the flow the same way a fresh capture
/// is, instead of only being usable as a live file-path reference.
#[tauri::command]
fn read_file_base64(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("failed to read '{path}': {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// The whole virtual desktop (every connected monitor) as a
/// base64-encoded PNG — the backend half of the in-app region picker
/// (`RegionPickerHost.tsx`), which shows this screenshot inside the
/// app's own window and lets the user drag-select on it directly,
/// like an ordinary image crop tool. Not a separate transparent
/// overlay window: that approach went through several rendering/input
/// failure modes on Windows (WebView2 doesn't support real alpha
/// blending, only fully transparent or fully opaque, among other
/// issues) before being replaced by this simpler, single-window
/// design.
#[tauri::command]
fn capture_full_screen_base64() -> Result<String, String> {
    automation::capture_full_screen_to_base64().map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct VirtualScreenBounds {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// The backend half of resizing the region picker's own window to
/// exactly cover the virtual desktop (see `RegionPickerHost.tsx`),
/// so drag-selecting on the captured screenshot lines up with the
/// real screen underneath it.
#[tauri::command]
fn virtual_screen_bounds() -> VirtualScreenBounds {
    let (x, y, width, height) = automation::virtual_screen_bounds();
    VirtualScreenBounds { x, y, width, height }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
enum StepEvent {
    Start { step_id: String },
    Success { step_id: String },
    Error { step_id: String, message: String },
    MonitorMismatch,
    MonitorRestored,
    Paused { step_id: String },
    Resumed,
}

/// Forwards each step's start/result to the GUI as a `flow-step` event,
/// so the canvas can highlight the node that's actually executing
/// instead of showing a status that was never really produced.
struct TauriObserver {
    app: AppHandle,
}

impl ExecutionObserver for TauriObserver {
    fn on_step_start(&mut self, step: &Step) {
        let _ = self.app.emit("flow-step", StepEvent::Start { step_id: step.id.clone() });
    }

    fn on_step_result(&mut self, step: &Step, outcome: &StepOutcome) {
        let event = match outcome {
            StepOutcome::Success => StepEvent::Success { step_id: step.id.clone() },
            StepOutcome::Failed(failure) => {
                StepEvent::Error { step_id: step.id.clone(), message: failure.message.clone() }
            }
        };
        let _ = self.app.emit("flow-step", event);
    }

    fn on_monitor_mismatch(&mut self) {
        let _ = self.app.emit("flow-step", StepEvent::MonitorMismatch);
    }

    fn on_monitor_restored(&mut self) {
        let _ = self.app.emit("flow-step", StepEvent::MonitorRestored);
    }

    fn on_paused(&mut self, step: &Step) {
        let _ = self.app.emit("flow-step", StepEvent::Paused { step_id: step.id.clone() });
    }

    fn on_resumed(&mut self) {
        let _ = self.app.emit("flow-step", StepEvent::Resumed);
    }

    fn on_variables_changed(&mut self, variables: &std::collections::HashMap<String, String>) {
        let _ = self.app.emit("flow-variables", variables);
    }
}

/// Parses and runs a flow end to end, emitting a `flow-step` event per
/// step so the GUI reflects real progress rather than a canned demo.
/// Off the command-dispatch thread for the same reason as
/// `cursor_position_after_delay` — any `wait`/retry step otherwise
/// froze the window for as long as the flow ran.
/// `step_mode: true` starts the run already paused before its first
/// step (the toolbar's "ステップ" command) instead of running freely
/// until the first `Step.breakpoint` (a plain "実行").
#[tauri::command]
async fn run_flow_yaml(app: AppHandle, yaml: String, step_mode: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let flow: Flow = parse_flow(&yaml).map_err(|e| e.to_string())?;
        let mut observer = TauriObserver { app };
        engine::run_flow(&flow, &mut observer, step_mode).map_err(|f| format!("{}: {}", f.step_id, f.message))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The "force stop" command — the Escape-while-running shortcut and
/// the toolbar's Stop button both call this. `run_flow_yaml`'s own
/// `engine::run_flow` polls this between steps (and during any wait),
/// so the running flow notices within tens of milliseconds rather than
/// running to its natural end regardless of what the user wants.
#[tauri::command]
fn stop_flow() {
    engine::request_stop();
}

/// Advances a paused run (step-through mode, or a hit breakpoint) by
/// exactly one step, then pauses again before the step after that.
#[tauri::command]
fn debug_step() {
    engine::request_step();
}

/// Resumes a paused run, letting it run freely until the next
/// `Step.breakpoint` or the flow ends.
#[tauri::command]
fn debug_continue() {
    engine::request_continue();
}

/// Whether Relay itself is currently running elevated — Windows'
/// UIPI blocks a non-elevated process from sending input to an
/// elevated one's windows at all, so a flow that clicks/types into an
/// app running "as administrator" silently does nothing unless Relay
/// is elevated too. The UI surfaces this so that failure isn't a
/// mystery.
#[tauri::command]
fn is_elevated() -> bool {
    automation::is_process_elevated()
}

/// Every Chromium-family browser (Chrome, Edge) actually found
/// installed on this machine — feeds the `LaunchBrowser` node's
/// browser picker in the Inspector.
#[tauri::command]
fn list_installed_browsers() -> Vec<automation::BrowserInfo> {
    automation::find_installed_browsers()
}

/// Relaunches Relay elevated (via Windows' own UAC prompt) and exits
/// the current, non-elevated instance once the new one is confirmed
/// running — used by the "管理者として再起動" action for automating a
/// target app that itself runs elevated.
#[tauri::command]
fn relaunch_as_admin(app: AppHandle) -> Result<(), String> {
    automation::relaunch_elevated().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// Writes `yaml` to `path` as-is, creating any missing parent
/// directories first — the whole of "保存"/"名前を付けて保存".
#[tauri::command]
fn save_flow_to_path(path: String, yaml: String) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, yaml).map_err(|e| e.to_string())
}

/// Reads a previously-saved flow file back as raw YAML text — parsing
/// it into the frontend's editable node graph is the frontend's job
/// (`parseFlowYaml` in flowModel.ts), not this command's.
#[tauri::command]
fn load_flow_from_path(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[derive(Clone, Serialize, Deserialize)]
struct RecentFile {
    path: String,
    name: String,
    /// Unix seconds — only used to keep the list sorted newest-first,
    /// never shown to the user as a real timestamp.
    opened_at: u64,
}

fn recent_flows_store_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("recent_flows.json"))
}

fn read_recent_flows(app: &AppHandle) -> Result<Vec<RecentFile>, String> {
    let store = recent_flows_store_path(app)?;
    if !store.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&store).map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

fn write_recent_flows(app: &AppHandle, list: &[RecentFile]) -> Result<(), String> {
    let store = recent_flows_store_path(app)?;
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&store, text).map_err(|e| e.to_string())
}

/// The home screen's recent-flows list — newest first, capped at 10.
#[tauri::command]
fn list_recent_flows(app: AppHandle) -> Result<Vec<RecentFile>, String> {
    read_recent_flows(&app)
}

/// Moves `path` to the front of the recent-flows list (adding it if
/// new), and returns the updated list so the frontend doesn't need a
/// second round trip to refresh its home-screen display.
#[tauri::command]
fn remember_recent_flow(app: AppHandle, path: String) -> Result<Vec<RecentFile>, String> {
    let mut list = read_recent_flows(&app)?;
    list.retain(|f| f.path != path);
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let opened_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    list.insert(0, RecentFile { path, name, opened_at });
    list.truncate(10);
    write_recent_flows(&app, &list)?;
    Ok(list)
}

/// Removes one entry from the recent-flows list — used for "リストから削除"
/// when a file has moved or the user just wants it gone, without
/// touching the file itself.
#[tauri::command]
fn forget_recent_flow(app: AppHandle, path: String) -> Result<Vec<RecentFile>, String> {
    let mut list = read_recent_flows(&app)?;
    list.retain(|f| f.path != path);
    write_recent_flows(&app, &list)?;
    Ok(list)
}

/// A sensible default folder to suggest in the "New flow" save dialog
/// — the user's Documents folder (falling back to their home
/// directory), under a `Relay Flows` subfolder created on demand.
#[tauri::command]
fn default_flows_dir(app: AppHandle) -> Result<String, String> {
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| e.to_string())?;
    let flows_dir = base.join("Relay Flows");
    std::fs::create_dir_all(&flows_dir).map_err(|e| e.to_string())?;
    Ok(flows_dir.to_string_lossy().to_string())
}

/// The port the companion browser extension's WebSocket connects to.
/// Fixed rather than configurable for now — matches the port baked
/// into `browser-extension/background.js`.
const BROWSER_BRIDGE_PORT: u16 = 17_845;

/// Whether the browser extension currently has a live connection —
/// lets the UI show "browser bridge: connected/not connected" instead
/// of a `BrowserClick` step's failure being the first sign anything
/// was wrong.
#[tauri::command]
fn browser_bridge_status() -> bool {
    browser_bridge::is_connected()
}

#[derive(Clone, Serialize)]
struct PickedBrowserElement {
    selector: String,
    preview: String,
}

/// Tells the extension to enter "click something on the page" mode and
/// waits (up to 60s, since this is gated on the user actually clicking)
/// for the picked element's CSS selector. Off the command-dispatch
/// thread for the same reason as `cursor_position_after_delay` — a
/// blocking wait this long left the whole window unresponsive for the
/// entire pick.
#[tauri::command]
async fn browser_pick_element() -> Result<PickedBrowserElement, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let result = browser_bridge::send_command_with_timeout(
            None,
            "pick_element",
            serde_json::json!({}),
            std::time::Duration::from_secs(60),
        )?;
        let selector = result
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "browser extension returned an unexpected reply".to_string())?
            .to_string();
        let preview = result.get("preview").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        Ok(PickedBrowserElement { selector, preview })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Tells the extension to tear down its picker overlay in every tab
/// right now, for the "cancel" button — otherwise every tab keeps
/// hijacking clicks until the pending `pick_element` call's own 60s
/// timeout, since that command only resolves once picking actually
/// ends one way or another.
#[tauri::command]
async fn browser_cancel_pick() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        browser_bridge::send_command_with_timeout(None, "cancel_pick", serde_json::json!({}), std::time::Duration::from_secs(5))
            .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|_app| {
            browser_bridge::start_server(BROWSER_BRIDGE_PORT);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_flow_yaml,
            click_at,
            run_flow_yaml,
            stop_flow,
            debug_step,
            debug_continue,
            cursor_position_after_delay,
            capture_full_screen_base64,
            virtual_screen_bounds,
            read_file_base64,
            save_flow_to_path,
            load_flow_from_path,
            list_recent_flows,
            remember_recent_flow,
            forget_recent_flow,
            default_flows_dir,
            browser_bridge_status,
            pick_ui_element_after_delay,
            browser_pick_element,
            browser_cancel_pick,
            is_elevated,
            relaunch_as_admin,
            list_installed_browsers
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
