//! Talking to the real `claude` binary.
//!
//! `claude auth login` is interactive (it opens a browser for OAuth, runs a local
//! callback server and may ask to paste a code), so it always gets the terminal
//! untouched: inherited stdin/stdout/stderr, nothing captured.

use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::state::{Env, Result};

/// The real claude: `claude` on PATH (the shell function is invisible to child
/// processes), or CLAUDE_SWITCHER_CLAUDE when set.
pub fn command() -> Command {
    Command::new(std::env::var_os("CLAUDE_SWITCHER_CLAUDE").unwrap_or_else(|| "claude".into()))
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

pub fn spawn_error(e: std::io::Error) -> String {
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

/// How many values an option of claude takes, read from `claude --help`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arity {
    Flag,
    One,
    Optional,
    Many,
}

/// Every option `claude --help` lists, with its arity. Empty if unreadable.
pub fn option_arities() -> HashMap<String, Arity> {
    let Ok(out) = command().arg("--help").stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return HashMap::new();
    };
    parse_options(&String::from_utf8_lossy(&out.stdout))
}

fn parse_options(help: &str) -> HashMap<String, Arity> {
    let mut opts = HashMap::new();
    let mut inside = false;
    for line in help.lines() {
        if line.trim_end() == "Options:" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        // "  -r, --resume [value]    Description": names and value up to the
        // first run of two spaces; descriptions and wrapped lines go deeper.
        let Some(rest) = line.strip_prefix("  ").filter(|r| r.starts_with('-')) else { continue };
        let head = rest.split("  ").next().unwrap_or_default();
        let mut names = vec![];
        let mut arity = Arity::Flag;
        for tok in head.split_whitespace() {
            if tok.starts_with('-') {
                names.push(tok.trim_end_matches(',').to_owned());
            } else if tok.starts_with('<') {
                arity = if tok.contains("...") { Arity::Many } else { Arity::One };
            } else if tok.starts_with('[') {
                arity = if tok.contains("...") { Arity::Many } else { Arity::Optional };
            }
        }
        for n in names {
            opts.insert(n, arity);
        }
    }
    opts
}

/// Options that pick which conversation claude opens; a resume replaces them.
const SESSION_OPTIONS: &[&str] = &["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr"];

/// The arguments that resume `session` in place of the original launch: the same
/// options, minus the prompt (already in the conversation) and minus any choice
/// of conversation, plus `--resume <session>`.
pub fn resume_args(args: &[OsString], session: &str, arities: &HashMap<String, Arity>) -> Vec<OsString> {
    let mut out = vec![];
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        i += 1;
        if arg == "--" {
            break; // everything after it is the prompt
        }
        if !arg.starts_with('-') || arg == "-" {
            continue; // the prompt
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, _)) if arg.starts_with("--") => (n.to_owned(), true),
            _ => (arg.to_string(), false),
        };
        let start = i - 1;
        if !inline {
            let value_next = |i: usize| args.get(i).is_some_and(|a| !a.to_string_lossy().starts_with('-'));
            match arities.get(&name).copied().unwrap_or(Arity::Flag) {
                Arity::Flag => {}
                Arity::One => i = (i + 1).min(args.len()),
                Arity::Optional => {
                    if value_next(i) {
                        i += 1;
                    }
                }
                Arity::Many => {
                    while value_next(i) {
                        i += 1;
                    }
                }
            }
        }
        if !SESSION_OPTIONS.contains(&name.as_str()) {
            out.extend_from_slice(&args[start..i]);
        }
    }
    out.push("--resume".into());
    out.push(session.into());
    out
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

/// `claude auth logout`, quietly: callers report the outcome in their own words.
pub fn logout(env: &Env, config_dir: &Path) -> Result<bool> {
    let out = command_for(env, config_dir)
        .args(["auth", "logout"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(spawn_error)?;
    Ok(out.success())
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

    const HELP: &str = "Usage: claude [options] [command] [prompt]\n\nOptions:\n\
        \x20 --add-dir <directories...>            Additional directories\n\
        \x20 --allowedTools, --allowed-tools <tools...>\n\
        \x20     Comma or space-separated list\n\
        \x20 -c, --continue                        Continue\n\
        \x20 --dangerously-skip-permissions        Bypass\n\
        \x20 -d, --debug [filter]                  Debug\n\
        \x20 --model <model>                       Model\n\
        \x20 -r, --resume [value]                  Resume\n\
        \x20 --session-id <uuid>                   Session\n\nCommands:\n  auth  Auth\n";

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn options_come_with_their_arity() {
        let o = parse_options(HELP);
        assert_eq!(o["--add-dir"], Arity::Many);
        assert_eq!(o["--allowed-tools"], Arity::Many);
        assert_eq!(o["--allowedTools"], Arity::Many);
        assert_eq!(o["-c"], Arity::Flag);
        assert_eq!(o["--debug"], Arity::Optional);
        assert_eq!(o["--model"], Arity::One);
        assert_eq!(o["-r"], Arity::Optional);
        assert!(!o.contains_key("auth"));
    }

    #[test]
    fn a_resume_keeps_options_and_drops_prompt_and_session_choice() {
        let o = parse_options(HELP);
        let r = |a: &[&str]| resume_args(&os(a), "S", &o);
        assert_eq!(r(&[]), os(&["--resume", "S"]));
        assert_eq!(r(&["--dangerously-skip-permissions"]), os(&["--dangerously-skip-permissions", "--resume", "S"]));
        assert_eq!(r(&["-c", "--model", "opus"]), os(&["--model", "opus", "--resume", "S"]));
        assert_eq!(r(&["--resume", "old", "--debug"]), os(&["--debug", "--resume", "S"]));
        assert_eq!(r(&["-r", "--model=opus"]), os(&["--model=opus", "--resume", "S"]));
        assert_eq!(r(&["--session-id", "u", "fix the bug"]), os(&["--resume", "S"]));
        assert_eq!(
            r(&["--add-dir", "a", "b", "--debug", "api", "hi"]),
            os(&["--add-dir", "a", "b", "--debug", "api", "--resume", "S"])
        );
        assert_eq!(r(&["--model", "opus", "--", "-p looks like a flag"]), os(&["--model", "opus", "--resume", "S"]));
    }
}
