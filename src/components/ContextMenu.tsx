import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";

export interface MenuItem {
  label: string;
  onSelect?: () => void;
  items?: MenuItem[];
  danger?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/** How far from the viewport edge a menu keeps itself, in either
 *  direction — matches the canvas's own edge padding elsewhere. */
const EDGE_MARGIN = 8;

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  // Starts at the requested point; `useLayoutEffect` below clamps it to
  // the viewport before the browser paints, once the menu's actual
  // rendered size is known — so a menu opened near an edge never
  // renders (even for a frame) partly off-screen.
  const [pos, setPos] = useState({ x, y });

  useLayoutEffect(() => {
    setPos(clampToViewport(x, y, ref.current));
  }, [x, y, items]);

  useEffect(() => {
    function handlePointerDown(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as globalThis.Node)) onClose();
    }
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKey);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKey);
    };
  }, [onClose]);

  return (
    <div className="ctx-menu" style={{ left: pos.x, top: pos.y }} ref={ref}>
      {items.map((item, i) => (
        <MenuRow key={i} item={item} onClose={onClose} />
      ))}
    </div>
  );
}

/** Slides `(x, y)` back onto the viewport just far enough that a menu
 *  of `el`'s actual rendered size fits with `EDGE_MARGIN` to spare on
 *  every side it would otherwise overflow — never zooms/shrinks the
 *  menu itself, only repositions it. */
function clampToViewport(x: number, y: number, el: HTMLElement | null): { x: number; y: number } {
  if (!el) return { x, y };
  const rect = el.getBoundingClientRect();
  const maxX = window.innerWidth - rect.width - EDGE_MARGIN;
  const maxY = window.innerHeight - rect.height - EDGE_MARGIN;
  return {
    x: Math.min(Math.max(x, EDGE_MARGIN), Math.max(maxX, EDGE_MARGIN)),
    y: Math.min(Math.max(y, EDGE_MARGIN), Math.max(maxY, EDGE_MARGIN)),
  };
}

function MenuRow({ item, onClose }: { item: MenuItem; onClose: () => void }) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLDivElement>(null);
  const nestedRef = useRef<HTMLDivElement>(null);
  // A submenu is `position: fixed` (not the CSS `left: 100%` anchor
  // this used to be) so it can be clamped with the exact same
  // viewport-fitting logic as the top-level menu regardless of which
  // edge — right, left, or bottom — it would otherwise overflow.
  const [nestedPos, setNestedPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    const trigger = triggerRef.current.getBoundingClientRect();
    setNestedPos(clampToViewport(trigger.right, trigger.top - 4, nestedRef.current));
  }, [open]);

  if (item.items) {
    return (
      <div
        className="ctx-menu-item ctx-menu-submenu"
        ref={triggerRef}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
      >
        <span>{item.label}</span>
        <ChevronRight size={12} strokeWidth={2} aria-hidden="true" />
        {open && (
          <div
            className="ctx-menu ctx-menu-nested"
            ref={nestedRef}
            style={nestedPos ? { left: nestedPos.x, top: nestedPos.y } : undefined}
          >
            {item.items.map((sub, i) => (
              <MenuRow key={i} item={sub} onClose={onClose} />
            ))}
          </div>
        )}
      </div>
    );
  }

  const disabled = !item.onSelect;
  return (
    <button
      className={`ctx-menu-item ${item.danger ? "danger" : ""}`}
      disabled={disabled}
      onClick={() => {
        if (disabled) return;
        item.onSelect?.();
        onClose();
      }}
    >
      {item.label}
    </button>
  );
}
