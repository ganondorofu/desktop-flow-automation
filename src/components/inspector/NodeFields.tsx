import { useTranslation } from "react-i18next";
import { KEY_NAMES, type DateTimeFormat, type FlowNode, type TextOp } from "../../data/flowModel";
import { RecordPositionButton, PickUiElementButton } from "./pickers";
import { VariableTextArea } from "./VariableTextArea";
import { VariableTextInput } from "./VariableTextInput";
import { VariablePicker } from "./VariablePicker";
import { BrowserSelectorFields } from "./BrowserSelectorFields";
import { WindowSelectorFields } from "./WindowSelectorFields";
import { ImageSourceFields } from "./ImageSourceFields";
import { PathField } from "./PathField";
import { VariableOutputList } from "./VariableOutputList";
import { BrowserPicker } from "./BrowserPicker";
import { pickScreenRegion } from "../../data/regionPicker";
import { Toggle } from "./Toggle";

/** "認識パフォーマンス" presets for `find_image`'s `similar` mode —
 *  sets `minScale`/`maxScale`/`scaleSteps` together instead of making
 *  every user tune three raw numbers by hand. More scale steps (and a
 *  wider min/max range) costs roughly proportionally more search time
 *  in exchange for a finer-grained, more size-tolerant match; fewer
 *  is faster but coarser. Threshold (how similar counts as a match)
 *  is a separate, orthogonal knob this doesn't touch. */
const PERF_PRESETS = {
  Fast: { minScale: 0.85, maxScale: 1.15, scaleSteps: 4 },
  Balanced: { minScale: 0.7, maxScale: 1.4, scaleSteps: 12 },
  Thorough: { minScale: 0.5, maxScale: 2.0, scaleSteps: 24 },
} as const satisfies Record<string, { minScale: number; maxScale: number; scaleSteps: number }>;

function matchesPerfPreset(node: FlowNode & { kind: "find_image" }, preset: keyof typeof PERF_PRESETS): boolean {
  const p = PERF_PRESETS[preset];
  return node.minScale === p.minScale && node.maxScale === p.maxScale && node.scaleSteps === p.scaleSteps;
}

const DATE_TIME_FORMATS: DateTimeFormat[] = ["iso8601", "date_only", "time_only", "unix_seconds"];

const TEXT_OP_GROUPS: { key: string; ops: TextOp[] }[] = [
  { key: "format", ops: ["uppercase", "lowercase", "trim"] },
  { key: "replace", ops: ["replace"] },
  { key: "extract", ops: ["substring", "split"] },
  { key: "check", ops: ["length", "contains", "starts_with", "ends_with"] },
  { key: "encode", ops: ["base64_encode", "base64_decode", "json_escape"] },
  { key: "hash", ops: ["md5", "sha256"] },
  { key: "json", ops: ["json_get"] },
  { key: "regex", ops: ["regex_test", "regex_match"] },
];

function textOpGroup(op: TextOp) {
  return TEXT_OP_GROUPS.find((group) => group.ops.includes(op)) ?? TEXT_OP_GROUPS[0];
}

/** Which of `TextTransform`'s `arg1`/`arg2` fields a given `op`
 *  actually reads — see `flow_schema::TextOp`'s doc comment for what
 *  each one means per operation. Neither field applies to an op not
 *  listed in either set. */
const TEXT_OP_USES_ARG1 = new Set<TextOp>([
  "replace",
  "substring",
  "contains",
  "starts_with",
  "ends_with",
  "split",
  "json_get",
  "regex_test",
  "regex_match",
]);
const TEXT_OP_USES_ARG2 = new Set<TextOp>(["replace", "substring", "split", "regex_match"]);

export function NodeFields({
  node,
  detailed,
  onChange,
  variableNames,
  functionNames,
  isDuplicateFunctionName,
}: {
  node: FlowNode;
  detailed: boolean;
  onChange: (updater: (node: FlowNode) => FlowNode) => void;
  variableNames: string[];
  functionNames: string[];
  isDuplicateFunctionName: boolean;
}) {
  const { t } = useTranslation();

  switch (node.kind) {
    case "wait":
      return (
        <div className="field">
          <label>{t("inspector.fields.seconds")}</label>
          <input
            className="num-input"
            type="number"
            min={0}
            step={0.1}
            value={node.seconds}
            onChange={(e) => {
              const seconds = Number(e.target.value);
              onChange((n) => (n.kind === "wait" ? { ...n, seconds } : n));
            }}
          />
        </div>
      );
    case "set_variable":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.name")}</label>
            <input
              className="num-input"
              value={node.name}
              onChange={(e) => {
                const name = e.target.value;
                onChange((n) => (n.kind === "set_variable" ? { ...n, name } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.value")}</label>
              <VariableTextArea
                value={node.value}
                variableNames={variableNames}
                onChangeValue={(value) => onChange((n) => (n.kind === "set_variable" ? { ...n, value } : n))}
              />
            </div>
          )}
        </>
      );
    case "calculate":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.operandA")}</label>
            <input
              className="num-input"
              value={node.a}
              onChange={(e) => {
                const a = e.target.value;
                onChange((n) => (n.kind === "calculate" ? { ...n, a } : n));
              }}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.operator")}</label>
            <select
              className="num-input"
              value={node.op}
              onChange={(e) => {
                const op = e.target.value as typeof node.op;
                onChange((n) => (n.kind === "calculate" ? { ...n, op } : n));
              }}
            >
              <option value="add">{t("inspector.fields.opAdd")}</option>
              <option value="subtract">{t("inspector.fields.opSubtract")}</option>
              <option value="multiply">{t("inspector.fields.opMultiply")}</option>
              <option value="divide">{t("inspector.fields.opDivide")}</option>
              <option value="round">{t("inspector.fields.opRound")}</option>
              <option value="floor">{t("inspector.fields.opFloor")}</option>
              <option value="ceil">{t("inspector.fields.opCeil")}</option>
            </select>
          </div>
          <div className="field">
            <label>
              {node.op === "round" || node.op === "floor" || node.op === "ceil"
                ? t("inspector.fields.decimalPlaces")
                : t("inspector.fields.operandB")}
            </label>
            <input
              className="num-input"
              value={node.b}
              onChange={(e) => {
                const b = e.target.value;
                onChange((n) => (n.kind === "calculate" ? { ...n, b } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.calcResult"),
                    name: node.variable,
                    onChange: (variable) => onChange((n) => (n.kind === "calculate" ? { ...n, variable } : n)),
                  },
                ]}
              />
            </div>
          )}
        </>
      );
    case "type_text":
      return (
        <div className="field">
          <label>{t("inspector.fields.text")}</label>
          <VariableTextArea
            value={node.text}
            variableNames={variableNames}
            onChangeValue={(text) => onChange((n) => (n.kind === "type_text" ? { ...n, text } : n))}
          />
          <p className="insp-hint">{t("inspector.fields.typeTextEnterHint")}</p>
        </div>
      );
    case "click":
      return (
        <>
          <p className="insp-hint">{t("inspector.fields.clickHint")}</p>
          <div className="field">
            <label>{t("inspector.fields.button")}</label>
            <div className="switch-row" role="group" aria-label={t("inspector.fields.button")}>
              {(["left", "right", "middle"] as const).map((button) => (
                <button
                  key={button}
                  className={node.button === button ? "active" : ""}
                  onClick={() => onChange((n) => (n.kind === "click" ? { ...n, button } : n))}
                >
                  {t(`inspector.fields.button${button.charAt(0).toUpperCase()}${button.slice(1)}`)}
                </button>
              ))}
            </div>
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.clickKind")}</label>
              <div className="switch-row" role="group" aria-label={t("inspector.fields.clickKind")}>
                <button
                  className={node.clickKind === "single" ? "active" : ""}
                  onClick={() => onChange((n) => (n.kind === "click" ? { ...n, clickKind: "single" } : n))}
                >
                  {t("inspector.fields.clickKindSingle")}
                </button>
                <button
                  className={node.clickKind === "double" ? "active" : ""}
                  onClick={() => onChange((n) => (n.kind === "click" ? { ...n, clickKind: "double" } : n))}
                >
                  {t("inspector.fields.clickKindDouble")}
                </button>
              </div>
            </div>
          )}
        </>
      );
    case "move_mouse":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.targetKind")}</label>
            <div className="switch-row" role="group" aria-label={t("inspector.fields.targetKind")}>
              <button
                className={node.targetKind !== "last_match" ? "active" : ""}
                onClick={() => onChange((n) => (n.kind === "move_mouse" ? { ...n, targetKind: "coordinate" } : n))}
              >
                {t("inspector.fields.targetKindCoordinate")}
              </button>
              <button
                className={node.targetKind === "last_match" ? "active" : ""}
                onClick={() => onChange((n) => (n.kind === "move_mouse" ? { ...n, targetKind: "last_match" } : n))}
              >
                {t("inspector.fields.targetKindLastMatch")}
              </button>
            </div>
          </div>
          {node.targetKind === "last_match" ? (
            <p className="insp-hint">{t("inspector.fields.targetKindLastMatchHint")}</p>
          ) : (
            <div className="field">
              <label>
                {t("inspector.fields.x")} / {t("inspector.fields.y")}
              </label>
              <div className="field-grid-2">
                <input
                  className="num-input"
                  type="number"
                  value={node.x}
                  onChange={(e) => {
                    const x = Number(e.target.value);
                    onChange((n) => (n.kind === "move_mouse" ? { ...n, x } : n));
                  }}
                />
                <input
                  className="num-input"
                  type="number"
                  value={node.y}
                  onChange={(e) => {
                    const y = Number(e.target.value);
                    onChange((n) => (n.kind === "move_mouse" ? { ...n, y } : n));
                  }}
                />
              </div>
              <RecordPositionButton
                onRecorded={(x, y) => onChange((n) => (n.kind === "move_mouse" ? { ...n, x, y } : n))}
              />
            </div>
          )}
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.durationMs")}</label>
              <input
                className="num-input"
                type="number"
                min={0}
                step={50}
                value={node.durationMs}
                onChange={(e) => {
                  const durationMs = Math.max(0, Number(e.target.value));
                  onChange((n) => (n.kind === "move_mouse" ? { ...n, durationMs } : n));
                }}
              />
            </div>
          )}
        </>
      );
    case "key_press":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.key")}</label>
            <input
              className="num-input"
              list="key-press-key-names"
              value={node.key}
              placeholder={t("inspector.fields.keyPlaceholder")}
              onChange={(e) => {
                const key = e.target.value;
                onChange((n) => (n.kind === "key_press" ? { ...n, key } : n));
              }}
            />
            <datalist id="key-press-key-names">
              {KEY_NAMES.map((name) => (
                <option key={name} value={name} />
              ))}
            </datalist>
          </div>
          <div className="field">
            <label>{t("inspector.fields.keyMode")}</label>
            <div className="switch-row" role="group" aria-label={t("inspector.fields.keyMode")}>
              {(["tap", "press", "release"] as const).map((mode) => (
                <button
                  key={mode}
                  className={node.mode === mode ? "active" : ""}
                  onClick={() => onChange((n) => (n.kind === "key_press" ? { ...n, mode } : n))}
                >
                  {t(`inspector.fields.keyMode${mode.charAt(0).toUpperCase()}${mode.slice(1)}`)}
                </button>
              ))}
            </div>
          </div>
          {node.mode !== "release" && (
            <div className="field">
              <label>{t("inspector.fields.keyModifiers")}</label>
              <div className="field-row">
                {(["ctrl", "alt", "shift", "win"] as const).map((mod) => (
                  <Toggle
                    key={mod}
                    checked={node.modifiers[mod]}
                    onChange={(checked) => onChange((n) => (n.kind === "key_press" ? { ...n, modifiers: { ...n.modifiers, [mod]: checked } } : n))}
                    label={t(`inspector.fields.keyModifier${mod.charAt(0).toUpperCase()}${mod.slice(1)}`)}
                  />
                ))}
              </div>
            </div>
          )}
          {detailed && node.mode === "press" && <p className="insp-hint">{t("inspector.fields.keyModePressHint")}</p>}
          {detailed && node.mode === "release" && <p className="insp-hint">{t("inspector.fields.keyModeReleaseHint")}</p>}
        </>
      );
    case "find_image":
      return (
        <>
          <ImageSourceFields
            image={node.image}
            onChangeImage={(image) => onChange((n) => (n.kind === "find_image" ? { ...n, image } : n))}
          />
          <div className="field">
            <label>{t("inspector.fields.mode")}</label>
            <div className="switch-row" role="group" aria-label={t("inspector.fields.mode")}>
              <button
                className={node.mode === "exact" ? "active" : ""}
                onClick={() => onChange((n) => (n.kind === "find_image" ? { ...n, mode: "exact" } : n))}
              >
                {t("inspector.fields.modeExact")}
              </button>
              <button
                className={node.mode === "similar" ? "active" : ""}
                onClick={() => onChange((n) => (n.kind === "find_image" ? { ...n, mode: "similar" } : n))}
              >
                {t("inspector.fields.modeSimilar")}
              </button>
              <button
                className={node.mode === "ai" ? "active" : ""}
                onClick={() => onChange((n) => (n.kind === "find_image" ? { ...n, mode: "ai" } : n))}
              >
                {t("inspector.fields.modeAi")}
              </button>
            </div>
            {node.mode === "ai" && <p className="insp-hint">{t("inspector.fields.modeAiHint")}</p>}
          </div>
          {(node.mode === "similar" || node.mode === "ai") && (
            <div className="field">
              <label>{t("inspector.fields.perfPreset")}</label>
              <div className="switch-row" role="group" aria-label={t("inspector.fields.perfPreset")}>
                {(Object.keys(PERF_PRESETS) as Array<keyof typeof PERF_PRESETS>).map((preset) => (
                  <button
                    key={preset}
                    className={matchesPerfPreset(node, preset) ? "active" : ""}
                    onClick={() =>
                      onChange((n) => (n.kind === "find_image" ? { ...n, ...PERF_PRESETS[preset] } : n))
                    }
                  >
                    {t(`inspector.fields.perfPreset${preset}`)}
                  </button>
                ))}
              </div>
              <p className="insp-hint">{t("inspector.fields.perfPresetHint")}</p>
            </div>
          )}
          {detailed && (node.mode === "similar" || node.mode === "ai") && (
            <div className="field">
              <label>{t("inspector.fields.threshold")}</label>
              <div className="slider-row">
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.round(node.threshold * 100)}
                  onChange={(e) => {
                    const threshold = Number(e.target.value) / 100;
                    onChange((n) => (n.kind === "find_image" ? { ...n, threshold } : n));
                  }}
                  aria-label={t("inspector.fields.threshold")}
                />
                <span className="slider-val">{node.threshold.toFixed(2)}</span>
              </div>
            </div>
          )}
          {detailed && (node.mode === "similar" || node.mode === "ai") && (
            <div className="field">
              <label>{t("inspector.fields.sizeTolerance")}</label>
              <div className="field-row">
                <input
                  type="number"
                  className="num-input"
                  min={10}
                  max={100}
                  step={5}
                  value={Math.round(node.minScale * 100)}
                  onChange={(e) => {
                    const minScale = Math.max(0.1, Number(e.target.value) / 100);
                    onChange((n) => (n.kind === "find_image" ? { ...n, minScale } : n));
                  }}
                  aria-label={t("inspector.fields.sizeToleranceMin")}
                />
                <span className="field-row-sep">〜</span>
                <input
                  type="number"
                  className="num-input"
                  min={100}
                  max={400}
                  step={5}
                  value={Math.round(node.maxScale * 100)}
                  onChange={(e) => {
                    const maxScale = Math.max(1, Number(e.target.value) / 100);
                    onChange((n) => (n.kind === "find_image" ? { ...n, maxScale } : n));
                  }}
                  aria-label={t("inspector.fields.sizeToleranceMax")}
                />
                <span className="field-row-sep">%</span>
              </div>
              <p className="insp-hint">{t("inspector.fields.sizeToleranceHint")}</p>
            </div>
          )}
        </>
      );
    case "find_text_ocr":
      return (
        <div className="field">
          <label>{t("inspector.fields.ocrText")}</label>
          <input
            className="num-input"
            value={node.text}
            onChange={(e) => {
              const text = e.target.value;
              onChange((n) => (n.kind === "find_text_ocr" ? { ...n, text } : n));
            }}
          />
          <p className="insp-hint">{t("inspector.fields.ocrHint")}</p>
          <label>{t("inspector.fields.ocrRegion")}</label>
          <div className="insp-image-actions">
            <button
              type="button"
              className="insp-record-btn"
              onClick={() =>
                void pickScreenRegion().then((region) => {
                  if (region) onChange((n) => (n.kind === "find_text_ocr" ? { ...n, region } : n));
                })
              }
            >
              {node.region ? t("inspector.fields.ocrRegionChange") : t("inspector.fields.ocrRegionPick")}
            </button>
            {node.region && (
              <button
                type="button"
                className="insp-cancel-btn"
                onClick={() => onChange((n) => (n.kind === "find_text_ocr" ? { ...n, region: undefined } : n))}
              >
                {t("inspector.fields.ocrRegionClear")}
              </button>
            )}
          </div>
          {node.region && (
            <span className="insp-capture-status">
              {t("inspector.fields.ocrRegionSummary", { width: node.region.width, height: node.region.height })}
            </span>
          )}
        </div>
      );
    case "wait_for_window":
      return (
        <>
          <WindowSelectorFields window={node.window} onChangeWindow={(window) => onChange((n) => (n.kind === "wait_for_window" ? { ...n, window } : n))} />
          <div className="field">
            <label>{t("inspector.fields.windowWaitTimeout")}</label>
            <input
              type="number"
              min={0}
              step={500}
              className="num-input"
              value={node.timeoutMs}
              onChange={(e) => {
                const timeoutMs = Math.max(0, Number(e.target.value));
                onChange((n) => (n.kind === "wait_for_window" ? { ...n, timeoutMs } : n));
              }}
            />
          </div>
        </>
      );
    case "focus_window":
      return <WindowSelectorFields window={node.window} onChangeWindow={(window) => onChange((n) => (n.kind === "focus_window" ? { ...n, window } : n))} />;
    case "power_action":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.powerActionMode")}</label>
            <div className="switch-row" role="group" aria-label={t("inspector.fields.powerActionMode")}>
              {(["shutdown", "restart"] as const).map((mode) => (
                <button
                  key={mode}
                  className={node.mode === mode ? "active" : ""}
                  onClick={() => onChange((n) => (n.kind === "power_action" ? { ...n, mode } : n))}
                >
                  {t(`palette.items.${mode}`)}
                </button>
              ))}
            </div>
          </div>
          <div className="field-row">
            <Toggle
              checked={node.force}
              onChange={(force) => onChange((n) => (n.kind === "power_action" ? { ...n, force } : n))}
              label={t("inspector.fields.forceClose")}
            />
          </div>
        </>
      );
    case "lock_workstation":
      return <p className="insp-hint">{t("inspector.fields.lockWorkstationHint")}</p>;
    case "read_clipboard":
      return (
        <div className="field">
          <label>{t("inspector.fields.outputVariables")}</label>
          <VariableOutputList
            items={[
              {
                label: t("inspector.fields.clipboardText"),
                name: node.variable,
                onChange: (variable) => onChange((n) => (n.kind === "read_clipboard" ? { ...n, variable } : n)),
              },
            ]}
          />
        </div>
      );
    case "write_clipboard":
      return (
        <div className="field">
          <label>{t("inspector.fields.content")}</label>
          <VariableTextArea
            value={node.text}
            variableNames={variableNames}
            onChangeValue={(text) => onChange((n) => (n.kind === "write_clipboard" ? { ...n, text } : n))}
          />
        </div>
      );
    case "show_message":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.title")}</label>
            <input
              className="num-input"
              value={node.title}
              onChange={(e) => {
                const title = e.target.value;
                onChange((n) => (n.kind === "show_message" ? { ...n, title } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.message")}</label>
              <VariableTextArea
                value={node.message}
                variableNames={variableNames}
                onChangeValue={(message) => onChange((n) => (n.kind === "show_message" ? { ...n, message } : n))}
              />
            </div>
          )}
          <div className="field-row">
            <Toggle
              checked={node.blocking}
              onChange={(blocking) => onChange((n) => (n.kind === "show_message" ? { ...n, blocking } : n))}
              label={t("inspector.fields.showMessageBlocking")}
            />
          </div>
          <p className="insp-hint">{node.blocking ? t("inspector.fields.showMessageHint") : t("inspector.fields.showMessageNonBlockingHint")}</p>
        </>
      );
    case "show_confirm":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.title")}</label>
            <input
              className="num-input"
              value={node.title}
              onChange={(e) => {
                const title = e.target.value;
                onChange((n) => (n.kind === "show_confirm" ? { ...n, title } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.message")}</label>
              <VariableTextArea
                value={node.message}
                variableNames={variableNames}
                onChangeValue={(message) => onChange((n) => (n.kind === "show_confirm" ? { ...n, message } : n))}
              />
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.confirmResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "show_confirm" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
          <p className="insp-hint">{t("inspector.fields.showConfirmHint")}</p>
        </>
      );
    case "show_input":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.title")}</label>
            <input
              className="num-input"
              value={node.title}
              onChange={(e) => {
                const title = e.target.value;
                onChange((n) => (n.kind === "show_input" ? { ...n, title } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.message")}</label>
              <VariableTextArea
                value={node.message}
                variableNames={variableNames}
                onChangeValue={(message) => onChange((n) => (n.kind === "show_input" ? { ...n, message } : n))}
              />
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.defaultValue")}</label>
            <VariableTextInput
              value={node.defaultValue}
              variableNames={variableNames}
              onChangeValue={(defaultValue) => onChange((n) => (n.kind === "show_input" ? { ...n, defaultValue } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.inputResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "show_input" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "launch_app":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.appPath")}</label>
            <input
              className="num-input"
              value={node.path}
              onChange={(e) => {
                const path = e.target.value;
                onChange((n) => (n.kind === "launch_app" ? { ...n, path } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.appArgs")}</label>
              <input
                className="num-input"
                value={node.args}
                onChange={(e) => {
                  const args = e.target.value;
                  onChange((n) => (n.kind === "launch_app" ? { ...n, args } : n));
                }}
              />
            </div>
          )}
        </>
      );
    case "open_url":
      return (
        <div className="field">
          <label>{t("inspector.fields.url")}</label>
          <VariableTextInput
            value={node.url}
            variableNames={variableNames}
            onChangeValue={(url) => onChange((n) => (n.kind === "open_url" ? { ...n, url } : n))}
          />
        </div>
      );
    case "notify":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.title")}</label>
            <input
              className="num-input"
              value={node.title}
              onChange={(e) => {
                const title = e.target.value;
                onChange((n) => (n.kind === "notify" ? { ...n, title } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.message")}</label>
              <VariableTextArea
                value={node.message}
                variableNames={variableNames}
                onChangeValue={(message) => onChange((n) => (n.kind === "notify" ? { ...n, message } : n))}
              />
            </div>
          )}
        </>
      );
    case "read_file":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "read_file" ? { ...n, path } : n))} />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.fileContents"),
                    name: node.variable,
                    onChange: (variable) => onChange((n) => (n.kind === "read_file" ? { ...n, variable } : n)),
                  },
                ]}
              />
            </div>
          )}
        </>
      );
    case "write_file":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "write_file" ? { ...n, path } : n))} />
          </div>
          <div className="field">
            <label>{t("inspector.fields.content")}</label>
            <VariableTextArea
              value={node.content}
              variableNames={variableNames}
              onChangeValue={(content) => onChange((n) => (n.kind === "write_file" ? { ...n, content } : n))}
            />
          </div>
          {detailed && (
            <div className="field-row">
              <Toggle
                checked={node.append}
                onChange={(append) => onChange((n) => (n.kind === "write_file" ? { ...n, append } : n))}
                label={t("inspector.fields.append")}
              />
            </div>
          )}
        </>
      );
    case "copy_file":
    case "move_file":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.source")}</label>
            <PathField
              value={node.source}
              onChange={(source) => onChange((n) => (n.kind === "copy_file" || n.kind === "move_file" ? { ...n, source } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.destination")}</label>
            <PathField
              value={node.destination}
              onChange={(destination) =>
                onChange((n) => (n.kind === "copy_file" || n.kind === "move_file" ? { ...n, destination } : n))
              }
            />
          </div>
        </>
      );
    case "delete_file":
      return (
        <div className="field">
          <label>{t("inspector.fields.path")}</label>
          <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "delete_file" ? { ...n, path } : n))} />
        </div>
      );
    case "create_directory":
      return (
        <div className="field">
          <label>{t("inspector.fields.path")}</label>
          <PathField
            value={node.path}
            directory
            onChange={(path) => onChange((n) => (n.kind === "create_directory" ? { ...n, path } : n))}
          />
        </div>
      );
    case "list_directory":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField
              value={node.path}
              directory
              onChange={(path) => onChange((n) => (n.kind === "list_directory" ? { ...n, path } : n))}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.folderListing"),
                    name: node.variable,
                    onChange: (variable) => onChange((n) => (n.kind === "list_directory" ? { ...n, variable } : n)),
                  },
                ]}
              />
            </div>
          )}
        </>
      );
    case "http":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.method")}</label>
            <select
              className="num-input"
              value={node.method}
              onChange={(e) => {
                const method = e.target.value as typeof node.method;
                onChange((n) => (n.kind === "http" ? { ...n, method } : n));
              }}
            >
              {(["get", "post", "put", "patch", "delete"] as const).map((method) => (
                <option key={method} value={method}>
                  {method.toUpperCase()}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>{t("inspector.fields.url")}</label>
            <VariableTextInput
              value={node.url}
              variableNames={variableNames}
              onChangeValue={(url) => onChange((n) => (n.kind === "http" ? { ...n, url } : n))}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.httpHeaders")}</label>
              <VariableTextArea
                value={node.headers}
                variableNames={variableNames}
                onChangeValue={(headers) => onChange((n) => (n.kind === "http" ? { ...n, headers } : n))}
              />
              <p className="insp-hint">{t("inspector.fields.httpHeadersHint")}</p>
            </div>
          )}
          {detailed && node.method !== "get" && node.method !== "delete" && (
            <div className="field">
              <label>{t("inspector.fields.content")}</label>
              <VariableTextArea
                value={node.body}
                variableNames={variableNames}
                onChangeValue={(body) => onChange((n) => (n.kind === "http" ? { ...n, body } : n))}
              />
            </div>
          )}
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.httpResponseBody"),
                    name: node.variable || "http_response",
                    enabled: node.variable !== "",
                    onChange: (variable) => onChange((n) => (n.kind === "http" ? { ...n, variable } : n)),
                    onToggleEnabled: (enabled) =>
                      onChange((n) => (n.kind === "http" ? { ...n, variable: enabled ? node.variable || "http_response" : "" } : n)),
                  },
                  {
                    label: t("inspector.fields.httpStatusCode"),
                    name: node.statusVariable || "http_status",
                    enabled: node.statusVariable !== "",
                    onChange: (statusVariable) => onChange((n) => (n.kind === "http" ? { ...n, statusVariable } : n)),
                    onToggleEnabled: (enabled) =>
                      onChange((n) => (n.kind === "http" ? { ...n, statusVariable: enabled ? node.statusVariable || "http_status" : "" } : n)),
                  },
                ]}
              />
            </div>
          )}
        </>
      );
    case "http_download":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.url")}</label>
            <VariableTextInput
              value={node.url}
              variableNames={variableNames}
              onChangeValue={(url) => onChange((n) => (n.kind === "http_download" ? { ...n, url } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "http_download" ? { ...n, path } : n))} />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.httpHeaders")}</label>
              <VariableTextArea
                value={node.headers}
                variableNames={variableNames}
                onChangeValue={(headers) => onChange((n) => (n.kind === "http_download" ? { ...n, headers } : n))}
              />
              <p className="insp-hint">{t("inspector.fields.httpHeadersHint")}</p>
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.httpDownloadPath"),
                  name: node.pathVariable || "saved_path",
                  enabled: node.pathVariable !== "",
                  onChange: (pathVariable) => onChange((n) => (n.kind === "http_download" ? { ...n, pathVariable } : n)),
                  onToggleEnabled: (enabled) =>
                    onChange((n) =>
                      n.kind === "http_download" ? { ...n, pathVariable: enabled ? node.pathVariable || "saved_path" : "" } : n,
                    ),
                },
                {
                  label: t("inspector.fields.httpStatusCode"),
                  name: node.variable || "status_code",
                  enabled: node.variable !== "",
                  onChange: (variable) => onChange((n) => (n.kind === "http_download" ? { ...n, variable } : n)),
                  onToggleEnabled: (enabled) =>
                    onChange((n) => (n.kind === "http_download" ? { ...n, variable: enabled ? node.variable || "status_code" : "" } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "ping":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.host")}</label>
            <VariableTextInput
              value={node.host}
              variableNames={variableNames}
              onChangeValue={(host) => onChange((n) => (n.kind === "ping" ? { ...n, host } : n))}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.pingTimeoutMs")}</label>
              <input
                className="num-input"
                type="number"
                min={100}
                step={100}
                value={node.timeoutMs}
                onChange={(e) => {
                  const timeoutMs = Math.max(100, Number(e.target.value));
                  onChange((n) => (n.kind === "ping" ? { ...n, timeoutMs } : n));
                }}
              />
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.pingReachable"),
                  name: node.variable || "ping_reachable",
                  enabled: node.variable !== "",
                  onChange: (variable) => onChange((n) => (n.kind === "ping" ? { ...n, variable } : n)),
                  onToggleEnabled: (enabled) =>
                    onChange((n) => (n.kind === "ping" ? { ...n, variable: enabled ? node.variable || "ping_reachable" : "" } : n)),
                },
                ...(node.variable ? [{ label: t("inspector.fields.pingLatencyMs"), name: `${node.variable}_latency_ms` }] : []),
              ]}
            />
          </div>
        </>
      );
    case "dns_lookup":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.hostname")}</label>
            <VariableTextInput
              value={node.hostname}
              variableNames={variableNames}
              onChangeValue={(hostname) => onChange((n) => (n.kind === "dns_lookup" ? { ...n, hostname } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.dnsLookupResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "dns_lookup" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "screenshot":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "screenshot" ? { ...n, path } : n))} />
          </div>
          <div className="field">
            <label>{t("inspector.fields.ocrRegion")}</label>
            <div className="insp-image-actions">
              <button
                type="button"
                className="insp-record-btn"
                onClick={() =>
                  void pickScreenRegion().then((region) => {
                    if (region) onChange((n) => (n.kind === "screenshot" ? { ...n, region } : n));
                  })
                }
              >
                {node.region ? t("inspector.fields.ocrRegionChange") : t("inspector.fields.ocrRegionPick")}
              </button>
              {node.region && (
                <button
                  type="button"
                  className="insp-cancel-btn"
                  onClick={() => onChange((n) => (n.kind === "screenshot" ? { ...n, region: undefined } : n))}
                >
                  {t("inspector.fields.ocrRegionClear")}
                </button>
              )}
            </div>
            {node.region && (
              <span className="insp-capture-status">
                {t("inspector.fields.ocrRegionSummary", { width: node.region.width, height: node.region.height })}
              </span>
            )}
          </div>
        </>
      );
    case "browser_screenshot":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "browser_screenshot" ? { ...n, path } : n))} />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_screenshot" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "get_env_var":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.envVarName")}</label>
            <VariableTextInput
              value={node.name}
              variableNames={variableNames}
              onChangeValue={(name) => onChange((n) => (n.kind === "get_env_var" ? { ...n, name } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.envVarResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "get_env_var" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "check_process":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.processName")}</label>
            <VariableTextInput
              value={node.name}
              variableNames={variableNames}
              placeholder="notepad.exe"
              onChangeValue={(name) => onChange((n) => (n.kind === "check_process" ? { ...n, name } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.checkProcessResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "check_process" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "kill_process":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.processName")}</label>
            <VariableTextInput
              value={node.name}
              variableNames={variableNames}
              placeholder="notepad.exe"
              onChangeValue={(name) => onChange((n) => (n.kind === "kill_process" ? { ...n, name } : n))}
            />
          </div>
          {detailed && (
            <div className="field-row">
              <Toggle
                checked={node.force}
                onChange={(force) => onChange((n) => (n.kind === "kill_process" ? { ...n, force } : n))}
                label={t("inspector.fields.forceClose")}
              />
            </div>
          )}
        </>
      );
    case "wait_for_file":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.path")}</label>
            <PathField value={node.path} onChange={(path) => onChange((n) => (n.kind === "wait_for_file" ? { ...n, path } : n))} />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.waitForFileTimeoutMs")}</label>
              <input
                className="num-input"
                type="number"
                min={100}
                step={1000}
                value={node.timeoutMs}
                onChange={(e) => {
                  const timeoutMs = Math.max(100, Number(e.target.value));
                  onChange((n) => (n.kind === "wait_for_file" ? { ...n, timeoutMs } : n));
                }}
              />
            </div>
          )}
        </>
      );
    case "generate_random":
      return (
        <>
          <div className="field-row">
            <div className="field">
              <label>{t("inspector.fields.randomMin")}</label>
              <VariableTextInput
                value={node.min}
                variableNames={variableNames}
                onChangeValue={(min) => onChange((n) => (n.kind === "generate_random" ? { ...n, min } : n))}
              />
            </div>
            <div className="field">
              <label>{t("inspector.fields.randomMax")}</label>
              <VariableTextInput
                value={node.max}
                variableNames={variableNames}
                onChangeValue={(max) => onChange((n) => (n.kind === "generate_random" ? { ...n, max } : n))}
              />
            </div>
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.randomResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "generate_random" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "get_element_text":
      return (
        <>
          <div className="field">
            <PickUiElementButton
              onPicked={(windowTitle, elementName, automationId) => {
                onChange((n) => (n.kind === "get_element_text" ? { ...n, windowTitle, elementName, automationId } : n));
              }}
            />
          </div>
          {node.automationId && (
            <div className="field">
              <label>{t("inspector.fields.automationId")}</label>
              <input
                className="num-input"
                value={node.automationId}
                onChange={(e) => {
                  const automationId = e.target.value;
                  onChange((n) => (n.kind === "get_element_text" ? { ...n, automationId } : n));
                }}
              />
            </div>
          )}
          <div className="field">
            <label>{node.automationId ? t("inspector.fields.elementNameHint") : t("inspector.fields.elementName")}</label>
            <input
              className="num-input"
              value={node.elementName}
              onChange={(e) => {
                const elementName = e.target.value;
                onChange((n) => (n.kind === "get_element_text" ? { ...n, elementName } : n));
              }}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.windowTitle")}</label>
              <VariableTextInput
                value={node.windowTitle}
                variableNames={variableNames}
                onChangeValue={(windowTitle) => onChange((n) => (n.kind === "get_element_text" ? { ...n, windowTitle } : n))}
              />
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.elementText"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "get_element_text" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "launch_browser":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.url")}</label>
            <VariableTextInput
              value={node.url}
              variableNames={variableNames}
              onChangeValue={(url) => onChange((n) => (n.kind === "launch_browser" ? { ...n, url } : n))}
            />
          </div>
          <div className="field">
            <label>{t("inspector.fields.browserChoice")}</label>
            <BrowserPicker value={node.browser} onChange={(browser) => onChange((n) => (n.kind === "launch_browser" ? { ...n, browser } : n))} />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.profileDir")}</label>
              <PathField
                value={node.profileDir}
                directory
                onChange={(profileDir) => onChange((n) => (n.kind === "launch_browser" ? { ...n, profileDir } : n))}
              />
              <p className="insp-hint">{t("inspector.fields.profileDirHint")}</p>
            </div>
          )}
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.browserTabInstance"),
                    name: node.variable || "browser_tab",
                    enabled: node.variable !== "",
                    onChange: (variable) => onChange((n) => (n.kind === "launch_browser" ? { ...n, variable } : n)),
                    onToggleEnabled: (enabled) =>
                      onChange((n) => (n.kind === "launch_browser" ? { ...n, variable: enabled ? node.variable || "browser_tab" : "" } : n)),
                  },
                ]}
              />
              <p className="insp-hint">{t("inspector.fields.browserInstanceHint")}</p>
            </div>
          )}
        </>
      );
    case "browser_navigate":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.url")}</label>
            <VariableTextInput
              value={node.url}
              variableNames={variableNames}
              onChangeValue={(url) => onChange((n) => (n.kind === "browser_navigate" ? { ...n, url } : n))}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_navigate" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "browser_click":
      return (
        <>
          <BrowserSelectorFields
            selector={node.selector}
            onChangeSelector={(selector) => onChange((n) => (n.kind === "browser_click" ? { ...n, selector } : n))}
          />
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_click" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "browser_get_text":
      return (
        <>
          <BrowserSelectorFields
            selector={node.selector}
            onChangeSelector={(selector) => onChange((n) => (n.kind === "browser_get_text" ? { ...n, selector } : n))}
          />
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.elementText"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "browser_get_text" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_get_text" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "browser_set_value":
      return (
        <>
          <BrowserSelectorFields
            selector={node.selector}
            onChangeSelector={(selector) => onChange((n) => (n.kind === "browser_set_value" ? { ...n, selector } : n))}
          />
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.value")}</label>
              <VariableTextArea
                value={node.value}
                variableNames={variableNames}
                onChangeValue={(value) => onChange((n) => (n.kind === "browser_set_value" ? { ...n, value } : n))}
              />
            </div>
          )}
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_set_value" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "browser_wait_for_selector":
      return (
        <>
          <BrowserSelectorFields
            selector={node.selector}
            onChangeSelector={(selector) => onChange((n) => (n.kind === "browser_wait_for_selector" ? { ...n, selector } : n))}
          />
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.browserInstance")}</label>
              <VariablePicker
                value={node.instance}
                wrap
                options={variableNames}
                onChangeValue={(instance) => onChange((n) => (n.kind === "browser_wait_for_selector" ? { ...n, instance } : n))}
              />
            </div>
          )}
        </>
      );
    case "if":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.name")}</label>
            <VariablePicker
              value={node.condition.variable}
              options={variableNames}
              onChangeValue={(variable) => onChange((n) => (n.kind === "if" ? { ...n, condition: { ...n.condition, variable } } : n))}
            />
          </div>
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.value")}</label>
              <input
                className="num-input"
                value={node.condition.equals}
                onChange={(e) => {
                  const equals = e.target.value;
                  onChange((n) => (n.kind === "if" ? { ...n, condition: { ...n.condition, equals } } : n));
                }}
              />
            </div>
          )}
        </>
      );
    case "loop":
      return (
        <div className="field">
          <label>{t("inspector.fields.count")}</label>
          <input
            className="num-input"
            type="number"
            min={1}
            value={node.count}
            onChange={(e) => {
              const count = Math.max(1, Number(e.target.value));
              onChange((n) => (n.kind === "loop" ? { ...n, count } : n));
            }}
          />
        </div>
      );
    case "try_catch":
      return <p className="insp-hint">{t("inspector.fields.tryCatchHint")}</p>;
    case "function_def":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.functionName")}</label>
            <input
              className="num-input"
              type="text"
              value={node.name}
              onChange={(e) => {
                const name = e.target.value;
                onChange((n) => (n.kind === "function_def" ? { ...n, name } : n));
              }}
            />
          </div>
          {isDuplicateFunctionName && <p className="insp-hint insp-warning">{t("inspector.fields.functionNameDuplicate")}</p>}
          <p className="insp-hint">{t("inspector.fields.functionDefHint")}</p>
        </>
      );
    case "break":
      return <p className="insp-hint">{t("inspector.fields.breakHint")}</p>;
    case "continue":
      return <p className="insp-hint">{t("inspector.fields.continueHint")}</p>;
    case "return":
      return <p className="insp-hint">{t("inspector.fields.returnHint")}</p>;
    case "get_date_time":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.dateTimeFormat")}</label>
            <select
              className="num-input"
              value={node.format}
              onChange={(e) => {
                const format = e.target.value as DateTimeFormat;
                onChange((n) => (n.kind === "get_date_time" ? { ...n, format } : n));
              }}
            >
              {DATE_TIME_FORMATS.map((format) => (
                <option key={format} value={format}>
                  {t(`inspector.fields.dateTimeFormat${format.charAt(0).toUpperCase()}${format.slice(1).replace(/_([a-z])/g, (_m, c: string) => c.toUpperCase())}`)}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>{t("inspector.fields.outputVariables")}</label>
            <VariableOutputList
              items={[
                {
                  label: t("inspector.fields.dateTimeResult"),
                  name: node.variable,
                  onChange: (variable) => onChange((n) => (n.kind === "get_date_time" ? { ...n, variable } : n)),
                },
              ]}
            />
          </div>
        </>
      );
    case "get_system_info": {
      const fields = [
        { key: "hostname" as const, label: t("inspector.fields.sysInfoHostname"), fallback: "sys_hostname" },
        { key: "osVersion" as const, label: t("inspector.fields.sysInfoOsVersion"), fallback: "sys_os_version" },
        { key: "cpuPercent" as const, label: t("inspector.fields.sysInfoCpuPercent"), fallback: "sys_cpu_percent" },
        { key: "memoryPercent" as const, label: t("inspector.fields.sysInfoMemoryPercent"), fallback: "sys_memory_percent" },
        { key: "ipAddress" as const, label: t("inspector.fields.sysInfoIpAddress"), fallback: "sys_ip_address" },
      ];
      return (
        <div className="field">
          <label>{t("inspector.fields.outputVariables")}</label>
          <VariableOutputList
            items={fields.map(({ key, label, fallback }) => ({
              label,
              name: node[key] || fallback,
              enabled: node[key] !== "",
              onChange: (name) => onChange((n) => (n.kind === "get_system_info" ? { ...n, [key]: name } : n)),
              onToggleEnabled: (enabled) =>
                onChange((n) => (n.kind === "get_system_info" ? { ...n, [key]: enabled ? node[key] || fallback : "" } : n)),
            }))}
          />
        </div>
      );
    }
    case "text_transform": {
      const group = textOpGroup(node.op);
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.textTransformGroup")}</label>
            <select
              className="num-input"
              value={group.key}
              onChange={(e) => {
                const op = TEXT_OP_GROUPS.find(({ key }) => key === e.target.value)?.ops[0] ?? "uppercase";
                onChange((n) => (n.kind === "text_transform" ? { ...n, op } : n));
              }}
            >
              {TEXT_OP_GROUPS.map(({ key }) => (
                <option key={key} value={key}>
                  {t(`inspector.fields.textGroup${key.charAt(0).toUpperCase()}${key.slice(1)}`)}
                </option>
              ))}
            </select>
          </div>
          {group.ops.length > 1 && (
            <div className="field">
              <label>{t("inspector.fields.textTransformOp")}</label>
              <select
                className="num-input"
                value={node.op}
                onChange={(e) => {
                  const op = e.target.value as TextOp;
                  onChange((n) => (n.kind === "text_transform" ? { ...n, op } : n));
                }}
              >
                {group.ops.map((op) => (
                  <option key={op} value={op}>
                    {t(`inspector.fields.textOp${op.charAt(0).toUpperCase()}${op.slice(1).replace(/_([a-z])/g, (_m, c: string) => c.toUpperCase())}`)}
                  </option>
                ))}
              </select>
            </div>
          )}
          <div className="field">
            <label>{t("inspector.fields.text")}</label>
            <VariableTextArea
              value={node.text}
              variableNames={variableNames}
              onChangeValue={(text) => onChange((n) => (n.kind === "text_transform" ? { ...n, text } : n))}
            />
          </div>
          {TEXT_OP_USES_ARG1.has(node.op) && (
            <div className="field">
              <label>{t(`inspector.fields.textOpArg1_${node.op}`)}</label>
              <VariableTextInput
                value={node.arg1}
                variableNames={variableNames}
                onChangeValue={(arg1) => onChange((n) => (n.kind === "text_transform" ? { ...n, arg1 } : n))}
              />
            </div>
          )}
          {TEXT_OP_USES_ARG2.has(node.op) && (
            <div className="field">
              <label>{t(`inspector.fields.textOpArg2_${node.op}`)}</label>
              <VariableTextInput
                value={node.arg2}
                variableNames={variableNames}
                onChangeValue={(arg2) => onChange((n) => (n.kind === "text_transform" ? { ...n, arg2 } : n))}
              />
            </div>
          )}
          {detailed && (
            <div className="field">
              <label>{t("inspector.fields.outputVariables")}</label>
              <VariableOutputList
                items={[
                  {
                    label: t("inspector.fields.textTransformResult"),
                    name: node.variable,
                    onChange: (variable) => onChange((n) => (n.kind === "text_transform" ? { ...n, variable } : n)),
                  },
                ]}
              />
            </div>
          )}
        </>
      );
    }
    case "call_function":
      return (
        <>
          <div className="field">
            <label>{t("inspector.fields.callFunctionName")}</label>
            <select
              className="num-input"
              value={node.name}
              onChange={(e) => {
                const name = e.target.value;
                onChange((n) => (n.kind === "call_function" ? { ...n, name } : n));
              }}
            >
              <option value="" disabled>
                {t("inspector.fields.callFunctionName")}
              </option>
              {(functionNames.includes(node.name) || !node.name ? functionNames : [node.name, ...functionNames]).map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </div>
          <p className="insp-hint">{t("inspector.fields.callFunctionHint")}</p>
        </>
      );
  }
}
