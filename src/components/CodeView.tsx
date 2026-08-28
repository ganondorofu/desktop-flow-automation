import type { ReactNode } from "react";

const TOKEN_RE = /(#.*$)|("(?:[^"\\]|\\.)*")|(-?\b\d+(?:\.\d+)?\b)|([a-zA-Z_][\w.]*(?=:))/gm;

function highlightLine(line: string): ReactNode[] {
  const parts: ReactNode[] = [];
  let lastIndex = 0;
  let key = 0;
  for (const m of line.matchAll(TOKEN_RE)) {
    const [full, comment, str, num] = m;
    const start = m.index ?? 0;
    if (start > lastIndex) parts.push(line.slice(lastIndex, start));
    if (comment) {
      parts.push(
        <span key={key++} style={{ color: "var(--text-faintest, var(--text-faint))" }}>
          {full}
        </span>,
      );
    } else if (str || num) {
      parts.push(
        <span key={key++} style={{ color: "var(--copper-soft)" }}>
          {full}
        </span>,
      );
    } else {
      parts.push(
        <span key={key++} style={{ color: "var(--teal-strong, var(--teal))" }}>
          {full}
        </span>,
      );
    }
    lastIndex = start + full.length;
  }
  if (lastIndex < line.length) parts.push(line.slice(lastIndex));
  return parts;
}

interface CodeViewProps {
  yaml: string;
  fileName: string;
}

export function CodeView({ yaml, fileName }: CodeViewProps) {
  const lines = yaml.replace(/\n$/, "").split("\n");

  return (
    <div className="code-view">
      <div className="code-view-head">
        <span>{fileName}</span>
        <button className="code-copy-btn">コピー</button>
      </div>
      <div className="code-view-body">
        {lines.map((line, i) => (
          <div className="code-line" key={i}>
            <span className="code-lineno">{String(i + 1).padStart(2, "0")}</span>
            <span>{highlightLine(line)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
