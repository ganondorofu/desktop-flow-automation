import { useTranslation } from "react-i18next";
import { X, Check, Loader2, AlertTriangle, Pause } from "lucide-react";
import type { LogEntry } from "./StatusBar";

function formatTime(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** The status bar's own log strip only ever shows the last few lines
 *  (and truncates long error text to fit one line) — this is the full
 *  history, every entry, full error messages, for when something
 *  actually went wrong and the user needs to see exactly what and
 *  where. */
export function LogViewer({ log, onClose }: { log: LogEntry[]; onClose: () => void }) {
  const { t } = useTranslation();

  return (
    <div className="log-viewer-overlay" onClick={onClose}>
      <div className="log-viewer" onClick={(e) => e.stopPropagation()}>
        <div className="log-viewer-head">
          <span>{t("logViewer.title")}</span>
          <button className="log-viewer-close" onClick={onClose} title={t("logViewer.close")} aria-label={t("logViewer.close")}>
            <X size={14} strokeWidth={2} aria-hidden="true" />
          </button>
        </div>
        <div className="log-viewer-body">
          {log.length === 0 ? (
            <p className="log-viewer-empty">{t("logViewer.empty")}</p>
          ) : (
            log.map((entry) => (
              <div className={`log-viewer-row ${entry.kind === "error" ? "log-viewer-row-error" : ""}`} key={entry.key}>
                <span className="log-viewer-time">{formatTime(entry.time)}</span>
                {/* A finished log line is a record, not a live status — none of
                 *  these icons animate here even though the same icons do
                 *  elsewhere (the toolbar, the node badges) to reflect the
                 *  flow's *current* state. A frozen spinner next to a step
                 *  that finished minutes ago reads as "still running". */}
                {entry.kind === "start" && <Loader2 size={13} strokeWidth={2.5} aria-hidden="true" />}
                {entry.kind === "done" && <Check size={13} strokeWidth={2.5} className="sb-icon-done" aria-hidden="true" />}
                {(entry.kind === "error" || entry.kind === "monitor-mismatch") && <AlertTriangle size={13} strokeWidth={2.5} aria-hidden="true" />}
                {entry.kind === "monitor-restored" && <Check size={13} strokeWidth={2.5} className="sb-icon-done" aria-hidden="true" />}
                {entry.kind === "paused" && <Pause size={13} strokeWidth={2.5} aria-hidden="true" />}
                <span className="log-viewer-text">
                  {entry.kind === "start" && t("statusbar.stepStarted", { id: entry.stepId })}
                  {entry.kind === "done" && t("statusbar.stepDone", { id: entry.stepId })}
                  {entry.kind === "error" && t("statusbar.stepFailed", { id: entry.stepId, message: entry.message })}
                  {entry.kind === "monitor-mismatch" && t("statusbar.monitorMismatchLog")}
                  {entry.kind === "monitor-restored" && t("statusbar.monitorRestoredLog")}
                  {entry.kind === "paused" && t("statusbar.pausedAtStep", { id: entry.stepId })}
                </span>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
