import { useTranslation } from "react-i18next";
import { Grid2x2, Check, Loader2, AlertTriangle, Pause, ScrollText } from "lucide-react";

export type RunState =
  | { status: "idle" }
  | { status: "running" }
  | { status: "success" }
  | { status: "error"; message: string };

export type LogEntry =
  | { key: string; kind: "start"; stepId: string; time: number }
  | { key: string; kind: "done"; stepId: string; time: number }
  | { key: string; kind: "error"; stepId: string; message: string; time: number }
  | { key: string; kind: "monitor-mismatch"; time: number }
  | { key: string; kind: "monitor-restored"; time: number }
  | { key: string; kind: "paused"; stepId: string; time: number };

interface StatusBarProps {
  runState: RunState;
  log: LogEntry[];
  elapsedMs: number;
  zoomPercent: number;
  monitorPaused: boolean;
  debugPaused: boolean;
  onOpenLogViewer: () => void;
}

export function StatusBar({ runState, log, elapsedMs, zoomPercent, monitorPaused, debugPaused, onOpenLogViewer }: StatusBarProps) {
  const { t } = useTranslation();

  return (
    <div className="statusbar">
      <div className="sb-item">
        <Grid2x2 size={12} strokeWidth={1.7} aria-hidden="true" />
        <span>{t("statusbar.zoom", { percent: zoomPercent, size: 22 })}</span>
      </div>
      <div className="sb-sep" />

      <button className="sb-log-viewer-btn" onClick={onOpenLogViewer} title={t("logViewer.open")} aria-label={t("logViewer.open")}>
        <ScrollText size={12} strokeWidth={1.9} aria-hidden="true" />
      </button>

      <div className="sb-log">
        {log.length === 0 ? (
          <span className="sb-log-line">{t("statusbar.idleLog")}</span>
        ) : (
          log.slice(-4).map((entry) => (
            <span className={`sb-log-line ${entry.kind === "error" ? "sb-log-error" : ""}`} key={entry.key}>
              {/* Not the animated spinner class — see `LogViewer`'s
               *  identical note: a log line (even one still within
               *  this strip's last-4 window) is a record of when a
               *  step started, not a live "is it running right now"
               *  indicator. */}
              {entry.kind === "start" && <Loader2 size={11} strokeWidth={2.5} aria-hidden="true" />}
              {entry.kind === "done" && <Check size={11} strokeWidth={2.5} className="sb-icon-done" aria-hidden="true" />}
              {(entry.kind === "error" || entry.kind === "monitor-mismatch") && (
                <AlertTriangle size={11} strokeWidth={2.5} aria-hidden="true" />
              )}
              {entry.kind === "monitor-restored" && <Check size={11} strokeWidth={2.5} className="sb-icon-done" aria-hidden="true" />}
              {entry.kind === "paused" && <Pause size={11} strokeWidth={2.5} aria-hidden="true" />}
              {entry.kind === "start" && t("statusbar.stepStarted", { id: entry.stepId })}
              {entry.kind === "done" && t("statusbar.stepDone", { id: entry.stepId })}
              {entry.kind === "error" && t("statusbar.stepFailed", { id: entry.stepId, message: entry.message })}
              {entry.kind === "monitor-mismatch" && t("statusbar.monitorMismatchLog")}
              {entry.kind === "monitor-restored" && t("statusbar.monitorRestoredLog")}
              {entry.kind === "paused" && t("statusbar.pausedAtStep", { id: entry.stepId })}
            </span>
          ))
        )}
      </div>

      <div className="sb-right">
        {runState.status === "running" && monitorPaused && (
          <>
            <AlertTriangle size={12} strokeWidth={2.5} className="sb-icon-monitor-paused" aria-hidden="true" />
            <span className="sb-monitor-paused">{t("statusbar.monitorPaused")}</span>
          </>
        )}
        {runState.status === "running" && !monitorPaused && debugPaused && (
          <>
            <Pause size={12} strokeWidth={2.5} className="sb-icon-monitor-paused" aria-hidden="true" />
            <span className="sb-monitor-paused">{t("statusbar.debugPaused")}</span>
          </>
        )}
        {runState.status === "running" && !monitorPaused && !debugPaused && (
          <>
            <Loader2 size={12} strokeWidth={2.5} className="sb-icon-running" aria-hidden="true" />
            <span>{t("statusbar.backendRunning")}</span>
          </>
        )}
        {runState.status === "success" && (
          <>
            <Check size={12} strokeWidth={2.5} className="sb-icon-done" aria-hidden="true" />
            <span>{t("statusbar.backendSuccess")}</span>
          </>
        )}
        {runState.status === "error" && (
          <>
            <AlertTriangle size={12} strokeWidth={2.5} aria-hidden="true" />
            <span>{t("statusbar.backendError", { message: runState.message })}</span>
          </>
        )}
        {(runState.status === "running" || runState.status === "success") && (
          <span className="sb-step">{t("statusbar.elapsed", { seconds: (elapsedMs / 1000).toFixed(1) })}</span>
        )}
      </div>
    </div>
  );
}
