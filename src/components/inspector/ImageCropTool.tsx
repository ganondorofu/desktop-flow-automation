import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

const MAX_PREVIEW_WIDTH = 320;

/** A drag-to-select crop tool for an already-loaded image (browsed
 *  from disk) — the "参照" counterpart to the screen region picker's
 *  drag-select, letting an imported screenshot be trimmed down to
 *  just the relevant icon/button instead of only usable whole. Crops
 *  client-side via an offscreen `<canvas>`; `onCropped` receives the
 *  result as base64 PNG (no `data:` prefix). */
export function ImageCropTool({
  base64Png,
  onCropped,
  onUseWholeImage,
}: {
  base64Png: string;
  onCropped: (base64Png: string) => void;
  onUseWholeImage: () => void;
}) {
  const { t } = useTranslation();
  const imgRef = useRef<HTMLImageElement>(null);
  const [naturalSize, setNaturalSize] = useState<{ width: number; height: number } | null>(null);
  const [displayWidth, setDisplayWidth] = useState(MAX_PREVIEW_WIDTH);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [current, setCurrent] = useState<{ x: number; y: number } | null>(null);
  const draggingRef = useRef(false);

  function onImageLoad() {
    const img = imgRef.current;
    if (!img) return;
    setNaturalSize({ width: img.naturalWidth, height: img.naturalHeight });
    setDisplayWidth(Math.min(MAX_PREVIEW_WIDTH, img.naturalWidth));
  }

  function clientToLocal(e: React.MouseEvent): { x: number; y: number } | null {
    const el = imgRef.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  }

  function onMouseDown(e: React.MouseEvent) {
    const p = clientToLocal(e);
    if (!p) return;
    draggingRef.current = true;
    setStart(p);
    setCurrent(p);
  }

  function onMouseMove(e: React.MouseEvent) {
    if (!draggingRef.current) return;
    const p = clientToLocal(e);
    if (p) setCurrent(p);
  }

  function onMouseUp() {
    draggingRef.current = false;
  }

  function confirmCrop() {
    if (!start || !current || !naturalSize || !imgRef.current) return;
    const scale = naturalSize.width / displayWidth;
    const left = Math.round(Math.min(start.x, current.x) * scale);
    const top = Math.round(Math.min(start.y, current.y) * scale);
    const width = Math.round(Math.abs(current.x - start.x) * scale);
    const height = Math.round(Math.abs(current.y - start.y) * scale);
    if (width < 2 || height < 2) return;

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(imgRef.current, left, top, width, height, 0, 0, width, height);
    const dataUrl = canvas.toDataURL("image/png");
    onCropped(dataUrl.slice(dataUrl.indexOf(",") + 1));
  }

  const rect =
    start && current
      ? {
          left: Math.min(start.x, current.x),
          top: Math.min(start.y, current.y),
          width: Math.abs(current.x - start.x),
          height: Math.abs(current.y - start.y),
        }
      : null;

  return (
    <div className="insp-crop-tool">
      <p className="insp-hint">{t("inspector.fields.cropHint")}</p>
      <div
        className="insp-crop-canvas"
        style={{ width: displayWidth, position: "relative", cursor: "crosshair" }}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
      >
        <img
          ref={imgRef}
          src={`data:image/png;base64,${base64Png}`}
          alt=""
          onLoad={onImageLoad}
          style={{ width: displayWidth, display: "block", userSelect: "none" }}
          draggable={false}
        />
        {rect && (
          <div
            style={{
              position: "absolute",
              left: rect.left,
              top: rect.top,
              width: rect.width,
              height: rect.height,
              border: "1.5px solid #4fd1c5",
              background: "rgba(79,209,197,0.15)",
              boxSizing: "border-box",
              pointerEvents: "none",
            }}
          />
        )}
      </div>
      <div className="insp-image-actions">
        <button type="button" className="insp-record-btn" onClick={confirmCrop} disabled={!rect || rect.width < 2 || rect.height < 2}>
          {t("inspector.fields.cropConfirm")}
        </button>
        <button type="button" className="insp-cancel-btn" onClick={onUseWholeImage}>
          {t("inspector.fields.cropUseWhole")}
        </button>
      </div>
    </div>
  );
}
