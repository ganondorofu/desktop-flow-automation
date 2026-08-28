/** Shared logic behind `VariableTextInput`/`VariableTextArea`'s
 *  chip-based editor — a `contentEditable` div where a confirmed
 *  `%name%` reference renders as a single non-editable "block"
 *  (`contentEditable="false"`, so Backspace/Delete removes it whole),
 *  the same idea as a mail client turning a typed address into a
 *  pill once it resolves to a contact.
 *
 *  This replaced an earlier "real input with `color: transparent` +
 *  a colored text overlay stacked on top" approach that looked right
 *  in isolated testing but broke three real, load-bearing browser
 *  behaviors at once in the actual app: the native text-selection
 *  highlight (browsers paint `::selection` outside the normal
 *  per-property CSS cascade, so overriding `color` there silently
 *  zeroed out `background-color` too), pixel-accurate cursor
 *  positioning (a `<div>` overlay can only *approximate* an
 *  `<input>`/`<textarea>`'s native text metrics, never guarantee an
 *  exact match), and native undo/redo (a fully browser-value-driven
 *  overlay still sits on top of a React-controlled element, and nothing
 *  here changes that). A real DOM node per chip sidesteps all three:
 *  there's only one rendered text layer, so selection/cursor accuracy
 *  is entirely native, and — critically for undo/redo — this module
 *  only ever touches the DOM imperatively for *external* changes (see
 *  `lastEmitted` in the components), leaving ordinary typing to the
 *  browser's own `contentEditable` undo stack untouched. */

/** `%`, then one or more non-`%`/non-whitespace characters, then `%` —
 *  the same shape `resolve()` (crates/engine/src/runner.rs) accepts. */
const TOKEN_RE = /%[^%\s]+%/g;

export type VariableChipDatasetKey = "varName";
export const CHIP_DATA_ATTR = "varName" satisfies VariableChipDatasetKey;

export function isChipElement(node: Node): node is HTMLElement {
  return node instanceof HTMLElement && node.dataset[CHIP_DATA_ATTR] !== undefined;
}

function makeChip(name: string): HTMLSpanElement {
  const span = document.createElement("span");
  span.className = "var-chip";
  span.contentEditable = "false";
  span.dataset[CHIP_DATA_ATTR] = name;
  span.textContent = name;
  return span;
}

/** Rebuilds `el`'s children from `value` — used only for changes that
 *  didn't originate from the user typing into `el` itself (initial
 *  mount, an external prop change like undo/redo of the flow graph).
 *  Ordinary typing never calls this, so it never fights the browser's
 *  own undo stack or cursor tracking. */
export function rebuildChipDom(el: HTMLElement, value: string, multiline: boolean) {
  el.textContent = "";
  let lastIndex = 0;
  TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  const appendText = (text: string) => {
    if (!multiline) {
      if (text) el.appendChild(document.createTextNode(text));
      return;
    }
    const lines = text.split("\n");
    lines.forEach((line, i) => {
      if (i > 0) el.appendChild(document.createElement("br"));
      if (line) el.appendChild(document.createTextNode(line));
    });
  };
  while ((match = TOKEN_RE.exec(value))) {
    if (match.index > lastIndex) appendText(value.slice(lastIndex, match.index));
    el.appendChild(makeChip(match[0].slice(1, -1)));
    lastIndex = match.index + match[0].length;
  }
  appendText(value.slice(lastIndex));
}

/** The inverse of `rebuildChipDom` — reads `el`'s current DOM back
 *  into the `%name%`-string shape the engine's `resolve()` expects. */
export function serializeChipDom(el: HTMLElement): string {
  let out = "";
  for (const node of Array.from(el.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      out += node.textContent ?? "";
    } else if (node instanceof HTMLElement && isChipElement(node)) {
      out += `%${node.dataset[CHIP_DATA_ATTR]}%`;
    } else if (node instanceof HTMLElement && node.tagName === "BR") {
      out += "\n";
    } else {
      out += node.textContent ?? "";
    }
  }
  return out;
}

/** Inserts a chip for `name` at the current cursor position (falling
 *  back to the end of the content if the editor never had a tracked
 *  selection, e.g. the "insert variable" button was clicked before
 *  the field was ever focused), then places the cursor right after
 *  it — mirrors how the old plain-text insert always left the cursor
 *  immediately past the inserted token. */
export function insertChipAtSelection(el: HTMLElement, name: string, savedRange: Range | null) {
  el.focus();
  const selection = window.getSelection();
  let range = savedRange;
  if (!range || !el.contains(range.commonAncestorContainer)) {
    range = document.createRange();
    range.selectNodeContents(el);
    range.collapse(false);
  }
  const chip = makeChip(name);
  range.deleteContents();
  range.insertNode(chip);
  const after = document.createRange();
  after.setStartAfter(chip);
  after.collapse(true);
  selection?.removeAllRanges();
  selection?.addRange(after);
}

/** Saves the current selection if it's inside `el` — call before
 *  focus moves away to open the insert-variable menu, since the menu
 *  button stealing focus would otherwise collapse the selection. */
export function saveSelectionIfInside(el: HTMLElement): Range | null {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  return el.contains(range.commonAncestorContainer) ? range.cloneRange() : null;
}

export interface PendingToken {
  /** What's been typed so far after the `%`, e.g. `"user_n"` for
   *  `%user_n` with the cursor right after the `n`. */
  query: string;
  /** Spans from the triggering `%` up to the cursor — accepting a
   *  suggestion deletes exactly this range and inserts the chip in
   *  its place, the same shape `insertChipAtSelection` already
   *  expects for its `savedRange` argument. */
  range: Range;
}

/** VSCode-Tab-completion-style live detection: true only when the
 *  cursor sits right after an unclosed `%partial` run being typed —
 *  a caret positioned inside plain text, elsewhere, or straight after
 *  a chip, all return `null` (no suggestions to show). Only scans the
 *  cursor's own text node, so a chip or a line break earlier in the
 *  field correctly stops the scan rather than reading through it. */
export function findPendingToken(el: HTMLElement): PendingToken | null {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || !selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  if (!el.contains(range.startContainer)) return null;
  const node = range.startContainer;
  if (node.nodeType !== Node.TEXT_NODE) return null;
  const text = node.textContent ?? "";
  const offset = range.startOffset;
  const before = text.slice(0, offset);
  const percentIndex = before.lastIndexOf("%");
  if (percentIndex === -1) return null;
  const query = before.slice(percentIndex + 1);
  if (/[%\s]/.test(query)) return null;
  const tokenRange = document.createRange();
  tokenRange.setStart(node, percentIndex);
  tokenRange.setEnd(node, offset);
  return { query, range: tokenRange };
}
