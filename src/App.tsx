import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm as confirmDialog } from "@tauri-apps/plugin-dialog";
import { check as checkForUpdate } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useTranslation } from "react-i18next";
import { Toolbar, type EditorMode } from "./components/Toolbar";
import { Palette } from "./components/Palette";
import { HomeScreen } from "./components/HomeScreen";
import { Canvas, computeAutoPositions, type DisconnectPayload, type EdgeStyle, type NodePositions, type Orientation } from "./components/Canvas";
import { CodeView } from "./components/CodeView";
import { VariablesPanel } from "./components/VariablesPanel";
import { Inspector } from "./components/Inspector";
import { StatusBar, type RunState, type LogEntry } from "./components/StatusBar";
import { LogViewer } from "./components/LogViewer";
import { RegionPickerHost } from "./components/RegionPickerHost";
import {
  addIntoBranch,
  addStep,
  allIds,
  buildFlowYaml,
  cloneNodes,
  connect,
  connectBranchEntry,
  dangerousActionKinds,
  deleteNode,
  disconnect,
  ensureStartNode,
  newFlowBranch,
  findNode,
  makeBranch,
  makeLeaf,
  parseFlowYaml,
  setBranchEntry,
  updateNode,
  type Branch,
  type BranchKey,
  type FlowNode,
  type NodeNameMode,
} from "./data/flowModel";
import {
  fileStem,
  listRecentFlows,
  pickFlowFileToOpen,
  pickFlowSavePath,
  readFlowFile,
  rememberRecentFlow,
  writeFlowFile,
  type RecentFile,
} from "./data/fileOps";
import { hashFlowText, isFlowAcknowledged, rememberFlowAcknowledged } from "./data/flowTrust";

type StepEvent =
  | { phase: "start"; step_id: string }
  | { phase: "success"; step_id: string }
  | { phase: "error"; step_id: string; message: string }
  | { phase: "monitor_mismatch" }
  | { phase: "monitor_restored" }
  | { phase: "paused"; step_id: string }
  | { phase: "resumed" };

function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
}

/** True when the user has plain text selected anywhere on the page
 *  (the YAML code view, a node's body text, ...) — Ctrl+C/X should
 *  copy/cut *that* natively instead of being hijacked into "copy/cut
 *  the selected canvas nodes" just because focus isn't inside an
 *  input. */
function hasTextSelection(): boolean {
  const selection = window.getSelection();
  return selection !== null && selection.toString().length > 0;
}

/** Drops a run of already-cloned nodes into the same container as
 *  `anchorId`, each completely unconnected — the paste/duplicate
 *  equivalent of a fresh palette drop. */
function dropMany(branch: Branch, anchorId: string | null, toInsert: FlowNode[]): Branch {
  return toInsert.reduce((acc, node) => addStep(acc, anchorId, node), branch);
}

const HISTORY_LIMIT = 50;

type View = "home" | "editor";

function App() {
  const [view, setView] = useState<View>("home");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [fileName, setFileName] = useState<string>("");
  const [isDirty, setIsDirty] = useState(false);
  const [mode, setMode] = useState<EditorMode>("flow");
  const [flow, setFlow] = useState<Branch>(newFlowBranch());
  const [history, setHistory] = useState<Branch[]>([]);
  const [future, setFuture] = useState<Branch[]>([]);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [clipboard, setClipboard] = useState<FlowNode[]>([]);
  const [runState, setRunState] = useState<RunState>({ status: "idle" });
  const [liveStatus, setLiveStatus] = useState<Record<string, string>>({});
  const [liveVariables, setLiveVariables] = useState<Record<string, string>>({});
  const [hasRun, setHasRun] = useState(false);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [zoomPercent, setZoomPercent] = useState(100);
  const [edgeStyle, setEdgeStyle] = useState<EdgeStyle>(
    () => (localStorage.getItem("relay.edgeStyle") as EdgeStyle | null) ?? "straight",
  );
  const [orientation, setOrientation] = useState<Orientation>(
    () => (localStorage.getItem("relay.orientation") as Orientation | null) ?? "horizontal",
  );
  const [nodeNameMode, setNodeNameMode] = useState<NodeNameMode>(
    () => (localStorage.getItem("relay.nodeNameMode") as NodeNameMode | null) ?? "beginner",
  );
  const [positions, setPositions] = useState<NodePositions>({});
  const [stepDelayMs, setStepDelayMs] = useState(0);
  const [recentFlows, setRecentFlows] = useState<RecentFile[]>([]);
  const [isElevated, setIsElevated] = useState(true);
  const [monitorPaused, setMonitorPaused] = useState(false);
  const [showLogViewer, setShowLogViewer] = useState(false);
  const [pausedStepId, setPausedStepId] = useState<string | null>(null);
  const { t } = useTranslation();

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    listRecentFlows().then(setRecentFlows).catch(() => {});
  }, [view]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    invoke<boolean>("is_elevated").then(setIsElevated).catch(() => {});
  }, []);

  // `onCloseRequested`'s callback is registered once below and would
  // otherwise close over whatever `isDirty` was at that moment — this
  // ref keeps it reading the current value instead.
  const isDirtyRef = useRef(isDirty);
  useEffect(() => {
    isDirtyRef.current = isDirty;
  }, [isDirty]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    // `window.confirm` blocking the whole close-requested callback
    // hung the window entirely (unresponsive to both the titlebar's
    // close button and Task Manager's End Task) — WebView2 doesn't
    // reliably support a synchronous confirm dialog from inside an
    // async Tauri event callback. `preventDefault` unconditionally
    // first, then ask asynchronously via the dialog plugin (already
    // used elsewhere for file pickers) and close for real only after
    // the user actually confirms.
    const unlistenPromise = getCurrentWindow().onCloseRequested(async (event) => {
      // Always prevent the default close and decide explicitly via
      // `destroy()` instead of conditionally letting it fall through —
      // relying on "not calling preventDefault" to mean "close as
      // normal" turned out to not reliably close the window at all.
      event.preventDefault();
      if (isDirtyRef.current) {
        const discard = await confirmDialog(t("home.discardChangesConfirm"), { kind: "warning" });
        if (!discard) return;
      }
      await getCurrentWindow()
        .destroy()
        .catch((err) => console.error("failed to close window:", err));
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [t]);

  // Canvas display preferences, not flow content — kept across
  // restarts (and separate from any saved .relay file) since they're
  // about how the editor looks, not what the flow does.
  useEffect(() => {
    localStorage.setItem("relay.edgeStyle", edgeStyle);
  }, [edgeStyle]);
  useEffect(() => {
    localStorage.setItem("relay.orientation", orientation);
  }, [orientation]);
  useEffect(() => {
    localStorage.setItem("relay.nodeNameMode", nodeNameMode);
  }, [nodeNameMode]);

  /** Windows' UIPI blocks a non-elevated process from sending input to
   *  an elevated one's windows at all — clicks/keystrokes silently do
   *  nothing rather than erroring, so this is the one way the user
   *  finds out *why* before spending time debugging a flow that looks
   *  broken. Confirmed in chat rather than a plain click since it
   *  relaunches the whole app (Windows' own UAC prompt is the actual
   *  gate, but asking first avoids surprising the user with it). */
  async function handleRelaunchAsAdmin() {
    if (!window.confirm(t("menubar.relaunchAsAdminConfirm"))) return;
    try {
      await invoke("relaunch_as_admin");
    } catch (error) {
      window.alert(t("menubar.relaunchAsAdminFailed", { message: String(error) }));
    }
  }

  async function handleCheckForUpdate() {
    try {
      const update = await checkForUpdate();
      if (!update) {
        window.alert(t("menubar.upToDate"));
        return;
      }
      const proceed = await confirmDialog(
        t("menubar.updateAvailable", { version: update.version }),
        { kind: "info" },
      );
      if (!proceed) return;
      await update.downloadAndInstall();
      await relaunch();
    } catch (error) {
      window.alert(t("menubar.updateFailed", { message: String(error) }));
    }
  }

  const flowYaml = useMemo(
    () => buildFlowYaml(flow, fileName || t("home.untitled"), positions, stepDelayMs),
    [flow, fileName, positions, stepDelayMs, t],
  );

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unlisten = listen<StepEvent>("flow-step", (event) => {
      const { payload } = event;
      if (payload.phase === "start") {
        setLiveStatus((prev) => ({ ...prev, [payload.step_id]: "running" }));
        const time = Date.now();
        setLog((prev) => [...prev, { key: `${payload.step_id}-start-${time}`, kind: "start", stepId: payload.step_id, time }]);
      } else if (payload.phase === "success") {
        setLiveStatus((prev) => ({ ...prev, [payload.step_id]: "done" }));
        const time = Date.now();
        setLog((prev) => [...prev, { key: `${payload.step_id}-done-${time}`, kind: "done", stepId: payload.step_id, time }]);
      } else if (payload.phase === "error") {
        setLiveStatus((prev) => ({ ...prev, [payload.step_id]: "error" }));
        const time = Date.now();
        setLog((prev) => [
          ...prev,
          {
            key: `${payload.step_id}-error-${time}`,
            kind: "error",
            stepId: payload.step_id,
            message: payload.message,
            time,
          },
        ]);
      } else if (payload.phase === "monitor_mismatch") {
        setMonitorPaused(true);
        const time = Date.now();
        setLog((prev) => [...prev, { key: `monitor-mismatch-${time}`, kind: "monitor-mismatch", time }]);
      } else if (payload.phase === "monitor_restored") {
        setMonitorPaused(false);
        const time = Date.now();
        setLog((prev) => [...prev, { key: `monitor-restored-${time}`, kind: "monitor-restored", time }]);
      } else if (payload.phase === "paused") {
        setPausedStepId(payload.step_id);
        setLiveStatus((prev) => ({ ...prev, [payload.step_id]: "paused" }));
        const time = Date.now();
        setLog((prev) => [...prev, { key: `${payload.step_id}-paused-${time}`, kind: "paused", stepId: payload.step_id, time }]);
      } else if (payload.phase === "resumed") {
        setPausedStepId(null);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unlisten = listen<Record<string, string>>("flow-variables", (event) => {
      setLiveVariables(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (runState.status !== "running") return;
    const interval = setInterval(() => {
      if (startedAt !== null) setElapsedMs(Date.now() - startedAt);
    }, 100);
    return () => clearInterval(interval);
  }, [runState.status, startedAt]);

  const primaryId = selectedIds.length > 0 ? selectedIds[selectedIds.length - 1] : null;

  function applyFlow(updater: (prev: Branch) => Branch) {
    const next = updater(flow);
    setHistory((h) => [...h.slice(-(HISTORY_LIMIT - 1)), flow]);
    setFuture([]);
    setFlow(next);
    setIsDirty(true);
  }

  function confirmDiscardIfDirty(): boolean {
    if (!isDirty) return true;
    return window.confirm(t("home.discardChangesConfirm"));
  }

  function resetEditorState(nextFlow: Branch, nextPositions: NodePositions = {}, nextStepDelayMs = 0) {
    setFlow(nextFlow);
    setHistory([]);
    setFuture([]);
    setSelectedIds([]);
    setPositions(nextPositions);
    setStepDelayMs(nextStepDelayMs);
    setLiveStatus({});
    setLiveVariables({});
    setHasRun(false);
    setLog([]);
    setRunState({ status: "idle" });
    setMonitorPaused(false);
    setPausedStepId(null);
    setIsDirty(false);
  }

  function handleNewFlow() {
    if (!confirmDiscardIfDirty()) return;
    setFilePath(null);
    setFileName(t("home.untitled"));
    resetEditorState(newFlowBranch());
    setView("editor");
  }

  /** One-time heads-up before opening a flow that can affect the
   *  machine outside the flow itself (see `dangerousActionKinds`'s
   *  doc comment) — asked once per distinct flow *content*, then
   *  remembered, so re-opening the same unmodified file never nags
   *  again. Mirrors Office's "trusted document" model rather than
   *  PAD/UiPath's convention of no per-action confirmation at all —
   *  this only ever runs once at open time, never during a run. */
  async function confirmDangerousFlow(yaml: string, flow: Branch): Promise<boolean> {
    const kinds = dangerousActionKinds(flow);
    if (kinds.length === 0) return true;
    const hash = await hashFlowText(yaml);
    if (isFlowAcknowledged(hash)) return true;
    const actionLabels = kinds
      .map((kind) => kind.replace(/_([a-z])/g, (_m, c: string) => c.toUpperCase()))
      .map((key) => t(`palette.items.${key}`));
    const proceed = await confirmDialog(t("home.dangerousFlowWarning", { actions: actionLabels.join(t("home.dangerousFlowSeparator")) }), {
      kind: "warning",
    });
    if (proceed) rememberFlowAcknowledged(hash);
    return proceed;
  }

  async function loadFlowFromPath(path: string) {
    const text = await readFlowFile(path);
    const { flow: loaded, positions: loadedPositions, stepDelayMs: loadedStepDelayMs } = parseFlowYaml(text);
    if (!(await confirmDangerousFlow(text, loaded))) return;
    const { branch: repairedFlow, repaired } = ensureStartNode(loaded);
    setFilePath(path);
    setFileName(fileStem(path));
    resetEditorState(repairedFlow, loadedPositions, loadedStepDelayMs);
    setView("editor");
    if (repaired) {
      setIsDirty(true);
      window.alert(t("home.startNodeRepaired"));
    }
    await rememberRecentFlow(path).catch(() => {});
  }

  async function handleOpenFlow() {
    if (!confirmDiscardIfDirty()) return;
    const path = await pickFlowFileToOpen();
    if (!path) return;
    try {
      await loadFlowFromPath(path);
    } catch (error) {
      window.alert(t("home.openFailed", { message: String(error) }));
    }
  }

  async function handleOpenRecentFlow(path: string) {
    if (!confirmDiscardIfDirty()) return;
    try {
      await loadFlowFromPath(path);
    } catch (error) {
      window.alert(t("home.openFailed", { message: String(error) }));
    }
  }

  async function handleSave() {
    if (!filePath) {
      await handleSaveAs();
      return;
    }
    try {
      await writeFlowFile(filePath, flowYaml);
      setIsDirty(false);
    } catch (error) {
      window.alert(t("home.saveFailed", { message: String(error) }));
    }
  }

  async function handleSaveAs() {
    const path = await pickFlowSavePath(fileName || t("home.untitled"));
    if (!path) return;
    try {
      await writeFlowFile(path, flowYaml);
      setFilePath(path);
      setFileName(fileStem(path));
      setIsDirty(false);
      await rememberRecentFlow(path).catch(() => {});
    } catch (error) {
      window.alert(t("home.saveFailed", { message: String(error) }));
    }
  }

  function handleBackToHome() {
    if (!confirmDiscardIfDirty()) return;
    setView("home");
  }

  /** Drops a fresh node into the canvas, completely unconnected — the
   *  Scratch/n8n-style "add a block" gesture. It lands in whichever
   *  container the currently-selected step lives in (or top-level if
   *  nothing's selected); the user wires it up explicitly afterward. */
  function handleAddStep(kind: FlowNode["kind"], preset?: Partial<FlowNode>) {
    // "start" isn't reachable through this — every flow already has
    // exactly one, created once by `newFlowBranch`/`ensureStartNode`
    // and never deletable, so there's never a moment where adding
    // another one is a meaningful UI action (see `ensureStartNode`'s
    // doc comment).
    const node = kind === "if" || kind === "loop" || kind === "try_catch" || kind === "function_def" ? makeBranch(kind) : makeLeaf(kind);
    const withPreset = preset ? ({ ...node, ...preset } as FlowNode) : node;
    applyFlow((prev) => addStep(prev, primaryId, withPreset));
    setSelectedIds([withPreset.id]);
  }

  /** Drops a fresh node into a `loop`'s body or an `if`'s then/otherwise
   *  from that node's context menu ("ループの中に追加" / "「はい」の中に
   *  追加" / "「いいえ」の中に追加") — unconnected, but already living
   *  inside the right branch instead of the enclosing container. */
  function handleAddIntoBranch(ownerId: string, branchKey: BranchKey, kind: FlowNode["kind"]) {
    const node = kind === "if" || kind === "loop" || kind === "try_catch" || kind === "function_def" ? makeBranch(kind) : makeLeaf(kind);
    applyFlow((prev) => addIntoBranch(prev, ownerId, branchKey, node));
    setSelectedIds([node.id]);
  }

  function handleUpdateStep(stepId: string, updater: (node: FlowNode) => FlowNode) {
    applyFlow((prev) => updateNode(prev, stepId, updater));
  }

  function handleDeleteStep(stepId: string) {
    handleDeleteSteps([stepId]);
  }

  /** The flow's `start` node is never deletable — every flow always
   *  has exactly one, and always runs from it (see `ensureStartNode`).
   *  Silently drops it from a multi-select delete rather than
   *  refusing the whole batch, so deleting "everything selected"
   *  still clears out everything else. */
  function handleDeleteSteps(stepIds: string[]) {
    const deletable = stepIds.filter((id) => findNode(flow, id)?.kind !== "start");
    const ids = new Set(deletable);
    applyFlow((prev) => deletable.reduce((acc, id) => deleteNode(acc, id), prev));
    setSelectedIds((current) => current.filter((id) => !ids.has(id)));
  }

  /** The "temporarily detach a piece" the user actually wants: the step
   *  stays exactly where it is in the tree — nothing moves or
   *  reconnects — but the engine skips it entirely at run time. Toggle
   *  back on to resume running it in place. */
  function handleToggleEnabled(stepId: string) {
    applyFlow((prev) => updateNode(prev, stepId, (n) => ({ ...n, enabled: !n.enabled })));
  }

  /** Step-through debugging: marks/unmarks a step so a plain "実行" run
   *  pauses right before it instead of running straight through. */
  function handleToggleBreakpoint(stepId: string) {
    applyFlow((prev) => updateNode(prev, stepId, (n) => ({ ...n, breakpoint: !n.breakpoint })));
  }

  function handlePositionChange(id: string, position: { x: number; y: number }) {
    setPositions((prev) => ({ ...prev, [id]: position }));
  }

  /** "整列" — snapshots the algorithmic tidy layout into the free-placement
   *  positions, overwriting any manual dragging. The result stays freely
   *  draggable afterward; this is a one-shot cleanup, not a locked mode. */
  function handleArrange() {
    setPositions(computeAutoPositions(flow, t, orientation));
  }

  /** Drags a wire from `sourceId`'s plain output onto `targetId`. If
   *  `targetId` isn't already in `sourceId`'s container, it's moved in
   *  first — connecting across a branch boundary works the same as
   *  connecting within one, instead of silently doing nothing. Only
   *  ever adds one `Connection`, replacing whatever that output was
   *  previously wired to. */
  function handleConnect(sourceId: string, targetId: string) {
    applyFlow((prev) => connect(prev, sourceId, null, targetId));
  }

  /** Drags a wire from a branch anchor (a loop's body dot, an if's
   *  yes/no, a try_catch's try/catch, a function's body) onto
   *  `targetId` — sets that branch's entry to `targetId`, moving it
   *  into the branch first if it wasn't already there. */
  function handleConnectBranchEntry(ownerId: string, branchKey: BranchKey, targetId: string) {
    applyFlow((prev) => connectBranchEntry(prev, ownerId, branchKey, targetId));
  }

  /** Right-click a wire → "切断": removes exactly that one connection
   *  (or clears a loop's/if's derived "this branch starts here" wire).
   *  Both steps stay exactly where they are — nothing else in the flow
   *  changes, and nothing gets silently reattached anywhere. */
  function handleDisconnect(payload: DisconnectPayload) {
    applyFlow((prev) =>
      payload.kind === "connection" ? disconnect(prev, payload.from, payload.fromPort) : setBranchEntry(prev, payload.ownerId, payload.branchKey, null),
    );
  }

  function handleSetBranchEntry(ownerId: string, branchKey: BranchKey, entryId: string) {
    applyFlow((prev) => setBranchEntry(prev, ownerId, branchKey, entryId));
  }

  function handleSelectionChange(ids: string[]) {
    setSelectedIds(ids);
  }

  function handleSelectAll() {
    setSelectedIds(allIds(flow));
  }

  function handleCopy() {
    if (selectedIds.length === 0) return;
    // Excludes `start` — copy/paste-ing it would produce a second one,
    // which the flow can never actually have (see `ensureStartNode`).
    const nodes = selectedIds.map((id) => findNode(flow, id)).filter((n): n is FlowNode => n !== null && n.kind !== "start");
    if (nodes.length > 0) setClipboard(nodes);
  }

  function handleCut() {
    if (selectedIds.length === 0) return;
    handleCopy();
    handleDeleteSteps(selectedIds);
  }

  function handlePaste() {
    if (clipboard.length === 0) return;
    const pasted = cloneNodes(clipboard);
    applyFlow((prev) => dropMany(prev, primaryId, pasted));
    setSelectedIds(pasted.map((n) => n.id));
  }

  function handleDuplicate() {
    if (selectedIds.length === 0) return;
    // Excludes `start` for the same reason as `handleCopy`.
    const nodes = selectedIds.map((id) => findNode(flow, id)).filter((n): n is FlowNode => n !== null && n.kind !== "start");
    if (nodes.length === 0) return;
    const duplicated = cloneNodes(nodes);
    applyFlow((prev) => dropMany(prev, primaryId, duplicated));
    setSelectedIds(duplicated.map((n) => n.id));
  }

  function handleUndo() {
    if (history.length === 0) return;
    const prev = history[history.length - 1];
    setHistory((h) => h.slice(0, -1));
    setFuture((f) => [...f.slice(-(HISTORY_LIMIT - 1)), flow]);
    setFlow(prev);
    setIsDirty(true);
  }

  function handleRedo() {
    if (future.length === 0) return;
    const next = future[future.length - 1];
    setFuture((f) => f.slice(0, -1));
    setHistory((h) => [...h.slice(-(HISTORY_LIMIT - 1)), flow]);
    setFlow(next);
    setIsDirty(true);
  }

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (isTypingTarget(e.target)) return;
      const key = e.key.toLowerCase();
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && key === "z") {
        e.preventDefault();
        handleUndo();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && (key === "y" || (e.shiftKey && key === "z"))) {
        e.preventDefault();
        handleRedo();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "a") {
        e.preventDefault();
        handleSelectAll();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && key === "s") {
        e.preventDefault();
        void handleSaveAs();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "s") {
        e.preventDefault();
        void handleSave();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "o") {
        e.preventDefault();
        void handleOpenFlow();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "n") {
        e.preventDefault();
        handleNewFlow();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "c") {
        if (hasTextSelection()) return;
        e.preventDefault();
        handleCopy();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "x") {
        if (hasTextSelection()) return;
        e.preventDefault();
        handleCut();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "v") {
        e.preventDefault();
        handlePaste();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && key === "d") {
        e.preventDefault();
        handleDuplicate();
        return;
      }
      if ((e.key === "Delete" || e.key === "Backspace") && selectedIds.length > 0) {
        e.preventDefault();
        handleDeleteSteps(selectedIds);
      } else if (e.key === "Escape") {
        // While a flow is running, Escape is the emergency stop
        // instead of deselecting — there's nothing to select-away from
        // when the canvas is mid-run, and "immediately stop whatever's
        // happening" is what a person reaching for Escape wants then.
        if (runState.status === "running") {
          e.preventDefault();
          handleStop();
        } else {
          setSelectedIds([]);
        }
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedIds, flow, positions, stepDelayMs, history, future, clipboard, filePath, fileName, isDirty, runState.status]);

  /** `stepMode: true` is the toolbar's "ステップ" command — the run
   *  starts already paused before its first step instead of running
   *  freely until the first breakpoint. */
  async function handleRun(stepMode = false) {
    const idleStatus: Record<string, string> = Object.fromEntries(allIds(flow).map((id) => [id, "idle"]));
    setLiveStatus(idleStatus);
    setLiveVariables({});
    setHasRun(true);
    setLog([]);
    setStartedAt(Date.now());
    setElapsedMs(0);
    setMonitorPaused(false);
    setPausedStepId(null);
    setRunState({ status: "running" });
    try {
      await invoke("run_flow_yaml", { yaml: flowYaml, stepMode });
      setRunState({ status: "success" });
    } catch (error) {
      setRunState({ status: "error", message: String(error) });
    } finally {
      setMonitorPaused(false);
      setPausedStepId(null);
    }
  }

  /** The "force stop" button/shortcut — tells the backend to end the
   *  run at its next step boundary (see `engine::request_stop`).
   *  `handleRun`'s own `await` still resolves normally once the
   *  backend reports back, same as a flow that finished on its own. */
  function handleStop() {
    void invoke("stop_flow");
  }

  /** Advances a paused run by exactly one step, then pauses again
   *  right before the step after that. */
  function handleDebugStep() {
    void invoke("debug_step");
  }

  /** Resumes a paused run, letting it run freely until the next
   *  breakpoint or the flow ends. */
  function handleDebugContinue() {
    void invoke("debug_continue");
  }

  if (view === "home") {
    return <HomeScreen onNew={handleNewFlow} onOpen={() => void handleOpenFlow()} onOpenRecent={(path) => void handleOpenRecentFlow(path)} />;
  }

  return (
    <div className="app">
      <Toolbar
        mode={mode}
        onModeChange={setMode}
        runState={runState}
        onRun={() => void handleRun(false)}
        onStepRun={() => void handleRun(true)}
        onStop={handleStop}
        isPaused={pausedStepId !== null}
        onDebugStep={handleDebugStep}
        onDebugContinue={handleDebugContinue}
        stepDelayMs={stepDelayMs}
        onStepDelayChange={setStepDelayMs}
        isElevated={isElevated}
        onRelaunchAsAdmin={() => void handleRelaunchAsAdmin()}
        onCheckForUpdate={() => void handleCheckForUpdate()}
        edgeStyle={edgeStyle}
        onEdgeStyleChange={setEdgeStyle}
        orientation={orientation}
        onOrientationChange={setOrientation}
        onArrange={handleArrange}
        onUndo={handleUndo}
        onRedo={handleRedo}
        canUndo={history.length > 0}
        canRedo={future.length > 0}
        onCut={handleCut}
        onCopy={handleCopy}
        onPaste={handlePaste}
        onDuplicate={handleDuplicate}
        onSelectAll={handleSelectAll}
        onDeleteSelected={() => handleDeleteSteps(selectedIds)}
        hasSelection={selectedIds.length > 0}
        hasClipboard={clipboard.length > 0}
        fileName={filePath ? `${fileName}.relay` : fileName}
        isDirty={isDirty}
        onNewFlow={handleNewFlow}
        onOpenFlow={() => void handleOpenFlow()}
        onSave={() => void handleSave()}
        onSaveAs={() => void handleSaveAs()}
        onBackToHome={handleBackToHome}
        recentFlows={recentFlows}
        onOpenRecent={(path) => void handleOpenRecentFlow(path)}
      />
      <Palette onAddStep={handleAddStep} nodeNameMode={nodeNameMode} onNodeNameModeChange={setNodeNameMode} />
      {mode === "flow" ? (
        <Canvas
          flow={flow}
          selectedIds={selectedIds}
          onSelectionChange={handleSelectionChange}
          liveStatus={liveStatus}
          onDeleteStep={handleDeleteStep}
          onAddStep={handleAddStep}
          onAddIntoBranch={handleAddIntoBranch}
          onConnect={handleConnect}
          onConnectBranchEntry={handleConnectBranchEntry}
          onDisconnect={handleDisconnect}
          onSetBranchEntry={handleSetBranchEntry}
          onToggleEnabled={handleToggleEnabled}
          onToggleBreakpoint={handleToggleBreakpoint}
          onZoomChange={setZoomPercent}
          edgeStyle={edgeStyle}
          orientation={orientation}
          positions={positions}
          onPositionChange={handlePositionChange}
          onUndo={handleUndo}
          onRedo={handleRedo}
          canUndo={history.length > 0}
          canRedo={future.length > 0}
          onPaste={handlePaste}
          hasClipboard={clipboard.length > 0}
          onSelectAll={handleSelectAll}
          onArrange={handleArrange}
          nodeNameMode={nodeNameMode}
        />
      ) : mode === "code" ? (
        <CodeView yaml={flowYaml} fileName={filePath ? `${fileName}.relay` : fileName} />
      ) : (
        <VariablesPanel flow={flow} liveVariables={liveVariables} hasRun={hasRun} />
      )}
      <Inspector
        flow={flow}
        selectedIds={selectedIds}
        onUpdateStep={handleUpdateStep}
        onDeleteStep={handleDeleteStep}
        onDeleteSteps={handleDeleteSteps}
        onToggleEnabled={handleToggleEnabled}
        nodeNameMode={nodeNameMode}
      />
      <StatusBar
        runState={runState}
        log={log}
        elapsedMs={elapsedMs}
        zoomPercent={zoomPercent}
        monitorPaused={monitorPaused}
        debugPaused={pausedStepId !== null}
        onOpenLogViewer={() => setShowLogViewer(true)}
      />
      {showLogViewer && <LogViewer log={log} onClose={() => setShowLogViewer(false)} />}
      <RegionPickerHost />
    </div>
  );
}

export default App;
