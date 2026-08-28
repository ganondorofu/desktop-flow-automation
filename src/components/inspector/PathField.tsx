import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { primaryScaleFactor } from "../../data/regionPicker";

/** A file/folder path text field with an Explorer "Browse…" button and
 *  native drag-and-drop — every `*_file`/`*_directory` node's path
 *  field uses this instead of a plain `<input>`.
 *
 *  Drag-and-drop deliberately doesn't use the browser's own
 *  `onDrop`/`DataTransfer` — Tauri intercepts OS-level drag-and-drop
 *  before it reaches HTML5 DnD by default (`dragDropEnabled`, on by
 *  default), and that's the *only* path that hands back a real
 *  absolute filesystem path; HTML5 `DataTransfer.files` in a webview
 *  either carries no real path at all or a sandboxed fake one, same
 *  as in a regular browser. `getCurrentWebview().onDragDropEvent`
 *  fires globally (not per-element), so this checks the drop's
 *  window-relative position against this field's own input on every
 *  drop — the input the cursor was actually over at drop time is the
 *  one that accepts it. */
export function PathField({
  value,
  onChange,
  directory = false,
}: {
  value: string;
  onChange: (path: string) => void;
  directory?: boolean;
}) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const webview = getCurrentWebview();
    let cancelled = false;
    const unlistenPromise = webview.onDragDropEvent(async (event) => {
      if (cancelled || event.payload.type !== "drop") return;
      const [path] = event.payload.paths;
      if (!path) return;
      const el = inputRef.current;
      if (!el) return;
      const scale = await primaryScaleFactor();
      const rect = el.getBoundingClientRect();
      const x = event.payload.position.x / scale;
      const y = event.payload.position.y / scale;
      if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) {
        onChange(path);
      }
    });
    return () => {
      cancelled = true;
      void unlistenPromise.then((unlisten) => unlisten());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function browse() {
    const path = await openDialog({ multiple: false, directory });
    if (typeof path === "string") onChange(path);
  }

  return (
    <div className="field-row">
      <input ref={inputRef} className="num-input insp-path-input" value={value} onChange={(e) => onChange(e.target.value)} />
      <button type="button" className="insp-record-btn" onClick={() => void browse()}>
        {t("inspector.fields.browse")}
      </button>
    </div>
  );
}
