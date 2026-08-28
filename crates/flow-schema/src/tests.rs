use super::*;

#[test]
fn roundtrips_a_minimal_flow() {
    let flow = Flow {
        name: "Save report".into(),
        steps: vec![Step {
            id: "click_save".into(),
            action: Action::Click {
                target: ClickTarget::Cursor,
                button: MouseButton::Left,
                click_kind: ClickKind::Single,
            },
            retry: RetryPolicy {
                max_attempts: 3,
                interval_ms: 500,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("click_save".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_calculate_step_and_a_nonzero_step_delay() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "sum".into(),
            action: Action::Calculate {
                a: "{{price}}".into(),
                op: CalcOp::Multiply,
                b: "{{quantity}}".into(),
                variable: "subtotal".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("sum".into()),
        step_delay_ms: 250,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: calculate"));
    assert!(yaml.contains("op: multiply"));
    assert!(yaml.contains("step_delay_ms: 250"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

/// A flow with no `step_delay_ms` at all (an older saved file)
/// still parses, defaulting to no delay — and the field is left
/// out of freshly-serialized output when it's zero, so an
/// untouched flow's YAML doesn't grow a meaningless line.
#[test]
fn step_delay_ms_defaults_to_zero_and_is_omitted_when_zero() {
    let legacy = "name: test\nsteps: []\nconnections: []\n";
    let parsed = parse_flow(legacy).expect("legacy flow without step_delay_ms should still parse");
    assert_eq!(parsed.step_delay_ms, 0);
    let yaml = to_yaml(&parsed).expect("serialize");
    assert!(!yaml.contains("step_delay_ms"));
}

#[test]
fn rejects_malformed_yaml() {
    let result = parse_flow("steps: [this is not a step]");
    assert!(result.is_err());
}

/// An `if` is a nested container exactly like `loop`: `type_result`
/// (the `then` branch) and nothing (the `otherwise` branch, empty —
/// there's nothing to do) live inside `check_found`'s own `then`/
/// `otherwise`, not as siblings wired via a port.
#[test]
fn roundtrips_branch_and_loop_steps() {
    let flow = Flow {
        name: "Branch and loop".into(),
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
                        steps: vec![Step {
                            id: "type_result".into(),
                            action: Action::TypeText {
                                text: "matched".into(),
                            },
                            retry: RetryPolicy::default(),
                            enabled: true,
                            breakpoint: false,
                        }],
                        connections: vec![],
                        entry: Some("type_result".into()),
                    },
                    otherwise: Branch::default(),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "retry_loop".into(),
                action: Action::Loop {
                    count: 3,
                    body: Branch {
                        steps: vec![Step {
                            id: "wait_a_bit".into(),
                            action: Action::Wait { seconds: 1.0 },
                            retry: RetryPolicy::default(),
                            enabled: true,
                            breakpoint: false,
                        }],
                        connections: vec![],
                        entry: Some("wait_a_bit".into()),
                    },
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![
            Connection {
                from: "remember_found".into(),
                from_port: None,
                to: "check_found".into(),
            },
            // The `if`'s own single output — reached regardless of
            // which branch actually ran.
            Connection {
                from: "check_found".into(),
                from_port: None,
                to: "retry_loop".into(),
            },
        ],
        entry: Some("remember_found".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_try_catch_step() {
    let flow = Flow {
        name: "Try catch".into(),
        steps: vec![Step {
            id: "attempt_save".into(),
            action: Action::TryCatch {
                try_branch: Branch {
                    steps: vec![Step {
                        id: "click_save".into(),
                        action: Action::Click {
                            target: ClickTarget::Cursor,
                            button: MouseButton::Left,
                            click_kind: ClickKind::Single,
                        },
                        retry: RetryPolicy::default(),
                        enabled: true,
                        breakpoint: false,
                    }],
                    connections: vec![],
                    entry: Some("click_save".into()),
                },
                catch: Branch {
                    steps: vec![Step {
                        id: "log_failure".into(),
                        action: Action::TypeText {
                            text: "save failed: {{caught_error}}".into(),
                        },
                        retry: RetryPolicy::default(),
                        enabled: true,
                        breakpoint: false,
                    }],
                    connections: vec![],
                    entry: Some("log_failure".into()),
                },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("attempt_save".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: try_catch"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

/// Matches the shape `buildFlowYaml(INITIAL_FLOW)` in
/// `src/data/flowModel.ts` produces on the frontend — that's the
/// flow the app starts with and what Run sends by default.
/// If this test breaks, the frontend's starting flow is broken too.
#[test]
fn parses_the_frontend_demo_flow() {
    let yaml = r#"name: Backend smoke test
steps:
  - id: start
    type: set_variable
    name: ready
    value: "yes"
  - id: check_ready
    type: if
    condition:
      variable: ready
      equals: "yes"
    then:
      steps:
        - id: wait_ok
          type: wait
          seconds: 0.6
      connections: []
      entry: wait_ok
    otherwise:
      steps:
        - id: wait_fallback
          type: wait
          seconds: 0.6
      connections: []
      entry: wait_fallback
  - id: finish
    type: wait
    seconds: 0.3
connections:
  - from: start
    to: check_ready
  - from: check_ready
    to: finish
entry: start
"#;
    let flow = parse_flow(yaml).expect("frontend demo flow must parse");
    assert_eq!(flow.steps.len(), 3);
}

#[test]
fn roundtrips_a_click_by_element() {
    let flow = Flow {
        name: "Click by element".into(),
        steps: vec![Step {
            id: "click_save".into(),
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
        entry: Some("click_save".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_find_image_step() {
    let flow = Flow {
        name: "Find image".into(),
        steps: vec![Step {
            id: "find_save_button".into(),
            action: Action::FindImage {
                image: "assets/save_button.png".into(),
                mode: MatchMode::Similar,
                threshold: 0.85,
                min_scale: 0.7,
                max_scale: 1.4,
                scale_steps: 12,
            },
            retry: RetryPolicy {
                max_attempts: 3,
                interval_ms: 500,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("find_save_button".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

/// `image: "target.png"` (a bare string) still parses straight into
/// `ImageSource::Path` — every flow saved before embedding existed
/// keeps working unchanged.
#[test]
fn find_image_embedded_data_roundtrips_and_a_bare_path_string_still_parses() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "find_logo".into(),
            action: Action::FindImage {
                image: ImageSource::Embedded { data: "aGVsbG8=".into() },
                mode: MatchMode::Exact,
                threshold: 0.85,
                min_scale: 0.7,
                max_scale: 1.4,
                scale_steps: 12,
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("find_logo".into()),
        step_delay_ms: 0,
    };
    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);

    let legacy = "name: test\nsteps:\n  - id: a\n    type: find_image\n    image: \"target.png\"\nconnections: []\n";
    let parsed_legacy = parse_flow(legacy).expect("legacy plain-string image should still parse");
    match &parsed_legacy.steps[0].action {
        Action::FindImage { image, .. } => assert_eq!(*image, ImageSource::Path("target.png".into())),
        other => panic!("expected FindImage, got {other:?}"),
    }
}

#[test]
fn roundtrips_a_find_text_ocr_step_and_a_skip_on_failure_policy() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "find_sign_in".into(),
            action: Action::FindTextOcr { text: "Sign in".into(), region: None },
            retry: RetryPolicy {
                max_attempts: 5,
                interval_ms: 200,
                on_failure: FailureBehavior::Skip,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("find_sign_in".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: find_text_ocr"));
    assert!(yaml.contains("on_failure: skip"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);

    // A step delay's-worth of region-scoping: OCR limited to just
    // part of the screen, so it doesn't accidentally match unrelated
    // text elsewhere.
    let scoped = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "find_sign_in".into(),
            action: Action::FindTextOcr {
                text: "Sign in".into(),
                region: Some(CaptureRegion { x: 10, y: 20, width: 200, height: 80 }),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("find_sign_in".into()),
        step_delay_ms: 0,
    };
    let scoped_yaml = to_yaml(&scoped).expect("serialize");
    let scoped_parsed = parse_flow(&scoped_yaml).expect("deserialize");
    assert_eq!(scoped, scoped_parsed);

    // The default (`Fail`) is omitted from freshly-serialized YAML.
    let default_yaml = to_yaml(&Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "a".into(),
            action: Action::Wait { seconds: 0.0 },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("a".into()),
        step_delay_ms: 0,
    })
    .expect("serialize");
    assert!(!default_yaml.contains("on_failure"));
}

#[test]
fn find_image_mode_defaults_to_exact() {
    let yaml = r#"name: test
steps:
  - id: find_it
    type: find_image
    image: "logo.png"
"#;
    let flow = parse_flow(yaml).expect("must parse with defaults");
    match &flow.steps[0].action {
        Action::FindImage {
            mode, threshold, ..
        } => {
            assert_eq!(*mode, MatchMode::Exact);
            assert_eq!(*threshold, 0.85);
        }
        other => panic!("expected FindImage, got {other:?}"),
    }
}

/// Kept in sync with the exact YAML shape `nodeYamlLines` in
/// `src/data/flowModel.ts` emits for each leaf kind the palette
/// can append. If this test breaks, the frontend's "click a
/// palette item to add a real step" flow is broken too.
#[test]
fn parses_every_step_kind_the_frontend_palette_can_append() {
    let yaml = r#"name: user-built tail
steps:
  - id: start_1
    type: start
  - id: wait_1
    type: wait
    seconds: 1
  - id: set_variable_1
    type: set_variable
    name: my_var
    value: "value"
  - id: type_text_1
    type: type_text
    text: "Hello"
  - id: click_1
    type: click
    target:
      kind: cursor
    button: left
    click_kind: single
  - id: move_mouse_1
    type: move_mouse
    target:
      kind: coordinate
      monitor_id: primary
      x: 100
      y: 100
    duration_ms: 0
  - id: key_press_1
    type: key_press
    key: enter
    hold_ms: 0
  - id: find_image_1
    type: find_image
    image: "target.png"
    mode: similar
    threshold: 0.85
"#;
    let flow = parse_flow(yaml).expect("frontend-appendable step kinds must all parse");
    assert_eq!(flow.steps.len(), 8);
}

#[test]
fn roundtrips_a_start_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "start_1".into(),
            action: Action::Start,
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("start_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: start"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_an_error_handler_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "handler_1".into(),
            action: Action::ErrorHandler,
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("handler_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: error_handler"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_stop_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "stop_1".into(),
            action: Action::Stop,
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("stop_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: stop"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_wait_for_window_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "wait_for_window_1".into(),
            action: Action::WaitForWindow {
                window: "Notepad".into(),
                timeout_ms: 10_000,
            },
            retry: RetryPolicy {
                max_attempts: 5,
                interval_ms: 500,
                on_failure: FailureBehavior::Fail,
            },
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("wait_for_window_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: wait_for_window"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_launch_app_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "launch_1".into(),
            action: Action::LaunchApp {
                path: "notepad.exe".into(),
                args: "C:\\temp\\notes.txt".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("launch_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_an_open_url_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "open_1".into(),
            action: Action::OpenUrl {
                url: "https://example.com".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("open_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_notify_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "notify_1".into(),
            action: Action::Notify {
                title: "Done".into(),
                message: "The flow finished.".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("notify_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_every_file_operation_step() {
    fn leaf(id: &str, action: Action) -> Step {
        Step { id: id.into(), action, retry: RetryPolicy::default(), enabled: true, breakpoint: false }
    }

    let steps = vec![
        leaf("read", Action::ReadFile { path: "C:\\notes.txt".into(), variable: "notes".into() }),
        leaf("write", Action::WriteFile { path: "C:\\notes.txt".into(), content: "hello".into(), append: true }),
        leaf("copy", Action::CopyFile { source: "a.txt".into(), destination: "b.txt".into() }),
        leaf("move", Action::MoveFile { source: "b.txt".into(), destination: "c.txt".into() }),
        leaf("delete", Action::DeleteFile { path: "c.txt".into() }),
        leaf("mkdir", Action::CreateDirectory { path: "C:\\output".into() }),
        leaf("list", Action::ListDirectory { path: "C:\\output".into(), variable: "entries".into() }),
    ];
    let (connections, entry) = {
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
        let connections = ids
            .windows(2)
            .map(|pair| Connection { from: pair[0].into(), from_port: None, to: pair[1].into() })
            .collect();
        (connections, ids.first().map(|s| s.to_string()))
    };

    let flow = Flow { name: "File ops".into(), steps, connections, entry, step_delay_ms: 0 };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: read_file"));
    assert!(yaml.contains("type: write_file"));
    assert!(yaml.contains("type: copy_file"));
    assert!(yaml.contains("type: move_file"));
    assert!(yaml.contains("type: delete_file"));
    assert!(yaml.contains("type: create_directory"));
    assert!(yaml.contains("type: list_directory"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_an_http_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "post_it".into(),
            action: Action::Http {
                method: HttpMethod::Post,
                url: "https://example.com/api".into(),
                headers: "Content-Type: application/json\nAuthorization: Bearer {{token}}".into(),
                body: "{\"ok\":true}".into(),
                variable: "response".into(),
                status_variable: "response_status".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("post_it".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: http"));
    assert!(yaml.contains("method: post"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_every_system_and_clipboard_step() {
    fn leaf(id: &str, action: Action) -> Step {
        Step { id: id.into(), action, retry: RetryPolicy::default(), enabled: true, breakpoint: false }
    }

    let steps = vec![
        leaf("focus", Action::FocusWindow { window: "Notepad".into() }),
        leaf("lock", Action::LockWorkstation),
        leaf("read_cb", Action::ReadClipboard { variable: "clip".into() }),
        leaf("write_cb", Action::WriteClipboard { text: "hello".into() }),
        leaf("restart", Action::PowerAction { mode: PowerMode::Restart, force: true }),
        leaf("shutdown", Action::PowerAction { mode: PowerMode::Shutdown, force: false }),
    ];
    let (connections, entry) = {
        let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
        let connections = ids
            .windows(2)
            .map(|pair| Connection { from: pair[0].into(), from_port: None, to: pair[1].into() })
            .collect();
        (connections, ids.first().map(|s| s.to_string()))
    };

    let flow = Flow { name: "System and clipboard".into(), steps, connections, entry, step_delay_ms: 0 };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: focus_window"));
    assert!(yaml.contains("type: lock_workstation"));
    assert!(yaml.contains("type: read_clipboard"));
    assert!(yaml.contains("type: write_clipboard"));
    assert!(yaml.contains("type: power_action"));
    assert!(yaml.contains("mode: restart"));
    assert!(yaml.contains("mode: shutdown"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_launch_browser_step_and_an_instance_targeted_navigate() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "open_tab".into(),
                action: Action::LaunchBrowser {
                    url: "https://example.com".into(),
                    variable: "tab".into(),
                    browser: Some("chrome".into()),
                    profile_dir: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "nav_that_tab".into(),
                action: Action::BrowserNavigate {
                    url: "https://example.com/page2".into(),
                    instance: Some("{{tab}}".into()),
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![Connection { from: "open_tab".into(), from_port: None, to: "nav_that_tab".into() }],
        entry: Some("open_tab".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: launch_browser"));
    assert!(yaml.contains("{{tab}}"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_function_def_and_a_call_to_it() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "greet_fn".into(),
                action: Action::FunctionDef {
                    name: "greet".into(),
                    body: Branch {
                        steps: vec![Step {
                            id: "say_hi".into(),
                            action: Action::TypeText { text: "hi".into() },
                            retry: RetryPolicy::default(),
                            enabled: true,
                            breakpoint: false,
                        }],
                        connections: vec![],
                        entry: Some("say_hi".into()),
                    },
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
        ],
        connections: vec![],
        entry: Some("call_it".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: function_def"));
    assert!(yaml.contains("type: call_function"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_get_element_text_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "read_1".into(),
            action: Action::GetElementText {
                selector: ElementSelector {
                    window_title: Some("Invoices — Notepad".into()),
                    automation_id: None,
                    name: Some("Total".into()),
                    control_type: None,
                },
                variable: "total_text".into(),
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("read_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_every_browser_step_kind() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "nav_1".into(),
                action: Action::BrowserNavigate {
                    url: "https://example.com".into(),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "click_1".into(),
                action: Action::BrowserClick {
                    selector: "#submit".into(),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "get_text_1".into(),
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
                id: "set_value_1".into(),
                action: Action::BrowserSetValue {
                    selector: "#search".into(),
                    value: "hello".into(),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "wait_1".into(),
                action: Action::BrowserWaitForSelector {
                    selector: ".results".into(),
                    instance: None,
                },
                retry: RetryPolicy {
                    max_attempts: 10,
                    interval_ms: 500,
                    on_failure: FailureBehavior::Fail,
                },
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![],
        entry: Some("nav_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("type: browser_navigate"));
    assert!(yaml.contains("type: browser_click"));
    assert!(yaml.contains("type: browser_get_text"));
    assert!(yaml.contains("type: browser_set_value"));
    assert!(yaml.contains("type: browser_wait_for_selector"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn browser_selector_supports_text_and_attribute_kinds_and_plain_css_strings() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "click_css".into(),
                action: Action::BrowserClick {
                    selector: "#submit".into(),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "click_text".into(),
                action: Action::BrowserClick {
                    selector: BrowserSelector::Other(BrowserSelectorSpec::Text { value: "Log in".into() }),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "click_attr".into(),
                action: Action::BrowserClick {
                    selector: BrowserSelector::Other(BrowserSelectorSpec::Attribute {
                        name: "placeholder".into(),
                        value: "Search".into(),
                    }),
                    instance: None,
                },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![],
        entry: Some("click_css".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("#submit"));
    assert!(yaml.contains("kind: text"));
    assert!(yaml.contains("kind: attribute"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);

    // A bare CSS string saved before this feature existed still parses.
    let legacy = "name: test\nsteps:\n  - id: a\n    type: browser_click\n    selector: \".old\"\nconnections: []\n";
    let parsed_legacy = parse_flow(legacy).expect("legacy plain-string selector should still parse");
    assert_eq!(
        parsed_legacy.steps[0].action,
        Action::BrowserClick { selector: ".old".into(), instance: None }
    );
}

/// Matches the shape `nodeYamlLines` emits for a freshly-created
/// `if`/`loop` node (via `makeBranch` in flowModel.ts) before the
/// user has wired anything to it — an `if` needs nothing beyond
/// its condition, and a `loop`'s `body` may be empty.
#[test]
fn parses_freshly_created_branch_nodes_with_empty_children() {
    let yaml = r#"name: test
steps:
  - id: if_1
    type: if
    condition:
      variable: my_var
      equals: "value"
  - id: loop_1
    type: loop
    count: 3
    body: {}
"#;
    let flow = parse_flow(yaml).expect("freshly-created branch nodes must parse");
    assert_eq!(flow.steps.len(), 2);
}

#[test]
fn parses_a_loop_with_nested_steps() {
    let yaml = r#"name: test
steps:
  - id: loop_1
    type: loop
    count: 3
    body:
      steps:
        - id: wait_1
          type: wait
          seconds: 0.2
      connections: []
      entry: wait_1
"#;
    let flow = parse_flow(yaml).expect("loop with nested steps must parse");
    match &flow.steps[0].action {
        Action::Loop { count, body } => {
            assert_eq!(*count, 3);
            assert_eq!(body.steps.len(), 1);
            assert_eq!(body.entry.as_deref(), Some("wait_1"));
        }
        other => panic!("expected Loop, got {other:?}"),
    }
}

#[test]
fn parses_an_if_with_nested_then_and_otherwise_branches() {
    let yaml = r#"name: test
steps:
  - id: if_1
    type: if
    condition:
      variable: my_var
      equals: "value"
    then:
      steps:
        - id: wait_yes
          type: wait
          seconds: 0.2
      connections: []
      entry: wait_yes
    otherwise:
      steps:
        - id: wait_no
          type: wait
          seconds: 0.4
      connections: []
      entry: wait_no
"#;
    let flow = parse_flow(yaml).expect("if with nested branches must parse");
    match &flow.steps[0].action {
        Action::If { then, otherwise, .. } => {
            assert_eq!(then.entry.as_deref(), Some("wait_yes"));
            assert_eq!(otherwise.entry.as_deref(), Some("wait_no"));
        }
        other => panic!("expected If, got {other:?}"),
    }
}

#[test]
fn click_defaults_to_left_single_when_omitted() {
    let yaml = r#"name: test
steps:
  - id: click_1
    type: click
    target:
      kind: cursor
"#;
    let flow = parse_flow(yaml).expect("click without button/click_kind must parse");
    match &flow.steps[0].action {
        Action::Click {
            button, click_kind, ..
        } => {
            assert_eq!(*button, MouseButton::Left);
            assert_eq!(*click_kind, ClickKind::Single);
        }
        other => panic!("expected Click, got {other:?}"),
    }
}

#[test]
fn roundtrips_a_right_double_click() {
    let flow = Flow {
        name: "Right double click".into(),
        steps: vec![Step {
            id: "click_1".into(),
            action: Action::Click {
                target: ClickTarget::Cursor,
                button: MouseButton::Right,
                click_kind: ClickKind::Double,
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("click_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_move_mouse_step() {
    let flow = Flow {
        name: "Move mouse".into(),
        steps: vec![Step {
            id: "move_1".into(),
            action: Action::MoveMouse {
                target: PointTarget::Coordinate(MonitorPoint {
                    monitor_id: "primary".into(),
                    x: 300,
                    y: 150,
                }),
                duration_ms: 250,
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("move_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn roundtrips_a_key_press_with_modifiers() {
    let flow = Flow {
        name: "Hold shift".into(),
        steps: vec![Step {
            id: "key_1".into(),
            action: Action::KeyPress {
                key: "a".into(),
                mode: KeyPressMode::Press,
                modifiers: KeyModifiers { ctrl: true, alt: false, shift: true, win: false },
            },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("key_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

#[test]
fn enabled_defaults_to_true_when_omitted() {
    let yaml = r#"name: test
steps:
  - id: wait_1
    type: wait
    seconds: 1
"#;
    let flow = parse_flow(yaml).expect("step without enabled must parse");
    assert!(flow.steps[0].enabled);
}

#[test]
fn enabled_true_is_omitted_from_serialized_yaml() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "wait_1".into(),
            action: Action::Wait { seconds: 1.0 },
            retry: RetryPolicy::default(),
            enabled: true,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("wait_1".into()),
        step_delay_ms: 0,
    };
    let yaml = to_yaml(&flow).expect("serialize");
    assert!(!yaml.contains("enabled"), "enabled: true should stay implicit, got:\n{yaml}");
}

#[test]
fn roundtrips_a_disabled_step() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![Step {
            id: "click_1".into(),
            action: Action::Click {
                target: ClickTarget::Cursor,
                button: MouseButton::Left,
                click_kind: ClickKind::Single,
            },
            retry: RetryPolicy::default(),
            enabled: false,
            breakpoint: false,
        }],
        connections: vec![],
        entry: Some("click_1".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    assert!(yaml.contains("enabled: false"));
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}

/// A step with no incoming connection and no entry pointing at it
/// is unreachable — it's the "free-floating, wired to nothing"
/// case the graph model exists to represent. It still round-trips;
/// whether it *runs* is the engine's concern, not the schema's.
#[test]
fn roundtrips_a_disconnected_step_not_reachable_from_entry() {
    let flow = Flow {
        name: "test".into(),
        steps: vec![
            Step {
                id: "start".into(),
                action: Action::Wait { seconds: 0.1 },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
            Step {
                id: "orphan".into(),
                action: Action::Wait { seconds: 0.2 },
                retry: RetryPolicy::default(),
                enabled: true,
                breakpoint: false,
            },
        ],
        connections: vec![],
        entry: Some("start".into()),
        step_delay_ms: 0,
    };

    let yaml = to_yaml(&flow).expect("serialize");
    let parsed = parse_flow(&yaml).expect("deserialize");
    assert_eq!(flow, parsed);
}
