import { describe, expect, it } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App, {
  CANVAS_MIN_WIDTH,
  COLLAPSED_PANEL_SIZE,
  EXECUTION_MIN_HEIGHT,
  INSPECTOR_DEFAULT_WIDTH,
  PALETTE_DEFAULT_WIDTH,
} from "./App";

/** Gets past the home screen into the editor, where the toolbar,
 *  panels, and execution widget this file cares about actually
 *  render — every test below needs this first. */
async function openEditor() {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: "新規フロー" }));
  return user;
}

// `getByRole(..., { name: /実行状況/ })`/`{ name: "実行" }` are
// ambiguous in this tree — the same label shows up on more than one
// element (the run button's text is duplicated into an always-present
// shortcuts list, and the execution panel's own close button's label
// contains "実行状況" too once it's open). Targeting the exact
// elements by their own class instead of accessible name sidesteps
// that collision entirely.
function executionToggleButton(): HTMLElement {
  const el = document.querySelector(".sb-execution-toggle");
  if (!el) throw new Error("execution toggle button not found");
  return el as HTMLElement;
}
function runButton(): HTMLElement {
  const el = document.querySelector(".tbtn.primary:not(.stopping)");
  if (!el) throw new Error("run button not found");
  return el as HTMLElement;
}

describe("editor layout", () => {
  it("grows the grid's bottom row only while the execution panel is open", async () => {
    const user = await openEditor();
    const app = document.querySelector(".app");
    expect(app).not.toBeNull();
    expect(app).not.toHaveClass("execution-open");

    await user.click(executionToggleButton());
    expect(app).toHaveClass("execution-open");

    await user.click(executionToggleButton());
    expect(app).not.toHaveClass("execution-open");
  });

  it("stays within an 800px-wide window at every panel's default size", () => {
    // jsdom doesn't run a real layout engine (no computed
    // `getBoundingClientRect`), so this can't observe actual overflow
    // the way a real-browser test could — it instead checks the same
    // arithmetic app.css's grid depends on: default palette width +
    // the canvas's own minimum + default inspector width must fit
    // under the app's 800px launch width (see tauri.conf.json), with
    // room to spare for the canvas to still be usable. Collapsing
    // either side panel only ever shrinks this further.
    const total = PALETTE_DEFAULT_WIDTH + CANVAS_MIN_WIDTH + INSPECTOR_DEFAULT_WIDTH;
    expect(total).toBeLessThan(800);
    expect(COLLAPSED_PANEL_SIZE).toBeLessThan(PALETTE_DEFAULT_WIDTH);
    expect(COLLAPSED_PANEL_SIZE).toBeLessThan(INSPECTOR_DEFAULT_WIDTH);
  });

  it("switches between the variables and log tabs without losing either panel", async () => {
    const user = await openEditor();
    await user.click(executionToggleButton());

    const variablesTab = screen.getByRole("tab", { name: /変数トレース/ });
    const logTab = screen.getByRole("tab", { name: /実行ログ/ });
    expect(variablesTab).toHaveAttribute("aria-selected", "true");
    expect(logTab).toHaveAttribute("aria-selected", "false");

    await user.click(logTab);
    expect(logTab).toHaveAttribute("aria-selected", "true");
    expect(variablesTab).toHaveAttribute("aria-selected", "false");

    await user.click(variablesTab);
    expect(variablesTab).toHaveAttribute("aria-selected", "true");
  });

  it("opens the execution panel automatically when a run starts", async () => {
    const user = await openEditor();
    const app = document.querySelector(".app");
    expect(app).not.toHaveClass("execution-open");

    await user.click(runButton());
    expect(app).toHaveClass("execution-open");
  });

  it("shows the failed state once a run errors out", async () => {
    const user = await openEditor();
    // There's no real Tauri backend in this test environment, so
    // `invoke("run_flow_yaml", ...)` rejects — exercising the exact
    // same error path a real failed run takes, rather than a
    // hand-mocked stand-in for it.
    await user.click(runButton());
    await waitFor(() => expect(screen.getByText("実行失敗")).toBeInTheDocument());
  });

  it("keeps the execution panel's height override present once set", async () => {
    // A full drag can't be simulated meaningfully in jsdom (no real
    // layout to drag against), so this checks the persisted-size
    // plumbing instead: once a height is in localStorage, the very
    // next mount applies it as the panel's resize floor/ceiling
    // stayed respected.
    localStorage.setItem("relay.executionHeight", String(EXECUTION_MIN_HEIGHT));
    const user = await openEditor();
    await user.click(executionToggleButton());
    const app = document.querySelector(".app") as HTMLElement;
    expect(app.style.getPropertyValue("--execution-h")).toBe(`${EXECUTION_MIN_HEIGHT}px`);
    localStorage.removeItem("relay.executionHeight");
  });
});
