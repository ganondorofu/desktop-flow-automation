//! A tiny local WebSocket server that bridges the flow engine to a
//! companion browser extension — the "browser automation" half of
//! Relay's action set (click/read/fill inside a real, already-open
//! browser tab) that native Win32 UI Automation has no way to reach,
//! since a web page's DOM isn't exposed as desktop controls.
//!
//! Deliberately NOT using Chrome DevTools Protocol here: CDP requires
//! relaunching the browser with a debug flag (a separate profile,
//! losing the user's actual logged-in session) and fights over the
//! debug port with any other CDP-based tool already attached. A
//! WebSocket the extension dials into instead needs no relaunch, no
//! port negotiation on the browser's side, and works with whatever
//! tab the user already has open.
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
//!   process (its own window, its own profile) with the Relay Bridge
//!   extension pre-loaded via `--load-extension`, [`spawn_instance`]
//!   correlates the resulting connection by *timing*: it registers
//!   itself as "the next connection belongs to me" immediately before
//!   spawning the process, and whichever connection arrives next is
//!   assigned that instance's id. Unambiguous, since nothing else is
//!   connecting at that exact moment.
//! - If it's instead reusing a browser that's already running (no
//!   dedicated `profile_dir` — the default), there's no new
//!   connection event to correlate with at all, so
//!   `automation::launch_browser_instance` looks up which existing
//!   connection is *already* identified (via `identify`, using the
//!   OS process behind each connection's TCP port — not a guess)
//!   as the specific browser the step asked for, and reuses that.
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

use futures_util::{SinkExt, StreamExt};
use once_cell::sync::{Lazy, OnceCell};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

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
    connections: Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>,
    /// The id/order a connection was established in — `None`'s
    /// fallback ("whichever connection is current") is "the highest
    /// value here", i.e. the most recently connected instance.
    connection_order: Mutex<HashMap<String, u64>>,
    /// Which browser (`"chrome"`/`"comet"`/`"edge"`/...) owns each
    /// connection, identified via `identify::browser_id_for_peer_port`
    /// — `None` when it couldn't be determined (non-Windows, or the
    /// owning process query failed). Used by `spawn_instance`'s
    /// fallback to pick a same-browser connection instead of
    /// literally any connection when several different browsers are
    /// connected at once.
    connection_browser: Mutex<HashMap<String, Option<String>>>,
    next_order: AtomicU64,
    /// Set by `spawn_instance` just before it spawns a process; the
    /// very next connection to arrive claims this id and fulfills the
    /// waiting sender instead of being assigned an auto-generated one.
    awaiting_connect: Mutex<Option<(String, oneshot::Sender<()>)>>,
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

/// Starts the WebSocket server on `127.0.0.1:port` — safe to call
/// more than once (e.g. defensively on every app launch); later calls
/// are no-ops as long as the first bind succeeded. Only ever binds to
/// loopback, never accepting a connection from outside this machine.
pub fn start_server(port: u16) {
    bridge();
    RUNTIME.spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("browser-bridge: failed to bind 127.0.0.1:{port}: {e}");
                return;
            }
        };
        loop {
            let (stream, _addr) = match listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    eprintln!("browser-bridge: accept failed: {e}");
                    continue;
                }
            };
            RUNTIME.spawn(handle_connection(stream));
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
pub fn spawn_instance(program: &str, args: &[String], timeout: Duration) -> Result<String, String> {
    let b = bridge();
    let id = format!("instance_{}", b.next_order.fetch_add(1, Ordering::SeqCst) + 1);
    let (tx, rx) = oneshot::channel();
    *b.awaiting_connect.lock().unwrap() = Some((id.clone(), tx));

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
            Err("browser window opened but its Relay Bridge extension never connected — is the extension installed and enabled in this browser?".into())
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

async fn handle_connection(stream: TcpStream) {
    let peer_port = stream.peer_addr().ok().map(|addr| addr.port());
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(_) => return,
    };
    let (mut write, mut read) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    let b = bridge();
    let awaited = b.awaiting_connect.lock().unwrap().take();
    let id = match awaited {
        Some((id, notify)) => {
            let _ = notify.send(());
            id
        }
        // Nobody was waiting — either the zero-config workflow (the
        // user loaded the extension into a browser by hand) or an
        // extension reconnecting after a reload. Either way it still
        // gets its own id and becomes the new "most recent" default.
        None => format!("auto_{}", b.next_order.fetch_add(1, Ordering::SeqCst) + 1),
    };

    b.connections.lock().unwrap().insert(id.clone(), tx.clone());
    b.connection_order.lock().unwrap().insert(id.clone(), b.next_order.fetch_add(1, Ordering::SeqCst));
    let browser_id = peer_port.and_then(identify::browser_id_for_peer_port);
    b.connection_browser.lock().unwrap().insert(id.clone(), browser_id);

    let writer = async {
        while let Some(msg) = rx.recv().await {
            if write.send(msg).await.is_err() {
                break;
            }
        }
    };

    let reader = async {
        while let Some(Ok(msg)) = read.next().await {
            if let Message::Text(text) = msg {
                if let Ok(reply) = serde_json::from_str::<Reply>(&text) {
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

    let sender = resolve_sender(bridge, instance);
    let Some(sender) = sender else {
        bridge.pending.lock().unwrap().remove(&id);
        return Err(match instance {
            Some(instance) => format!("browser instance '{instance}' is not connected (closed?)"),
            None => "browser extension is not connected — open a tab with the Relay Bridge extension enabled, or add a Launch Browser step".into(),
        });
    };

    let envelope = Envelope {
        id: &id,
        action,
        params,
    };
    let text = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    if sender.send(Message::Text(text)).is_err() {
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

fn resolve_sender(bridge: &Bridge, instance: Option<&str>) -> Option<mpsc::UnboundedSender<Message>> {
    let connections = bridge.connections.lock().unwrap();
    match instance {
        Some(id) => connections.get(id).cloned(),
        None => {
            let order = bridge.connection_order.lock().unwrap();
            order
                .iter()
                .max_by_key(|(_, seq)| **seq)
                .and_then(|(id, _)| connections.get(id))
                .cloned()
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
    /// fake "extension" client that echoes back a canned success
    /// reply and prove `send_command(None, ...)` round-trips through
    /// the real WebSocket loop end to end, then connect a *second*
    /// fake client and prove each instance is addressable
    /// independently by id while `None` still reaches whichever
    /// connected most recently.
    #[test]
    fn send_command_round_trips_and_addresses_instances_independently() {
        let port = 17_882;
        start_server(port);
        // Give the listener a moment to bind.
        std::thread::sleep(Duration::from_millis(200));

        let disconnected = send_command(None, "click", json!({}));
        assert!(disconnected.is_err());

        async fn mock_extension(port: u16, tag: &'static str) {
            let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await.expect("mock extension failed to connect");
            let (mut write, mut read) = ws.split();
            while let Some(Ok(Message::Text(text))) = read.next().await {
                let req: Value = serde_json::from_str(&text).unwrap();
                let reply = json!({
                    "id": req["id"],
                    "ok": true,
                    "result": format!("echo:{tag}:{}", req["action"].as_str().unwrap()),
                });
                if write.send(Message::Text(reply.to_string())).await.is_err() {
                    break;
                }
            }
        }

        RUNTIME.spawn(mock_extension(port, "first"));
        std::thread::sleep(Duration::from_millis(300));

        let result = send_command(None, "click", json!({ "selector": "#go" }));
        assert_eq!(result, Ok(Value::String("echo:first:click".into())));

        RUNTIME.spawn(mock_extension(port, "second"));
        std::thread::sleep(Duration::from_millis(300));

        // `None` now reaches the second (more recently connected) client.
        let result = send_command(None, "click", json!({}));
        assert_eq!(result, Ok(Value::String("echo:second:click".into())));

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
