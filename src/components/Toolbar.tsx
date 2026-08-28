import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Play, Square, Save, Folder, ChevronRight, Loader2, Globe, Timer, ShieldAlert, StepForward, Pause } from "lucide-react";
import type { RunState } from "./StatusBar";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import type { EdgeStyle, Orientation } from "./Canvas";
import type { RecentFile } from "../data/fileOps";

export type EditorMode = "flow" | "code" | "variables";

/** Adding a language only ever grows this list, not the button on
 *  screen — the trigger always just shows the current one, unlike a
 *  row of per-language toggle buttons that gets wider forever. */
const LANGUAGES = [
  { code: "ja", label: "日本語" },
  { code: "en", label: "English" },
] as const;

interface ToolbarProps {
  mode: EditorMode;
  onModeChange: (mode: EditorMode) => void;
  runState: RunState;
  onRun: () => void;
  onStepRun: () => void;
  onStop: () => void;
  isPaused: boolean;
  onDebugStep: () => void;
  onDebugContinue: () => void;
  stepDelayMs: number;
  onStepDelayChange: (ms: number) => void;
  isElevated: boolean;
  onRelaunchAsAdmin: () => void;
  edgeStyle: EdgeStyle;
  onEdgeStyleChange: (style: EdgeStyle) => void;
  orientation: Orientation;
  onOrientationChange: (orientation: Orientation) => void;
  onArrange: () => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  onCut: () => void;
  onCopy: () => void;
  onPaste: () => void;
  onDuplicate: () => void;
  onSelectAll: () => void;
  onDeleteSelected: () => void;
  hasSelection: boolean;
  hasClipboard: boolean;
  fileName: string;
  isDirty: boolean;
  onNewFlow: () => void;
  onOpenFlow: () => void;
  onSave: () => void;
  onSaveAs: () => void;
  onBackToHome: () => void;
  recentFlows: RecentFile[];
  onOpenRecent: (path: string) => void;
}

/** A VSCode-style menu bar: each top-level button opens a dropdown
 *  anchored under itself (reusing the same ContextMenu the canvas uses
 *  for right-click, just positioned from the trigger button's rect
 *  instead of the cursor). */
function MenuBarButton({ label, items }: { label: ReactNode; items: MenuItem[] }) {
  const ref = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState<{ x: number; y: number } | null>(null);

  return (
    <>
      <button
        ref={ref}
        className={`menubar-btn ${open ? "open" : ""}`}
        onClick={() => {
          const rect = ref.current?.getBoundingClientRect();
          setOpen(rect ? { x: rect.left, y: rect.bottom } : null);
        }}
      >
        {label}
      </button>
      {open && <ContextMenu x={open.x} y={open.y} items={items} onClose={() => setOpen(null)} />}
    </>
  );
}

export function Toolbar({
  mode,
  onModeChange,
  runState,
  onRun,
  onStepRun,
  onStop,
  isPaused,
  onDebugStep,
  onDebugContinue,
  stepDelayMs,
  onStepDelayChange,
  isElevated,
  onRelaunchAsAdmin,
  edgeStyle,
  onEdgeStyleChange,
  orientation,
  onOrientationChange,
  onArrange,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
  onCut,
  onCopy,
  onPaste,
  onDuplicate,
  onSelectAll,
  onDeleteSelected,
  hasSelection,
  hasClipboard,
  fileName,
  isDirty,
  onNewFlow,
  onOpenFlow,
  onSave,
  onSaveAs,
  onBackToHome,
  recentFlows,
  onOpenRecent,
}: ToolbarProps) {
  const { t, i18n } = useTranslation();
  const isRunning = runState.status === "running";

  const recentItems: MenuItem[] =
    recentFlows.length > 0
      ? recentFlows.map((f) => ({ label: f.name, onSelect: () => onOpenRecent(f.path) }))
      : [{ label: t("menubar.noRecent") }];

  const fileItems: MenuItem[] = [
    { label: t("menubar.newFlow"), onSelect: onNewFlow },
    { label: t("menubar.openFlow"), onSelect: onOpenFlow },
    { label: t("menubar.openRecent"), items: recentItems },
    { label: t("toolbar.save"), onSelect: onSave },
    { label: t("menubar.saveAs"), onSelect: onSaveAs },
    {
      label: isElevated ? t("menubar.alreadyElevated") : t("menubar.relaunchAsAdmin"),
      onSelect: isElevated ? undefined : onRelaunchAsAdmin,
    },
    { label: t("menubar.backToHome"), onSelect: onBackToHome },
  ];

  const editItems: MenuItem[] = [
    { label: t("menubar.undo"), onSelect: canUndo ? onUndo : undefined },
    { label: t("menubar.redo"), onSelect: canRedo ? onRedo : undefined },
    { label: t("menubar.cut"), onSelect: hasSelection ? onCut : undefined },
    { label: t("menubar.copy"), onSelect: hasSelection ? onCopy : undefined },
    { label: t("menubar.paste"), onSelect: hasClipboard ? onPaste : undefined },
    { label: t("menubar.duplicate"), onSelect: hasSelection ? onDuplicate : undefined },
    { label: t("menubar.selectAll"), onSelect: onSelectAll },
    { label: t("menubar.delete"), onSelect: hasSelection ? onDeleteSelected : undefined, danger: hasSelection },
  ];

  const viewItems: MenuItem[] = [
    {
      label: t("menubar.displayMode"),
      items: [
        { label: mode === "flow" ? `✓ ${t("toolbar.modeFlow")}` : t("toolbar.modeFlow"), onSelect: () => onModeChange("flow") },
        { label: mode === "code" ? `✓ ${t("toolbar.modeCode")}` : t("toolbar.modeCode"), onSelect: () => onModeChange("code") },
        { label: mode === "variables" ? `✓ ${t("toolbar.modeVariables")}` : t("toolbar.modeVariables"), onSelect: () => onModeChange("variables") },
      ],
    },
    { label: t("menubar.arrange"), onSelect: onArrange },
    {
      label: t("menubar.orientation"),
      items: [
        {
          label: orientation === "horizontal" ? `✓ ${t("menubar.orientationHorizontal")}` : t("menubar.orientationHorizontal"),
          onSelect: () => onOrientationChange("horizontal"),
        },
        {
          label: orientation === "vertical" ? `✓ ${t("menubar.orientationVertical")}` : t("menubar.orientationVertical"),
          onSelect: () => onOrientationChange("vertical"),
        },
      ],
    },
    {
      label: t("menubar.edgeStyle"),
      items: [
        {
          label: edgeStyle === "orthogonal" ? `✓ ${t("menubar.edgeOrthogonal")}` : t("menubar.edgeOrthogonal"),
          onSelect: () => onEdgeStyleChange("orthogonal"),
        },
        {
          label: edgeStyle === "straight" ? `✓ ${t("menubar.edgeStraight")}` : t("menubar.edgeStraight"),
          onSelect: () => onEdgeStyleChange("straight"),
        },
        {
          label: edgeStyle === "curved" ? `✓ ${t("menubar.edgeCurved")}` : t("menubar.edgeCurved"),
          onSelect: () => onEdgeStyleChange("curved"),
        },
      ],
    },
  ];

  const runItems: MenuItem[] = [
    { label: t("toolbar.run"), onSelect: isRunning ? undefined : onRun },
    { label: t("toolbar.step"), onSelect: isRunning ? undefined : onStepRun },
    { label: t("toolbar.stop"), onSelect: isRunning ? onStop : undefined },
  ];

  const currentLanguage = LANGUAGES.find((l) => l.code === i18n.language) ?? LANGUAGES[0];
  const languageItems: MenuItem[] = LANGUAGES.map((lang) => ({
    label: lang.code === currentLanguage.code ? `✓ ${lang.label}` : lang.label,
    onSelect: () => i18n.changeLanguage(lang.code),
  }));

  return (
    <>
      <div className="menubar" role="menubar">
        <MenuBarButton label={t("menubar.file")} items={fileItems} />
        <MenuBarButton label={t("menubar.edit")} items={editItems} />
        <MenuBarButton label={t("menubar.view")} items={viewItems} />
        <MenuBarButton label={t("menubar.run")} items={runItems} />
        <div className="menubar-right">
          {!isElevated && (
            <button className="menubar-elevate-btn" onClick={onRelaunchAsAdmin} title={t("menubar.notElevatedHint")}>
              <ShieldAlert size={12} strokeWidth={2} aria-hidden="true" />
              {t("menubar.notElevated")}
            </button>
          )}
          <button className="menubar-icon-btn" onClick={onSave} title={t("toolbar.save")}>
            <Save size={13} strokeWidth={2} aria-hidden="true" />
          </button>
          <MenuBarButton
            label={
              <>
                <Globe size={12} strokeWidth={2} aria-hidden="true" />
                {currentLanguage.code.toUpperCase()}
              </>
            }
            items={languageItems}
          />
        </div>
      </div>
      <div className="toolbar">
      <div className="wordmark">
        <svg className="wordmark-mark" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M6 6.5C9 6.5 10 12 12 12C14 12 15 17.5 18 17.5" className="wordmark-mark-path" fill="none" strokeWidth="2.4" strokeLinecap="round" />
          <circle cx="6" cy="6.5" r="2.4" className="wordmark-mark-node" />
          <circle cx="18" cy="17.5" r="2.4" className="wordmark-mark-node" />
          <circle cx="12" cy="12" r="3.6" className="wordmark-mark-hub" />
        </svg>
        {t("app.wordmark")}
      </div>
      <div className="breadcrumb">
        <Folder size={14} strokeWidth={1.8} aria-hidden="true" />
        <ChevronRight size={12} strokeWidth={2.25} aria-hidden="true" />
        <b>{fileName}</b>
        {isDirty && <span className="unsaved-dot" title={t("toolbar.unsavedChanges")} />}
      </div>
      <div className="toolbar-end">
        <label className="step-delay" title={t("toolbar.stepDelay")}>
          <Timer size={13} strokeWidth={1.9} aria-hidden="true" />
          <input
            type="number"
            min={0}
            step={0.1}
            value={stepDelayMs / 1000}
            onChange={(e) => onStepDelayChange(Math.max(0, Number(e.target.value)) * 1000)}
            aria-label={t("toolbar.stepDelay")}
          />
          <span>s</span>
        </label>
        {isPaused ? (
          <>
            <button className="tbtn" onClick={onDebugStep} title={t("toolbar.step")}>
              <StepForward className="glyph" size={13} strokeWidth={2.25} aria-hidden="true" />
              {t("toolbar.step")}
            </button>
            <button className="tbtn" onClick={onDebugContinue} title={t("toolbar.continue")}>
              <Play className="glyph" size={13} strokeWidth={2.25} aria-hidden="true" />
              {t("toolbar.continue")}
            </button>
            <button className="tbtn primary stopping" onClick={onStop} title={t("toolbar.stopHint")}>
              <Pause className="glyph" size={13} strokeWidth={2.25} aria-hidden="true" />
              {t("toolbar.stop")}
              <Square className="glyph" size={11} strokeWidth={2.25} aria-hidden="true" />
            </button>
          </>
        ) : isRunning ? (
          <button className="tbtn primary stopping" onClick={onStop} title={t("toolbar.stopHint")}>
            <Loader2 className="glyph spin" size={13} strokeWidth={2.25} />
            {t("toolbar.stop")}
            <Square className="glyph" size={11} strokeWidth={2.25} aria-hidden="true" />
          </button>
        ) : (
          <button className="tbtn primary" onClick={onRun}>
            <Play className="glyph" size={13} strokeWidth={2.25} />
            {t("toolbar.run")}
          </button>
        )}
        <div className="modeswitch" role="group" aria-label="Editor mode">
          <button className={mode === "flow" ? "active" : ""} onClick={() => onModeChange("flow")}>
            {t("toolbar.modeFlow")}
          </button>
          <button className={mode === "code" ? "active" : ""} onClick={() => onModeChange("code")}>
            {t("toolbar.modeCode")}
          </button>
          <button className={mode === "variables" ? "active" : ""} onClick={() => onModeChange("variables")}>
            {t("toolbar.modeVariables")}
          </button>
        </div>
      </div>
      </div>
    </>
  );
}
