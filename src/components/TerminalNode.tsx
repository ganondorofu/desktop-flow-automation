import { Handle, Position, type NodeProps, type Node } from "@xyflow/react";
import { useTranslation } from "react-i18next";
import { Check, Loader2, AlertTriangle, X, Clock, Crosshair, MousePointerClick, GitBranch, PowerOff, Pause, Circle, MessageSquare } from "lucide-react";
import type { BranchHandle, HandleSide, TerminalNodeData } from "../data/canvasTypes";

const KIND_ICON: Record<TerminalNodeData["kind"], typeof Clock> = {
  trigger: Clock,
  image: Crosshair,
  action: MousePointerClick,
  control: GitBranch,
};

const SIDE_TO_POSITION: Record<HandleSide, Position> = {
  left: Position.Left,
  right: Position.Right,
  top: Position.Top,
  bottom: Position.Bottom,
};

export function TerminalNode({ id, data, selected }: NodeProps<Node<TerminalNodeData>>) {
  const { t } = useTranslation();
  const classes = ["term-node"];
  if (selected) classes.push("selected");
  if (!data.enabled) classes.push("muted");
  if (data.status === "paused") classes.push("paused");

  const KindIcon = KIND_ICON[data.kind];
  const branchHandles = data.branchHandles ?? [];
  const inputPosition = data.orientation === "horizontal" ? Position.Left : Position.Top;
  const defaultOutputSide: HandleSide = data.orientation === "horizontal" ? "right" : "bottom";
  const outputPosition = SIDE_TO_POSITION[data.outputPosition ?? defaultOutputSide];

  /** React Flow centers a handle at 50% of its side by default — two
   *  handles sharing a side (an `if`'s yes/no both on "right") need an
   *  explicit offset or they'd render as one indistinguishable dot. */
  function offsetStyle(bh: BranchHandle) {
    if (bh.offsetPercent === undefined) return undefined;
    const axis = bh.position === "left" || bh.position === "right" ? "top" : "left";
    return { [axis]: `${bh.offsetPercent}%` };
  }

  return (
    <div className={classes.join(" ")}>
      {data.hasInput !== false && <Handle type="target" position={inputPosition} />}

      {data.breakpoint && (
        <span className="term-node-breakpoint" title={t("canvas.removeBreakpoint")}>
          <span className="sr-only">{t("canvas.removeBreakpoint")}</span>
          <Circle size={9} strokeWidth={0} fill="currentColor" aria-hidden="true" />
        </span>
      )}

      {!data.enabled && (
        <span className="term-node-badge" data-status="disabled">
          <span className="sr-only">{t("status.disabled")}</span>
          <PowerOff size={11} strokeWidth={2.5} aria-hidden="true" />
        </span>
      )}

      {data.enabled && data.status !== "idle" && (
        <span className="term-node-badge" data-status={data.status}>
          <span className="sr-only">{t(`status.${data.status}`)}</span>
          {data.status === "done" && <Check size={11} strokeWidth={2.75} aria-hidden="true" />}
          {data.status === "running" && <Loader2 size={11} strokeWidth={2.75} aria-hidden="true" />}
          {data.status === "error" && <AlertTriangle size={11} strokeWidth={2.5} aria-hidden="true" />}
          {data.status === "paused" && <Pause size={11} strokeWidth={2.75} aria-hidden="true" />}
        </span>
      )}

      {data.deletable && typeof data.onDelete === "function" && (
        <button
          className="term-node-delete"
          onClick={(e) => {
            e.stopPropagation();
            (data.onDelete as (nodeId: string) => void)(id);
          }}
          title={t("canvas.deleteStep")}
          aria-label={t("canvas.deleteStep")}
        >
          <X size={10} strokeWidth={2.5} aria-hidden="true" />
        </button>
      )}

      <div className="term-node-head">
        <div className="term-node-icon" data-kind={data.kind}>
          <KindIcon size={13} strokeWidth={1.9} aria-hidden="true" />
        </div>
        <div className="term-node-headtext">
          <div className="term-node-title" title={data.title}>{data.title}</div>
        </div>
      </div>
      <div className="term-node-body" title={data.body}>{data.body}</div>

      {data.comment && (
        <span className="term-node-comment" title={data.comment}>
          <span className="sr-only">{data.comment}</span>
          <MessageSquare size={11} strokeWidth={2} aria-hidden="true" />
        </span>
      )}

      {branchHandles.map((bh) => (
        <Handle
          key={bh.id}
          type="source"
          position={SIDE_TO_POSITION[bh.position]}
          id={bh.id}
          className={bh.className}
          isConnectable={bh.connectable}
          style={offsetStyle(bh)}
        />
      ))}
      {/* No explicit `id` here — the layout code's default-chain edges
       *  are built with `sourceHandle: undefined` (see `makeEdge`), and
       *  React Flow only matches an edge's sourceHandle to a Handle
       *  that itself has no id. Giving this an id (it used to be
       *  "out") made those edges fail to match it and fall back to
       *  rendering from whichever source handle happened to come
       *  first in the DOM — usually a branch handle on the same
       *  node, drawing the line from the wrong dot. */}
      {data.hasDefaultOutput && <Handle type="source" position={outputPosition} />}
    </div>
  );
}
