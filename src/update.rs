//! Knowing when to update, and updating.
//!
//! The check follows the common CLI convention (gh, npm's update-notifier,
//! rustup): the notice goes to stderr at the end of a command, only on an
//! interactive terminal, never in CI, never with --json, and the network is
//! consulted at most once a day by a detached background process, so no
//! command ever waits on it. `doctor` reports the same thing headless.
//!
//! Updating does not reimplement installation: `update` runs the official
//! installer from the repository, which always knows how to install the latest
//! stable release, whatever its layout. GitHub's "latest release" skips
//! pre-releases and drafts, so only stable versions are ever offered.

use serde_json::{Value, json};
use std::io::IsTerminal;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::state::{Env, Result};
use crate::ui;

const LATEST_URL: &str = "https://api.github.com/repos/diananerd/claude-account-switcher/releases/latest";
// Straight from GitHub, not the short switcher.diananerd.com address: updating
// must not depend on anything but the repository.
const INSTALLER_URL: &str = "https://raw.githubusercontent.com/diananerd/claude-account-switcher/main/install.sh";
const DAY: u64 = 24 * 60 * 60;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cache_file(env: &Env) -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| env.home.join(".cache"))
        .join("claude-account/update.json")
}

fn disabled() -> bool {
    std::env::var_os("CLAUDE_ACCOUNT_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty())
}

/// A version as (major, minor, patch, pre-release suffix); a leading `v` is
/// allowed. `0.2.0-rc.1` is ((0, 2, 0), Some("rc.1")).
type Version<'a> = ((u64, u64, u64), Option<&'a str>);

fn parse(v: &str) -> Option<Version<'_>> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let mut it = core.split('.');
    let n = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    it.next().is_none().then_some((n, pre))
}

pub fn is_prerelease(v: &str) -> bool {
    parse(v).is_some_and(|(_, pre)| pre.is_some())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Level {
    Patch,
    Minor,
    Major,
}

/// How far `current` is behind the stable `latest`, or None when it is not.
/// Semver order: a pre-release comes before its own release, so 0.2.0-rc.1 is
/// behind 0.2.0 but ahead of 0.1.9. Pre-release `latest` values never count.
pub fn behind(current: &str, latest: &str) -> Option<Level> {
    let ((c, cpre), (l, lpre)) = (parse(current)?, parse(latest)?);
    if lpre.is_some() {
        return None;
    }
    let newer = l > c || (l == c && cpre.is_some());
    if !newer {
        return None;
    }
    Some(if l.0 != c.0 {
        Level::Major
    } else if l.1 != c.1 {
        Level::Minor
    } else {
        Level::Patch
    })
}

/// What to say about `current` next to the stable `latest`: (up to date, text).
fn standing(current: &str, latest: &str) -> (bool, String) {
    match behind(current, latest) {
        Some(_) => (false, format!("claude-account {latest} is available (you have {current})")),
        None if is_prerelease(current) => (
            true,
            format!(
                "claude-account {current} is a pre-release, newer than the latest stable {latest}; \
                 to go back to it: claude-account update --version v{latest}"
            ),
        ),
        None => (true, format!("claude-account {current} is the latest stable release")),
    }
}

/// Ask GitHub for the latest stable version (blocking, short timeout).
pub fn fetch_latest() -> Result<String> {
    let url = std::env::var("CLAUDE_ACCOUNT_UPDATE_URL").unwrap_or_else(|_| LATEST_URL.into());
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "5", "-H", "Accept: application/vnd.github+json", &url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !out.status.success() {
        return Err("cannot reach GitHub".into());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "unexpected answer from GitHub".to_string())?;
    let tag = v.get("tag_name").and_then(Value::as_str).ok_or("no tag in GitHub's answer")?;
    Ok(tag.trim_start_matches('v').to_string())
}

/// Hidden `refresh-update-cache`: fetch and store, quietly.
pub fn refresh(env: &Env) -> Result<ExitCode> {
    let file = cache_file(env);
    let latest = fetch_latest().ok();
    let body = json!({ "checked": now(), "latest": latest });
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = file.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, body.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
    Ok(ExitCode::SUCCESS)
}

/// End-of-command notice. Reads the cached answer; when it is older than a day,
/// starts a detached refresh for next time. Never blocks, never fails.
pub fn notice(env: &Env) {
    if disabled() || std::env::var_os("CI").is_some() || !std::io::stderr().is_terminal() {
        return;
    }
    let file = cache_file(env);
    let cached: Option<Value> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok());
    let checked = cached.as_ref().and_then(|v| v.get("checked")).and_then(Value::as_u64).unwrap_or(0);
    if now().saturating_sub(checked) > DAY
        && let Ok(exe) = std::env::current_exe()
    {
        // Mark the attempt first, so a slow network never starts two at once.
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let latest = cached.as_ref().and_then(|v| v.get("latest")).cloned().unwrap_or(Value::Null);
        let _ = std::fs::write(&file, json!({ "checked": now(), "latest": latest }).to_string());
        let mut cmd = Command::new(exe);
        cmd.arg("refresh-update-cache").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // Its own session: closing the terminal right away must not kill it.
        // SAFETY: setsid is async-signal-safe and touches no parent state.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let _ = cmd.spawn();
    }
    let Some(latest) = cached.as_ref().and_then(|v| v.get("latest")).and_then(Value::as_str) else { return };
    let Some(level) = behind(CURRENT, latest) else { return };
    let msg = format!("claude-account {latest} is available (you have {CURRENT}). Update: claude-account update");
    match level {
        Level::Patch => ui::info(&msg),
        Level::Minor => ui::warning(&format!("{msg} (new features)")),
        Level::Major => ui::danger(&format!("{msg} (major release: check the changelog first)")),
    }
}

/// How this copy was installed, when a package manager owns it.
fn managed_by(exe: &Path, env: &Env) -> Option<String> {
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/") || s.contains("/homebrew/") {
        Some("brew upgrade claude-account".into())
    } else if exe.starts_with(env.home.join(".cargo")) {
        Some(
            "cargo install --locked --git https://github.com/diananerd/claude-account-switcher claude-account-switcher"
                .into(),
        )
    } else if s.contains("/target/debug/") || s.contains("/target/release/") {
        Some("git pull && cargo build --release (this is a development build)".into())
    } else {
        None
    }
}

/// `update [--version TAG]`: run the official installer into this
/// binary's own folder. Interactive in a terminal (the installer asks), `-y`
/// otherwise.
pub fn update(env: &Env, version: Option<String>, mode: crate::Mode) -> Result<ExitCode> {
    let exe =
        std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok()).ok_or("cannot locate this binary")?;
    if let Some(cmd) = managed_by(&exe, env) {
        ui::hint(&format!("this copy is managed elsewhere; update it with: {cmd}"));
        return Ok(ExitCode::SUCCESS);
    }
    if version.is_none() {
        match fetch_latest() {
            Ok(latest) if behind(CURRENT, &latest).is_none() => {
                let (_, text) = standing(CURRENT, &latest);
                if is_prerelease(CURRENT) {
                    ui::info(&text)
                } else {
                    ui::done(&text)
                }
                let _ = std::fs::remove_file(cache_file(env));
                return Ok(ExitCode::SUCCESS);
            }
            Ok(_) => {}
            Err(e) => ui::warning(&format!("could not check the latest version ({e}); running the installer anyway")),
        }
    }
    let dir = exe.parent().ok_or("cannot locate this binary's folder")?;
    let url = std::env::var("CLAUDE_ACCOUNT_INSTALLER_URL").unwrap_or_else(|_| INSTALLER_URL.into());
    let mut args = vec!["-s".to_string(), "--".into(), "--bin-dir".into(), dir.to_string_lossy().into_owned()];
    if !mode.prompt || mode.yes {
        args.push("--yes".into());
    }
    if let Some(v) = version {
        args.extend(["--version".into(), v]);
    }
    let script = Command::new("curl")
        .args(["-fsSL", "--max-time", "30", &url])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    if !script.status.success() {
        return Err(format!("cannot download the installer from {url}; check your connection"));
    }
    let mut child = Command::new("sh").args(&args).stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    std::io::Write::write_all(child.stdin.as_mut().ok_or("cannot start the installer")?, &script.stdout)
        .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(cache_file(env));
    Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::from(status.code().unwrap_or(1) as u8) })
}

/// For doctor: (level, message, fix).
pub fn doctor_line() -> Option<(bool, String, Option<String>)> {
    if disabled() {
        return None;
    }
    Some(match fetch_latest() {
        Ok(latest) => {
            let (ok, text) = standing(CURRENT, &latest);
            (ok, text, (!ok).then(|| "claude-account update".to_string()))
        }
        Err(e) => (false, format!("could not check for updates ({e})"), Some("check your connection".into())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_semver() {
        assert_eq!(behind("0.1.3", "0.1.4"), Some(Level::Patch));
        assert_eq!(behind("0.1.3", "v0.2.0"), Some(Level::Minor));
        assert_eq!(behind("0.1.3", "1.0.0"), Some(Level::Major));
        assert_eq!(behind("0.1.3", "0.1.3"), None);
        assert_eq!(behind("0.2.0", "0.1.9"), None);
        assert_eq!(behind("0.1.3", "0.2.0-rc.1"), None, "pre-releases are never offered");
        assert_eq!(behind("0.1.3", "garbage"), None);
        // Running a pre-release: its own release is an update, older stables are not.
        assert_eq!(behind("0.2.0-rc.1", "0.2.0"), Some(Level::Patch));
        assert_eq!(behind("0.2.0-rc.1", "0.1.9"), None);
        assert_eq!(behind("0.2.0-rc.1", "0.2.1"), Some(Level::Patch));
        assert!(standing("0.2.0-rc.1", "0.1.9").1.contains("is a pre-release, newer than the latest stable 0.1.9"));
        assert!(standing("0.1.9", "0.1.9").1.contains("is the latest stable release"));
    }
}
