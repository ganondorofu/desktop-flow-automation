import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ReactFlow,
  Background,
  BackgroundVariant,
  ConnectionLineType,
  applyNodeChanges,
  type NodeTypes,
  type EdgeTypes,
  type NodeMouseHandler,
  type EdgeMouseHandler,
  type OnConnect,
  type OnConnectEnd,
  type OnNodesChange,
  type Node,
  type Edge,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useTranslation } from "react-i18next";
import { TerminalNode } from "./TerminalNode";
import { CenteredEdge, type CenteredEdgeData } from "./CenteredEdge";
import type { TerminalNodeData } from "../data/canvasTypes";
import {
  describeNode,
  nodeIsIncomplete,
  duplicateFunctionDefIds,
  findBranchOwner,
  findContainer,
  findNode,
  NODE_KIND_OF,
  paletteLabel,
  type Branch,
  type BranchKey,
  type Connection,
  type FlowNode,
  type NodeNameMode,
} from "../data/flowModel";
import { ContextMenu, type MenuItem } from "./ContextMenu";

const nodeTypes: NodeTypes = { terminal: TerminalNode };
const edgeTypes: EdgeTypes = { centered: CenteredEdge };
const NODE_WIDTH = 260;
const NODE_HEIGHT = 96;
// Gaps are fixed pixel amounts sized to the axis they move along,
// regardless of which orientation is using them for what: X_GAP
// (width-based) whenever something moves horizontally, Y_GAP
// (height-based) whenever something moves vertically.
const X_GAP = NODE_WIDTH + 70;
const Y_GAP = NODE_HEIGHT + NODE_HEIGHT / 2;
// How far an `if`/`try_catch`'s `then`/`try` branch sits above its
// own trunk row (and `otherwise`/`catch` below) in horizontal mode —
// deliberately smaller than `Y_GAP` (which is sized for two full rows
// of unrelated content stacked in sequence): straddling the *same*
// row just needs enough clearance not to touch, the same gap feel as
// any two adjacent normal steps.
const BRANCH_STRADDLE_GAP = NODE_HEIGHT + 40;

export type Orientation = "horizontal" | "vertical";
export type EdgeStyle = "orthogonal" | "straight" | "curved";

const EDGE_SHAPE_OF: Record<EdgeStyle, CenteredEdgeData["shape"]> = {
  orthogonal: "smoothstep",
  straight: "straight",
  curved: "default",
};

/** What a right-click on a wire (or "detach branch start") needs to
 *  identify exactly what to cut. `connection` is an ordinary wire (a
 *  step's plain output); `entry` is a loop's or if's derived "this
 *  branch starts here" wire. */
export type DisconnectPayload =
  | { kind: "connection"; from: string; fromPort: string | null }
  | { kind: "entry"; ownerId: string; branchKey: BranchKey };

interface LayoutCtx {
  t: (key: string, opts?: Record<string, unknown>) => string;
  selectedIds: Set<string>;
  liveStatus: Record<string, string>;
  onDelete: (id: string) => void;
  edgeShape: CenteredEdgeData["shape"];
  nodeNameMode: NodeNameMode;
  duplicateFunctionDefIds: Set<string>;
}

/** Describes the wire leading into whatever node gets laid out next —
 *  either a plain predecessor, an if's yes/no fork, or a loop's
 *  derived body-entry anchor. `null` means "no incoming wire" (the
 *  very first node of a container, or a disconnected island). */
interface EntryEdge {
  source: string;
  sourceHandle: string | null;
  className?: string;
}

function findConn(connections: Connection[], from: string, fromPort: string | null): string | null {
  return connections.find((c) => c.from === from && c.fromPort === fromPort)?.to ?? null;
}

function makeEdge(entry: EntryEdge, targetId: string, ctx: LayoutCtx): Edge {
  return {
    id: `w:${entry.source}:${entry.sourceHandle ?? ""}->${targetId}`,
    source: entry.source,
    sourceHandle: entry.sourceHandle ?? undefined,
    target: targetId,
    type: "centered",
    data: { shape: ctx.edgeShape },
    interactionWidth: 28,
    className: entry.className,
  };
}

interface ChainLayout {
  nodes: Node<TerminalNodeData>[];
  edges: Edge[];
  maxX: number;
  maxY: number;
  /** How far up this chain's content reaches — only meaningful (and
   *  only ever read) in horizontal mode, where an `if`/`try_catch`
   *  branch can run above its own trunk row. */
  minY?: number;
  /** How far left this chain's content reaches — the vertical-mode
   *  counterpart of `minY`, for an `if`/`try_catch` branch running
   *  left of its own trunk column. */
  minX?: number;
}

function nodeKindFlags(item: FlowNode) {
  return {
    isIf: item.kind === "if",
    isLoop: item.kind === "loop",
    isTryCatch: item.kind === "try_catch",
    isFunctionDef: item.kind === "function_def",
  };
}

function pushNode(
  nodes: Node<TerminalNodeData>[],
  item: FlowNode,
  x: number,
  y: number,
  ctx: LayoutCtx,
  orientation: Orientation,
  branchHandles: import("../data/canvasTypes").BranchHandle[],
) {
  const { isIf, isLoop, isTryCatch, isFunctionDef } = nodeKindFlags(item);
  nodes.push({
    id: item.id,
    type: "terminal",
    position: { x, y },
    selected: ctx.selectedIds.has(item.id),
    style: { width: NODE_WIDTH, height: NODE_HEIGHT },
    data: {
      ...describeNode(item, ctx.t, ctx.nodeNameMode),
      kind: NODE_KIND_OF[item.kind],
      orientation,
      // A function definition is never reached by a normal wire and
      // never continues into one either — it only runs via a
      // `call_function` step elsewhere, so its only real connector is
      // the body handle below/to the right.
      hasDefaultOutput: item.kind !== "stop" && item.kind !== "function_def",
      // A loop's/function_def's body handle claims "right" in
      // horizontal mode (the branch continues the same row) or
      // "bottom" in vertical mode (it continues the same column) —
      // either way, the node's own "continue after the branch ends"
      // output has to move to the other axis instead, or they'd stack
      // on the same spot. An if's/try_catch's then/otherwise (or
      // try/catch) sit on the perpendicular axis already (above/below
      // in horizontal, left/right in vertical), so only loop/
      // function_def need this. See `outputPosition`'s doc comment in
      // canvasTypes.ts.
      outputPosition:
        (isLoop || isIf || isTryCatch || isFunctionDef) && orientation === "horizontal"
          ? "bottom"
          : (isLoop || isFunctionDef) && orientation === "vertical"
            ? "right"
            : undefined,
      hasInput: item.kind !== "start" && item.kind !== "function_def",
      branchHandles,
      // The flow's `start` node is never deletable — every flow
      // always has exactly one, and always runs from it.
      deletable: item.kind !== "start",
      onDelete: ctx.onDelete,
      enabled: item.enabled,
      breakpoint: item.breakpoint === true,
      comment: item.comment,
      incomplete: nodeIsIncomplete(item) || ctx.duplicateFunctionDefIds.has(item.id),
      status: (ctx.liveStatus[item.id] as TerminalNodeData["status"]) ?? "idle",
    } as TerminalNodeData,
  });
}

/** How many connections lead into `id` — 2+ means it's where two
 *  branches (or, degenerate but possible, more) reconverge. */
function incomingCount(connections: Connection[], id: string): number {
  return connections.filter((c) => c.to === id).length;
}

/** Vertical mode: the main trunk runs top-to-bottom. An `if`/`try_catch`
 *  diverges sideways (its two paths offset in x) since that's
 *  perpendicular to the trunk's own direction — same idea as
 *  horizontal mode's `if` straddling above/below its trunk row. A
 *  `loop`/function's body, though, continues *inline* straight down
 *  through the trunk's own column (matching the trunk's own
 *  direction, the vertical-mode mirror of horizontal's loop body
 *  continuing rightward through the trunk's own row) — so whatever
 *  comes after it has to explicitly break out to a fresh column
 *  (aligned back to `rootY`) instead of continuing down through the
 *  same column the body just used. */
function layoutChainVertical(
  branch: Branch,
  headId: string,
  desiredLeft: number,
  y0: number,
  rootY: number,
  visited: Set<string>,
  ctx: LayoutCtx,
  entryEdge: EntryEdge | null,
): ChainLayout {
  // Reserving wrapper — an `if`/`try_catch`'s `then`/`try` branch is
  // allowed to run left of its own trunk column, so measure how far
  // left this chain would reach on its own (a free trial pass at
  // x=0), then run it for real shifted right by exactly that much,
  // guaranteeing its leftmost content lands precisely at `desiredLeft`.
  const trial = layoutChainVerticalRaw(branch, headId, 0, y0, rootY, new Set(visited), ctx, entryEdge);
  const x = desiredLeft - trial.minX!;
  return layoutChainVerticalRaw(branch, headId, x, y0, rootY, visited, ctx, entryEdge);
}

function layoutChainVerticalRaw(
  branch: Branch,
  headId: string,
  x: number,
  y0: number,
  rootY: number,
  visited: Set<string>,
  ctx: LayoutCtx,
  entryEdge: EntryEdge | null,
): ChainLayout {
  const nodes: Node<TerminalNodeData>[] = [];
  const edges: Edge[] = [];
  let y = y0;
  let currentId: string | null = headId;
  let incoming: EntryEdge | null = entryEdge;
  let maxX = x;
  let maxY = y;
  let minX = x;

  /** Renders `nextId` in a fresh column to the right of everything
   *  laid out so far, aligned back to `rootY` — the vertical-mode
   *  counterpart of `layoutChainHorizontal`'s `breakToNewRow`. */
  function breakToNewColumn(nextId: string, from: EntryEdge) {
    if (visited.has(nextId)) {
      edges.push(makeEdge(from, nextId, ctx));
      return;
    }
    const r = layoutChainVertical(branch, nextId, maxX + X_GAP, rootY, rootY, visited, ctx, from);
    nodes.push(...r.nodes);
    edges.push(...r.edges);
    maxX = Math.max(maxX, r.maxX);
    maxY = Math.max(maxY, r.maxY);
    minX = Math.min(minX, r.minX!);
  }

  while (currentId) {
    if (visited.has(currentId)) {
      if (incoming) edges.push(makeEdge(incoming, currentId, ctx));
      break;
    }
    visited.add(currentId);
    const item = branch.steps.find((s) => s.id === currentId);
    if (!item) break;
    const { isIf, isLoop, isTryCatch, isFunctionDef } = nodeKindFlags(item);

    pushNode(
      nodes,
      item,
      x,
      y,
      ctx,
      "vertical",
      isIf
        ? [
            { id: "then", position: "left", className: "handle-yes", connectable: true },
            { id: "otherwise", position: "right", className: "handle-no", connectable: true },
          ]
        : isTryCatch
          ? [
              { id: "try", position: "left", className: "handle-try", connectable: true },
              { id: "catch", position: "right", className: "handle-catch", connectable: true },
            ]
          : isLoop
            ? [{ id: "body", position: "bottom", className: "handle-loop", connectable: true }]
            : isFunctionDef
              ? [{ id: "body", position: "bottom", className: "handle-function", connectable: true }]
              : [],
    );
    if (incoming) edges.push(makeEdge(incoming, item.id, ctx));
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);

    let nextY = y + Y_GAP;

    if (isIf && item.kind === "if") {
      // `then` runs left of the trunk column, `otherwise` right —
      // safe now that `layoutBranchVertical` reports each branch's
      // true `minX` upward instead of silently absorbing it, so
      // whatever encloses this `if` (a loop/function body it's nested
      // inside) can shift the *whole* unit — including its own owner
      // node — right by exactly as much room as `then` needs, rather
      // than only this branch's content moving while its owner node
      // stays put beside a gap.
      const branchY = y + Y_GAP;
      const thenLayout = layoutBranchVertical(item.then, x - X_GAP, branchY, branchY, ctx, { source: item.id, sourceHandle: "then", className: "edge-yes" });
      nodes.push(...thenLayout.nodes);
      edges.push(...thenLayout.edges);
      nextY = Math.max(nextY, thenLayout.maxY);
      maxX = Math.max(maxX, thenLayout.maxX);
      minX = Math.min(minX, thenLayout.minX!);

      const otherwiseLayout = layoutBranchVertical(item.otherwise, x + X_GAP, branchY, branchY, ctx, { source: item.id, sourceHandle: "otherwise", className: "edge-no" });
      nodes.push(...otherwiseLayout.nodes);
      edges.push(...otherwiseLayout.edges);
      nextY = Math.max(nextY, otherwiseLayout.maxY);
      maxX = Math.max(maxX, otherwiseLayout.maxX);
      minX = Math.min(minX, otherwiseLayout.minX!);

      y = nextY;
      maxY = Math.max(maxY, y);
      incoming = { source: item.id, sourceHandle: null };
      currentId = findConn(branch.connections, item.id, null);
    } else if (isTryCatch && item.kind === "try_catch") {
      // Same straddled placement as `if`, above.
      const branchY = y + Y_GAP;
      const tryLayout = layoutBranchVertical(item.tryBranch, x - X_GAP, branchY, branchY, ctx, { source: item.id, sourceHandle: "try", className: "edge-try" });
      nodes.push(...tryLayout.nodes);
      edges.push(...tryLayout.edges);
      nextY = Math.max(nextY, tryLayout.maxY);
      maxX = Math.max(maxX, tryLayout.maxX);
      minX = Math.min(minX, tryLayout.minX!);

      const catchLayout = layoutBranchVertical(item.catch, x + X_GAP, branchY, branchY, ctx, { source: item.id, sourceHandle: "catch", className: "edge-catch" });
      nodes.push(...catchLayout.nodes);
      edges.push(...catchLayout.edges);
      nextY = Math.max(nextY, catchLayout.maxY);
      maxX = Math.max(maxX, catchLayout.maxX);
      minX = Math.min(minX, catchLayout.minX!);

      y = nextY;
      maxY = Math.max(maxY, y);
      incoming = { source: item.id, sourceHandle: null };
      currentId = findConn(branch.connections, item.id, null);
    } else if (isLoop && item.kind === "loop") {
      // The body continues straight down through this same column
      // (`x` unchanged) instead of offsetting sideways — see this
      // function's doc comment.
      const bodyLayout = layoutBranchVertical(item.body, x, y + Y_GAP, y + Y_GAP, ctx, { source: item.id, sourceHandle: "body", className: "edge-loop" });
      nodes.push(...bodyLayout.nodes);
      edges.push(...bodyLayout.edges);
      maxX = Math.max(maxX, bodyLayout.maxX);
      maxY = Math.max(maxY, bodyLayout.maxY);
      minX = Math.min(minX, bodyLayout.minX!);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewColumn(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else if (isFunctionDef && item.kind === "function_def") {
      // Same inline-body treatment as `loop`, above.
      const bodyLayout = layoutBranchVertical(item.body, x, y + Y_GAP, y + Y_GAP, ctx, { source: item.id, sourceHandle: "body", className: "edge-function" });
      nodes.push(...bodyLayout.nodes);
      edges.push(...bodyLayout.edges);
      maxX = Math.max(maxX, bodyLayout.maxX);
      maxY = Math.max(maxY, bodyLayout.maxY);
      minX = Math.min(minX, bodyLayout.minX!);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewColumn(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else {
      y = nextY;
      maxY = Math.max(maxY, y);
      incoming = { source: item.id, sourceHandle: null };
      currentId = findConn(branch.connections, item.id, null);
    }
  }

  return { nodes, edges, maxX, maxY, minX };
}

/** Horizontal mode (the default): the main trunk runs left-to-right.
 *  An `if` diverges its yes/no paths above/below the trunk, each
 *  continuing rightward on its own row — same idea as vertical mode,
 *  just rotated. A `loop`'s body, though, continues *inline* on the
 *  loop's own row (rightward) rather than beside it, since here
 *  "sideways" (right) is the trunk's own direction. That means, unlike
 *  vertical mode, "down" is *not* the trunk's direction — so whatever
 *  comes after an if reconverges, or after a loop's body ends, has to
 *  explicitly drop to a fresh row below (aligned back to `rootX`,
 *  the column the enclosing chain itself started at) rather than
 *  bleeding rightward through whichever branch happens to reach it
 *  first, which is what made a loop's continuation and an if's feel
 *  inconsistent before this rule existed. */
function layoutChainHorizontal(
  branch: Branch,
  headId: string,
  x0: number,
  desiredTop: number,
  rootX: number,
  visited: Set<string>,
  ctx: LayoutCtx,
  entryEdge: EntryEdge | null,
): ChainLayout {
  // An `if`/`try_catch` branch is allowed to run *above* its own
  // trunk row (see the `edge-yes`/`edge-try` placement below) — the
  // one direction `maxY` alone can't see coming. Measure how far up
  // this chain would reach on its own (a free trial pass at y=0, using
  // a throwaway copy of `visited` so it leaves no trace), then run it
  // for real shifted down by exactly that much, so its topmost content
  // lands precisely at `desiredTop` no matter how deep the upward
  // escape goes — the caller's `desiredTop` is a hard guarantee, not a
  // hint. That's what keeps a branch nested inside another (e.g. an
  // `if` inside a function's body) from ever poking up into whatever
  // an enclosing branch already placed above this row.
  const trial = layoutChainHorizontalRaw(branch, headId, x0, 0, rootX, new Set(visited), ctx, entryEdge);
  const y = desiredTop - trial.minY!;
  return layoutChainHorizontalRaw(branch, headId, x0, y, rootX, visited, ctx, entryEdge);
}

function layoutChainHorizontalRaw(
  branch: Branch,
  headId: string,
  x0: number,
  y: number,
  rootX: number,
  visited: Set<string>,
  ctx: LayoutCtx,
  entryEdge: EntryEdge | null,
): ChainLayout {
  const nodes: Node<TerminalNodeData>[] = [];
  const edges: Edge[] = [];
  let x = x0;
  let currentId: string | null = headId;
  let incoming: EntryEdge | null = entryEdge;
  let maxX = x;
  let maxY = y;
  let minY = y;

  /** Renders `nextId` on a fresh row below everything laid out so far,
   *  aligned back to `rootX` — used both for an if's reconvergence
   *  point and for whatever follows a loop. */
  function breakToNewRow(nextId: string, from: EntryEdge) {
    if (visited.has(nextId)) {
      edges.push(makeEdge(from, nextId, ctx));
      return;
    }
    const r = layoutChainHorizontal(branch, nextId, rootX, maxY + Y_GAP, rootX, visited, ctx, from);
    nodes.push(...r.nodes);
    edges.push(...r.edges);
    maxX = Math.max(maxX, r.maxX);
    maxY = Math.max(maxY, r.maxY);
    minY = Math.min(minY, r.minY!);
  }

  while (currentId) {
    if (visited.has(currentId)) {
      if (incoming) edges.push(makeEdge(incoming, currentId, ctx));
      break;
    }
    visited.add(currentId);
    const item = branch.steps.find((s) => s.id === currentId);
    if (!item) break;
    const { isIf, isLoop, isTryCatch, isFunctionDef } = nodeKindFlags(item);

    pushNode(
      nodes,
      item,
      x,
      y,
      ctx,
      "horizontal",
      isIf
        ? // Both then/otherwise exit from the right, like every other
          // step's plain output — matching the trunk direction reads
          // as "the flow keeps moving right" instead of "it forks away
          // from the trunk", and it's what actually happens: both
          // branches are laid out to the right of this node.
          // Draggable (`connectable: true`) — dropping a wire here
          // sets which of the branch's steps runs first, same as
          // "ここから開始する". `offsetPercent` keeps the two dots
          // from stacking on the same spot.
          [
            { id: "then", position: "right", className: "handle-yes", connectable: true, offsetPercent: 32 },
            { id: "otherwise", position: "right", className: "handle-no", connectable: true, offsetPercent: 68 },
          ]
        : isTryCatch
          ? [
              { id: "try", position: "right", className: "handle-try", connectable: true, offsetPercent: 32 },
              { id: "catch", position: "right", className: "handle-catch", connectable: true, offsetPercent: 68 },
            ]
          : isLoop
            ? [{ id: "body", position: "right", className: "handle-loop", connectable: true }]
            : isFunctionDef
              ? [{ id: "body", position: "right", className: "handle-function", connectable: true }]
              : [],
    );
    if (incoming) edges.push(makeEdge(incoming, item.id, ctx));
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);

    if (isIf && item.kind === "if") {
      // `then` runs above the trunk row, `otherwise` below — restored
      // to this straddled look now that `layoutBranchHorizontal`
      // guarantees (via `layoutChainHorizontal`'s reserving wrapper)
      // that neither branch's own content can escape past where it
      // was actually placed, however deep its own nesting goes. That
      // guarantee is also *why* this is safe now: an enclosing branch
      // (a loop/function body this `if` sits inside) sees this whole
      // chain's true `minY` and reserves room for it instead of it
      // silently overlapping whatever sits above.
      const branchX = x + X_GAP;
      const thenLayout = layoutBranchHorizontal(item.then, branchX, y - BRANCH_STRADDLE_GAP, branchX, ctx, { source: item.id, sourceHandle: "then", className: "edge-yes" });
      nodes.push(...thenLayout.nodes);
      edges.push(...thenLayout.edges);
      maxX = Math.max(maxX, thenLayout.maxX);
      maxY = Math.max(maxY, thenLayout.maxY);
      minY = Math.min(minY, thenLayout.minY!);

      const otherwiseLayout = layoutBranchHorizontal(item.otherwise, branchX, y + BRANCH_STRADDLE_GAP, branchX, ctx, { source: item.id, sourceHandle: "otherwise", className: "edge-no" });
      nodes.push(...otherwiseLayout.nodes);
      edges.push(...otherwiseLayout.edges);
      maxX = Math.max(maxX, otherwiseLayout.maxX);
      maxY = Math.max(maxY, otherwiseLayout.maxY);
      minY = Math.min(minY, otherwiseLayout.minY!);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewRow(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else if (isTryCatch && item.kind === "try_catch") {
      // Same straddled placement as `if`, above.
      const branchX = x + X_GAP;
      const tryLayout = layoutBranchHorizontal(item.tryBranch, branchX, y - BRANCH_STRADDLE_GAP, branchX, ctx, { source: item.id, sourceHandle: "try", className: "edge-try" });
      nodes.push(...tryLayout.nodes);
      edges.push(...tryLayout.edges);
      maxX = Math.max(maxX, tryLayout.maxX);
      maxY = Math.max(maxY, tryLayout.maxY);
      minY = Math.min(minY, tryLayout.minY!);

      const catchLayout = layoutBranchHorizontal(item.catch, branchX, y + BRANCH_STRADDLE_GAP, branchX, ctx, { source: item.id, sourceHandle: "catch", className: "edge-catch" });
      nodes.push(...catchLayout.nodes);
      edges.push(...catchLayout.edges);
      maxX = Math.max(maxX, catchLayout.maxX);
      maxY = Math.max(maxY, catchLayout.maxY);
      minY = Math.min(minY, catchLayout.minY!);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewRow(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else if (isLoop && item.kind === "loop") {
      const bodyLayout = layoutBranchHorizontal(item.body, x + X_GAP, y, x + X_GAP, ctx, { source: item.id, sourceHandle: "body", className: "edge-loop" });
      nodes.push(...bodyLayout.nodes);
      edges.push(...bodyLayout.edges);
      maxX = Math.max(maxX, bodyLayout.maxX);
      maxY = Math.max(maxY, bodyLayout.maxY);
      minY = Math.min(minY, bodyLayout.minY!);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewRow(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else if (isFunctionDef && item.kind === "function_def") {
      const bodyLayout = layoutBranchHorizontal(item.body, x + X_GAP, y, x + X_GAP, ctx, { source: item.id, sourceHandle: "body", className: "edge-function" });
      nodes.push(...bodyLayout.nodes);
      edges.push(...bodyLayout.edges);
      maxX = Math.max(maxX, bodyLayout.maxX);
      minY = Math.min(minY, bodyLayout.minY!);
      maxY = Math.max(maxY, bodyLayout.maxY);

      const nextId = findConn(branch.connections, item.id, null);
      if (nextId) breakToNewRow(nextId, { source: item.id, sourceHandle: null });
      currentId = null;
    } else {
      const nextId = findConn(branch.connections, item.id, null);
      if (!nextId) {
        currentId = null;
      } else if (incomingCount(branch.connections, nextId) > 1) {
        breakToNewRow(nextId, { source: item.id, sourceHandle: null });
        currentId = null;
      } else {
        x = x + X_GAP;
        incoming = { source: item.id, sourceHandle: null };
        currentId = nextId;
      }
    }
  }

  return { nodes, edges, maxX, maxY, minY };
}

interface BranchLayout {
  nodes: Node<TerminalNodeData>[];
  edges: Edge[];
  maxX: number;
  maxY: number;
  /** See `ChainLayout.minY` — only set (and only read) in horizontal
   *  mode. */
  minY?: number;
  /** See `ChainLayout.minX` — only set (and only read) in vertical
   *  mode. */
  minX?: number;
}

/** Lays out every step in `branch`'s pool — not just the ones reachable
 *  from `entry`. Each connected chain (and every fully-unconnected
 *  step) gets its own column, side by side, so a step the user
 *  disconnected is still visible sitting right where it landed instead
 *  of vanishing from the canvas. `ownerEdge`, when set, is a loop
 *  node's derived "the body starts here" wire into `branch.entry`. */
function layoutBranchVertical(branch: Branch, x0: number, y0: number, rootY: number, ctx: LayoutCtx, ownerEdge: EntryEdge | null): BranchLayout {
  const { heads, visited } = branchHeads(branch);
  const nodes: Node<TerminalNodeData>[] = [];
  const edges: Edge[] = [];
  let maxX = x0;
  let maxY = y0;
  let minX = x0;
  let cursorX = x0;
  let firstCol = true;
  for (const head of heads) {
    if (visited.has(head)) continue;
    const entryEdge = ownerEdge && head === branch.entry ? ownerEdge : null;
    let chain: ChainLayout;
    if (firstCol) {
      // The first column is placed *without* reserving — see
      // `layoutBranchHorizontal`'s identical treatment of its first
      // row for the full reasoning. Its true leftward reach is
      // reported via `minX` rather than locally absorbed, so an
      // enclosing branch can shift the whole unit it belongs to.
      chain = layoutChainVerticalRaw(branch, head, x0, y0, rootY, visited, ctx, entryEdge);
      minX = Math.min(minX, chain.minX!);
    } else {
      // A later independent chain is a genuinely new column that must
      // not collide with the column(s) before it, so it does reserve.
      chain = layoutChainVertical(branch, head, cursorX + X_GAP, y0, rootY, visited, ctx, entryEdge);
    }
    firstCol = false;
    nodes.push(...chain.nodes);
    edges.push(...chain.edges);
    maxX = Math.max(maxX, chain.maxX);
    maxY = Math.max(maxY, chain.maxY);
    cursorX = chain.maxX;
  }

  return { nodes, edges, maxX, maxY, minX };
}

/** Horizontal counterpart of `layoutBranchVertical` — independent
 *  chains stack as separate rows (not columns), since rows are
 *  already how this orientation makes room for anything that doesn't
 *  fit the trunk's own row. */
function layoutBranchHorizontal(branch: Branch, x0: number, y0: number, rootX: number, ctx: LayoutCtx, ownerEdge: EntryEdge | null): BranchLayout {
  const { heads, visited } = branchHeads(branch);
  const nodes: Node<TerminalNodeData>[] = [];
  const edges: Edge[] = [];
  let maxX = x0;
  let maxY = y0;
  let minY = y0;
  let cursorY = y0;
  let firstRow = true;
  for (const head of heads) {
    if (visited.has(head)) continue;
    const entryEdge = ownerEdge && head === branch.entry ? ownerEdge : null;
    let chain: ChainLayout;
    if (firstRow) {
      // The first row is placed *without* reserving — it's rendered
      // literally at `y0`, and however far up its own content escapes
      // (an `if` inside it, however deeply nested) is reported
      // truthfully in `minY` rather than being locally absorbed. That
      // lets the caller (an enclosing `if`'s branch, a loop/function
      // body) fold this branch's true shape into its *own* reserving
      // decision, so the whole unit this branch lives inside — right
      // up to wherever it's anchored in the outer trunk — shifts down
      // together as one piece instead of only this branch's content
      // moving while its owner node stays put beside a gap.
      chain = layoutChainHorizontalRaw(branch, head, x0, y0, rootX, visited, ctx, entryEdge);
      minY = Math.min(minY, chain.minY!);
    } else {
      // A second (or later) independent chain in this same branch is
      // a genuinely new row that must not collide with the row(s)
      // above it, so it *does* reserve.
      chain = layoutChainHorizontal(branch, head, x0, cursorY + Y_GAP, rootX, visited, ctx, entryEdge);
    }
    firstRow = false;
    nodes.push(...chain.nodes);
    edges.push(...chain.edges);
    maxX = Math.max(maxX, chain.maxX);
    maxY = Math.max(maxY, chain.maxY);
    cursorY = chain.maxY;
  }

  return { nodes, edges, maxX, maxY, minY };
}

function branchHeads(branch: Branch): { heads: string[]; visited: Set<string> } {
  const heads: string[] = [];
  if (branch.entry && branch.steps.some((s) => s.id === branch.entry)) heads.push(branch.entry);
  for (const s of branch.steps) {
    if (heads.includes(s.id)) continue;
    if (!branch.connections.some((c) => c.to === s.id)) heads.push(s.id);
  }
  for (const s of branch.steps) if (!heads.includes(s.id)) heads.push(s.id);
  return { heads, visited: new Set<string>() };
}

function layoutBranch(branch: Branch, x0: number, y0: number, ctx: LayoutCtx, ownerEdge: EntryEdge | null, orientation: Orientation): BranchLayout {
  return orientation === "horizontal"
    ? layoutBranchHorizontal(branch, x0, y0, x0, ctx, ownerEdge)
    : layoutBranchVertical(branch, x0, y0, y0, ctx, ownerEdge);
}

export type NodePositions = Record<string, { x: number; y: number }>;

/** Computes the algorithmic (tidy) position for every node in `flow` —
 *  used both for the initial/default layout (nodes with no manual
 *  position yet fall back to this) and for the "整列" (arrange) action,
 *  which snapshots these positions into the free-placement state so
 *  the user gets a clean starting point they can keep dragging from. */
export function computeAutoPositions(
  flow: Branch,
  t: (key: string, opts?: Record<string, unknown>) => string,
  orientation: Orientation = "horizontal",
): NodePositions {
  const layout = layoutBranch(
    flow,
    40,
    300,
    // Node-name mode doesn't affect where anything is positioned (the
    // node box is a fixed size regardless of label text) — any value
    // works here.
    {
      t,
      selectedIds: new Set(),
      liveStatus: {},
      onDelete: () => {},
      edgeShape: "straight",
      nodeNameMode: "beginner",
      duplicateFunctionDefIds: new Set(),
    },
    null,
    orientation,
  );
  const positions: NodePositions = {};
  for (const node of layout.nodes) positions[node.id] = node.position;
  return positions;
}

interface CanvasProps {
  flow: Branch;
  selectedIds: string[];
  onSelectionChange: (ids: string[]) => void;
  liveStatus: Record<string, string>;
  onDeleteStep: (stepId: string) => void;
  onAddStep: (kind: FlowNode["kind"]) => void;
  onAddIntoBranch: (ownerId: string, branchKey: BranchKey, kind: FlowNode["kind"]) => void;
  onConnect: (sourceId: string, targetId: string) => void;
  onConnectBranchEntry: (ownerId: string, branchKey: BranchKey, targetId: string) => void;
  onDisconnect: (payload: DisconnectPayload) => void;
  onSetBranchEntry: (ownerId: string, branchKey: BranchKey, entryId: string) => void;
  onToggleEnabled: (stepId: string) => void;
  onToggleBreakpoint: (stepId: string) => void;
  onZoomChange: (percent: number) => void;
  edgeStyle: EdgeStyle;
  orientation: Orientation;
  positions: NodePositions;
  onPositionChange: (id: string, position: { x: number; y: number }) => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  onPaste: () => void;
  hasClipboard: boolean;
  onSelectAll: () => void;
  onArrange: () => void;
  nodeNameMode: NodeNameMode;
}

export function Canvas({
  flow,
  selectedIds,
  onSelectionChange,
  liveStatus,
  onDeleteStep,
  onAddStep,
  onAddIntoBranch,
  onConnect,
  onConnectBranchEntry,
  onDisconnect,
  onSetBranchEntry,
  onToggleEnabled,
  onToggleBreakpoint,
  onZoomChange,
  edgeStyle,
  orientation,
  positions,
  onPositionChange,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
  onPaste,
  hasClipboard,
  onSelectAll,
  onArrange,
  nodeNameMode,
}: CanvasProps) {
  const { t, i18n } = useTranslation();
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const selectedSet = useMemo(() => new Set(selectedIds), [selectedIds]);

  const duplicateFnIds = useMemo(() => duplicateFunctionDefIds(flow), [flow]);

  const layout = useMemo(
    () =>
      layoutBranch(
        flow,
        40,
        300,
        {
          t,
          selectedIds: selectedSet,
          liveStatus,
          onDelete: onDeleteStep,
          edgeShape: EDGE_SHAPE_OF[edgeStyle],
          nodeNameMode,
          duplicateFunctionDefIds: duplicateFnIds,
        },
        null,
        orientation,
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [flow, selectedSet, liveStatus, t, i18n.language, edgeStyle, orientation, nodeNameMode, duplicateFnIds],
  );

  // Nodes are always freely draggable — a node keeps its manually-set
  // position once dragged, and otherwise falls back to the tidy
  // algorithmic spot so a never-touched flow still looks sane.
  //
  // Rendered nodes live in local state (not derived fresh on every
  // render) so an active drag is applied via `applyNodeChanges` and
  // stays entirely inside this component — the parent's `positions`
  // state is only updated once, when the drag ends. Updating the
  // parent on every drag tick was re-rendering the whole app each
  // frame and fighting React Flow's own drag reconciliation, which is
  // what made nodes flicker out during a drag.
  const [rfNodes, setRfNodes] = useState<Node<TerminalNodeData>[]>([]);

  useEffect(() => {
    setRfNodes(layout.nodes.map((node) => ({ ...node, position: positions[node.id] ?? node.position })));
  }, [layout.nodes, positions]);

  const handleNodesChange: OnNodesChange<Node<TerminalNodeData>> = useCallback(
    (changes) => {
      setRfNodes((nds) => applyNodeChanges(changes, nds));
      for (const change of changes) {
        if (change.type === "position" && change.position && change.dragging === false) {
          onPositionChange(change.id, change.position);
        }
      }
    },
    [onPositionChange],
  );

  /** A `body`/`then`/`otherwise` edge is a derived "this branch starts
   *  here" anchor, not a real `Connection` — cutting it clears that
   *  branch's `entry` instead of removing a wire from `flow.connections`. */
  const BRANCH_ANCHOR_HANDLES: Record<string, BranchKey> = { body: "body", then: "then", otherwise: "otherwise", try: "try", catch: "catch" };

  // A drag starting from a branch anchor (a loop's body dot, an if's
  // yes/no, ...) sets that branch's entry instead of adding a plain
  // `Connection` — same distinction `handleEdgeContextMenu` makes when
  // cutting one of these edges back apart.
  const handleReactFlowConnect: OnConnect = (connection) => {
    if (!connection.source || !connection.target) return;
    const branchKey = connection.sourceHandle ? BRANCH_ANCHOR_HANDLES[connection.sourceHandle] : undefined;
    if (branchKey) onConnectBranchEntry(connection.source, branchKey, connection.target);
    else onConnect(connection.source, connection.target);
  };

  // Dragging a wire and releasing it precisely on the tiny target dot
  // is fiddly — this lets a drop land anywhere on the node's body too.
  // `onConnect` above still fires (and this is skipped) when the drop
  // *did* land exactly on a handle, so a connection is never made
  // twice.
  const handleConnectEnd: OnConnectEnd = (_event, connectionState) => {
    if (connectionState.toHandle || !connectionState.toNode || !connectionState.fromNode) return;
    if (connectionState.toNode.id === connectionState.fromNode.id) return;
    const fromHandleId = connectionState.fromHandle?.id;
    const branchKey = fromHandleId ? BRANCH_ANCHOR_HANDLES[fromHandleId] : undefined;
    if (branchKey) onConnectBranchEntry(connectionState.fromNode.id, branchKey, connectionState.toNode.id);
    else onConnect(connectionState.fromNode.id, connectionState.toNode.id);
  };

  const handleEdgeContextMenu: EdgeMouseHandler = (event, edge) => {
    event.preventDefault();
    const branchKey = edge.sourceHandle ? BRANCH_ANCHOR_HANDLES[edge.sourceHandle] : undefined;
    if (branchKey) {
      setMenu({
        x: event.clientX,
        y: event.clientY,
        items: [{ label: t("canvas.disconnect"), onSelect: () => onDisconnect({ kind: "entry", ownerId: edge.source, branchKey }), danger: true }],
      });
      return;
    }
    setMenu({
      x: event.clientX,
      y: event.clientY,
      items: [{ label: t("canvas.disconnect"), onSelect: () => onDisconnect({ kind: "connection", from: edge.source, fromPort: null }), danger: true }],
    });
  };

  const handleNodeClick: NodeMouseHandler = (event, node) => {
    if (event.shiftKey || event.ctrlKey || event.metaKey) {
      if (selectedSet.has(node.id)) {
        onSelectionChange(selectedIds.filter((id) => id !== node.id));
      } else {
        onSelectionChange([...selectedIds, node.id]);
      }
    } else {
      onSelectionChange([node.id]);
    }
  };

  function addItems(onPick: (kind: FlowNode["kind"]) => void): MenuItem[] {
    return [
      { label: paletteLabel(t, "wait", nodeNameMode), onSelect: () => onPick("wait") },
      { label: paletteLabel(t, "setVariable", nodeNameMode), onSelect: () => onPick("set_variable") },
      { label: paletteLabel(t, "typeText", nodeNameMode), onSelect: () => onPick("type_text") },
      { label: paletteLabel(t, "click", nodeNameMode), onSelect: () => onPick("click") },
      { label: paletteLabel(t, "moveMouse", nodeNameMode), onSelect: () => onPick("move_mouse") },
      { label: paletteLabel(t, "keyPress", nodeNameMode), onSelect: () => onPick("key_press") },
      { label: paletteLabel(t, "findImageAi", nodeNameMode), onSelect: () => onPick("find_image") },
      { label: paletteLabel(t, "ifElse", nodeNameMode), onSelect: () => onPick("if") },
      { label: paletteLabel(t, "loop", nodeNameMode), onSelect: () => onPick("loop") },
      { label: paletteLabel(t, "tryCatch", nodeNameMode), onSelect: () => onPick("try_catch") },
      { label: paletteLabel(t, "callFunction", nodeNameMode), onSelect: () => onPick("call_function") },
    ];
  }

  const handleNodeContextMenu: NodeMouseHandler = (event, node) => {
    event.preventDefault();
    const inSelection = selectedSet.has(node.id) && selectedIds.length > 1;
    if (!inSelection) onSelectionChange([node.id]);
    const flowNode = findNode(flow, node.id);
    // Operations on the node the user actually clicked come first —
    // "add a new node" is the rarer intent when right-clicking an
    // *existing* step, so those go in submenus further down instead
    // of leading the menu.
    const items: MenuItem[] = [];

    if (!inSelection) {
      const container = findContainer(flow, node.id);
      // The top-level flow's entry is always its `start` node (see
      // `ensureStartNode`) — "start from here" only applies to a
      // nested branch (a loop/if/try_catch/function's own body),
      // never to the top-level flow itself.
      if (container && container !== flow && container.entry !== node.id) {
        const owner = findBranchOwner(flow, node.id);
        if (owner) {
          items.push({
            label: t("canvas.setBranchStart"),
            onSelect: () => onSetBranchEntry(owner.ownerId, owner.branchKey, node.id),
          });
        }
      }
      items.push({
        label: flowNode?.enabled === false ? t("canvas.enableStep") : t("canvas.disableStep"),
        onSelect: () => onToggleEnabled(node.id),
      });
      items.push({
        label: flowNode?.breakpoint ? t("canvas.removeBreakpoint") : t("canvas.addBreakpoint"),
        onSelect: () => onToggleBreakpoint(node.id),
      });
    }

    items.push({ label: t("canvas.addAfter"), items: addItems((kind) => onAddStep(kind)) });
    if (flowNode?.kind === "if") {
      items.push(
        { label: t("canvas.addIntoYes"), items: addItems((kind) => onAddIntoBranch(node.id, "then", kind)) },
        { label: t("canvas.addIntoNo"), items: addItems((kind) => onAddIntoBranch(node.id, "otherwise", kind)) },
      );
    } else if (flowNode?.kind === "loop") {
      items.push({ label: t("canvas.addIntoLoop"), items: addItems((kind) => onAddIntoBranch(node.id, "body", kind)) });
    } else if (flowNode?.kind === "try_catch") {
      items.push(
        { label: t("canvas.addIntoTry"), items: addItems((kind) => onAddIntoBranch(node.id, "try", kind)) },
        { label: t("canvas.addIntoCatch"), items: addItems((kind) => onAddIntoBranch(node.id, "catch", kind)) },
      );
    } else if (flowNode?.kind === "function_def") {
      items.push({ label: t("canvas.addIntoFunction"), items: addItems((kind) => onAddIntoBranch(node.id, "body", kind)) });
    }

    if (inSelection) {
      items.push({
        label: t("canvas.deleteSelected", { count: selectedIds.length }),
        onSelect: () => selectedIds.forEach((id) => onDeleteStep(id)),
        danger: true,
      });
    } else if (flowNode?.kind !== "start") {
      items.push({ label: t("canvas.deleteStep"), onSelect: () => onDeleteStep(node.id), danger: true });
    }

    setMenu({ x: event.clientX, y: event.clientY, items });
  };

  const handlePaneContextMenu = (event: React.MouseEvent | MouseEvent) => {
    event.preventDefault();
    const mouseEvent = event as React.MouseEvent;
    const items: MenuItem[] = [
      { label: t("menubar.undo"), onSelect: canUndo ? onUndo : undefined },
      { label: t("menubar.redo"), onSelect: canRedo ? onRedo : undefined },
      { label: t("menubar.paste"), onSelect: hasClipboard ? onPaste : undefined },
      { label: t("menubar.selectAll"), onSelect: onSelectAll },
      { label: t("menubar.arrange"), onSelect: onArrange },
      { label: t("canvas.addStep"), items: addItems((kind) => onAddStep(kind)) },
    ];
    setMenu({ x: mouseEvent.clientX, y: mouseEvent.clientY, items });
  };

  return (
    <div className="canvas-wrap">
      <ReactFlow
        className="relay-flow"
        nodes={rfNodes}
        edges={layout.edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        nodesDraggable
        nodesConnectable
        connectionLineType={ConnectionLineType.Straight}
        connectionRadius={45}
        elementsSelectable
        onNodeClick={handleNodeClick}
        onNodesChange={handleNodesChange}
        onConnect={handleReactFlowConnect}
        onConnectEnd={handleConnectEnd}
        onNodeContextMenu={handleNodeContextMenu}
        onEdgeContextMenu={handleEdgeContextMenu}
        onPaneContextMenu={handlePaneContextMenu}
        onPaneClick={() => {
          onSelectionChange([]);
          setMenu(null);
        }}
        onMoveStart={() => setMenu(null)}
        onMove={(_event, viewport) => onZoomChange(Math.round(viewport.zoom * 100))}
        onInit={(instance) => onZoomChange(Math.round(instance.getZoom() * 100))}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={22} size={1} color="var(--canvas-dot)" />
      </ReactFlow>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {flow.steps.length === 0 && (
        <div className="canvas-empty">
          <p>{t("canvas.emptyTitle")}</p>
          <p className="canvas-empty-hint">{t("canvas.emptyHint")}</p>
        </div>
      )}
    </div>
  );
}
