/** An iOS/macOS-style switch standing in for a plain checkbox — the
 *  native checkbox's own box (always plain white/square in Chromium,
 *  regardless of the app's dark theme) clashed with every surrounding
 *  control, so this hides the real `<input type="checkbox">`
 *  (kept for click/keyboard/focus handling) behind a themed track +
 *  thumb pair instead. Wraps its own `<label>`, so `<Toggle
 *  label="…" .../>` reads and clicks the same way the old
 *  `<label><input type="checkbox" />text</label>` pattern did. */
export function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
}) {
  return (
    <label className="toggle-row">
      <span className="switch-toggle">
        <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
        <span className="switch-track">
          <span className="switch-thumb" />
        </span>
      </span>
      {label && <span className="toggle-label">{label}</span>}
    </label>
  );
}
