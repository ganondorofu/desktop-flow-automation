import { useTranslation } from "react-i18next";
import type { BrowserSelectorField } from "../../data/flowModel";
import { PickBrowserElementButton } from "./pickers";

/** The selector fields for every `Browser*` step: the pick-on-page
 *  button (always produces a CSS selector), a strategy dropdown, and
 *  whichever value field(s) that strategy needs. Mirrors
 *  `flow_schema::BrowserSelector` — offered as an alternative to CSS
 *  because a page redesign can change class names out from under a
 *  CSS selector while the element's own visible text or a semantic
 *  attribute stays put, the way PAD's own element descriptor matches
 *  on more than one property. */
export function BrowserSelectorFields({
  selector,
  onChangeSelector,
}: {
  selector: BrowserSelectorField;
  onChangeSelector: (next: BrowserSelectorField) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <div className="field">
        <PickBrowserElementButton onPicked={(value) => onChangeSelector({ kind: "css", value, name: "" })} />
      </div>
      <div className="field">
        <label>{t("inspector.fields.selectorKind")}</label>
        <select
          className="num-input"
          value={selector.kind}
          onChange={(e) => onChangeSelector({ ...selector, kind: e.target.value as BrowserSelectorField["kind"] })}
        >
          <option value="css">{t("inspector.fields.selectorKindCss")}</option>
          <option value="text">{t("inspector.fields.selectorKindText")}</option>
          <option value="attribute">{t("inspector.fields.selectorKindAttribute")}</option>
        </select>
      </div>
      {selector.kind === "attribute" && (
        <div className="field">
          <label>{t("inspector.fields.attributeName")}</label>
          <input
            className="num-input"
            value={selector.name}
            onChange={(e) => onChangeSelector({ ...selector, name: e.target.value })}
          />
        </div>
      )}
      <div className="field">
        <label>
          {selector.kind === "css"
            ? t("inspector.fields.selector")
            : selector.kind === "text"
              ? t("inspector.fields.selectorValueText")
              : t("inspector.fields.selectorValueAttribute")}
        </label>
        <input
          className="num-input"
          value={selector.value}
          onChange={(e) => onChangeSelector({ ...selector, value: e.target.value })}
        />
      </div>
    </>
  );
}
