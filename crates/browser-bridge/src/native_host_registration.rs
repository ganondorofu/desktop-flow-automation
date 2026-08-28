//! Registers this app as a Chrome/Edge Native Messaging host — writes
//! the host manifest JSON both browsers look for, plus the registry
//! key that points each of them at it. Idempotent (safe to call on
//! every launch); both writes are cheap and always overwrite with the
//! current exe's own path, so a moved/reinstalled app re-registers
//! itself automatically the next time it starts rather than needing a
//! separate installer step.
//!
//! **Why the extension id is hardcoded.** Chrome only spawns a native
//! host for an extension id it finds in the manifest's
//! `allowed_origins` — and an *unpacked* extension (this project's
//! `relay-bridge-extension/`, loaded via developer mode rather than the
//! Chrome Web Store) normally gets a fresh, random id every time it's
//! reloaded, which would make any fixed allowlist useless. Pinning
//! `relay-bridge-extension/manifest.json`'s `"key"` field to a specific
//! RSA public key makes Chrome derive the *same* id every time
//! instead (`SHA-256(DER-encoded public key)`'s first 16 bytes, each
//! nibble mapped through `0..15 -> 'a'..'p'`). The keypair behind that
//! `"key"` value was generated once, offline, purely to get a stable
//! id — nothing here ever signs anything with it, so the private half
//! was discarded immediately after; recovering it would need
//! regenerating a *different* key (and id) from scratch, not
//! "cracking" the public one.
pub const EXTENSION_ID: &str = "ahneaplcpkohdalpgiiigmejakbbibfj";

const HOST_NAME: &str = "dev.relay.app.bridge";

/// Strips a Windows "verbatim"/extended-length path prefix
/// (`\\?\C:\...`) if present. `std::env::current_exe()` can return a
/// path in this form (observed in practice: a debug build launched
/// via `target\debug\relay.exe` directly), and while `\\?\` paths
/// work fine for this app's own file I/O, Chrome apparently doesn't
/// handle one in a native-messaging host manifest's `"path"` field —
/// it fails to launch the host with no error surfaced anywhere on
/// this app's side (Chrome owns that spawn, and the native host
/// itself never gets far enough to log anything), which looks
/// identical to "the extension never connected" from every other
/// possible cause. A bare drive-letter path works in every case this
/// path can realistically take, so stripping the prefix is safe.
fn strip_verbatim_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

fn manifest_path() -> std::io::Result<std::path::PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "%LOCALAPPDATA% is not set"))?;
    let dir = std::path::PathBuf::from(base).join("Relay");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("native-host-manifest.json"))
}

/// Writes the native-host manifest pointing at `native_host_exe`, and
/// registers it with both Chrome and Edge. `native_host_exe` should
/// be `native-host.exe`'s own path (the thin stdio<->pipe relay in
/// `crates/native-host`), not this app's own main executable — Chrome
/// launches exactly that path with no extra arguments, so it can't be
/// this app's exe plus a flag telling it to behave differently.
pub fn register(native_host_exe: &std::path::Path) -> std::io::Result<()> {
    // Also tells `identify::is_registered_native_host` what a
    // legitimate pipe connection's exe path should look like — see
    // that function's doc comment for why a full-path comparison
    // instead of a name-only one.
    crate::identify::set_expected_native_host_path(native_host_exe);
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Relay Bridge native messaging host",
        "path": strip_verbatim_prefix(&native_host_exe.to_string_lossy()),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    let path = manifest_path()?;
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;

    imp::write_registry_pointer(r"Software\Google\Chrome\NativeMessagingHosts", &path)?;
    // Not fatal if a given browser isn't installed — its key simply
    // goes unused in that case, same as any of the others' would.
    //
    // Edge, Vivaldi, and Opera all read native-messaging host entries
    // out of Chrome's own `Software\Google\Chrome\NativeMessagingHosts`
    // key on Windows in addition to (or instead of) any key of their
    // own, so the write above already covers them — confirmed for
    // Edge and Opera by Chrome/Edge/Opera's own native-messaging docs,
    // and for Vivaldi by user reports of exactly this fallback
    // behavior (Vivaldi has no working `Software\Vivaldi\
    // NativeMessagingHosts` key of its own on Windows). Comet has
    // been observed to work the same way in this project's own
    // testing, but that's undocumented, unverified compatibility
    // behavior, not a guarantee — if a future Comet version stops
    // reading Chrome's key, this stops working for it silently.
    //
    // Brave is the one browser confirmed to need its *own* explicit
    // key (it does not fall back to Chrome's) — write it too.
    let _ = imp::write_registry_pointer(r"Software\Microsoft\Edge\NativeMessagingHosts", &path);
    let _ = imp::write_registry_pointer(r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts", &path);
    // Arc's native-messaging registry behavior on Windows is
    // undocumented and unverified here — not registered, so
    // `LaunchBrowser` with `browser: "arc"` should be expected not to
    // connect until this is confirmed one way or the other.
    Ok(())
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ};
    use windows::core::PCWSTR;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Creates (or opens) `HKCU\{browser_hosts_key}\{HOST_NAME}` and
    /// sets its default value to `manifest_path` — exactly what
    /// Chrome/Edge's own native-messaging documentation specifies as
    /// how a host registers itself per-user (no admin rights needed,
    /// unlike the HKLM alternative).
    pub fn write_registry_pointer(browser_hosts_key: &str, manifest_path: &std::path::Path) -> std::io::Result<()> {
        let full_key = format!(r"{browser_hosts_key}\{}", super::HOST_NAME);
        let key_w = wide(&full_key);
        let mut hkey = Default::default();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_w.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(status.0 as i32));
        }
        let value_w = wide(&manifest_path.to_string_lossy());
        let value_bytes = unsafe { std::slice::from_raw_parts(value_w.as_ptr() as *const u8, value_w.len() * 2) };
        let status = unsafe { RegSetValueExW(hkey, PCWSTR::null(), 0, REG_SZ, Some(value_bytes)) };
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        if status != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(status.0 as i32));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn write_registry_pointer(_browser_hosts_key: &str, _manifest_path: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
}
