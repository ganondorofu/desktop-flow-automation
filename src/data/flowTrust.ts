const STORAGE_KEY = "relay.acknowledgedDangerousFlows";

/** SHA-256 of the flow's own YAML text, hex-encoded — identifies
 *  "this exact flow content", not the file path (a renamed/moved copy
 *  of an already-acknowledged flow shouldn't re-prompt, but editing
 *  the flow afterward should, since the actions it contains may have
 *  changed). */
export async function hashFlowText(yaml: string): Promise<string> {
  const bytes = new TextEncoder().encode(yaml);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function readAcknowledged(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}

export function isFlowAcknowledged(hash: string): boolean {
  return readAcknowledged().has(hash);
}

/** Caps how many hashes accumulate in `localStorage` over a long
 *  history of opened flows — drops the oldest entries once past the
 *  limit rather than growing unbounded. */
const MAX_REMEMBERED = 200;

export function rememberFlowAcknowledged(hash: string): void {
  const acknowledged = readAcknowledged();
  acknowledged.add(hash);
  const trimmed = Array.from(acknowledged).slice(-MAX_REMEMBERED);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(trimmed));
  } catch {
    // Best-effort — a full/unavailable localStorage just means this
    // flow gets re-prompted next time, not a hard failure to open it.
  }
}
