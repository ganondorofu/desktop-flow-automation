# Relay Bridge (browser extension)

Lets Relay flows drive whatever page is open in your browser — navigate,
click, read text, and fill fields — in the browser and tab you're
already using, with your existing session.

## Install (unpacked, once)

1. Open `chrome://extensions` (or `edge://extensions` on Edge).
2. Turn on **Developer mode** (top right).
3. Click **Load unpacked** and select this `relay-bridge-extension/`
   folder.

Relay's desktop app registers itself as a
[Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
host the first time it runs — no manual setup needed on that side.
This extension connects to it automatically whenever Relay is
running, and reconnects automatically if Relay restarts.

**Load this exact folder, not a renamed copy.** Chrome (in this
environment, at least) has been observed silently dropping this
specific extension from `chrome://extensions` across a full restart
after the *same on-disk folder path* has been repeatedly
loaded/reloaded many times over — reproduced directly by comparing a
byte-identical copy in a fresh, never-before-loaded path (which
survived a restart fine) against the original, heavily-reloaded
folder (which didn't). Not linked to the `nativeMessaging` permission
or the manifest's pinned `key` — both were tested and ruled out. If
this ever recurs, the fix that's worked is loading a fresh copy from
a brand-new folder path rather than continuing to reload the same one
that's already showing the problem.

## What it can do

The `Browser*` steps in Relay act on a specific tab (opened via
`LaunchBrowser`, or whichever tab is active if a flow never mentions
an instance):

- **Navigate** — loads a URL.
- **Click** — clicks the first element matching a CSS selector (or a
  text/attribute match).
- **Get Text** — reads an element's text (or its value, for form
  fields) into a variable.
- **Set Value** — fills an `<input>`/`<textarea>`/`<select>`.
- **Wait for Element** — polls until a selector appears in the page.
- **Screenshot** — captures the visible tab.

## Known limitations

- Chrome (as of version 137) removed the `--load-extension`
  command-line flag from official builds, so Relay can no longer
  auto-install this extension into a freshly-spawned browser process.
  It has to already be installed (see above) in whichever browser/
  profile a flow's `LaunchBrowser` step targets — Chrome activates an
  already-installed extension on its own at every normal startup, no
  flag needed for that part.
- A Manifest V3 background service worker gets suspended after
  periods of inactivity. A live Native Messaging connection is
  supposed to keep it alive on its own; a recurring alarm is also
  wired up as a defensive fallback in case that connection ever drops
  silently.
