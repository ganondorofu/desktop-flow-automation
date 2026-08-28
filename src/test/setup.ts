import "@testing-library/jest-dom/vitest";
import "../i18n";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// `test.globals` isn't enabled in vitest.config.ts (this file imports
// `describe`/`it`/`expect` explicitly instead), so
// `@testing-library/react`'s own auto-cleanup — which hooks into a
// *global* `afterEach` — never fires on its own. Without this, each
// test's `render(<App />)` would pile up on top of the previous
// test's undisposed tree instead of starting from a clean document.
afterEach(cleanup);

// jsdom doesn't implement `Element.prototype.scrollTo` — the run-log
// tab autoscrolls to its latest entry, which would otherwise throw
// every time a test switches to it.
Element.prototype.scrollTo ??= () => {};

// `@xyflow/react` (the canvas) measures its container via
// ResizeObserver — jsdom has no rendering engine and therefore no
// real implementation of it, so every test that mounts `<App />`
// would otherwise throw "ResizeObserver is not defined" before
// getting anywhere near what it's actually testing.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
window.ResizeObserver ??= ResizeObserverStub;

// Some libraries in the dependency tree probe `matchMedia` at import
// or mount time; jsdom doesn't implement it either.
window.matchMedia ??= (query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addListener: () => {},
  removeListener: () => {},
  addEventListener: () => {},
  removeEventListener: () => {},
  dispatchEvent: () => false,
});
