import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import {
  primaryScaleFactor,
  resolveRegionPicker,
  subscribeRegionPicker,
  type RegionPickResult,
} from "../data/regionPicker";

type VirtualScreenBounds = { x: number; y: number; width: number; height: number };

const OVERSHOOT_MARGIN_PX = 8;

/** While the picker is in its "selecting" phase, temporarily resizes
 *  and repositions the app's own window to exactly cover the virtual
 *  desktop, strips its title bar/border, and hides it from
 *  Alt-Tab/the taskbar (`setSkipTaskbar`) — otherwise the temporarily
 *  huge, monitor-spanning main window shows up there as a strange
 *  fused-together entry, which is exactly what a real capture tool
 *  (Snipping Tool, ShareX, PAD) never lets you see.
 *
 *  Deliberately does *not* start any of this during the "countdown"
 *  phase — that was tried, to overlap the resize with the wait the
 *  user already tolerates for Alt-Tabbing away, but it backfired
 *  badly: making Relay's own window huge and undecorated *before* the
 *  countdown finishes means the screenshot it takes right after can
 *  capture Relay's own blank overlay instead of whatever the user
 *  switched to, and it reads as a confusing, unexplained window
 *  suddenly taking over the screen. The window stays exactly as it
 *  was throughout the countdown; all of this only starts once
 *  "selecting" begins, same moment the screenshot capture itself
 *  starts (kicked off together, not back-to-back — see
 *  `pickAndCropImage`).
 *
 *  Restores the window's original bounds/decorations/taskbar
 *  visibility the moment the phase changes away from "selecting"
 *  (confirm, cancel, or Escape all funnel through the same
 *  `resolveRegionPicker` call that flips the phase back to idle). */
function useFullscreenOverlayWindow(active: boolean) {
  useEffect(() => {
    if (!active) return;
    const win = getCurrentWindow();
    let disposed = false;
    let saved: { position: PhysicalPosition; size: PhysicalSize; decorated: boolean } | null = null;

    (async () => {
      const [position, size, decorated] = await Promise.all([win.outerPosition(), win.outerSize(), win.isDecorated()]);
      if (disposed) return;
      saved = { position, size, decorated };
      const exact = await invoke<VirtualScreenBounds>("virtual_screen_bounds");
      if (disposed) return;
      // Overshoot slightly rather than match exactly — the captured
      // screenshot still comes from the real (non-padded) virtual
      // screen metrics, so this only ever adds a few pixels of
      // harmless black margin (see `containObject`'s letterboxing),
      // never misaligns anything. Better that than under-covering by
      // even one pixel and leaving a sliver of real desktop clickable
      // underneath the picker.
      const bounds: VirtualScreenBounds = {
        x: exact.x - OVERSHOOT_MARGIN_PX,
        y: exact.y - OVERSHOOT_MARGIN_PX,
        width: exact.width + OVERSHOOT_MARGIN_PX * 2,
        height: exact.height + OVERSHOOT_MARGIN_PX * 2,
      };
      await win.setSkipTaskbar(true);
      await win.setDecorations(false);
      await win.setAlwaysOnTop(true);

      // Windows sometimes applies a `setPosition`/`setSize` request
      // slightly off from what was asked — often because the window
      // manager clamps/adjusts the *old*, still-small window's
      // placement before the new size takes effect — leaving a
      // sliver of real desktop exposed at an edge (clickable, so a
      // drag started there falls through to whatever's behind
      // instead of the picker). Re-apply and re-verify a few times;
      // a second explicit set reliably "sticks" where the first one
      // got adjusted.
      for (let attempt = 0; attempt < 3; attempt++) {
        if (disposed) return;
        await win.setPosition(new PhysicalPosition(bounds.x, bounds.y));
        await win.setSize(new PhysicalSize(bounds.width, bounds.height));
        const [actualPosition, actualSize] = await Promise.all([win.outerPosition(), win.outerSize()]);
        const matches =
          actualPosition.x === bounds.x &&
          actualPosition.y === bounds.y &&
          actualSize.width === bounds.width &&
          actualSize.height === bounds.height;
        if (matches) break;
      }
      await win.setFocus();
    })();

    return () => {
      disposed = true;
      void (async () => {
        await win.setAlwaysOnTop(false);
        await win.setSkipTaskbar(false);
        if (saved) {
          await win.setSize(saved.size);
          await win.setPosition(saved.position);
          await win.setDecorations(saved.decorated);
        }
      })();
    };
  }, [active]);
}

type DisplayRect = { width: number; height: number; offsetX: number; offsetY: number };

/** The screenshot is shown with `object-fit: contain` (never
 *  stretched) so a window-size/DPI mismatch on a multi-monitor setup
 *  shows as a thin letterbox bar at worst, never a warped image —
 *  this computes exactly where within its box the image is actually
 *  painted, so hit-testing and cropping can be measured against the
 *  real pixels instead of the (possibly larger) box around them. */
function containObject(box: { width: number; height: number }, naturalWidth: number, naturalHeight: number): DisplayRect {
  if (!naturalWidth || !naturalHeight || !box.width || !box.height) {
    return { width: box.width, height: box.height, offsetX: 0, offsetY: 0 };
  }
  const boxRatio = box.width / box.height;
  const naturalRatio = naturalWidth / naturalHeight;
  const width = naturalRatio > boxRatio ? box.width : box.height * naturalRatio;
  const height = naturalRatio > boxRatio ? box.width / naturalRatio : box.height;
  return { width, height, offsetX: (box.width - width) / 2, offsetY: (box.height - height) / 2 };
}

/** Renders itself (a fixed overlay inside the app's own window)
 *  whenever `pickAndCropImage`/`pickScreenRegion` is called — see
 *  `regionPicker.ts`'s doc comment for why this replaced a separate
 *  transparent overlay window. Mount this once near the root of the
 *  app; it's invisible/inert until a picker request comes in. */
export function RegionPickerHost() {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<"idle" | "countdown" | "selecting">("idle");
  const [countdownSeconds, setCountdownSeconds] = useState(0);
  const [screenshot, setScreenshot] = useState<string | null>(null);
  const [rendered, setRendered] = useState<DisplayRect | null>(null);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [current, setCurrent] = useState<{ x: number; y: number } | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const draggingRef = useRef(false);

  useEffect(
    () =>
      subscribeRegionPicker((state) => {
        setPhase(state.phase);
        setCountdownSeconds(state.countdownSeconds);
        setScreenshot(state.screenshot);
      }),
    [],
  );

  useFullscreenOverlayWindow(phase === "selecting");

  useEffect(() => {
    if (phase !== "selecting") {
      setRendered(null);
      setStart(null);
      setCurrent(null);
    }
  }, [phase]);

  // The window is still being resized/repositioned to cover the
  // virtual desktop (several async `invoke` round-trips, see
  // `useFullscreenOverlayWindow`) when the screenshot `<img>` first
  // loads, so measuring its displayed rect once on `onLoad` captured
  // the old, pre-resize layout — `confirm()` would then crop using
  // the wrong scale and silently fail. A `ResizeObserver` re-measures
  // (accounting for `object-fit: contain` letterboxing, see
  // `containObject`) whenever the box actually changes size, no
  // matter when the window finishes resizing or the image finishes
  // decoding.
  useEffect(() => {
    if (phase !== "selecting") return;
    const el = imgRef.current;
    if (!el) return;
    const measure = () => {
      const box = el.getBoundingClientRect();
      setRendered(containObject({ width: box.width, height: box.height }, el.naturalWidth, el.naturalHeight));
    };
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    measure();
    return () => observer.disconnect();
  }, [phase, screenshot]);

  useEffect(() => {
    if (phase === "idle") return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") resolveRegionPicker(null);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [phase]);

  if (phase === "idle") return null;

  if (phase === "countdown") {
    // The window is deliberately left alone during the countdown (see
    // `useFullscreenOverlayWindow`'s doc comment) — this renders at
    // whatever size/position the main window already is, not
    // fullscreen, so a small centered overlay is the right treatment
    // here (unlike the "selecting" phase's screen-spanning one).
    return (
      <div className="region-picker-backdrop">
        <p className="region-picker-hint">{t("regionPicker.countdown", { seconds: countdownSeconds })}</p>
        <div className="region-picker-actions">
          <button type="button" className="insp-cancel-btn" onClick={() => resolveRegionPicker(null)}>
            {t("regionPicker.cancel")}
          </button>
        </div>
      </div>
    );
  }

  // The window is already resizing into place (triggered by the
  // phase change to "selecting", running concurrently with this
  // screenshot capture — see `pickAndCropImage`) — show a plain black
  // fill in the meantime instead of nothing, so there's no flash of
  // whatever was behind the app before the resize finished.
  if (!screenshot) return <div className="region-picker-fullscreen" />;

  function onImageLoad() {
    const el = imgRef.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    setRendered(containObject({ width: box.width, height: box.height }, el.naturalWidth, el.naturalHeight));
  }

  function clientToLocal(e: React.MouseEvent): { x: number; y: number } | null {
    const el = imgRef.current;
    if (!el || !rendered) return null;
    const box = el.getBoundingClientRect();
    return {
      x: Math.min(Math.max(e.clientX - box.left - rendered.offsetX, 0), rendered.width),
      y: Math.min(Math.max(e.clientY - box.top - rendered.offsetY, 0), rendered.height),
    };
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

  // Releasing the mouse ends and confirms the selection in one motion
  // — no separate "use this region" button to click, matching
  // Snipping Tool/PAD (and the fact that a button floating over a
  // fullscreen, undecorated, always-on-top window was easy to miss).
  // The just-released point is read straight from the event instead
  // of the `current` state set by the last `mousemove`, since that
  // update might not have flushed yet when `mouseup` fires.
  function onMouseUp(e: React.MouseEvent) {
    draggingRef.current = false;
    if (!start) return;
    const end = clientToLocal(e);
    if (!end) return;
    setCurrent(end);
    void confirm(start, end);
  }

  async function confirm(from: { x: number; y: number }, to: { x: number; y: number }) {
    const el = imgRef.current;
    if (!el || !rendered) return;
    const left = Math.round(Math.min(from.x, to.x));
    const top = Math.round(Math.min(from.y, to.y));
    const width = Math.round(Math.abs(to.x - from.x));
    const height = Math.round(Math.abs(to.y - from.y));
    if (width < 2 || height < 2) return;

    // `from`/`to` are in the image's *displayed* pixel space (already
    // adjusted for `object-fit: contain` letterboxing by
    // `clientToLocal`) — scale up to the image's natural pixel space
    // so the embedded reference image isn't a blurry downscaled copy.
    const renderToNatural = el.naturalWidth / rendered.width;
    const naturalLeft = Math.round(left * renderToNatural);
    const naturalTop = Math.round(top * renderToNatural);
    const naturalWidth = Math.round(width * renderToNatural);
    const naturalHeight = Math.round(height * renderToNatural);

    const canvas = document.createElement("canvas");
    canvas.width = naturalWidth;
    canvas.height = naturalHeight;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.drawImage(el, naturalLeft, naturalTop, naturalWidth, naturalHeight, 0, 0, naturalWidth, naturalHeight);
    const dataUrl = canvas.toDataURL("image/png");
    const croppedPng = dataUrl.slice(dataUrl.indexOf(",") + 1);

    // The stored region is in *logical* (DPI-independent) coordinates
    // — every other stored point in a flow uses that same space — so
    // divide the natural-pixel selection by the display scale factor.
    const scale = await primaryScaleFactor();
    const region = {
      x: Math.round(naturalLeft / scale),
      y: Math.round(naturalTop / scale),
      width: Math.round(naturalWidth / scale),
      height: Math.round(naturalHeight / scale),
    };

    resolveRegionPicker({ region, croppedPng } satisfies RegionPickResult);
  }

  const rect =
    start && current
      ? {
          left: Math.min(start.x, current.x) + (rendered?.offsetX ?? 0),
          top: Math.min(start.y, current.y) + (rendered?.offsetY ?? 0),
          width: Math.abs(current.x - start.x),
          height: Math.abs(current.y - start.y),
        }
      : null;

  return (
    <div className="region-picker-fullscreen">
      <div className="region-picker-fullscreen-canvas" onMouseDown={onMouseDown} onMouseMove={onMouseMove} onMouseUp={onMouseUp}>
        <img ref={imgRef} src={`data:image/png;base64,${screenshot}`} alt="" onLoad={onImageLoad} draggable={false} />
        {!rect && <div className="region-picker-dim" />}
        {rect && <div className="region-picker-selection" style={{ left: rect.left, top: rect.top, width: rect.width, height: rect.height }} />}
      </div>
      <div className="region-picker-float-bar">
        <p className="region-picker-hint">{t("regionPicker.hint")}</p>
      </div>
    </div>
  );
}
