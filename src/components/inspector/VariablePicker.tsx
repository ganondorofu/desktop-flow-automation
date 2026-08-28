import { useTranslation } from "react-i18next";

/** A dropdown for a field whose entire value is meant to be "an
 *  existing variable's name" — not a text template that can embed a
 *  variable among other text (that's `VariableTextInput`'s job).
 *  Picking from a list of known names, the way a function call's
 *  argument is picked rather than typed, avoids the two ways typing
 *  `%value%` by hand goes wrong here: a typo that silently resolves
 *  to nothing, and not knowing what's actually available to pick
 *  from. `wrap` controls the stored value's shape: `true` writes
 *  `%name%` (an `instance` field, resolved through the same
 *  `%variable%` substitution as free text); `false` writes the bare
 *  name (`if`'s `condition.variable`, looked up directly). The
 *  current value is always shown as an option even when it isn't in
 *  `options` — a stale/renamed reference stays visible and editable
 *  instead of silently reverting to blank, mirroring how
 *  `call_function`'s own name picker already behaves. */
export function VariablePicker({
  value,
  onChangeValue,
  options,
  wrap = false,
  emptyLabel,
}: {
  value: string;
  onChangeValue: (value: string) => void;
  options: string[];
  wrap?: boolean;
  emptyLabel?: string;
}) {
  const { t } = useTranslation();
  const unwrap = (raw: string) => {
    if (!wrap) return raw;
    const trimmed = raw.trim();
    const m = trimmed.match(/^%(.+)%$/);
    return m ? m[1] : raw;
  };
  const current = unwrap(value);
  const allOptions = current && !options.includes(current) ? [current, ...options] : options;

  return (
    <select
      className="num-input"
      value={current}
      onChange={(e) => {
        const name = e.target.value;
        onChangeValue(wrap ? (name ? `%${name}%` : "") : name);
      }}
    >
      <option value="">{emptyLabel ?? t("inspector.fields.varPickerNone")}</option>
      {allOptions.map((name) => (
        <option key={name} value={name}>
          {name}
        </option>
      ))}
    </select>
  );
}
