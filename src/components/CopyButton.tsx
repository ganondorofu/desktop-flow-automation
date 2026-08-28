import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, CopyCheck } from "lucide-react";

interface CopyButtonProps {
  text: string;
  className?: string;
}

/** A small "copy this text" icon button — used next to error messages
 *  (a failed run's status-bar summary, a run log's error row, ...)
 *  that are otherwise only selectable a few words at a time inside a
 *  cramped, ellipsis-truncated line. Briefly swaps to a checkmark on
 *  success as the only feedback (no toast/alert) since this sits
 *  inline in places where a popup would be more disruptive than the
 *  copy itself. */
export function CopyButton({ text, className }: CopyButtonProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  async function handleCopy(event: React.MouseEvent) {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access denied/unavailable — nothing more to do here;
      // the text is still visible and selectable by hand.
    }
  }

  return (
    <button type="button" className={`copy-btn ${className ?? ""}`} onClick={(e) => void handleCopy(e)} title={t("common.copy")} aria-label={t("common.copy")}>
      {copied ? <CopyCheck size={12} strokeWidth={2} aria-hidden="true" /> : <Copy size={12} strokeWidth={1.9} aria-hidden="true" />}
    </button>
  );
}
