//! `doctor`: check the whole setup and say how to fix what is wrong.

use serde_json::json;
use std::collections::BTreeMap;
use std::process::ExitCode;

use crate::shell::{self, Shell};
use crate::state::{self, Env, Result};
use crate::{claude, paths, profiles, ui};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Ok,
    Warn,
    Error,
}

struct Report {
    items: Vec<(Level, String, Option<String>)>,
}

impl Report {
    fn ok(&mut self, msg: impl Into<String>) {
        self.items.push((Level::Ok, msg.into(), None));
    }
    fn warn(&mut self, msg: impl Into<String>, fix: impl Into<String>) {
        self.items.push((Level::Warn, msg.into(), Some(fix.into())));
    }
    fn error(&mut self, msg: impl Into<String>, fix: impl Into<String>) {
        self.items.push((Level::Error, msg.into(), Some(fix.into())));
    }
}

pub fn doctor(env: &Env, fix: bool, json: bool, mode: crate::Mode) -> Result<ExitCode> {
    let code = run(env, fix, json)?;
    // Interactive: offer the repairs instead of expecting --fix to be known.
    if !fix && !json && mode.prompt && FIXABLE.load(std::sync::atomic::Ordering::Relaxed) > 0 {
        let n = FIXABLE.load(std::sync::atomic::Ordering::Relaxed);
        let q = format!("Fix {n} of these now (relink shared files, drop deleted folders)?");
        if mode.yes || ui::confirm(&q, true) == Some(true) {
            eprintln!();
            return run(env, true, false);
        }
    }
    Ok(code)
}

/// Issues the last `run` found that `--fix` repairs.
static FIXABLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn run(env: &Env, fix: bool, json: bool) -> Result<ExitCode> {
    FIXABLE.store(0, std::sync::atomic::Ordering::Relaxed);
    let mut r = Report { items: vec![] };

    if let Some((up_to_date, msg, fix)) = crate::update::doctor_line() {
        if up_to_date {
            r.ok(msg);
        } else {
            r.warn(msg, fix.unwrap_or_default());
        }
    }

    match claude::version() {
        Some(v) => r.ok(format!("claude found: {v}")),
        None => r.error("claude is not on PATH", "install Claude Code: https://code.claude.com/docs/en/setup"),
    }

    let cfg = match env.load() {
        Ok(c) => {
            r.ok(format!("configuration readable: {}", env.tilde(&env.config_file)));
            c
        }
        Err(e) => {
            r.error(e, format!("fix or move {} aside", env.tilde(&env.config_file)));
            return finish(r, json);
        }
    };
    if cfg.profiles.is_empty() {
        r.warn("no profiles yet", "claude-switcher setup");
    }

    // Profiles and logins.
    let mut accounts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in cfg.ordered() {
        let dir = match cfg.config_dir(&name) {
            Ok(d) => d,
            Err(e) => {
                r.error(
                    format!("{name}: {e}"),
                    format!("fix it in {} or remove the profile", env.tilde(&env.config_file)),
                );
                continue;
            }
        };
        if let Some(t) = cfg.alias_target(&name) {
            r.ok(format!("{name}: same login as {t}"));
            continue;
        }
        if env.is_managed(&dir) {
            let before = profiles::link_shared_dry(env, &dir);
            if !before.missing.is_empty() && fix {
                profiles::link_shared(env, &dir)?;
                r.ok(format!("{name}: relinked {}", before.missing.join(", ")));
            } else if !before.missing.is_empty() {
                FIXABLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                r.warn(format!("{name}: not sharing {}", before.missing.join(", ")), "claude-switcher doctor --fix");
            }
            for c in before.conflicts {
                r.warn(
                    format!("{name}: {} is a real file, not shared with {}", c, env.tilde(&env.base_dir)),
                    format!(
                        "merge it into {} by hand, delete it, then run doctor --fix",
                        env.tilde(&env.base_dir.join(&c))
                    ),
                );
            }
        }
        match claude::auth_status(env, &dir) {
            Ok(a) if a.logged_in => {
                let who = a.email.clone().unwrap_or_else(|| "unknown account".into());
                if let Some(e) = a.email {
                    accounts.entry(e).or_default().push(name.clone());
                }
                r.ok(format!("{name}: logged in as {who}"));
            }
            Ok(_) => r.warn(format!("{name}: not logged in"), format!("claude-switcher login {name}")),
            Err(e) => r.warn(format!("{name}: login state unknown ({e})"), "check that claude runs"),
        }
    }
    for (email, names) in accounts.iter().filter(|(_, n)| n.len() > 1) {
        r.warn(
            format!("{} are separate logins to the same account {email}", names.join(" and ")),
            format!("if on purpose, make one an alias: claude-switcher add <name> --same-as {}", names[0]),
        );
    }
    if let Some(d) = &cfg.default
        && !cfg.exists(d)
    {
        r.warn(format!("the default profile {d} does not exist"), "claude-switcher default <profile>");
    }

    // Mappings.
    let stale: Vec<_> = cfg.map.keys().filter(|k| state::reach(k) == state::Reach::Deleted).cloned().collect();
    let away: Vec<_> = cfg.map.keys().filter(|k| state::reach(k) == state::Reach::Unreachable).cloned().collect();
    if !stale.is_empty() {
        if fix {
            env.update(|c| {
                for k in &stale {
                    c.map.remove(k);
                }
                Ok(())
            })?;
            r.ok(format!("dropped {} mappings to deleted folders", stale.len()));
        } else {
            FIXABLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let list: Vec<String> = stale.iter().map(|s| env.tilde(s)).collect();
            r.warn(format!("mapped folders that were deleted: {}", list.join(", ")), "claude-switcher prune");
        }
    }
    for k in &away {
        r.warn(
            format!("{} is not reachable (unmounted volume or moved parent?)", env.tilde(k)),
            format!("if it is gone for good: claude-switcher forget {}", env.tilde(k)),
        );
    }
    for (dir, p) in cfg.map.iter().filter(|(_, p)| !cfg.exists(p)) {
        r.error(
            format!("{} is mapped to the unknown profile {p}", env.tilde(dir)),
            format!("claude-switcher use <profile> {}", env.tilde(dir)),
        );
    }
    if let Some(here) = paths::logical_cwd() {
        for (dir, p) in cfg.ignored_pins(&here) {
            r.warn(
                format!(
                    "{}/{} names {p}, which does not exist here; it is ignored",
                    env.tilde(&dir),
                    state::LOCAL_FILE
                ),
                format!("create {p}, or override the folder: claude-switcher use <profile> {}", env.tilde(&dir)),
            );
        }
    }
    if stale.is_empty() && away.is_empty() && cfg.map.values().all(|p| cfg.exists(p)) {
        r.ok(format!("{} mapped folders", cfg.map.len()));
    }

    // Shell integration.
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("claude-switcher").is_file()));
    if on_path {
        r.ok("claude-switcher is on PATH");
        if let Some(exe) = std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok()) {
            let names: Vec<String> = crate::setup::short_commands(&exe)
                .iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            if !names.is_empty() {
                r.ok(format!("short command: {}", names.join(", ")));
            }
        }
    } else {
        r.warn("claude-switcher is not on PATH", "add its folder to PATH (the installer does this)");
    }
    match Shell::detect() {
        Some(sh) if shell::installed_in(env).iter().any(|(s, _)| *s == sh) => {
            r.ok(format!("shell integration installed for {}", sh.name()));
        }
        Some(sh) => r.warn(format!("no shell integration for {}", sh.name()), "claude-switcher shell install"),
        None => r.warn("unknown login shell", "add `eval \"$(claude-switcher init <shell>)\"` to your rc file"),
    }
    if std::env::var_os("CLAUDECODE").is_some() && std::env::var_os("CLAUDE_SWITCHER_PROFILE").is_none() {
        r.warn(
            "this Claude Code session was not started through claude-switcher",
            "open a new terminal so the shell integration loads, then relaunch claude",
        );
    }

    finish(r, json)
}

fn finish(r: Report, json: bool) -> Result<ExitCode> {
    let worst = r.items.iter().map(|i| i.0).max().unwrap_or(Level::Ok);
    if json {
        let items: Vec<_> = r
            .items
            .iter()
            .map(|(l, m, f)| {
                let level = match l {
                    Level::Ok => "ok",
                    Level::Warn => "warning",
                    Level::Error => "error",
                };
                json!({"level": level, "message": m, "fix": f})
            })
            .collect();
        println!("{:#}", json!(items));
    } else {
        for (l, m, f) in &r.items {
            let mark = match l {
                Level::Ok => "ok  ",
                Level::Warn => "warn",
                Level::Error => "FAIL",
            };
            println!("{mark}  {}", crate::ui::cmd(m));
            if let Some(f) = f {
                println!("      -> {}", crate::ui::cmd(f));
            }
        }
    }
    Ok(if worst == Level::Error { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}
