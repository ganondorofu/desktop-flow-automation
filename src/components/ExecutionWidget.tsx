import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Activity, AlertTriangle, Check, Loader2, LocateFixed, Pause, ScrollText, Variable, X } from "lucide-react";
import { collectVariableDefinitions, findNode } from "../data/flowGraph";
import { describeNode, type Branch } from "../data/flowModel";
import type { LogEntry, RunState } from "./StatusBar";
import { CopyButton } from "./CopyButton";

type ExecutionWidgetTab = "variables" | "log";

interface ExecutionWidgetProps {
  open: boolean;
  flow: Branch;
  liveVariables: Record<string, string>;
  hasRun: boolean;
  log: LogEntry[];
  runState: RunState;
  elapsedMs: number;
  onClose: () => void;
  onOpenNode: (nodeId: string) => void;
  /** Pointer handlers for the top-edge drag handle that resizes this
   *  panel's height — owned by `App.tsx` since the actual height it
   *  changes lives on `.app`'s grid, outside this component. */
  onResizePointerDown: (event: React.PointerEvent) => void;
  onResizePointerMove: (event: React.PointerEvent) => void;
  onResizePointerUp: (event: React.PointerEvent) => void;
}

function formatTime(ms: number): string {
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function ExecutionWidget({
  open,
  flow,
  liveVariables,
  hasRun,
  log,
  runState,
  elapsedMs,
  onClose,
  onOpenNode,
  onResizePointerDown,
  onResizePointerMove,
  onResizePointerUp,
}: ExecutionWidgetProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<ExecutionWidgetTab>("variables");
  const [changedNames, setChangedNames] = useState<Set<string>>(new Set());
  const previousVariables = useRef<Record<string, string>>({});
  const logBodyRef = useRef<HTMLDivElement>(null);

  const variables = useMemo(
    () =>
      collectVariableDefinitions(flow)
        .map((definition) => {
          const sourceId = definition.sourceNodeIds[0];
          const sourceNode = sourceId ? findNode(flow, sourceId) : null;
          return {
            ...definition,
            sourceId,
            sourceLabel: sourceNode ? describeNode(sourceNode, t).title : "",
          };
        })
        .sort((a, b) => a.name.localeCompare(b.name)),
    [flow, t],
  );

  useEffect(() => {
    const previous = previousVariables.current;
    const changed = Object.keys(liveVariables).filter(
      (name) => !Object.prototype.hasOwnProperty.call(previous, name) || previous[name] !== liveVariables[name],
    );
    previousVariables.current = liveVariables;
    if (changed.length === 0) return;
    setChangedNames(new Set(changed));
    const timeout = window.setTimeout(() => setChangedNames(new Set()), 1200);
    return () => window.clearTimeout(timeout);
  }, [liveVariables]);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose, open]);

  useEffect(() => {
    if (!open || tab !== "log") return;
    logBodyRef.current?.scrollTo({ top: logBodyRef.current.scrollHeight });
  }, [log, open, tab]);

  if (!open) return null;

  const knownCount = variables.filter(({ name }) => Object.prototype.hasOwnProperty.call(liveVariables, name)).length;
  const stateKey = runState.status === "running" ? "running" : runState.status === "success" ? "success" : runState.status === "error" ? "error" : "idle";

  return (
    <aside id="execution-widget" className="execution-widget" aria-label={t("executionWidget.title")}>
      <div
        className="resize-handle resize-handle-y"
        role="separator"
        aria-orientation="horizontal"
        aria-label={t("executionWidget.resizeHandle")}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
      />
      <header className="execution-widget-head">
        <div className={`execution-widget-state ${stateKey}`}>
          {runState.status === "running" && <Loader2 size={15} strokeWidth={2.3} aria-hidden="true" />}
          {runState.status === "success" && <Check size={15} strokeWidth={2.3} aria-hidden="true" />}
          {runState.status === "error" && <AlertTriangle size={15} strokeWidth={2.3} aria-hidden="true" />}
          {runState.status === "idle" && <Activity size={15} strokeWidth={1.9} aria-hidden="true" />}
          <strong>{t(`executionWidget.state.${stateKey}`)}</strong>
          {(runState.status === "running" || runState.status === "success") && (
            <span>{t("statusbar.elapsed", { seconds: (elapsedMs / 1000).toFixed(1) })}</span>
          )}
        </div>

        <div className="execution-widget-tabs" role="tablist" aria-label={t("executionWidget.tabsLabel")}>
          <button type="button" role="tab" aria-selected={tab === "variables"} className={tab === "variables" ? "active" : ""} onClick={() => setTab("variables")}>
            <Variable size={14} strokeWidth={1.9} aria-hidden="true" />
            <span className="execution-tab-label">{t("executionWidget.variablesTab")}</span>
            <span className="execution-tab-count">{knownCount}/{variables.length}</span>
          </button>
          <button type="button" role="tab" aria-selected={tab === "log"} className={tab === "log" ? "active" : ""} onClick={() => setTab("log")}>
            <ScrollText size={14} strokeWidth={1.9} aria-hidden="true" />
            <span className="execution-tab-label">{t("executionWidget.logTab")}</span>
            <span className="execution-tab-count">{log.length}</span>
          </button>
        </div>

        <button type="button" className="execution-widget-close" onClick={onClose} title={t("executionWidget.close")} aria-label={t("executionWidget.close")}>
          <X size={15} strokeWidth={2} aria-hidden="true" />
        </button>
      </header>

      {tab === "variables" ? (
        <div className="execution-widget-body execution-variable-trace" role="tabpanel">
          <div className="execution-widget-toolbar">
            <span>{hasRun ? t("executionWidget.variablesLiveHint") : t("executionWidget.variablesBeforeRun")}</span>
          </div>
          {variables.length === 0 ? (
            <p className="execution-widget-empty">{t("executionWidget.noVariables")}</p>
          ) : (
            <table className="execution-variable-table">
              <thead>
                <tr>
                  <th scope="col">{t("variables.name")}</th>
                  <th scope="col">{t("variables.value")}</th>
                  <th scope="col">{t("variables.source")}</th>
                </tr>
              </thead>
              <tbody>
                {variables.map((variable) => {
                  const known = Object.prototype.hasOwnProperty.call(liveVariables, variable.name);
                  const value = known ? liveVariables[variable.name] : "";
                  return (
                    <tr key={variable.name} className={changedNames.has(variable.name) ? "changed" : ""}>
                      <td><code>{variable.name}</code></td>
                      <td>
                        <code className={!known ? "unset" : value === "" ? "empty" : ""}>
                          {!known ? t("variables.unset") : value === "" ? t("variables.emptyValue") : value}
                        </code>
                      </td>
                      <td>
                        {variable.sourceId && (
                          <button type="button" className="execution-source" onClick={() => onOpenNode(variable.sourceId)} title={t("variables.openSource", { source: variable.sourceLabel })}>
                            <span>{variable.sourceLabel}</span>
                            <LocateFixed size={13} strokeWidth={1.9} aria-hidden="true" />
                          </button>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      ) : (
        <div className="execution-widget-body execution-log" role="tabpanel" ref={logBodyRef}>
          {log.length === 0 ? (
            <p className="execution-widget-empty">{t("executionWidget.noLogs")}</p>
          ) : (
            log.map((entry) => {
              const targetId = "stepId" in entry ? entry.stepId : null;
              const targetExists = targetId !== null && findNode(flow, targetId) !== null;
              const rowClass = `execution-log-row ${entry.kind === "error" ? "error" : ""}${targetExists ? " clickable" : ""}`;
              const rowContent = (
                <>
                  <span className="execution-log-time">{formatTime(entry.time)}</span>
                  {entry.kind === "start" && <Loader2 size={13} strokeWidth={2.3} aria-hidden="true" />}
                  {entry.kind === "done" && <Check size={13} strokeWidth={2.3} className="sb-icon-done" aria-hidden="true" />}
                  {(entry.kind === "error" || entry.kind === "monitor-mismatch") && <AlertTriangle size={13} strokeWidth={2.3} aria-hidden="true" />}
                  {entry.kind === "monitor-restored" && <Check size={13} strokeWidth={2.3} className="sb-icon-done" aria-hidden="true" />}
                  {entry.kind === "paused" && <Pause size={13} strokeWidth={2.3} aria-hidden="true" />}
                  <span className="execution-log-message">
                    <span>
                      {entry.kind === "start" && t("statusbar.stepStarted", { id: entry.stepId })}
                      {entry.kind === "done" && t("statusbar.stepDone", { id: entry.stepId })}
                      {entry.kind === "error" && t("statusbar.stepFailed", { id: entry.stepId, message: entry.message })}
                      {entry.kind === "monitor-mismatch" && t("statusbar.monitorMismatchLog")}
                      {entry.kind === "monitor-restored" && t("statusbar.monitorRestoredLog")}
                      {entry.kind === "paused" && t("statusbar.pausedAtStep", { id: entry.stepId })}
                    </span>
                    {entry.kind === "error" && <CopyButton text={entry.message} />}
                  </span>
                </>
              );
              // A log row for a step no longer in the flow (deleted
              // since the run that produced it) stays a plain,
              // unclickable row — nothing for `onOpenNode` to select.
              return targetExists ? (
                <button type="button" className={rowClass} key={entry.key} onClick={() => onOpenNode(targetId)} title={t("executionWidget.jumpToStep")}>
                  {rowContent}
                </button>
              ) : (
                <div className={rowClass} key={entry.key}>
                  {rowContent}
                </div>
              );
            })
          )}
        </div>
      )}
    </aside>
  );
}
