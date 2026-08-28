//! A tiny local server that bridges the flow engine to a companion
//! browser extension — the "browser automation" half of Relay's
//! action set (click/read/fill inside a real, already-open browser
//! tab) that native Win32 UI Automation has no way to reach, since a
//! web page's DOM isn't exposed as desktop controls.
//!
//! Deliberately NOT using Chrome DevTools Protocol here: CDP requires
//! relaunching the browser with a debug flag (a separate profile,
//! losing the user's actual logged-in session) and fights over the
//! debug port with any other CDP-based tool already attached.
//!
//! **Transport.** The extension talks to this process via Chrome's
//! own [Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
//! mechanism, not a raw socket the extension dials directly — Chrome
//! itself spawns a small relay process (`crates/native-host`) for the
//! *specific* extension id listed in the native-host manifest's
//! `allowed_origins` (see `register_native_host`), and owns that
//! process's stdin/stdout for as long as the extension's
//! `chrome.runtime.connectNative()` port stays open. That relay
//! process does nothing but copy length-prefixed frames (`framing.rs`
//! — the exact format Native Messaging itself uses) between its own
//! stdio and a named pipe this server listens on. Earlier versions of
//! this crate ran a plain WebSocket server any local process (or, far
//! worse, any web page in some *other* browser tab — cross-origin
//! `WebSocket` isn't blocked by the same-origin policy the way
//! `fetch` is) could dial into directly; Native Messaging's OS-level
//! process spawning plus the extension-id allowlist closes that off
//! at the browser boundary, and the named pipe's default ACL
//! (same user + admins only, no open network port at all) closes off
//! the rest.
//!
//! **Instances.** Each `Browser*` step's `instance` field is set once
//! — by `LaunchBrowser`, into a flow variable — and then just
//! addresses that exact connection (and, within it, that exact tab —
//! see `automation::launch_browser_instance`'s doc comment) every
//! time it's referenced after that, the same way an I2C device
//! address always reaches the same chip: no re-guessing which
//! connection is "probably" the right one on every single command.
//! All the guessing this module does happens *once*, in
//! `automation::launch_browser_instance`, at the moment that address
//! is first minted:
//!
//! - If `LaunchBrowser` is spawning a genuinely separate browser
//!   process (its own window, its own profile) whose Relay Bridge
//!   extension was already installed into that profile beforehand,
//!   [`spawn_instance`] correlates the resulting connection by *timing*: it registers
//!   itself as "the next connection belongs to me" immediately before
//!   spawning the process, and whichever connection arrives next is
//!   assigned that instance's id. Unambiguous, since nothing else is
//!   connecting at that exact moment.
//! - If it's instead reusing a browser that's already running (no
//!   dedicated `profile_dir` — the default), there's no new
//!   connection event to correlate with at all, so
//!   `automation::launch_browser_instance` looks up which existing
//!   connection is *already* identified (via `identify`, using the
//!   OS process behind each connection's native-host relay — not a
//!   guess) as the specific browser the step asked for, and reuses
//!   that.
//!
//! A connection that shows up with nothing waiting (the original
//! zero-config workflow: the user manually loads the extension into a
//! browser they already have open, never having gone through
//! [`spawn_instance`] at all) becomes the implicit "default" instance
//! — `send_command`'s `instance: None` reaches whichever connection
//! was most recently established, matching this crate's original
//! single-connection behavior exactly for flows that never mention an
//! instance.
//!
//! This crate owns a private Tokio runtime so [`send_command`] can be
//! called synchronously from any thread — including the engine's
//! plain `AutomationBackend` trait methods — regardless of whether
//! the caller happens to be inside some other async context.

mod identify;
pub mod framing;
pub mod native_host_registration;
pub mod rendezvous;

use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::os::windows::io::AsRawHandle;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{mpsc, oneshot};

/// The auth token every `native-host` connection must present (as
/// its first frame) before this server treats it as trusted — see
/// `rendezvous`'s doc comment. Set once, by `start_server`, before
/// any connection can arrive.
static EXPECTED_TOKEN: OnceCell<[u8; 32]> = OnceCell::new();

static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to start browser-bridge runtime")
});

static BRIDGE: OnceCell<Bridge> = OnceCell::new();

#[derive(Serialize)]
struct Envelope<'a> {
    id: &'a str,
    action: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct Reply {
    id: String,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

struct Bridge {
    /// One entry per live extension connection, keyed by the instance
    /// id `spawn_instance` handed back (or an auto-generated one for
    /// a connection nobody was waiting on).
    connections: Mutex<HashMap<String, mpsc::UnboundedSender<String>>>,
    /// The id/order a connection was established in — `None`'s
    /// fallback ("whichever connection is current") is "the highest
    /// value here", i.e. the most recently connected instance.
    connection_order: Mutex<HashMap<String, u64>>,
    /// Which browser (`"chrome"`/`"comet"`/`"edge"`/...) owns each
    /// connection, identified via `identify::browser_id_for_pipe_client`
    /// — `None` when it couldn't be determined (non-Windows, or the
    /// owning process query failed). Used by `spawn_instance`'s
    /// fallback to pick a same-browser connection instead of
    /// literally any connection when several different browsers are
    /// connected at once.
    connection_browser: Mutex<HashMap<String, Option<String>>>,
    next_order: AtomicU64,
    /// Set by `spawn_instance` just before it spawns a process; the
    /// next connection identified (via `identify`) as belonging to
    /// the `String` browser id here claims the instance id and
    /// fulfills the waiting sender, instead of being assigned an
    /// auto-generated one. Guards against an unrelated browser's
    /// extension reconnecting (e.g. after a reload) in the timing gap
    /// while `spawn_instance` is waiting — without checking the
    /// browser id, that unrelated reconnect would silently claim the
    /// slot meant for the browser actually being launched, and the
    /// caller would end up routing commands into the wrong window.
    awaiting_connect: Mutex<Option<(String, String, oneshot::Sender<()>)>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
}

impl Bridge {
    fn new() -> Self {
        Bridge {
            connections: Mutex::new(HashMap::new()),
            connection_order: Mutex::new(HashMap::new()),
            connection_browser: Mutex::new(HashMap::new()),
            next_order: AtomicU64::new(0),
            awaiting_connect: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
        }
    }
}

fn bridge() -> &'static Bridge {
    BRIDGE.get_or_init(Bridge::new)
}

/// Starts the named-pipe server — safe to call more than once (e.g.
/// defensively on every app launch); later calls are no-ops as long
/// as the first pipe instance was created. Every connection is a
/// `native-host` relay process Chrome itself spawned (see this
/// module's doc comment) — never a page or process dialing straight
/// in over the network, since there's no network listener here at
/// all.
///
/// Generates a fresh [`rendezvous`] (random pipe name + auth token)
/// on every call rather than using a fixed, compiled-in name — see
/// `rendezvous`'s doc comment for why a fixed name isn't safe once
/// `.first_pipe_instance(true)` turned out to be unusable (below).
/// The rendezvous info is only published to disk (where
/// `crates/native-host` reads it) *after* this server confirms it's
/// actually listening, closing the pre-creation race down to nothing.
pub fn start_server() {
    let rendezvous = rendezvous::generate();
    if EXPECTED_TOKEN.set(rendezvous.token).is_err() {
        // Already started once this process; nothing to do.
        return;
    }
    let (ready_tx, ready_rx) = oneshot::channel();
    start_server_at(rendezvous.pipe_name.clone(), Some(ready_tx));
    let ready = RUNTIME.block_on(async { tokio::time::timeout(Duration::from_secs(5), ready_rx).await });
    if !matches!(ready, Ok(Ok(()))) {
        eprintln!("browser-bridge: pipe server did not confirm startup in time; not publishing rendezvous info");
        return;
    }
    if let Err(e) = rendezvous::publish(&rendezvous) {
        eprintln!("browser-bridge: failed to publish pipe rendezvous info: {e}");
    }
}

/// The actual implementation, parameterized over the pipe name so
/// tests can run against a private, uniquely-named pipe instead of a
/// real [`rendezvous`]-generated one — a fixed test name would
/// otherwise be a magnet for *actual* browser connections on a dev
/// machine where Chrome/Comet are sitting there auto-reconnecting
/// every couple of seconds. `ready`, if given, fires once the first
/// pipe instance is confirmed created — see `start_server`.
fn start_server_at(pipe_name: String, ready: Option<oneshot::Sender<()>>) {
    bridge();
    RUNTIME.spawn(async move {
        // Deliberately NOT using `.first_pipe_instance(true)` here,
        // despite it being the textbook defense against a malicious
        // process pre-creating this pipe name before Relay starts and
        // sitting there intercepting connections meant for the real
        // server: tried it, and on real hardware it made
        // `ServerOptions::create` fail with `ERROR_ACCESS_DENIED`
        // *even on a completely clean pipe name nothing else had ever
        // touched* — confirmed by a direct A/B rebuild (same exe,
        // flag on vs. off) and by successfully creating a pipe of the
        // exact same name from a separate, unrelated process (a
        // `System.IO.Pipes.NamedPipeServerStream` from PowerShell)
        // while Relay's own attempt with the flag set was still
        // failing. Root cause not fully pinned down (a
        // tokio/windows-rs interaction, or a security-descriptor
        // detail `first_pipe_instance` needs that isn't being met
        // here) — but the practical effect was Relay's bridge server
        // never starting *at all*, on every launch, on two separate
        // machines, which is a strictly worse outcome than the
        // pre-creation attack this flag defends against. That attack
        // is instead defended against by generating a fresh,
        // unpredictable pipe name and token every launch (see
        // `rendezvous`) rather than relying on this flag at all.
        let mut server = match ServerOptions::new().create(&pipe_name) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("browser-bridge: failed to create named pipe {pipe_name}: {e}");
                return;
            }
        };
        if let Some(ready) = ready {
            let _ = ready.send(());
        }
        loop {
            if let Err(e) = server.connect().await {
                eprintln!("browser-bridge: pipe connect failed: {e}");
                // The instance that just failed to connect is no
                // longer usable — replace it before looping back, or
                // every later `.connect()` call would fail the same
                // way forever.
                server = match ServerOptions::new().create(&pipe_name) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("browser-bridge: failed to recreate named pipe {pipe_name}: {e}");
                        return;
                    }
                };
                continue;
            }
            let connected = server;
            // The next instance has to exist *before* `connected` is
            // handed off to its own task below — otherwise a client
            // that tries to connect in the gap between "this instance
            // just got claimed" and "the next one exists" finds no
            // pipe instance waiting at all and fails outright, rather
            // than just waiting its turn. This bit us for real: the
            // very first connection worked, then every one after it
            // silently had nothing left to connect to.
            server = match ServerOptions::new().create(&pipe_name) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("browser-bridge: failed to create the next named pipe instance: {e}");
                    return;
                }
            };
            RUNTIME.spawn(handle_connection(connected));
        }
    });
}

/// True once at least one extension instance is connected — lets the
/// UI show "browser bridge: connected/not connected" instead of
/// leaving a browser-step failure as the first sign anything was
/// wrong.
pub fn is_connected() -> bool {
    BRIDGE.get().map(|b| !b.connections.lock().unwrap().is_empty()).unwrap_or(false)
}

/// Spawns `program` (a browser executable) with `args`, and waits up
/// to `timeout` for the extension inside the window it opens to dial
/// back in. Returns the instance id newly assigned to that
/// connection — every later `Browser*`/`send_command` call passes
/// this as `instance` to reach that exact window specifically.
///
/// Only ever call this when a fresh process is actually expected —
/// `automation::launch_browser_instance` checks
/// [`find_connection_for_browser`] first and skips spawning entirely
/// when that browser already has a connection to reuse instead. The
/// correlation here is purely by timing (see this module's doc
/// comment) — `awaiting_connect` is armed *before* the process is
/// spawned so there is no window where a fast-connecting extension
/// could arrive before anyone was watching for it — which only holds
/// up when the caller already knows this really will be a new
/// process, not one that might silently delegate to an existing one.
pub fn spawn_instance(program: &str, args: &[String], timeout: Duration, expected_browser_id: &str) -> Result<String, String> {
    let b = bridge();
    let id = format!("instance_{}", b.next_order.fetch_add(1, Ordering::SeqCst) + 1);
    let (tx, rx) = oneshot::channel();
    *b.awaiting_connect.lock().unwrap() = Some((id.clone(), expected_browser_id.to_string(), tx));

    let spawn_result = Command::new(program).args(args).spawn();
    if let Err(e) = spawn_result {
        b.awaiting_connect.lock().unwrap().take();
        return Err(format!("failed to launch {program}: {e}"));
    }

    let connected = RUNTIME.block_on(async { tokio::time::timeout(timeout, rx).await });
    match connected {
        Ok(Ok(())) => Ok(id),
        _ => {
            b.awaiting_connect.lock().unwrap().take();
            Err("browser window opened but its Relay Bridge extension never connected — Chrome/Edge no longer support installing an extension automatically (a security restriction added in 2025), so it needs to already be installed and enabled (chrome://extensions) in this exact browser/profile beforehand.".into())
        }
    }
}

/// The most recently connected instance identified (via `identify`,
/// the actual OS process behind its connection — not a guess) as
/// `browser_id`, if any is currently connected. `None` means either
/// nothing matching is connected yet, or (non-Windows, or the process
/// query failed) it couldn't be determined at all — either way, the
/// caller should spawn a fresh instance instead of guessing.
pub fn find_connection_for_browser(browser_id: &str) -> Option<String> {
    let b = bridge();
    let order = b.connection_order.lock().unwrap();
    let browsers = b.connection_browser.lock().unwrap();
    order
        .iter()
        .filter(|(id, _)| browsers.get(*id).and_then(|b| b.as_deref()) == Some(browser_id))
        .max_by_key(|(_, seq)| **seq)
        .map(|(id, _)| id.clone())
}

async fn handle_connection(pipe: NamedPipeServer) {
    // Reject any connection that isn't literally this app's own
    // relay-native-host.exe before doing anything else with it — the
    // pipe name is public to every process running as the current
    // Windows user, so without this check any of them could connect
    // directly and either read automation commands meant for the
    // browser or post back fake replies. Dropping the pipe here
    // (never handed off to `handle_connection`'s reader/writer loops,
    // never registered in `connections`) just closes the connection;
    // the real native host retries on its own.
    //
    // Skipped under `#[cfg(test)]`: the round-trip test below
    // connects straight from the test binary's own process (there's
    // no real relay-native-host.exe to spawn in a unit test), so this
    // exact-process-identity check would reject every test connection
    // too — it's exercising `send_command`/routing, not this specific
    // access-control check.
    #[cfg(not(test))]
    if !identify::is_registered_native_host(pipe.as_raw_handle()) {
        eprintln!(
            "browser-bridge: rejected a pipe connection — expected relay-native-host.exe, saw {}",
            identify::describe_pipe_client(pipe.as_raw_handle())
        );
        return;
    }
    let browser_id = identify::browser_id_for_pipe_client(pipe.as_raw_handle());
    let (mut read_half, mut write_half) = tokio::io::split(pipe);

    // The auth handshake — see `rendezvous`'s doc comment. Every real
    // `native-host` sends the token it read from the rendezvous file
    // as its very first frame, before any real traffic; anything else
    // (wrong token, no token within the timeout, connection closed
    // early) means this connection isn't a `native-host` that learned
    // the pipe name legitimately, even if it somehow passed the
    // process-identity check above (e.g. a second instance of the
    // real exe pointed at this pipe name by something other than the
    // real rendezvous flow). Skipped under `#[cfg(test)]` for the same
    // reason as the identity check above: `EXPECTED_TOKEN` is never
    // set in the round-trip test, which exercises routing, not this
    // access-control layer.
    #[cfg(not(test))]
    {
        let handshake = tokio::time::timeout(Duration::from_secs(5), framing::read_frame(&mut read_half)).await;
        let presented = match handshake {
            Ok(Ok(Some(bytes))) => bytes,
            _ => {
                eprintln!("browser-bridge: rejected a pipe connection — no valid auth handshake within 5s");
                return;
            }
        };
        let expected = EXPECTED_TOKEN.get();
        if expected.is_none_or(|t| presented.as_slice() != t.as_slice()) {
            eprintln!("browser-bridge: rejected a pipe connection — auth token did not match");
            return;
        }
    }
    eprintln!("browser-bridge: accepted a native-host connection, identified browser = {browser_id:?}");
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let b = bridge();
    // Only take the waiting slot if this connection is *positively*
    // identified as the expected browser — a connection whose browser
    // couldn't be determined at all (`None`) no longer gets the
    // benefit of the doubt here, even though every connection
    // reaching this point has already passed `is_registered_native_host`
    // (so it's confirmed to be the real native-host exe): an
    // unidentified ancestor chain is still a real signal something
    // about this spawn is unexpected, and `spawn_instance` failing
    // with a clear timeout is a better outcome than silently routing
    // a `LaunchBrowser` step's commands into a connection nobody could
    // verify belongs to the browser that was actually asked for.
    let claimed = {
        let mut awaiting = b.awaiting_connect.lock().unwrap();
        let claims_awaited = awaiting.as_ref().is_some_and(|(_, expected, _)| browser_id.as_deref() == Some(expected.as_str()));
        if let Some((waiting_id, expected, _)) = awaiting.as_ref() {
            if !claims_awaited {
                eprintln!(
                    "browser-bridge: a connection arrived while spawn_instance was waiting for instance {waiting_id:?} (expected browser {expected:?}), but this connection identified as {browser_id:?} — not claiming the slot"
                );
            }
        }
        if claims_awaited { awaiting.take() } else { None }
    };
    let id = if let Some((id, _, notify)) = claimed {
        let _ = notify.send(());
        id
    } else {
        // Nobody (matching) was waiting — either the zero-config
        // workflow (the user loaded the extension into a browser by
        // hand), an extension reconnecting after a reload, or a
        // different browser than the one `spawn_instance` is
        // currently waiting for. Either way it still gets its own id
        // and becomes the new "most recent" default.
        format!("auto_{}", b.next_order.fetch_add(1, Ordering::SeqCst) + 1)
    };

    b.connections.lock().unwrap().insert(id.clone(), tx.clone());
    b.connection_order.lock().unwrap().insert(id.clone(), b.next_order.fetch_add(1, Ordering::SeqCst));
    b.connection_browser.lock().unwrap().insert(id.clone(), browser_id);

    let writer = async {
        while let Some(text) = rx.recv().await {
            if framing::write_frame(&mut write_half, text.as_bytes()).await.is_err() {
                break;
            }
        }
    };

    let reader = async {
        while let Ok(Some(bytes)) = framing::read_frame(&mut read_half).await {
            if let Ok(reply) = serde_json::from_slice::<Reply>(&bytes) {
                if let Some(sender) = bridge().pending.lock().unwrap().remove(&reply.id) {
                    let result = if reply.ok {
                        Ok(reply.result)
                    } else {
                        Err(reply.error.unwrap_or_else(|| "unknown error".into()))
                    };
                    let _ = sender.send(result);
                }
            }
        }
    };

    tokio::select! {
        _ = writer => {}
        _ = reader => {}
    }

    let mut connections = b.connections.lock().unwrap();
    if connections.get(&id).is_some_and(|current| current.same_channel(&tx)) {
        connections.remove(&id);
        b.connection_order.lock().unwrap().remove(&id);
        b.connection_browser.lock().unwrap().remove(&id);
    }
}

/// Sends `action`/`params` to `instance`'s connection (or, when
/// `instance` is `None`, whichever connection was most recently
/// established — the original single-connection behavior, still
/// exactly right for a flow that's never mentioned an instance) and
/// blocks the calling thread until it replies (or 15s elapses). Safe
/// to call from a plain synchronous function on any thread.
pub fn send_command(instance: Option<&str>, action: &str, params: Value) -> Result<Value, String> {
    send_command_with_timeout(instance, action, params, Duration::from_secs(15))
}

/// Same as [`send_command`], but with a caller-chosen timeout — used
/// for the element picker, which waits on the user actually clicking
/// something in the page rather than an instant DOM query.
pub fn send_command_with_timeout(instance: Option<&str>, action: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    let bridge = bridge();
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    bridge.pending.lock().unwrap().insert(id.clone(), tx);

    let sender = match resolve_sender(bridge, instance) {
        SenderResolution::Found(sender) => sender,
        SenderResolution::NoneConnected => {
            bridge.pending.lock().unwrap().remove(&id);
            return Err(match instance {
                Some(instance) => format!("browser instance '{instance}' is not connected (closed?)"),
                None => "browser extension is not connected — open a tab with the Relay Bridge extension enabled, or add a Launch Browser step".into(),
            });
        }
        SenderResolution::Ambiguous(count) => {
            bridge.pending.lock().unwrap().remove(&id);
            return Err(format!(
                "{count} browser connections are open at once, so this step needs an explicit instance to know which one to use — set it to the variable a Launch Browser step saved, e.g. %browser_tab%"
            ));
        }
    };

    let envelope = Envelope {
        id: &id,
        action,
        params,
    };
    let text = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    if sender.send(text).is_err() {
        bridge.pending.lock().unwrap().remove(&id);
        return Err("failed to send command to browser extension".into());
    }

    RUNTIME.block_on(async {
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("browser extension disconnected before responding".into()),
            Err(_) => {
                bridge.pending.lock().unwrap().remove(&id);
                Err("browser extension did not respond in time".into())
            }
        }
    })
}

enum SenderResolution {
    Found(mpsc::UnboundedSender<String>),
    NoneConnected,
    /// `instance: None` with more than one connection currently open —
    /// picking "whichever connected most recently" used to be a safe
    /// bet back when only one browser process ever dialed in at a
    /// time, but Native Messaging makes it routine for more than one
    /// browser (Chrome *and* some Chromium fork, say) to each have
    /// their own connection open simultaneously. Silently guessing
    /// wrong here doesn't fail loudly — it sends a command into
    /// whatever tab happens to be "most recent" instead of the one a
    /// flow author actually meant, so a later step addressing a
    /// specific instance ends up looking at a page that was never
    /// actually navigated. Erroring instead makes that mistake
    /// visible immediately, at the step that's actually ambiguous,
    /// instead of several steps later as a confusing "element not
    /// found".
    Ambiguous(usize),
}

fn resolve_sender(bridge: &Bridge, instance: Option<&str>) -> SenderResolution {
    let connections = bridge.connections.lock().unwrap();
    match instance {
        Some(id) => connections.get(id).cloned().map_or(SenderResolution::NoneConnected, SenderResolution::Found),
        None => {
            if connections.len() > 1 {
                return SenderResolution::Ambiguous(connections.len());
            }
            let order = bridge.connection_order.lock().unwrap();
            order
                .iter()
                .max_by_key(|(_, seq)| **seq)
                .and_then(|(id, _)| connections.get(id))
                .cloned()
                .map_or(SenderResolution::NoneConnected, SenderResolution::Found)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Single combined scenario (rather than several separate
    /// `#[test]` fns) because `BRIDGE`/`RUNTIME` are process-wide
    /// statics — running independent tests would let cargo's default
    /// parallel test threads race on that shared state. In order:
    /// prove a command fails fast with nothing connected, connect a
    /// fake "native-host relay" client that echoes back a canned
    /// success reply and prove `send_command(None, ...)` round-trips
    /// through the real named-pipe loop end to end, then connect a
    /// *second* fake client and prove each instance is addressable
    /// independently by id — and that `None` now refuses to guess
    /// once there's more than one connection to guess between.
    #[test]
    fn send_command_round_trips_and_addresses_instances_independently() {
        use tokio::net::windows::named_pipe::ClientOptions;

        // A private, uniquely-named pipe rather than a real
        // `rendezvous`-generated one — on a dev machine with the real
        // extension installed, Chrome/Comet auto-reconnect every
        // couple of seconds and would otherwise dial straight into
        // this test's own server the moment it starts listening on
        // the real name, polluting the connection count this test
        // asserts on.
        let pipe_name = format!(r"\\.\pipe\relay-bridge-test-{}", std::process::id());
        start_server_at(pipe_name.clone(), None);
        // Give the listener a moment to create the first pipe instance.
        std::thread::sleep(Duration::from_millis(200));

        let disconnected = send_command(None, "click", json!({}));
        assert!(disconnected.is_err());

        async fn mock_native_host(tag: &'static str, pipe_name: String) {
            let client = ClientOptions::new().open(&pipe_name).expect("mock native-host relay failed to connect");
            let (mut read_half, mut write_half) = tokio::io::split(client);
            while let Ok(Some(bytes)) = framing::read_frame(&mut read_half).await {
                let req: Value = serde_json::from_slice(&bytes).unwrap();
                let reply = json!({
                    "id": req["id"],
                    "ok": true,
                    "result": format!("echo:{tag}:{}", req["action"].as_str().unwrap()),
                });
                if framing::write_frame(&mut write_half, reply.to_string().as_bytes()).await.is_err() {
                    break;
                }
            }
        }

        RUNTIME.spawn(mock_native_host("first", pipe_name.clone()));
        std::thread::sleep(Duration::from_millis(300));

        let result = send_command(None, "click", json!({ "selector": "#go" }));
        assert_eq!(result, Ok(Value::String("echo:first:click".into())));

        RUNTIME.spawn(mock_native_host("second", pipe_name.clone()));
        std::thread::sleep(Duration::from_millis(300));

        // With two connections open at once, `None` is now ambiguous —
        // silently guessing "whichever is most recent" is exactly the
        // footgun that let a real flow send a step to the wrong
        // browser (see `SenderResolution::Ambiguous`'s doc comment).
        let result = send_command(None, "click", json!({}));
        assert!(result.is_err());

        // Both auto-assigned instance ids are still individually addressable.
        let ids: Vec<String> = bridge().connections.lock().unwrap().keys().cloned().collect();
        assert_eq!(ids.len(), 2);
        for id in &ids {
            let result = send_command(Some(id), "click", json!({}));
            assert!(result.is_ok());
        }

        let missing = send_command(Some("nonexistent"), "click", json!({}));
        assert!(missing.is_err());
    }
}
