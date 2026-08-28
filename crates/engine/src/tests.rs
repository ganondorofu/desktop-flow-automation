use super::*;
use crate::runner::run_branch;
use flow_schema::{
    Action, Branch, BrowserSelector, CalcOp, CaptureRegion, ClickKind, ClickTarget, Condition,
    Connection, ElementSelector, FailureBehavior, Flow, ImageMatch, ImageSource, KeyModifiers, KeyPressMode, MatchMode,
    MonitorPoint, MouseButton, PointTarget, RetryPolicy, Step, TextOp,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct RecordingObserver {
    started: Vec<String>,
    monitor_mismatches: u32,
    monitor_restores: u32,
    paused_at: Vec<String>,
    resumes: u32,
    /// The most recent `on_variables_changed` snapshot — lets a test
    /// confirm the variables-panel runtime trace actually reflects
    /// what a step just wrote, not just that the run finished.
    last_variables: HashMap<String, String>,
}
impl RecordingObserver {
    fn new() -> Self {
        Self {
            started: Vec::new(),
            monitor_mismatches: 0,
            monitor_restores: 0,
            paused_at: Vec::new(),
            resumes: 0,
            last_variables: HashMap::new(),
        }
    }
}
impl ExecutionObserver for RecordingObserver {
    fn on_step_start(&mut self, step: &Step) {
        self.started.push(step.id.clone());
    }
    fn on_monitor_mismatch(&mut self) {
        self.monitor_mismatches += 1;
    }
    fn on_monitor_restored(&mut self) {
        self.monitor_restores += 1;
    }
    fn on_paused(&mut self, step: &Step) {
        self.paused_at.push(step.id.clone());
    }
    fn on_resumed(&mut self) {
        self.resumes += 1;
    }
    fn on_variables_changed(&mut self, variables: &HashMap<String, String>) {
        self.last_variables = variables.clone();
    }
}

/// Never touches the real screen. `remaining_failures` lets a test
/// assert retry behaviour: the backend fails that many times, then
/// starts succeeding.
struct MockBackend {
    remaining_failures: Cell<u32>,
    click_calls: Cell<u32>,
    /// Counts `monitor_signature()` calls. Call 0 is
    /// `run_flow_with_backend`'s own initial snapshot; when
    /// `mismatch_on_call_1` is set, call 1 (the first per-step
    /// check inside `run_branch`) returns a mismatched value and
    /// every call after that reverts to normal — simulating a
    /// monitor reconnecting right away, without a test needing to
    /// sit through the real pause loop's polling interval.
    monitor_calls: Cell<u32>,
    mismatch_on_call_1: Cell<bool>,
    /// The point `move_mouse` was last actually called with — lets a
    /// test confirm `PointTarget::LastMatch` resolved to the right
    /// coordinates rather than only checking that *a* move happened.
    last_point: Cell<(i32, i32)>,
    /// Every key `key_release` was actually called for, in order —
    /// lets a test confirm the engine's end-of-run force-release
    /// safety net (see `run_flow_with_backend`) really fired, not
    /// just that the run finished without error.
    key_release_calls: RefCell<Vec<String>>,
}
impl MockBackend {
    fn always_succeeds() -> Self {
        Self {
            remaining_failures: Cell::new(0),
            click_calls: Cell::new(0),
            monitor_calls: Cell::new(0),
            mismatch_on_call_1: Cell::new(false),
            last_point: Cell::new((0, 0)),
            key_release_calls: RefCell::new(Vec::new()),
        }
    }
    fn fails_first(times: u32) -> Self {
        Self {
            remaining_failures: Cell::new(times),
            click_calls: Cell::new(0),
            monitor_calls: Cell::new(0),
            mismatch_on_call_1: Cell::new(false),
            last_point: Cell::new((0, 0)),
            key_release_calls: RefCell::new(Vec::new()),
        }
    }
    fn mismatched_monitor_once() -> Self {
        Self {
            remaining_failures: Cell::new(0),
            click_calls: Cell::new(0),
            monitor_calls: Cell::new(0),
            mismatch_on_call_1: Cell::new(true),
            last_point: Cell::new((0, 0)),
            key_release_calls: RefCell::new(Vec::new()),
        }
    }
}
impl AutomationBackend for MockBackend {
    fn click_at_cursor(&self, _button: MouseButton, _click_kind: ClickKind) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock click failure".into())
        } else {
            Ok(())
        }
    }
    fn click_element(&self, _selector: &ElementSelector) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock click failure".into())
        } else {
            Ok(())
        }
    }
    fn move_mouse(&self, point: &MonitorPoint, _duration_ms: u32) -> Result<(), String> {
        self.last_point.set((point.x, point.y));
        Ok(())
    }
    fn type_text(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn key_tap(&self, _key: &str, _modifiers: KeyModifiers) -> Result<(), String> {
        Ok(())
    }
    fn key_hold_down(&self, _key: &str, _modifiers: KeyModifiers) -> Result<(), String> {
        Ok(())
    }
    fn key_release(&self, key: &str, _modifiers: KeyModifiers) -> Result<(), String> {
        self.key_release_calls.borrow_mut().push(key.to_string());
        Ok(())
    }
    fn find_image(
        &self,
        _image: &ImageSource,
        _mode: MatchMode,
        _threshold: f64,
        _min_scale: f64,
        _max_scale: f64,
        _scale_steps: u32,
    ) -> Result<ImageMatch, String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock image not found".into())
        } else {
            Ok(ImageMatch {
                point: MonitorPoint { monitor_id: "primary".into(), x: 42, y: 24 },
                score: 0.99,
            })
        }
    }
    fn find_text_ocr(&self, _text: &str, _region: Option<&CaptureRegion>) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock text not found".into())
        } else {
            Ok(())
        }
    }
    fn wait_for_window(&self, _window_title: &str) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock window not found".into())
        } else {
            Ok(())
        }
    }
    fn focus_window(&self, _window_title: &str) -> Result<(), String> {
        Ok(())
    }
    fn shutdown(&self, _force: bool) -> Result<(), String> {
        Ok(())
    }
    fn restart(&self, _force: bool) -> Result<(), String> {
        Ok(())
    }
    fn lock_workstation(&self) -> Result<(), String> {
        Ok(())
    }
    fn read_clipboard(&self) -> Result<String, String> {
        Ok("mock clipboard text".into())
    }
    fn write_clipboard(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn show_message(&self, _title: &str, _message: &str) -> Result<(), String> {
        Ok(())
    }
    fn show_message_async(&self, _title: &str, _message: &str) -> Result<(), String> {
        Ok(())
    }
    fn show_confirm(&self, _title: &str, _message: &str) -> Result<bool, String> {
        Ok(true)
    }
    fn show_input(&self, _title: &str, _message: &str, default_value: &str) -> Result<String, String> {
        Ok(default_value.to_string())
    }
    fn get_date_time(&self, _format: flow_schema::DateTimeFormat) -> String {
        "2026-01-01 00:00:00".to_string()
    }
    fn get_system_info(&self, _include_cpu: bool) -> automation::SystemInfo {
        automation::SystemInfo {
            hostname: "mock-host".into(),
            os_version: "mock-os".into(),
            cpu_percent: 0.0,
            memory_percent: 0.0,
            ip_address: "127.0.0.1".into(),
        }
    }
    fn text_transform(&self, op: flow_schema::TextOp, text: &str, arg1: &str, arg2: &str) -> Result<String, String> {
        automation::text_transform(op, text, arg1, arg2).map_err(|e| e.to_string())
    }
    fn http_download(&self, _url: &str, _headers: &str, _path: &str) -> Result<u16, String> {
        Ok(200)
    }
    fn ping(&self, _host: &str, _timeout_ms: u32) -> Result<automation::PingResult, String> {
        Ok(automation::PingResult { reachable: true, latency_ms: Some(1) })
    }
    fn dns_lookup(&self, _hostname: &str) -> Result<String, String> {
        Ok("127.0.0.1".into())
    }
    fn take_screenshot(&self, _region: Option<&CaptureRegion>, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn browser_screenshot(&self, _path: &str, _instance: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    fn get_env_var(&self, name: &str) -> Result<String, String> {
        Ok(format!("mock-{name}"))
    }
    fn check_process(&self, _name: &str) -> Result<bool, String> {
        Ok(true)
    }
    fn kill_process(&self, _name: &str, _force: bool) -> Result<(), String> {
        Ok(())
    }
    fn wait_for_file(&self, _path: &str, _timeout_ms: u32) -> Result<(), String> {
        Ok(())
    }
    fn generate_random(&self, min: &str, max: &str) -> Result<i64, String> {
        automation::generate_random(min, max).map_err(|e| e.to_string())
    }
    fn launch_app(&self, _path: &str, _args: &str) -> Result<(), String> {
        Ok(())
    }
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
    fn show_notification(&self, _title: &str, _message: &str) -> Result<(), String> {
        Ok(())
    }
    fn read_file(&self, _path: &str) -> Result<String, String> {
        Ok("mock file contents".into())
    }
    fn write_file(&self, _path: &str, _content: &str, _append: bool) -> Result<(), String> {
        Ok(())
    }
    fn copy_file(&self, _source: &str, _destination: &str) -> Result<(), String> {
        Ok(())
    }
    fn move_file(&self, _source: &str, _destination: &str) -> Result<(), String> {
        Ok(())
    }
    fn delete_file(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn create_directory(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn list_directory(&self, _path: &str) -> Result<String, String> {
        Ok("a.txt\nb.txt".into())
    }
    fn http_request(&self, _method: flow_schema::HttpMethod, _url: &str, _headers: &str, _body: &str) -> Result<(String, u16), String> {
        Ok(("mock response".into(), 200))
    }
    fn get_element_text(&self, _selector: &ElementSelector) -> Result<String, String> {
        Ok("mock text".into())
    }
    fn launch_browser(&self, _url: &str, _browser: Option<&str>, _profile_dir: Option<&str>) -> Result<String, String> {
        Ok("42".into())
    }
    fn browser_navigate(&self, _url: &str, _instance: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    fn browser_click(&self, _selector: &BrowserSelector, _instance: Option<&str>) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock element not found".into())
        } else {
            Ok(())
        }
    }
    fn browser_get_text(&self, _selector: &BrowserSelector, _instance: Option<&str>) -> Result<String, String> {
        Ok("mock browser text".into())
    }
    fn browser_set_value(&self, _selector: &BrowserSelector, _value: &str, _instance: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    fn browser_wait_for_selector(&self, _selector: &BrowserSelector, _instance: Option<&str>) -> Result<(), String> {
        self.click_calls.set(self.click_calls.get() + 1);
        let remaining = self.remaining_failures.get();
        if remaining > 0 {
            self.remaining_failures.set(remaining - 1);
            Err("mock selector not found".into())
        } else {
            Ok(())
        }
    }
    fn monitor_signature(&self) -> String {
        let call = self.monitor_calls.get();
        self.monitor_calls.set(call + 1);
        if call == 1 && self.mismatch_on_call_1.get() {
            "mock-monitors-changed".into()
        } else {
            "mock-monitors".into()
        }
    }
}

fn wait_step(id: &str) -> Step {
    Step {
        id: id.into(),
        action: Action::Wait { seconds: 0.0 },
        retry: RetryPolicy::default(),
        enabled: true,
        breakpoint: false,
    }
}

fn click_step(id: &str, retry: RetryPolicy) -> Step {
    Step {
        id: id.into(),
        action: Action::Click {
            target: ClickTarget::Cursor,
            button: MouseButton::Left,
            click_kind: ClickKind::Single,
        },
        retry,
        enabled: true,
        breakpoint: false,
    }
}

/// Chains steps `id0 -> id1 -> ... -> idN` with plain (unnamed
/// port) connections, and sets entry to the first one — the graph
/// equivalent of "run this Vec<Step> in order" for tests that
/// don't care about branching/wiring specifics.
fn chain(ids: &[&str]) -> (Vec<Connection>, Option<String>) {
    let connections = ids
        .windows(2)
        .map(|pair| Connection {
            from: pair[0].into(),
            from_port: None,
            to: pair[1].into(),
        })
        .collect();
    (connections, ids.first().map(|s| s.to_string()))
}

/// The variables-panel runtime trace (`on_variables_changed`) is
/// meant to reflect the run's actual current state after every step,
/// not just fire once at the end.
#[test]
fn on_variables_changed_reflects_each_step_as_it_runs() {
    let (connections, entry) = chain(&["set_a", "set_b"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "set_a".into(),
                action: Action::SetVariable { name: "a".into(), value: "1".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "set_b".into(),
                action: Action::SetVariable { name: "b".into(), value: "2".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.last_variables.get("a"), Some(&"1".to_string()));
    assert_eq!(observer.last_variables.get("b"), Some(&"2".to_string()));
}

#[test]
fn runs_a_wait_only_flow_to_completion() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "wait_a_moment".into(),
            action: Action::Wait { seconds: 0.001 },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("wait_a_moment".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
}

#[test]
fn reports_progress_through_the_observer() {
    let (connections, entry) = chain(&["one", "two"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("one"), wait_step("two")],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(observer.started, vec!["one", "two"]);
}

/// A step with no incoming connection (and not `entry`) is
/// unreachable and must not run — the graph-model equivalent of a
/// free-floating, unwired block.
#[test]
fn a_step_with_no_incoming_connection_never_runs() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("start"), wait_step("orphan")],
        connections: vec![],
        entry: Some("start".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(observer.started, vec!["start"]);
}

#[test]
fn a_flow_with_no_entry_runs_nothing() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("unreachable")],
        connections: vec![],
        entry: None,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert!(observer.started.is_empty());
}

/// A cyclic wire (here, `b`'s plain output pointing back to `a`)
/// must fail cleanly instead of looping forever — the frontend
/// refuses to create this via the UI, but the engine has to defend
/// against it too (hand-edited YAML, future bugs, etc).
#[test]
fn a_circular_connection_fails_instead_of_looping_forever() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("a"), wait_step("b")],
        connections: vec![
            Connection {
                from: "a".into(),
                from_port: None,
                to: "b".into(),
            },
            Connection {
                from: "b".into(),
                from_port: None,
                to: "a".into(),
            },
        ],
        entry: Some("a".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
    // Both steps ran once before the cycle was caught on the
    // second visit to "a" — proves this terminates rather than
    // hanging, without over-specifying exactly which id the error
    // names.
    assert_eq!(observer.started, vec!["a", "b"]);
}

/// An `if` is a nested container exactly like `loop`: `then_branch`
/// and `else_branch` live inside `check_found`'s own `then`/
/// `otherwise` branches, not as siblings wired via a port.
#[test]
fn if_takes_the_yes_path_when_the_variable_matches() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "remember_found".into(),
                action: Action::SetVariable {
                    name: "found".into(),
                    value: "yes".into(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "check_found".into(),
                action: Action::If {
                    condition: Condition {
                        variable: "found".into(),
                        equals: "yes".into(),
                    },
                    then: Branch {
                        steps: vec![wait_step("then_branch")],
                        connections: vec![],
                        entry: Some("then_branch".into()),
                    },
                    otherwise: Branch {
                        steps: vec![wait_step("else_branch")],
                        connections: vec![],
                        entry: Some("else_branch".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: chain(&["remember_found", "check_found"]).0,
        entry: Some("remember_found".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(
        observer.started,
        vec!["remember_found", "check_found", "then_branch"]
    );
}

#[test]
fn if_takes_the_no_path_when_the_variable_is_unset() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "check_found".into(),
            action: Action::If {
                condition: Condition {
                    variable: "found".into(),
                    equals: "yes".into(),
                },
                then: Branch {
                    steps: vec![wait_step("then_branch")],
                    connections: vec![],
                    entry: Some("then_branch".into()),
                },
                otherwise: Branch {
                    steps: vec![wait_step("else_branch")],
                    connections: vec![],
                    entry: Some("else_branch".into()),
                },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("check_found".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(observer.started, vec!["check_found", "else_branch"]);
}

/// After either branch finishes, execution falls back out to the
/// `if` step's own plain output — both paths always rejoin here,
/// there's no way for one branch to lead somewhere the other doesn't.
#[test]
fn after_either_if_branch_finishes_execution_continues_from_the_if_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "check_found".into(),
                action: Action::If {
                    condition: Condition {
                        variable: "found".into(),
                        equals: "yes".into(),
                    },
                    then: Branch::default(),
                    otherwise: Branch {
                        steps: vec![wait_step("else_branch")],
                        connections: vec![],
                        entry: Some("else_branch".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            wait_step("after_if"),
        ],
        connections: chain(&["check_found", "after_if"]).0,
        entry: Some("check_found".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(
        observer.started,
        vec!["check_found", "else_branch", "after_if"]
    );
}

#[test]
fn loop_runs_its_body_the_requested_number_of_times() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "retry_loop".into(),
            action: Action::Loop {
                count: 3,
                body: Branch {
                    steps: vec![wait_step("inner")],
                    connections: vec![],
                    entry: Some("inner".into()),
                },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("retry_loop".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(
        observer.started,
        vec!["retry_loop", "inner", "inner", "inner"]
    );
}

#[test]
fn a_failure_inside_a_loop_stops_the_flow() {
    let (connections, entry) = chain(&["retry_loop", "never_reached"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "retry_loop".into(),
                action: Action::Loop {
                    count: 5,
                    body: Branch {
                        steps: vec![click_step("click_missing_button", RetryPolicy::default())],
                        connections: vec![],
                        entry: Some("click_missing_button".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            wait_step("never_reached"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    // Always-failing backend: the very first click inside the loop fails.
    let backend = MockBackend::fails_first(u32::MAX);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_err());
    assert!(!observer.started.contains(&"never_reached".to_string()));
}

#[test]
fn retry_policy_recovers_from_a_transient_failure() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![click_step(
            "click_flaky_button",
            RetryPolicy {
                max_attempts: 3,
                interval_ms: 0,
                on_failure: FailureBehavior::Fail,
            },
        )],
        connections: vec![],
        entry: Some("click_flaky_button".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    // Fails twice, then succeeds on the third attempt — within the
    // step's max_attempts of 3, so the flow should still succeed.
    let backend = MockBackend::fails_first(2);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 3);
}

#[test]
fn retry_policy_gives_up_after_max_attempts() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![click_step(
            "click_broken_button",
            RetryPolicy {
                max_attempts: 2,
                interval_ms: 0,
                on_failure: FailureBehavior::Fail,
            },
        )],
        connections: vec![],
        entry: Some("click_broken_button".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let backend = MockBackend::fails_first(u32::MAX);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_err());
    assert_eq!(backend.click_calls.get(), 2);
}

/// `on_failure: Skip` reports the failure to the observer (the UI
/// still shows the step as failed) but doesn't abort the flow over
/// it — the next step still runs.
#[test]
fn on_failure_skip_reports_the_failure_but_continues_to_the_next_step() {
    let (connections, entry) = chain(&["click_broken_button", "after"]);
    let mut steps = vec![click_step(
        "click_broken_button",
        RetryPolicy {
            max_attempts: 1,
            interval_ms: 0,
            on_failure: FailureBehavior::Skip,
        },
    )];
    steps.push(wait_step("after"));
    let flow = Flow {
        name: "test".into(),
        steps,
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::fails_first(u32::MAX);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(observer.started, vec!["click_broken_button", "after"]);
}

#[test]
fn clicking_by_element_goes_through_click_element_not_click() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "click_save_button".into(),
            action: Action::Click {
                target: ClickTarget::Element(ElementSelector {
                    window_title: Some("Invoices — Notepad".into()),
                    automation_id: None,
                    name: Some("Save".into()),
                    control_type: Some("Button".into()),
                }),
                button: MouseButton::Left,
                click_kind: ClickKind::Single,
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("click_save_button".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 1);
}

#[test]
fn find_image_retries_until_found_then_a_branch_can_use_it() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "find_save_button".into(),
            action: Action::FindImage {
                image: "save_button.png".into(),
                mode: MatchMode::Similar,
                threshold: 0.85,
                min_scale: 0.7,
                max_scale: 1.4,
                scale_steps: 12,
            },
            retry: RetryPolicy {
                max_attempts: 3,
                interval_ms: 0,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("find_save_button".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    // Not found on the first two captures, found on the third.
    let backend = MockBackend::fails_first(2);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 3);
}

/// A `Click` targeting `ClickTarget::LastMatch` clicks wherever the
/// preceding `find_image` actually found its match — MockBackend's
/// successful match is fixed at (42, 24), see `find_image`'s impl.
/// (`Click` itself never carries a target anymore — see
/// `ClickTarget`'s doc comment — so this is `MoveMouse`'s job.)
#[test]
fn move_mouse_targeting_last_match_uses_the_most_recent_find_image_result() {
    let (connections, entry) = chain(&["find_it", "move_to_it"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "find_it".into(),
                action: Action::FindImage {
                    image: "save_button.png".into(),
                    mode: MatchMode::Similar,
                    threshold: 0.85,
                    min_scale: 0.7,
                    max_scale: 1.4,
                    scale_steps: 12,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "move_to_it".into(),
                action: Action::MoveMouse {
                    target: PointTarget::LastMatch,
                    duration_ms: 0,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.last_point.get(), (42, 24));
}

/// `PointTarget::LastMatch` fails the step (rather than moving to
/// (0, 0)) when nothing has been found yet.
#[test]
fn move_mouse_targeting_last_match_fails_when_nothing_found_yet() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "move_to_it".into(),
            action: Action::MoveMouse {
                target: PointTarget::LastMatch,
                duration_ms: 0,
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("move_to_it".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
}

#[test]
fn find_image_fails_the_flow_when_never_found() {
    let (connections, entry) = chain(&["find_missing_icon", "never_reached"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "find_missing_icon".into(),
                action: Action::FindImage {
                    image: "missing.png".into(),
                    mode: MatchMode::Exact,
                    threshold: 0.85,
                    min_scale: 0.7,
                    max_scale: 1.4,
                    scale_steps: 12,
                },
                retry: RetryPolicy {
                    max_attempts: 2,
                    interval_ms: 0,
                    on_failure: FailureBehavior::Fail,
                },
                enabled: true,
                breakpoint: false,
            },
            wait_step("never_reached"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::fails_first(u32::MAX);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_err());
    assert!(!observer.started.contains(&"never_reached".to_string()));
}

#[test]
fn move_mouse_and_key_press_run_without_error() {
    let (connections, entry) = chain(&["move_1", "hold_shift"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "move_1".into(),
                action: Action::MoveMouse {
                    target: PointTarget::Coordinate(MonitorPoint {
                        monitor_id: "primary".into(),
                        x: 50,
                        y: 50,
                    }),
                    duration_ms: 0,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "hold_shift".into(),
                action: Action::KeyPress {
                    key: "shift".into(),
                    mode: KeyPressMode::Tap,
                    modifiers: KeyModifiers::default(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.started, vec!["move_1", "hold_shift"]);
}

/// A `KeyPress` step with `mode: Press` and no matching `Release`
/// anywhere in the flow must still get released once the run ends —
/// the whole point of the end-of-run safety net in
/// `run_flow_with_backend`, so a flow can never leave the real
/// keyboard's modifier state stuck down.
#[test]
fn an_unreleased_held_key_is_force_released_when_the_run_ends() {
    let (connections, entry) = chain(&["hold_ctrl"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "hold_ctrl".into(),
            action: Action::KeyPress {
                key: "ctrl".into(),
                mode: KeyPressMode::Press,
                modifiers: KeyModifiers::default(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(*backend.key_release_calls.borrow(), vec!["ctrl".to_string()]);
}

/// A `Press` followed by its matching `Release` must not be
/// force-released a second time at end of run.
#[test]
fn a_key_press_with_a_matching_release_is_only_released_once() {
    let (connections, entry) = chain(&["hold_ctrl", "release_ctrl"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "hold_ctrl".into(),
                action: Action::KeyPress {
                    key: "ctrl".into(),
                    mode: KeyPressMode::Press,
                    modifiers: KeyModifiers::default(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "release_ctrl".into(),
                action: Action::KeyPress {
                    key: "ctrl".into(),
                    mode: KeyPressMode::Release,
                    modifiers: KeyModifiers::default(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(*backend.key_release_calls.borrow(), vec!["ctrl".to_string()]);
}

#[test]
fn a_disabled_step_is_skipped_but_the_chain_continues() {
    let (connections, entry) = chain(&["before", "disabled_click", "after"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            wait_step("before"),
            Step {
                id: "disabled_click".into(),
                action: Action::Click {
                    target: ClickTarget::Cursor,
                    button: MouseButton::Left,
                    click_kind: ClickKind::Single,
                },
                retry: RetryPolicy::default(),
                enabled: false,
                breakpoint: false,
            },
            wait_step("after"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    // The disabled step never starts, and — critically — its click
    // never reaches the backend, proving it's truly skipped rather
    // than run-but-ignored. Execution still walks past it to "after".
    assert_eq!(observer.started, vec!["before", "after"]);
    assert_eq!(backend.click_calls.get(), 0);
}

fn stop_step(id: &str) -> Step {
    Step {
        id: id.into(),
        action: Action::Stop,
        retry: RetryPolicy::default(),
        enabled: true,
        breakpoint: false,
    }
}

#[test]
fn a_stop_step_ends_the_flow_early_without_failing() {
    let (connections, entry) = chain(&["before", "stop_here", "never_reached"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("before"), stop_step("stop_here"), wait_step("never_reached")],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    // Ending early on purpose is a success, not a failure.
    assert!(result.is_ok());
    assert_eq!(observer.started, vec!["before", "stop_here"]);
}

/// A `Stop` inside a `loop` body ends the *entire* run, not just
/// that one iteration — it must not silently get treated as "done
/// with this pass, start the next one".
#[test]
fn a_stop_step_inside_a_loop_body_ends_the_whole_run() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "retry_loop".into(),
                action: Action::Loop {
                    count: 5,
                    body: Branch {
                        steps: vec![wait_step("inner"), stop_step("stop_here")],
                        connections: vec![Connection {
                            from: "inner".into(),
                            from_port: None,
                            to: "stop_here".into(),
                        }],
                        entry: Some("inner".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            wait_step("never_reached"),
        ],
        connections: vec![Connection {
            from: "retry_loop".into(),
            from_port: None,
            to: "never_reached".into(),
        }],
        entry: Some("retry_loop".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    // Only the first iteration ran — no second "inner", and the
    // step after the loop never runs either.
    assert_eq!(observer.started, vec!["retry_loop", "inner", "stop_here"]);
}

#[test]
fn wait_for_window_retries_until_the_window_exists() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "wait_for_notepad".into(),
            action: Action::WaitForWindow {
                window_title: "Notepad".into(),
            },
            retry: RetryPolicy {
                max_attempts: 3,
                interval_ms: 0,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("wait_for_notepad".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    // Not open on the first two checks, open by the third.
    let backend = MockBackend::fails_first(2);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 3);
}

#[test]
fn wait_for_window_fails_the_flow_when_it_never_appears() {
    let (connections, entry) = chain(&["wait_for_notepad", "never_reached"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "wait_for_notepad".into(),
                action: Action::WaitForWindow {
                    window_title: "Notepad".into(),
                },
                retry: RetryPolicy {
                    max_attempts: 2,
                    interval_ms: 0,
                    on_failure: FailureBehavior::Fail,
                },
                enabled: true,
                breakpoint: false,
            },
            wait_step("never_reached"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let backend = MockBackend::fails_first(u32::MAX);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_err());
    assert!(!observer.started.contains(&"never_reached".to_string()));
}

/// `GetElementText` actually feeds the flow's own variable table —
/// proven by having an `if` right after it branch on the value it
/// just wrote, the same way a `SetVariable`-then-`If` pair would.
#[test]
fn get_element_text_writes_into_a_variable_the_flow_can_branch_on() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "read_label".into(),
                action: Action::GetElementText {
                    selector: ElementSelector::default(),
                    variable: "label_text".into(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "check_text".into(),
                action: Action::If {
                    condition: Condition {
                        variable: "label_text".into(),
                        equals: "mock text".into(),
                    },
                    then: Branch {
                        steps: vec![wait_step("matched")],
                        connections: vec![],
                        entry: Some("matched".into()),
                    },
                    otherwise: Branch {
                        steps: vec![wait_step("unmatched")],
                        connections: vec![],
                        entry: Some("unmatched".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: chain(&["read_label", "check_text"]).0,
        entry: Some("read_label".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(observer.started, vec!["read_label", "check_text", "matched"]);
}

#[test]
fn browser_get_text_writes_into_a_variable() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "read_price".into(),
                action: Action::BrowserGetText {
                    selector: ".price".into(),
                    variable: "price_text".into(),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "check_price".into(),
                action: Action::If {
                    condition: Condition {
                        variable: "price_text".into(),
                        equals: "mock browser text".into(),
                    },
                    then: Branch {
                        steps: vec![wait_step("matched")],
                        connections: vec![],
                        entry: Some("matched".into()),
                    },
                    otherwise: Branch {
                        steps: vec![wait_step("unmatched")],
                        connections: vec![],
                        entry: Some("unmatched".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: chain(&["read_price", "check_price"]).0,
        entry: Some("read_price".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false).unwrap();
    assert_eq!(observer.started, vec!["read_price", "check_price", "matched"]);
}

#[test]
fn browser_wait_for_selector_retries_until_it_shows_up() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "wait_results".into(),
            action: Action::BrowserWaitForSelector {
                selector: ".results".into(),
                instance: None,
            },
            retry: RetryPolicy {
                max_attempts: 3,
                interval_ms: 0,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("wait_results".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let backend = MockBackend::fails_first(2);
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 3);
}

#[test]
fn calculate_applies_each_operator_and_resolves_variables_first() {
    for (op, expect) in [
        (CalcOp::Add, "7"),
        (CalcOp::Subtract, "3"),
        (CalcOp::Multiply, "10"),
        (CalcOp::Divide, "2.5"),
    ] {
        let flow = Flow {
            name: "test".into(),
            steps: vec![
                Step {
                    id: "set_a".into(),
                    action: Action::SetVariable { name: "a".into(), value: "5".into() },
                    retry: RetryPolicy::default(),
                    enabled: true,
                    breakpoint: false,
                },
                Step {
                    id: "calc".into(),
                    action: Action::Calculate {
                        a: "%a%".into(),
                        op,
                        b: "2".into(),
                        variable: "result".into(),
                    },
                    retry: RetryPolicy::default(),
                    enabled: true,
                    breakpoint: false,
                },
            ],
            connections: chain(&["set_a", "calc"]).0,
            entry: Some("set_a".into()),
            step_delay_ms: 0,
        };
        let backend = MockBackend::always_succeeds();
        let mut ctx = ExecutionContext {
            initial_monitor_signature: backend.monitor_signature(),
            ..Default::default()
        };
        let mut observer = NullObserver;
        run_branch(&flow.steps, &flow.connections, flow.entry.as_deref(), &mut ctx, &mut observer, &backend)
            .unwrap();
        assert_eq!(ctx.variables.get("result").map(String::as_str), Some(expect));
    }
}

#[test]
fn calculate_dividing_by_zero_fails_the_step_instead_of_producing_inf() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "calc".into(),
            action: Action::Calculate { a: "1".into(), op: CalcOp::Divide, b: "0".into(), variable: "r".into() },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("calc".into()),
        step_delay_ms: 0,
    };
    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
}

#[test]
fn calculate_rounds_to_the_requested_decimal_places() {
    for (op, expect) in [(CalcOp::Round, "3.14"), (CalcOp::Floor, "3.14"), (CalcOp::Ceil, "3.15")] {
        let flow = Flow {
            name: "test".into(),
            steps: vec![Step {
                id: "calc".into(),
                action: Action::Calculate {
                    a: "3.14159".into(),
                    op,
                    b: "2".into(),
                    variable: "r".into(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            }],
            connections: vec![],
            entry: Some("calc".into()),
            step_delay_ms: 0,
        };
        let mut observer = NullObserver;
        let backend = MockBackend::always_succeeds();
        let mut ctx = ExecutionContext {
            initial_monitor_signature: backend.monitor_signature(),
            ..Default::default()
        };
        run_branch(
            &flow.steps,
            &flow.connections,
            flow.entry.as_deref(),
            &mut ctx,
            &mut observer,
            &backend,
        )
        .unwrap();
        assert_eq!(ctx.variables.get("r").map(String::as_str), Some(expect));
    }
}

#[test]
fn calculate_rounding_with_negative_decimal_places_fails_the_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "calc".into(),
            action: Action::Calculate {
                a: "3.14".into(),
                op: CalcOp::Round,
                b: "-1".into(),
                variable: "r".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("calc".into()),
        step_delay_ms: 0,
    };
    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
}

/// A monitor layout change mid-run pauses (notifying the observer)
/// rather than clicking blind at now-invalid coordinates, and
/// resumes on its own once the layout matches again — no separate
/// "resume" action needed.
#[test]
fn a_monitor_change_mid_run_pauses_and_then_resumes_once_restored() {
    let (connections, entry) = chain(&["one", "two"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![wait_step("one"), wait_step("two")],
        connections,
        entry,
        step_delay_ms: 0,
    };
    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::mismatched_monitor_once(), false);
    assert!(result.is_ok());
    assert_eq!(observer.monitor_mismatches, 1);
    assert_eq!(observer.monitor_restores, 1);
    assert_eq!(observer.started, vec!["one", "two"]);
}
#[test]
fn a_disabled_step_never_pauses_even_with_a_breakpoint() {
    let (connections, entry) = chain(&["before", "disabled_with_breakpoint", "after"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            wait_step("before"),
            Step {
                id: "disabled_with_breakpoint".into(),
                action: Action::Wait { seconds: 0.0 },
                retry: RetryPolicy::default(),
                enabled: false,
                breakpoint: true,
            },
            wait_step("after"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.paused_at, Vec::<String>::new());
    assert_eq!(observer.resumes, 0);
    assert_eq!(observer.started, vec!["before", "after"]);
}

// `request_stop`/`clear_stop`/`is_stop_requested` aren't covered by
// an automated test here: `STOP_REQUESTED` is a single process-wide
// static, and cargo runs this crate's tests in parallel on the
// same process — a test that calls `request_stop()` would race
// every other concurrently-running test's `run_flow_with_backend`
// call (which clears the flag at its own start and checks it at
// every step), risking spurious failures in unrelated tests rather
// than testing anything reliably. The mechanism itself is a single
// `AtomicBool` read/write guarding a couple of early-returns —
// reviewed by hand instead.
//
// `STEP_MODE`/`RESUME`/`request_step`/`request_continue` are the
// same story: process-wide statics that only a real paused run
// (from another thread) would ever resume, and `reset_debug_state`
// is called at the start of every `run_flow_with_backend` the same
// way `clear_stop` is — so a test that pokes `RESUME` from a
// spawned thread would race every other concurrently-running
// test's own run just like `request_stop()` would. Covered by
// `a_disabled_step_never_pauses_even_with_a_breakpoint` below
// instead, which only ever exercises the `step.enabled` guard and
// never actually enters the pause loop.

/// A `try_branch` step that always fails routes execution into
/// `catch` instead of aborting the whole flow — and `catch` can see
/// what went wrong via the `caught_error` variable, no dedicated
/// "image if"/"try if" node needed (see `Action::TryCatch`'s doc
/// comment). Uses `run_branch` directly (rather than
/// `run_flow_with_backend`) so the test can inspect `ctx.variables`
/// afterward — same pattern `calculate` operator tests already use.
#[test]
fn try_catch_runs_catch_and_sets_caught_error_when_try_fails() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "try_it".into(),
            action: Action::TryCatch {
                try_branch: Branch {
                    steps: vec![click_step(
                        "broken_click",
                        RetryPolicy { max_attempts: 1, interval_ms: 0, on_failure: FailureBehavior::Fail },
                    )],
                    connections: vec![],
                    entry: Some("broken_click".into()),
                },
                catch: Branch {
                    steps: vec![wait_step("handle_error")],
                    connections: vec![],
                    entry: Some("handle_error".into()),
                },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("try_it".into()),
        step_delay_ms: 0,
    };

    let backend = MockBackend::fails_first(u32::MAX);
    let mut ctx = ExecutionContext { initial_monitor_signature: backend.monitor_signature(), ..Default::default() };
    let mut observer = RecordingObserver::new();
    let result = run_branch(&flow.steps, &flow.connections, flow.entry.as_deref(), &mut ctx, &mut observer, &backend);

    assert!(result.is_ok(), "a caught failure must not abort the flow");
    assert_eq!(observer.started, vec!["try_it", "broken_click", "handle_error"]);
    assert_eq!(ctx.variables.get("caught_error").map(String::as_str), Some("mock click failure"));
}

/// The mirror case: `try_branch` succeeding means `catch` never runs
/// at all, and nothing is left behind claiming an error happened.
#[test]
fn try_catch_skips_catch_when_try_succeeds() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "try_it".into(),
            action: Action::TryCatch {
                try_branch: Branch {
                    steps: vec![click_step("working_click", RetryPolicy::default())],
                    connections: vec![],
                    entry: Some("working_click".into()),
                },
                catch: Branch {
                    steps: vec![wait_step("handle_error")],
                    connections: vec![],
                    entry: Some("handle_error".into()),
                },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("try_it".into()),
        step_delay_ms: 0,
    };

    let backend = MockBackend::always_succeeds();
    let mut ctx = ExecutionContext { initial_monitor_signature: backend.monitor_signature(), ..Default::default() };
    let mut observer = RecordingObserver::new();
    let result = run_branch(&flow.steps, &flow.connections, flow.entry.as_deref(), &mut ctx, &mut observer, &backend);

    assert!(result.is_ok());
    assert_eq!(observer.started, vec!["try_it", "working_click"]);
    assert_eq!(ctx.variables.get("caught_error"), None);
}

/// A `FunctionDef` step just sits inertly in `flow.steps` (never
/// itself reached by a wire) — `run_flow_with_backend` collects its
/// `body` into `ctx.functions` before the run starts, and a
/// `CallFunction` step elsewhere runs that body then returns to
/// whatever follows the call, exactly like a plain Rust function
/// call/return (no separate call-stack data structure — `run_branch`
/// recursing into the function's `Branch` and returning *is* the
/// return).
#[test]
fn call_function_runs_the_named_functions_body_and_returns_to_the_caller() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "greet_fn".into(),
                action: Action::FunctionDef {
                    name: "greet".into(),
                    body: Branch { steps: vec![wait_step("inside_fn")], connections: vec![], entry: Some("inside_fn".into()) },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "call_it".into(),
                action: Action::CallFunction { name: "greet".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            wait_step("after"),
        ],
        connections: chain(&["call_it", "after"]).0,
        entry: Some("call_it".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    // "greet_fn" itself never runs (nothing wires into it) — only its
    // body, via the call, then whatever follows the call step.
    assert_eq!(observer.started, vec!["call_it", "inside_fn", "after"]);
}

/// The same function, called from two unrelated places, runs its body
/// twice — proving it's genuinely reusable, not tied to one call site
/// the way a `Loop`'s body is tied to that one `loop` node.
#[test]
fn call_function_runs_again_from_a_second_unrelated_call_site() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "click_fn".into(),
                action: Action::FunctionDef {
                    name: "click_it".into(),
                    body: Branch {
                        steps: vec![click_step("do_click", RetryPolicy::default())],
                        connections: vec![],
                        entry: Some("do_click".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "call_1".into(),
                action: Action::CallFunction { name: "click_it".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "call_2".into(),
                action: Action::CallFunction { name: "click_it".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: chain(&["call_1", "call_2"]).0,
        entry: Some("call_1".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let backend = MockBackend::always_succeeds();
    let result = run_flow_with_backend(&flow, &mut observer, &backend, false);
    assert!(result.is_ok());
    assert_eq!(backend.click_calls.get(), 2);
}

#[test]
fn call_function_fails_the_flow_when_the_name_is_undefined() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "call_missing".into(),
            action: Action::CallFunction { name: "does_not_exist".into() },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("call_missing".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("does_not_exist"));
}

/// A function whose own body calls itself is rejected instead of
/// blowing the (real, native) call stack — same defensive posture as
/// `run_branch`'s `visited` set catching a step-level wiring cycle,
/// just for the call graph instead of one branch's wiring.
#[test]
fn call_function_fails_when_it_recurses_into_itself() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "recursive_fn".into(),
                action: Action::FunctionDef {
                    name: "loopy".into(),
                    body: Branch {
                        steps: vec![Step {
                            id: "call_self".into(),
                            action: Action::CallFunction { name: "loopy".into() },
                            retry: RetryPolicy::default(),
                            enabled: true,
                            breakpoint: false,
                        }],
                        connections: vec![],
                        entry: Some("call_self".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "call_it".into(),
                action: Action::CallFunction { name: "loopy".into() },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![],
        entry: Some("call_it".into()),
        step_delay_ms: 0,
    };

    let mut observer = NullObserver;
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("recursive"));
}

fn action_step(id: &str, action: Action) -> Step {
    Step {
        id: id.into(),
        action,
        retry: RetryPolicy::default(),
        enabled: true,
        breakpoint: false,
    }
}

/// `Break` ends the loop immediately (skipping any remaining
/// iterations) rather than merely skipping the rest of the current
/// one — a `Loop { count: 3, .. }` whose body breaks on its very
/// first step must still only run that step once, not three times.
#[test]
fn break_ends_the_loop_immediately() {
    let (body_connections, body_entry) = chain(&["iter", "break_step"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![action_step(
            "loop_3",
            Action::Loop {
                count: 3,
                body: Branch {
                    steps: vec![wait_step("iter"), action_step("break_step", Action::Break)],
                    connections: body_connections,
                    entry: body_entry,
                },
            },
        )],
        connections: vec![],
        entry: Some("loop_3".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.started.iter().filter(|id| *id == "iter").count(), 1);
}

/// `Continue` skips the rest of the current iteration but still runs
/// every iteration the loop was asked for — the step after `Continue`
/// in the body never runs, while the step before it runs once per
/// iteration.
#[test]
fn continue_skips_the_rest_of_the_current_iteration_only() {
    let (body_connections, body_entry) = chain(&["each_iter", "continue_step", "never_reached"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![action_step(
            "loop_3",
            Action::Loop {
                count: 3,
                body: Branch {
                    steps: vec![
                        wait_step("each_iter"),
                        action_step("continue_step", Action::Continue),
                        wait_step("never_reached"),
                    ],
                    connections: body_connections,
                    entry: body_entry,
                },
            },
        )],
        connections: vec![],
        entry: Some("loop_3".into()),
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.started.iter().filter(|id| *id == "each_iter").count(), 3);
    assert!(!observer.started.contains(&"never_reached".to_string()));
}

/// `Return` ends the function call right there — the rest of the
/// function's body never runs — but the flow's own chain continues
/// normally from whatever came after the `CallFunction` step.
#[test]
fn return_ends_the_function_call_but_the_caller_continues() {
    let (fn_connections, fn_entry) = chain(&["before_return", "return_step", "never_reached"]);
    let (connections, entry) = chain(&["call_it", "after_call"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            action_step(
                "define_fn",
                Action::FunctionDef {
                    name: "early_return".into(),
                    body: Branch {
                        steps: vec![
                            wait_step("before_return"),
                            action_step("return_step", Action::Return),
                            wait_step("never_reached"),
                        ],
                        connections: fn_connections,
                        entry: fn_entry,
                    },
                },
            ),
            action_step("call_it", Action::CallFunction { name: "early_return".into() }),
            wait_step("after_call"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert!(observer.started.contains(&"before_return".to_string()));
    assert!(!observer.started.contains(&"never_reached".to_string()));
    assert!(observer.started.contains(&"after_call".to_string()));
}

/// `Return` inside a `Loop` inside a function passes straight through
/// the loop boundary — it ends the whole function call, not just that
/// one loop iteration, so the loop must not run again after it.
#[test]
fn return_inside_a_loop_still_returns_the_whole_function() {
    let (loop_body_connections, loop_body_entry) = chain(&["loop_iter", "return_step"]);
    let (fn_connections, fn_entry) = chain(&["loop_5", "never_reached"]);
    let (connections, entry) = chain(&["call_it", "after_call"]);
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            action_step(
                "define_fn",
                Action::FunctionDef {
                    name: "returns_in_loop".into(),
                    body: Branch {
                        steps: vec![
                            action_step(
                                "loop_5",
                                Action::Loop {
                                    count: 5,
                                    body: Branch {
                                        steps: vec![wait_step("loop_iter"), action_step("return_step", Action::Return)],
                                        connections: loop_body_connections,
                                        entry: loop_body_entry,
                                    },
                                },
                            ),
                            wait_step("never_reached"),
                        ],
                        connections: fn_connections,
                        entry: fn_entry,
                    },
                },
            ),
            action_step("call_it", Action::CallFunction { name: "returns_in_loop".into() }),
            wait_step("after_call"),
        ],
        connections,
        entry,
        step_delay_ms: 0,
    };

    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.started.iter().filter(|id| *id == "loop_iter").count(), 1);
    assert!(!observer.started.contains(&"never_reached".to_string()));
    assert!(observer.started.contains(&"after_call".to_string()));
}

/// A handful of `TextTransform` operations, run through the full
/// engine (not just `automation::text_transform` directly) so the
/// variable-writing side is covered too.
#[test]
fn text_transform_covers_a_few_representative_operations() {
    let cases: &[(TextOp, &str, &str, &str, &str)] = &[
        (TextOp::Uppercase, "hello", "", "", "HELLO"),
        (TextOp::Trim, "  hi  ", "", "", "hi"),
        (TextOp::Replace, "a-b-c", "-", "_", "a_b_c"),
        (TextOp::Length, "hello", "", "", "5"),
        (TextOp::Contains, "hello world", "world", "", "true"),
        (TextOp::Base64Encode, "hi", "", "", "aGk="),
        (TextOp::Sha256, "", "", "", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        (TextOp::JsonGet, r#"{"user":{"name":"Ada"}}"#, "user.name", "", "Ada"),
        (TextOp::JsonGet, r#"{"items":[{"id":1},{"id":2}]}"#, "items[1].id", "", "2"),
        (TextOp::JsonEscape, "line\"break", "", "", "line\\\"break"),
        (TextOp::RegexTest, "hello123", r"\d+", "", "true"),
        (TextOp::RegexTest, "hello", r"\d+", "", "false"),
        (TextOp::RegexMatch, "hello123world", r"(\d+)", "1", "123"),
    ];
    for (op, text, arg1, arg2, expected) in cases {
        let flow = Flow {
            name: "test".into(),
            steps: vec![action_step(
                "transform",
                Action::TextTransform {
                    op: *op,
                    text: (*text).into(),
                    arg1: (*arg1).into(),
                    arg2: (*arg2).into(),
                    variable: "result".into(),
                },
            )],
            connections: vec![],
            entry: Some("transform".into()),
            step_delay_ms: 0,
        };
        let mut observer = RecordingObserver::new();
        let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
        assert!(result.is_ok());
        assert_eq!(observer.last_variables.get("result"), Some(&expected.to_string()));
    }
}

#[test]
fn generate_random_stays_within_the_requested_range() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![action_step(
            "roll",
            Action::GenerateRandom { min: "1".into(), max: "6".into(), variable: "roll".into() },
        )],
        connections: vec![],
        entry: Some("roll".into()),
        step_delay_ms: 0,
    };
    for _ in 0..20 {
        let mut observer = RecordingObserver::new();
        let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
        assert!(result.is_ok());
        let value: i64 = observer.last_variables.get("roll").expect("roll should be set").parse().expect("should be a number");
        assert!((1..=6).contains(&value), "rolled {value}, expected 1..=6");
    }
}

#[test]
fn get_env_var_writes_into_a_variable() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![action_step(
            "read_env",
            Action::GetEnvVar { name: "PATH".into(), variable: "path_value".into() },
        )],
        connections: vec![],
        entry: Some("read_env".into()),
        step_delay_ms: 0,
    };
    let mut observer = RecordingObserver::new();
    let result = run_flow_with_backend(&flow, &mut observer, &MockBackend::always_succeeds(), false);
    assert!(result.is_ok());
    assert_eq!(observer.last_variables.get("path_value"), Some(&"mock-PATH".to_string()));
}
