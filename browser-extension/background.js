// Relay Bridge — connects this browser to the Relay desktop app over a
// local WebSocket, so a `Browser*` flow step can navigate, click,
// read, and fill a specific tab. A `LaunchBrowser` step opens that
// tab via `open_tab` (below), which hands back the tab's real Chrome
// id; later `Browser*` steps address it by that id through
// `params.tabId` (see `resolveTabId`). This two-layer addressing
// exists because `LaunchBrowser` can either spawn a genuinely
// separate browser process (its own window, its own
// `--user-data-dir` profile — a distinct WebSocket connection the
// Relay app picks between, see `browser-bridge`'s doc comment) *or*,
// when no dedicated profile is requested, just open another tab in
// whatever instance of the user's own browser is already running —
// in which case several `LaunchBrowser` instances end up sharing this
// one extension connection, and `params.tabId` is the only thing that
// still tells them apart.
//
// Why WebSocket instead of Chrome's Native Messaging: native messaging
// needs a host manifest registered in the OS registry pointing at a
// helper executable, which is one more install step and one more thing
// that can go stale after an update. A plain WebSocket the app already
// listens on needs no OS-level registration at all — install this
// extension once, and it finds the app whenever it's running.
//
// Why not just leave the tab's own dev tools open and drive it via
// CDP: CDP requires relaunching the browser with a debug flag (a
// separate profile, so the user's actual logged-in session isn't the
// one being automated) and only one CDP client can usually hold the
// debug port at a time. This extension instead rides along in the
// browser the user already has open, with whatever tab and session
// they're already using.

const PORT = 17845;
let ws = null;
let reconnectTimer = null;

function connect() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
  ws = new WebSocket(`ws://127.0.0.1:${PORT}`);

  ws.onopen = () => {
    console.log("[Relay Bridge] connected to the Relay app");
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  ws.onclose = () => {
    scheduleReconnect();
  };

  ws.onerror = () => {
    // onclose fires right after; the reconnect is scheduled there.
    ws.close();
  };

  ws.onmessage = async (event) => {
    let request;
    try {
      request = JSON.parse(event.data);
    } catch {
      return;
    }
    const { id, action, params } = request;
    try {
      const result = await runAction(action, params ?? {});
      ws.send(JSON.stringify({ id, ok: true, result }));
    } catch (err) {
      ws.send(JSON.stringify({ id, ok: false, error: String((err && err.message) || err) }));
    }
  };
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, 2000);
}

// A Manifest V3 service worker gets suspended after ~30s of
// inactivity, silently dropping the WebSocket with it. This alarm
// periodically wakes the worker back up so a dropped connection
// doesn't stay dropped for longer than the alarm interval.
chrome.alarms.create("relay-bridge-keepalive", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(() => connect());
chrome.runtime.onStartup.addListener(() => connect());
chrome.runtime.onInstalled.addListener(() => connect());

async function activeTabId() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab) throw new Error("no active browser tab to act on");
  return tab.id;
}

// `LaunchBrowser` opens its tab via `open_tab` (below) rather than
// relying on whatever tab happens to become active afterward — this
// browser process may be shared by several `LaunchBrowser` instances
// at once (the common case now that a step with no explicit
// `profile_dir` reuses the user's own already-running browser), so
// "the active tab" is ambiguous by the time a later `Browser*` step
// runs. `params.tabId` (the real Chrome tab id `open_tab` returned,
// captured into the flow's instance variable) targets that specific
// tab instead. Falls back to `activeTabId()` when absent, so a flow
// that never went through `open_tab` (or was authored before this
// existed) still works.
async function resolveTabId(params) {
  if (params.tabId === undefined || params.tabId === null) return activeTabId();
  const tabId = Number(params.tabId);
  try {
    await chrome.tabs.get(tabId);
  } catch {
    throw new Error(`browser instance tab ${tabId} no longer exists (closed?)`);
  }
  return tabId;
}

function waitForTabLoad(tabId) {
  return new Promise((resolve) => {
    function finish() {
      chrome.tabs.onUpdated.removeListener(listener);
      resolve();
    }
    function listener(id, info) {
      if (id === tabId && info.status === "complete") finish();
    }
    chrome.tabs.onUpdated.addListener(listener);
    // In case navigation already finished before this listener attached.
    chrome.tabs.get(tabId, (tab) => {
      if (tab && tab.status === "complete") finish();
    });
  });
}

async function runAction(action, params) {
  if (action === "open_tab") {
    // The backend half of `LaunchBrowser` — opens a fresh tab (in the
    // background by default, so it doesn't steal focus from whatever
    // the user is doing) and hands back its real tab id as the
    // "instance" every later `Browser*` step addresses via
    // `params.tabId`.
    const tab = await chrome.tabs.create({ url: params.url || "about:blank", active: params.active === true });
    if (params.url) await waitForTabLoad(tab.id);
    return tab.id;
  }

  if (action === "navigate") {
    const tabId = await resolveTabId(params);
    await chrome.tabs.update(tabId, { url: params.url });
    await waitForTabLoad(tabId);
    return null;
  }

  if (action === "screenshot") {
    // `captureVisibleTab` only ever captures whichever tab is
    // currently active in its window — there's no Chrome API to
    // screenshot an arbitrary background tab directly — so this
    // brings the target tab to the front first if it isn't already
    // (a real, visible side effect, unavoidable given the API).
    const tabId = await resolveTabId(params);
    const tab = await chrome.tabs.get(tabId);
    if (!tab.active) {
      await chrome.tabs.update(tabId, { active: true });
    }
    return await chrome.tabs.captureVisibleTab(tab.windowId, { format: "png" });
  }

  if (action === "pick_element") {
    return await pickElementAcrossTabs();
  }

  if (action === "cancel_pick") {
    const tabs = await chrome.tabs.query({});
    for (const t of tabs) {
      if (t.id !== undefined) chrome.tabs.sendMessage(t.id, { __relayCancelPick: true }).catch(() => {});
    }
    return null;
  }

  const tabId = await resolveTabId(params);
  const [{ result }] = await chrome.scripting.executeScript({
    target: { tabId },
    func: pageAction,
    args: [action, params],
  });
  if (result && result.__error) throw new Error(result.__error);
  return result ? result.value : null;
}

// Element picking isn't limited to whichever tab was active when
// picking started — the user may switch tabs first and click in
// whatever tab they land on. So instead of injecting the picker into
// one tab and awaiting its result, it's injected into every open
// (http/https) tab at once; whichever one actually gets clicked
// reports back over `chrome.runtime.sendMessage`, and every other tab
// is then told to tear its overlay down.
async function pickElementAcrossTabs() {
  const tabs = await chrome.tabs.query({});
  const targets = tabs.filter((t) => t.id !== undefined && /^https?:/.test(t.url ?? ""));
  if (targets.length === 0) throw new Error("no browser tab available to pick an element from");

  await Promise.all(
    targets.map((t) =>
      chrome.scripting.executeScript({ target: { tabId: t.id }, func: startPickerOnPage }).catch(() => {}),
    ),
  );

  let picked;
  try {
    picked = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        chrome.runtime.onMessage.removeListener(listener);
        reject(new Error("element picking timed out — click an element within 60s"));
      }, 60_000);

      function listener(message) {
        if (!message || message.__relayPick !== true) return;
        clearTimeout(timer);
        chrome.runtime.onMessage.removeListener(listener);
        if (message.error) reject(new Error(message.error));
        else resolve({ selector: message.selector, preview: message.preview });
      }
      chrome.runtime.onMessage.addListener(listener);
    });
  } finally {
    for (const t of targets) {
      chrome.tabs.sendMessage(t.id, { __relayCancelPick: true }).catch(() => {});
    }
  }
  return picked;
}

// Runs inside a page via chrome.scripting.executeScript — lets the
// user hover to highlight and click to pick an element, the same
// "point at what you mean instead of typing a selector" flow PAD's own
// element picker offers. Reports the result to the background worker
// via `chrome.runtime.sendMessage` rather than resolving/returning it
// directly, since it's injected into every open tab simultaneously and
// only one of them will ever actually get clicked. Escape cancels just
// this tab's overlay; `__relayCancelPick` (sent once some other tab
// wins) tears it down the same way.
function startPickerOnPage() {
  if (window.__relayPickerActive) return;
  window.__relayPickerActive = true;

  const overlay = document.createElement("div");
  overlay.style.cssText =
    "position:fixed;z-index:2147483647;pointer-events:none;" +
    "border:2px solid #3b82f6;background:rgba(59,130,246,0.15);" +
    "border-radius:2px;transition:all 60ms ease-out;";
  document.documentElement.appendChild(overlay);

  function updateOverlay(el) {
    const r = el.getBoundingClientRect();
    overlay.style.left = `${r.left}px`;
    overlay.style.top = `${r.top}px`;
    overlay.style.width = `${r.width}px`;
    overlay.style.height = `${r.height}px`;
  }

  function cssSelectorFor(el) {
    if (el.id) return `#${CSS.escape(el.id)}`;
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body) {
      if (node.id) {
        parts.unshift(`#${CSS.escape(node.id)}`);
        break;
      }
      let part = node.tagName.toLowerCase();
      const parent = node.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((c) => c.tagName === node.tagName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(" > ");
  }

  function onMove(e) {
    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (el && el !== overlay) updateOverlay(el);
  }

  function cleanup() {
    window.__relayPickerActive = false;
    document.removeEventListener("mousemove", onMove, true);
    document.removeEventListener("click", onClick, true);
    document.removeEventListener("keydown", onKey, true);
    chrome.runtime.onMessage.removeListener(onCancelMessage);
    overlay.remove();
  }

  function onClick(e) {
    e.preventDefault();
    e.stopPropagation();
    const el = document.elementFromPoint(e.clientX, e.clientY);
    cleanup();
    if (!el) {
      chrome.runtime.sendMessage({ __relayPick: true, error: "no element found at the clicked position" });
      return;
    }
    const selector = cssSelectorFor(el);
    const text = (el.innerText || el.value || "").trim().slice(0, 60);
    const preview = text ? `${el.tagName.toLowerCase()} "${text}"` : el.tagName.toLowerCase();
    chrome.runtime.sendMessage({ __relayPick: true, selector, preview });
  }

  function onKey(e) {
    if (e.key === "Escape") {
      cleanup();
      chrome.runtime.sendMessage({ __relayPick: true, error: "element picking was cancelled" });
    }
  }

  function onCancelMessage(message) {
    if (message && message.__relayCancelPick) cleanup();
  }

  document.addEventListener("mousemove", onMove, true);
  document.addEventListener("click", onClick, true);
  document.addEventListener("keydown", onKey, true);
  chrome.runtime.onMessage.addListener(onCancelMessage);
}

// Runs inside the page itself via chrome.scripting.executeScript — it
// only has access to the DOM and whatever it closes over in its own
// argument list, never anything from the service worker's scope.
// Always resolves to `{ value }` or `{ __error }` rather than
// rejecting, since how a thrown/rejected injected-script promise
// surfaces back to the caller is inconsistent across Chrome versions.
async function pageAction(action, params) {
  // `selector` is whatever `BrowserSelector` (crates/flow-schema)
  // serialized to: a bare string for the plain-CSS case, or
  // `{ kind: "text", value }` / `{ kind: "attribute", name, value }`
  // for the alternative match strategies — offered because a page
  // redesign can change class names out from under a CSS selector
  // while the element's own visible text or a semantic attribute
  // (`placeholder`, `aria-label`, `name`, ...) stays put.
  function tryFindElement(selector) {
    if (typeof selector === "string") return document.querySelector(selector);
    if (selector.kind === "text") return findByText(selector.value);
    if (selector.kind === "attribute") {
      for (const el of document.querySelectorAll(`[${CSS.escape(selector.name)}]`)) {
        if (el.getAttribute(selector.name) === selector.value) return el;
      }
      return null;
    }
    throw new Error(`unknown selector kind "${selector.kind}"`);
  }

  function findElement(selector) {
    const el = tryFindElement(selector);
    if (!el) throw new Error(`no element matches ${describeSelector(selector)}`);
    return el;
  }

  // The element whose own trimmed visible text equals `text` exactly —
  // walks in document order and keeps the *last* match, which (since
  // children are visited right after their parent) ends up being the
  // most specific single descendant with that exact text rather than
  // some large ancestor that merely contains it.
  function findByText(text) {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
    let best = null;
    for (let node = walker.currentNode; node; node = walker.nextNode()) {
      if (node.textContent && node.textContent.trim() === text) best = node;
    }
    return best;
  }

  function describeSelector(selector) {
    if (typeof selector === "string") return `"${selector}"`;
    if (selector.kind === "text") return `text "${selector.value}"`;
    if (selector.kind === "attribute") return `${selector.name}="${selector.value}"`;
    return JSON.stringify(selector);
  }

  function waitForSelector(selector, timeoutMs) {
    return new Promise((resolve, reject) => {
      if (tryFindElement(selector)) {
        resolve();
        return;
      }
      const timer = setTimeout(() => {
        observer.disconnect();
        reject(new Error(`${describeSelector(selector)} did not appear within ${timeoutMs}ms`));
      }, timeoutMs);
      const observer = new MutationObserver(() => {
        if (tryFindElement(selector)) {
          clearTimeout(timer);
          observer.disconnect();
          resolve();
        }
      });
      observer.observe(document.documentElement, { childList: true, subtree: true });
    });
  }

  try {
    if (action === "click") {
      findElement(params.selector).click();
      return { value: null };
    }
    if (action === "get_text") {
      const el = findElement(params.selector);
      const value = el.value !== undefined ? el.value : el.innerText;
      return { value };
    }
    if (action === "set_value") {
      const el = findElement(params.selector);
      // Directly assigning `el.value` doesn't notify frameworks
      // (React, Vue, ...) that intercept the native setter — calling
      // the prototype's setter first makes this look like a real
      // keystroke to that code, and the dispatched events cover
      // plain vanilla-JS listeners.
      const proto = Object.getPrototypeOf(el);
      const nativeSetter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
      if (nativeSetter) {
        nativeSetter.call(el, params.value);
      } else {
        el.value = params.value;
      }
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return { value: null };
    }
    if (action === "wait_for_selector") {
      await waitForSelector(params.selector, 5000);
      return { value: null };
    }
    return { __error: `unknown action "${action}"` };
  } catch (err) {
    return { __error: String((err && err.message) || err) };
  }
}

connect();
