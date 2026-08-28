import { useTranslation } from "react-i18next";
import { Activity, AlertTriangle, Check, ChevronDown, ChevronUp, Grid2x2, Loader2, Pause, Variable } from "lucide-react";
import { CopyButton } from "./CopyButton";

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
  executionWidgetOpen: boolean;
  knownVariableCount: number;
  variableCount: number;
  onToggleExecutionWidget: () => void;
}

export function StatusBar({
  runState,
  log,
  elapsedMs,
  zoomPercent,
  monitorPaused,
  debugPaused,
  executionWidgetOpen,
  knownVariableCount,
  variableCount,
  onToggleExecutionWidget,
}: StatusBarProps) {
  const { t } = useTranslation();
  const latest = log.at(-1);
  const latestText = latest
    ? latest.kind === "start"
      ? t("statusbar.stepStarted", { id: latest.stepId })
      : latest.kind === "done"
        ? t("statusbar.stepDone", { id: latest.stepId })
        : latest.kind === "error"
          ? t("statusbar.stepFailed", { id: latest.stepId, message: latest.message })
          : latest.kind === "monitor-mismatch"
            ? t("statusbar.monitorMismatchLog")
            : latest.kind === "monitor-restored"
              ? t("statusbar.monitorRestoredLog")
              : t("statusbar.pausedAtStep", { id: latest.stepId })
    : t("statusbar.idleLog");

  return (
    <div className="statusbar">
      <div className="sb-item">
        <Grid2x2 size={12} strokeWidth={1.7} aria-hidden="true" />
        <span>{t("statusbar.zoom", { percent: zoomPercent, size: 22 })}</span>
      </div>
      <div className="sb-sep" />

      <button
        type="button"
        className={`sb-execution-toggle ${runState.status === "error" ? "error" : ""}`}
        onClick={onToggleExecutionWidget}
        aria-expanded={executionWidgetOpen}
        aria-controls="execution-widget"
      >
        <Activity size={12} strokeWidth={1.9} aria-hidden="true" />
        <strong>{t("executionWidget.title")}</strong>
        <span className="sb-execution-latest">{latestText}</span>
        <span className="sb-variable-count">
          <Variable size={11} strokeWidth={1.9} aria-hidden="true" />
          {knownVariableCount}/{variableCount}
        </span>
        {executionWidgetOpen ? <ChevronDown size={13} aria-hidden="true" /> : <ChevronUp size={13} aria-hidden="true" />}
      </button>

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
            <CopyButton text={runState.message} />
          </>
        )}
        {(runState.status === "running" || runState.status === "success") && (
          <span className="sb-step">{t("statusbar.elapsed", { seconds: (elapsedMs / 1000).toFixed(1) })}</span>
        )}
      </div>
    </div>
  );
}
