/** Turns a `Branch` graph into the `.relay` YAML text the Rust engine
 *  parses (`flow_schema::parse_flow`) — see `flowYamlParse.ts` for the
 *  inverse direction. */
import { collectComments } from "./flowGraph";
import type { Branch, BrowserSelectorField, FlowNode, WindowSelectorField } from "./flowModel";

/** Emits `value` as a double-quoted YAML scalar. Every field this
 *  writes is exactly one physical line (the emitter builds the file
 *  line by line), so any literal control character in `value` — a raw
 *  newline typed into one of the multi-line text boxes, a tab, a CR —
 *  has to become its two-character YAML escape (`\n`, `\t`, `\r`)
 *  rather than the real character, or it would split into extra
 *  physical lines and corrupt the file's structure. */
function yamlString(value: string): string {
  return `"${value
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\n/g, "\\n")
    .replace(/\r/g, "\\r")
    .replace(/\t/g, "\\t")}"`;
}

/** Mirrors `flow_schema::BrowserSelector`'s untagged shape: `css`
 *  emits as a plain quoted string (and is the only shape the on-page
 *  picker ever produces), while `text`/`attribute` emit as a small
 *  nested mapping tagged by `kind` — matching what `background.js`'s
 *  `tryFindElement` expects to receive. */
function browserSelectorYamlLines(indent: string, field: BrowserSelectorField): string[] {
  if (field.kind === "css") {
    return [`${indent}selector: ${yamlString(field.value)}`];
  }
  if (field.kind === "text") {
    return [`${indent}selector:`, `${indent}  kind: text`, `${indent}  value: ${yamlString(field.value)}`];
  }
  return [
    `${indent}selector:`,
    `${indent}  kind: attribute`,
    `${indent}  name: ${yamlString(field.name)}`,
    `${indent}  value: ${yamlString(field.value)}`,
  ];
}

/** Mirrors `flow_schema::WindowSelector`'s untagged shape: `title`
 *  emits as a plain quoted string (the exact-match/simple case), the
 *  other three modes emit as a small nested mapping tagged by `kind`.
 */
function windowSelectorYamlLines(indent: string, label: string, field: WindowSelectorField): string[] {
  if (field.kind === "title") {
    return [`${indent}${label}: ${yamlString(field.title)}`];
  }
  if (field.kind === "title_contains") {
    return [`${indent}${label}:`, `${indent}  kind: title_contains`, `${indent}  text: ${yamlString(field.text)}`];
  }
  if (field.kind === "process") {
    return [`${indent}${label}:`, `${indent}  kind: process`, `${indent}  process_name: ${yamlString(field.processName)}`];
  }
  return [
    `${indent}${label}:`,
    `${indent}  kind: title_then_process`,
    `${indent}  title: ${yamlString(field.title)}`,
    `${indent}  process_name: ${yamlString(field.processName)}`,
  ];
}

function branchYamlLines(indent: string, label: string, branch: Branch): string[] {
  return [`${indent}${label}:`, ...branchBodyYamlLines(`${indent}  `, branch)];
}

function branchBodyYamlLines(indent: string, branch: Branch): string[] {
  const lines: string[] = [];
  if (branch.steps.length === 0) {
    lines.push(`${indent}steps: []`);
  } else {
    lines.push(`${indent}steps:`);
    for (const child of branch.steps) lines.push(...nodeYamlLines(child, `${indent}  `));
  }
  if (branch.connections.length === 0) {
    lines.push(`${indent}connections: []`);
  } else {
    lines.push(`${indent}connections:`);
    for (const c of branch.connections) {
      lines.push(`${indent}  - from: ${c.from}`);
      if (c.fromPort) lines.push(`${indent}    from_port: ${yamlString(c.fromPort)}`);
      lines.push(`${indent}    to: ${c.to}`);
    }
  }
  if (branch.entry) lines.push(`${indent}entry: ${branch.entry}`);
  return lines;
}

function nodeYamlLines(node: FlowNode, indent: string): string[] {
  const head = `${indent}- id: ${node.id}`;
  const lines = nodeActionYamlLines(node, indent, head);
  if (!node.enabled) lines.push(`${indent}  enabled: false`);
  if (node.breakpoint) lines.push(`${indent}  breakpoint: true`);
  // Mirrors `flow_schema::RetryPolicy` — a nested `retry:` block, not a
  // flat field, and only emitted at all when it says something other
  // than the defaults (no retry, fail the flow on failure).
  const maxAttempts = node.retryMaxAttempts ?? 1;
  const intervalMs = node.retryIntervalMs ?? 0;
  const onFailure = node.onFailure ?? "fail";
  if (maxAttempts > 1 || intervalMs > 0 || onFailure === "skip") {
    lines.push(`${indent}  retry:`);
    if (maxAttempts > 1) lines.push(`${indent}    max_attempts: ${Math.round(maxAttempts)}`);
    if (intervalMs > 0) lines.push(`${indent}    interval_ms: ${Math.round(intervalMs)}`);
    if (onFailure === "skip") lines.push(`${indent}    on_failure: skip`);
  }
  return lines;
}

function nodeActionYamlLines(node: FlowNode, indent: string, head: string): string[] {
  switch (node.kind) {
    case "start":
      return [head, `${indent}  type: start`];
    case "error_handler":
      return [head, `${indent}  type: error_handler`];
    case "wait":
      return [head, `${indent}  type: wait`, `${indent}  seconds: ${node.seconds}`];
    case "set_variable":
      return [head, `${indent}  type: set_variable`, `${indent}  name: ${node.name}`, `${indent}  value: ${yamlString(node.value)}`];
    case "calculate":
      return [
        head,
        `${indent}  type: calculate`,
        `${indent}  a: ${yamlString(node.a)}`,
        `${indent}  op: ${node.op}`,
        `${indent}  b: ${yamlString(node.b)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "type_text":
      return [head, `${indent}  type: type_text`, `${indent}  text: ${yamlString(node.text)}`];
    case "click":
      return [
        head,
        `${indent}  type: click`,
        `${indent}  target:`,
        `${indent}    kind: cursor`,
        `${indent}  button: ${node.button}`,
        `${indent}  click_kind: ${node.clickKind}`,
      ];
    case "move_mouse":
      return [
        head,
        `${indent}  type: move_mouse`,
        ...(node.targetKind === "last_match"
          ? [`${indent}  target:`, `${indent}    kind: last_match`]
          : [
              `${indent}  target:`,
              `${indent}    kind: coordinate`,
              `${indent}    monitor_id: primary`,
              `${indent}    x: ${node.x}`,
              `${indent}    y: ${node.y}`,
            ]),
        `${indent}  duration_ms: ${node.durationMs}`,
      ];
    case "key_press":
      return [
        head,
        `${indent}  type: key_press`,
        `${indent}  key: ${node.key}`,
        `${indent}  mode: ${node.mode}`,
        `${indent}  modifiers:`,
        `${indent}    ctrl: ${node.modifiers.ctrl}`,
        `${indent}    alt: ${node.modifiers.alt}`,
        `${indent}    shift: ${node.modifiers.shift}`,
        `${indent}    win: ${node.modifiers.win}`,
      ];
    case "find_image":
      return [
        head,
        `${indent}  type: find_image`,
        ...(node.image.kind === "embedded"
          ? [`${indent}  image:`, `${indent}    data: ${yamlString(node.image.data)}`]
          : [`${indent}  image: ${yamlString(node.image.value)}`]),
        `${indent}  mode: ${node.mode}`,
        `${indent}  threshold: ${node.threshold}`,
        `${indent}  min_scale: ${node.minScale}`,
        `${indent}  max_scale: ${node.maxScale}`,
        `${indent}  scale_steps: ${node.scaleSteps}`,
      ];
    case "find_text_ocr":
      return [
        head,
        `${indent}  type: find_text_ocr`,
        `${indent}  text: ${yamlString(node.text)}`,
        ...(node.region
          ? [
              `${indent}  region:`,
              `${indent}    x: ${node.region.x}`,
              `${indent}    y: ${node.region.y}`,
              `${indent}    width: ${node.region.width}`,
              `${indent}    height: ${node.region.height}`,
            ]
          : []),
      ];
    case "wait_for_window":
      return [
        head,
        `${indent}  type: wait_for_window`,
        ...windowSelectorYamlLines(`${indent}  `, "window", node.window),
        `${indent}  timeout_ms: ${Math.round(node.timeoutMs)}`,
      ];
    case "focus_window":
      return [head, `${indent}  type: focus_window`, ...windowSelectorYamlLines(`${indent}  `, "window", node.window)];
    case "power_action":
      return [head, `${indent}  type: power_action`, `${indent}  mode: ${node.mode}`, ...(node.force ? [`${indent}  force: true`] : [])];
    case "lock_workstation":
      return [head, `${indent}  type: lock_workstation`];
    case "read_clipboard":
      return [head, `${indent}  type: read_clipboard`, `${indent}  variable: ${node.variable}`];
    case "write_clipboard":
      return [head, `${indent}  type: write_clipboard`, `${indent}  text: ${yamlString(node.text)}`];
    case "show_message":
      return [
        head,
        `${indent}  type: show_message`,
        `${indent}  title: ${yamlString(node.title)}`,
        `${indent}  message: ${yamlString(node.message)}`,
        ...(node.blocking ? [] : [`${indent}  blocking: false`]),
      ];
    case "show_confirm":
      return [
        head,
        `${indent}  type: show_confirm`,
        `${indent}  title: ${yamlString(node.title)}`,
        `${indent}  message: ${yamlString(node.message)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "show_input":
      return [
        head,
        `${indent}  type: show_input`,
        `${indent}  title: ${yamlString(node.title)}`,
        `${indent}  message: ${yamlString(node.message)}`,
        `${indent}  default_value: ${yamlString(node.defaultValue)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "stop":
      return [head, `${indent}  type: stop`];
    case "break":
      return [head, `${indent}  type: break`];
    case "continue":
      return [head, `${indent}  type: continue`];
    case "return":
      return [head, `${indent}  type: return`];
    case "get_date_time":
      return [head, `${indent}  type: get_date_time`, `${indent}  format: ${node.format}`, `${indent}  variable: ${node.variable}`];
    case "get_system_info":
      return [
        head,
        `${indent}  type: get_system_info`,
        ...(node.hostname ? [`${indent}  hostname: ${node.hostname}`] : []),
        ...(node.osVersion ? [`${indent}  os_version: ${node.osVersion}`] : []),
        ...(node.cpuPercent ? [`${indent}  cpu_percent: ${node.cpuPercent}`] : []),
        ...(node.memoryPercent ? [`${indent}  memory_percent: ${node.memoryPercent}`] : []),
        ...(node.ipAddress ? [`${indent}  ip_address: ${node.ipAddress}`] : []),
      ];
    case "text_transform":
      return [
        head,
        `${indent}  type: text_transform`,
        `${indent}  op: ${node.op}`,
        `${indent}  text: ${yamlString(node.text)}`,
        `${indent}  arg1: ${yamlString(node.arg1)}`,
        `${indent}  arg2: ${yamlString(node.arg2)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "launch_app":
      return [head, `${indent}  type: launch_app`, `${indent}  path: ${yamlString(node.path)}`, `${indent}  args: ${yamlString(node.args)}`];
    case "open_url":
      return [head, `${indent}  type: open_url`, `${indent}  url: ${yamlString(node.url)}`];
    case "notify":
      return [head, `${indent}  type: notify`, `${indent}  title: ${yamlString(node.title)}`, `${indent}  message: ${yamlString(node.message)}`];
    case "read_file":
      return [head, `${indent}  type: read_file`, `${indent}  path: ${yamlString(node.path)}`, `${indent}  variable: ${node.variable}`];
    case "write_file":
      return [
        head,
        `${indent}  type: write_file`,
        `${indent}  path: ${yamlString(node.path)}`,
        `${indent}  content: ${yamlString(node.content)}`,
        ...(node.append ? [`${indent}  append: true`] : []),
      ];
    case "copy_file":
      return [head, `${indent}  type: copy_file`, `${indent}  source: ${yamlString(node.source)}`, `${indent}  destination: ${yamlString(node.destination)}`];
    case "move_file":
      return [head, `${indent}  type: move_file`, `${indent}  source: ${yamlString(node.source)}`, `${indent}  destination: ${yamlString(node.destination)}`];
    case "delete_file":
      return [head, `${indent}  type: delete_file`, `${indent}  path: ${yamlString(node.path)}`];
    case "create_directory":
      return [head, `${indent}  type: create_directory`, `${indent}  path: ${yamlString(node.path)}`];
    case "list_directory":
      return [head, `${indent}  type: list_directory`, `${indent}  path: ${yamlString(node.path)}`, `${indent}  variable: ${node.variable}`];
    case "http":
      return [
        head,
        `${indent}  type: http`,
        `${indent}  method: ${node.method}`,
        `${indent}  url: ${yamlString(node.url)}`,
        `${indent}  headers: ${yamlString(node.headers)}`,
        `${indent}  body: ${yamlString(node.body)}`,
        `${indent}  variable: ${node.variable}`,
        `${indent}  status_variable: ${node.statusVariable}`,
      ];
    case "http_download":
      return [
        head,
        `${indent}  type: http_download`,
        `${indent}  url: ${yamlString(node.url)}`,
        `${indent}  headers: ${yamlString(node.headers)}`,
        `${indent}  path: ${yamlString(node.path)}`,
        `${indent}  variable: ${node.variable}`,
        `${indent}  path_variable: ${node.pathVariable}`,
      ];
    case "ping":
      return [
        head,
        `${indent}  type: ping`,
        `${indent}  host: ${yamlString(node.host)}`,
        `${indent}  timeout_ms: ${node.timeoutMs}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "dns_lookup":
      return [head, `${indent}  type: dns_lookup`, `${indent}  hostname: ${yamlString(node.hostname)}`, `${indent}  variable: ${node.variable}`];
    case "screenshot":
      return [
        head,
        `${indent}  type: screenshot`,
        ...(node.region
          ? [
              `${indent}  region:`,
              `${indent}    x: ${node.region.x}`,
              `${indent}    y: ${node.region.y}`,
              `${indent}    width: ${node.region.width}`,
              `${indent}    height: ${node.region.height}`,
            ]
          : []),
        `${indent}  path: ${yamlString(node.path)}`,
      ];
    case "browser_screenshot":
      return [
        head,
        `${indent}  type: browser_screenshot`,
        `${indent}  path: ${yamlString(node.path)}`,
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "get_env_var":
      return [head, `${indent}  type: get_env_var`, `${indent}  name: ${yamlString(node.name)}`, `${indent}  variable: ${node.variable}`];
    case "check_process":
      return [head, `${indent}  type: check_process`, `${indent}  name: ${yamlString(node.name)}`, `${indent}  variable: ${node.variable}`];
    case "kill_process":
      return [
        head,
        `${indent}  type: kill_process`,
        `${indent}  name: ${yamlString(node.name)}`,
        ...(node.force ? [`${indent}  force: true`] : []),
      ];
    case "wait_for_file":
      return [head, `${indent}  type: wait_for_file`, `${indent}  path: ${yamlString(node.path)}`, `${indent}  timeout_ms: ${node.timeoutMs}`];
    case "generate_random":
      return [
        head,
        `${indent}  type: generate_random`,
        `${indent}  min: ${yamlString(node.min)}`,
        `${indent}  max: ${yamlString(node.max)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "get_element_text":
      return [
        head,
        `${indent}  type: get_element_text`,
        `${indent}  selector:`,
        ...(node.windowTitle ? [`${indent}    window_title: ${yamlString(node.windowTitle)}`] : []),
        ...(node.automationId ? [`${indent}    automation_id: ${yamlString(node.automationId)}`] : []),
        `${indent}    name: ${yamlString(node.elementName)}`,
        `${indent}  variable: ${node.variable}`,
      ];
    case "launch_browser":
      return [
        head,
        `${indent}  type: launch_browser`,
        `${indent}  url: ${yamlString(node.url)}`,
        `${indent}  variable: ${node.variable}`,
        ...(node.browser ? [`${indent}  browser: ${yamlString(node.browser)}`] : []),
        ...(node.profileDir ? [`${indent}  profile_dir: ${yamlString(node.profileDir)}`] : []),
      ];
    case "browser_navigate":
      return [
        head,
        `${indent}  type: browser_navigate`,
        `${indent}  url: ${yamlString(node.url)}`,
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "browser_click":
      return [
        head,
        `${indent}  type: browser_click`,
        ...browserSelectorYamlLines(`${indent}  `, node.selector),
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "browser_get_text":
      return [
        head,
        `${indent}  type: browser_get_text`,
        ...browserSelectorYamlLines(`${indent}  `, node.selector),
        `${indent}  variable: ${node.variable}`,
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "browser_set_value":
      return [
        head,
        `${indent}  type: browser_set_value`,
        ...browserSelectorYamlLines(`${indent}  `, node.selector),
        `${indent}  value: ${yamlString(node.value)}`,
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "browser_wait_for_selector":
      return [
        head,
        `${indent}  type: browser_wait_for_selector`,
        ...browserSelectorYamlLines(`${indent}  `, node.selector),
        ...(node.instance ? [`${indent}  instance: ${yamlString(node.instance)}`] : []),
      ];
    case "if":
      return [
        head,
        `${indent}  type: if`,
        `${indent}  condition:`,
        `${indent}    variable: ${node.condition.variable}`,
        `${indent}    equals: ${yamlString(node.condition.equals)}`,
        ...branchYamlLines(`${indent}  `, "then", node.then),
        ...branchYamlLines(`${indent}  `, "otherwise", node.otherwise),
      ];
    case "loop":
      return [
        head,
        `${indent}  type: loop`,
        `${indent}  count: ${node.count}`,
        ...branchYamlLines(`${indent}  `, "body", node.body),
      ];
    case "try_catch":
      return [
        head,
        `${indent}  type: try_catch`,
        ...branchYamlLines(`${indent}  `, "try_branch", node.tryBranch),
        ...branchYamlLines(`${indent}  `, "catch", node.catch),
      ];
    case "function_def":
      return [
        head,
        `${indent}  type: function_def`,
        `${indent}  name: ${yamlString(node.name)}`,
        ...branchYamlLines(`${indent}  `, "body", node.body),
      ];
    case "call_function":
      return [head, `${indent}  type: call_function`, `${indent}  name: ${yamlString(node.name)}`];
  }
}

/** `positions` is purely an editor concern (manual drag placement) —
 *  the Rust engine's `Flow` struct has no `layout` field and silently
 *  ignores it, so this only round-trips through `parseFlowYaml` back
 *  into the canvas. Omitted entirely for a flow with no manually
 *  placed nodes, so a from-scratch flow's file doesn't carry a
 *  meaningless empty section. */
export function buildFlowYaml(
  flow: Branch,
  name = "Backend smoke test",
  positions: Record<string, { x: number; y: number }> = {},
  stepDelayMs = 0,
): string {
  const lines = [`name: ${name}`, ...branchBodyYamlLines("", flow)];
  if (stepDelayMs > 0) lines.push(`step_delay_ms: ${Math.round(stepDelayMs)}`);
  const entries = Object.entries(positions);
  if (entries.length > 0) {
    lines.push("layout:");
    for (const [id, pos] of entries) {
      lines.push(`  ${id}: { x: ${pos.x}, y: ${pos.y} }`);
    }
  }
  const comments = Object.entries(collectComments(flow));
  if (comments.length > 0) {
    lines.push("comments:");
    for (const [id, text] of comments) {
      lines.push(`  ${id}: ${yamlString(text)}`);
    }
  }
  return lines.join("\n") + "\n";
}
