import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Variable } from "lucide-react";
import { insertChipAtSelection, rebuildChipDom, saveSelectionIfInside, serializeChipDom } from "./variableChip";
import { useVariableSuggest } from "./useVariableSuggest";

/** Single-line counterpart of `VariableTextArea` — same "insert
 *  variable" menu, live Tab-completion, and chip-editor behavior, for
 *  a one-line field (a URL, a window title, a selector value, ...)
 *  instead of a multi-line one. See `variableChip.ts`'s doc comment
 *  for why this is a `contentEditable` div with real chip nodes
 *  rather than a plain `<input>`. */
export function VariableTextInput({
  value,
  onChangeValue,
  variableNames,
  placeholder,
}: {
  value: string;
  onChangeValue: (value: string) => void;
  variableNames: string[];
  placeholder?: string;
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const lastEmitted = useRef<string | null>(null);
  const savedRange = useRef<Range | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el || value === lastEmitted.current) return;
    rebuildChipDom(el, value, false);
    lastEmitted.current = value;
  }, [value]);

  function emit() {
    const el = ref.current;
    if (!el) return;
    const next = serializeChipDom(el);
    lastEmitted.current = next;
    onChangeValue(next);
  }

  const { suggest, refresh, accept, handleKeyDown, close } = useVariableSuggest(ref, variableNames, emit);

  function insertVariable(name: string) {
    const el = ref.current;
    if (!el) return;
    insertChipAtSelection(el, name, savedRange.current);
    setMenuOpen(false);
    emit();
  }

  return (
    <div className="insp-vartext insp-vartext-inline">
      <div className="insp-var-live">
        <div
          ref={ref}
          className="num-input insp-var-editable"
          contentEditable
          suppressContentEditableWarning
          data-placeholder={placeholder}
          onInput={() => {
            emit();
            refresh();
          }}
          onKeyUp={refresh}
          onClick={refresh}
          onBlur={close}
          onKeyDown={(e) => {
            if (handleKeyDown(e)) return;
            if (e.key === "Enter") e.preventDefault();
          }}
        />
        {suggest && (
          <div className="insp-var-menu insp-var-suggest-menu">
            {suggest.matches.map((name, i) => (
              <button
                type="button"
                key={name}
                className={i === suggest.active ? "active" : ""}
                onMouseDown={(e) => {
                  e.preventDefault();
                  accept(name);
                }}
              >
                {`%${name}%`}
              </button>
            ))}
          </div>
        )}
      </div>
      {variableNames.length > 0 && (
        <div className="insp-var-insert">
          <button
            type="button"
            className="insp-var-btn"
            onClick={() => {
              savedRange.current = saveSelectionIfInside(ref.current!);
              setMenuOpen((v) => !v);
            }}
            title={t("inspector.fields.insertVariable")}
            aria-label={t("inspector.fields.insertVariable")}
          >
            <Variable size={12} strokeWidth={1.9} aria-hidden="true" />
          </button>
          {menuOpen && (
            <div className="insp-var-menu">
              {variableNames.map((name) => (
                <button type="button" key={name} onClick={() => insertVariable(name)}>
                  {`%${name}%`}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
