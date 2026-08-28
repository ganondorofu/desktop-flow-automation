import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Crosshair } from "lucide-react";
import { pickAndCropImage } from "../../data/regionPicker";

export const RECORD_DELAY_MS = 3000;

/** Countdown-based "record position" button: the user clicks it, moves
 *  the mouse to the target over 3 seconds, and the backend samples the
 *  cursor position once the countdown ends via `cursor_position_after_delay`
 *  — no drag-select overlay needed for the common "point at a spot" case. */
export function RecordPositionButton({ onRecorded }: { onRecorded: (x: number, y: number) => void }) {
  const { t } = useTranslation();
  const [countdown, setCountdown] = useState<number | null>(null);

  async function start() {
    setCountdown(Math.ceil(RECORD_DELAY_MS / 1000));
    const tick = setInterval(() => {
      setCountdown((prev) => (prev !== null && prev > 1 ? prev - 1 : prev));
    }, 1000);
    try {
      const [x, y] = await invoke<[number, number]>("cursor_position_after_delay", { delayMs: RECORD_DELAY_MS });
      onRecorded(x, y);
    } finally {
      clearInterval(tick);
      setCountdown(null);
    }
  }

  return (
    <button type="button" className="insp-record-btn" onClick={start} disabled={countdown !== null}>
      <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
      {countdown !== null ? t("inspector.fields.recordingCountdown", { seconds: countdown }) : t("inspector.fields.recordPosition")}
    </button>
  );
}

type CaptureStage = "idle" | "picking" | "done" | "error";

/** Opens the Snip & Sketch-style drag-select overlay (`pickAndCropImage`),
 *  which crops the selected region client-side and hands back its PNG
 *  bytes as base64 directly — replaces an earlier two-corner countdown
 *  flow that was hard to aim precisely. The image is embedded straight
 *  into the flow (see `ImageSourceField`), not saved to a path the
 *  user has to keep track of separately. */
export function CaptureRegionButton({ onCaptured }: { onCaptured: (base64Png: string) => void }) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<CaptureStage>("idle");

  async function start() {
    setStage("picking");
    try {
      const result = await pickAndCropImage();
      if (!result) {
        setStage("idle");
        return;
      }
      onCaptured(result.croppedPng);
      setStage("done");
    } catch (error) {
      console.error("region capture failed", error);
      setStage("error");
    }
  }

  return (
    <div className="insp-capture">
      <button type="button" className="insp-record-btn" onClick={start} disabled={stage === "picking"}>
        <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
        {stage === "picking" ? t("inspector.fields.capturePicking") : t("inspector.fields.captureRegion")}
      </button>
      {stage === "done" && <span className="insp-capture-status">{t("inspector.fields.captureSaved")}</span>}
      {stage === "error" && <span className="insp-capture-status insp-capture-error">{t("inspector.fields.captureFailed")}</span>}
    </div>
  );
}

/** Countdown-based desktop UI element picker (same primitive as
 *  `RecordPositionButton`): the user clicks it, hovers the control
 *  they mean over 3 seconds, and the backend hit-tests whatever's
 *  under the cursor via UI Automation once the countdown ends —
 *  Relay's answer to PAD's "point at the control" element picker,
 *  without needing a global mouse hook. */
export function PickUiElementButton({
  onPicked,
}: {
  onPicked: (windowTitle: string, elementName: string, automationId: string) => void;
}) {
  const { t } = useTranslation();
  const [countdown, setCountdown] = useState<number | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const requestToken = useRef(0);

  async function start() {
    setFailed(false);
    setCountdown(Math.ceil(RECORD_DELAY_MS / 1000));
    const token = ++requestToken.current;
    const tick = setInterval(() => {
      setCountdown((prev) => (prev !== null && prev > 1 ? prev - 1 : prev));
    }, 1000);
    try {
      const picked = await invoke<{ window_title: string; element_name: string; automation_id: string; preview: string }>(
        "pick_ui_element_after_delay",
        { delayMs: RECORD_DELAY_MS },
      );
      if (token !== requestToken.current) return;
      onPicked(picked.window_title, picked.element_name, picked.automation_id);
      setPreview(picked.preview);
    } catch {
      if (token !== requestToken.current) return;
      setFailed(true);
    } finally {
      if (token === requestToken.current) setCountdown(null);
      clearInterval(tick);
    }
  }

  function cancel() {
    requestToken.current += 1;
    setCountdown(null);
  }

  return (
    <div className="insp-capture">
      {countdown !== null ? (
        <>
          <button type="button" className="insp-record-btn" disabled>
            <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
            {t("inspector.fields.recordingCountdown", { seconds: countdown })}
          </button>
          <button type="button" className="insp-cancel-btn" onClick={cancel}>
            {t("inspector.fields.cancelPick")}
          </button>
        </>
      ) : (
        <button type="button" className="insp-record-btn" onClick={start}>
          <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
          {t("inspector.fields.pickUiElement")}
        </button>
      )}
      {preview && !failed && <span className="insp-capture-status">{preview}</span>}
      {failed && <span className="insp-capture-status insp-capture-error">{t("inspector.fields.pickFailed")}</span>}
    </div>
  );
}

/** Asks the browser extension to enter "click something on the page"
 *  mode and waits for the picked element's CSS selector — the browser
 *  counterpart to `PickUiElementButton`, mirroring PAD's own web
 *  element picker instead of asking the user to hand-write a CSS
 *  selector. */
export function PickBrowserElementButton({ onPicked }: { onPicked: (selector: string) => void }) {
  const { t } = useTranslation();
  const [picking, setPicking] = useState(false);
  const [preview, setPreview] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const requestToken = useRef(0);

  async function start() {
    setFailed(null);
    setPicking(true);
    const token = ++requestToken.current;
    try {
      const picked = await invoke<{ selector: string; preview: string }>("browser_pick_element");
      if (token !== requestToken.current) return;
      onPicked(picked.selector);
      setPreview(picked.preview);
    } catch (e) {
      if (token !== requestToken.current) return;
      setFailed(String(e));
    } finally {
      if (token === requestToken.current) setPicking(false);
    }
  }

  function cancel() {
    requestToken.current += 1;
    setPicking(false);
    void invoke("browser_cancel_pick").catch(() => {});
  }

  return (
    <div className="insp-capture">
      {picking ? (
        <>
          <button type="button" className="insp-record-btn" disabled>
            <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
            {t("inspector.fields.pickingOnPage")}
          </button>
          <button type="button" className="insp-cancel-btn" onClick={cancel}>
            {t("inspector.fields.cancelPick")}
          </button>
        </>
      ) : (
        <button type="button" className="insp-record-btn" onClick={start}>
          <Crosshair size={13} strokeWidth={1.9} aria-hidden="true" />
          {t("inspector.fields.pickOnPage")}
        </button>
      )}
      {preview && !failed && <span className="insp-capture-status">{preview}</span>}
      {failed && <span className="insp-capture-status insp-capture-error">{failed}</span>}
    </div>
  );
}
