# Relay Bridge (browser extension)

Lets Relay flows drive whatever page is open in your browser — navigate,
click, read text, and fill fields — in the browser and tab you're
already using, with your existing session.

## Install (unpacked, once)

1. Open `chrome://extensions` (or `edge://extensions` on Edge).
2. Turn on **Developer mode** (top right).
3. Click **Load unpacked** and select this `browser-extension/` folder.

Relay's desktop app hosts a local WebSocket server on
`ws://127.0.0.1:17845`; this extension connects to it automatically
whenever it's running, and reconnects automatically if the app
restarts.

## What it can do

The `Browser*` steps in Relay act on your active tab:

- **Navigate** — loads a URL.
- **Click** — clicks the first element matching a CSS selector.
- **Get Text** — reads an element's text (or its value, for form
  fields) into a variable.
- **Set Value** — fills an `<input>`/`<textarea>`/`<select>`.
- **Wait for Element** — polls until a selector appears in the page.

## Known limitation

Chrome suspends this extension's background worker after ~30 seconds
of inactivity, which drops the WebSocket connection. The extension
wakes itself back up roughly every 30 seconds to reconnect, so a
`Browser*` step can occasionally take a couple of seconds longer than
expected right after the extension has been idle — this is a Manifest
V3 platform limitation, not something this extension can fully avoid.
