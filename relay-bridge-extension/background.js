// Relay Bridge — connects this browser to the Relay desktop app via
// Chrome's Native Messaging, so a `Browser*` flow step can navigate,
// click, read, and fill a specific tab. A `LaunchBrowser` step opens
// that tab via `open_tab` (below), which hands back the tab's real
// Chrome id; later `Browser*` steps address it by that id through
// `params.tabId` (see `resolveTabId`). This two-layer addressing
// exists because `LaunchBrowser` can either spawn a genuinely
// separate browser process (its own window, its own
// `--user-data-dir` profile — a distinct connection the Relay app
// picks between, see `browser-bridge`'s doc comment) *or*, when no
// dedicated profile is requested, just open another tab in whatever
// instance of the user's own browser is already running — in which
// case several `LaunchBrowser` instances end up sharing this one
// extension connection, and `params.tabId` is the only thing that
// still tells them apart.
//
// Why Native Messaging rather than a plain WebSocket the app listens
// on: a WebSocket server any local process can dial into is also a
// WebSocket server any *web page* (in some other, unrelated tab) can
// dial into — cross-origin `WebSocket` isn't blocked by the
// same-origin policy the way `fetch` is, so only the server checking
// `Origin` would stop that, and nothing stops a non-browser local
// process either. Native Messaging's host is spawned by Chrome itself,
// only for the specific extension id listed in the host manifest's
// `allowed_origins` — no open network port, no page or process able
// to impersonate this extension by guessing a port number.
//
// Why not just leave the tab's own dev tools open and drive it via
// CDP: CDP requires relaunching the browser with a debug flag (a
// separate profile, so the user's actual logged-in session isn't the
// one being automated) and only one CDP client can usually hold the
// debug port at a time. This extension instead rides along in the
// browser the user already has open, with whatever tab and session
// they're already using.

const HOST_NAME = "dev.relay.app.bridge";
let port = null;
let reconnectTimer = null;

function connect() {
  console.log("[Relay Bridge] connect() called, port =", port);
  if (port) return;
  try {
    port = chrome.runtime.connectNative(HOST_NAME);
    console.log("[Relay Bridge] connectNative() returned a port, waiting for it to settle…");
  } catch (err) {
    console.error("[Relay Bridge] connectNative() threw synchronously:", err);
    scheduleReconnect();
    return;
  }

  // Captures `port` into `replyPort` right away, rather than reading
  // the outer `let port` again after `await runAction` returns —
  // `runAction` can take a while (a navigation, a wait_for_selector
  // poll), and if this port disconnects/reconnects in the meantime
  // (a service-worker restart, Relay restarting, ...) the outer
  // `port` variable gets reassigned to the *new* connection or to
  // `null`. Replying on whatever it points to by then would either
  // post the response to a connection that never saw the request (a
  // reply the Relay side can't match to anything real, since it comes
  // in with the old request's `id` on a port whose `pending` map
  // doesn't have it) or throw on a `null` port. Replying on the
  // specific port that actually received the request avoids both.
  const replyPort = port;
  replyPort.onMessage.addListener(async (request) => {
    console.log("[Relay Bridge] received command:", request);
    const { id, action, params } = request;
    let message;
    try {
      const result = await runAction(action, params ?? {});
      message = { id, ok: true, result };
    } catch (err) {
      message = { id, ok: false, error: String((err && err.message) || err) };
    }
    try {
      replyPort.postMessage(message);
    } catch (err) {
      // The port disconnected while `runAction` was still running —
      // nothing to reply to anymore; the native host on the other end
      // is gone too, so there's no reconnect-and-retry that would help.
      console.warn("[Relay Bridge] couldn't post reply, port already disconnected:", err);
    }
  });

  port.onDisconnect.addListener(() => {
    // chrome.runtime.lastError is set here when the native host
    // never started at all (e.g. Relay isn't running, or isn't
    // registered) — surfaced to the console rather than left silent,
    // since there's otherwise no sign anything went wrong until a
    // flow step times out.
    console.log("[Relay Bridge] port disconnected, lastError =", chrome.runtime.lastError);
    port = null;
    scheduleReconnect();
  });
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, 2000);
}

// A Manifest V3 service worker gets suspended after periods of
// inactivity — a live `connectNative` port is supposed to keep it
// alive on its own, but this alarm is a cheap defensive fallback in
// case that connection ever silently drops without a disconnect event
// (kept from the WebSocket-based version, where it was load-bearing
// rather than just a backstop).
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

const TAB_LOAD_TIMEOUT_MS = 30000;

// Navigation can hang forever (a page that never fires "complete"), and
// the tab can be closed by the user mid-wait — neither used to be
// handled, so the listener leaked and the calling flow step hung
// indefinitely instead of failing. Both paths now clean up every
// listener and settle the promise one way or another.
function waitForTabLoad(tabId) {
  return new Promise((resolve, reject) => {
    let settled = false;
    function cleanup() {
      chrome.tabs.onUpdated.removeListener(updateListener);
      chrome.tabs.onRemoved.removeListener(removeListener);
      clearTimeout(timer);
    }
    function finish() {
      if (settled) return;
      settled = true;
      cleanup();
      resolve();
    }
    function fail(message) {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error(message));
    }
    function updateListener(id, info) {
      if (id === tabId && info.status === "complete") finish();
    }
    function removeListener(id) {
      if (id === tabId) fail(`tab ${tabId} was closed before it finished loading`);
    }
    const timer = setTimeout(() => fail(`tab ${tabId} did not finish loading within ${TAB_LOAD_TIMEOUT_MS}ms`), TAB_LOAD_TIMEOUT_MS);
    chrome.tabs.onUpdated.addListener(updateListener);
    chrome.tabs.onRemoved.addListener(removeListener);
    // In case navigation already finished before this listener attached.
    chrome.tabs.get(tabId, (tab) => {
      if (chrome.runtime.lastError) {
        fail(`tab ${tabId} no longer exists`);
      } else if (tab && tab.status === "complete") {
        finish();
      }
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
