//! The wire format Chrome's own Native Messaging protocol uses: each
//! message is a 4-byte length (native/little-endian on Windows)
//! followed by exactly that many bytes of UTF-8 content. Reused
//! unchanged for the local hop between the native-host relay process
//! (`crates/native-host`) and this crate's named-pipe server, so the
//! relay never needs to understand the payload at all — it just
//! copies whole frames in both directions between Chrome's stdio and
//! the pipe. See
//! <https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging>.
//!
//! Chrome enforces an *asymmetric* size limit on its own side of this
//! protocol: a message *from* the native host is capped at 1 MiB, but
//! a message *to* it (i.e. from the extension, via `port.postMessage`)
//! is allowed up to 64 MiB — plenty of headroom for a `screenshot`
//! step's base64 PNG data URL, which routinely exceeds 1 MiB on its
//! own. An earlier version of this reader applied the *stricter* of
//! those two limits to both directions uniformly, which meant any
//! screenshot larger than ~750 KB of raw PNG data made `read_frame`
//! return an `Err` — not just failing that one step, but tearing down
//! the whole native-messaging connection outright (the caller's
//! `while let Ok(Some(bytes)) = read_frame(...)` loop exits on any
//! `Err`). The limit here now matches Chrome's own *inbound* ceiling
//! instead, since that's the direction size actually matters for in
//! practice; Chrome still independently enforces its own 1 MiB cap on
//! whatever this process sends outward, so relaxing our own inbound
//! check doesn't let anything through that Chrome wouldn't otherwise.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_FRAME_LEN: u32 = 64 * 1024 * 1024;

/// Reads one length-prefixed frame, or `None` on a clean EOF exactly
/// at a frame boundary (the peer closed the connection normally).
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("frame of {len} bytes exceeds the {MAX_FRAME_LEN}-byte limit")));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

/// Writes one length-prefixed frame.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large to send"))?;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(payload).await?;
    writer.flush().await
}
