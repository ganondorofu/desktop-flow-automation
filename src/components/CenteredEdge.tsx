import {
  BaseEdge,
  EdgeLabelRenderer,
  Position,
  getStraightPath,
  getBezierPath,
  getSmoothStepPath,
  type EdgeProps,
  type Edge,
} from "@xyflow/react";

/** Must match the `width`/`height` set on `.react-flow__handle` in
 *  app.css — this is the correction applied below, so it only cancels
 *  the gap out exactly if it agrees with the real rendered handle size. */
const HANDLE_SIZE = 10;

export type CenteredEdgeData = { shape: "straight" | "default" | "smoothstep" };
export type CenteredEdgeType = Edge<CenteredEdgeData, "centered">;

/** React Flow's `sourceX`/`sourceY` land on a handle's outward-facing
 *  boundary (e.g. the right edge of a Position.Right dot), not its
 *  center — so a straight line visibly grazes the rim of the circle
 *  instead of passing through its middle, worse the bigger the handle
 *  or the further you zoom in. This edge nudges each endpoint back
 *  in-model — the same distance React Flow pushed the dot out — before
 *  handing off to the normal straight/bezier/smoothstep path builders,
 *  so the line always passes through the dot's true center regardless
 *  of handle size or zoom level. */
export function CenteredEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  label,
  labelStyle,
  markerEnd,
  markerStart,
  interactionWidth,
  data,
}: EdgeProps<CenteredEdgeType>) {
  // HANDLE_SIZE is a CSS dimension declared on an element that already
  // lives inside the zoomed `.react-flow__viewport`, so it's already a
  // flow-space-local length — no need to divide by zoom here (that was
  // an earlier bug: it undershot the correction at any zoom other than 1).
  const half = HANDLE_SIZE / 2;

  const [sx, sy] = nudgeToCenter(sourceX, sourceY, sourcePosition, half);
  const [tx, ty] = nudgeToCenter(targetX, targetY, targetPosition, half);

  const shape = data?.shape ?? "default";
  const [path, labelX, labelY] =
    shape === "straight"
      ? getStraightPath({ sourceX: sx, sourceY: sy, targetX: tx, targetY: ty })
      : shape === "smoothstep"
        ? getSmoothStepPath({
            sourceX: sx,
            sourceY: sy,
            sourcePosition,
            targetX: tx,
            targetY: ty,
            targetPosition,
            borderRadius: 12,
          })
        : getBezierPath({ sourceX: sx, sourceY: sy, sourcePosition, targetX: tx, targetY: ty, targetPosition });

  return (
    <>
      <BaseEdge
        id={id}
        path={path}
        style={style}
        markerEnd={markerEnd}
        markerStart={markerStart}
        interactionWidth={interactionWidth}
      />
      {label && (
        <EdgeLabelRenderer>
          <div
            className="edge-label"
            style={{
              position: "absolute",
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              ...labelStyle,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}

function nudgeToCenter(x: number, y: number, position: Position | undefined, half: number): [number, number] {
  switch (position) {
    case Position.Left:
      return [x + half, y];
    case Position.Right:
      return [x - half, y];
    case Position.Top:
      return [x, y + half];
    case Position.Bottom:
      return [x, y - half];
    default:
      return [x, y];
  }
}
