/** The state/promise plumbing behind the in-app region picker
 *  (`RegionPickerHost.tsx`) — a Snip & Sketch-style drag-select shown
 *  *inside* the app's own window over a captured screenshot, not a
 *  separate transparent overlay window. That approach (a fresh
 *  always-on-top window created at runtime) went through several
 *  rendering/input failure modes on Windows before being replaced:
 *  WebView2 only supports fully-transparent or fully-opaque pixels
 *  (no real alpha blending), a URL-hash routing scheme silently
 *  failed to load any content, and a visibility/paint-confirmation
 *  handshake meant to guard against that starved itself because
 *  hidden windows don't reliably fire animation frames. Doing the
 *  whole thing in one already-working window sidesteps all of it.
 *
 *  The screenshot itself covers the whole virtual desktop (every
 *  connected monitor, not just primary) and is taken after a short
 *  countdown — like Power Automate Desktop's picker, so the user can
 *  Alt-Tab to whatever window (or monitor) they actually want to
 *  capture from before Relay takes the screenshot, instead of only
 *  ever capturing whatever's on screen (Relay itself included) the
 *  instant the button is pressed.
 *
 *  `pickScreenRegion`/`pickAndCropImage` are the two things callers
 *  actually want: a region spec (for `find_text_ocr`'s scan area) or
 *  a cropped PNG (for embedding a `find_image` reference image) —
 *  both drive the exact same on-screen picker, just reading a
 *  different part of its result. */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type ScreenRegion = { x: number; y: number; width: number; height: number };
export type RegionPickResult = { region: ScreenRegion; croppedPng: string };

export const CAPTURE_DELAY_MS = 3000;

type PickerPhase = "idle" | "countdown" | "selecting";
type PickerState = { phase: PickerPhase; countdownSeconds: number; screenshot: string | null };
type Listener = (state: PickerState) => void;

let state: PickerState = { phase: "idle", countdownSeconds: 0, screenshot: null };
const listeners = new Set<Listener>();
let pendingResolve: ((result: RegionPickResult | null) => void) | null = null;

function setState(next: Partial<PickerState>) {
  state = { ...state, ...next };
  listeners.forEach((listener) => listener(state));
}

/** Used only by `RegionPickerHost` to know when to render itself. */
export function subscribeRegionPicker(listener: Listener): () => void {
  listeners.add(listener);
  listener(state);
  return () => listeners.delete(listener);
}

/** Counts down `delayMs` (giving the user time to Alt-Tab to their
 *  actual target), captures the whole virtual desktop, then shows the
 *  drag-select picker over it — resolving with both the
 *  logical-coordinate region and that region's already-cropped PNG
 *  once the user confirms a selection, or `null` if they cancel
 *  (Escape, or the cancel button). Only one picker can be active at a
 *  time — a second call while one is already up resolves the first
 *  with `null` rather than leaving it orphaned. */
export async function pickAndCropImage(delayMs = CAPTURE_DELAY_MS): Promise<RegionPickResult | null> {
  if (state.phase !== "idle") {
    resolveRegionPicker(null);
  }

  setState({ phase: "countdown", countdownSeconds: Math.ceil(delayMs / 1000), screenshot: null });
  const tick = setInterval(() => {
    setState({ countdownSeconds: Math.max(0, state.countdownSeconds - 1) });
  }, 1000);
  await new Promise((resolve) => setTimeout(resolve, delayMs));
  clearInterval(tick);

  // Flip to "selecting" *before* the screenshot comes back, not
  // after — `RegionPickerHost`'s window resize/undecorate/focus
  // chain (several `invoke` round-trips of its own) is triggered by
  // this phase change, so starting it now lets it run concurrently
  // with the capture instead of only starting once the capture (and
  // its IPC transfer, which isn't small for a multi-monitor PNG) has
  // already finished — that back-to-back sequencing was the whole
  // delay the user was seeing between the countdown hitting zero and
  // the picker actually becoming usable.
  setState({ phase: "selecting", screenshot: null });

  try {
    const screenshot = await invoke<string>("capture_full_screen_base64");
    // The user likely Alt-Tabbed away during the countdown — bring
    // Relay back to the foreground so the crop overlay is visible.
    await getCurrentWindow().setFocus();
    setState({ screenshot });
  } catch (error) {
    setState({ phase: "idle", countdownSeconds: 0, screenshot: null });
    throw error;
  }

  return new Promise((resolve) => {
    pendingResolve = resolve;
  });
}

/** Same picker, but callers that only need the region spec (not the
 *  cropped pixels) — `find_text_ocr`'s scan-area field. */
export async function pickScreenRegion(): Promise<ScreenRegion | null> {
  const result = await pickAndCropImage();
  return result?.region ?? null;
}

/** Called by `RegionPickerHost` once the user confirms or cancels. */
export function resolveRegionPicker(result: RegionPickResult | null) {
  setState({ phase: "idle", countdownSeconds: 0, screenshot: null });
  const resolve = pendingResolve;
  pendingResolve = null;
  resolve?.(result);
}

/** The DPI scale factor (physical pixels per logical pixel) needed to
 *  convert a selection made on the screenshot (which is captured at
 *  physical resolution) back into the logical, DPI-independent
 *  coordinates every stored point in a flow uses. */
export async function primaryScaleFactor(): Promise<number> {
  return getCurrentWindow().scaleFactor();
}
