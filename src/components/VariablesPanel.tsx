import { useTranslation } from "react-i18next";
import { collectVariableNames } from "../data/flowGraph";
import type { Branch } from "../data/flowModel";

/** Lists every variable name the flow could write to (see
 *  `collectVariableNames`'s doc comment — this includes auto-generated
 *  ones like `http`'s `_status` companion or `find_image`'s
 *  `last_match_x`/`y`/`score`, not just user-named `set_variable`s),
 *  each next to its live current value during/after a run — a flat
 *  "stack trace" of the run's variable state, refreshed after every
 *  step via the `flow-variables` event `App.tsx` listens for. Values
 *  persist after the run ends (rather than clearing) so the last run's
 *  result stays inspectable until the next run starts. */
export function VariablesPanel({ flow, liveVariables, hasRun }: { flow: Branch; liveVariables: Record<string, string>; hasRun: boolean }) {
  const { t } = useTranslation();
  const names = collectVariableNames(flow).sort((a, b) => a.localeCompare(b));

  if (names.length === 0) {
    return (
      <div className="variables-panel">
        <p className="variables-empty">{t("variables.empty")}</p>
      </div>
    );
  }

  return (
    <div className="variables-panel">
      <div className="variables-head">
        <span>{t("variables.title")}</span>
        {!hasRun && <span className="variables-hint">{t("variables.notRunYet")}</span>}
      </div>
      <div className="variables-list">
        <div className="variables-row variables-row-head">
          <span>{t("variables.name")}</span>
          <span>{t("variables.value")}</span>
        </div>
        {names.map((name) => {
          const known = Object.prototype.hasOwnProperty.call(liveVariables, name);
          return (
            <div className="variables-row" key={name}>
              <span className="variables-name">{name}</span>
              <span className={known ? "variables-value" : "variables-value variables-value-unset"}>
                {known ? liveVariables[name] : t("variables.unset")}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
