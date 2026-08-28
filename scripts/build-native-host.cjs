// Builds crates/native-host, working around a real Windows lock issue:
// with the browser extension installed and enabled, Chrome/Comet
// respawn relay-native-host.exe every couple of seconds (their own
// reconnect timer), so the exe is essentially always held open by
// *some* process by the time a rebuild tries to replace it — cargo's
// own "remove the old file, write the new one" step then fails with
// "Access is denied" no matter how quickly you retry.
//
// Renaming the locked file out of the way first sidesteps this: an
// open exe *can* be renamed on Windows even while a process still has
// it mapped (that process keeps running against the old, now-unnamed
// file handle until it exits), which is exactly how self-updating
// Windows apps replace their own running executable. This script does
// that rename (best-effort — if it fails, the plain cargo build below
// still runs and reports its own real error), then builds normally.
const { existsSync, readdirSync, renameSync, unlinkSync } = require("fs");
const { execFileSync } = require("child_process");
const path = require("path");

const release = process.argv.includes("--release");
const profileDir = release ? "release" : "debug";
const targetDir = path.join(__dirname, "..", "target", profileDir);
const exePath = path.join(targetDir, "relay-native-host.exe");

// Best-effort sweep of previous runs' renamed-aside copies — whichever
// of them are no longer locked (the process that was holding it has
// since exited) get cleaned up instead of accumulating forever.
if (existsSync(targetDir)) {
  for (const name of readdirSync(targetDir)) {
    if (!name.startsWith("relay-native-host.exe.old-")) continue;
    try {
      unlinkSync(path.join(targetDir, name));
    } catch {
      // Still locked — leave it for next time.
    }
  }
}

let renamedAsidePath = null;
if (existsSync(exePath)) {
  const asideName = `${exePath}.old-${Date.now()}`;
  try {
    renameSync(exePath, asideName);
    renamedAsidePath = asideName;
  } catch {
    // Not locked, or some other reason it couldn't move — let the
    // build itself surface whatever actually goes wrong.
  }
}

const args = ["build", "-p", "native-host"];
if (release) args.push("--release");
try {
  execFileSync("cargo", args, { stdio: "inherit" });
} catch (err) {
  // The build failed after the previous exe was already renamed
  // aside above — without restoring it, the app is left with *no*
  // relay-native-host.exe at this path at all (both dev startup and
  // the release bundle expect one here), which is worse than just
  // keeping the old, still-working one in place until the next
  // successful build. `stdio: "inherit"` already surfaced cargo's own
  // real error to the console.
  if (renamedAsidePath && existsSync(renamedAsidePath) && !existsSync(exePath)) {
    try {
      renameSync(renamedAsidePath, exePath);
    } catch {
      // Couldn't restore it either — nothing more to do here; the
      // build failure below is still the real, reportable error.
    }
  }
  process.exit(typeof err.status === "number" ? err.status : 1);
}
