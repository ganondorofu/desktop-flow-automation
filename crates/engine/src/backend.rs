use flow_schema::{
    BrowserSelector, CaptureRegion, ClickKind, ElementSelector, ImageMatch, ImageSource, KeyModifiers, MatchMode, MonitorPoint,
    MouseButton,
};

/// Splits a `LaunchBrowser`-captured instance string (`"<connection>#<tabId>"`,
/// see `automation::launch_browser_instance`'s doc comment) back into
/// the connection id `browser_bridge::send_command` routes to and the
/// tab id its `params.tabId` should carry — `relay-bridge-extension`'s
/// `resolveTabId` is what actually reads that back out on the other
/// end. An instance string that doesn't contain `#` (or is absent) is
/// treated as connection-only with no specific tab, so an older saved
/// flow (or a hand-written `.relay` file) with a bare instance still
/// works the same way it always has.
fn parse_instance(instance: Option<&str>) -> (Option<&str>, Option<i64>) {
    let Some(instance) = instance else { return (None, None) };
    match instance.split_once('#') {
        Some((connection, tab)) => (Some(connection), tab.parse::<i64>().ok()),
        None => (Some(instance), None),
    }
}

/// Merges a parsed tab id into `params` as `tabId` — see
/// `parse_instance`'s doc comment.
fn with_tab_id(tab_id: Option<i64>, mut params: serde_json::Value) -> serde_json::Value {
    if let Some(id) = tab_id {
        if let serde_json::Value::Object(map) = &mut params {
            map.insert("tabId".into(), serde_json::Value::from(id));
        }
    }
    params
}

/// The OS-facing side of a leaf action. Production code runs against
/// `WindowsBackend`; tests run against a mock so the suite has no real
/// screen side effects and is safe to run unattended.
pub trait AutomationBackend {
    /// Clicks wherever the cursor already is — `Action::Click` never
    /// carries a coordinate itself; see `ClickTarget::Cursor`'s doc
    /// comment for why.
    fn click_at_cursor(&self, button: MouseButton, click_kind: ClickKind) -> Result<(), String>;
    fn click_element(&self, selector: &ElementSelector) -> Result<(), String>;
    fn move_mouse(&self, point: &MonitorPoint, duration_ms: u32) -> Result<(), String>;
    fn type_text(&self, text: &str) -> Result<(), String>;
    /// Presses and immediately releases `key` plus `modifiers` — see
    /// `flow_schema::Action::KeyPress`'s doc comment.
    fn key_tap(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String>;
    /// Presses `key` plus `modifiers` down, leaving them held.
    fn key_hold_down(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String>;
    /// Releases `key` plus `modifiers` — the inverse of `key_hold_down`.
    fn key_release(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String>;
    fn find_image(&self, image: &ImageSource, mode: MatchMode, threshold: f64, min_scale: f64, max_scale: f64, scale_steps: u32) -> Result<ImageMatch, String>;
    fn find_text_ocr(&self, text: &str, region: Option<&CaptureRegion>) -> Result<(), String>;
    fn wait_for_window(&self, window: &flow_schema::WindowSelector, timeout_ms: u32) -> Result<(), String>;
    fn focus_window(&self, window: &flow_schema::WindowSelector) -> Result<(), String>;
    fn shutdown(&self, force: bool) -> Result<(), String>;
    fn restart(&self, force: bool) -> Result<(), String>;
    fn lock_workstation(&self) -> Result<(), String>;
    fn read_clipboard(&self) -> Result<String, String>;
    fn write_clipboard(&self, text: &str) -> Result<(), String>;
    fn show_message(&self, title: &str, message: &str) -> Result<(), String>;
    /// Same box as `show_message`, but returns as soon as it's shown
    /// instead of waiting for the user to dismiss it — the box stays
    /// open on its own thread. Used for `ShowMessage { blocking: false, .. }`.
    fn show_message_async(&self, title: &str, message: &str) -> Result<(), String>;
    /// Returns `true` for Yes.
    fn show_confirm(&self, title: &str, message: &str) -> Result<bool, String>;
    fn show_input(&self, title: &str, message: &str, default_value: &str) -> Result<String, String>;
    fn get_date_time(&self, format: flow_schema::DateTimeFormat) -> String;
    /// `include_cpu` skips the ~200ms CPU-usage sampling window when
    /// nothing asked for that field — see `Action::GetSystemInfo`'s
    /// doc comment.
    fn get_system_info(&self, include_cpu: bool) -> automation::SystemInfo;
    fn text_transform(&self, op: flow_schema::TextOp, text: &str, arg1: &str, arg2: &str) -> Result<String, String>;
    /// Returns the numeric HTTP status code.
    fn http_download(&self, url: &str, headers: &str, path: &str) -> Result<u16, String>;
    fn ping(&self, host: &str, timeout_ms: u32) -> Result<automation::PingResult, String>;
    fn dns_lookup(&self, hostname: &str) -> Result<String, String>;
    fn take_screenshot(&self, region: Option<&CaptureRegion>, path: &str) -> Result<(), String>;
    fn browser_screenshot(&self, path: &str, instance: Option<&str>) -> Result<(), String>;
    fn get_env_var(&self, name: &str) -> Result<String, String>;
    fn check_process(&self, name: &str) -> Result<bool, String>;
    fn kill_process(&self, name: &str, force: bool) -> Result<(), String>;
    fn wait_for_file(&self, path: &str, timeout_ms: u32) -> Result<(), String>;
    fn generate_random(&self, min: &str, max: &str) -> Result<i64, String>;
    fn launch_app(&self, path: &str, args: &str) -> Result<(), String>;
    fn open_url(&self, url: &str) -> Result<(), String>;
    fn show_notification(&self, title: &str, message: &str) -> Result<(), String>;
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, content: &str, append: bool) -> Result<(), String>;
    fn copy_file(&self, source: &str, destination: &str) -> Result<(), String>;
    fn move_file(&self, source: &str, destination: &str) -> Result<(), String>;
    fn delete_file(&self, path: &str) -> Result<(), String>;
    fn create_directory(&self, path: &str) -> Result<(), String>;
    fn list_directory(&self, path: &str) -> Result<String, String>;
    /// Returns `(response body, status code)`.
    fn http_request(&self, method: flow_schema::HttpMethod, url: &str, headers: &str, body: &str) -> Result<(String, u16), String>;
    fn get_element_text(&self, selector: &ElementSelector) -> Result<String, String>;
    /// Spawns a genuinely new browser window (not just a new tab in
    /// whatever's already open) and returns its instance id — ready
    /// to drop straight into a flow variable, and the "instance"
    /// every `Browser*` method's `instance` param can later address to
    /// reach this exact window specifically. `browser` picks which
    /// installed browser to launch (an id from
    /// `list_installed_browsers`; `None`/unrecognized falls back to
    /// the first one found). `profile_dir` points the new window at a
    /// specific `--user-data-dir`; `None`/empty uses a dedicated
    /// Relay-owned automation profile instead of the user's normal
    /// one, so this never touches their real cookies/logins/history.
    fn launch_browser(&self, url: &str, browser: Option<&str>, profile_dir: Option<&str>) -> Result<String, String>;
    fn browser_navigate(&self, url: &str, instance: Option<&str>) -> Result<(), String>;
    fn browser_click(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<(), String>;
    fn browser_get_text(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<String, String>;
    fn browser_set_value(&self, selector: &BrowserSelector, value: &str, instance: Option<&str>) -> Result<(), String>;
    fn browser_wait_for_selector(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<(), String>;
    /// A string that changes whenever the monitor layout changes —
    /// see `automation::monitor_signature`'s doc comment for why this
    /// matters. Not meant to be parsed, only compared for equality.
    fn monitor_signature(&self) -> String;
}

pub struct WindowsBackend;
impl AutomationBackend for WindowsBackend {
    fn click_at_cursor(&self, button: MouseButton, click_kind: ClickKind) -> Result<(), String> {
        automation::click_at_cursor(button, click_kind).map_err(|e| e.to_string())
    }
    fn click_element(&self, selector: &ElementSelector) -> Result<(), String> {
        automation::click_element(selector).map_err(|e| e.to_string())
    }
    fn move_mouse(&self, point: &MonitorPoint, duration_ms: u32) -> Result<(), String> {
        automation::move_mouse(point, duration_ms).map_err(|e| e.to_string())
    }
    fn type_text(&self, text: &str) -> Result<(), String> {
        automation::type_text(text).map_err(|e| e.to_string())
    }
    fn key_tap(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String> {
        automation::key_tap_combo(key, &modifiers).map_err(|e| e.to_string())
    }
    fn key_hold_down(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String> {
        automation::key_press_combo(key, &modifiers).map_err(|e| e.to_string())
    }
    fn key_release(&self, key: &str, modifiers: KeyModifiers) -> Result<(), String> {
        automation::key_release_combo(key, &modifiers).map_err(|e| e.to_string())
    }
    fn find_image(&self, image: &ImageSource, mode: MatchMode, threshold: f64, min_scale: f64, max_scale: f64, scale_steps: u32) -> Result<ImageMatch, String> {
        automation::find_image_on_screen(image, mode, threshold, min_scale, max_scale, scale_steps).map_err(|e| e.to_string())
    }
    fn find_text_ocr(&self, text: &str, region: Option<&CaptureRegion>) -> Result<(), String> {
        automation::find_text_on_screen(text, region).map_err(|e| e.to_string())
    }
    fn wait_for_window(&self, window: &flow_schema::WindowSelector, timeout_ms: u32) -> Result<(), String> {
        automation::wait_for_window(window, timeout_ms).map_err(|e| e.to_string())
    }
    fn focus_window(&self, window: &flow_schema::WindowSelector) -> Result<(), String> {
        automation::focus_window(window).map_err(|e| e.to_string())
    }
    fn shutdown(&self, force: bool) -> Result<(), String> {
        automation::shutdown(force).map_err(|e| e.to_string())
    }
    fn restart(&self, force: bool) -> Result<(), String> {
        automation::restart(force).map_err(|e| e.to_string())
    }
    fn lock_workstation(&self) -> Result<(), String> {
        automation::lock_workstation().map_err(|e| e.to_string())
    }
    fn read_clipboard(&self) -> Result<String, String> {
        automation::read_clipboard().map_err(|e| e.to_string())
    }
    fn write_clipboard(&self, text: &str) -> Result<(), String> {
        automation::write_clipboard(text).map_err(|e| e.to_string())
    }
    fn show_message(&self, title: &str, message: &str) -> Result<(), String> {
        automation::show_message(title, message).map_err(|e| e.to_string())
    }
    fn show_message_async(&self, title: &str, message: &str) -> Result<(), String> {
        let title = title.to_string();
        let message = message.to_string();
        std::thread::spawn(move || {
            let _ = automation::show_message(&title, &message);
        });
        Ok(())
    }
    fn show_confirm(&self, title: &str, message: &str) -> Result<bool, String> {
        automation::show_confirm(title, message).map_err(|e| e.to_string())
    }
    fn show_input(&self, title: &str, message: &str, default_value: &str) -> Result<String, String> {
        automation::show_input(title, message, default_value).map_err(|e| e.to_string())
    }
    fn get_date_time(&self, format: flow_schema::DateTimeFormat) -> String {
        automation::get_date_time(format)
    }
    fn get_system_info(&self, include_cpu: bool) -> automation::SystemInfo {
        automation::get_system_info(include_cpu)
    }
    fn text_transform(&self, op: flow_schema::TextOp, text: &str, arg1: &str, arg2: &str) -> Result<String, String> {
        automation::text_transform(op, text, arg1, arg2).map_err(|e| e.to_string())
    }
    fn http_download(&self, url: &str, headers: &str, path: &str) -> Result<u16, String> {
        automation::http_download(url, headers, path).map_err(|e| e.to_string())
    }
    fn ping(&self, host: &str, timeout_ms: u32) -> Result<automation::PingResult, String> {
        automation::ping(host, timeout_ms).map_err(|e| e.to_string())
    }
    fn dns_lookup(&self, hostname: &str) -> Result<String, String> {
        automation::dns_lookup(hostname).map_err(|e| e.to_string())
    }
    fn take_screenshot(&self, region: Option<&CaptureRegion>, path: &str) -> Result<(), String> {
        match region {
            Some(r) => automation::capture_region_to_file(r.x, r.y, r.width, r.height, path),
            None => automation::capture_full_screen_to_file(path),
        }
        .map_err(|e| e.to_string())
    }
    fn browser_screenshot(&self, path: &str, instance: Option<&str>) -> Result<(), String> {
        let (connection, tab_id) = parse_instance(instance);
        let result = browser_bridge::send_command(connection, "screenshot", with_tab_id(tab_id, serde_json::json!({})))?;
        let data_url = result.as_str().ok_or_else(|| "browser extension returned an unexpected reply".to_string())?;
        automation::save_data_url_to_file(data_url, path).map_err(|e| e.to_string())
    }
    fn get_env_var(&self, name: &str) -> Result<String, String> {
        automation::get_env_var(name).map_err(|e| e.to_string())
    }
    fn check_process(&self, name: &str) -> Result<bool, String> {
        automation::check_process(name).map_err(|e| e.to_string())
    }
    fn kill_process(&self, name: &str, force: bool) -> Result<(), String> {
        automation::kill_process(name, force).map_err(|e| e.to_string())
    }
    fn wait_for_file(&self, path: &str, timeout_ms: u32) -> Result<(), String> {
        automation::wait_for_file(path, timeout_ms).map_err(|e| e.to_string())
    }
    fn generate_random(&self, min: &str, max: &str) -> Result<i64, String> {
        automation::generate_random(min, max).map_err(|e| e.to_string())
    }
    fn launch_app(&self, path: &str, args: &str) -> Result<(), String> {
        automation::launch_app(path, args).map_err(|e| e.to_string())
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        automation::open_url(url).map_err(|e| e.to_string())
    }
    fn show_notification(&self, title: &str, message: &str) -> Result<(), String> {
        automation::show_notification(title, message).map_err(|e| e.to_string())
    }
    fn read_file(&self, path: &str) -> Result<String, String> {
        automation::read_file(path).map_err(|e| e.to_string())
    }
    fn write_file(&self, path: &str, content: &str, append: bool) -> Result<(), String> {
        automation::write_file(path, content, append).map_err(|e| e.to_string())
    }
    fn copy_file(&self, source: &str, destination: &str) -> Result<(), String> {
        automation::copy_file(source, destination).map_err(|e| e.to_string())
    }
    fn move_file(&self, source: &str, destination: &str) -> Result<(), String> {
        automation::move_file(source, destination).map_err(|e| e.to_string())
    }
    fn delete_file(&self, path: &str) -> Result<(), String> {
        automation::delete_file(path).map_err(|e| e.to_string())
    }
    fn create_directory(&self, path: &str) -> Result<(), String> {
        automation::create_directory(path).map_err(|e| e.to_string())
    }
    fn list_directory(&self, path: &str) -> Result<String, String> {
        automation::list_directory(path).map_err(|e| e.to_string())
    }
    fn http_request(&self, method: flow_schema::HttpMethod, url: &str, headers: &str, body: &str) -> Result<(String, u16), String> {
        automation::http_request(method, url, headers, body).map_err(|e| e.to_string())
    }
    fn get_element_text(&self, selector: &ElementSelector) -> Result<String, String> {
        automation::get_element_text(selector).map_err(|e| e.to_string())
    }
    fn launch_browser(&self, url: &str, browser: Option<&str>, profile_dir: Option<&str>) -> Result<String, String> {
        automation::launch_browser_instance(url, browser, profile_dir).map_err(|e| e.to_string())
    }
    fn browser_navigate(&self, url: &str, instance: Option<&str>) -> Result<(), String> {
        let (connection, tab_id) = parse_instance(instance);
        browser_bridge::send_command(connection, "navigate", with_tab_id(tab_id, serde_json::json!({ "url": url }))).map(|_| ())
    }
    fn browser_click(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<(), String> {
        let (connection, tab_id) = parse_instance(instance);
        browser_bridge::send_command(connection, "click", with_tab_id(tab_id, serde_json::json!({ "selector": selector }))).map(|_| ())
    }
    fn browser_get_text(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<String, String> {
        let (connection, tab_id) = parse_instance(instance);
        let result = browser_bridge::send_command(connection, "get_text", with_tab_id(tab_id, serde_json::json!({ "selector": selector })))?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "browser extension returned an unexpected reply".to_string())
    }
    fn browser_set_value(&self, selector: &BrowserSelector, value: &str, instance: Option<&str>) -> Result<(), String> {
        let (connection, tab_id) = parse_instance(instance);
        browser_bridge::send_command(connection, "set_value", with_tab_id(tab_id, serde_json::json!({ "selector": selector, "value": value })))
            .map(|_| ())
    }
    fn browser_wait_for_selector(&self, selector: &BrowserSelector, instance: Option<&str>) -> Result<(), String> {
        let (connection, tab_id) = parse_instance(instance);
        browser_bridge::send_command(connection, "wait_for_selector", with_tab_id(tab_id, serde_json::json!({ "selector": selector })))
            .map(|_| ())
    }
    fn monitor_signature(&self) -> String {
        automation::monitor_signature()
    }
}
