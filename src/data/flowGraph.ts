/** Pure functions over the `Branch` graph — building fresh nodes,
 *  finding/cloning/updating/deleting them, and wiring/unwiring
 *  connections. No YAML here (see `flowYamlSerialize`/`flowYamlParse`)
 *  and no rendering (see `describeNode` in `flowModel`) — just the
 *  graph shape itself. See `flowModel.ts`'s top-of-file comment for
 *  the overall `Branch`/`Connection`/`if`-vs-`loop` model this all
 *  operates on. */
import {
  makeBrowserSelector,
  makeImageSource,
  makeKeyModifiers,
  type Branch,
  type BranchKind,
  type Connection,
  type FlowNode,
  type LeafKind,
} from "./flowModel";

export function emptyBranch(): Branch {
  return { steps: [], connections: [], entry: null };
}

/** A brand-new flow always has exactly one `start` node, already
 *  wired as the entry — used for "New Flow" so there's never a moment
 *  where a freshly created flow lacks one. */
export function newFlowBranch(): Branch {
  const id = makeLeaf("start").id;
  return { steps: [{ id, kind: "start", enabled: true }], connections: [], entry: id };
}

/** Guarantees the top-level flow has exactly one `start` node and
 *  that it — not anything else — is `entry`, repairing it if not:
 *  - No `start` node at all (a hand-edited file, or one saved before
 *    this was enforced): adds one and wires it ahead of whatever
 *    `entry` already pointed to, preserving the rest of the flow
 *    unchanged.
 *  - A `start` node exists but isn't `entry` (same situation, or a
 *    hand-edited file that pointed `entry` elsewhere): repoints
 *    `entry` at the first `start` node found, wiring it ahead of
 *    whatever `entry` used to be.
 *  Only ever touches the top-level branch — a loop/if/try_catch/
 *  function's own nested entry is a separate, unrelated mechanism.
 *  Returns `repaired: true` when it actually changed anything, so the
 *  caller can tell the user their file was fixed up. */
export function ensureStartNode(root: Branch): { branch: Branch; repaired: boolean } {
  const starts = root.steps.filter((n) => n.kind === "start");
  if (starts.length >= 1 && root.entry === starts[0].id) {
    return { branch: root, repaired: false };
  }

  let steps = root.steps;
  let connections = root.connections;
  let startId: string;

  if (starts.length === 0) {
    startId = makeLeaf("start").id;
    steps = [{ id: startId, kind: "start", enabled: true }, ...steps];
  } else {
    startId = starts[0].id;
  }

  if (root.entry && root.entry !== startId) {
    connections = [...connections.filter((c) => !(c.from === startId && c.fromPort === null)), { from: startId, fromPort: null, to: root.entry }];
  }

  return { branch: { steps, connections, entry: startId }, repaired: true };
}

export const INITIAL_FLOW: Branch = {
  steps: [
    { id: "flow_start", kind: "start", enabled: true },
    { id: "set_ready", kind: "set_variable", name: "ready", value: "yes", enabled: true },
    {
      id: "check_ready",
      kind: "if",
      condition: { variable: "ready", equals: "yes" },
      then: { steps: [{ id: "wait_ok", kind: "wait", seconds: 0.6, enabled: true }], connections: [], entry: "wait_ok" },
      otherwise: { steps: [{ id: "wait_fallback", kind: "wait", seconds: 0.6, enabled: true }], connections: [], entry: "wait_fallback" },
      enabled: true,
    },
    { id: "finish", kind: "wait", seconds: 0.3, enabled: true },
  ],
  connections: [
    { from: "flow_start", fromPort: null, to: "set_ready" },
    { from: "set_ready", fromPort: null, to: "check_ready" },
    { from: "check_ready", fromPort: null, to: "finish" },
  ],
  entry: "flow_start",
};

let counter = 0;

export function makeLeaf(kind: LeafKind): FlowNode {
  counter += 1;
  const id = `${kind}_${counter}`;
  switch (kind) {
    case "start":
      return { id, kind, enabled: true };
    case "error_handler":
      return { id, kind, enabled: true };
    case "wait":
      return { id, kind, seconds: 1, enabled: true };
    case "set_variable":
      return { id, kind, name: "my_var", value: "value", enabled: true };
    case "calculate":
      return { id, kind, a: "%my_var%", op: "add", b: "1", variable: "calc_result", enabled: true };
    case "type_text":
      return { id, kind, text: "Hello", enabled: true };
    case "click":
      return { id, kind, button: "left", clickKind: "single", enabled: true };
    case "move_mouse":
      return { id, kind, x: 100, y: 100, durationMs: 0, enabled: true };
    case "key_press":
      return { id, kind, key: "enter", mode: "tap", modifiers: makeKeyModifiers(), enabled: true };
    case "find_image":
      return { id, kind, image: makeImageSource("target.png"), mode: "similar", threshold: 0.85, minScale: 0.7, maxScale: 1.4, scaleSteps: 12, enabled: true };
    case "find_text_ocr":
      return { id, kind, text: "Sign in", enabled: true };
    case "wait_for_window":
      return { id, kind, windowTitle: "Untitled - Notepad", enabled: true };
    case "focus_window":
      return { id, kind, windowTitle: "Untitled - Notepad", enabled: true };
    case "power_action":
      return { id, kind, mode: "shutdown", force: false, enabled: true };
    case "lock_workstation":
      return { id, kind, enabled: true };
    case "read_clipboard":
      return { id, kind, variable: "clipboard_text", enabled: true };
    case "write_clipboard":
      return { id, kind, text: "", enabled: true };
    case "show_message":
      return { id, kind, title: "Relay", message: "", blocking: true, enabled: true };
    case "show_confirm":
      return { id, kind, title: "Relay", message: "", variable: "confirm_result", enabled: true };
    case "show_input":
      return { id, kind, title: "Relay", message: "", defaultValue: "", variable: "input_result", enabled: true };
    case "stop":
      return { id, kind, enabled: true };
    case "break":
      return { id, kind, enabled: true };
    case "continue":
      return { id, kind, enabled: true };
    case "return":
      return { id, kind, enabled: true };
    case "get_date_time":
      return { id, kind, format: "iso8601", variable: "date_time", enabled: true };
    case "get_system_info":
      return { id, kind, hostname: "sys_hostname", osVersion: "", cpuPercent: "", memoryPercent: "", ipAddress: "sys_ip_address", enabled: true };
    case "text_transform":
      return { id, kind, op: "uppercase", text: "", arg1: "", arg2: "", variable: "text_result", enabled: true };
    case "launch_app":
      return { id, kind, path: "notepad.exe", args: "", enabled: true };
    case "open_url":
      return { id, kind, url: "https://example.com", enabled: true };
    case "notify":
      return { id, kind, title: "Relay", message: "Flow finished", enabled: true };
    case "read_file":
      return { id, kind, path: "C:\\path\\to\\file.txt", variable: "file_contents", enabled: true };
    case "write_file":
      return { id, kind, path: "C:\\path\\to\\file.txt", content: "", append: false, enabled: true };
    case "copy_file":
      return { id, kind, source: "C:\\source.txt", destination: "C:\\destination.txt", enabled: true };
    case "move_file":
      return { id, kind, source: "C:\\source.txt", destination: "C:\\destination.txt", enabled: true };
    case "delete_file":
      return { id, kind, path: "C:\\path\\to\\file.txt", enabled: true };
    case "create_directory":
      return { id, kind, path: "C:\\path\\to\\folder", enabled: true };
    case "list_directory":
      return { id, kind, path: "C:\\path\\to\\folder", variable: "folder_entries", enabled: true };
    case "http":
      return {
        id,
        kind,
        method: "get",
        url: "https://example.com/api",
        headers: "",
        body: "",
        variable: "http_response",
        statusVariable: "http_status",
        enabled: true,
      };
    case "http_download":
      return {
        id,
        kind,
        url: "https://example.com/file.zip",
        headers: "",
        path: "C:\\path\\to\\file.zip",
        variable: "download_status",
        pathVariable: "download_path",
        enabled: true,
      };
    case "ping":
      return { id, kind, host: "example.com", timeoutMs: 2000, variable: "ping_result", enabled: true };
    case "dns_lookup":
      return { id, kind, hostname: "example.com", variable: "resolved_ip", enabled: true };
    case "screenshot":
      return { id, kind, path: "C:\\path\\to\\screenshot.png", enabled: true };
    case "browser_screenshot":
      return { id, kind, path: "C:\\path\\to\\screenshot.png", instance: "", enabled: true };
    case "get_env_var":
      return { id, kind, name: "PATH", variable: "env_value", enabled: true };
    case "check_process":
      return { id, kind, name: "notepad.exe", variable: "is_running", enabled: true };
    case "kill_process":
      return { id, kind, name: "notepad.exe", force: false, enabled: true };
    case "wait_for_file":
      return { id, kind, path: "C:\\path\\to\\file.txt", timeoutMs: 30000, enabled: true };
    case "generate_random":
      return { id, kind, min: "1", max: "100", variable: "random_value", enabled: true };
    case "get_element_text":
      return { id, kind, windowTitle: "", elementName: "", automationId: "", variable: "element_text", enabled: true };
    case "launch_browser":
      return { id, kind, url: "https://example.com", variable: "browser_tab", browser: "", profileDir: "", enabled: true };
    case "browser_navigate":
      return { id, kind, url: "https://example.com", instance: "", enabled: true };
    case "browser_click":
      return { id, kind, selector: makeBrowserSelector("#submit"), instance: "", enabled: true };
    case "browser_get_text":
      return { id, kind, selector: makeBrowserSelector("#result"), variable: "browser_text", instance: "", enabled: true };
    case "browser_set_value":
      return { id, kind, selector: makeBrowserSelector("#input"), value: "value", instance: "", enabled: true };
    case "browser_wait_for_selector":
      return { id, kind, selector: makeBrowserSelector("#result"), instance: "", enabled: true };
    case "call_function":
      return { id, kind, name: "", enabled: true };
  }
}

export function makeBranch(kind: BranchKind): FlowNode {
  counter += 1;
  const id = `${kind}_${counter}`;
  if (kind === "if") {
    return { id, kind: "if", condition: { variable: "my_var", equals: "value" }, then: emptyBranch(), otherwise: emptyBranch(), enabled: true };
  }
  if (kind === "try_catch") {
    return { id, kind: "try_catch", tryBranch: emptyBranch(), catch: emptyBranch(), enabled: true };
  }
  if (kind === "function_def") {
    return { id, kind: "function_def", name: "my_function", body: emptyBranch(), enabled: true };
  }
  return { id, kind: "loop", count: 3, body: emptyBranch(), enabled: true };
}

/** Every nested `Branch` a node owns — `loop`'s and `function_def`'s
 *  single `body`, `if`'s `then`+`otherwise`, or `try_catch`'s
 *  `tryBranch`+`catch` (in that order), empty for every leaf kind
 *  (including `call_function`, which only *references* a
 *  `function_def` by name — it owns no branch of its own). The one
 *  place that needs to know which branch kinds carry nested pools;
 *  every recursive graph function below is written against this
 *  instead of special-casing `loop`/`if`/`try_catch`/`function_def`
 *  separately. */
function childBranches(node: FlowNode): Branch[] {
  if (node.kind === "loop") return [node.body];
  if (node.kind === "if") return [node.then, node.otherwise];
  if (node.kind === "try_catch") return [node.tryBranch, node.catch];
  if (node.kind === "function_def") return [node.body];
  return [];
}

/** Rebuilds `node` with its child branches replaced by `branches`
 *  (same order `childBranches` returned them in) — the write-side
 *  counterpart used by every recursive function that needs to
 *  reconstruct a node after recursing into its children. */
function withChildBranches(node: FlowNode, branches: Branch[]): FlowNode {
  if (node.kind === "loop") return { ...node, body: branches[0] };
  if (node.kind === "if") return { ...node, then: branches[0], otherwise: branches[1] };
  if (node.kind === "try_catch") return { ...node, tryBranch: branches[0], catch: branches[1] };
  if (node.kind === "function_def") return { ...node, body: branches[0] };
  return node;
}

export function findNode(branch: Branch, id: string): FlowNode | null {
  for (const n of branch.steps) {
    if (n.id === id) return n;
    for (const child of childBranches(n)) {
      const found = findNode(child, id);
      if (found) return found;
    }
  }
  return null;
}

/** Every user-configurable step kind with a plain `variable: string`
 *  output field — `set_variable` is handled separately (its `name`
 *  field plays the same role but isn't called `variable`), and
 *  `http` additionally gets a `_status` companion (see
 *  `collectVariableNames`). */
const VARIABLE_OUTPUT_KINDS = new Set<FlowNode["kind"]>([
  "calculate",
  "read_file",
  "list_directory",
  "read_clipboard",
  "show_confirm",
  "show_input",
  "http",
  "http_download",
  "get_element_text",
  "launch_browser",
  "browser_get_text",
  "get_date_time",
  "text_transform",
  "dns_lookup",
  "get_env_var",
  "check_process",
  "generate_random",
]);

export interface VariableDefinition {
  name: string;
  sourceNodeIds: string[];
  automatic: boolean;
}

/** Variables a flow can write, together with the steps that create
 *  them. Fixed companion outputs are marked automatic so the UI can
 *  distinguish them from names the user can edit in an Inspector. */
export function collectVariableDefinitions(branch: Branch): VariableDefinition[] {
  const definitions = new Map<string, VariableDefinition>();

  function add(name: string, sourceNodeId: string, automatic = false) {
    if (!name) return;
    const current = definitions.get(name);
    if (current) {
      if (!current.sourceNodeIds.includes(sourceNodeId)) current.sourceNodeIds.push(sourceNodeId);
      current.automatic = current.automatic && automatic;
      return;
    }
    definitions.set(name, { name, sourceNodeIds: [sourceNodeId], automatic });
  }

  function walk(current: Branch) {
    for (const n of current.steps) {
      if (n.kind === "set_variable") add(n.name, n.id);
      else if (VARIABLE_OUTPUT_KINDS.has(n.kind) && "variable" in n) {
        if (n.variable) add(n.variable, n.id);
        if (n.kind === "http" && n.statusVariable) add(n.statusVariable, n.id);
        if (n.kind === "http_download" && n.pathVariable) add(n.pathVariable, n.id);
      } else if (n.kind === "get_system_info") {
        for (const name of [n.hostname, n.osVersion, n.cpuPercent, n.memoryPercent, n.ipAddress]) add(name, n.id);
      } else if (n.kind === "ping" && n.variable) {
        add(n.variable, n.id);
        add(`${n.variable}_latency_ms`, n.id, true);
      } else if (n.kind === "find_image") {
        add("last_match_found", n.id, true);
        add("last_match_x", n.id, true);
        add("last_match_y", n.id, true);
        add("last_match_score", n.id, true);
      } else if (n.kind === "try_catch") {
        add("caught_error", n.id, true);
      }
      for (const child of childBranches(n)) walk(child);
    }
  }

  walk(branch);
  return Array.from(definitions.values());
}

/** Every variable name any step in the graph could write. Feeds the
 *  variable insertion controls as well as the variables workspace. */
export function collectVariableNames(branch: Branch): string[] {
  return collectVariableDefinitions(branch).map(({ name }) => name);
}

/** Every `function_def` node's `name`, anywhere in the graph — feeds
 *  `call_function`'s name picker. Only top-level `function_def`
 *  nodes are actually callable at run time (`run_flow_with_backend`
 *  only scans `flow.steps`, not nested branches), but this still
 *  walks the whole tree so a `function_def` accidentally nested
 *  inside a loop/if/try_catch shows up in the picker too — better to
 *  let the user see and fix a name that won't resolve than to hide it
 *  silently. */
export function collectFunctionNames(branch: Branch, acc: Set<string> = new Set()): string[] {
  for (const n of branch.steps) {
    if (n.kind === "function_def" && n.name) acc.add(n.name);
    for (const child of childBranches(n)) collectFunctionNames(child, acc);
  }
  return Array.from(acc);
}

/** Step kinds worth calling out by name in the open-flow warning
 *  (see `App.tsx`'s `confirmDangerousFlow`) when present — running
 *  another program, deleting/overwriting a file, ending a process,
 *  reading a file/the clipboard/an environment variable that might
 *  hold something sensitive, making an HTTP request (including a
 *  POST — the read-then-send-it-out combination is exactly how a
 *  malicious flow would exfiltrate whatever it just read), moving a
 *  file, opening a URL, or shutting down/restarting. This list is
 *  illustrative, not the security boundary itself: `App.tsx` confirms
 *  *every* externally-opened flow once, regardless of what's in it,
 *  because any new action kind added later (or any combination this
 *  list doesn't happen to enumerate) would otherwise silently bypass
 *  a purely blacklist-based check. Deliberately not `write_file` with
 *  `append: true` — appending to a file the flow's own note-taking
 *  already knows about doesn't carry the same "could clobber
 *  something" risk delete/overwrite does. */
export function dangerousActionKinds(branch: Branch, acc: Set<FlowNode["kind"]> = new Set()): FlowNode["kind"][] {
  for (const n of branch.steps) {
    if (
      n.kind === "launch_app" ||
      n.kind === "delete_file" ||
      n.kind === "kill_process" ||
      n.kind === "power_action" ||
      n.kind === "http_download" ||
      n.kind === "get_env_var" ||
      n.kind === "read_file" ||
      n.kind === "read_clipboard" ||
      n.kind === "http" ||
      n.kind === "move_file" ||
      n.kind === "open_url" ||
      (n.kind === "write_file" && !n.append)
    ) {
      acc.add(n.kind);
    }
    for (const child of childBranches(n)) dangerousActionKinds(child, acc);
  }
  return Array.from(acc);
}

/** True if some *other* `function_def` node in the graph already
 *  claims `name` — two functions sharing a name would silently
 *  collide in `ctx.functions` (a plain map keyed by name) on the Rust
 *  side, with whichever one happens to come last in `flow.steps`
 *  quietly winning; the Inspector uses this to warn before that
 *  happens instead of after. */
export function isDuplicateFunctionName(branch: Branch, nodeId: string, name: string): boolean {
  if (!name) return false;
  function walk(b: Branch): boolean {
    for (const n of b.steps) {
      if (n.kind === "function_def" && n.id !== nodeId && n.name === name) return true;
      if (childBranches(n).some(walk)) return true;
    }
    return false;
  }
  return walk(branch);
}

/** The id of every `function_def` node whose name collides with
 *  another one, anywhere in the graph — the canvas-wide counterpart
 *  of `isDuplicateFunctionName` (which only answers for one node at a
 *  time). Used to flag every colliding node with the same visible
 *  warning badge `nodeIsIncomplete` uses, instead of leaving the
 *  collision discoverable only by opening each node's Inspector. */
export function duplicateFunctionDefIds(branch: Branch): Set<string> {
  const byName = new Map<string, string[]>();
  function walk(b: Branch) {
    for (const n of b.steps) {
      if (n.kind === "function_def" && n.name) {
        const ids = byName.get(n.name) ?? [];
        ids.push(n.id);
        byName.set(n.name, ids);
      }
      for (const child of childBranches(n)) walk(child);
    }
  }
  walk(branch);
  const duplicates = new Set<string>();
  for (const ids of byName.values()) {
    if (ids.length > 1) for (const id of ids) duplicates.add(id);
  }
  return duplicates;
}

/** Every id in the graph, including steps nested inside a loop body
 *  or an if's then/otherwise. */
export function allIds(branch: Branch, acc: string[] = []): string[] {
  for (const n of branch.steps) {
    acc.push(n.id);
    for (const child of childBranches(n)) allIds(child, acc);
  }
  return acc;
}

/** Every non-empty `comment` in the graph, keyed by step id — what
 *  `buildFlowYaml` writes into the top-level `comments:` map (see its
 *  doc comment on `FlowNode.comment` for why it isn't a field inside
 *  each step's own YAML). */
export function collectComments(branch: Branch, acc: Record<string, string> = {}): Record<string, string> {
  for (const n of branch.steps) {
    if (n.comment) acc[n.id] = n.comment;
    for (const child of childBranches(n)) collectComments(child, acc);
  }
  return acc;
}

/** Finds which node owns the specific child branch containing `id` —
 *  `null` if `id` isn't nested inside anything. Used for "make this
 *  the branch's start" and to locate the owner when disconnecting a
 *  branch's entry wire. `branchKey` distinguishes which of the
 *  owner's branches `id` actually lives in (`"body"` for a loop;
 *  `"then"`/`"otherwise"` for an if; `"try"`/`"catch"` for a
 *  try_catch) — a loop only has one, but the other two's pairs are
 *  otherwise indistinguishable from the id alone. */
export function findBranchOwner(branch: Branch, id: string): { ownerId: string; branchKey: BranchKey } | null {
  for (const n of branch.steps) {
    if (n.kind === "loop") {
      if (n.body.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "body" };
      const deeper = findBranchOwner(n.body, id);
      if (deeper) return deeper;
    } else if (n.kind === "if") {
      if (n.then.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "then" };
      if (n.otherwise.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "otherwise" };
      const deeper = findBranchOwner(n.then, id) ?? findBranchOwner(n.otherwise, id);
      if (deeper) return deeper;
    } else if (n.kind === "try_catch") {
      if (n.tryBranch.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "try" };
      if (n.catch.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "catch" };
      const deeper = findBranchOwner(n.tryBranch, id) ?? findBranchOwner(n.catch, id);
      if (deeper) return deeper;
    } else if (n.kind === "function_def") {
      if (n.body.steps.some((s) => s.id === id)) return { ownerId: n.id, branchKey: "body" };
      const deeper = findBranchOwner(n.body, id);
      if (deeper) return deeper;
    }
  }
  return null;
}

/** Clones a whole branch's step pool with fresh ids, remapping that
 *  branch's own `connections`/`entry` to the new ids so internal
 *  wiring survives the copy intact. */
function cloneBranch(branch: Branch): Branch {
  const idMap = new Map<string, string>();
  const steps = branch.steps.map((n) => {
    const cloned = cloneNodeWithNewId(n);
    idMap.set(n.id, cloned.id);
    return cloned;
  });
  const connections = branch.connections
    .filter((c) => idMap.has(c.from) && idMap.has(c.to))
    .map((c) => ({ from: idMap.get(c.from)!, fromPort: c.fromPort, to: idMap.get(c.to)! }));
  const entry = branch.entry && idMap.has(branch.entry) ? idMap.get(branch.entry)! : null;
  return { steps, connections, entry };
}

function cloneNodeWithNewId(node: FlowNode): FlowNode {
  counter += 1;
  const id = `${node.kind}_${counter}`;
  if (node.kind === "loop") return { ...node, id, body: cloneBranch(node.body) };
  if (node.kind === "if") return { ...node, id, then: cloneBranch(node.then), otherwise: cloneBranch(node.otherwise) };
  if (node.kind === "try_catch") return { ...node, id, tryBranch: cloneBranch(node.tryBranch), catch: cloneBranch(node.catch) };
  if (node.kind === "function_def") {
    // A cloned function keeps its own body, but not its own `name` —
    // two `function_def` nodes sharing a name would silently collide
    // in `ctx.functions` (a plain map keyed by name) on the Rust
    // side, with whichever one happens to come last in `flow.steps`
    // quietly winning. `_copy` matches the id's own `_N` counter
    // suffix, so it reads as "a fresh, distinct one" the same way the
    // id does.
    return { ...node, id, name: `${node.name}_copy`, body: cloneBranch(node.body) };
  }
  return { ...node, id };
}

/** Deep-clones nodes (including a loop's nested body) with fresh ids —
 *  used for copy/paste and duplicate, so pasted steps never collide
 *  with the originals. A loop's own internal body wiring is
 *  preserved; like a fresh palette drop, the clones land unconnected
 *  to anything outside themselves — there's no wiring between
 *  separately-selected top-level nodes to preserve in the first
 *  place. */
export function cloneNodes(nodes: FlowNode[]): FlowNode[] {
  return nodes.map(cloneNodeWithNewId);
}

export function updateNode(branch: Branch, id: string, updater: (n: FlowNode) => FlowNode): Branch {
  return {
    ...branch,
    steps: branch.steps.map((n) =>
      n.id === id ? updater(n) : withChildBranches(n, childBranches(n).map((child) => updateNode(child, id, updater))),
    ),
  };
}

/** Removes `id` (and its whole subtree, if it's a loop/if) from
 *  wherever it lives, drops every connection that touched it, and
 *  clears any `entry` that pointed at it. Nothing else moves or gets
 *  reattached — a deleted node's neighbors are simply left unwired. */
export function deleteNode(branch: Branch, id: string): Branch {
  const steps = branch.steps
    .filter((n) => n.id !== id)
    .map((n) => withChildBranches(n, childBranches(n).map((child) => deleteNode(child, id))));
  const connections = branch.connections.filter((c) => c.from !== id && c.to !== id);
  const entry = branch.entry === id ? null : branch.entry;
  return { steps, connections, entry };
}

/** Finds the container (top-level flow, a loop's body, or an if's
 *  then/otherwise) whose step pool directly holds `id`. */
export function findContainer(branch: Branch, id: string): Branch | null {
  if (branch.steps.some((n) => n.id === id)) return branch;
  for (const n of branch.steps) {
    for (const child of childBranches(n)) {
      const found = findContainer(child, id);
      if (found) return found;
    }
  }
  return null;
}

function updateContainer(branch: Branch, anchorId: string, fn: (b: Branch) => Branch): Branch {
  if (branch.steps.some((n) => n.id === anchorId)) return fn(branch);
  return {
    ...branch,
    steps: branch.steps.map((n) => withChildBranches(n, childBranches(n).map((child) => updateContainer(child, anchorId, fn)))),
  };
}

/** Drops `newNode` into the container holding `anchorId` (or the
 *  top-level root if `anchorId` is null/not found) — completely
 *  unconnected, exactly like a new n8n node before it's wired to
 *  anything. If the container was empty, the new node becomes its
 *  entry so a never-touched flow still does something when run. */
export function addStep(root: Branch, anchorId: string | null, newNode: FlowNode): Branch {
  const drop = (b: Branch): Branch => ({ ...b, steps: [...b.steps, newNode], entry: b.steps.length === 0 ? newNode.id : b.entry });
  if (anchorId && findContainer(root, anchorId)) return updateContainer(root, anchorId, drop);
  return drop(root);
}

/** Which of `ownerId`'s nested branches to operate on — `"body"` for
 *  a loop (it only has one); `"then"`/`"otherwise"` for an if;
 *  `"try"`/`"catch"` for a try_catch. */
export type BranchKey = "body" | "then" | "otherwise" | "try" | "catch";

function readBranch(node: FlowNode, key: BranchKey): Branch | null {
  if (node.kind === "loop" && key === "body") return node.body;
  if (node.kind === "if" && key === "then") return node.then;
  if (node.kind === "if" && key === "otherwise") return node.otherwise;
  if (node.kind === "try_catch" && key === "try") return node.tryBranch;
  if (node.kind === "try_catch" && key === "catch") return node.catch;
  if (node.kind === "function_def" && key === "body") return node.body;
  return null;
}

function writeBranch(node: FlowNode, key: BranchKey, branch: Branch): FlowNode {
  if (node.kind === "loop" && key === "body") return { ...node, body: branch };
  if (node.kind === "if" && key === "then") return { ...node, then: branch };
  if (node.kind === "if" && key === "otherwise") return { ...node, otherwise: branch };
  if (node.kind === "try_catch" && key === "try") return { ...node, tryBranch: branch };
  if (node.kind === "try_catch" && key === "catch") return { ...node, catch: branch };
  if (node.kind === "function_def" && key === "body") return { ...node, body: branch };
  return node;
}

/** Drops `newNode`, unconnected, into `ownerId`'s `branchKey` branch
 *  (a loop's body, or an if's then/otherwise). */
export function addIntoBranch(root: Branch, ownerId: string, branchKey: BranchKey, newNode: FlowNode): Branch {
  return updateNode(root, ownerId, (n) => {
    const branch = readBranch(n, branchKey);
    if (!branch) return n;
    return writeBranch(n, branchKey, { ...branch, steps: [...branch.steps, newNode], entry: branch.steps.length === 0 ? newNode.id : branch.entry });
  });
}

/** True if `startId` can already reach `targetId` by following zero or
 *  more existing connections (any port). Used to refuse a new wire
 *  that would close a loop — the engine has no concept of "run this
 *  chain again" outside a `loop` node's body, so any other cycle
 *  would just run forever. */
function canReach(connections: Connection[], startId: string, targetId: string, seen: Set<string> = new Set()): boolean {
  if (startId === targetId) return true;
  if (seen.has(startId)) return false;
  seen.add(startId);
  for (const c of connections) {
    if (c.from === startId && canReach(connections, c.to, targetId, seen)) return true;
  }
  return false;
}

/** True if `id` is `node` itself or lives somewhere inside one of its
 *  nested branches — the containment check `connect` needs before
 *  moving a node into a branch, since moving a node into a branch
 *  that's nested inside *that same node* would create a cycle in the
 *  tree structure itself (not just in `connections`). */
function nodeContains(node: FlowNode, id: string): boolean {
  if (node.id === id) return true;
  return childBranches(node).some((child) => findNode(child, id) !== null);
}

/** Wires `fromId`'s plain output (`fromPort` is always `null` now —
 *  every step has exactly one unnamed output; a `loop`'s body and an
 *  `if`'s then/otherwise are set via `setBranchEntry` instead, not
 *  dragged) to `toId`. Replaces whatever that output was previously
 *  wired to. Refuses a wire that would close a cycle back to `fromId`
 *  — nothing here would ever terminate that.
 *
 *  If `toId` currently lives in a different container than `fromId`
 *  (dragging a wire across a branch boundary — e.g. from a step
 *  inside a loop's body out to a step sitting at the top level),
 *  `toId`'s node is moved into `fromId`'s container first, same as
 *  every other node's placement being driven entirely by connections,
 *  not by which visual group it happened to be dropped into. Refuses
 *  the move (leaving the flow unchanged) if that would nest `toId`
 *  inside its own branch. */
export function connect(root: Branch, fromId: string, fromPort: string | null, toId: string): Branch {
  const container = findContainer(root, fromId);
  if (!container || fromId === toId) return root;
  if (canReach(container.connections, toId, fromId)) return root;

  if (container.steps.some((n) => n.id === toId)) {
    return updateContainer(root, fromId, (b) => ({
      ...b,
      connections: [...b.connections.filter((c) => !(c.from === fromId && c.fromPort === fromPort)), { from: fromId, fromPort, to: toId }],
    }));
  }

  const targetNode = findNode(root, toId);
  if (!targetNode || nodeContains(targetNode, fromId)) return root;
  const withoutTarget = deleteNode(root, toId);
  return updateContainer(withoutTarget, fromId, (b) => ({
    ...b,
    steps: [...b.steps, targetNode],
    entry: b.steps.length === 0 ? targetNode.id : b.entry,
    connections: [...b.connections.filter((c) => !(c.from === fromId && c.fromPort === fromPort)), { from: fromId, fromPort, to: toId }],
  }));
}

/** Cuts exactly the one wire leaving `fromId`'s `fromPort` output.
 *  Both steps stay exactly where they are — nothing else in the flow
 *  changes. */
export function disconnect(root: Branch, fromId: string, fromPort: string | null): Branch {
  return updateContainer(root, fromId, (b) => ({
    ...b,
    connections: b.connections.filter((c) => !(c.from === fromId && c.fromPort === fromPort)),
  }));
}

/** Sets which step `ownerId`'s `branchKey` branch starts from
 *  (`entryId`), or clears it (`null`). `entryId` must already be a
 *  step inside that specific branch. */
export function setBranchEntry(root: Branch, ownerId: string, branchKey: BranchKey, entryId: string | null): Branch {
  return updateNode(root, ownerId, (n) => {
    const branch = readBranch(n, branchKey);
    if (!branch) return n;
    if (entryId !== null && !branch.steps.some((s) => s.id === entryId)) return n;
    return writeBranch(n, branchKey, { ...branch, entry: entryId });
  });
}

/** Dragging a wire from a branch anchor (a loop's body dot, an if's
 *  yes/no, a try_catch's try/catch, a function's body) onto `toId` —
 *  the drag-and-drop counterpart of `setBranchEntry`, for when `toId`
 *  isn't already sitting inside that branch. Moves `toId`'s node into
 *  the branch (out of wherever it currently lives, same as `connect`
 *  moving a node across a container boundary) and sets it as the
 *  branch's entry. Refuses (leaving the flow unchanged) if that would
 *  nest the branch's owner inside its own branch. */
export function connectBranchEntry(root: Branch, ownerId: string, branchKey: BranchKey, toId: string): Branch {
  if (ownerId === toId) return root;
  const targetNode = findNode(root, toId);
  if (!targetNode || nodeContains(targetNode, ownerId)) return root;

  const ownerNode = findNode(root, ownerId);
  const branch = ownerNode ? readBranch(ownerNode, branchKey) : null;
  if (!branch) return root;

  if (branch.steps.some((s) => s.id === toId)) {
    return setBranchEntry(root, ownerId, branchKey, toId);
  }

  const withoutTarget = deleteNode(root, toId);
  return updateNode(withoutTarget, ownerId, (n) => {
    const b = readBranch(n, branchKey);
    if (!b) return n;
    return writeBranch(n, branchKey, { ...b, steps: [...b.steps, targetNode], entry: toId });
  });
}

/** Raises the fresh-id counter (used by makeLeaf/makeBranch/clone) so
 *  new nodes never collide with ids loaded from a file saved in an
 *  earlier session, which may already contain ids like `click_9`. */
export function bumpCounterPast(ids: string[]) {
  for (const id of ids) {
    const match = /_(\d+)$/.exec(id);
    if (match) counter = Math.max(counter, Number(match[1]));
  }
}
