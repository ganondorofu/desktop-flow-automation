import { useTranslation } from "react-i18next";
import type { WindowSelectorField } from "../../data/flowModel";
import { WindowPickerSelect } from "./pickers";

/** The window-target fields for `wait_for_window`/`focus_window`: the
 *  "pick from open windows" dropdown (always produces the combined
 *  title-then-process mode), a matching-mode dropdown, and whichever
 *  value field(s) that mode needs. Mirrors `flow_schema::WindowSelector` —
 *  offered because an exact-title-only match breaks the moment a
 *  title changes even slightly, the same "give up on textual
 *  exactness, match the underlying thing instead" idea OBS's own
 *  window-capture source offers as matching-priority modes. */
export function WindowSelectorFields({
  window,
  onChangeWindow,
}: {
  window: WindowSelectorField;
  onChangeWindow: (next: WindowSelectorField) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <div className="field">
        <WindowPickerSelect onPicked={(title, processName) => onChangeWindow({ kind: "title_then_process", title, text: "", processName })} />
      </div>
      <div className="field">
        <label>{t("inspector.fields.windowSelectorKind")}</label>
        <select
          className="num-input"
          value={window.kind}
          onChange={(e) => onChangeWindow({ ...window, kind: e.target.value as WindowSelectorField["kind"] })}
        >
          <option value="title">{t("inspector.fields.windowSelectorKindTitle")}</option>
          <option value="title_contains">{t("inspector.fields.windowSelectorKindTitleContains")}</option>
          <option value="process">{t("inspector.fields.windowSelectorKindProcess")}</option>
          <option value="title_then_process">{t("inspector.fields.windowSelectorKindTitleThenProcess")}</option>
        </select>
      </div>
      {(window.kind === "title" || window.kind === "title_then_process") && (
        <div className="field">
          <label>{t("inspector.fields.windowTitle")}</label>
          <input className="num-input" value={window.title} onChange={(e) => onChangeWindow({ ...window, title: e.target.value })} />
        </div>
      )}
      {window.kind === "title_contains" && (
        <div className="field">
          <label>{t("inspector.fields.windowTitleContains")}</label>
          <input className="num-input" value={window.text} onChange={(e) => onChangeWindow({ ...window, text: e.target.value })} />
        </div>
      )}
      {(window.kind === "process" || window.kind === "title_then_process") && (
        <div className="field">
          <label>{t("inspector.fields.windowProcessName")}</label>
          <input
            className="num-input"
            placeholder="notepad.exe"
            value={window.processName}
            onChange={(e) => onChangeWindow({ ...window, processName: e.target.value })}
          />
        </div>
      )}
    </>
  );
}
