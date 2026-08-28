/**
 * The whole flow as a graph the user can genuinely wire and unwire —
 * close to how n8n wires nodes together. Every kind here maps 1:1 to
 * an Action the engine actually implements (see crates/flow-schema):
 * wait / set_variable / type_text / click / move_mouse / key_press /
 * find_image / if / loop. Trigger types (hotkey/schedule), OCR,
 * window-wait, and a stop action are NOT representable yet — the Rust
 * side has no such Action variant, so the palette leaves them visibly
 * disabled rather than pretending.
 *
 * A `Branch` is a pool of steps plus explicit `connections` between
 * them, and an `entry` marking which step runs first. Execution order
 * is NOT array order — it's whatever `connections` says. A step that
 * exists in the pool but has no incoming wire (and isn't `entry`)
 * simply never runs: it sits on the canvas, wired to nothing, exactly
 * like an unconnected n8n node. Disconnecting a wire only ever removes
 * that one `Connection` — the steps on either end never move and
 * never get reattached anywhere else.
 *
 * `if` is a nested container exactly like `loop`: its `then`/
 * `otherwise` are each a self-contained `Branch` (their own steps,
 * connections, and entry). Whichever one actually runs, execution
 * falls back out to the `if` step's own single plain output once
 * that branch's chain ends — both paths always rejoin there, there's
 * no way for one to lead somewhere the other doesn't.
 *
 * This file holds the core types plus `describeNode` (the canvas
 * summary renderer). The graph-manipulation functions (add/delete/
 * connect/clone/...) live in `flowGraph.ts`, and YAML round-tripping
 * lives in `flowYamlSerialize.ts`/`flowYamlParse.ts` — both are
 * re-exported from here so existing imports of `"./flowModel"` /
 * `"../data/flowModel"` keep working unchanged.
 */
export type MouseButtonKind = "left" | "right" | "middle";
export type ClickKind = "single" | "double";

/** Mirrors `flow_schema::KeyPressMode` — see its doc comment. `tap`
 *  presses and releases in one step (the ordinary case); `press`
 *  holds the key down without releasing (for a custom multi-step
 *  hold, e.g. holding Shift across several `click` steps to
 *  multi-select); `release` lets go of a key a matching `press` left
 *  held. Any key still held when the flow ends is force-released by
 *  the engine regardless — see `KeyPress`'s inspector hint. */
export type KeyPressMode = "tap" | "press" | "release";

/** Mirrors `flow_schema::KeyModifiers` — modifier keys to hold
 *  alongside a `key_press` step's `key`, so e.g. `key: "a"` with
 *  `ctrl: true` sends Ctrl+A in one step instead of three separate
 *  `key_press` steps (hold Ctrl, tap A, release Ctrl). Meaningless
 *  for `mode: "release"` — see `KeyPressMode`'s doc comment. */
export interface KeyModifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  win: boolean;
}

export function makeKeyModifiers(): KeyModifiers {
  return { ctrl: false, alt: false, shift: false, win: false };
}

/** Mirrors `flow_schema::DateTimeFormat` — see its doc comment for
 *  each preset's exact output shape. */
export type DateTimeFormat = "iso8601" | "date_only" | "time_only" | "unix_seconds";

/** Mirrors `flow_schema::TextOp` — see its doc comment for what each
 *  operation does and how `arg1`/`arg2` are used. */
export type TextOp =
  | "uppercase"
  | "lowercase"
  | "trim"
  | "replace"
  | "substring"
  | "length"
  | "contains"
  | "starts_with"
  | "ends_with"
  | "split"
  | "base64_encode"
  | "base64_decode"
  | "md5"
  | "sha256"
  | "json_get"
  | "json_escape"
  | "regex_test"
  | "regex_match";

export function textOpPaletteKey(op: TextOp): string {
  if (op === "uppercase" || op === "lowercase" || op === "trim") return "textFormat";
  if (op === "replace") return "textReplace";
  if (op === "substring" || op === "split") return "textExtract";
  if (op === "length" || op === "contains" || op === "starts_with" || op === "ends_with") return "textCheck";
  if (op === "base64_encode" || op === "base64_decode" || op === "json_escape") return "textEncode";
  if (op === "md5" || op === "sha256") return "textHash";
  if (op === "json_get") return "textJson";
  return "textRegex";
}

/** `"Ctrl+Shift+A"`-style human-readable summary of a key + its held
 *  modifiers, for the canvas node preview and inspector title. */
export function formatKeyCombo(key: string, modifiers: KeyModifiers): string {
  const parts: string[] = [];
  if (modifiers.ctrl) parts.push("Ctrl");
  if (modifiers.alt) parts.push("Alt");
  if (modifiers.shift) parts.push("Shift");
  if (modifiers.win) parts.push("Win");
  parts.push(key);
  return parts.join("+");
}

/** Arithmetic and rounding operations — mirrors `flow_schema::CalcOp`. */
export type CalcOp = "add" | "subtract" | "multiply" | "divide" | "round" | "floor" | "ceil";

const CALC_OP_SYMBOL: Record<CalcOp, string> = {
  add: "+",
  subtract: "−",
  multiply: "×",
  divide: "÷",
  round: "round",
  floor: "floor",
  ceil: "ceil",
};

/** How a `Browser*` step finds its element — mirrors
 *  `flow_schema::BrowserSelector`/`BrowserSelectorSpec`. `css` is the
 *  plain/default case (and the only thing the on-page picker
 *  produces); `text`/`attribute` are alternatives for when a page
 *  redesign could change class names out from under a CSS selector
 *  but the element's own visible text or a semantic attribute
 *  (`placeholder`, `aria-label`, `name`, ...) stays put. `name` is
 *  only meaningful for `kind: "attribute"`. */
export type BrowserSelectorField = { kind: "css" | "text" | "attribute"; value: string; name: string };

export function makeBrowserSelector(value = ""): BrowserSelectorField {
  return { kind: "css", value, name: "" };
}

/** How `wait_for_window`/`focus_window` find the window they mean —
 *  mirrors `flow_schema::WindowSelector`/`WindowSelectorSpec`. `title`
 *  is exact-match (the original, simplest case); `title_contains`
 *  survives a title that grew a prefix/suffix; `process` ignores
 *  title entirely and matches by owning executable; `title_then_process`
 *  tries the exact title first and falls back to the process — what
 *  the "pick from open windows" button actually fills in, since a
 *  live window naturally has both. `title`/`text`/`processName` are
 *  the fields relevant to whichever `kind` is selected; the others
 *  sit unused, the same flat-shape convention `BrowserSelectorField`
 *  already uses. */
export type WindowSelectorField = {
  kind: "title" | "title_contains" | "process" | "title_then_process";
  title: string;
  text: string;
  processName: string;
};

export function makeWindowSelector(title = ""): WindowSelectorField {
  return { kind: "title", title, text: "", processName: "" };
}

function describeWindowSelector(field: WindowSelectorField): string {
  if (field.kind === "title_contains") return `⊃ "${field.text}"`;
  if (field.kind === "process") return field.processName;
  if (field.kind === "title_then_process") return `"${field.title}" / ${field.processName}`;
  return field.title;
}

/** Where a `find_image` step's reference image comes from — mirrors
 *  `flow_schema::ImageSource`. `path` points at a file on disk (the
 *  original behavior); `embedded` carries the image's base64-encoded
 *  bytes straight in the flow file, so the `.relay` file is
 *  self-contained and safe to copy/share without a separate asset. */
export type ImageSourceField = { kind: "path"; value: string } | { kind: "embedded"; data: string };

export function makeImageSource(value = ""): ImageSourceField {
  return { kind: "path", value };
}

function describeBrowserSelector(field: BrowserSelectorField): string {
  if (field.kind === "text") return `text: "${field.value}"`;
  if (field.kind === "attribute") return `${field.name}="${field.value}"`;
  return field.value;
}

/** A directed wire from one step's output to another step, both
 *  living in the same `Branch`. `fromPort` distinguishes which output
 *  the wire leaves from — most steps have a single unnamed output
 *  (`null`), while an `if` step has two (`"yes"` / `"no"`). */
export type Connection = { from: string; fromPort: string | null; to: string };

/** A self-contained pool of steps + wires — the top-level flow, or a
 *  `loop`'s `body`. `entry` is which step this container starts from;
 *  `null` means nothing runs here yet (an empty or fully-disconnected
 *  container). */
export type Branch = { steps: FlowNode[]; connections: Connection[]; entry: string | null };

/** Once retries are exhausted, `"fail"` (the default) stops the whole
 *  flow the same as before; `"skip"` logs the failure but moves on to
 *  the next step anyway — for a step whose absence shouldn't sink the
 *  rest of the flow (an optional click, a best-effort read). Mirrors
 *  `flow_schema::FailureBehavior`; `undefined` (an older saved file,
 *  or a control-flow step that doesn't expose it in the UI) means
 *  `"fail"`. */
export type FailureBehavior = "fail" | "skip";

/** `enabled: false` marks a step "muted" — it stays wired in place
 *  (nothing else has to move or reconnect) but the engine skips it
 *  entirely at run time. See crates/flow-schema's `Step::enabled`. */
export type FlowNode = (
  | { id: string; kind: "start"; enabled: boolean }
  /** A pure marker, like `start` — at most one per flow, top-level
   *  only (both enforced by `App.tsx`'s `handleAddStep`, not by this
   *  type). When the flow fails with an uncaught error, the engine
   *  jumps to whatever this node's own plain output is wired to
   *  instead of ending the run as failed — see
   *  `crates/flow-schema/src/action.rs`'s `Action::ErrorHandler` doc
   *  comment. Never triggered by the Stop button. */
  | { id: string; kind: "error_handler"; enabled: boolean }
  | { id: string; kind: "wait"; seconds: number; enabled: boolean }
  | { id: string; kind: "set_variable"; name: string; value: string; enabled: boolean }
  | { id: string; kind: "calculate"; a: string; op: CalcOp; b: string; variable: string; enabled: boolean }
  | { id: string; kind: "type_text"; text: string; enabled: boolean }
  | {
      /** Never carries a position — that's `move_mouse`'s job (a
       *  fixed coordinate, or the last `find_image` match). Mirrors
       *  `flow_schema::ClickTarget::Cursor`: this just presses a
       *  button wherever the cursor already is. Move there first with
       *  a `move_mouse` step, then click. */
      id: string;
      kind: "click";
      button: MouseButtonKind;
      clickKind: ClickKind;
      enabled: boolean;
    }
  | {
      id: string;
      kind: "move_mouse";
      x: number;
      y: number;
      durationMs: number;
      /** See `click`'s `targetKind` — mirrors
       *  `flow_schema::PointTarget::LastMatch`. */
      targetKind?: "coordinate" | "last_match";
      enabled: boolean;
    }
  | { id: string; kind: "key_press"; key: string; mode: KeyPressMode; modifiers: KeyModifiers; enabled: boolean }
  | {
      id: string;
      kind: "find_image";
      image: ImageSourceField;
      mode: "exact" | "similar";
      threshold: number;
      /** Only used by `mode: "similar"` — how much smaller/larger
       *  than the reference image a match is still allowed to be
       *  (1.0 = same size). Mirrors `flow_schema::Action::FindImage`'s
       *  `min_scale`/`max_scale`. */
      minScale: number;
      maxScale: number;
      /** Only used by `mode: "similar"` — how many scale steps to
       *  sample across `[minScale, maxScale]`. More steps costs
       *  roughly proportionally more time for a finer-grained match;
       *  fewer is faster but coarser. Mirrors
       *  `flow_schema::Action::FindImage`'s `scale_steps`; the
       *  Inspector's "認識パフォーマンス" low/balanced/high presets set
       *  this together with `minScale`/`maxScale`. */
      scaleSteps: number;
      enabled: boolean;
    }
  | {
      id: string;
      kind: "find_text_ocr";
      text: string;
      /** Limits the OCR scan to part of the screen instead of always
       *  reading the whole thing — mirrors
       *  `flow_schema::CaptureRegion`. `undefined` (an older saved
       *  file, or nobody's set one) means the whole screen. */
      region?: { x: number; y: number; width: number; height: number };
      enabled: boolean;
    }
  | { id: string; kind: "wait_for_window"; window: WindowSelectorField; timeoutMs: number; enabled: boolean }
  | { id: string; kind: "stop"; enabled: boolean }
  | { id: string; kind: "launch_app"; path: string; args: string; enabled: boolean }
  | { id: string; kind: "open_url"; url: string; enabled: boolean }
  | { id: string; kind: "notify"; title: string; message: string; enabled: boolean }
  | { id: string; kind: "read_file"; path: string; variable: string; enabled: boolean }
  | { id: string; kind: "write_file"; path: string; content: string; append: boolean; enabled: boolean }
  | { id: string; kind: "copy_file"; source: string; destination: string; enabled: boolean }
  | { id: string; kind: "move_file"; source: string; destination: string; enabled: boolean }
  | { id: string; kind: "delete_file"; path: string; enabled: boolean }
  | { id: string; kind: "create_directory"; path: string; enabled: boolean }
  | { id: string; kind: "list_directory"; path: string; variable: string; enabled: boolean }
  | { id: string; kind: "focus_window"; window: WindowSelectorField; enabled: boolean }
  | { id: string; kind: "power_action"; mode: "shutdown" | "restart"; force: boolean; enabled: boolean }
  | { id: string; kind: "lock_workstation"; enabled: boolean }
  | { id: string; kind: "read_clipboard"; variable: string; enabled: boolean }
  | { id: string; kind: "write_clipboard"; text: string; enabled: boolean }
  /** `blocking` mirrors `flow_schema::Action::ShowMessage`'s field of
   *  the same name: whether the flow waits for the user to dismiss
   *  the box before continuing, or moves on immediately, leaving it
   *  open. */
  | { id: string; kind: "show_message"; title: string; message: string; blocking: boolean; enabled: boolean }
  /** `variable` receives `"yes"`/`"no"` — mirrors
   *  `flow_schema::Action::ShowConfirm`. */
  | { id: string; kind: "show_confirm"; title: string; message: string; variable: string; enabled: boolean }
  | { id: string; kind: "show_input"; title: string; message: string; defaultValue: string; variable: string; enabled: boolean }
  /** Exits the innermost enclosing `loop` immediately — mirrors
   *  `flow_schema::Action::Break`. */
  | { id: string; kind: "break"; enabled: boolean }
  /** Skips to the next iteration of the innermost enclosing `loop` —
   *  mirrors `flow_schema::Action::Continue`. */
  | { id: string; kind: "continue"; enabled: boolean }
  /** Ends the current `function_def` call early, returning to right
   *  after the `call_function` step that invoked it — mirrors
   *  `flow_schema::Action::Return`. */
  | { id: string; kind: "return"; enabled: boolean }
  | { id: string; kind: "get_date_time"; format: DateTimeFormat; variable: string; enabled: boolean }
  /** Each field is independently `""` (skip gathering it) or a
   *  variable name to write it to — mirrors
   *  `flow_schema::Action::GetSystemInfo`'s doc comment: every output
   *  gets its own independently-named variable, none of them a fixed
   *  suffix on a shared prefix. */
  | {
      id: string;
      kind: "get_system_info";
      hostname: string;
      osVersion: string;
      cpuPercent: string;
      memoryPercent: string;
      ipAddress: string;
      enabled: boolean;
    }
  | { id: string; kind: "text_transform"; op: TextOp; text: string; arg1: string; arg2: string; variable: string; enabled: boolean }
  | {
      id: string;
      kind: "http";
      method: "get" | "post" | "put" | "patch" | "delete";
      url: string;
      /** `Name: Value` pairs, one per line. */
      headers: string;
      body: string;
      /** Response body; the status code lands separately in
       *  `statusVariable` — mirrors `flow_schema::Action::Http`: two
       *  independently-named outputs, neither a fixed suffix on the
       *  other. */
      variable: string;
      statusVariable: string;
      enabled: boolean;
    }
  /** Downloads `url`'s response body straight to `path` (a file, an
   *  image, a zip, ...) instead of reading it into a text variable —
   *  mirrors `flow_schema::Action::HttpDownload`. `variable` receives
   *  the numeric status code; `pathVariable` receives the actual saved
   *  path (`path` after `%variable%` substitution). */
  | { id: string; kind: "http_download"; url: string; headers: string; path: string; variable: string; pathVariable: string; enabled: boolean }
  /** `variable` receives `"true"`/`"false"`; `{variable}_latency_ms`
   *  is also written when reachable — mirrors
   *  `flow_schema::Action::Ping`. */
  | { id: string; kind: "ping"; host: string; timeoutMs: number; variable: string; enabled: boolean }
  | { id: string; kind: "dns_lookup"; hostname: string; variable: string; enabled: boolean }
  /** `region` unset captures the whole (virtual) desktop — mirrors
   *  `flow_schema::Action::Screenshot`. */
  | { id: string; kind: "screenshot"; region?: { x: number; y: number; width: number; height: number }; path: string; enabled: boolean }
  | { id: string; kind: "browser_screenshot"; path: string; instance: string; enabled: boolean }
  | { id: string; kind: "get_env_var"; name: string; variable: string; enabled: boolean }
  /** `variable` receives `"true"`/`"false"` — mirrors
   *  `flow_schema::Action::CheckProcess`. */
  | { id: string; kind: "check_process"; name: string; variable: string; enabled: boolean }
  | { id: string; kind: "kill_process"; name: string; force: boolean; enabled: boolean }
  | { id: string; kind: "wait_for_file"; path: string; timeoutMs: number; enabled: boolean }
  | { id: string; kind: "generate_random"; min: string; max: string; variable: string; enabled: boolean }
  | {
      id: string;
      kind: "get_element_text";
      windowTitle: string;
      elementName: string;
      automationId: string;
      variable: string;
      enabled: boolean;
    }
  | { id: string; kind: "launch_browser"; url: string; variable: string; browser: string; profileDir: string; enabled: boolean }
  /** `instance` is a `LaunchBrowser`-captured tab id (typically
   *  `"%variable%"`), or empty to fall back to whatever tab is
   *  currently active — mirrors `flow_schema::Action::BrowserNavigate`
   *  etc's `instance: Option<String>`. */
  | { id: string; kind: "browser_navigate"; url: string; instance: string; enabled: boolean }
  | { id: string; kind: "browser_click"; selector: BrowserSelectorField; instance: string; enabled: boolean }
  | { id: string; kind: "browser_get_text"; selector: BrowserSelectorField; variable: string; instance: string; enabled: boolean }
  | { id: string; kind: "browser_set_value"; selector: BrowserSelectorField; value: string; instance: string; enabled: boolean }
  | { id: string; kind: "browser_wait_for_selector"; selector: BrowserSelectorField; instance: string; enabled: boolean }
  | {
      id: string;
      kind: "if";
      condition: { variable: string; equals: string };
      /** Runs when `condition` matches — a self-contained pool of
       *  steps with its own `entry`, same as a `loop`'s `body`.
       *  Whichever of `then`/`otherwise` actually runs, execution
       *  falls back out to this step's own single next connection
       *  once that branch's chain ends — mirrors
       *  `flow_schema::Action::If`. */
      then: Branch;
      /** Runs when `condition` doesn't match. */
      otherwise: Branch;
      enabled: boolean;
    }
  | { id: string; kind: "loop"; count: number; body: Branch; enabled: boolean }
  | {
      id: string;
      kind: "try_catch";
      /** Runs first, in isolation. If every one of its steps
       *  succeeds, `catch` never runs at all — mirrors
       *  `flow_schema::Action::TryCatch`'s `try_branch`. (Named
       *  `tryBranch` here, not `try` — a reserved word is fine as an
       *  object *property* name in TS, but `try` was picked as the
       *  Rust field name specifically to sidestep `try` also being
       *  reserved there, so this mirrors that choice for
       *  consistency rather than because it's required here too.) */
      tryBranch: Branch;
      /** Runs instead, if any step inside `tryBranch` fails — the
       *  failure's message is available to it as the `caught_error`
       *  variable. Whichever branch actually ran, execution falls
       *  back out to this step's own single next connection once
       *  that branch's chain ends, same reconvergence rule as `if`. */
      catch: Branch;
      enabled: boolean;
    }
  | {
      id: string;
      kind: "function_def";
      /** What `call_function` nodes elsewhere reference by name —
       *  Scratch's custom block, not tied to one call site the way a
       *  `loop`'s `body` is tied to that one node. Mirrors
       *  `flow_schema::Action::FunctionDef`. Nothing wires into this
       *  node in normal use (by convention, not enforcement) — its
       *  `body` only ever runs via a matching `call_function`'s
       *  lookup by name. */
      name: string;
      body: Branch;
      enabled: boolean;
    }
  | {
      id: string;
      kind: "call_function";
      /** The name of the `function_def` node whose `body` this runs —
       *  looked up at run time, not a wire, so the same function can
       *  be called from many places. */
      name: string;
      enabled: boolean;
    }
) & {
  onFailure?: FailureBehavior;
  /** Mirrors `flow_schema::RetryPolicy` — undefined/1 means "don't
   *  retry, just run once" (the pre-retry-UI default every step
   *  already ran with). */
  retryMaxAttempts?: number;
  retryIntervalMs?: number;
  /** Step-through debugging pauses the run just before this step —
   *  mirrors `flow_schema::Step::breakpoint`. `undefined` (an older
   *  saved file, or a step nobody's flagged) means false. */
  breakpoint?: boolean;
  /** A free-text sticky note attached to this step — purely an
   *  editor/documentation aid, like a Scratch comment. Never reaches
   *  the engine: stored the same way `layout` (node positions) is,
   *  as a sibling top-level `comments:` map in the YAML rather than a
   *  field on the step itself, so `flow_schema::Step` never needs to
   *  know it exists. `undefined`/empty means no comment. */
  comment?: string;
};

export type BranchKind = "if" | "loop" | "try_catch" | "function_def";
export type LeafKind = Exclude<FlowNode["kind"], BranchKind>;

export type CanvasKind = "trigger" | "image" | "action" | "control";

export const NODE_KIND_OF: Record<FlowNode["kind"], CanvasKind> = {
  start: "trigger",
  error_handler: "trigger",
  wait: "control",
  set_variable: "control",
  calculate: "control",
  if: "control",
  loop: "control",
  try_catch: "control",
  function_def: "control",
  call_function: "control",
  type_text: "action",
  click: "action",
  move_mouse: "action",
  key_press: "action",
  find_image: "image",
  find_text_ocr: "image",
  wait_for_window: "action",
  focus_window: "action",
  power_action: "action",
  lock_workstation: "action",
  read_clipboard: "action",
  write_clipboard: "action",
  show_message: "action",
  show_confirm: "action",
  show_input: "action",
  break: "control",
  continue: "control",
  return: "control",
  get_date_time: "action",
  get_system_info: "action",
  text_transform: "control",
  stop: "control",
  launch_app: "action",
  open_url: "action",
  notify: "action",
  read_file: "action",
  write_file: "action",
  copy_file: "action",
  move_file: "action",
  delete_file: "action",
  create_directory: "action",
  list_directory: "action",
  http: "action",
  http_download: "action",
  ping: "action",
  dns_lookup: "action",
  screenshot: "action",
  browser_screenshot: "action",
  get_env_var: "action",
  check_process: "action",
  kill_process: "action",
  wait_for_file: "action",
  generate_random: "control",
  get_element_text: "action",
  launch_browser: "action",
  browser_navigate: "action",
  browser_click: "action",
  browser_get_text: "action",
  browser_set_value: "action",
  browser_wait_for_selector: "action",
};

export const KEY_NAMES = [
  "enter",
  "tab",
  "escape",
  "space",
  "backspace",
  "delete",
  "up",
  "down",
  "left",
  "right",
  "home",
  "end",
  "page_up",
  "page_down",
  "ctrl",
  "alt",
  "shift",
  "win",
  "f1",
  "f2",
  "f3",
  "f4",
  "f5",
  "f6",
  "f7",
  "f8",
  "f9",
  "f10",
  "f11",
  "f12",
] as const;

export type NodeNameMode = "beginner" | "normal";

/** A palette item's display name — `palette.items.<key>` (the
 *  friendlier, more descriptive label every item already had) in
 *  "beginner" mode, or `palette.itemsNormal.<key>` (a shorter, more
 *  literal/technical label — set for control-flow-ish items like
 *  "if"/"loop"/"variable" where the two framings genuinely differ) in
 *  "normal" mode, falling back to the beginner label for any item
 *  that has no normal-mode entry of its own. */
export function paletteLabel(t: (key: string, opts?: Record<string, unknown>) => string, key: string, mode: NodeNameMode): string {
  if (mode === "beginner") return t(`palette.items.${key}`);
  const normal = t(`palette.itemsNormal.${key}`);
  return normal === `palette.itemsNormal.${key}` ? t(`palette.items.${key}`) : normal;
}

function isBlank(value: string | undefined): boolean {
  return !value || value.trim() === "";
}

function selectorIsBlank(selector: BrowserSelectorField): boolean {
  return selector.kind === "attribute"
    ? isBlank(selector.name) || isBlank(selector.value)
    : isBlank(selector.value);
}

function windowSelectorIsBlank(window: WindowSelectorField): boolean {
  if (window.kind === "title_contains") return isBlank(window.text);
  if (window.kind === "process") return isBlank(window.processName);
  if (window.kind === "title_then_process") return isBlank(window.title) || isBlank(window.processName);
  return isBlank(window.title);
}

function imageSourceIsBlank(image: ImageSourceField): boolean {
  return image.kind === "embedded" ? isBlank(image.data) : isBlank(image.value);
}

/** A sentinel key (not a real `inspector.fields.*` key) for the
 *  `get_element_text` case, where either `elementName` or
 *  `automationId` alone satisfies the requirement — rendered as a
 *  combined "one of these two" hint instead of naming a single field.
 *  See `MISSING_FIELD_LABEL_KEYS`' doc comment in Inspector.tsx. */
export const MISSING_ELEMENT_SELECTOR = "__elementSelector";
/** Same idea for `get_system_info`, where at least one of its five
 *  independently-optional fields must be set. */
export const MISSING_SYSTEM_INFO_FIELD = "__systemInfoField";

/** The `inspector.fields.*` key(s) for every field this node needs to
 *  actually run but currently has empty — an empty required text/
 *  variable/selector/path, not just an unusual default value. Empty
 *  means the canvas warning badge (`nodeIsIncomplete`); non-empty also
 *  drives the Inspector's own summary of *which* fields to fix,
 *  instead of a bare "something's wrong" badge with no further clue.
 *  Intentionally conservative: fields where an empty value is a
 *  legitimate choice (a message body, a value to assign, free-text
 *  content) are never included. */
export function nodeMissingFieldKeys(node: FlowNode): string[] {
  const missing: string[] = [];
  const need = (blank: boolean, key: string) => {
    if (blank) missing.push(key);
  };
  switch (node.kind) {
    case "set_variable":
      need(isBlank(node.name), "name");
      break;
    case "calculate":
      need(isBlank(node.a), "operandA");
      need(isBlank(node.b), "operandB");
      need(isBlank(node.variable), "calcResult");
      break;
    case "key_press":
      need(isBlank(node.key), "key");
      break;
    case "find_image":
      need(imageSourceIsBlank(node.image), "image");
      break;
    case "find_text_ocr":
      need(isBlank(node.text), "text");
      break;
    case "wait_for_window":
    case "focus_window":
      need(windowSelectorIsBlank(node.window), "window");
      break;
    case "launch_app":
      need(isBlank(node.path), "appPath");
      break;
    case "open_url":
    case "browser_navigate":
      need(isBlank(node.url), "url");
      break;
    case "notify":
    case "show_message":
      need(isBlank(node.title), "title");
      break;
    case "read_file":
      need(isBlank(node.path), "path");
      need(isBlank(node.variable), "fileContents");
      break;
    case "list_directory":
      need(isBlank(node.path), "path");
      need(isBlank(node.variable), "folderListing");
      break;
    case "write_file":
    case "delete_file":
    case "create_directory":
    case "wait_for_file":
    case "screenshot":
    case "browser_screenshot":
      need(isBlank(node.path), "path");
      break;
    case "copy_file":
    case "move_file":
      need(isBlank(node.source), "source");
      need(isBlank(node.destination), "destination");
      break;
    case "read_clipboard":
      need(isBlank(node.variable), "clipboardText");
      break;
    case "show_confirm":
      need(isBlank(node.title), "title");
      need(isBlank(node.variable), "confirmResult");
      break;
    case "show_input":
      need(isBlank(node.title), "title");
      need(isBlank(node.variable), "inputResult");
      break;
    case "get_date_time":
      need(isBlank(node.variable), "name");
      break;
    case "get_system_info":
      need(
        isBlank(node.hostname) &&
          isBlank(node.osVersion) &&
          isBlank(node.cpuPercent) &&
          isBlank(node.memoryPercent) &&
          isBlank(node.ipAddress),
        MISSING_SYSTEM_INFO_FIELD,
      );
      break;
    case "text_transform":
      need(isBlank(node.text), "text");
      need(isBlank(node.variable), "name");
      break;
    case "http":
      need(isBlank(node.url), "url");
      break;
    case "http_download":
      need(isBlank(node.url), "url");
      need(isBlank(node.path), "path");
      break;
    case "ping":
      need(isBlank(node.host), "host");
      break;
    case "dns_lookup":
      need(isBlank(node.hostname), "hostname");
      need(isBlank(node.variable), "name");
      break;
    case "get_env_var":
      need(isBlank(node.name), "name");
      need(isBlank(node.variable), "name");
      break;
    case "check_process":
      need(isBlank(node.name), "name");
      need(isBlank(node.variable), "name");
      break;
    case "kill_process":
      need(isBlank(node.name), "name");
      break;
    case "generate_random":
      need(isBlank(node.min), "randomMin");
      need(isBlank(node.max), "randomMax");
      need(isBlank(node.variable), "name");
      break;
    case "get_element_text":
      need(isBlank(node.variable), "elementText");
      need(isBlank(node.elementName) && isBlank(node.automationId), MISSING_ELEMENT_SELECTOR);
      break;
    case "browser_click":
    case "browser_wait_for_selector":
      need(selectorIsBlank(node.selector), "selector");
      break;
    case "browser_get_text":
      need(selectorIsBlank(node.selector), "selector");
      need(isBlank(node.variable), "name");
      break;
    case "browser_set_value":
      need(selectorIsBlank(node.selector), "selector");
      break;
    case "if":
      need(isBlank(node.condition.variable), "name");
      break;
    case "function_def":
      need(isBlank(node.name), "functionName");
      break;
    case "call_function":
      need(isBlank(node.name), "callFunctionName");
      break;
  }
  return missing;
}

/** Whether this node is missing a field it needs to actually run —
 *  see `nodeMissingFieldKeys`. Used to flag a placed-but-unfinished
 *  node on the canvas before the user ever presses run, the same way
 *  a form highlights an empty required input. */
export function nodeIsIncomplete(node: FlowNode): boolean {
  return nodeMissingFieldKeys(node).length > 0;
}

/** Builds the canvas node's title/sub/body for a live node, localized via `t`. */
export function describeNode(
  node: FlowNode,
  t: (key: string, opts?: Record<string, unknown>) => string,
  mode: NodeNameMode = "beginner",
) {
  const kindLabel = t(`inspector.kindLabel.${NODE_KIND_OF[node.kind]}`);
  switch (node.kind) {
    case "start":
      return { title: paletteLabel(t, "start", mode), sub: kindLabel, body: "" };
    case "error_handler":
      return { title: paletteLabel(t, "errorHandler", mode), sub: kindLabel, body: "" };
    case "wait":
      return { title: paletteLabel(t, "wait", mode), sub: `${kindLabel} · ${t("inspector.fields.seconds")}`, body: `${node.seconds}s` };
    case "set_variable":
      return { title: paletteLabel(t, "setVariable", mode), sub: kindLabel, body: `${node.name} = "${node.value}"` };
    case "calculate":
      return {
        title: paletteLabel(t, "calculate", mode),
        sub: kindLabel,
        body: `${node.variable} = ${node.a} ${CALC_OP_SYMBOL[node.op]} ${node.b}`,
      };
    case "type_text":
      return { title: paletteLabel(t, "typeText", mode), sub: kindLabel, body: `"${node.text}"` };
    case "click": {
      const buttonLabel = t(`inspector.fields.button${node.button.charAt(0).toUpperCase()}${node.button.slice(1)}`);
      const kindTag = node.clickKind === "double" ? "×2" : "";
      return { title: paletteLabel(t, "click", mode), sub: kindLabel, body: `${buttonLabel}${kindTag}` };
    }
    case "move_mouse": {
      const lastMatch = node.targetKind === "last_match";
      const where = lastMatch ? t("inspector.fields.targetKindLastMatch") : `(${node.x}, ${node.y})`;
      return { title: t(lastMatch ? "palette.items.moveToImage" : "palette.items.moveMouse"), sub: kindLabel, body: `${where} · ${node.durationMs}ms` };
    }
    case "key_press": {
      const combo = formatKeyCombo(node.key, node.modifiers);
      const modeTag = node.mode === "press" ? ` ↓ ${t("inspector.fields.keyModePress")}` : node.mode === "release" ? ` ↑ ${t("inspector.fields.keyModeRelease")}` : "";
      return { title: paletteLabel(t, "keyPress", mode), sub: kindLabel, body: `${combo}${modeTag}` };
    }
    case "find_image": {
      const imageLabel = node.image.kind === "embedded" ? t("inspector.fields.imageEmbedded") : node.image.value;
      const waiting = (node.retryMaxAttempts ?? 1) > 1;
      return { title: t(waiting ? "palette.items.waitForImage" : "palette.items.findImageAi"), sub: kindLabel, body: `${imageLabel} · ${node.mode}` };
    }
    case "find_text_ocr":
      return { title: paletteLabel(t, "findTextOcr", mode), sub: kindLabel, body: `"${node.text}"` };
    case "wait_for_window":
      return { title: paletteLabel(t, "waitForWindow", mode), sub: kindLabel, body: describeWindowSelector(node.window) };
    case "focus_window":
      return { title: paletteLabel(t, "focusWindow", mode), sub: kindLabel, body: describeWindowSelector(node.window) };
    case "power_action":
      return { title: paletteLabel(t, "powerAction", mode), sub: kindLabel, body: paletteLabel(t, node.mode, mode) };
    case "lock_workstation":
      return { title: paletteLabel(t, "lockWorkstation", mode), sub: kindLabel, body: "" };
    case "read_clipboard":
      return { title: paletteLabel(t, "readClipboard", mode), sub: kindLabel, body: `→ ${node.variable}` };
    case "write_clipboard":
      return { title: paletteLabel(t, "writeClipboard", mode), sub: kindLabel, body: node.text };
    case "show_message":
      return { title: paletteLabel(t, "showMessage", mode), sub: kindLabel, body: node.title };
    case "show_confirm":
      return { title: paletteLabel(t, "showConfirm", mode), sub: kindLabel, body: `${node.title} → ${node.variable}` };
    case "show_input":
      return { title: paletteLabel(t, "showInput", mode), sub: kindLabel, body: `${node.title} → ${node.variable}` };
    case "stop":
      return { title: paletteLabel(t, "stop", mode), sub: kindLabel, body: "" };
    case "break":
      return { title: paletteLabel(t, "break", mode), sub: kindLabel, body: "" };
    case "continue":
      return { title: paletteLabel(t, "continue", mode), sub: kindLabel, body: "" };
    case "return":
      return { title: paletteLabel(t, "return", mode), sub: kindLabel, body: "" };
    case "get_date_time":
      return { title: paletteLabel(t, "getDateTime", mode), sub: kindLabel, body: `→ ${node.variable}` };
    case "get_system_info": {
      const picked = [node.hostname, node.osVersion, node.cpuPercent, node.memoryPercent, node.ipAddress].filter(Boolean);
      return { title: paletteLabel(t, "getSystemInfo", mode), sub: kindLabel, body: picked.length > 0 ? `→ ${picked.join(", ")}` : "" };
    }
    case "text_transform":
      return { title: paletteLabel(t, textOpPaletteKey(node.op), mode), sub: kindLabel, body: `${node.op}("${node.text}") → ${node.variable}` };
    case "launch_app":
      return { title: paletteLabel(t, "launchApp", mode), sub: kindLabel, body: node.args ? `${node.path} ${node.args}` : node.path };
    case "open_url":
      return { title: paletteLabel(t, "openUrl", mode), sub: kindLabel, body: node.url };
    case "notify":
      return { title: paletteLabel(t, "notify", mode), sub: kindLabel, body: node.title };
    case "read_file":
      return { title: paletteLabel(t, "readFile", mode), sub: kindLabel, body: `${node.path} → ${node.variable}` };
    case "write_file":
      return { title: paletteLabel(t, "writeFile", mode), sub: kindLabel, body: node.path };
    case "copy_file":
      return { title: paletteLabel(t, "copyFile", mode), sub: kindLabel, body: `${node.source} → ${node.destination}` };
    case "move_file":
      return { title: paletteLabel(t, "moveFile", mode), sub: kindLabel, body: `${node.source} → ${node.destination}` };
    case "delete_file":
      return { title: paletteLabel(t, "deleteFile", mode), sub: kindLabel, body: node.path };
    case "create_directory":
      return { title: paletteLabel(t, "createDirectory", mode), sub: kindLabel, body: node.path };
    case "list_directory":
      return { title: paletteLabel(t, "listDirectory", mode), sub: kindLabel, body: `${node.path} → ${node.variable}` };
    case "http":
      return { title: paletteLabel(t, "http", mode), sub: kindLabel, body: `${node.method.toUpperCase()} ${node.url}` };
    case "http_download":
      return { title: paletteLabel(t, "httpDownload", mode), sub: kindLabel, body: `${node.url} → ${node.path}` };
    case "ping":
      return { title: paletteLabel(t, "ping", mode), sub: kindLabel, body: `${node.host} → ${node.variable}` };
    case "dns_lookup":
      return { title: paletteLabel(t, "dnsLookup", mode), sub: kindLabel, body: `${node.hostname} → ${node.variable}` };
    case "screenshot":
      return { title: paletteLabel(t, "screenshot", mode), sub: kindLabel, body: node.path };
    case "browser_screenshot":
      return { title: paletteLabel(t, "browserScreenshot", mode), sub: kindLabel, body: node.path };
    case "get_env_var":
      return { title: paletteLabel(t, "getEnvVar", mode), sub: kindLabel, body: `${node.name} → ${node.variable}` };
    case "check_process":
      return { title: paletteLabel(t, "checkProcess", mode), sub: kindLabel, body: `${node.name} → ${node.variable}` };
    case "kill_process":
      return { title: paletteLabel(t, "killProcess", mode), sub: kindLabel, body: node.name };
    case "wait_for_file":
      return { title: paletteLabel(t, "waitForFile", mode), sub: kindLabel, body: node.path };
    case "generate_random":
      return { title: paletteLabel(t, "generateRandom", mode), sub: kindLabel, body: `${node.min}–${node.max} → ${node.variable}` };
    case "get_element_text":
      return { title: paletteLabel(t, "getElementText", mode), sub: kindLabel, body: `${node.elementName || "?"} → ${node.variable}` };
    case "launch_browser":
      return { title: paletteLabel(t, "launchBrowser", mode), sub: kindLabel, body: `${node.url} → ${node.variable}` };
    case "browser_navigate":
      return { title: paletteLabel(t, "browserNavigate", mode), sub: kindLabel, body: node.url };
    case "browser_click":
      return { title: paletteLabel(t, "browserClick", mode), sub: kindLabel, body: describeBrowserSelector(node.selector) };
    case "browser_get_text":
      return {
        title: paletteLabel(t, "browserGetText", mode),
        sub: kindLabel,
        body: `${describeBrowserSelector(node.selector)} → ${node.variable}`,
      };
    case "browser_set_value":
      return {
        title: paletteLabel(t, "browserSetValue", mode),
        sub: kindLabel,
        body: `${describeBrowserSelector(node.selector)} = "${node.value}"`,
      };
    case "browser_wait_for_selector":
      return { title: paletteLabel(t, "browserWaitForSelector", mode), sub: kindLabel, body: describeBrowserSelector(node.selector) };
    case "if":
      return {
        title: paletteLabel(t, "ifElse", mode),
        sub: kindLabel,
        body: `IF ${node.condition.variable} == "${node.condition.equals}"`,
      };
    case "loop":
      return { title: paletteLabel(t, "loop", mode), sub: kindLabel, body: `× ${node.count}` };
    case "try_catch":
      return { title: paletteLabel(t, "tryCatch", mode), sub: kindLabel, body: "" };
    case "function_def":
      return { title: paletteLabel(t, "functionDef", mode), sub: kindLabel, body: node.name };
    case "call_function":
      return { title: paletteLabel(t, "callFunction", mode), sub: kindLabel, body: node.name };
  }
}

export * from "./flowGraph";
export * from "./flowYamlSerialize";
export * from "./flowYamlParse";
