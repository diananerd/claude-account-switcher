//! Talking to the real `claude` binary.
//!
//! `claude auth login` is interactive (it opens a browser for OAuth, runs a local
//! callback server and may ask to paste a code), so it always gets the terminal
//! untouched: inherited stdin/stdout/stderr, nothing captured.

use serde_json::Value;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::state::{Env, Result};

/// The real claude: `claude` on PATH (the shell function is invisible to child
/// processes), or CLAUDE_ACCOUNT_CLAUDE when set.
pub fn command() -> Command {
    Command::new(std::env::var_os("CLAUDE_ACCOUNT_CLAUDE").unwrap_or_else(|| "claude".into()))
}

/// A claude command bound to a config dir: the base dir runs with
/// CLAUDE_CONFIG_DIR unset, any other with it set.
pub fn command_for(env: &Env, config_dir: &Path) -> Command {
    let mut cmd = command();
    if env.is_base(config_dir) {
        cmd.env_remove("CLAUDE_CONFIG_DIR");
    } else {
        cmd.env("CLAUDE_CONFIG_DIR", config_dir);
    }
    cmd
}

fn spawn_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "claude not found. Install Claude Code: https://code.claude.com/docs/en/setup".into()
    } else {
        format!("cannot run claude: {e}")
    }
}

/// Names of claude's subcommands (aliases included), parsed from the
/// "Commands:" section of `claude --help`. Empty if that cannot be read.
pub fn subcommands() -> Vec<String> {
    let Ok(out) = command().arg("--help").stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return vec![];
    };
    parse_subcommands(&String::from_utf8_lossy(&out.stdout))
}

fn parse_subcommands(help: &str) -> Vec<String> {
    let mut names = vec![];
    let mut inside = false;
    for line in help.lines() {
        if line.trim_end() == "Commands:" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        // Entries sit at a two-space indent; wrapped descriptions go deeper.
        if let Some(rest) = line.strip_prefix("  ")
            && !rest.starts_with(' ')
            && let Some(word) = rest.split_whitespace().next()
        {
            names.extend(word.split('|').map(str::to_owned));
        }
    }
    names
}

pub fn version() -> Option<String> {
    let out = command().arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Debug, Clone, Default)]
pub struct Auth {
    pub logged_in: bool,
    pub email: Option<String>,
    pub org: Option<String>,
}

/// Authoritative login state of a config dir, from `claude auth status`.
pub fn auth_status(env: &Env, config_dir: &Path) -> Result<Auth> {
    let out = command_for(env, config_dir)
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(spawn_error)?;
    let v: Value =
        serde_json::from_slice(&out.stdout).map_err(|_| "unexpected output from `claude auth status`".to_string())?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    Ok(Auth {
        logged_in: v.get("loggedIn").and_then(Value::as_bool).unwrap_or(false),
        email: s("email"),
        org: s("orgName"),
    })
}

/// Fast, offline guess of the account behind a config dir: what Claude Code
/// cached in `.claude.json`. Use `auth_status` when it has to be right.
pub fn cached_email(env: &Env, config_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(env.claude_json(config_dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.pointer("/oauthAccount/emailAddress")?.as_str().map(str::to_owned)
}

/// `claude auth login` with the terminal handed over.
pub fn login(env: &Env, config_dir: &Path, args: &[OsString]) -> Result<bool> {
    let status = command_for(env, config_dir).args(["auth", "login"]).args(args).status().map_err(spawn_error)?;
    Ok(status.success())
}

pub fn logout(env: &Env, config_dir: &Path) -> Result<bool> {
    let status =
        command_for(env, config_dir).args(["auth", "logout"]).stdin(Stdio::null()).status().map_err(spawn_error)?;
    Ok(status.success())
}

/// Replace this process with claude. Only returns on failure.
pub fn exec(mut cmd: Command) -> String {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let err = cmd.exec();
    if err.kind() == std::io::ErrorKind::NotFound {
        format!("{program} not found. Install Claude Code: https://code.claude.com/docs/en/setup")
    } else {
        format!("cannot run {program}: {err}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subcommands_come_from_the_commands_section() {
        let help = "Usage: claude [options] [command] [prompt]\n\nOptions:\n  -p, --print  Print\n\n\
                    Commands:\n  auth                     Manage auth\n  install|i [options]      Install\n\
                    \x20                          wrapped description line\n  plugin|plugins           Plugins\n";
        assert_eq!(parse_subcommands(help), vec!["auth", "install", "i", "plugin", "plugins"]);
        assert!(parse_subcommands("no commands here").is_empty());
    }
}
