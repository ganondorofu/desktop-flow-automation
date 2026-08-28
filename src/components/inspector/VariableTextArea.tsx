import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Variable } from "lucide-react";
import { insertChipAtSelection, rebuildChipDom, saveSelectionIfInside, serializeChipDom } from "./variableChip";
import { useVariableSuggest } from "./useVariableSuggest";

/** A free-form, multi-line text field with an "insert variable" menu —
 *  Relay's answer to iOS Shortcuts' magic variables. Typing `%` then
 *  a few characters of a known variable's name live-suggests it
 *  (Tab/Enter, or a click, to accept — VSCode-style), and however a
 *  reference gets in, it renders as a single non-editable chip (like
 *  an email client turning a typed address into a pill) rather than
 *  raw `%name%` text — see `variableChip.ts`'s doc comment for why
 *  this is a `contentEditable` div with real chip nodes, not a styled
 *  overlay on top of a plain `<textarea>`. `variableNames` is every
 *  name written anywhere in the flow (see `collectVariableNames`), not
 *  just ones guaranteed to run before this step. */
export function VariableTextArea({
  value,
  onChangeValue,
  variableNames,
}: {
  value: string;
  onChangeValue: (value: string) => void;
  variableNames: string[];
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const lastEmitted = useRef<string | null>(null);
  const savedRange = useRef<Range | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el || value === lastEmitted.current) return;
    rebuildChipDom(el, value, true);
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
    <div className="insp-vartext">
      <div className="insp-var-live">
        <div
          ref={ref}
          className="num-input insp-textarea insp-var-editable insp-var-editable-area"
          contentEditable
          suppressContentEditableWarning
          onInput={() => {
            emit();
            refresh();
          }}
          onKeyUp={refresh}
          onClick={refresh}
          onBlur={close}
          onKeyDown={(e) => {
            if (handleKeyDown(e)) return;
            if (e.key === "Enter") {
              e.preventDefault();
              document.execCommand("insertLineBreak");
              emit();
            }
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
          >
            <Variable size={12} strokeWidth={1.9} aria-hidden="true" />
            {t("inspector.fields.insertVariable")}
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
