import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Search,
  ChevronDown,
  Clock,
  Crosshair,
  MousePointerClick,
  GitBranch,
  AppWindow,
  Globe,
  FolderOpen,
  Radio,
  Rocket,
  Layers,
  Type,
  Info,
  MessageSquare,
} from "lucide-react";
import { paletteLabel, type FlowNode, type NodeNameMode } from "../data/flowModel";
import { resources } from "../i18n/resources";

/** Every label a palette item could be found under — both node-name
 *  modes (friendly and technical), in both languages, regardless of
 *  which one is currently active. Typing "http" should find "HTTP
 *  リクエスト" while the UI is in Japanese, and typing "変数" should
 *  find "Set Variable" while it's in English — search shouldn't
 *  require guessing which language or mode a label happens to be
 *  shown in right now. */
function allLabelsForItem(item: string): string[] {
  const labels: string[] = [];
  for (const lang of ["en", "ja"] as const) {
    const palette = resources[lang].translation.palette;
    const beginner = (palette.items as Record<string, string | undefined>)[item];
    if (beginner) labels.push(beginner);
    const technical = (palette.itemsNormal as Record<string, string | undefined>)[item];
    if (technical) labels.push(technical);
  }
  return labels;
}

const GROUPS: {
  heading:
    | "triggers"
    | "vision"
    | "input"
    | "window"
    | "apps"
    | "system"
    | "dialog"
    | "info"
    | "files"
    | "network"
    | "browser"
    | "text"
    | "control";
  icon: typeof Clock;
  kind: "trigger" | "image" | "action" | "control";
  items: string[];
}[] = [
  // "start" isn't listed here — every flow already has exactly one
  // (auto-created, never deletable; see `ensureStartNode`), so
  // there's nothing left for the palette to add.
  { heading: "triggers", icon: Clock, kind: "trigger", items: ["hotkey", "schedule"] },
  { heading: "vision", icon: Crosshair, kind: "image", items: ["findImageAi", "findTextOcr", "waitForImage", "screenshot"] },
  {
    heading: "input",
    icon: MousePointerClick,
    kind: "action",
    items: ["click", "moveMouse", "typeText", "keyPress"],
  },
  {
    // Window/UI-automation actions — distinct from plain mouse/keyboard
    // input above, since they target a specific window rather than
    // wherever the cursor happens to be.
    heading: "window",
    icon: Layers,
    kind: "action",
    items: ["waitForWindow", "focusWindow", "getElementText"],
  },
  {
    // Launching other programs — distinct from "system" below, which
    // is Windows-level control (shutdown/lock/clipboard) rather than
    // starting/opening something.
    heading: "apps",
    icon: Rocket,
    kind: "action",
    items: ["launchApp", "openUrl"],
  },
  {
    heading: "system",
    icon: AppWindow,
    kind: "action",
    items: ["notify", "readClipboard", "writeClipboard", "lockWorkstation", "powerAction", "checkProcess", "killProcess"],
  },
  {
    // Split out of "system" — a message box/confirm/input prompt is a
    // user-facing popup, not an OS-level control action like the
    // group above.
    heading: "dialog",
    icon: MessageSquare,
    kind: "action",
    items: ["showMessage", "showConfirm", "showInput"],
  },
  {
    // Split out of "system" — date/time and machine-info lookups are
    // read-only queries, not OS-level actions like the group above.
    heading: "info",
    icon: Info,
    kind: "action",
    items: ["getDateTime", "getSystemInfo", "getEnvVar"],
  },
  {
    heading: "text",
    icon: Type,
    kind: "control",
    items: ["textTransform"],
  },
  {
    heading: "files",
    icon: FolderOpen,
    kind: "action",
    items: ["readFile", "writeFile", "copyFile", "moveFile", "deleteFile", "createDirectory", "listDirectory", "waitForFile"],
  },
  {
    heading: "network",
    icon: Radio,
    kind: "action",
    items: ["http", "httpDownload", "ping", "dnsLookup"],
  },
  {
    heading: "browser",
    icon: Globe,
    kind: "action",
    items: [
      "launchBrowser",
      "browserNavigate",
      "browserClick",
      "browserGetText",
      "browserSetValue",
      "browserWaitForSelector",
      "browserScreenshot",
    ],
  },
  {
    // Branching/looping first (what most people reach for), then data
    // (variable/calc), then plain flow control (wait/stop) last.
    heading: "control",
    icon: GitBranch,
    kind: "control",
    items: [
      "ifElse",
      "loop",
      "tryCatch",
      "functionDef",
      "callFunction",
      "setVariable",
      "calculate",
      "generateRandom",
      "wait",
      "stop",
      "break",
      "continue",
      "return",
    ],
  },
];

/** Only these palette items map to an action the engine actually
 *  implements today — everything else is shown but inert. Trigger
 *  types (hotkey/schedule) still need a background listener service
 *  (the app has to be running and watching for the hotkey/time even
 *  when no flow is open), not just a new Action variant, so those stay
 *  disabled. `openUrl` opens the system's default browser (like
 *  clicking a link) — it is not browser-DOM automation, which would
 *  need a separate WebDriver/CDP integration. */
/// `click` never carries a target at all — see
/// `flow_schema::ClickTarget`'s doc comment — so "click the image" is
/// a `moveMouse` step (with its "対象" toggled to the last `find_image`
/// match in the Inspector) followed by a plain `click`, not a
/// separate palette entry.
const IMPLEMENTED: Partial<Record<string, FlowNode["kind"]>> = {
  start: "start",
  click: "click",
  moveMouse: "move_mouse",
  typeText: "type_text",
  keyPress: "key_press",
  wait: "wait",
  findImageAi: "find_image",
  waitForImage: "find_image",
  findTextOcr: "find_text_ocr",
  waitForWindow: "wait_for_window",
  focusWindow: "focus_window",
  getElementText: "get_element_text",
  launchApp: "launch_app",
  openUrl: "open_url",
  notify: "notify",
  readClipboard: "read_clipboard",
  writeClipboard: "write_clipboard",
  lockWorkstation: "lock_workstation",
  powerAction: "power_action",
  showMessage: "show_message",
  showConfirm: "show_confirm",
  showInput: "show_input",
  readFile: "read_file",
  writeFile: "write_file",
  copyFile: "copy_file",
  moveFile: "move_file",
  deleteFile: "delete_file",
  createDirectory: "create_directory",
  listDirectory: "list_directory",
  http: "http",
  httpDownload: "http_download",
  ping: "ping",
  dnsLookup: "dns_lookup",
  screenshot: "screenshot",
  launchBrowser: "launch_browser",
  browserNavigate: "browser_navigate",
  browserClick: "browser_click",
  browserGetText: "browser_get_text",
  browserSetValue: "browser_set_value",
  browserWaitForSelector: "browser_wait_for_selector",
  browserScreenshot: "browser_screenshot",
  getEnvVar: "get_env_var",
  checkProcess: "check_process",
  killProcess: "kill_process",
  waitForFile: "wait_for_file",
  generateRandom: "generate_random",
  setVariable: "set_variable",
  calculate: "calculate",
  ifElse: "if",
  loop: "loop",
  tryCatch: "try_catch",
  functionDef: "function_def",
  callFunction: "call_function",
  stop: "stop",
  break: "break",
  continue: "continue",
  return: "return",
  getDateTime: "get_date_time",
  getSystemInfo: "get_system_info",
  textTransform: "text_transform",
};

/// "Wait for image" isn't a separate action from `findImageAi` under
/// the hood — both are a `find_image` node, same as `moveMouse` and
/// the old "move to image" preset used to share `move_mouse`. What
/// actually differs is the retry policy it starts with: a plain
/// `find_image` runs once and fails if nothing's there yet, while
/// this preset comes with a generous retry (checks twice a second,
/// effectively "keep checking until this image shows up or the flow
/// is stopped" — every retry loop already checks Stop between
/// attempts). Still just a `find_image` node underneath; the
/// Inspector's retry fields can be changed at any time.
const WAIT_FOR_IMAGE_RETRY_PRESET: Partial<FlowNode> = { retryMaxAttempts: 100_000, retryIntervalMs: 500 };

interface PaletteProps {
  onAddStep: (kind: FlowNode["kind"], preset?: Partial<FlowNode>) => void;
  nodeNameMode: NodeNameMode;
  onNodeNameModeChange: (mode: NodeNameMode) => void;
}

export function Palette({ onAddStep, nodeNameMode, onNodeNameModeChange }: PaletteProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<Record<string, boolean>>({
    triggers: true,
    vision: true,
    input: true,
    window: true,
    apps: true,
    system: true,
    dialog: true,
    info: true,
    files: true,
    network: true,
    browser: true,
    text: true,
    control: true,
  });
  const [query, setQuery] = useState("");

  const searching = query.trim().length > 0;
  const normalizedQuery = query.trim().toLowerCase();
  const visibleGroups = GROUPS.map((group) => ({
    ...group,
    items: searching
      ? group.items.filter((item) => allLabelsForItem(item).some((label) => label.toLowerCase().includes(normalizedQuery)))
      : group.items,
  })).filter((group) => !searching || group.items.length > 0);

  return (
    <div className="palette">
      <div className="palette-search">
        <Search size={13} strokeWidth={2} aria-hidden="true" />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("palette.search")}
          aria-label={t("palette.search")}
        />
      </div>
      <div className="switch-row pal-name-mode" role="group" aria-label={t("palette.nodeNameMode")}>
        <button className={nodeNameMode === "beginner" ? "active" : ""} onClick={() => onNodeNameModeChange("beginner")}>
          {t("palette.nodeNameModeBeginner")}
        </button>
        <button className={nodeNameMode === "normal" ? "active" : ""} onClick={() => onNodeNameModeChange("normal")}>
          {t("palette.nodeNameModeNormal")}
        </button>
      </div>
      <div className="palette-scroll">
        {visibleGroups.map((group) => {
          const isOpen = searching || expanded[group.heading];
          const GroupIcon = group.icon;
          return (
            <div className="pal-group" key={group.heading}>
              <button
                className="pal-heading"
                onClick={() => setExpanded((prev) => ({ ...prev, [group.heading]: !prev[group.heading] }))}
                aria-expanded={isOpen}
              >
                <ChevronDown
                  size={11}
                  strokeWidth={2.25}
                  className={`pal-chevron ${isOpen ? "open" : ""}`}
                  aria-hidden="true"
                />
                <span className="pal-icon" data-kind={group.kind}>
                  <GroupIcon size={13} strokeWidth={1.9} aria-hidden="true" />
                </span>
                <span>{t(`palette.${group.heading}`)}</span>
              </button>
              {isOpen && (
                <div className="pal-items">
                  {group.items.map((item) => {
                    const stepKind = IMPLEMENTED[item];
                    const enabled = stepKind !== undefined;
                    return (
                      <button
                        className={`pal-item ${enabled ? "" : "disabled"}`}
                        key={item}
                        disabled={!enabled}
                        title={enabled ? undefined : t("palette.notImplemented")}
                        onClick={() => stepKind && onAddStep(stepKind, item === "waitForImage" ? WAIT_FOR_IMAGE_RETRY_PRESET : undefined)}
                      >
                        <span className="pal-dot" />
                        <span>{paletteLabel(t, item, nodeNameMode)}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
