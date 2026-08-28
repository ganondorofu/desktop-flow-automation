/** The inverse of `flowYamlSerialize.ts` — turns a saved flow file's
 *  text back into the editable `Branch` graph plus the flow's name. */
import { parse as parseYamlDocument } from "yaml";
import { allIds, bumpCounterPast, updateNode } from "./flowGraph";
import type {
  Branch,
  BrowserSelectorField,
  CalcOp,
  ClickKind,
  DateTimeFormat,
  FlowNode,
  ImageSourceField,
  KeyPressMode,
  MouseButtonKind,
  TextOp,
} from "./flowModel";

function parseBranchYaml(raw: unknown): Branch {
  const obj = (raw ?? {}) as Record<string, unknown>;
  const steps = Array.isArray(obj.steps) ? obj.steps.map(parseStepYaml) : [];
  const connections = Array.isArray(obj.connections)
    ? obj.connections.map((c) => {
        const conn = c as Record<string, unknown>;
        return {
          from: String(conn.from),
          fromPort: conn.from_port != null ? String(conn.from_port) : null,
          to: String(conn.to),
        };
      })
    : [];
  const entry = typeof obj.entry === "string" ? obj.entry : null;
  return { steps, connections, entry };
}

/** Inverse of `browserSelectorYamlLines` — accepts a bare string (the
 *  plain-CSS shorthand, and everything the on-page picker or an
 *  older saved file ever produced) or a `{ kind, value, name? }`
 *  mapping for the text/attribute strategies. */
function parseBrowserSelector(raw: unknown): BrowserSelectorField {
  if (typeof raw === "string") return { kind: "css", value: raw, name: "" };
  const s = (raw ?? {}) as Record<string, unknown>;
  if (s.kind === "attribute") {
    return { kind: "attribute", value: String(s.value ?? ""), name: String(s.name ?? "") };
  }
  if (s.kind === "text") {
    return { kind: "text", value: String(s.value ?? ""), name: "" };
  }
  return { kind: "css", value: "", name: "" };
}

/** Inverse of `find_image`'s image-lines — a bare string (the path
 *  shorthand, and everything every flow saved before embedding
 *  existed ever produced) or a `{ data }` mapping for the embedded
 *  base64 case. Mirrors `flow_schema::ImageSource`'s untagged shape. */
function parseImageSource(raw: unknown): ImageSourceField {
  if (typeof raw === "string") return { kind: "path", value: raw };
  const s = (raw ?? {}) as Record<string, unknown>;
  if (typeof s.data === "string") return { kind: "embedded", data: s.data };
  return { kind: "path", value: "" };
}

function parseStepYaml(raw: unknown): FlowNode {
  const node = parseStepAction(raw);
  const s = raw as Record<string, unknown>;
  const retry = (s.retry ?? {}) as Record<string, unknown>;
  return {
    ...node,
    ...(retry.on_failure === "skip" ? { onFailure: "skip" as const } : {}),
    ...(typeof retry.max_attempts === "number" ? { retryMaxAttempts: retry.max_attempts } : {}),
    ...(typeof retry.interval_ms === "number" ? { retryIntervalMs: retry.interval_ms } : {}),
    ...(s.breakpoint === true ? { breakpoint: true } : {}),
  };
}

function parseStepAction(raw: unknown): FlowNode {
  const s = raw as Record<string, unknown>;
  const id = String(s.id);
  const enabled = s.enabled !== false;
  const type = String(s.type);
  switch (type) {
    case "start":
      return { id, kind: "start", enabled };
    case "error_handler":
      return { id, kind: "error_handler", enabled };
    case "wait":
      return { id, kind: "wait", seconds: Number(s.seconds), enabled };
    case "set_variable":
      return { id, kind: "set_variable", name: String(s.name), value: String(s.value), enabled };
    case "calculate":
      return {
        id,
        kind: "calculate",
        a: String(s.a),
        op: (s.op as CalcOp) ?? "add",
        b: String(s.b),
        variable: String(s.variable),
        enabled,
      };
    case "type_text":
      return { id, kind: "type_text", text: String(s.text), enabled };
    case "click": {
      const target = s.target as Record<string, unknown> | undefined;
      if (target && target.kind !== "cursor") {
        throw new Error(
          target.kind === "element"
            ? `click step "${id}" uses an element target, which this editor can't display yet`
            : `click step "${id}" targets a fixed coordinate — this editor no longer supports that on a click step directly. Use a "move mouse" step to that position, followed by a plain click.`,
        );
      }
      return {
        id,
        kind: "click",
        button: (s.button as MouseButtonKind) ?? "left",
        clickKind: (s.click_kind as ClickKind) ?? "single",
        enabled,
      };
    }
    case "move_mouse": {
      const target = (s.target ?? {}) as Record<string, unknown>;
      if (target.kind === "last_match") {
        return { id, kind: "move_mouse", x: 0, y: 0, targetKind: "last_match", durationMs: Number(s.duration_ms ?? 0), enabled };
      }
      return { id, kind: "move_mouse", x: Number(target.x), y: Number(target.y), durationMs: Number(s.duration_ms ?? 0), enabled };
    }
    case "key_press": {
      const modifiers = (s.modifiers ?? {}) as Record<string, unknown>;
      return {
        id,
        kind: "key_press",
        key: String(s.key),
        mode: (s.mode as KeyPressMode) ?? "tap",
        modifiers: {
          ctrl: Boolean(modifiers.ctrl),
          alt: Boolean(modifiers.alt),
          shift: Boolean(modifiers.shift),
          win: Boolean(modifiers.win),
        },
        enabled,
      };
    }
    case "find_image":
      return {
        id,
        kind: "find_image",
        image: parseImageSource(s.image),
        mode: (s.mode as "exact" | "similar") ?? "exact",
        threshold: Number(s.threshold ?? 0.85),
        minScale: Number(s.min_scale ?? 0.7),
        maxScale: Number(s.max_scale ?? 1.4),
        scaleSteps: Number(s.scale_steps ?? 12),
        enabled,
      };
    case "find_text_ocr": {
      const region = s.region as Record<string, unknown> | undefined;
      return {
        id,
        kind: "find_text_ocr",
        text: String(s.text),
        ...(region
          ? { region: { x: Number(region.x), y: Number(region.y), width: Number(region.width), height: Number(region.height) } }
          : {}),
        enabled,
      };
    }
    case "wait_for_window":
      return { id, kind: "wait_for_window", windowTitle: String(s.window_title), enabled };
    case "focus_window":
      return { id, kind: "focus_window", windowTitle: String(s.window_title), enabled };
    case "power_action":
      return { id, kind: "power_action", mode: s.mode === "restart" ? "restart" : "shutdown", force: Boolean(s.force ?? false), enabled };
    case "lock_workstation":
      return { id, kind: "lock_workstation", enabled };
    case "read_clipboard":
      return { id, kind: "read_clipboard", variable: String(s.variable), enabled };
    case "write_clipboard":
      return { id, kind: "write_clipboard", text: String(s.text ?? ""), enabled };
    case "show_message":
      return {
        id,
        kind: "show_message",
        title: String(s.title ?? ""),
        message: String(s.message ?? ""),
        blocking: s.blocking === undefined ? true : Boolean(s.blocking),
        enabled,
      };
    case "show_confirm":
      return {
        id,
        kind: "show_confirm",
        title: String(s.title ?? ""),
        message: String(s.message ?? ""),
        variable: String(s.variable),
        enabled,
      };
    case "show_input":
      return {
        id,
        kind: "show_input",
        title: String(s.title ?? ""),
        message: String(s.message ?? ""),
        defaultValue: String(s.default_value ?? ""),
        variable: String(s.variable),
        enabled,
      };
    case "stop":
      return { id, kind: "stop", enabled };
    case "break":
      return { id, kind: "break", enabled };
    case "continue":
      return { id, kind: "continue", enabled };
    case "return":
      return { id, kind: "return", enabled };
    case "get_date_time":
      return { id, kind: "get_date_time", format: (s.format as DateTimeFormat) ?? "iso8601", variable: String(s.variable), enabled };
    case "get_system_info":
      return {
        id,
        kind: "get_system_info",
        hostname: s.hostname != null ? String(s.hostname) : "",
        osVersion: s.os_version != null ? String(s.os_version) : "",
        cpuPercent: s.cpu_percent != null ? String(s.cpu_percent) : "",
        memoryPercent: s.memory_percent != null ? String(s.memory_percent) : "",
        ipAddress: s.ip_address != null ? String(s.ip_address) : "",
        enabled,
      };
    case "text_transform":
      return {
        id,
        kind: "text_transform",
        op: s.op as TextOp,
        text: String(s.text ?? ""),
        arg1: String(s.arg1 ?? ""),
        arg2: String(s.arg2 ?? ""),
        variable: String(s.variable),
        enabled,
      };
    case "launch_app":
      return { id, kind: "launch_app", path: String(s.path), args: String(s.args ?? ""), enabled };
    case "open_url":
      return { id, kind: "open_url", url: String(s.url), enabled };
    case "notify":
      return { id, kind: "notify", title: String(s.title), message: String(s.message ?? ""), enabled };
    case "read_file":
      return { id, kind: "read_file", path: String(s.path), variable: String(s.variable), enabled };
    case "write_file":
      return { id, kind: "write_file", path: String(s.path), content: String(s.content ?? ""), append: Boolean(s.append ?? false), enabled };
    case "copy_file":
      return { id, kind: "copy_file", source: String(s.source), destination: String(s.destination), enabled };
    case "move_file":
      return { id, kind: "move_file", source: String(s.source), destination: String(s.destination), enabled };
    case "delete_file":
      return { id, kind: "delete_file", path: String(s.path), enabled };
    case "create_directory":
      return { id, kind: "create_directory", path: String(s.path), enabled };
    case "list_directory":
      return { id, kind: "list_directory", path: String(s.path), variable: String(s.variable), enabled };
    case "http":
      return {
        id,
        kind: "http",
        method: (s.method as "get" | "post" | "put" | "patch" | "delete") ?? "get",
        url: String(s.url),
        headers: String(s.headers ?? ""),
        body: String(s.body ?? ""),
        variable: String(s.variable),
        statusVariable: s.status_variable != null ? String(s.status_variable) : `${String(s.variable)}_status`,
        enabled,
      };
    case "http_download":
      return {
        id,
        kind: "http_download",
        url: String(s.url),
        headers: String(s.headers ?? ""),
        path: String(s.path),
        variable: String(s.variable),
        pathVariable: s.path_variable != null ? String(s.path_variable) : `${String(s.variable)}_path`,
        enabled,
      };
    case "ping":
      return {
        id,
        kind: "ping",
        host: String(s.host),
        timeoutMs: Number(s.timeout_ms ?? 2000),
        variable: String(s.variable),
        enabled,
      };
    case "dns_lookup":
      return { id, kind: "dns_lookup", hostname: String(s.hostname), variable: String(s.variable), enabled };
    case "screenshot": {
      const region = s.region as Record<string, unknown> | undefined;
      return {
        id,
        kind: "screenshot",
        region: region ? { x: Number(region.x), y: Number(region.y), width: Number(region.width), height: Number(region.height) } : undefined,
        path: String(s.path),
        enabled,
      };
    }
    case "browser_screenshot":
      return { id, kind: "browser_screenshot", path: String(s.path), instance: s.instance != null ? String(s.instance) : "", enabled };
    case "get_env_var":
      return { id, kind: "get_env_var", name: String(s.name), variable: String(s.variable), enabled };
    case "check_process":
      return { id, kind: "check_process", name: String(s.name), variable: String(s.variable), enabled };
    case "kill_process":
      return { id, kind: "kill_process", name: String(s.name), force: Boolean(s.force ?? false), enabled };
    case "wait_for_file":
      return { id, kind: "wait_for_file", path: String(s.path), timeoutMs: Number(s.timeout_ms ?? 30000), enabled };
    case "generate_random":
      return { id, kind: "generate_random", min: String(s.min), max: String(s.max), variable: String(s.variable), enabled };
    case "get_element_text": {
      const selector = (s.selector ?? {}) as Record<string, unknown>;
      return {
        id,
        kind: "get_element_text",
        windowTitle: selector.window_title != null ? String(selector.window_title) : "",
        elementName: selector.name != null ? String(selector.name) : "",
        automationId: selector.automation_id != null ? String(selector.automation_id) : "",
        variable: String(s.variable),
        enabled,
      };
    }
    case "launch_browser":
      return {
        id,
        kind: "launch_browser",
        url: String(s.url ?? ""),
        variable: String(s.variable),
        browser: s.browser != null ? String(s.browser) : "",
        profileDir: s.profile_dir != null ? String(s.profile_dir) : "",
        enabled,
      };
    case "browser_navigate":
      return { id, kind: "browser_navigate", url: String(s.url), instance: String(s.instance ?? ""), enabled };
    case "browser_click":
      return { id, kind: "browser_click", selector: parseBrowserSelector(s.selector), instance: String(s.instance ?? ""), enabled };
    case "browser_get_text":
      return {
        id,
        kind: "browser_get_text",
        selector: parseBrowserSelector(s.selector),
        variable: String(s.variable),
        instance: String(s.instance ?? ""),
        enabled,
      };
    case "browser_set_value":
      return {
        id,
        kind: "browser_set_value",
        selector: parseBrowserSelector(s.selector),
        value: String(s.value),
        instance: String(s.instance ?? ""),
        enabled,
      };
    case "browser_wait_for_selector":
      return {
        id,
        kind: "browser_wait_for_selector",
        selector: parseBrowserSelector(s.selector),
        instance: String(s.instance ?? ""),
        enabled,
      };
    case "if": {
      const condition = s.condition as Record<string, unknown>;
      return {
        id,
        kind: "if",
        condition: { variable: String(condition.variable), equals: String(condition.equals) },
        then: parseBranchYaml(s.then),
        otherwise: parseBranchYaml(s.otherwise),
        enabled,
      };
    }
    case "loop":
      return { id, kind: "loop", count: Number(s.count), body: parseBranchYaml(s.body), enabled };
    case "try_catch":
      return { id, kind: "try_catch", tryBranch: parseBranchYaml(s.try_branch), catch: parseBranchYaml(s.catch), enabled };
    case "function_def":
      return { id, kind: "function_def", name: String(s.name), body: parseBranchYaml(s.body), enabled };
    case "call_function":
      return { id, kind: "call_function", name: String(s.name), enabled };
    default:
      throw new Error(`step "${id}" has an unrecognized type "${type}"`);
  }
}

/** The inverse of `buildFlowYaml` — turns a saved flow file's text
 *  back into the editable graph plus the flow's name. Throws with a
 *  human-readable message on anything it can't make sense of, so a
 *  hand-edited or corrupted file fails to open cleanly rather than
 *  silently producing a broken canvas. */
export function parseFlowYaml(
  yamlText: string,
): { name: string; flow: Branch; positions: Record<string, { x: number; y: number }>; stepDelayMs: number } {
  const doc = parseYamlDocument(yamlText);
  if (!doc || typeof doc !== "object") throw new Error("This file isn't a valid flow (expected a YAML mapping at the top level).");
  const obj = doc as Record<string, unknown>;
  const name = typeof obj.name === "string" ? obj.name : "Untitled";
  let flow = parseBranchYaml(obj);
  bumpCounterPast(allIds(flow));
  if (obj.comments && typeof obj.comments === "object") {
    for (const [id, text] of Object.entries(obj.comments as Record<string, unknown>)) {
      if (typeof text === "string" && text) flow = updateNode(flow, id, (n) => ({ ...n, comment: text }));
    }
  }
  const positions: Record<string, { x: number; y: number }> = {};
  if (obj.layout && typeof obj.layout === "object") {
    for (const [id, pos] of Object.entries(obj.layout as Record<string, unknown>)) {
      const p = pos as Record<string, unknown>;
      if (typeof p?.x === "number" && typeof p?.y === "number") positions[id] = { x: p.x, y: p.y };
    }
  }
  const stepDelayMs = typeof obj.step_delay_ms === "number" ? obj.step_delay_ms : 0;
  return { name, flow, positions, stepDelayMs };
}
