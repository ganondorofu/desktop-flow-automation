//! Identifies which *specific* installed browser (`"chrome"`,
//! `"comet"`, `"edge"`, `"brave"`, ...) owns a given named-pipe
//! connection to this server — needed because [`crate::spawn_instance`]'s
//! "next connection to arrive" correlation only works when launching
//! genuinely starts a new OS process. When a `LaunchBrowser` step
//! reuses an already-running browser (no dedicated `profile_dir`, see
//! `automation::launch_browser_instance`'s doc comment), no new
//! connection ever appears — and if more than one different browser
//! (Chrome *and* Comet, say) each already has the Relay Bridge
//! extension connected, guessing "whichever connected most recently"
//! has a real chance of picking the wrong one, silently sending
//! commands into a browser the flow never asked for.
//!
//! A connection here is never the browser itself, though — Chrome's
//! Native Messaging spawns a separate small relay process
//! (`crates/native-host`) per `connectNative()` call, and *that*
//! process is what actually opens the named pipe. So identifying the
//! browser takes extra hops past what the WebSocket-based version of
//! this module used to do: look up the connecting pipe client's own
//! process id, then walk up its ancestors until one of them is
//! recognizable as the browser itself. Confirmed empirically (not
//! just from Chrome's own docs, which don't actually specify this):
//! on Windows, Chrome doesn't spawn the native-messaging host as a
//! *direct* child of `chrome.exe` — it goes through `cmd.exe` as an
//! intermediate launcher first, i.e. `chrome.exe -> cmd.exe ->
//! relay-native-host.exe`. Stopping at the immediate parent (`cmd`)
//! instead of walking past it made every reused-connection lookup
//! fail silently, which in turn made `launch_browser_instance` spawn
//! a whole new browser process on *every* `LaunchBrowser` step
//! instead of reusing the one already connected.

#[cfg(windows)]
mod imp {
    use std::os::windows::io::RawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// The full, lowercased, `\\?\`-prefix-stripped path of the exe
    /// behind `pid`, or `None` if the process can't be
    /// opened/queried (already exited, insufficient rights, ...).
    fn process_exe_path(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(handle);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            Some(normalize_path(&path))
        }
    }

    /// Lowercases and strips a Windows verbatim/extended-length
    /// prefix (`\\?\`) so paths obtained from different APIs (a
    /// registered exe path built from `std::env::current_exe`, vs.
    /// one read back via `QueryFullProcessImageNameW`) compare equal
    /// even when only one of them happens to carry the prefix.
    fn normalize_path(path: &str) -> String {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_lowercase()
    }

    /// The lowercase filename (no extension) of the exe behind `pid`
    /// — e.g. `"chrome"`, `"comet"`, `"msedge"` — or `None` if the
    /// process can't be opened/queried.
    fn process_exe_stem(pid: u32) -> Option<String> {
        process_exe_path(pid).map(|p| std::path::Path::new(&p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(p))
    }

    /// The full path of this app's own `relay-native-host.exe`, as
    /// registered via [`crate::native_host_registration::register`]
    /// — the yardstick [`is_registered_native_host`] compares an
    /// incoming pipe client against. Set once, at registration time;
    /// `is_registered_native_host` treats "never set yet" as "reject
    /// everything" rather than falling back to a name-only check,
    /// since a name-only check is exactly the bypass this exists to
    /// close (any process could rename itself to match).
    static EXPECTED_NATIVE_HOST_PATH: OnceLock<String> = OnceLock::new();

    pub fn set_expected_native_host_path(path: &std::path::Path) {
        let _ = EXPECTED_NATIVE_HOST_PATH.set(normalize_path(&path.to_string_lossy()));
    }

    /// `pid`'s parent process id, via a full process-table snapshot —
    /// Win32 has no direct "get parent of this pid" call short of the
    /// undocumented `NtQueryInformationProcess`, so this walks the
    /// same toolhelp snapshot `tasklist`/Task Manager use instead.
    fn parent_pid(pid: u32) -> Option<u32> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    if entry.th32ProcessID == pid {
                        found = Some(entry.th32ParentProcessID);
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
            found
        }
    }

    /// Process names Chrome/Edge are known to interpose between
    /// themselves and a spawned native-messaging host, on the way up
    /// the ancestor chain — never the answer itself, just a hop to
    /// keep walking past.
    const KNOWN_INTERMEDIARIES: &[&str] = &["cmd", "conhost"];

    /// Maps a browser process's exe stem to the `BrowserInfo::id`
    /// used everywhere else in this app (`automation::browser_candidates`,
    /// the Inspector's browser picker, `resolve_browser`, ...) — kept
    /// as an explicit allowlist rather than accepting whatever name
    /// turns up first past the known intermediaries, since that would
    /// misidentify a launcher/updater/helper process sitting between
    /// the real browser and the native host as the browser itself
    /// (e.g. an elevation or update-service process, or a shell like
    /// `powershell`/`explorer` if a user manually spawned the chain).
    fn known_browser_id(stem: &str) -> Option<String> {
        if let Some(id) = match stem {
            "chrome" => Some("chrome"),
            "msedge" => Some("edge"),
            "brave" => Some("brave"),
            "comet" => Some("comet"),
            "vivaldi" => Some("vivaldi"),
            "opera" => Some("opera"),
            "arc" => Some("arc"),
            _ => None,
        } {
            return Some(id.to_string());
        }
        custom_browser_id_for_stem(stem)
    }

    /// Mirrors `automation::custom_browsers_from_env`'s parsing of
    /// `RELAY_CUSTOM_BROWSERS` (`id:Display Name:C:\path\to.exe`
    /// entries separated by `;`) so a browser registered that way is
    /// also recognized here — by the *exe filename stem* of its
    /// configured path, matched case-insensitively, since that's what
    /// actually shows up in the process table. Duplicated rather than
    /// shared from the `automation` crate: `browser-bridge` doesn't
    /// depend on it (and shouldn't just for this), and the format is
    /// simple enough that keeping both parsers in sync by hand is a
    /// reasonable trade.
    fn custom_browser_id_for_stem(stem: &str) -> Option<String> {
        let spec = std::env::var("RELAY_CUSTOM_BROWSERS").ok()?;
        spec.split(';').filter(|e| !e.trim().is_empty()).find_map(|entry| {
            let mut parts = entry.splitn(3, ':');
            let id = parts.next()?.trim();
            let _name = parts.next()?;
            let path = parts.next()?.trim();
            let entry_stem = std::path::Path::new(path).file_stem()?.to_string_lossy().to_lowercase();
            (entry_stem == stem).then(|| id.to_string())
        })
    }

    /// How many ancestor hops to walk before giving up — generous
    /// enough for any intermediary chain actually seen in practice,
    /// bounded so a pathological/cyclic process table can't spin
    /// forever.
    const MAX_ANCESTOR_HOPS: u32 = 6;

    /// The browser id (`"chrome"`/`"comet"`/`"edge"`/`"brave"`/...)
    /// of whichever ancestor of the process on the other end of
    /// `pipe_handle` is a *recognized* browser executable — i.e. the
    /// browser that (however indirectly) spawned the native-host
    /// relay process holding this pipe connection. `None` if an
    /// ancestor exits mid-walk, or nothing recognized turns up within
    /// [`MAX_ANCESTOR_HOPS`] — deliberately not a guess: an
    /// unrecognized ancestor name (a helper/updater/elevation process,
    /// or a shell someone used to launch the chain manually) is
    /// skipped and walked past rather than accepted as the answer.
    pub fn browser_id_for_pipe_client(pipe_handle: RawHandle) -> Option<String> {
        let mut client_pid: u32 = 0;
        unsafe {
            GetNamedPipeClientProcessId(HANDLE(pipe_handle), &mut client_pid).ok()?;
        }
        let mut pid = client_pid;
        for _ in 0..MAX_ANCESTOR_HOPS {
            pid = parent_pid(pid)?;
            let stem = process_exe_stem(pid)?;
            if KNOWN_INTERMEDIARIES.contains(&stem.as_str()) {
                continue;
            }
            if let Some(id) = known_browser_id(&stem) {
                return Some(id.to_string());
            }
        }
        None
    }

    /// True if the process directly on the other end of
    /// `pipe_handle` is, by full path, this app's own registered
    /// `relay-native-host.exe` — the actual security boundary for
    /// `\\.\pipe\relay-bridge`, unlike [`browser_id_for_pipe_client`]
    /// above (which is best-effort, used only for *routing* between
    /// several already-accepted connections). Without this check, any
    /// other local process running as the same Windows user could
    /// open the pipe directly — impersonating the extension (sending
    /// fake command replies), reading real automation commands headed
    /// for the browser, or just holding the connection open to deny
    /// it to the real host. Compares the *full path*, not just the
    /// filename: an exe-name-only check can be defeated trivially by
    /// copying/renaming any other binary to `relay-native-host.exe`
    /// and running it from anywhere. Checked on the *immediate*
    /// client, not walked up through ancestors like the browser id
    /// is: the native host is always what actually calls
    /// `CreateFile`/connects to the pipe, so there's no intermediary
    /// to walk past here.
    pub fn is_registered_native_host(pipe_handle: RawHandle) -> bool {
        let Some(expected) = EXPECTED_NATIVE_HOST_PATH.get() else {
            return false;
        };
        let mut client_pid: u32 = 0;
        let ok = unsafe { GetNamedPipeClientProcessId(HANDLE(pipe_handle), &mut client_pid) };
        if ok.is_err() {
            return false;
        }
        process_exe_path(client_pid).as_deref() == Some(expected.as_str())
    }

    /// Diagnostic-only: describes whatever `is_registered_native_host`
    /// actually saw for `pipe_handle`'s client — the pid and, if it
    /// could be queried, its exe path — so a rejection can be logged
    /// with the real reason instead of just "rejected", making a
    /// false-positive rejection (a legitimate `relay-native-host.exe`
    /// getting turned away for some unexpected reason) diagnosable
    /// from the log alone.
    pub fn describe_pipe_client(pipe_handle: RawHandle) -> String {
        let mut client_pid: u32 = 0;
        let ok = unsafe { GetNamedPipeClientProcessId(HANDLE(pipe_handle), &mut client_pid) };
        if ok.is_err() {
            return "GetNamedPipeClientProcessId failed".to_string();
        }
        match process_exe_path(client_pid) {
            Some(path) => format!("pid {client_pid}, exe path {path:?} (expected {:?})", EXPECTED_NATIVE_HOST_PATH.get()),
            None => format!("pid {client_pid}, exe path could not be queried (process exited, or OpenProcess/QueryFullProcessImageNameW failed)"),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn browser_id_for_pipe_client(_pipe_handle: usize) -> Option<String> {
        None
    }

    // Non-Windows builds don't actually serve the named pipe at all
    // (see `lib.rs`), so there's nothing real to verify here — kept
    // permissive rather than hard-failing a platform this crate isn't
    // used on in production.
    pub fn is_registered_native_host(_pipe_handle: usize) -> bool {
        true
    }

    pub fn describe_pipe_client(_pipe_handle: usize) -> String {
        "not available on this platform".to_string()
    }

    pub fn set_expected_native_host_path(_path: &std::path::Path) {}
}

pub use imp::{browser_id_for_pipe_client, describe_pipe_client, is_registered_native_host, set_expected_native_host_path};
