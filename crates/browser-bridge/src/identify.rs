//! Identifies which *specific* installed browser (`"chrome"`,
//! `"comet"`, `"edge"`, `"brave"`, ...) owns a given WebSocket
//! connection to this server — needed because [`crate::spawn_instance`]'s
//! "next connection to arrive" correlation only works when launching
//! genuinely starts a new OS process. When a `LaunchBrowser` step
//! reuses an already-running browser (no dedicated `profile_dir`, see
//! `automation::launch_browser_instance`'s doc comment), no new
//! connection ever appears — and if more than one different browser
//! (Chrome *and* Comet, say) each already has the Relay Bridge
//! extension connected, guessing "whichever connected most recently"
//! has a real chance of picking the wrong one, silently sending
//! commands into a browser the flow never asked for. Resolving the
//! actual OS process behind each connection's local TCP port removes
//! the guessing entirely — it works regardless of whether a
//! Chromium fork's `navigator.userAgent` happens to still say
//! "Chrome" (most do, to avoid breaking sites that sniff it).

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows::Win32::Networking::WinSock::AF_INET;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// Every currently-established loopback TCP connection's local
    /// port and owning process id, via `GetExtendedTcpTable`.
    fn tcp_table() -> Vec<(u16, u32)> {
        unsafe {
            let mut size: u32 = 0;
            let _ = GetExtendedTcpTable(None, &mut size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0);
            if size == 0 {
                return Vec::new();
            }
            let mut buf = vec![0u8; size as usize];
            let ret = GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if ret != 0 {
                return Vec::new();
            }
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let num_entries = table.dwNumEntries as usize;
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), num_entries);
            rows.iter()
                .map(|row| {
                    // `dwLocalPort` packs the port into the low 16 bits
                    // in network (big-endian) byte order.
                    let port = u16::from_be((row.dwLocalPort & 0xffff) as u16);
                    (port, row.dwOwningPid)
                })
                .collect()
        }
    }

    /// The lowercase filename (no extension) of the exe behind `pid`
    /// — e.g. `"chrome"`, `"comet"`, `"msedge"` — or `None` if the
    /// process can't be opened/queried (already exited, insufficient
    /// rights, ...).
    fn process_exe_stem(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(handle);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_lowercase())
        }
    }

    /// The browser id (`"chrome"`/`"comet"`/`"edge"`/`"brave"`/...)
    /// of whichever process owns the *local* end of a loopback TCP
    /// connection whose peer (this server's) port was `peer_port` —
    /// i.e. the browser sitting on the other end of a WebSocket
    /// connection this server accepted from it. `None` if the
    /// connection has already closed, or the owning process isn't a
    /// browser this crate recognizes.
    pub fn browser_id_for_peer_port(peer_port: u16) -> Option<String> {
        let pid = tcp_table().into_iter().find(|(port, _)| *port == peer_port).map(|(_, pid)| pid)?;
        let stem = process_exe_stem(pid)?;
        Some(match stem.as_str() {
            "msedge" => "edge".to_string(),
            other => other.to_string(),
        })
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn browser_id_for_peer_port(_peer_port: u16) -> Option<String> {
        None
    }
}

pub use imp::browser_id_for_peer_port;
