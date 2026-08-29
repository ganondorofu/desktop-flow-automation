//! Minimal Win32 input backend: coordinate clicks and text entry via
//! `SendInput`, with DPI normalization so flows store logical
//! (DPI-independent) coordinates rather than raw physical pixels.
//!
//! Screen capture, image/text search, and mouse click/move all reach
//! across the whole virtual desktop (every connected monitor), not
//! just the primary one — `SendInput` uses `MOUSEEVENTF_VIRTUALDESK`
//! and every physical-pixel helper is normalized against
//! `SM_C{X,Y}VIRTUALSCREEN`. What's still unimplemented: real
//! per-monitor DPI. `MonitorPoint::monitor_id` isn't resolved to a
//! specific `HMONITOR`, and every logical<->physical conversion
//! assumes the *primary* monitor's DPI applies everywhere — accurate
//! when every display uses the same scale factor (the common case),
//! off by that ratio on a secondary monitor scaled differently. Real
//! per-monitor DPI resolution is Phase 1 work per docs/roadmap.md.

#![cfg(windows)]

use base64::Engine;
use flow_schema::{ClickKind, ElementSelector, ImageSource, MatchMode, MonitorPoint, MouseButton};
use uiautomation::UIAutomation;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, LPARAM, POINT, RECT};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, MonitorFromPoint, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10,
    VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_LEFT, VK_LWIN, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW, GA_ROOT, SM_CXSCREEN,
    SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

const DEFAULT_DPI: u32 = 96;

#[derive(Debug)]
pub struct AutomationError(pub String);

impl std::fmt::Display for AutomationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for AutomationError {}

/// Moves the cursor to `point` and clicks with the given button/kind
/// (single or double click). Low-level primitive — `Action::Click`
/// itself never carries a coordinate (see `ClickTarget`'s doc
/// comment); this exists for callers that already have a point in
/// hand outside that action (there are none in the engine today, but
/// it's a reasonable building block to keep).
pub fn click_at(point: &MonitorPoint, button: MouseButton, click_kind: ClickKind) -> Result<(), AutomationError> {
    let (physical_x, physical_y) = to_physical_coordinates(point)?;
    let (abs_x, abs_y) = to_absolute(physical_x, physical_y)?;
    let mut inputs = vec![mouse_input(abs_x, abs_y, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)];
    inputs.extend(click_inputs(abs_x, abs_y, button, click_kind));
    send(&mut inputs)
}

/// Clicks wherever the cursor already is — what `Action::Click`
/// actually runs, since positioning is `MoveMouse`'s job (see
/// `ClickTarget::Cursor`'s doc comment).
pub fn click_at_cursor(button: MouseButton, click_kind: ClickKind) -> Result<(), AutomationError> {
    let (abs_x, abs_y) = cursor_position_absolute()?;
    let mut inputs = click_inputs(abs_x, abs_y, button, click_kind);
    send(&mut inputs)
}

fn click_inputs(abs_x: i32, abs_y: i32, button: MouseButton, click_kind: ClickKind) -> Vec<INPUT> {
    let (down, up) = match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    };
    let clicks = match click_kind {
        ClickKind::Single => 1,
        ClickKind::Double => 2,
    };
    let mut inputs = Vec::with_capacity(clicks * 2);
    for _ in 0..clicks {
        inputs.push(mouse_input(abs_x, abs_y, down | MOUSEEVENTF_ABSOLUTE));
        inputs.push(mouse_input(abs_x, abs_y, up | MOUSEEVENTF_ABSOLUTE));
    }
    inputs
}

/// Moves the cursor to `point` without clicking. `duration_ms` of 0
/// moves instantly; anything above that interpolates in ~15ms steps so
/// the motion is visible at human speed instead of teleporting.
pub fn move_mouse(point: &MonitorPoint, duration_ms: u32) -> Result<(), AutomationError> {
    let (physical_x, physical_y) = to_physical_coordinates(point)?;
    let (target_x, target_y) = to_absolute(physical_x, physical_y)?;

    if duration_ms == 0 {
        let mut inputs = [mouse_input(target_x, target_y, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)];
        return send(&mut inputs);
    }

    let (start_x, start_y) = cursor_position_absolute()?;
    const STEP_MS: u32 = 15;
    let steps = (duration_ms / STEP_MS).max(1);
    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        let x = start_x as f64 + (target_x - start_x) as f64 * t;
        let y = start_y as f64 + (target_y - start_y) as f64 * t;
        let mut inputs = [mouse_input(x.round() as i32, y.round() as i32, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)];
        send(&mut inputs)?;
        std::thread::sleep(std::time::Duration::from_millis(STEP_MS as u64));
    }
    Ok(())
}

/// The current cursor position in logical (DPI-independent) primary-monitor pixels.
pub fn cursor_position() -> Result<(i32, i32), AutomationError> {
    let (x, y) = unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).map_err(|e| AutomationError(format!("GetCursorPos failed: {e}")))?;
        (point.x, point.y)
    };
    let dpi = primary_monitor_dpi()?;
    let scale = dpi as f64 / DEFAULT_DPI as f64;
    Ok(((x as f64 / scale).round() as i32, (y as f64 / scale).round() as i32))
}

fn cursor_position_absolute() -> Result<(i32, i32), AutomationError> {
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).map_err(|e| AutomationError(format!("GetCursorPos failed: {e}")))?;
        to_absolute(point.x, point.y)
    }
}

/// Presses `key` down without releasing it. Supported names: single
/// characters/digits, `enter`, `tab`, `escape`/`esc`, `space`,
/// `backspace`, `delete`, arrow keys (`up`/`down`/`left`/`right`),
/// `home`, `end`, `page_up`, `page_down`, `f1`..`f12`, and the
/// modifier keys `ctrl`, `alt`, `shift`, `win`.
pub fn key_down(key: &str) -> Result<(), AutomationError> {
    let vk = key_from_name(key).ok_or_else(|| AutomationError(format!("unknown key name: '{key}'")))?;
    let mut down = [key_vk_input(vk, false)];
    send(&mut down)
}

/// Releases `key` — see `key_down`'s doc comment for supported names.
pub fn key_up(key: &str) -> Result<(), AutomationError> {
    let vk = key_from_name(key).ok_or_else(|| AutomationError(format!("unknown key name: '{key}'")))?;
    let mut up = [key_vk_input(vk, true)];
    send(&mut up)
}

/// `modifiers`' set fields as `key_down`/`key_up`-compatible names,
/// in a fixed order — shared by every modifier-combo function below
/// so press/release order is always the mirror image of each other
/// (modifiers down outside-in, up inside-out, same convention a
/// physical keyboard combo like Ctrl+Shift+A follows).
fn modifier_key_names(modifiers: &flow_schema::KeyModifiers) -> Vec<&'static str> {
    let mut names = Vec::with_capacity(4);
    if modifiers.ctrl {
        names.push("ctrl");
    }
    if modifiers.alt {
        names.push("alt");
    }
    if modifiers.shift {
        names.push("shift");
    }
    if modifiers.win {
        names.push("win");
    }
    names
}

/// Presses and immediately releases `key` together with `modifiers`
/// — e.g. `key: "a"` with `ctrl: true` sends the Ctrl+A combo as one
/// call, instead of the caller having to sequence `key_down("ctrl")`
/// / `key_down("a")` / `key_up("a")` / `key_up("ctrl")` itself.
pub fn key_tap_combo(key: &str, modifiers: &flow_schema::KeyModifiers) -> Result<(), AutomationError> {
    let mods = modifier_key_names(modifiers);
    for m in &mods {
        key_down(m)?;
    }
    key_down(key)?;
    key_up(key)?;
    for m in mods.iter().rev() {
        key_up(m)?;
    }
    Ok(())
}

/// Presses `key` and `modifiers` down, leaving all of them held —
/// the caller (`engine::runner`) is responsible for eventually
/// calling `key_release_combo` with the same `modifiers` (or relying
/// on the engine's end-of-run force-release safety net).
pub fn key_press_combo(key: &str, modifiers: &flow_schema::KeyModifiers) -> Result<(), AutomationError> {
    for m in modifier_key_names(modifiers) {
        key_down(m)?;
    }
    key_down(key)
}

/// Releases `key` and `modifiers` — the inverse of `key_press_combo`.
pub fn key_release_combo(key: &str, modifiers: &flow_schema::KeyModifiers) -> Result<(), AutomationError> {
    key_up(key)?;
    for m in modifier_key_names(modifiers).into_iter().rev() {
        key_up(m)?;
    }
    Ok(())
}

/// Maps a PAD-style key name to its Win32 virtual-key code.
fn key_from_name(key: &str) -> Option<VIRTUAL_KEY> {
    let lower = key.to_ascii_lowercase();
    Some(match lower.as_str() {
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "escape" | "esc" => VK_ESCAPE,
        "space" => VK_SPACE,
        "backspace" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "home" => VK_HOME,
        "end" => VK_END,
        "page_up" | "pageup" => VK_PRIOR,
        "page_down" | "pagedown" => VK_NEXT,
        "ctrl" | "control" => VK_CONTROL,
        "alt" => VK_MENU,
        "shift" => VK_SHIFT,
        "win" | "windows" => VK_LWIN,
        "f1" => VK_F1,
        "f2" => VK_F2,
        "f3" => VK_F3,
        "f4" => VK_F4,
        "f5" => VK_F5,
        "f6" => VK_F6,
        "f7" => VK_F7,
        "f8" => VK_F8,
        "f9" => VK_F9,
        "f10" => VK_F10,
        "f11" => VK_F11,
        "f12" => VK_F12,
        single if single.chars().count() == 1 => {
            let ch = single.chars().next().unwrap().to_ascii_uppercase();
            if ch.is_ascii_alphanumeric() {
                VIRTUAL_KEY(ch as u16)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn key_vk_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { Default::default() },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Finds a UI element via Windows UI Automation and clicks it. Unlike
/// `click_at`, this survives window moves, DPI changes, and (for
/// standard Windows controls) resizes, because it targets the control
/// itself rather than a screen coordinate.
pub fn click_element(selector: &ElementSelector) -> Result<(), AutomationError> {
    let element = find_element(selector)?;
    element
        .click()
        .map_err(|e| AutomationError(format!("failed to click element: {e}")))
}

/// Reads text out of a UI element — its editable value (a text box's
/// contents, say) when it has one, falling back to its accessible
/// name (a label's or button's caption) otherwise. Whichever it
/// returns, this is the one "read something off the screen into a
/// variable" primitive; the caller decides what to do with the text.
pub fn get_element_text(selector: &ElementSelector) -> Result<String, AutomationError> {
    let element = find_element(selector)?;
    if let Ok(pattern) = element.get_pattern::<uiautomation::patterns::UIValuePattern>() {
        if let Ok(value) = pattern.get_value() {
            return Ok(value);
        }
    }
    element
        .get_name()
        .map_err(|e| AutomationError(format!("failed to read element text: {e}")))
}

/// What "hover the cursor over a control, then look here" turns into —
/// enough for the caller to both fill in an `ElementSelector` and show
/// the user a human-readable confirmation of what got picked.
pub struct PickedUiElement {
    pub window_title: String,
    pub element_name: String,
    pub automation_id: String,
    pub preview: String,
}

/// Sleeps `delay_ms` (giving the user time to move the mouse over the
/// control they mean, mirroring `cursor_position_after_delay`'s
/// countdown), then hit-tests whatever's under the cursor via UI
/// Automation. Mirrors PAD's element picker without needing a global
/// mouse hook — just "wait, then look at where the cursor ended up".
pub fn pick_ui_element_after_delay(delay_ms: u64) -> Result<PickedUiElement, AutomationError> {
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));

    let (x, y) = unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).map_err(|e| AutomationError(format!("GetCursorPos failed: {e}")))?;
        (point.x, point.y)
    };

    let automation =
        UIAutomation::new().map_err(|e| AutomationError(format!("UI Automation init failed: {e}")))?;
    let element = automation
        .element_from_point(uiautomation::types::Point::new(x, y))
        .map_err(|e| AutomationError(format!("no UI element under the cursor: {e}")))?;

    // Walking Relay's own UI Automation tree (its WebView2) from this
    // background thread while the app's own window is mid-render is
    // what was crashing the picker when the user focused Relay during
    // the countdown — refuse instead of touching it.
    if element.get_process_id().unwrap_or(0) as u32 == std::process::id() {
        return Err(AutomationError(
            "the cursor was over Relay's own window — point at the target application instead".into(),
        ));
    }

    let element_name = element.get_name().unwrap_or_default();
    let automation_id = element.get_automation_id().unwrap_or_default();
    let control_type = element
        .get_localized_control_type()
        .or_else(|_| element.get_classname())
        .unwrap_or_default();

    // `uiautomation` pulls in its own `windows` crate version, so its
    // `Handle` doesn't convert directly to *our* `windows` crate's
    // `HWND` — round-trip through the raw pointer value instead.
    let handle_ptr: isize = element.get_native_window_handle().map(Into::into).unwrap_or_default();
    let hwnd = HWND(handle_ptr as *mut std::ffi::c_void);
    let window_title = unsafe { window_text(GetAncestor(hwnd, GA_ROOT)) };

    let preview = if element_name.is_empty() {
        control_type
    } else {
        format!("{control_type} \"{element_name}\"")
    };

    Ok(PickedUiElement { window_title, element_name, automation_id, preview })
}

unsafe fn window_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let copied = GetWindowTextW(hwnd, &mut buf);
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..copied as usize])
}

/// `CREATE_NO_WINDOW` — for a console helper this crate shells out to
/// on the user's behalf (`ping`, `tasklist`, ...), not one the flow is
/// deliberately launching as a visible target app (`launch_app`
/// itself never uses this). Without it, `Command` still allocates a
/// console for a console-subsystem child, so a plain `cmd`/
/// `powershell` call flashes a real window on screen for however long
/// it runs — invisible to the flow author's log, but very visible on
/// their desktop.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden_command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Starts a new process at `path`. `args` is split on whitespace and
/// passed through as-is — no shell, no quoting rules to worry about,
/// just argv the way `Command` expects it.
pub fn launch_app(path: &str, args: &str) -> Result<(), AutomationError> {
    std::process::Command::new(path)
        .args(args.split_whitespace())
        .spawn()
        .map(|_| ())
        .map_err(|e| AutomationError(format!("failed to launch '{path}': {e}")))
}

/// An installed Chromium-family browser `LaunchBrowser` can spawn a
/// fresh instance of — `id` is what a flow's `browser` field stores
/// ("chrome"/"edge"), `path` is the exe `launch_browser_instance`
/// actually spawns.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Every well-known install location for each browser this crate
/// knows how to launch, checked in order — the first one that exists
/// on disk wins. Only Chromium-family browsers are listed: Firefox's
/// extension APIs differ enough from Chrome's that the bundled Relay
/// Bridge extension (a Manifest V3 Chrome extension) wouldn't load
/// into it unmodified.
fn browser_candidates() -> Vec<(&'static str, &'static str, &'static str, Vec<String>)> {
    let program_files = std::env::var("ProgramFiles").unwrap_or_default();
    let program_files_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_default();
    let local_app_data = std::env::var("LocalAppData").unwrap_or_default();
    vec![
        (
            "chrome",
            "Google Chrome",
            "chrome.exe",
            vec![
                format!(r"{program_files}\Google\Chrome\Application\chrome.exe"),
                format!(r"{program_files_x86}\Google\Chrome\Application\chrome.exe"),
                format!(r"{local_app_data}\Google\Chrome\Application\chrome.exe"),
            ],
        ),
        (
            "edge",
            "Microsoft Edge",
            "msedge.exe",
            vec![
                format!(r"{program_files_x86}\Microsoft\Edge\Application\msedge.exe"),
                format!(r"{program_files}\Microsoft\Edge\Application\msedge.exe"),
            ],
        ),
        (
            "brave",
            "Brave",
            "brave.exe",
            vec![
                format!(r"{program_files}\BraveSoftware\Brave-Browser\Application\brave.exe"),
                format!(r"{program_files_x86}\BraveSoftware\Brave-Browser\Application\brave.exe"),
                format!(r"{local_app_data}\BraveSoftware\Brave-Browser\Application\brave.exe"),
            ],
        ),
        (
            "comet",
            "Comet",
            "comet.exe",
            vec![format!(r"{local_app_data}\Perplexity\Comet\Application\comet.exe")],
        ),
        (
            "vivaldi",
            "Vivaldi",
            "vivaldi.exe",
            vec![
                format!(r"{local_app_data}\Vivaldi\Application\vivaldi.exe"),
                format!(r"{program_files}\Vivaldi\Application\vivaldi.exe"),
            ],
        ),
        (
            "opera",
            "Opera",
            "opera.exe",
            vec![
                format!(r"{local_app_data}\Programs\Opera\opera.exe"),
                format!(r"{program_files}\Opera\opera.exe"),
            ],
        ),
        (
            "arc",
            "Arc",
            "Arc.exe",
            vec![format!(r"{local_app_data}\Programs\Arc\Arc.exe")],
        ),
    ]
}

/// Looks up `exe_filename` (e.g. `"chrome.exe"`) in the registry's
/// `App Paths` key — the same mechanism Windows itself uses to
/// resolve a bare exe name typed into Run or the shell, and the way
/// most Chromium-based installers (including ones at custom install
/// locations or per-user installs) register themselves regardless of
/// where their files actually live. Checked before the hardcoded
/// path list below so a non-default install location is still found;
/// the hardcoded paths remain only as a fallback for the rare
/// installer that skips this registration.
#[cfg(windows)]
fn app_path_from_registry(exe_filename: &str) -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let subkey = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe_filename}");
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(key) = RegKey::predef(hive).open_subkey(&subkey) {
            if let Ok(path) = key.get_value::<String, _>("") {
                if std::path::Path::new(&path).is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn app_path_from_registry(_exe_filename: &str) -> Option<String> {
    None
}

/// An explicit override for a browser's exe path, so a browser
/// installed somewhere neither the registry nor the hardcoded
/// fallback list can find it (a portable build, an unusual install
/// location) can still be used — set `RELAY_BROWSER_<ID>_PATH`
/// (e.g. `RELAY_BROWSER_COMET_PATH`) to the full exe path.
fn app_path_from_env_override(id: &str) -> Option<String> {
    let var = format!("RELAY_BROWSER_{}_PATH", id.to_uppercase());
    std::env::var(var).ok().filter(|p| std::path::Path::new(p).is_file())
}

/// Parses `RELAY_CUSTOM_BROWSERS` — `id:Display Name:C:\path\to.exe`
/// entries separated by `;` — into `BrowserInfo`s for browsers this
/// app has no built-in knowledge of at all (an unlisted Chromium
/// fork, a portable build, one installed somewhere `find_installed_browsers`'s
/// registry/hardcoded lookups don't check). Complements
/// `RELAY_BROWSER_<ID>_PATH` (which only *redirects* one of the
/// already-known ids above) by letting an entirely new id be
/// registered — the only way to reach a browser this app's built-in
/// candidate list doesn't name, short of a code change. Entries whose
/// path doesn't exist, or that collide with a built-in id, are
/// skipped rather than erroring — a stale/malformed entry in this env
/// var shouldn't break browser detection for everything else.
fn custom_browsers_from_env() -> Vec<BrowserInfo> {
    let Ok(spec) = std::env::var("RELAY_CUSTOM_BROWSERS") else {
        return Vec::new();
    };
    let known_ids: Vec<&str> = browser_candidates().into_iter().map(|(id, ..)| id).collect();
    spec.split(';')
        .filter(|entry| !entry.trim().is_empty())
        .filter_map(|entry| {
            let mut parts = entry.splitn(3, ':');
            let id = parts.next()?.trim();
            let name = parts.next()?.trim();
            let path = parts.next()?.trim();
            if id.is_empty() || name.is_empty() || path.is_empty() || known_ids.contains(&id) || !std::path::Path::new(path).is_file() {
                return None;
            }
            Some(BrowserInfo { id: id.into(), name: name.into(), path: path.into() })
        })
        .collect()
}

/// Every Chromium-family browser actually found installed on this
/// machine, in a stable preference order (Chrome, then Edge, then any
/// `RELAY_CUSTOM_BROWSERS` entries) — feeds the Inspector's browser
/// picker for `LaunchBrowser`. For each built-in candidate, tries (in
/// order) an explicit env-var override, the registry's `App Paths`
/// registration, then the hardcoded well-known install locations.
pub fn find_installed_browsers() -> Vec<BrowserInfo> {
    browser_candidates()
        .into_iter()
        .filter_map(|(id, name, exe_filename, fallback_paths)| {
            let path = app_path_from_env_override(id)
                .or_else(|| app_path_from_registry(exe_filename))
                .or_else(|| fallback_paths.into_iter().find(|p| std::path::Path::new(p).is_file()))?;
            Some(BrowserInfo { id: id.into(), name: name.into(), path })
        })
        .chain(custom_browsers_from_env())
        .collect()
}

/// Resolves `browser` (a `BrowserInfo::id`) to its exe path — falls
/// back to the first installed browser found only when `browser`
/// itself is `None`/empty (an unset/stale-saved choice), but fails
/// loudly if it names a specific browser that isn't actually
/// installed, rather than silently launching a different one instead.
fn resolve_browser(browser: Option<&str>) -> Result<BrowserInfo, AutomationError> {
    let installed = find_installed_browsers();
    if let Some(id) = browser.filter(|s| !s.trim().is_empty()) {
        // A specific browser was asked for by id — finding a
        // *different* one instead and silently launching that would
        // send a `LaunchBrowser` step's automation into a browser the
        // flow never asked for (no extension connected there yet,
        // possibly a different logged-in session entirely), with
        // nothing about the failure mode hinting that's what
        // happened. Fail loudly instead of guessing.
        return installed
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| AutomationError(format!("'{id}' isn't installed (or isn't a browser Relay recognizes) on this machine")));
    }
    installed
        .into_iter()
        .next()
        .ok_or_else(|| AutomationError("no supported browser (Chrome or Edge) found installed on this machine".into()))
}

/// Gets a connection to `browser` — reusing an already-connected one
/// for the default-profile case (see below), or else spawning a
/// browser window and waiting for its *already-installed* Relay
/// Bridge extension to connect on its own — then explicitly asks it
/// to open `url` in a fresh tab via `open_tab` rather than passing
/// `url` on the browser's own command line: a command-line URL lands
/// in *whatever tab the browser decides is active*, which is
/// unambiguous for a genuinely fresh process but not when several
/// `LaunchBrowser` steps end up sharing one reused connection — they'd
/// otherwise race over which of them the URL actually opened for.
/// Returns `"<connection>#<tabId>"` — later `Browser*` steps'
/// `instance` field holds this whole string, split back apart by
/// `resolve_instance` to route to the right connection *and* the
/// right tab within it, every time, without re-identifying anything —
/// all the "which connection is this, really" work happens once,
/// right here, not on every later step.
///
/// This used to also pre-load the extension into a freshly spawned
/// process via `--load-extension`, so a completely cold-started
/// browser could bootstrap itself with no manual step at all. Chrome
/// 137 removed that flag from official builds entirely (abused too
/// often by malware sideloading extensions this same way) with no
/// direct replacement short of driving the browser over the DevTools
/// protocol in pipe mode — real scope, deliberately not taken on
/// here. The Relay Bridge extension now has to already be installed
/// in whichever profile this spawns/reuses; once it is, Chrome
/// activates it on every normal startup on its own (this crate's
/// `background.js` connects from its own `onStartup` listener),
/// same as any other installed extension — no flag needed for that
/// part at all.
pub fn launch_browser_instance(url: &str, browser: Option<&str>, profile_dir: Option<&str>) -> Result<String, AutomationError> {
    let target = resolve_browser(browser)?;
    let custom_profile = profile_dir.filter(|s| !s.trim().is_empty());

    // Empty/unset `profile_dir` means "just use this browser's own
    // normal default profile" — the same one the user already
    // browses with day to day, extension and all, rather than a
    // fresh Relay-owned one. When that's the case, check whether this
    // exact browser already has a connection open (identified by its
    // real OS process, not a guess — see `browser_bridge::identify`)
    // and reuse it directly instead of spawning another process that
    // would just silently delegate to that same running instance
    // anyway (Chrome refuses to start a second one against a
    // `--user-data-dir` already in use). Only the *first*
    // `LaunchBrowser` step for a given default-profile browser in a
    // run actually spawns anything; every one after it reuses this.
    if custom_profile.is_none() {
        if let Some(connection) = browser_bridge::find_connection_for_browser(&target.id) {
            return open_tab_in(&connection, url);
        }
    }

    let mut args = vec!["--new-window".to_string()];
    // Set `profile_dir` explicitly to opt into an isolated, dedicated
    // profile instead of the real default one above. Omitting
    // `--user-data-dir` is what actually selects the default profile;
    // passing anything here, even the "real" default path, makes
    // Chrome treat it as a *separate* profile from the one already
    // running. Note this only makes sense once the extension has
    // already been loaded into that specific profile by hand at least
    // once — a brand new, never-used profile directory has nothing
    // installed in it yet, and nothing here can install it anymore.
    if let Some(custom) = custom_profile {
        std::fs::create_dir_all(custom).map_err(|e| AutomationError(format!("couldn't create browser profile directory: {e}")))?;
        args.push(format!("--user-data-dir={custom}"));
    }
    args.push("--no-first-run".to_string());

    let connection = browser_bridge::spawn_instance(&target.path, &args, std::time::Duration::from_secs(20), &target.id).map_err(AutomationError)?;
    open_tab_in(&connection, url)
}

/// Asks `connection`'s extension to open `url` in a fresh tab (rather
/// than relying on wherever a command-line URL happened to land — see
/// `launch_browser_instance`'s doc comment) and returns
/// `"<connection>#<tabId>"`, the address a later `Browser*` step's
/// `instance` field holds to reach that exact tab specifically.
fn open_tab_in(connection: &str, url: &str) -> Result<String, AutomationError> {
    let result = browser_bridge::send_command(Some(connection), "open_tab", serde_json::json!({ "url": url, "active": true }))
        .map_err(AutomationError)?;
    let tab_id = result.as_i64().ok_or_else(|| AutomationError("browser extension returned an unexpected reply".into()))?;
    Ok(format!("{connection}#{tab_id}"))
}

/// Opens `url` in the user's default browser via the shell — the
/// desktop-automation equivalent of double-clicking a link, not a
/// browser-DOM automation primitive (driving page content needs a
/// WebDriver/CDP integration, which this crate doesn't have).
pub fn open_url(url: &str) -> Result<(), AutomationError> {
    // Deliberately not `cmd /C start ... url` — cmd.exe re-parses its
    // own received command line and treats `&`, `|`, `^`, and friends
    // as shell metacharacters *inside* a quoted argument, not just at
    // the top level, so a flow-supplied URL containing one of those
    // (an ordinary query string separator like `&`, or something a
    // flow read from a file/webpage/HTTP response) could inject and
    // run an entirely different command. `ShellExecuteW` with the
    // `"open"` verb goes straight to the shell's URL/file handler —
    // the same path double-clicking a link takes — without cmd.exe
    // or any other command-line reinterpretation in between.
    let url_wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let verb_wide: Vec<u16> = "open\0".encode_utf16().collect();
    unsafe {
        let result = ShellExecuteW(
            None,
            windows::core::PCWSTR::from_raw(verb_wide.as_ptr()),
            windows::core::PCWSTR::from_raw(url_wide.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
        // See `relaunch_elevated`'s comment on this same check —
        // ShellExecuteW packs a failure code (<= 32) into the
        // HINSTANCE return slot instead of using an HRESULT.
        if (result.0 as isize) <= 32 {
            return Err(AutomationError(format!("failed to open '{url}'")));
        }
    }
    Ok(())
}

/// Reads a file's entire contents as UTF-8 text into a variable — the
/// file-system counterpart of `GetElementText`/`BrowserGetText`. Not
/// encoding-aware (no legacy codepage/BOM handling) since every other
/// text value in a flow is already plain UTF-8.
pub fn read_file(path: &str) -> Result<String, AutomationError> {
    std::fs::read_to_string(path).map_err(|e| AutomationError(format!("failed to read '{path}': {e}")))
}

/// Writes `content` to `path`, creating it if it doesn't exist —
/// overwrites by default, or appends when `append` is set.
pub fn write_file(path: &str, content: &str, append: bool) -> Result<(), AutomationError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|e| AutomationError(format!("failed to open '{path}' for writing: {e}")))?;
    file.write_all(content.as_bytes())
        .map_err(|e| AutomationError(format!("failed to write to '{path}': {e}")))
}

pub fn copy_file(source: &str, destination: &str) -> Result<(), AutomationError> {
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|e| AutomationError(format!("failed to copy '{source}' to '{destination}': {e}")))
}

/// Moves (or, within the same directory, renames) a file. Fails if
/// `destination` already exists — `std::fs::rename`'s own behavior,
/// not something worth silently overriding for a file operation the
/// user may not expect to clobber something.
pub fn move_file(source: &str, destination: &str) -> Result<(), AutomationError> {
    std::fs::rename(source, destination)
        .map_err(|e| AutomationError(format!("failed to move '{source}' to '{destination}': {e}")))
}

pub fn delete_file(path: &str) -> Result<(), AutomationError> {
    std::fs::remove_file(path).map_err(|e| AutomationError(format!("failed to delete '{path}': {e}")))
}

/// Creates `path`, including any missing parent directories — a no-op
/// (not an error) if it already exists.
pub fn create_directory(path: &str) -> Result<(), AutomationError> {
    std::fs::create_dir_all(path).map_err(|e| AutomationError(format!("failed to create directory '{path}': {e}")))
}

/// Lists `path`'s immediate entries (files and subdirectories, not
/// recursive) as a newline-joined, alphabetically sorted string —
/// every other value a flow carries is a plain string variable (there
/// is no array/list variable type), so this is the same "one string"
/// shape as `read_file`, not a structured result.
pub fn list_directory(path: &str) -> Result<String, AutomationError> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map_err(|e| AutomationError(format!("failed to list directory '{path}': {e}")))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names.join("\n"))
}

/// Sends an HTTP request and returns `(response body, status code)`.
/// `headers` is a simple `Name: Value` per line format (matching the
/// flat, plain-text-only shape every other flow value already
/// uses — no array/object variable type to hold a real header list),
/// blank lines and lines without a `:` are ignored. Uses a fresh
/// `reqwest::blocking::Client` per call rather than a shared/pooled
/// one — flow HTTP steps are occasional, not a hot loop, so the
/// simplicity is worth more than reused connections here.
pub fn http_request(
    method: flow_schema::HttpMethod,
    url: &str,
    headers: &str,
    body: &str,
) -> Result<(String, u16), AutomationError> {
    use flow_schema::HttpMethod;

    let client = reqwest::blocking::Client::new();
    let mut builder = match method {
        HttpMethod::Get => client.get(url),
        HttpMethod::Post => client.post(url),
        HttpMethod::Put => client.put(url),
        HttpMethod::Patch => client.patch(url),
        HttpMethod::Delete => client.delete(url),
    };
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            continue;
        }
        builder = builder.header(name, value);
    }
    if !body.is_empty() {
        builder = builder.body(body.to_string());
    }

    let response = builder
        .send()
        .map_err(|e| AutomationError(format!("HTTP request to '{url}' failed: {e}")))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .map_err(|e| AutomationError(format!("failed to read the response body from '{url}': {e}")))?;
    Ok((text, status))
}

/// Sends a GET request and streams the response body straight to
/// `path` — unlike `http_request`, never buffers the whole body in
/// memory first, so this scales to a large download the same way it
/// would to a small one. Returns the numeric status code.
pub fn http_download(url: &str, headers: &str, path: &str) -> Result<u16, AutomationError> {
    let client = reqwest::blocking::Client::new();
    let mut builder = client.get(url);
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            continue;
        }
        builder = builder.header(name, value);
    }
    let mut response = builder
        .send()
        .map_err(|e| AutomationError(format!("HTTP request to '{url}' failed: {e}")))?;
    let status = response.status().as_u16();
    let mut file = std::fs::File::create(path).map_err(|e| AutomationError(format!("failed to create '{path}': {e}")))?;
    response
        .copy_to(&mut file)
        .map_err(|e| AutomationError(format!("failed to save the download to '{path}': {e}")))?;
    Ok(status)
}

/// Decodes a `data:...;base64,...` URL (what `chrome.tabs.captureVisibleTab`
/// returns) and writes the decoded bytes to `path` — the backend half
/// of `BrowserScreenshot`.
pub fn save_data_url_to_file(data_url: &str, path: &str) -> Result<(), AutomationError> {
    let base64_data = data_url.split_once(',').map(|(_, data)| data).unwrap_or(data_url);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| AutomationError(format!("failed to decode the screenshot data: {e}")))?;
    std::fs::write(path, bytes).map_err(|e| AutomationError(format!("failed to save the screenshot to '{path}': {e}")))
}

/// Whether `host` responded to a single ICMP echo (`ping`) within
/// `timeout_ms`, and if so, how long it took — shells out to
/// Windows' own `ping.exe` rather than crafting raw ICMP packets
/// directly, since that needs no elevated/raw-socket privileges and
/// "does the same thing the `ping` command does" is exactly the
/// expected behavior here.
pub struct PingResult {
    pub reachable: bool,
    pub latency_ms: Option<u32>,
}

pub fn ping(host: &str, timeout_ms: u32) -> Result<PingResult, AutomationError> {
    let output = hidden_command("ping")
        .args(["-n", "1", "-w", &timeout_ms.to_string(), host])
        .output()
        .map_err(|e| AutomationError(format!("failed to run ping: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !text.contains("TTL=") && !text.contains("ttl=") {
        return Ok(PingResult { reachable: false, latency_ms: None });
    }
    let latency_ms = text
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            let after = lower.split("time").nth(1)?;
            let digits: String = after.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok()
        })
        .or(Some(0));
    Ok(PingResult { reachable: true, latency_ms })
}

/// Resolves `hostname` to its first IP address via the system's
/// normal DNS resolution (`getaddrinfo` under the hood, via Rust's
/// standard library — no separate DNS client needed).
pub fn dns_lookup(hostname: &str) -> Result<String, AutomationError> {
    use std::net::ToSocketAddrs;
    let mut addrs = (hostname, 0)
        .to_socket_addrs()
        .map_err(|e| AutomationError(format!("failed to resolve '{hostname}': {e}")))?;
    addrs
        .next()
        .map(|addr| addr.ip().to_string())
        .ok_or_else(|| AutomationError(format!("'{hostname}' did not resolve to any address")))
}

/// Reads environment variable `name` — fails if it isn't set (an
/// absent variable and one set to an empty string are different
/// things, and conflating them would hide a real misconfiguration).
pub fn get_env_var(name: &str) -> Result<String, AutomationError> {
    std::env::var(name).map_err(|_| AutomationError(format!("environment variable '{name}' is not set")))
}

/// Whether at least one running process is named `name` (matched
/// case-insensitively) — shells out to `tasklist` rather than
/// enumerating processes via the Win32 API directly, since parsing
/// its well-defined CSV output is simpler and just as reliable.
pub fn check_process(name: &str) -> Result<bool, AutomationError> {
    let output = hidden_command("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
        .output()
        .map_err(|e| AutomationError(format!("failed to run tasklist: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    // A match prints a CSV row starting with `"name.exe",...`; no
    // match prints an "INFO: No tasks..." line instead — checking for
    // the quoted name is simpler than parsing CSV for this.
    Ok(text.to_ascii_lowercase().contains(&format!("\"{}\"", name.to_ascii_lowercase())))
}

/// Ends every running process named `name`. `force` (`taskkill /F`)
/// skips asking each process to close itself first.
pub fn kill_process(name: &str, force: bool) -> Result<(), AutomationError> {
    let mut args = vec!["/IM".to_string(), name.to_string()];
    if force {
        args.push("/F".to_string());
    }
    let output = hidden_command("taskkill")
        .args(&args)
        .output()
        .map_err(|e| AutomationError(format!("failed to run taskkill: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // taskkill exits non-zero when there's simply nothing to kill —
    // not a real failure for a step whose whole point is "make sure
    // this isn't running".
    if stderr.to_ascii_lowercase().contains("not found") {
        return Ok(());
    }
    Err(AutomationError(format!("failed to end process '{name}': {}", stderr.trim())))
}

/// Polls until `path` exists on disk, or fails after `timeout_ms` —
/// see `flow_schema::Action::WaitForFile`'s doc comment.
pub fn wait_for_file(path: &str, timeout_ms: u32) -> Result<(), AutomationError> {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        if std::path::Path::new(path).exists() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(AutomationError(format!("'{path}' did not appear within {timeout_ms}ms")));
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// A random integer in `[min, max]`, inclusive of both ends. `min`
/// and `max` are already resolved text at this point (the same
/// "parse a number out of a string" shape `Calculate`'s operands
/// use); swapped automatically if `min > max` rather than failing —
/// treating whichever bound happens to be larger as the max is more
/// useful than erroring over it.
pub fn generate_random(min: &str, max: &str) -> Result<i64, AutomationError> {
    use rand::Rng;
    let a: i64 = min.trim().parse().map_err(|_| AutomationError(format!("'{min}' is not a whole number")))?;
    let b: i64 = max.trim().parse().map_err(|_| AutomationError(format!("'{max}' is not a whole number")))?;
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    Ok(rand::thread_rng().gen_range(lo..=hi))
}

/// Locks the workstation — same effect as Win+L.
pub fn lock_workstation() -> Result<(), AutomationError> {
    use windows::Win32::System::Shutdown::LockWorkStation;
    unsafe { LockWorkStation() }.map_err(|e| AutomationError(format!("failed to lock the workstation: {e}")))
}

/// `ExitWindowsEx` needs `SE_SHUTDOWN_NAME` enabled on this process's
/// token first — not granted by default even to an interactive user's
/// process. Shared by `shutdown`/`restart` rather than each
/// duplicating the privilege dance.
fn enable_shutdown_privilege() -> Result<(), AutomationError> {
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_SHUTDOWN_NAME,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)
            .map_err(|e| AutomationError(format!("failed to open this process's token: {e}")))?;

        let mut luid = LUID::default();
        let lookup = LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid);
        if lookup.is_err() {
            let _ = CloseHandle(token);
            return Err(AutomationError(format!("failed to look up the shutdown privilege: {}", lookup.unwrap_err())));
        }

        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let adjust = AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None);
        let _ = CloseHandle(token);
        adjust.map_err(|e| AutomationError(format!("failed to enable the shutdown privilege: {e}")))
    }
}

/// Shuts down the machine. `force` closes apps that would otherwise
/// block shutdown by prompting to save unsaved work — off by default
/// so an automated flow doesn't silently discard something the user
/// would have wanted to keep.
pub fn shutdown(force: bool) -> Result<(), AutomationError> {
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, EWX_FORCEIFHUNG, EWX_SHUTDOWN, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION,
        SHTDN_REASON_MINOR_OTHER,
    };
    enable_shutdown_privilege()?;
    let flags = if force { EWX_SHUTDOWN | EWX_FORCEIFHUNG } else { EWX_SHUTDOWN };
    unsafe { ExitWindowsEx(flags, SHTDN_REASON_MAJOR_APPLICATION | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED) }
        .map_err(|e| AutomationError(format!("failed to shut down: {e}")))
}

/// Restarts the machine — see `shutdown` for what `force` does.
pub fn restart(force: bool) -> Result<(), AutomationError> {
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, EWX_FORCEIFHUNG, EWX_REBOOT, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION,
        SHTDN_REASON_MINOR_OTHER,
    };
    enable_shutdown_privilege()?;
    let flags = if force { EWX_REBOOT | EWX_FORCEIFHUNG } else { EWX_REBOOT };
    unsafe { ExitWindowsEx(flags, SHTDN_REASON_MAJOR_APPLICATION | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED) }
        .map_err(|e| AutomationError(format!("failed to restart: {e}")))
}

/// Encodes a Rust string as a NUL-terminated UTF-16 buffer for
/// passing to a WinAPI call expecting a `PCWSTR` — the caller must
/// keep the returned `Vec` alive for as long as the pointer is used.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Shows a blocking message box with an OK button — blocks the
/// calling thread (the engine's execution thread, not the UI thread)
/// until the user dismisses it, which is exactly the "pause the flow
/// until acknowledged" behavior a flow author wants.
pub fn show_message(title: &str, message: &str) -> Result<(), AutomationError> {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW};
    let title_w = wide(title);
    let message_w = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(message_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
        );
    }
    Ok(())
}

/// Shows a blocking Yes/No prompt, returning `true` for Yes.
pub fn show_confirm(title: &str, message: &str) -> Result<bool, AutomationError> {
    use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW};
    let title_w = wide(title);
    let message_w = wide(message);
    let result = unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(message_w.as_ptr()),
            PCWSTR::from_raw(title_w.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    Ok(result == IDYES)
}

/// Shows a blocking text-input prompt pre-filled with `default_value`.
/// Win32 has no built-in input box, so this shells out to a hidden
/// PowerShell process running a small WinForms dialog — a real native
/// dialog, just spawned rather than drawn directly. Built by hand
/// (rather than the one-line `Microsoft.VisualBasic.Interaction.InputBox`)
/// specifically so it can be marked `TopMost` and `Activate()`d — the
/// VB helper has no way to control that, so it could get lost behind
/// whatever window the flow's own automation had focused right
/// before this step, indistinguishable from the flow having silently
/// stalled. Returns `default_value` unchanged if the user cancels.
pub fn show_input(title: &str, message: &str, default_value: &str) -> Result<String, AutomationError> {
    fn escape_ps_single_quoted(s: &str) -> String {
        s.replace('\'', "''")
    }
    let script = format!(
        r#"Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.Text = '{title}'
$form.Width = 420
$form.Height = 160
$form.StartPosition = 'CenterScreen'
$form.TopMost = $true
$form.FormBorderStyle = 'FixedDialog'
$form.MaximizeBox = $false
$form.MinimizeBox = $false
$label = New-Object System.Windows.Forms.Label
$label.Text = '{message}'
$label.SetBounds(12, 12, 380, 40)
$form.Controls.Add($label)
$textbox = New-Object System.Windows.Forms.TextBox
$textbox.SetBounds(12, 55, 380, 22)
$textbox.Text = '{default_value}'
$form.Controls.Add($textbox)
$okButton = New-Object System.Windows.Forms.Button
$okButton.Text = 'OK'
$okButton.SetBounds(230, 90, 75, 25)
$okButton.DialogResult = [System.Windows.Forms.DialogResult]::OK
$form.Controls.Add($okButton)
$form.AcceptButton = $okButton
$cancelButton = New-Object System.Windows.Forms.Button
$cancelButton.Text = 'Cancel'
$cancelButton.SetBounds(315, 90, 75, 25)
$cancelButton.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
$form.Controls.Add($cancelButton)
$form.CancelButton = $cancelButton
$form.Add_Shown({{ $form.Activate(); $textbox.Focus(); $textbox.SelectAll() }})
$result = $form.ShowDialog()
if ($result -eq [System.Windows.Forms.DialogResult]::OK) {{
  [Console]::Out.Write($textbox.Text)
}} else {{
  [Console]::Out.Write('{default_value}')
}}"#,
        title = escape_ps_single_quoted(title),
        message = escape_ps_single_quoted(message),
        default_value = escape_ps_single_quoted(default_value),
    );
    let output = hidden_command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| AutomationError(format!("failed to show the input prompt: {e}")))?;
    if !output.status.success() {
        return Err(AutomationError(format!(
            "the input prompt failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.is_empty() { Ok(default_value.to_string()) } else { Ok(text) }
}

/// Brings the top-level window titled `title` to the foreground and
/// gives it keyboard focus — the desktop-native counterpart of
/// `WaitForWindow`, for a flow that needs to make sure the right
/// window has focus before typing/clicking into it.
pub fn focus_window(window: &flow_schema::WindowSelector) -> Result<(), AutomationError> {
    let element = resolve_window(window)?;
    element.set_focus().map_err(|e| AutomationError(format!("failed to focus window ({}): {e}", describe_window_selector(window))))
}

/// A window this app can see on the desktop right now — the
/// "OBS-style pick from a live list" the window-target field's own
/// picker button shows, and what it fills the field in with once you
/// click one. `process_name` is empty when the owning process
/// couldn't be resolved (rare — usually means it exited between
/// enumerating the window and looking its PID up).
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowInfo {
    pub title: String,
    pub process_name: String,
}

/// Every top-level, visible, titled window currently on the desktop —
/// what the window-target picker shows. Deliberately not using
/// UI Automation's own tree walk here (unlike `resolve_window` below):
/// a plain `EnumWindows` pass is far cheaper for "list everything"
/// than asking UIA to build accessibility info for every top-level
/// window just to read its title and owning process.
pub fn list_windows() -> Result<Vec<WindowInfo>, AutomationError> {
    let pid_names = process_name_map()?;
    Ok(enumerate_top_level_windows()
        .into_iter()
        .map(|w| WindowInfo { title: w.title, process_name: pid_names.get(&w.pid).cloned().unwrap_or_default() })
        .collect())
}

struct EnumeratedWindow {
    hwnd: windows::Win32::Foundation::HWND,
    pid: u32,
    title: String,
}

/// Raw `EnumWindows` pass — every visible top-level window with a
/// non-empty title (owned/tool windows, which don't get their own
/// taskbar entry either, are skipped, matching what a user would
/// actually recognize as "an open window").
fn enumerate_top_level_windows() -> Vec<EnumeratedWindow> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER};

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            // A window with an owner isn't a top-level app window a
            // user would pick from a list — a tooltip, a dropdown,
            // most dialog boxes.
            if GetWindow(hwnd, GW_OWNER).map(|owner| !owner.is_invalid()).unwrap_or(false) {
                return true.into();
            }
            let len = GetWindowTextLengthW(hwnd);
            if len == 0 {
                return true.into();
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let actual = GetWindowTextW(hwnd, &mut buf);
            if actual == 0 {
                return true.into();
            }
            let title = String::from_utf16_lossy(&buf[..actual as usize]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let windows = &mut *(lparam.0 as *mut Vec<EnumeratedWindow>);
            windows.push(EnumeratedWindow { hwnd, pid, title });
            true.into()
        }
    }

    let mut windows: Vec<EnumeratedWindow> = Vec::new();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(Some(enum_proc), LPARAM(&mut windows as *mut _ as isize));
    }
    windows
}

/// pid -> exe filename (e.g. `"notepad.exe"`), for every currently
/// running process — shells out to `tasklist` once and parses its CSV
/// output, the same approach `check_process`/`kill_process` already
/// use rather than walking the process table via raw Win32 calls.
fn process_name_map() -> Result<std::collections::HashMap<u32, String>, AutomationError> {
    let output = hidden_command("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|e| AutomationError(format!("failed to run tasklist: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        // Each row: "name.exe","1234","Console","1","10,000 K"
        let fields: Vec<&str> = line.split("\",\"").collect();
        let (Some(name), Some(pid_field)) = (fields.first(), fields.get(1)) else { continue };
        let name = name.trim_start_matches('"');
        if let Ok(pid) = pid_field.parse::<u32>() {
            map.insert(pid, name.to_string());
        }
    }
    Ok(map)
}

fn describe_window_selector(selector: &flow_schema::WindowSelector) -> String {
    use flow_schema::{WindowSelector, WindowSelectorSpec};
    match selector {
        WindowSelector::Title(title) => format!("title \"{title}\""),
        WindowSelector::Other(WindowSelectorSpec::TitleContains { text }) => format!("title containing \"{text}\""),
        WindowSelector::Other(WindowSelectorSpec::Process { process_name }) => format!("process \"{process_name}\""),
        WindowSelector::Other(WindowSelectorSpec::TitleThenProcess { title, process_name }) => {
            format!("title \"{title}\" (or process \"{process_name}\")")
        }
    }
}

/// Finds the window `selector` describes, as a UI Automation element
/// ready to focus/search within — a single, non-waiting check;
/// `wait_for_window` below is what actually polls. Each mode maps to
/// a different real-world durability tradeoff: `Title` is exact and
/// cheap but breaks the instant the title changes at all;
/// `TitleContains` survives an appended/prepended suffix but not a
/// title that changes completely; `Process` survives any title change
/// but can't tell two windows of the same app apart; `TitleThenProcess`
/// — what the picker actually produces — gets the precision of an
/// exact match when it still holds, without breaking outright once it
/// doesn't.
fn resolve_window(selector: &flow_schema::WindowSelector) -> Result<uiautomation::UIElement, AutomationError> {
    use flow_schema::{WindowSelector, WindowSelectorSpec};
    let automation = UIAutomation::new().map_err(|e| AutomationError(format!("UI Automation init failed: {e}")))?;
    match selector {
        WindowSelector::Title(title) => automation
            .create_matcher()
            .name(title)
            .timeout(300)
            .find_first()
            .map_err(|e| AutomationError(format!("window '{title}' not found: {e}"))),
        WindowSelector::Other(WindowSelectorSpec::TitleContains { text }) => automation
            .create_matcher()
            .contains_name(text)
            .timeout(300)
            .find_first()
            .map_err(|e| AutomationError(format!("no window with a title containing '{text}' found: {e}"))),
        WindowSelector::Other(WindowSelectorSpec::Process { process_name }) => window_by_process(&automation, process_name),
        WindowSelector::Other(WindowSelectorSpec::TitleThenProcess { title, process_name }) => automation
            .create_matcher()
            .name(title)
            .timeout(300)
            .find_first()
            .or_else(|_| window_by_process(&automation, process_name)),
    }
}

fn window_by_process(automation: &UIAutomation, process_name: &str) -> Result<uiautomation::UIElement, AutomationError> {
    let target = enumerate_top_level_windows()
        .into_iter()
        .find(|w| {
            process_name_map()
                .ok()
                .and_then(|m| m.get(&w.pid).cloned())
                .is_some_and(|name| name.eq_ignore_ascii_case(process_name))
        })
        .ok_or_else(|| AutomationError(format!("no window belonging to process '{process_name}' found")))?;
    // `uiautomation`'s own `Handle` wraps a *different* `windows`
    // crate version's `HWND` than this crate depends on (0.57 vs
    // 0.58 — both still pre-1.0, so cargo keeps them as distinct
    // types even though they're structurally identical), so `.into()`
    // straight from our `HWND` doesn't type-check. Going through the
    // raw pointer value (`Handle: From<isize>`) sidesteps that.
    automation
        .element_from_handle((target.hwnd.0 as isize).into())
        .map_err(|e| AutomationError(format!("failed to inspect the window for process '{process_name}': {e}")))
}

/// Polls for up to `timeout_ms`, checking every 500ms, until a window
/// matching `selector` exists — see `Action::WaitForWindow`'s doc
/// comment for why this waits on its own now instead of leaning on
/// the generic per-step retry policy.
pub fn wait_for_window(selector: &flow_schema::WindowSelector, timeout_ms: u32) -> Result<(), AutomationError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        if resolve_window(selector).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(AutomationError(format!(
                "no window matching {} appeared within {timeout_ms}ms",
                describe_window_selector(selector)
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Reads the clipboard's current text, or fails if it doesn't hold
/// text (an image, a file list, empty, ...).
pub fn read_clipboard() -> Result<String, AutomationError> {
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        OpenClipboard(None).map_err(|e| AutomationError(format!("failed to open the clipboard: {e}")))?;
        let result = (|| -> Result<String, AutomationError> {
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32)
                .map_err(|e| AutomationError(format!("no text on the clipboard: {e}")))?;
            let ptr = GlobalLock(hglobal_from(handle)) as *const u16;
            if ptr.is_null() {
                return Err(AutomationError("failed to lock the clipboard's memory".into()));
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(hglobal_from(handle));
            let _ = GMEM_MOVEABLE; // referenced only for write_clipboard's symmetry/documentation
            Ok(text)
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Replaces the clipboard's contents with `text`.
pub fn write_clipboard(text: &str) -> Result<(), AutomationError> {
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_UNICODETEXT;

    unsafe {
        OpenClipboard(None).map_err(|e| AutomationError(format!("failed to open the clipboard: {e}")))?;
        let result = (|| -> Result<(), AutomationError> {
            EmptyClipboard().map_err(|e| AutomationError(format!("failed to clear the clipboard: {e}")))?;
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let size = wide.len() * std::mem::size_of::<u16>();
            let hmem = GlobalAlloc(GMEM_MOVEABLE, size)
                .map_err(|e| AutomationError(format!("failed to allocate clipboard memory: {e}")))?;
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                return Err(AutomationError("failed to lock the clipboard's memory".into()));
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);
            // Ownership of `hmem` passes to the system on success — it
            // must not be freed here.
            SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hmem.0))
                .map_err(|e| AutomationError(format!("failed to set the clipboard's data: {e}")))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

/// `GetClipboardData` returns a `HANDLE` (an opaque `HGLOBAL` in this
/// context) — `GlobalLock` wants the `HGLOBAL` type specifically, so
/// this just re-wraps the same bit pattern rather than reaching for a
/// second, differently-typed handle.
unsafe fn hglobal_from(handle: windows::Win32::Foundation::HANDLE) -> windows::Win32::Foundation::HGLOBAL {
    windows::Win32::Foundation::HGLOBAL(handle.0)
}

/// The identity a Windows toast is filed under — needed because an
/// unpackaged Win32 exe (no MSIX identity) has no app identity of its
/// own to hang a toast on otherwise.
const TOAST_APP_USER_MODEL_ID: &str = "Relay.DesktopFlowAutomation";

/// Shows a Windows toast notification. `message` is optional (a
/// title-only toast is valid). Escapes `title`/`message` for the toast
/// XML payload — they're arbitrary flow-authored/resolved text, not
/// trusted markup.
pub fn show_notification(title: &str, message: &str) -> Result<(), AutomationError> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::core::HSTRING;

    unsafe {
        // Idempotent and cheap — safe to call every time rather than
        // tracking whether some earlier call already set it.
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(TOAST_APP_USER_MODEL_ID));
    }

    // Windows silently drops a toast filed under an AUMID with no
    // Start Menu shortcut carrying that same AUMID — confirmed by
    // testing on this exact machine: the calls below all succeed with
    // no error, but nothing appears on screen without this. Best
    // effort: if this fails (permissions, a locked-down profile,
    // whatever), still attempt the toast anyway rather than treating
    // it as fatal — worst case is the same silent no-op as before.
    let _ = ensure_toast_shortcut();

    let body = if message.is_empty() {
        format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text></binding></visual></toast>"#,
            xml_escape(title)
        )
    } else {
        format!(
            r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
            xml_escape(title),
            xml_escape(message)
        )
    };

    let doc = XmlDocument::new().map_err(|e| AutomationError(format!("failed to create toast XML document: {e}")))?;
    doc.LoadXml(&HSTRING::from(body))
        .map_err(|e| AutomationError(format!("failed to load toast XML: {e}")))?;
    let toast = ToastNotification::CreateToastNotification(&doc)
        .map_err(|e| AutomationError(format!("failed to create toast notification: {e}")))?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(TOAST_APP_USER_MODEL_ID))
        .map_err(|e| AutomationError(format!("failed to create toast notifier: {e}")))?;
    notifier
        .Show(&toast)
        .map_err(|e| AutomationError(format!("failed to show toast notification: {e}")))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Creates (or refreshes) a Start Menu shortcut to this running exe,
/// tagged with [`TOAST_APP_USER_MODEL_ID`] — the one-time setup an
/// unpackaged Win32 app needs before Windows will actually display a
/// toast filed under that AUMID (a bare
/// `SetCurrentProcessExplicitAppUserModelID` call alone silently does
/// nothing). Always rewrites the shortcut rather than no-opping once
/// one exists: a `.lnk` file's icon is resolved (and cached by the
/// shell) at creation time, not read fresh from the target exe on
/// every toast, so a shortcut created before this build's icon was
/// embedded correctly would otherwise permanently show a stale/generic
/// icon in every toast from then on, with nothing short of manually
/// deleting the file able to fix it. Cheap enough (one small file
/// write) to just always do.
fn ensure_toast_shortcut() -> Result<(), AutomationError> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED, IPersistFile};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface, PROPVARIANT};

    let appdata = std::env::var("APPDATA").map_err(|_| AutomationError("%APPDATA% is not set".into()))?;
    let shortcut_path = std::path::Path::new(&appdata)
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join("Relay.lnk");

    let exe_path = std::env::current_exe().map_err(|e| AutomationError(format!("failed to resolve the running exe's path: {e}")))?;
    let exe_path_hstring = HSTRING::from(exe_path.to_string_lossy().as_ref());

    unsafe {
        // `RPC_E_CHANGED_MODE`/`S_FALSE` (COM already initialized,
        // possibly with a different concurrency model, on this
        // thread) is not a failure worth surfacing — some other call
        // on this thread may already have initialized it.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| AutomationError(format!("failed to create IShellLinkW: {e}")))?;
        shell_link
            .SetPath(&exe_path_hstring)
            .map_err(|e| AutomationError(format!("failed to set shortcut target: {e}")))?;
        // Explicit rather than left to fall back on whatever icon
        // Windows resolves for the target on its own — pins the
        // toast's icon to this exe's own embedded icon (index 0)
        // deterministically.
        shell_link
            .SetIconLocation(&exe_path_hstring, 0)
            .map_err(|e| AutomationError(format!("failed to set shortcut icon: {e}")))?;

        let property_store: IPropertyStore = shell_link
            .cast()
            .map_err(|e| AutomationError(format!("IShellLinkW does not implement IPropertyStore: {e}")))?;
        let app_id_value = PROPVARIANT::from(TOAST_APP_USER_MODEL_ID);
        property_store
            .SetValue(&PKEY_AppUserModel_ID, &app_id_value)
            .map_err(|e| AutomationError(format!("failed to set the shortcut's AppUserModelID: {e}")))?;
        property_store
            .Commit()
            .map_err(|e| AutomationError(format!("failed to commit the shortcut's properties: {e}")))?;

        let persist_file: IPersistFile = shell_link
            .cast()
            .map_err(|e| AutomationError(format!("IShellLinkW does not implement IPersistFile: {e}")))?;
        persist_file
            .Save(&HSTRING::from(shortcut_path.to_string_lossy().as_ref()), true)
            .map_err(|e| AutomationError(format!("failed to save the shortcut file: {e}")))?;
    }
    Ok(())
}

/// Whether *this* process is running elevated. Windows' UIPI (User
/// Interface Privilege Isolation) blocks a non-elevated process from
/// sending input to an elevated one's windows at all — clicks and
/// keystrokes just silently do nothing — so a flow automating an
/// elevated target app needs Relay itself to be elevated too, and the
/// UI uses this to tell the user when that's the case.
pub fn is_process_elevated() -> bool {
    unsafe {
        let mut token = HANDLE(std::ptr::null_mut());
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let _ = CloseHandle(token);
        queried.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Relaunches this same executable elevated (the UAC prompt is
/// Windows' own, not something this app draws) and leaves the new,
/// elevated instance running — the caller is expected to exit the
/// current, non-elevated instance right after this returns `Ok`.
/// Fails (rather than silently doing nothing) if the user cancels the
/// UAC prompt, so the UI can tell them it didn't happen instead of
/// leaving them to notice a click that mysteriously never lands.
pub fn relaunch_elevated() -> Result<(), AutomationError> {
    let exe = std::env::current_exe()
        .map_err(|e| AutomationError(format!("failed to find this app's own executable path: {e}")))?;
    let exe_wide: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb_wide: Vec<u16> = "runas\0".encode_utf16().collect();
    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR::from_raw(verb_wide.as_ptr()),
            PCWSTR::from_raw(exe_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        // A failed ShellExecuteW returns a value <= 32 (packed into
        // the HINSTANCE return slot for historical reasons) rather
        // than an HRESULT — this is documented ShellExecute behavior,
        // not a general Windows API convention.
        if (result.0 as isize) <= 32 {
            return Err(AutomationError(
                "administrator relaunch was cancelled or failed — the UAC prompt may have been dismissed".into(),
            ));
        }
    }
    Ok(())
}

fn find_element(selector: &ElementSelector) -> Result<uiautomation::UIElement, AutomationError> {
    let automation =
        UIAutomation::new().map_err(|e| AutomationError(format!("UI Automation init failed: {e}")))?;

    let root = match &selector.window_title {
        Some(title) => automation
            .create_matcher()
            .name(title)
            .timeout(3000)
            .find_first()
            .map_err(|e| AutomationError(format!("window '{title}' not found: {e}")))?,
        None => automation
            .get_root_element()
            .map_err(|e| AutomationError(format!("failed to get desktop root: {e}")))?,
    };

    let mut matcher = automation.create_matcher().from(root).timeout(3000);
    // `automation_id` (when the target app sets one) identifies a
    // control regardless of what it's currently displaying — prefer it
    // over `name`, since for plenty of controls (labels, read-only
    // text) the accessible Name *is* the displayed content, which
    // makes matching by name circular for anything whose whole point
    // is reading a value that changes.
    match &selector.automation_id {
        Some(automation_id) => {
            let automation_id = automation_id.clone();
            matcher = matcher.filter_fn(Box::new(move |el: &uiautomation::UIElement| {
                Ok(el.get_automation_id().map(|id| id == automation_id).unwrap_or(false))
            }));
        }
        None => {
            if let Some(name) = &selector.name {
                matcher = matcher.name(name);
            }
        }
    }
    if let Some(control_type) = &selector.control_type {
        matcher = matcher.classname(control_type);
    }

    matcher
        .find_first()
        .map_err(|e| AutomationError(format!("element not found ({selector:?}): {e}")))
}

/// Captures the primary monitor as an RGB image.
pub fn capture_screen() -> Result<image::RgbImage, AutomationError> {
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    capture_screen_region(0, 0, width, height)
}

/// Captures the entire virtual desktop — the bounding box of every
/// connected monitor, including ones positioned left of or above the
/// primary monitor (negative coordinates) — as a single RGB image.
/// Used by the region picker and by `find_image`/`find_text_ocr`
/// search, so both can reach a secondary display, not just the
/// primary monitor.
pub fn capture_screen_virtual() -> Result<image::RgbImage, AutomationError> {
    let (origin_x, origin_y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    capture_screen_region(origin_x, origin_y, width, height)
}

/// The virtual desktop's bounding box in physical pixels: `(x, y,
/// width, height)`, `x`/`y` possibly negative for a monitor
/// positioned left of or above the primary one. Lets the region
/// picker resize/reposition the app's own window to exactly cover
/// every connected monitor before showing the captured screenshot, so
/// selecting a region feels like drawing directly on the real screen
/// rather than on a scaled-down preview inside a normal app window.
pub fn virtual_screen_bounds() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// Shared `BitBlt`/`GetDIBits` capture, reused by `capture_screen`
/// (primary monitor, source origin `(0, 0)`) and
/// `capture_screen_virtual` (every monitor, source origin possibly
/// negative) — `GetDC(None)`'s desktop DC covers the whole virtual
/// desktop regardless, so a negative source origin is valid.
fn capture_screen_region(src_x: i32, src_y: i32, width: i32, height: i32) -> Result<image::RgbImage, AutomationError> {
    if width <= 0 || height <= 0 {
        return Err(AutomationError("failed to read screen metrics".into()));
    }

    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        let previous = SelectObject(mem_dc, bitmap);

        let blit_result = BitBlt(mem_dc, 0, 0, width, height, screen_dc, src_x, src_y, SRCCOPY);

        let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height requests a top-down DIB, matching
                // `image::RgbImage`'s row order.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let dib_result = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, previous);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        blit_result.map_err(|e| AutomationError(format!("BitBlt failed: {e}")))?;
        if dib_result == 0 {
            return Err(AutomationError("GetDIBits failed".into()));
        }

        let mut rgb = image::RgbImage::new(width as u32, height as u32);
        for (i, pixel) in rgb.pixels_mut().enumerate() {
            let offset = i * 4;
            // GetDIBits with BI_RGB returns BGRA byte order.
            *pixel = image::Rgb([buffer[offset + 2], buffer[offset + 1], buffer[offset]]);
        }
        Ok(rgb)
    }
}

/// Captures a rectangular region of the primary monitor (logical,
/// DPI-independent coordinates) and saves it as a PNG at `save_path` —
/// used by the "capture a reference image" flow for `find_image`
/// steps, so the user can grab exactly the icon/button they want to
/// match without leaving the app.
pub fn capture_region_to_file(x: i32, y: i32, width: u32, height: u32, save_path: &str) -> Result<(), AutomationError> {
    let cropped = capture_region_cropped(x, y, width, height)?;
    cropped
        .save(save_path)
        .map_err(|e| AutomationError(format!("failed to save captured image to '{save_path}': {e}")))
}

/// Same capture as `capture_region_to_file`, but returns the region
/// as a base64-encoded PNG instead of writing it to disk — the
/// backend half of embedding a `find_image` reference image directly
/// into the flow file rather than pointing at a separate one.
pub fn capture_region_to_base64(x: i32, y: i32, width: u32, height: u32) -> Result<String, AutomationError> {
    encode_png_base64(&capture_region_cropped(x, y, width, height)?)
}

/// The whole virtual desktop (every connected monitor) as a
/// base64-encoded PNG — the backend half of the in-app region picker
/// (see `RegionPickerHost.tsx`): rather than a separate transparent
/// overlay window (fragile on Windows — WebView2 doesn't support real
/// alpha blending, only fully transparent or fully opaque, and a
/// from-scratch window ran into several rendering/focus failure modes
/// before this approach replaced it), the picker shows this one
/// static screenshot inside the app's own window and lets the user
/// drag-select on it like an ordinary image crop tool — including on
/// a secondary display, since the capture isn't limited to primary.
pub fn capture_full_screen_to_base64() -> Result<String, AutomationError> {
    encode_png_base64(&capture_screen_virtual()?)
}

/// Saves the whole (virtual) desktop as a PNG at `save_path` — the
/// backend half of `Action::Screenshot` when no region is set.
pub fn capture_full_screen_to_file(save_path: &str) -> Result<(), AutomationError> {
    capture_screen_virtual()?
        .save(save_path)
        .map_err(|e| AutomationError(format!("failed to save screenshot to '{save_path}': {e}")))
}

fn encode_png_base64(image: &image::RgbImage) -> Result<String, AutomationError> {
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgb8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| AutomationError(format!("failed to encode captured image as PNG: {e}")))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png_bytes))
}

fn capture_region_cropped(x: i32, y: i32, width: u32, height: u32) -> Result<image::RgbImage, AutomationError> {
    let screen = capture_screen()?;
    crop_to_logical_region(&screen, x, y, width, height)
}

/// Crops an already-captured screen image (physical pixels) down to a
/// logical (DPI-independent) region, `x`/`y` relative to that same
/// image's own top-left (so this works unchanged whether `screen` is
/// a primary-monitor or virtual-desktop capture, as long as the
/// region was measured against that same capture) — the shared half
/// of `capture_region_to_file`/`capture_region_to_base64` and
/// `find_text_on_screen`'s optional OCR region, so both convert
/// logical coordinates to the capture's physical pixel space the same
/// way.
fn crop_to_logical_region(screen: &image::RgbImage, x: i32, y: i32, width: u32, height: u32) -> Result<image::RgbImage, AutomationError> {
    let dpi = primary_monitor_dpi()?;
    let scale = dpi as f64 / DEFAULT_DPI as f64;
    let px = (x as f64 * scale).round() as i64;
    let py = (y as f64 * scale).round() as i64;
    let pw = (width as f64 * scale).round() as i64;
    let ph = (height as f64 * scale).round() as i64;

    let (screen_w, screen_h) = (screen.width() as i64, screen.height() as i64);
    let left = px.clamp(0, screen_w);
    let top = py.clamp(0, screen_h);
    let right = (px + pw).clamp(0, screen_w);
    let bottom = (py + ph).clamp(0, screen_h);
    if right <= left || bottom <= top {
        return Err(AutomationError("capture region is empty or off-screen".into()));
    }

    Ok(image::imageops::crop_imm(screen, left as u32, top as u32, (right - left) as u32, (bottom - top) as u32).to_image())
}

/// Captures the screen and searches it for `image_path`. Succeeds with
/// the match location if found above threshold, fails otherwise.
///
/// Deliberately scoped to the *primary* monitor only, not the whole
/// virtual desktop `capture_full_screen_to_base64`/`find_text_on_screen`
/// use — `vision::find_exact`/`find_similar` are brute-force
/// normalized cross-correlation (cost roughly proportional to
/// haystack pixels × needle pixels, and `find_similar` repeats that
/// at 13 scale steps). Searching the full virtual desktop was tried
/// and made a single `find_image` call take long enough to look hung
/// — worse, since `run_action` blocks synchronously on it, the stop
/// button couldn't even interrupt it, unlike the retry loop around it
/// which does check between attempts. Real multi-monitor search needs
/// a fundamentally faster matcher (coarse-to-fine pyramid search, or
/// real feature/keypoint matching — see docs/roadmap.md Phase 2), not
/// just a bigger haystack on the current algorithm.
pub fn find_image_on_screen(
    image: &ImageSource,
    mode: MatchMode,
    threshold: f64,
    min_scale: f64,
    max_scale: f64,
    scale_steps: u32,
) -> Result<flow_schema::ImageMatch, AutomationError> {
    let screen = capture_screen()?;
    let haystack = image::DynamicImage::ImageRgb8(screen.clone()).into_luma8();

    let needle_image = load_reference_image(image)?;
    let needle_rgb = needle_image.to_rgb8();
    let needle = needle_image.into_luma8();

    let dpi = primary_monitor_dpi()?;
    let scale = dpi as f64 / DEFAULT_DPI as f64;

    // A reference image captured on a machine at a different DPI scale
    // than this one needs to be searched at roughly
    // `current_scale / captured_scale` its embedded size, not around
    // 1.0 — see `ImageSource::Embedded`'s doc comment. `min_scale`/
    // `max_scale` (the flow author's own "give or take how much"
    // tolerance) apply *around* that predicted size instead of around
    // a flat 1.0 when we know it; falls back to today's behavior
    // (search around 1.0) for a `Path` image or an `Embedded` one
    // saved before this existed.
    let expected_scale = expected_image_scale(image, scale);
    let (min_scale, max_scale) = (min_scale * expected_scale, max_scale * expected_scale);

    let result = match mode {
        MatchMode::Exact => vision::find_exact(&haystack, &needle),
        MatchMode::Similar => vision::find_similar(&haystack, &needle, min_scale, max_scale, scale_steps, threshold),
        MatchMode::Ai => vision::find_ai_similar(&screen, &needle_rgb, min_scale, max_scale, scale_steps, threshold),
    };

    let result = result.ok_or_else(|| AutomationError(format!("no match above threshold {threshold}")))?;

    // `result.x`/`y`/`width`/`height` are physical screen pixels (the
    // capture's own coordinate space) — convert the matched region's
    // center back to the logical, DPI-independent coordinates every
    // other stored point (`MonitorPoint`) uses, the inverse of
    // `to_physical_coordinates`.
    let center_x_physical = result.x as f64 + result.width as f64 / 2.0;
    let center_y_physical = result.y as f64 + result.height as f64 / 2.0;
    Ok(flow_schema::ImageMatch {
        point: MonitorPoint {
            monitor_id: "primary".into(),
            x: (center_x_physical / scale).round() as i32,
            y: (center_y_physical / scale).round() as i32,
        },
        score: result.score,
    })
}

/// How much bigger/smaller `image` should be expected to appear on a
/// screen at `current_scale` (this machine's DPI scale factor — 1.0 at
/// 100%, 1.5 at 150%, ...) than it was when captured — see
/// `ImageSource::Embedded`'s doc comment. `1.0` (search around the
/// image's own embedded size, unchanged from before this existed)
/// whenever there's no captured-scale metadata to reason from at all.
fn expected_image_scale(image: &ImageSource, current_scale: f64) -> f64 {
    match image {
        ImageSource::Embedded { captured_scale: Some(captured), .. } if *captured > 0.0 => current_scale / captured,
        _ => 1.0,
    }
}

/// Loads a `find_image` step's reference image regardless of which
/// `ImageSource` variant it is — a path reads straight from disk (the
/// original behavior); embedded data is base64-decoded and parsed
/// from memory instead, no temp file involved.
fn load_reference_image(image: &ImageSource) -> Result<image::DynamicImage, AutomationError> {
    match image {
        ImageSource::Path(path) => {
            image::open(path).map_err(|e| AutomationError(format!("failed to load reference image '{path}': {e}")))
        }
        ImageSource::Embedded { data, .. } => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| AutomationError(format!("embedded reference image is not valid base64: {e}")))?;
            image::load_from_memory(&bytes)
                .map_err(|e| AutomationError(format!("embedded reference image data isn't a readable image: {e}")))
        }
    }
}

/// Captures the screen and runs Windows' built-in OCR (`Windows.Media.Ocr`,
/// the same engine behind Snipping Tool's "text actions" — no bundled
/// model or extra install needed) over it, succeeding if `text`
/// appears anywhere in the recognized text (case-insensitive). Reading
/// unknown UI content this way, rather than requiring a reference
/// image, is what makes `find_text_ocr` different from `find_image` —
/// it works even when the exact appearance (font, theme, scale) isn't
/// known ahead of time.
pub fn find_text_on_screen(text: &str, region: Option<&flow_schema::CaptureRegion>) -> Result<(), AutomationError> {
    // WinRT calls need the calling thread's apartment initialized —
    // harmless (and already a no-op) if something else on this thread
    // already did it, so an "already initialized" result is ignored
    // rather than treated as failure.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

    let screen = capture_screen_virtual()?;
    let screen = match region {
        Some(r) => crop_to_logical_region(&screen, r.x, r.y, r.width, r.height)?,
        None => screen,
    };
    let (width, height) = (screen.width(), screen.height());
    let mut bgra = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for pixel in screen.pixels() {
        bgra.push(pixel[2]);
        bgra.push(pixel[1]);
        bgra.push(pixel[0]);
        bgra.push(255);
    }

    let writer = DataWriter::new().map_err(|e| AutomationError(format!("OCR init failed: {e}")))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|e| AutomationError(format!("OCR buffer write failed: {e}")))?;
    let buffer = writer.DetachBuffer().map_err(|e| AutomationError(format!("OCR buffer failed: {e}")))?;

    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width as i32, height as i32)
        .map_err(|e| AutomationError(format!("failed to build OCR bitmap: {e}")))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| AutomationError(format!("no OCR language pack available: {e}")))?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.get())
        .map_err(|e| AutomationError(format!("OCR recognition failed: {e}")))?;

    let recognized = result
        .Text()
        .map_err(|e| AutomationError(format!("failed to read OCR result: {e}")))?
        .to_string_lossy();

    if recognized.to_lowercase().contains(&text.to_lowercase()) {
        Ok(())
    } else {
        Err(AutomationError(format!("text \"{text}\" was not found on screen")))
    }
}

/// Types `text` at the current focus via synthesized Unicode key
/// events. A `\n` (the inspector's multi-line text box lets the user
/// press Enter to add one) becomes a real `VK_RETURN` keystroke rather
/// than a literal Unicode line-feed character — a raw LF sent as
/// `KEYEVENTF_UNICODE` doesn't register as "Enter" to most
/// applications (no submit, no newline in a plain single-line field),
/// so without this a form with no visible submit button had no way to
/// actually be submitted from a `type_text` step. `\r` is dropped
/// rather than typed, so a `\r\n` pasted from elsewhere doesn't send
/// two keystrokes for one line break.
pub fn type_text(text: &str) -> Result<(), AutomationError> {
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            inputs.push(key_vk_input(VK_RETURN, false));
            inputs.push(key_vk_input(VK_RETURN, true));
            continue;
        }
        let mut buf = [0u16; 2];
        for unit in ch.encode_utf16(&mut buf) {
            inputs.push(key_input(*unit, false));
            inputs.push(key_input(*unit, true));
        }
    }
    send(&mut inputs)
}

// `MOUSEEVENTF_VIRTUALDESK` is only meaningful alongside
// `MOUSEEVENTF_ABSOLUTE` (every caller here always passes both), and
// tells `SendInput` to interpret the coordinates against the full
// virtual desktop rather than just the primary monitor — needed so a
// point on a secondary display (which `to_absolute` now normalizes
// against `SM_C{X,Y}VIRTUALSCREEN`) actually lands there.
fn mouse_input(x: i32, y: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x,
                dy: y,
                mouseData: 0,
                dwFlags: flags | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_input(utf16_unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: utf16_unit,
                dwFlags: if key_up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &mut [INPUT]) -> Result<(), AutomationError> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(AutomationError(
            "SendInput did not consume all synthesized events".into(),
        ));
    }
    Ok(())
}

/// Scales a logical point by the target monitor's effective DPI.
fn to_physical_coordinates(point: &MonitorPoint) -> Result<(i32, i32), AutomationError> {
    let dpi = primary_monitor_dpi()?;
    let scale = dpi as f64 / DEFAULT_DPI as f64;
    Ok((
        (point.x as f64 * scale).round() as i32,
        (point.y as f64 * scale).round() as i32,
    ))
}

/// A string that changes whenever the monitor layout changes —
/// a monitor unplugged, reconnected, resized, or moved. Every stored
/// coordinate in a flow is monitor-relative (see the module doc), so
/// a layout change invalidates them all at once; the engine snapshots
/// this at the start of a run and pauses (rather than clicking blind
/// at whatever now happens to be at that coordinate) if it ever
/// differs mid-run. Not meant to be parsed — only compared for
/// equality.
pub fn monitor_signature() -> String {
    unsafe extern "system" fn collect(
        _hmonitor: HMONITOR,
        _hdc: HDC,
        rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let rects = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
        rects.push(unsafe { *rect });
        BOOL(1)
    }

    let mut rects: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut rects as *mut Vec<RECT> as isize));
    }
    rects.sort_by_key(|r| (r.left, r.top, r.right, r.bottom));
    rects
        .iter()
        .map(|r| format!("{}:{}:{}:{}", r.left, r.top, r.right, r.bottom))
        .collect::<Vec<_>>()
        .join(",")
}

fn primary_monitor_dpi() -> Result<u32, AutomationError> {
    unsafe {
        let hmonitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTONEAREST);
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
            .map_err(|e| AutomationError(format!("GetDpiForMonitor failed: {e}")))?;
        Ok(dpi_x)
    }
}

/// `SendInput` with `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`
/// expects coordinates normalized to the 0..=65535 range across the
/// whole virtual desktop (every connected monitor's bounding box, not
/// just the primary one) — `x`/`y` here are physical pixels relative
/// to the primary monitor's top-left, the same origin `capture_screen`
/// and every other physical-coordinate helper in this module already
/// uses, which is why no origin shift is needed before the divide.
fn to_absolute(x: i32, y: i32) -> Result<(i32, i32), AutomationError> {
    unsafe {
        let origin_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let origin_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if width == 0 || height == 0 {
            return Err(AutomationError("failed to read screen metrics".into()));
        }
        let abs_x = ((x - origin_x) as i64 * 65535 / width as i64) as i32;
        let abs_y = ((y - origin_y) as i64 * 65535 / height as i64) as i32;
        Ok((abs_x, abs_y))
    }
}

/// The current local date/time, formatted per `format` — see
/// `flow_schema::DateTimeFormat`'s doc comment for each variant's
/// exact shape.
pub fn get_date_time(format: flow_schema::DateTimeFormat) -> String {
    use flow_schema::DateTimeFormat;
    let now = chrono::Local::now();
    match format {
        DateTimeFormat::Iso8601 => now.format("%Y-%m-%d %H:%M:%S").to_string(),
        DateTimeFormat::DateOnly => now.format("%Y-%m-%d").to_string(),
        DateTimeFormat::TimeOnly => now.format("%H:%M:%S").to_string(),
        DateTimeFormat::UnixSeconds => now.timestamp().to_string(),
    }
}

/// A snapshot of this machine's basic state — see
/// `flow_schema::Action::GetSystemInfo`'s doc comment for how each
/// field maps to a flow variable.
pub struct SystemInfo {
    pub hostname: String,
    pub os_version: String,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub ip_address: String,
}

/// Gathers `SystemInfo`. `include_cpu` skips `cpu_percent_sampled`'s
/// ~200ms sampling window (leaving `cpu_percent` at `0.0`) when
/// nothing asked for that field — every other field is effectively
/// free by comparison, so those are always gathered.
pub fn get_system_info(include_cpu: bool) -> SystemInfo {
    SystemInfo {
        hostname: hostname_string(),
        os_version: os_version_string(),
        cpu_percent: if include_cpu { cpu_percent_sampled() } else { 0.0 },
        memory_percent: memory_percent(),
        ip_address: local_ip_address(),
    }
}

fn hostname_string() -> String {
    use windows::Win32::System::SystemInformation::{ComputerNamePhysicalDnsHostname, GetComputerNameExW};
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    unsafe {
        if GetComputerNameExW(ComputerNamePhysicalDnsHostname, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            return String::from_utf16_lossy(&buf[..len as usize]);
        }
    }
    "unknown".to_string()
}

/// Shells out to `ver` rather than the deprecated (and manifest-gated
/// — it lies about the real OS version unless the calling exe
/// declares Windows 10/11 support in its manifest) `GetVersionExW`,
/// or the registry (another dependency to add just for this one
/// field) — `cmd /c ver` needs neither and has worked unchanged since
/// Windows XP.
fn os_version_string() -> String {
    hidden_command("cmd")
        .args(["/C", "ver"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// CPU usage isn't an instantaneous value — it's only meaningful as
/// "how busy was the CPU over some recent window", so this samples
/// `GetSystemTimes` twice, 200ms apart, and computes the percentage
/// from the delta.
fn cpu_percent_sampled() -> f64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetSystemTimes;

    fn filetime_to_u64(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }
    fn sample() -> Option<(u64, u64, u64)> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
        }
        Some((filetime_to_u64(idle), filetime_to_u64(kernel), filetime_to_u64(user)))
    }

    let Some((idle1, kernel1, user1)) = sample() else { return 0.0 };
    std::thread::sleep(std::time::Duration::from_millis(200));
    let Some((idle2, kernel2, user2)) = sample() else { return 0.0 };

    let idle_delta = idle2.saturating_sub(idle1);
    // `kernel` time already includes idle time (Windows' own
    // convention for `GetSystemTimes`), so total busy-eligible time is
    // kernel + user, and idle is subtracted back out below.
    let total_delta = kernel2.saturating_sub(kernel1) + user2.saturating_sub(user1);
    if total_delta == 0 {
        return 0.0;
    }
    let busy_delta = total_delta.saturating_sub(idle_delta);
    (busy_delta as f64 / total_delta as f64) * 100.0
}

fn memory_percent() -> f64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        if GlobalMemoryStatusEx(&mut status).is_ok() {
            return status.dwMemoryLoad as f64;
        }
    }
    0.0
}

/// This machine's local network address — a UDP "connect" (which
/// never actually sends a packet, just resolves routing) to a public
/// address, then reading back which local interface the OS picked,
/// is the standard portable trick for this since Windows has no
/// single "the" local IP API of its own.
fn local_ip_address() -> String {
    use std::net::UdpSocket;
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs a single `TextTransform` operation — see
/// `flow_schema::TextOp`'s doc comment for what each one does.
pub fn text_transform(op: flow_schema::TextOp, text: &str, arg1: &str, arg2: &str) -> Result<String, AutomationError> {
    use flow_schema::TextOp;
    Ok(match op {
        TextOp::Uppercase => text.to_uppercase(),
        TextOp::Lowercase => text.to_lowercase(),
        TextOp::Trim => text.trim().to_string(),
        TextOp::Replace => text.replace(arg1, arg2),
        TextOp::Substring => {
            let chars: Vec<char> = text.chars().collect();
            let start = arg1.parse::<usize>().unwrap_or(0).min(chars.len());
            let len = if arg2.is_empty() { chars.len() - start } else { arg2.parse::<usize>().unwrap_or(0) };
            let end = (start + len).min(chars.len());
            chars[start..end].iter().collect()
        }
        TextOp::Length => text.chars().count().to_string(),
        TextOp::Contains => text.contains(arg1).to_string(),
        TextOp::StartsWith => text.starts_with(arg1).to_string(),
        TextOp::EndsWith => text.ends_with(arg1).to_string(),
        TextOp::Split => {
            let parts: Vec<&str> = if arg1.is_empty() { text.split_whitespace().collect() } else { text.split(arg1).collect() };
            if arg2.is_empty() {
                parts.join("\n")
            } else {
                let idx = arg2
                    .parse::<usize>()
                    .map_err(|_| AutomationError(format!("split index '{arg2}' is not a number")))?;
                parts
                    .get(idx)
                    .map(|s| s.to_string())
                    .ok_or_else(|| AutomationError(format!("split index {idx} is out of range ({} piece(s))", parts.len())))?
            }
        }
        TextOp::Base64Encode => base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
        TextOp::Base64Decode => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|e| AutomationError(format!("invalid base64: {e}")))?;
            String::from_utf8(bytes).map_err(|e| AutomationError(format!("decoded base64 is not valid UTF-8: {e}")))?
        }
        TextOp::Md5 => {
            use md5::{Digest, Md5};
            let mut hasher = Md5::new();
            hasher.update(text.as_bytes());
            hex_encode(&hasher.finalize())
        }
        TextOp::Sha256 => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(text.as_bytes());
            hex_encode(&hasher.finalize())
        }
        TextOp::JsonGet => {
            let parsed: serde_json::Value =
                serde_json::from_str(text).map_err(|e| AutomationError(format!("invalid JSON: {e}")))?;
            let found = json_value_at_path(&parsed, arg1)
                .ok_or_else(|| AutomationError(format!("no value at JSON path '{arg1}'")))?;
            match found {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            }
        }
        TextOp::JsonEscape => {
            // `serde_json::to_string` on a `Value::String` produces
            // exactly the escaped-and-quoted JSON string literal;
            // stripping the surrounding quotes leaves just the
            // escaped contents, ready to splice between quotes of the
            // caller's own choosing.
            let quoted = serde_json::to_string(&serde_json::Value::String(text.to_string())).unwrap_or_default();
            quoted.trim_start_matches('"').trim_end_matches('"').to_string()
        }
        TextOp::RegexTest => {
            let re = regex::Regex::new(arg1).map_err(|e| AutomationError(format!("invalid regular expression: {e}")))?;
            re.is_match(text).to_string()
        }
        TextOp::RegexMatch => {
            let re = regex::Regex::new(arg1).map_err(|e| AutomationError(format!("invalid regular expression: {e}")))?;
            let group: usize = if arg2.trim().is_empty() { 0 } else { arg2.trim().parse().unwrap_or(0) };
            re.captures(text)
                .and_then(|caps| caps.get(group))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        }
    })
}

/// Walks a dot/bracket path (`"user.name"`, `"items[0].id"`) through a
/// parsed JSON value — see `TextOp::JsonGet`'s doc comment.
fn json_value_at_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for raw_segment in path.split('.') {
        if raw_segment.is_empty() {
            continue;
        }
        let key_end = raw_segment.find('[').unwrap_or(raw_segment.len());
        let key = &raw_segment[..key_end];
        if !key.is_empty() {
            current = current.get(key)?;
        }
        let mut rest = &raw_segment[key_end..];
        while let Some(after_bracket) = rest.strip_prefix('[') {
            let close = after_bracket.find(']')?;
            let index: usize = after_bracket[..close].parse().ok()?;
            current = current.get(index)?;
            rest = &after_bracket[close + 1..];
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_image_scale_predicts_the_dpi_size_difference() {
        // A desktop (100%, scale 1.0) captured this image; running on
        // a laptop at 150% (scale 1.5) should expect it to appear 1.5x
        // as large — the exact cross-device case that motivated this.
        let img = ImageSource::Embedded { data: String::new(), captured_scale: Some(1.0) };
        assert_eq!(expected_image_scale(&img, 1.5), 1.5);
        // The reverse: captured at 150%, run at 100% — expect it 1/1.5
        // (~0.667x) as large.
        let img = ImageSource::Embedded { data: String::new(), captured_scale: Some(1.5) };
        assert!((expected_image_scale(&img, 1.0) - (1.0 / 1.5)).abs() < 1e-9);
        // Same machine, same scale — no adjustment.
        let img = ImageSource::Embedded { data: String::new(), captured_scale: Some(1.25) };
        assert_eq!(expected_image_scale(&img, 1.25), 1.0);
    }

    #[test]
    fn expected_image_scale_falls_back_to_1_without_capture_metadata() {
        // No metadata at all (a flow saved before this existed).
        let img = ImageSource::Embedded { data: String::new(), captured_scale: None };
        assert_eq!(expected_image_scale(&img, 1.5), 1.0);
        // A file loaded from disk — never has a reliable captured scale.
        let img = ImageSource::Path("target.png".into());
        assert_eq!(expected_image_scale(&img, 1.5), 1.0);
    }

    /// Real GDI screen capture — needs an interactive desktop session,
    /// so it's `#[ignore]`d by default (headless CI has none) and run
    /// explicitly with `cargo test -- --ignored` to verify the actual
    /// Win32 calls work, not just that they compile.
    #[test]
    #[ignore]
    fn capture_screen_returns_a_nonempty_image_matching_screen_metrics() {
        let img = capture_screen().expect("capture_screen should succeed on an interactive desktop");
        let expected_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as u32;
        let expected_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as u32;
        assert_eq!(img.width(), expected_w);
        assert_eq!(img.height(), expected_h);
        // Not every pixel should be identical — a real screen has
        // variation. A capture that failed silently often comes back
        // as solid black.
        let first = img.get_pixel(0, 0);
        let has_variation = img.pixels().any(|p| p != first);
        assert!(has_variation, "captured image looks suspiciously uniform");
    }

    /// Real toast display, including the one-time Start Menu shortcut
    /// registration — needs an interactive desktop session and human
    /// eyes to confirm anything actually appeared, so `#[ignore]`d by
    /// default like `capture_screen_returns_a_nonempty_image_matching_screen_metrics`
    /// above. Confirmed manually working on 2026-08-26.
    #[test]
    #[ignore]
    fn show_notification_displays_a_real_toast() {
        show_notification("Relay test toast", "if you can see this, it works").expect("show_notification should succeed");
    }

    /// Needs a real clipboard (an interactive desktop session), so
    /// `#[ignore]`d like every other real-OS test in this module —
    /// but unlike the toast/capture ones, fully self-checking (no
    /// human eyes needed): round-trips a known string through the
    /// real Win32 clipboard and asserts it comes back unchanged.
    #[test]
    #[ignore]
    fn clipboard_round_trips_written_text() {
        write_clipboard("Relay clipboard round-trip テスト").expect("write_clipboard should succeed");
        let read_back = read_clipboard().expect("read_clipboard should succeed");
        assert_eq!(read_back, "Relay clipboard round-trip テスト");
    }
}
