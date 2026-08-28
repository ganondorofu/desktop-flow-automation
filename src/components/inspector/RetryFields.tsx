import { useTranslation } from "react-i18next";
import type { FlowNode } from "../../data/flowModel";

/** `start`/`stop`/`if`/`loop` never call out to the backend, so
 *  there's nothing that could actually fail (or benefit from a retry)
 *  — no point showing the retry/failure-behavior fields for them. */
export const RETRY_EXEMPT_KINDS = new Set<FlowNode["kind"]>(["start", "stop", "if", "loop", "try_catch", "function_def"]);

/** Retry count/interval and what happens once retries are exhausted —
 *  common to every actionable step (mirrors `flow_schema::RetryPolicy`),
 *  so it's rendered once here instead of duplicated into every case of
 *  `NodeFields`'s switch. */
export function RetryFields({ node, onChange }: { node: FlowNode; onChange: (updater: (n: FlowNode) => FlowNode) => void }) {
  const { t } = useTranslation();
  const maxAttempts = node.retryMaxAttempts ?? 1;
  const intervalMs = node.retryIntervalMs ?? 0;
  const onFailure = node.onFailure ?? "fail";

  return (
    <div className="insp-retry">
      <div className="insp-retry-title">{t("inspector.fields.retryTitle")}</div>
      <div className="field">
        <label>{t("inspector.fields.retryMaxAttempts")}</label>
        <input
          className="num-input"
          type="number"
          min={1}
          step={1}
          value={maxAttempts}
          onChange={(e) => {
            const retryMaxAttempts = Math.max(1, Math.round(Number(e.target.value) || 1));
            onChange((n) => ({ ...n, retryMaxAttempts }));
          }}
        />
      </div>
      {maxAttempts > 1 && (
        <div className="field">
          <label>{t("inspector.fields.retryIntervalMs")}</label>
          <input
            className="num-input"
            type="number"
            min={0}
            step={100}
            value={intervalMs}
            onChange={(e) => {
              const retryIntervalMs = Math.max(0, Number(e.target.value) || 0);
              onChange((n) => ({ ...n, retryIntervalMs }));
            }}
          />
        </div>
      )}
      <div className="field">
        <label>{t("inspector.fields.onFailure")}</label>
        <select
          className="num-input"
          value={onFailure}
          onChange={(e) => onChange((n) => ({ ...n, onFailure: e.target.value as "fail" | "skip" }))}
        >
          <option value="fail">{t("inspector.fields.onFailureFail")}</option>
          <option value="skip">{t("inspector.fields.onFailureSkip")}</option>
        </select>
      </div>
    </div>
  );
}
