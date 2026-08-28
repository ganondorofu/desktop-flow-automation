import { Toggle } from "./Toggle";

/** A step that writes one or more variables lists them here as
 *  label/name pairs instead of a plain labeled input (or, for HTTP,
 *  a prose sentence) — consistent across every variable-producing
 *  node, and each name is editable right in the list. A row without
 *  `onChange` is mechanically derived from another row and shown
 *  read-only instead. A row with `onToggleEnabled` additionally gets
 *  a switch to skip generating that variable at all — for a step like
 *  "get system info" where not every field is always wanted (no
 *  reason to spend time gathering CPU usage nobody reads). */
export function VariableOutputList({
  items,
}: {
  items: {
    label: string;
    name: string;
    onChange?: (name: string) => void;
    enabled?: boolean;
    onToggleEnabled?: (enabled: boolean) => void;
  }[];
}) {
  return (
    <dl className="var-output-list">
      {items.map((item, i) => {
        const enabled = item.enabled ?? true;
        return (
          <div className="var-output-row" key={i}>
            {item.onToggleEnabled && <Toggle checked={enabled} onChange={item.onToggleEnabled} />}
            <dt>{item.label}</dt>
            {item.onChange ? (
              <input
                className="var-output-input"
                value={item.name}
                disabled={!enabled}
                onChange={(e) => item.onChange?.(e.target.value)}
              />
            ) : (
              <dd>{item.name}</dd>
            )}
          </div>
        );
      })}
    </dl>
  );
}
