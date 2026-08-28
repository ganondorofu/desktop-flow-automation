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

use browser_bridge::framing::{read_frame, write_frame};
use browser_bridge::PIPE_NAME;
use tokio::net::windows::named_pipe::ClientOptions;

/// The main app might not have finished starting its pipe server yet
/// (e.g. Chrome launched this a moment after Relay itself started),
/// or the pipe might be transiently busy with another client — retry
/// briefly rather than failing on the very first attempt.
async fn connect_with_retry() -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let mut last_err = None;
    for _ in 0..20 {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(client) => return Ok(client),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
    Err(last_err.unwrap())
}

#[tokio::main]
async fn main() {
    let Ok(pipe) = connect_with_retry().await else {
        // Nothing to relay to — Relay itself likely isn't running.
        // Exiting quietly closes Chrome's native-messaging port, which
        // the extension surfaces as a normal disconnect rather than a
        // hang.
        return;
    };
    let (mut pipe_read, mut pipe_write) = tokio::io::split(pipe);
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let stdin_to_pipe = async {
        while let Ok(Some(bytes)) = read_frame(&mut stdin).await {
            if write_frame(&mut pipe_write, &bytes).await.is_err() {
                break;
            }
        }
    };
    let pipe_to_stdout = async {
        while let Ok(Some(bytes)) = read_frame(&mut pipe_read).await {
            if write_frame(&mut stdout, &bytes).await.is_err() {
                break;
            }
        }
    };

    tokio::select! {
        _ = stdin_to_pipe => {}
        _ = pipe_to_stdout => {}
    }
}
