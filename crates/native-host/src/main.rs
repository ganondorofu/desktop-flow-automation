//! Chrome/Edge spawns this process directly (via the registered
//! Native Messaging host manifest — see
//! `browser_bridge::native_host_registration`) whenever the Relay
//! Bridge extension calls `chrome.runtime.connectNative()`, and owns
//! its stdin/stdout for as long as that port stays open. This process
//! does nothing but copy whole length-prefixed frames
//! (`browser_bridge::framing` — Chrome's own Native Messaging wire
//! format, reused unchanged) between that stdio pair and a named pipe
//! the main Relay app is listening on: Chrome talks to *this*, this
//! talks to *that*, and neither side needs to know the other exists.
//!
//! Deliberately dumb: it never parses the JSON inside a frame, just
//! moves bytes. Keeping every actual protocol/automation decision in
//! the main app (which already has to run trusted, privileged
//! automation regardless) means this process — spawned by Chrome
//! itself, technically "less trusted" input than the rest of the app
//! ever handles directly — has as little surface as possible to get
//! wrong.
//!
//! Which pipe to connect to (and the token proving it's really this
//! app on the other end) come from `browser_bridge::rendezvous`
//! rather than a fixed, compiled-in name — see that module's doc
//! comment.
//!
//! **Logging.** Chrome owns this process's stdout (the actual
//! native-messaging wire) and doesn't capture stderr anywhere visible
//! — a bare `eprintln!` here would vanish into nothing. Diagnostics
//! instead go to `%LOCALAPPDATA%\Relay\native-host.log` (appended, not
//! truncated, so a whole failure sequence across several of Chrome's
//! respawn attempts stays readable in one file).

use browser_bridge::framing::{read_frame, write_frame};
use browser_bridge::rendezvous;
use std::io::Write;
use tokio::net::windows::named_pipe::ClientOptions;

fn log(msg: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = std::path::PathBuf::from(base).join("Relay");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("native-host.log")) {
        let _ = writeln!(file, "[pid {}] {msg}", std::process::id());
    }
}

/// The main app might not have finished starting its pipe server (and
/// publishing its rendezvous file) yet — e.g. Chrome launched this a
/// moment after Relay itself started — or the pipe might be
/// transiently busy with another client. Retry both reading the
/// rendezvous file and connecting rather than failing on the very
/// first attempt.
async fn connect_with_retry() -> std::io::Result<(tokio::net::windows::named_pipe::NamedPipeClient, [u8; 32])> {
    let mut last_err = None;
    for attempt in 0..40 {
        match rendezvous::read() {
            Ok(r) => match ClientOptions::new().open(&r.pipe_name) {
                Ok(client) => {
                    log(&format!("connected on attempt {attempt} to {}", r.pipe_name));
                    return Ok((client, r.token));
                }
                Err(e) => {
                    if attempt == 0 || attempt == 39 {
                        log(&format!("attempt {attempt}: rendezvous read ok (pipe {}), but ClientOptions::open failed: {e}", r.pipe_name));
                    }
                    last_err = Some(e);
                }
            },
            Err(e) => {
                if attempt == 0 || attempt == 39 {
                    log(&format!("attempt {attempt}: rendezvous::read() failed: {e}"));
                }
                last_err = Some(e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Err(last_err.unwrap())
}

#[tokio::main]
async fn main() {
    log("starting");
    let Ok((pipe, token)) = connect_with_retry().await else {
        // Nothing to relay to — Relay itself likely isn't running.
        // Exiting quietly closes Chrome's native-messaging port, which
        // the extension surfaces as a normal disconnect rather than a
        // hang.
        log("giving up after 40 attempts (10s) — exiting");
        return;
    };
    let (mut pipe_read, mut pipe_write) = tokio::io::split(pipe);
    // The auth handshake `handle_connection` on the server side
    // requires as the very first frame, before any real traffic —
    // see `rendezvous`'s doc comment.
    if let Err(e) = rendezvous::send_token(&mut pipe_write, &token).await {
        log(&format!("failed to send auth token: {e}"));
        return;
    }
    log("auth token sent, relaying stdio<->pipe");
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let stdin_to_pipe = async {
        while let Ok(Some(bytes)) = read_frame(&mut stdin).await {
            if write_frame(&mut pipe_write, &bytes).await.is_err() {
                log("stdin->pipe: pipe write failed, stopping");
                break;
            }
        }
        log("stdin->pipe: stdin closed (Chrome disconnected the extension port)");
    };
    let pipe_to_stdout = async {
        while let Ok(Some(bytes)) = read_frame(&mut pipe_read).await {
            if write_frame(&mut stdout, &bytes).await.is_err() {
                log("pipe->stdout: stdout write failed, stopping");
                break;
            }
        }
        log("pipe->stdout: pipe closed (Relay disconnected or exited)");
    };

    tokio::select! {
        _ = stdin_to_pipe => {}
        _ = pipe_to_stdout => {}
    }
    log("exiting");
}
