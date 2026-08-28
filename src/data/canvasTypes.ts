import type { Edge } from "@xyflow/react";
import type { CanvasKind } from "./flowModel";

export type NodeStatus = "done" | "running" | "idle" | "error" | "paused";
export type NodeKind = CanvasKind;

export type HandleSide = "left" | "right" | "top" | "bottom";

/** An extra output handle beyond the plain default one — an `if`
 *  node's yes/no handles (which side depends on the canvas's
 *  orientation: perpendicular to whichever way the main trunk runs,
 *  since a branch has to diverge sideways from it) or a `loop` node's
 *  body handle (parallel to the trunk in horizontal mode, since the
 *  body continues the same row; perpendicular in vertical mode, since
 *  the body sits beside the trunk so the step after the loop can
 *  continue straight down). An `if`'s handles are `connectable: true`
 *  (the user drags a real wire from them, same as any other output);
 *  a `loop`'s body handle is `connectable: false` — it's a fixed
 *  anchor for the derived "the body starts here" wire, and which step
 *  that is gets set via a context-menu action instead of dragging. */
export interface BranchHandle {
  id: string;
  position: HandleSide;
  className: string;
  connectable: boolean;
  /** 0–100 along the handle's side, overriding React Flow's default
   *  50% centering — needed whenever two handles share the same side
   *  (e.g. an `if`'s yes/no both on "right") so they render as two
   *  visibly separate dots instead of stacking on top of each other. */
  offsetPercent?: number;
}

export interface TerminalNodeData extends Record<string, unknown> {
  title: string;
  sub: string;
  body: string;
  kind: NodeKind;
  /** Which way the main trunk runs — decides where the plain
   *  input/output dots sit (left/right for horizontal, top/bottom for
   *  vertical). */
  orientation: "horizontal" | "vertical";
  /** The plain "continue to the next step" output. */
  hasDefaultOutput?: boolean;
  /** Overrides where the plain output dot sits, when the orientation
   *  default (right in horizontal mode, bottom in vertical mode) would
   *  collide with a `branchHandles` entry on the same side — a `loop`
   *  node needs this in both orientations: its body handle takes
   *  whichever side matches the trunk's own direction (the body
   *  continues inline — "right" in horizontal mode, "bottom" in
   *  vertical), so the loop's own "continue after the repeat ends"
   *  output moves to the perpendicular side instead ("bottom" in
   *  horizontal mode, "right" in vertical). An `if`/`try_catch`
   *  doesn't need this — its two branches already sit on the
   *  perpendicular axis, leaving the default output's own side free. */
  outputPosition?: HandleSide;
  /** False for `start` — nothing ever wires into the flow's own
   *  entry point, so it shouldn't show an input dot to drag onto. */
  hasInput?: boolean;
  branchHandles?: BranchHandle[];
  deletable?: boolean;
  onDelete?: (nodeId: string) => void;
  status: NodeStatus;
  /** False for a "temporarily detached" step — still in the flow, but
   *  the engine skips it entirely. Rendered dimmed with a muted badge. */
  enabled: boolean;
  /** Step-through debugging pauses the run just before this step —
   *  rendered as a small marker so the user can see (and toggle) it
   *  without opening the Inspector. */
  breakpoint: boolean;
  /** A free-text sticky note — see `FlowNode.comment`'s doc comment.
   *  Rendered as a small indicator (full text on hover); edited via
   *  the Inspector, not on the canvas itself. */
  comment?: string;
}

export interface RelayEdge extends Edge {
  labelKey?: "yes" | "no";
}
