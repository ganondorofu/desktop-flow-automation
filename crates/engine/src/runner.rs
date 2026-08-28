//! Graph-walking flow executor. A `flow_schema::Flow` (and each `loop`
//! body nested inside it) is a pool of steps plus explicit
//! `connections` between them — execution starts at `entry` and
//! follows the wire leaving each step to find the next one. A step
//! with no incoming wire (unreachable from `entry`) simply never runs,
//! the same way an unwired Scratch block or an unconnected n8n node
//! sits inert — there is no special-cased "disabled" state needed for
//! that; `Step.enabled` is a separate, lighter-weight per-step mute.
//!
//! `if` is a nested container exactly like `loop` (see
//! `Action::If`'s `then`/`otherwise`): whichever branch the condition
//! picks runs in isolation, then execution falls back out to the
//! `if` step's own plain output — there's no separate `"yes"`/`"no"`
//! port to dispatch on here.
//!
//! The backend is injected rather than called directly so tests can
//! run the full retry/branch/loop logic without moving the real mouse
//! or typing into whatever window happens to have focus — see
//! `MockBackend` in `tests.rs`.

use crate::backend::{AutomationBackend, WindowsBackend};
use crate::context::{ExecutionContext, FlowFailure};
use crate::debug::{
    clear_stop, is_stop_requested, reset_debug_state, sleep_interruptible, Signal, RESUME,
    RESUME_CONTINUE, RESUME_NONE, RESUME_STEP, STEP_MODE,
};
use crate::observer::{ExecutionObserver, StepFailure, StepOutcome};
use flow_schema::{
    Action, BrowserSelector, BrowserSelectorSpec, CalcOp, ClickTarget, Condition, Connection,
    ElementSelector, FailureBehavior, Flow, MonitorPoint, PointTarget, Step, WindowSelector, WindowSelectorSpec,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Duration;

pub fn run_flow(
    flow: &Flow,
    observer: &mut dyn ExecutionObserver,
    step_mode: bool,
    start_step_id: Option<&str>,
) -> Result<(), FlowFailure> {
    run_flow_with_backend(flow, observer, &WindowsBackend, step_mode, start_step_id)
}

/// `start_step_id`, when given, overrides `flow.entry` as where
/// execution begins — the "run from here" context-menu command, which
/// re-runs the exact same flow but starting partway through instead
/// of from the top. Variables/`last_match`/held keys are otherwise
/// fresh, same as any other run — a step downstream that depends on
/// something an upstream step would normally have set still needs
/// that upstream step to have actually run first in this same launch
/// (nothing here replays skipped steps' side effects).
pub fn run_flow_with_backend(
    flow: &Flow,
    observer: &mut dyn ExecutionObserver,
    backend: &dyn AutomationBackend,
    step_mode: bool,
    start_step_id: Option<&str>,
) -> Result<(), FlowFailure> {
    clear_stop();
    reset_debug_state(step_mode);
    let functions = flow
        .steps
        .iter()
        .filter_map(|step| match &step.action {
            Action::FunctionDef { name, body } => Some((name.clone(), body.clone())),
            _ => None,
        })
        .collect();
    let mut ctx = ExecutionContext {
        step_delay_ms: flow.step_delay_ms,
        initial_monitor_signature: backend.monitor_signature(),
        functions,
        ..Default::default()
    };
    let entry = start_step_id.or(flow.entry.as_deref());
    let mut result = run_branch(&flow.steps, &flow.connections, entry, &mut ctx, observer, backend);
    // An uncaught failure gets one more chance: if the flow has an
    // `ErrorHandler` step, jump to whatever it's wired to instead of
    // ending the run as failed — see `Action::ErrorHandler`'s doc
    // comment. Only for a real failure (`Err`), never for a `Stop`
    // (manual or from `Action::Stop`), which is already `Ok`. If the
    // handler branch itself fails, *that* failure is what's reported.
    if let Err(failure) = &result {
        if let Some(handler_next) = error_handler_entry(&flow.steps, &flow.connections) {
            ctx.variables.insert("caught_error".into(), failure.message.clone());
            ctx.variables.insert("failed_step_id".into(), failure.step_id.clone());
            result = run_branch(&flow.steps, &flow.connections, Some(handler_next), &mut ctx, observer, backend);
        }
    }
    // Runs regardless of success, failure, or a mid-run Stop — a
    // `KeyPress` step with `mode: Press` and no matching `Release`
    // (skipped by a `Stop`, an `if` that took the other branch, the
    // flow failing before it got there, ...) must never leave the
    // real keyboard's modifier state stuck down after the run ends.
    for (key, modifiers) in ctx.held_keys.drain(..) {
        let _ = backend.key_release(&key, modifiers);
    }
    result?;
    Ok(())
}

/// The step id an `Action::ErrorHandler` marker's own plain output is
/// wired to, if the flow has one — `None` if there's no such step, or
/// it exists but has nothing connected after it (nothing meaningful
/// to jump to).
fn error_handler_entry<'a>(steps: &'a [Step], connections: &'a [Connection]) -> Option<&'a str> {
    let handler = steps.iter().find(|s| matches!(s.action, Action::ErrorHandler))?;
    next_step_id(connections, &handler.id, None)
}

/// Walks one container (the top-level flow, or an `if`/`loop`
/// sub-branch) starting at `entry`, following each step's plain
/// (unnamed-port) outgoing connection to find what runs next. Stops
/// when a step has no such outgoing connection, `entry` is `None` (an
/// empty/disconnected container — nothing to run), or a step signals
/// `Stop`.
pub(crate) fn run_branch(
    steps: &[Step],
    connections: &[Connection],
    entry: Option<&str>,
    ctx: &mut ExecutionContext,
    observer: &mut dyn ExecutionObserver,
    backend: &dyn AutomationBackend,
) -> Result<Signal, FlowFailure> {
    // A cycle in `connections` (the user wiring an output back to a
    // step it already passed through) would otherwise make this loop
    // run forever — the frontend refuses to create such a wire in the
    // first place, but this is the safety net for any flow that
    // reaches the engine some other way (hand-edited YAML, a bug
    // upstream, etc). `visited` is local to this one walk, so a
    // `Loop` body — which legitimately runs the same steps again on
    // its next iteration via a fresh `run_branch` call — is unaffected.
    let mut visited: HashSet<&str> = HashSet::new();
    let mut current = entry;
    while let Some(id) = current {
        if is_stop_requested() {
            return Ok(Signal::Stop);
        }
        if backend.monitor_signature() != ctx.initial_monitor_signature {
            observer.on_monitor_mismatch();
            loop {
                if is_stop_requested() {
                    return Ok(Signal::Stop);
                }
                if backend.monitor_signature() == ctx.initial_monitor_signature {
                    observer.on_monitor_restored();
                    break;
                }
                if sleep_interruptible(Duration::from_millis(500)) == Signal::Stop {
                    return Ok(Signal::Stop);
                }
            }
        }
        if !visited.insert(id) {
            return Err(FlowFailure {
                step_id: id.to_string(),
                message: format!("circular connection detected back at step \"{id}\" — a flow's wiring must not loop back on itself"),
            });
        }
        let Some(step) = steps.iter().find(|s| s.id == id) else {
            break;
        };
        // Step-through debugging: pause before an enabled step when
        // either the whole run is in step mode (the "Step" command)
        // or this specific step carries a breakpoint. A disabled step
        // never runs at all, so it never pauses either — consistent
        // with `step.enabled`'s "no execution, no observer events"
        // contract above.
        if step.enabled && (STEP_MODE.load(Ordering::SeqCst) || step.breakpoint) {
            observer.on_paused(step);
            loop {
                if is_stop_requested() {
                    return Ok(Signal::Stop);
                }
                match RESUME.swap(RESUME_NONE, Ordering::SeqCst) {
                    RESUME_STEP => {
                        // Re-arm step mode so the *next* step pauses
                        // too, even if this run started via a plain
                        // breakpoint rather than "Step".
                        STEP_MODE.store(true, Ordering::SeqCst);
                        break;
                    }
                    RESUME_CONTINUE => {
                        STEP_MODE.store(false, Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
                if sleep_interruptible(Duration::from_millis(50)) == Signal::Stop {
                    return Ok(Signal::Stop);
                }
            }
            observer.on_resumed();
        }
        let mut signal = Signal::Continue;
        if step.enabled {
            observer.on_step_start(step);
            let (outcome, sig) = run_step_with_retry(step, ctx, observer, backend);
            signal = sig;
            let failure = match &outcome {
                StepOutcome::Failed(f) => Some(FlowFailure {
                    step_id: f.step_id.clone(),
                    message: f.message.clone(),
                }),
                StepOutcome::Success => None,
            };
            observer.on_step_result(step, &outcome);
            observer.on_variables_changed(&ctx.variables);
            if let Some(failure) = failure {
                // `Skip` still reports the failure to the observer above
                // (so the UI shows it as failed, not silently green) —
                // it just doesn't abort the rest of the flow over it.
                if step.retry.on_failure != FailureBehavior::Skip {
                    return Err(failure);
                }
            }
        }
        // Any non-`Continue` signal (`Stop`, `BreakLoop`,
        // `ContinueLoop`, `Return`) stops walking this container's own
        // chain right here — it's the caller (`Loop`'s iteration loop,
        // `CallFunction`'s call site, or `run_flow_with_backend` at the
        // very top) that decides what the signal actually means; see
        // `Signal`'s doc comment.
        if signal != Signal::Continue {
            return Ok(signal);
        }
        if step.enabled && ctx.step_delay_ms > 0 {
            if sleep_interruptible(Duration::from_millis(ctx.step_delay_ms)) == Signal::Stop {
                return Ok(Signal::Stop);
            }
        }
        current = next_step_id(connections, id, None);
    }
    Ok(Signal::Continue)
}

fn next_step_id<'a>(
    connections: &'a [Connection],
    from: &str,
    from_port: Option<&str>,
) -> Option<&'a str> {
    connections
        .iter()
        .find(|c| c.from == from && c.from_port.as_deref() == from_port)
        .map(|c| c.to.as_str())
}

fn run_step_with_retry(
    step: &Step,
    ctx: &mut ExecutionContext,
    observer: &mut dyn ExecutionObserver,
    backend: &dyn AutomationBackend,
) -> (StepOutcome, Signal) {
    let attempts = step.retry.max_attempts.max(1);
    let mut last_error = String::from("unknown error");
    for attempt in 0..attempts {
        if is_stop_requested() {
            return (StepOutcome::Success, Signal::Stop);
        }
        match run_action(&step.action, ctx, observer, backend) {
            Ok(signal) => return (StepOutcome::Success, signal),
            Err(e) => {
                last_error = e;
                if attempt + 1 < attempts && step.retry.interval_ms > 0 {
                    if sleep_interruptible(Duration::from_millis(step.retry.interval_ms)) == Signal::Stop {
                        return (StepOutcome::Success, Signal::Stop);
                    }
                }
            }
        }
    }
    (
        StepOutcome::Failed(StepFailure {
            step_id: step.id.clone(),
            message: last_error,
        }),
        Signal::Continue,
    )
}

/// Substitutes every `%name%` in `text` with `variables["name"]`'s
/// current value — Relay's answer to iOS Shortcuts' magic variables,
/// Windows batch's `%VAR%`, and PAD's `%Variable%`. A name that isn't
/// set (a typo, or a step that simply hasn't run yet) is left as the
/// literal `%name%` text rather than silently disappearing into an
/// empty string, so the mistake is visible in the step's actual
/// output. A `%` with no matching close (or an empty `%%`) is left
/// untouched — only a non-empty run of non-`%`/non-whitespace
/// characters between two `%`s counts as a token, so a lone `%` in
/// ordinary text (a percentage, a URL-encoded byte, ...) is never
/// mistaken for one.
fn resolve(text: &str, variables: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(start) = rest.find('%') else {
            out.push_str(rest);
            return out;
        };
        let after_open = &rest[start + 1..];
        let candidate_end = after_open.find(|c: char| c == '%' || c.is_whitespace());
        let Some(rel_end) = candidate_end.filter(|&i| after_open.as_bytes()[i] == b'%' && i > 0) else {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        };
        let end = start + 1 + rel_end;
        out.push_str(&rest[..start]);
        let name = &rest[start + 1..end];
        match variables.get(name) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..end + 1]),
        }
        rest = &rest[end + 1..];
    }
}

/// `resolve()` for whichever string(s) a `BrowserSelector` actually
/// holds — a CSS selector, or a text/attribute match's `value` (and
/// `name`, for the attribute kind).
fn resolve_selector(selector: &BrowserSelector, variables: &HashMap<String, String>) -> BrowserSelector {
    match selector {
        BrowserSelector::Css(css) => BrowserSelector::Css(resolve(css, variables)),
        BrowserSelector::Other(BrowserSelectorSpec::Text { value }) => {
            BrowserSelector::Other(BrowserSelectorSpec::Text { value: resolve(value, variables) })
        }
        BrowserSelector::Other(BrowserSelectorSpec::Attribute { name, value }) => {
            BrowserSelector::Other(BrowserSelectorSpec::Attribute {
                name: resolve(name, variables),
                value: resolve(value, variables),
            })
        }
    }
}

/// `resolve()` for whichever string(s) a `WindowSelector` actually
/// holds — a title, a title-contains fragment, or a process name (and
/// both, for the title-then-process fallback mode).
fn resolve_window_selector(selector: &WindowSelector, variables: &HashMap<String, String>) -> WindowSelector {
    match selector {
        WindowSelector::Title(title) => WindowSelector::Title(resolve(title, variables)),
        WindowSelector::Other(WindowSelectorSpec::TitleContains { text }) => {
            WindowSelector::Other(WindowSelectorSpec::TitleContains { text: resolve(text, variables) })
        }
        WindowSelector::Other(WindowSelectorSpec::Process { process_name }) => {
            WindowSelector::Other(WindowSelectorSpec::Process { process_name: resolve(process_name, variables) })
        }
        WindowSelector::Other(WindowSelectorSpec::TitleThenProcess { title, process_name }) => {
            WindowSelector::Other(WindowSelectorSpec::TitleThenProcess { title: resolve(title, variables), process_name: resolve(process_name, variables) })
        }
    }
}

fn run_action(
    action: &Action,
    ctx: &mut ExecutionContext,
    observer: &mut dyn ExecutionObserver,
    backend: &dyn AutomationBackend,
) -> Result<Signal, String> {
    match action {
        // Pure marker — see the doc comment on `Action::Start`.
        Action::Start => Ok(Signal::Continue),
        // Pure marker reached only via normal wiring (someone
        // connected something ahead of it, or ran the flow starting
        // from it directly) — its real behavior, jumping here after
        // an uncaught failure, is handled entirely in
        // `run_flow_with_backend`, not here.
        Action::ErrorHandler => Ok(Signal::Continue),
        // Ends the run right here — see the doc comment on `Action::Stop`.
        Action::Stop => Ok(Signal::Stop),
        Action::Break => Ok(Signal::BreakLoop),
        Action::Continue => Ok(Signal::ContinueLoop),
        Action::Return => Ok(Signal::Return),
        Action::Click {
            target,
            button,
            click_kind,
        } => match target {
            ClickTarget::Cursor => backend.click_at_cursor(*button, *click_kind),
            ClickTarget::Element(selector) => backend.click_element(selector),
        }
        .map(|()| Signal::Continue),
        Action::MoveMouse {
            target,
            duration_ms,
        } => {
            let point = match target {
                PointTarget::Coordinate(point) => point.clone(),
                PointTarget::LastMatch => last_match_point(ctx)?,
            };
            backend.move_mouse(&point, *duration_ms)
        }
        .map(|()| Signal::Continue),
        Action::TypeText { text } => backend.type_text(&resolve(text, &ctx.variables)).map(|()| Signal::Continue),
        Action::KeyPress { key, mode, modifiers } => match mode {
            flow_schema::KeyPressMode::Tap => backend.key_tap(key, *modifiers).map(|()| Signal::Continue),
            flow_schema::KeyPressMode::Press => {
                backend.key_hold_down(key, *modifiers)?;
                ctx.held_keys.push((key.clone(), *modifiers));
                Ok(Signal::Continue)
            }
            flow_schema::KeyPressMode::Release => {
                // Releases with the modifiers a matching `Press` for
                // this key actually pressed, not whatever (if
                // anything) this step's own `modifiers` says — see
                // `KeyModifiers`'s doc comment. A `Release` with no
                // matching `Press` (a stray/duplicate step, or a key
                // this run never held) just releases the bare key,
                // which is harmless if it isn't actually down.
                let held = ctx.held_keys.iter().rposition(|(held_key, _)| held_key == key).map(|i| ctx.held_keys.remove(i).1);
                backend.key_release(key, held.unwrap_or_default()).map(|()| Signal::Continue)
            }
        },
        Action::Wait { seconds } => Ok(sleep_interruptible(Duration::from_secs_f64(*seconds))),
        Action::SetVariable { name, value } => {
            let resolved = resolve(value, &ctx.variables);
            ctx.variables.insert(name.clone(), resolved);
            Ok(Signal::Continue)
        }
        Action::Calculate { a, op, b, variable } => {
            let a_text = resolve(a, &ctx.variables);
            let b_text = resolve(b, &ctx.variables);
            let a_num: f64 = a_text
                .trim()
                .parse()
                .map_err(|_| format!("\"{a_text}\" is not a number"))?;
            let b_num: f64 = b_text
                .trim()
                .parse()
                .map_err(|_| format!("\"{b_text}\" is not a number"))?;
            let result = match op {
                CalcOp::Add => a_num + b_num,
                CalcOp::Subtract => a_num - b_num,
                CalcOp::Multiply => a_num * b_num,
                CalcOp::Divide => {
                    if b_num == 0.0 {
                        return Err(format!("cannot divide {a_num} by zero"));
                    }
                    a_num / b_num
                }
                CalcOp::Round | CalcOp::Floor | CalcOp::Ceil => {
                    if b_num < 0.0 {
                        return Err(format!("decimal places {b_num} must not be negative"));
                    }
                    let factor = 10f64.powi(b_num.round() as i32);
                    let scaled = a_num * factor;
                    let rounded = match op {
                        CalcOp::Round => scaled.round(),
                        CalcOp::Floor => scaled.floor(),
                        CalcOp::Ceil => scaled.ceil(),
                        _ => unreachable!(),
                    };
                    rounded / factor
                }
            };
            ctx.variables.insert(variable.clone(), result.to_string());
            Ok(Signal::Continue)
        }
        // A nested container exactly like `Loop` below, just picking
        // one of two branches instead of repeating one — see the
        // module doc comment.
        Action::If {
            condition,
            then,
            otherwise,
        } => {
            let branch = if evaluate(condition, ctx) { then } else { otherwise };
            run_branch(
                &branch.steps,
                &branch.connections,
                branch.entry.as_deref(),
                ctx,
                observer,
                backend,
            )
            .map_err(|f| format!("{}: {}", f.step_id, f.message))
        }
        Action::Loop { count, body } => {
            for _ in 0..*count {
                let signal = run_branch(
                    &body.steps,
                    &body.connections,
                    body.entry.as_deref(),
                    ctx,
                    observer,
                    backend,
                )
                .map_err(|f| format!("{}: {}", f.step_id, f.message))?;
                match signal {
                    // `Stop` always unwinds everything, loop included.
                    Signal::Stop => return Ok(Signal::Stop),
                    // Absorbed here — this `Loop` is exactly what
                    // `Action::Break` means "exit".
                    Signal::BreakLoop => break,
                    // Absorbed here too — the Rust `for` loop is
                    // already about to move to its next iteration on
                    // its own, so there's nothing more to do.
                    Signal::ContinueLoop => {}
                    // Not this `Loop`'s to absorb — a `return` inside
                    // a loop still returns from the enclosing
                    // function (or ends the run, at the top level),
                    // not just breaks the loop.
                    Signal::Return => return Ok(Signal::Return),
                    Signal::Continue => {}
                }
            }
            Ok(Signal::Continue)
        }
        Action::TryCatch { try_branch, catch } => {
            let try_result = run_branch(
                &try_branch.steps,
                &try_branch.connections,
                try_branch.entry.as_deref(),
                ctx,
                observer,
                backend,
            );
            match try_result {
                Ok(signal) => Ok(signal),
                Err(failure) => {
                    // Every other `caught_error`-adjacent variable
                    // (`last_match_found`, etc) is set unconditionally
                    // on both outcomes of its own action; this one
                    // only exists once something has actually failed,
                    // so `catch` can tell "ran because of a real
                    // failure" apart from "ran because a stale value
                    // from three tries ago is still sitting there" —
                    // there is no meaningful value to set on success.
                    ctx.variables.insert("caught_error".into(), failure.message);
                    run_branch(
                        &catch.steps,
                        &catch.connections,
                        catch.entry.as_deref(),
                        ctx,
                        observer,
                        backend,
                    )
                    .map_err(|f| format!("{}: {}", f.step_id, f.message))
                }
            }
        }
        // `body` is only ever run via `CallFunction`'s explicit lookup
        // (see `run_flow_with_backend`, which collects every
        // `FunctionDef` into `ctx.functions` before the run starts) —
        // reaching this step directly through normal wiring, which
        // nothing does by convention, is a deliberate no-op rather
        // than running `body` inline; that would make "was this
        // function actually called, or just wired past" ambiguous.
        Action::FunctionDef { .. } => Ok(Signal::Continue),
        Action::CallFunction { name } => {
            if ctx.call_stack.iter().any(|f| f == name) {
                return Err(format!(
                    "function '{name}' called itself (directly or indirectly) — recursive calls aren't supported"
                ));
            }
            let Some(body) = ctx.functions.get(name).cloned() else {
                return Err(format!("no function named '{name}' is defined in this flow"));
            };
            ctx.call_stack.push(name.clone());
            let result = run_branch(&body.steps, &body.connections, body.entry.as_deref(), ctx, observer, backend)
                .map_err(|f| format!("{}: {}", f.step_id, f.message));
            ctx.call_stack.pop();
            // `Stop` always unwinds everything, function call included.
            // Everything else (`Return`, and a stray `BreakLoop`/
            // `ContinueLoop` that reached this function's own top
            // level with no enclosing `Loop` inside it to catch it —
            // lexically meaningless past this boundary) just ends this
            // call; the step after `CallFunction` runs normally.
            result.map(|signal| if signal == Signal::Stop { Signal::Stop } else { Signal::Continue })
        }
        Action::FindImage {
            image,
            mode,
            threshold,
            min_scale,
            max_scale,
            scale_steps,
        } => {
            // `last_match_found` is set on *both* outcomes (unlike
            // `last_match_x`/`y`/`score`, which stay meaningless when
            // nothing matched) — so `If` can branch on "does this
            // image exist" using the condition mechanism that already
            // exists, instead of a dedicated "image if" node: pair
            // this step with `on_failure: skip` (so a non-match
            // doesn't abort the flow) and an `If` checking
            // `last_match_found == true` right after it. The
            // not-found case still returns `Err` — retry/on_failure
            // still decide what happens next, so "keep retrying until
            // this image shows up" (a plain `find_image` with a
            // generous retry policy) is unaffected.
            match backend.find_image(image, *mode, *threshold, *min_scale, *max_scale, *scale_steps) {
                Ok(found) => {
                    ctx.variables.insert("last_match_found".into(), "true".into());
                    ctx.variables.insert("last_match_x".into(), found.point.x.to_string());
                    ctx.variables.insert("last_match_y".into(), found.point.y.to_string());
                    ctx.variables.insert("last_match_score".into(), found.score.to_string());
                    ctx.last_match = Some(found.point);
                    Ok(Signal::Continue)
                }
                Err(e) => {
                    ctx.variables.insert("last_match_found".into(), "false".into());
                    ctx.last_match = None;
                    Err(e)
                }
            }
        }
        Action::FindTextOcr { text, region } => backend
            .find_text_ocr(&resolve(text, &ctx.variables), region.as_ref())
            .map(|()| Signal::Continue),
        Action::WaitForWindow { window, timeout_ms } => backend
            .wait_for_window(&resolve_window_selector(window, &ctx.variables), *timeout_ms)
            .map(|()| Signal::Continue),
        Action::FocusWindow { window } => {
            backend.focus_window(&resolve_window_selector(window, &ctx.variables)).map(|()| Signal::Continue)
        }
        Action::PowerAction { mode, force } => match mode {
            flow_schema::PowerMode::Shutdown => backend.shutdown(*force).map(|()| Signal::Continue),
            flow_schema::PowerMode::Restart => backend.restart(*force).map(|()| Signal::Continue),
        },
        Action::LockWorkstation => backend.lock_workstation().map(|()| Signal::Continue),
        Action::ReadClipboard { variable } => {
            let text = backend.read_clipboard()?;
            ctx.variables.insert(variable.clone(), text);
            Ok(Signal::Continue)
        }
        Action::WriteClipboard { text } => {
            backend.write_clipboard(&resolve(text, &ctx.variables)).map(|()| Signal::Continue)
        }
        Action::ShowMessage { title, message, blocking } => {
            let title = resolve(title, &ctx.variables);
            let message = resolve(message, &ctx.variables);
            if *blocking {
                backend.show_message(&title, &message).map(|()| Signal::Continue)
            } else {
                backend.show_message_async(&title, &message).map(|()| Signal::Continue)
            }
        }
        Action::ShowConfirm { title, message, variable } => {
            let yes = backend.show_confirm(&resolve(title, &ctx.variables), &resolve(message, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), if yes { "yes".to_string() } else { "no".to_string() });
            Ok(Signal::Continue)
        }
        Action::ShowInput { title, message, default_value, variable } => {
            let text = backend.show_input(
                &resolve(title, &ctx.variables),
                &resolve(message, &ctx.variables),
                &resolve(default_value, &ctx.variables),
            )?;
            ctx.variables.insert(variable.clone(), text);
            Ok(Signal::Continue)
        }
        Action::GetDateTime { format, variable } => {
            let now = backend.get_date_time(*format);
            ctx.variables.insert(variable.clone(), now);
            Ok(Signal::Continue)
        }
        Action::GetSystemInfo { hostname, os_version, cpu_percent, memory_percent, ip_address } => {
            // CPU usage is the one genuinely slow field to gather (a
            // ~200ms sample window — see `automation::cpu_percent_sampled`),
            // so it's the only one worth skipping entirely when
            // nothing downstream actually wants it.
            let info = backend.get_system_info(cpu_percent.is_some());
            if let Some(name) = hostname {
                ctx.variables.insert(name.clone(), info.hostname);
            }
            if let Some(name) = os_version {
                ctx.variables.insert(name.clone(), info.os_version);
            }
            if let Some(name) = cpu_percent {
                ctx.variables.insert(name.clone(), info.cpu_percent.to_string());
            }
            if let Some(name) = memory_percent {
                ctx.variables.insert(name.clone(), info.memory_percent.to_string());
            }
            if let Some(name) = ip_address {
                ctx.variables.insert(name.clone(), info.ip_address);
            }
            Ok(Signal::Continue)
        }
        Action::TextTransform { op, text, arg1, arg2, variable } => {
            let result = backend.text_transform(
                *op,
                &resolve(text, &ctx.variables),
                &resolve(arg1, &ctx.variables),
                &resolve(arg2, &ctx.variables),
            )?;
            ctx.variables.insert(variable.clone(), result);
            Ok(Signal::Continue)
        }
        Action::LaunchApp { path, args } => backend
            .launch_app(&resolve(path, &ctx.variables), &resolve(args, &ctx.variables))
            .map(|()| Signal::Continue),
        Action::OpenUrl { url } => backend.open_url(&resolve(url, &ctx.variables)).map(|()| Signal::Continue),
        Action::Notify { title, message } => backend
            .show_notification(&resolve(title, &ctx.variables), &resolve(message, &ctx.variables))
            .map(|()| Signal::Continue),
        Action::ReadFile { path, variable } => {
            let content = backend.read_file(&resolve(path, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), content);
            Ok(Signal::Continue)
        }
        Action::WriteFile { path, content, append } => backend
            .write_file(&resolve(path, &ctx.variables), &resolve(content, &ctx.variables), *append)
            .map(|()| Signal::Continue),
        Action::CopyFile { source, destination } => backend
            .copy_file(&resolve(source, &ctx.variables), &resolve(destination, &ctx.variables))
            .map(|()| Signal::Continue),
        Action::MoveFile { source, destination } => backend
            .move_file(&resolve(source, &ctx.variables), &resolve(destination, &ctx.variables))
            .map(|()| Signal::Continue),
        Action::DeleteFile { path } => backend.delete_file(&resolve(path, &ctx.variables)).map(|()| Signal::Continue),
        Action::CreateDirectory { path } => {
            backend.create_directory(&resolve(path, &ctx.variables)).map(|()| Signal::Continue)
        }
        Action::ListDirectory { path, variable } => {
            let listing = backend.list_directory(&resolve(path, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), listing);
            Ok(Signal::Continue)
        }
        Action::Http {
            method,
            url,
            headers,
            body,
            variable,
            status_variable,
        } => {
            let (response_body, status) = backend.http_request(
                *method,
                &resolve(url, &ctx.variables),
                &resolve(headers, &ctx.variables),
                &resolve(body, &ctx.variables),
            )?;
            ctx.variables.insert(variable.clone(), response_body);
            ctx.variables.insert(status_variable.clone(), status.to_string());
            Ok(Signal::Continue)
        }
        Action::HttpDownload { url, headers, path, variable, path_variable } => {
            let resolved_path = resolve(path, &ctx.variables);
            let status = backend.http_download(&resolve(url, &ctx.variables), &resolve(headers, &ctx.variables), &resolved_path)?;
            ctx.variables.insert(variable.clone(), status.to_string());
            ctx.variables.insert(path_variable.clone(), resolved_path);
            Ok(Signal::Continue)
        }
        Action::Ping { host, timeout_ms, variable } => {
            let result = backend.ping(&resolve(host, &ctx.variables), *timeout_ms)?;
            ctx.variables.insert(variable.clone(), if result.reachable { "true".to_string() } else { "false".to_string() });
            if let Some(latency_ms) = result.latency_ms {
                ctx.variables.insert(format!("{variable}_latency_ms"), latency_ms.to_string());
            }
            Ok(Signal::Continue)
        }
        Action::DnsLookup { hostname, variable } => {
            let ip = backend.dns_lookup(&resolve(hostname, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), ip);
            Ok(Signal::Continue)
        }
        Action::Screenshot { region, path } => backend
            .take_screenshot(region.as_ref(), &resolve(path, &ctx.variables))
            .map(|()| Signal::Continue),
        Action::BrowserScreenshot { path, instance } => backend
            .browser_screenshot(&resolve(path, &ctx.variables), resolve_instance(instance, &ctx.variables).as_deref())
            .map(|()| Signal::Continue),
        Action::GetEnvVar { name, variable } => {
            let value = backend.get_env_var(&resolve(name, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), value);
            Ok(Signal::Continue)
        }
        Action::CheckProcess { name, variable } => {
            let running = backend.check_process(&resolve(name, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), if running { "true".to_string() } else { "false".to_string() });
            Ok(Signal::Continue)
        }
        Action::KillProcess { name, force } => {
            backend.kill_process(&resolve(name, &ctx.variables), *force)?;
            Ok(Signal::Continue)
        }
        Action::WaitForFile { path, timeout_ms } => {
            backend.wait_for_file(&resolve(path, &ctx.variables), *timeout_ms)?;
            Ok(Signal::Continue)
        }
        Action::GenerateRandom { min, max, variable } => {
            let value = backend.generate_random(&resolve(min, &ctx.variables), &resolve(max, &ctx.variables))?;
            ctx.variables.insert(variable.clone(), value.to_string());
            Ok(Signal::Continue)
        }
        Action::GetElementText { selector, variable } => {
            let resolved = ElementSelector {
                window_title: selector.window_title.as_deref().map(|t| resolve(t, &ctx.variables)),
                automation_id: selector.automation_id.clone(),
                name: selector.name.as_deref().map(|n| resolve(n, &ctx.variables)),
                control_type: selector.control_type.clone(),
            };
            let text = backend.get_element_text(&resolved)?;
            ctx.variables.insert(variable.clone(), text);
            Ok(Signal::Continue)
        }
        Action::LaunchBrowser { url, variable, browser, profile_dir } => {
            let resolved_profile = profile_dir.as_deref().map(|p| resolve(p, &ctx.variables));
            let instance_id =
                backend.launch_browser(&resolve(url, &ctx.variables), browser.as_deref(), resolved_profile.as_deref())?;
            ctx.variables.insert(variable.clone(), instance_id);
            Ok(Signal::Continue)
        }
        Action::BrowserNavigate { url, instance } => backend
            .browser_navigate(&resolve(url, &ctx.variables), resolve_instance(instance, &ctx.variables).as_deref())
            .map(|()| Signal::Continue),
        Action::BrowserClick { selector, instance } => backend
            .browser_click(&resolve_selector(selector, &ctx.variables), resolve_instance(instance, &ctx.variables).as_deref())
            .map(|()| Signal::Continue),
        Action::BrowserGetText { selector, variable, instance } => {
            let text = backend.browser_get_text(
                &resolve_selector(selector, &ctx.variables),
                resolve_instance(instance, &ctx.variables).as_deref(),
            )?;
            ctx.variables.insert(variable.clone(), text);
            Ok(Signal::Continue)
        }
        Action::BrowserSetValue { selector, value, instance } => backend
            .browser_set_value(
                &resolve_selector(selector, &ctx.variables),
                &resolve(value, &ctx.variables),
                resolve_instance(instance, &ctx.variables).as_deref(),
            )
            .map(|()| Signal::Continue),
        Action::BrowserWaitForSelector { selector, instance } => backend
            .browser_wait_for_selector(
                &resolve_selector(selector, &ctx.variables),
                resolve_instance(instance, &ctx.variables).as_deref(),
            )
            .map(|()| Signal::Continue),
    }
}

/// `instance` is `None` for a flow with no browser instances at all
/// (the common case, pre-dating instance support) — `resolve`
/// requires an actual `&str` to scan for `%...%`, so this only
/// calls it when there's something to resolve.
fn resolve_instance(instance: &Option<String>, variables: &HashMap<String, String>) -> Option<String> {
    instance.as_deref().map(|s| resolve(s, variables))
}

/// Resolves `ClickTarget::LastMatch`/`PointTarget::LastMatch` — fails
/// the step with a clear message rather than silently clicking at
/// (0, 0) when no `find_image` has actually succeeded yet in this run.
fn last_match_point(ctx: &ExecutionContext) -> Result<MonitorPoint, String> {
    ctx.last_match
        .clone()
        .ok_or_else(|| "no find_image match yet — this step needs a successful find_image earlier in the run".to_string())
}

fn evaluate(condition: &Condition, ctx: &ExecutionContext) -> bool {
    ctx.variables
        .get(&condition.variable)
        .map(|value| value == &resolve(&condition.equals, &ctx.variables))
        .unwrap_or(false)
}
