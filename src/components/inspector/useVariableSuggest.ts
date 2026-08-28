import { useCallback, useState, type KeyboardEvent, type RefObject } from "react";
import { findPendingToken, insertChipAtSelection, type PendingToken } from "./variableChip";

interface Suggest {
  token: PendingToken;
  matches: string[];
  active: number;
}

const MAX_SUGGESTIONS = 8;

/** VSCode-Tab-completion for `%variable%` references: while the
 *  cursor sits right after an unclosed `%partial` run, shows the
 *  matching variable names and turns the accepted one into a chip
 *  the instant it's chosen (Tab/Enter, or a click) — no need to type
 *  the closing `%` at all. Shared between `VariableTextInput` and
 *  `VariableTextArea` since the detection/accept logic is identical;
 *  only the surrounding field markup differs between them. */
export function useVariableSuggest(ref: RefObject<HTMLElement | null>, variableNames: string[], onAccepted: () => void) {
  const [suggest, setSuggest] = useState<Suggest | null>(null);

  const refresh = useCallback(() => {
    const el = ref.current;
    if (!el) {
      setSuggest(null);
      return;
    }
    const token = findPendingToken(el);
    if (!token) {
      setSuggest(null);
      return;
    }
    const q = token.query.toLowerCase();
    const matches = variableNames.filter((n) => n.toLowerCase().startsWith(q)).slice(0, MAX_SUGGESTIONS);
    setSuggest(matches.length > 0 ? { token, matches, active: 0 } : null);
  }, [ref, variableNames]);

  const accept = useCallback(
    (name?: string) => {
      if (!suggest) return;
      const el = ref.current;
      if (!el) return;
      insertChipAtSelection(el, name ?? suggest.matches[suggest.active], suggest.token.range);
      setSuggest(null);
      onAccepted();
    },
    [suggest, ref, onAccepted],
  );

  /** Returns true when the key was consumed by the suggestion
   *  dropdown — callers should skip their own handling of that key
   *  (e.g. `VariableTextArea`'s Enter-inserts-a-line-break) when this
   *  returns true. */
  function handleKeyDown(e: KeyboardEvent): boolean {
    if (!suggest) return false;
    if (e.key === "Tab" || e.key === "Enter") {
      e.preventDefault();
      accept();
      return true;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSuggest((s) => (s ? { ...s, active: (s.active + 1) % s.matches.length } : s));
      return true;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSuggest((s) => (s ? { ...s, active: (s.active - 1 + s.matches.length) % s.matches.length } : s));
      return true;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      setSuggest(null);
      return true;
    }
    return false;
  }

  return { suggest, refresh, accept, handleKeyDown, close: () => setSuggest(null) };
}
