import { useState } from "react";
import { useTranslation } from "react-i18next";
import { GitBranch, Power, Trash2 } from "lucide-react";
import {
  collectFunctionNames,
  collectVariableNames,
  describeNode,
  findNode,
  isDuplicateFunctionName,
  type Branch,
  type FlowNode,
  type NodeNameMode,
} from "../data/flowModel";
import { NodeFields } from "./inspector/NodeFields";
import { RetryFields, RETRY_EXEMPT_KINDS } from "./inspector/RetryFields";

interface InspectorProps {
  flow: Branch;
  selectedIds: string[];
  onUpdateStep: (stepId: string, updater: (node: FlowNode) => FlowNode) => void;
  onDeleteStep: (stepId: string) => void;
  onDeleteSteps: (stepIds: string[]) => void;
  onToggleEnabled: (stepId: string) => void;
  nodeNameMode: NodeNameMode;
}

export function Inspector({ flow, selectedIds, onUpdateStep, onDeleteStep, onDeleteSteps, onToggleEnabled, nodeNameMode }: InspectorProps) {
  const { t } = useTranslation();
  const [detailed, setDetailed] = useState(true);

  if (selectedIds.length === 0) {
    return (
      <div className="inspector">
        <p className="insp-empty">{t("inspector.empty")}</p>
      </div>
    );
  }

  if (selectedIds.length > 1) {
    return (
      <div className="inspector">
        <p className="insp-multi">{t("inspector.multiSelected", { count: selectedIds.length })}</p>
        <button className="insp-delete-multi" onClick={() => onDeleteSteps(selectedIds)}>
          <Trash2 size={14} strokeWidth={1.9} aria-hidden="true" />
          {t("inspector.deleteSelected", { count: selectedIds.length })}
        </button>
      </div>
    );
  }

  const node = findNode(flow, selectedIds[0]);

  if (!node) {
    return (
      <div className="inspector">
        <p className="insp-empty">{t("inspector.empty")}</p>
      </div>
    );
  }

  const { title, sub } = describeNode(node, t, nodeNameMode);

  return (
    <div className="inspector">
      <div className="insp-head">
        <div className="insp-head-icon">
          <GitBranch size={16} strokeWidth={1.9} aria-hidden="true" />
        </div>
        <div className="insp-headtext">
          <div className="insp-title">{title}</div>
          <div className="insp-kind">
            {sub} ・ #{node.id}
          </div>
        </div>
        <button
          className={`insp-power ${node.enabled ? "" : "off"}`}
          onClick={() => onToggleEnabled(node.id)}
          title={node.enabled ? t("canvas.disableStep") : t("canvas.enableStep")}
        >
          <Power size={14} strokeWidth={1.9} aria-hidden="true" />
        </button>
        {node.kind !== "start" && (
          <button className="insp-delete" onClick={() => onDeleteStep(node.id)} title={t("inspector.deleteStep")}>
            <Trash2 size={14} strokeWidth={1.9} aria-hidden="true" />
          </button>
        )}
      </div>
      <div className="switch-row insp-mode" role="group" aria-label="Inspector detail level">
        <button className={!detailed ? "active" : ""} onClick={() => setDetailed(false)}>
          {t("inspector.mode.simple")}
        </button>
        <button className={detailed ? "active" : ""} onClick={() => setDetailed(true)}>
          {t("inspector.mode.detailed")}
        </button>
      </div>

      <NodeFields
        node={node}
        detailed={detailed}
        onChange={(updater) => onUpdateStep(node.id, updater)}
        variableNames={collectVariableNames(flow)}
        functionNames={collectFunctionNames(flow)}
        isDuplicateFunctionName={node.kind === "function_def" && isDuplicateFunctionName(flow, node.id, node.name)}
      />

      {detailed && !RETRY_EXEMPT_KINDS.has(node.kind) && (
        <RetryFields node={node} onChange={(updater) => onUpdateStep(node.id, updater)} />
      )}

      <div className="field">
        <label>{t("inspector.fields.comment")}</label>
        <textarea
          className="insp-comment"
          rows={3}
          placeholder={t("inspector.fields.commentPlaceholder")}
          value={node.comment ?? ""}
          onChange={(e) => {
            const comment = e.target.value;
            onUpdateStep(node.id, (n) => ({ ...n, comment: comment || undefined }));
          }}
        />
      </div>
    </div>
  );
}
