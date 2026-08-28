//! Where this app tells `crates/native-host` which named pipe to
//! connect to, and proves to that pipe's server that it's really the
//! process the server is expecting.
//!
//! **Why not just a fixed, compiled-in pipe name.** That's what this
//! crate used to do (`PIPE_NAME`, a plain constant both sides
//! referenced). The textbook defense against a rogue local process
//! pre-creating that name before Relay ever starts — and then quietly
//! sitting in the middle of every future connection, since a
//! `native-host` relay just connects to whatever's listening under
//! that name — is `ServerOptions::first_pipe_instance(true)`,
//! refusing to create the pipe at all if something else already owns
//! the name. In practice, on real hardware, that flag made pipe
//! creation fail outright with `ERROR_ACCESS_DENIED` even against a
//! name nothing had ever touched (see `lib.rs`'s `start_server_at`),
//! so it can't be relied on here. Without it, a fixed name is
//! squattable by any same-user process, any time — including long
//! before Relay is even installed.
//!
//! **The fix**: generate a random pipe name and a random auth token
//! fresh on every launch, and only write them to disk *after* this
//! app's own pipe server is already listening on that name. A
//! would-be squatter can't pre-create a name it doesn't know yet, and
//! by the time the name becomes discoverable (this file existing),
//! the real server already owns it. The token closes the remaining
//! race even so: a squatter that somehow still manages to grab a
//! second instance of the same (by-then-known) pipe name can, at
//! worst, refuse connections — it can't complete the handshake
//! `native-host` performs immediately after connecting (send the
//! token as the very first frame), so `handle_connection` on the real
//! server side never treats an unauthenticated connection as trusted,
//! and a `native-host` that ends up talking to the squatter instead
//! just fails its handshake against *that* impostor with nothing of
//! value having crossed the wire.

use crate::framing;

pub struct Rendezvous {
    pub pipe_name: String,
    pub token: [u8; 32],
}

fn rendezvous_path() -> std::io::Result<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "%LOCALAPPDATA% is not set"))?;
    let dir = std::path::PathBuf::from(base).join("Relay");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("pipe-rendezvous.txt"))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    let mut filled = 0;
    while filled < N {
        let chunk = uuid::Uuid::new_v4();
        let bytes = chunk.as_bytes();
        let take = (N - filled).min(bytes.len());
        out[filled..filled + take].copy_from_slice(&bytes[..take]);
        filled += take;
    }
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Picks a fresh, unpredictable pipe name and token for this run.
/// Doesn't touch disk — see [`publish`].
pub fn generate() -> Rendezvous {
    let suffix = hex_encode(&random_bytes::<16>());
    Rendezvous { pipe_name: format!(r"\\.\pipe\relay-bridge-{suffix}"), token: random_bytes::<32>() }
}

/// Writes `rendezvous` to the well-known file `native-host` reads at
/// startup. Callers MUST only call this once the pipe server is
/// actually listening on `rendezvous.pipe_name` — see this module's
/// doc comment for why the ordering matters.
pub fn publish(rendezvous: &Rendezvous) -> std::io::Result<()> {
    let path = rendezvous_path()?;
    let contents = format!("{}\n{}\n", rendezvous.pipe_name, hex_encode(&rendezvous.token));
    std::fs::write(&path, contents)
}

/// Reads back whatever the currently-running Relay instance last
/// published. Used by `crates/native-host` to learn which pipe to
/// connect to and what token to present.
pub fn read() -> std::io::Result<Rendezvous> {
    let path = rendezvous_path()?;
    let contents = std::fs::read_to_string(&path)?;
    let mut lines = contents.lines();
    let pipe_name = lines.next().ok_or_else(|| invalid("missing pipe name line"))?.to_string();
    let token_hex = lines.next().ok_or_else(|| invalid("missing token line"))?;
    let token_bytes = hex_decode(token_hex).ok_or_else(|| invalid("token is not valid hex"))?;
    let token: [u8; 32] = token_bytes.try_into().map_err(|_| invalid("token is not 32 bytes"))?;
    Ok(Rendezvous { pipe_name, token })
}

fn invalid(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

/// Sends `token` as the first frame on `pipe` — the handshake
/// `native-host` performs right after connecting, before relaying any
/// real traffic. `handle_connection` on the server side reads and
/// checks exactly this before treating the connection as trusted.
pub async fn send_token<W: tokio::io::AsyncWrite + Unpin>(pipe: &mut W, token: &[u8; 32]) -> std::io::Result<()> {
    framing::write_frame(pipe, token).await
}
