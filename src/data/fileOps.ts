import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

export interface RecentFile {
  path: string;
  name: string;
  opened_at: number;
}

/** `.relay` is Relay's own extension — the file is YAML underneath (see
 *  `buildFlowYaml`/`parseFlowYaml`), but giving it a distinct name
 *  keeps it recognizable as a Relay flow instead of looking like any
 *  other stray `.yaml` file, the same way `.vue`/`.svelte` don't just
 *  say `.html`. */
const FILTERS = [{ name: "Relay Flow", extensions: ["relay"] }];

export function listRecentFlows(): Promise<RecentFile[]> {
  return invoke<RecentFile[]>("list_recent_flows");
}

export function rememberRecentFlow(path: string): Promise<RecentFile[]> {
  return invoke<RecentFile[]>("remember_recent_flow", { path });
}

export function forgetRecentFlow(path: string): Promise<RecentFile[]> {
  return invoke<RecentFile[]>("forget_recent_flow", { path });
}

export function defaultFlowsDir(): Promise<string> {
  return invoke<string>("default_flows_dir");
}

export function readFlowFile(path: string): Promise<string> {
  return invoke<string>("load_flow_from_path", { path });
}

export function writeFlowFile(path: string, yaml: string): Promise<void> {
  return invoke<void>("save_flow_to_path", { path, yaml });
}

/** Native "Open" dialog scoped to flow files. Returns `null` if the
 *  user cancelled. */
export async function pickFlowFileToOpen(): Promise<string | null> {
  const result = await openDialog({ multiple: false, directory: false, filters: FILTERS });
  return typeof result === "string" ? result : null;
}

/** Native "Save As" dialog, pre-filled with a sensible folder and the
 *  current flow's name. Returns `null` if the user cancelled. */
export async function pickFlowSavePath(suggestedName: string): Promise<string | null> {
  const dir = await defaultFlowsDir().catch(() => undefined);
  const fileName = suggestedName.endsWith(".relay") ? suggestedName : `${suggestedName}.relay`;
  const result = await saveDialog({
    defaultPath: dir ? `${dir}/${fileName}` : fileName,
    filters: FILTERS,
  });
  return result ?? null;
}

/** The filename without its directory or `.relay` extension — used
 *  both as the flow's display name and as the default text for the
 *  next "Save As". */
export function fileStem(path: string): string {
  const base = path.split(/[/\\]/).pop() ?? path;
  return base.replace(/\.relay$/i, "");
}
