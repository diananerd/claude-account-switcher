//! Profile lifecycle: new, login, logout, rename, remove, list, default.

use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::IsTerminal;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::state::{Config, Env, Profile, Result};
use crate::{aborted, claude, paths, require, ui, valid_name};

/// Shared with the base dir through symlinks: the user-level configuration
/// Claude Code documents, `hooks/`, and what lets a conversation continue under
/// another account. Everything else stays per account: credentials,
/// `.claude.json` (identity, per-project trust), caches, daemon, jobs. Files
/// settings reference by absolute path need no link.
pub const SHARED_DIRS: &[&str] =
    &["skills", "agents", "commands", "output-styles", "hooks", "plugins", "projects", "plans", "file-history"];
pub const SHARED_FILES: &[&str] = &["CLAUDE.md", "settings.json", "keybindings.json", "history.jsonl"];
/// Keys copied once from the base `.claude.json` into a new account so it does
/// not start from scratch: onboarding done, user-scope MCP servers, project trust.
const SEED_KEYS: &[&str] =
    &["hasCompletedOnboarding", "lastOnboardingVersion", "mcpServers", "projects", "autoUpdates", "installMethod"];

// ------------------------------------------------------------------ list / default

pub fn list(env: &Env, check: bool, json: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let def = cfg.default_name();
    let rows: Vec<Value> = cfg
        .ordered()
        .iter()
        .map(|n| {
            let dir = cfg.config_dir(n).ok();
            let alias = cfg.alias_target(n);
            let (logged_in, email) = match (&dir, check) {
                (Some(d), true) if alias.is_none() => match claude::auth_status(env, d) {
                    Ok(a) => (Some(a.logged_in), a.email),
                    Err(_) => (None, claude::cached_email(env, d)),
                },
                (Some(d), _) => (None, claude::cached_email(env, d)),
                (None, _) => (None, None),
            };
            json!({
                "name": n,
                "config_dir": dir,
                "email": email,
                "logged_in": logged_in,
                "same_as": alias,
                "default": Some(n) == def.as_ref(),
                "base": dir.as_deref().is_some_and(|d| env.is_base(d)),
                "managed": dir.as_deref().is_some_and(|d| env.is_managed(d)),
                "error": cfg.config_dir(n).err(),
            })
        })
        .collect();
    if json {
        println!("{:#}", Value::Array(rows));
        return Ok(ExitCode::SUCCESS);
    }
    if rows.is_empty() {
        println!("No profiles yet. Run: claude-switcher setup");
        return Ok(ExitCode::SUCCESS);
    }
    let width = cfg.profiles.keys().map(String::len).max().unwrap_or(0);
    for r in rows {
        let mut tags = vec![];
        if r["default"] == true {
            tags.push("default".to_string());
        }
        if let Some(t) = r["same_as"].as_str() {
            tags.push(format!("same login as {t}"));
        } else if r["base"] == true {
            tags.push(env.tilde(&env.base_dir));
        }
        if let Some(e) = r["error"].as_str() {
            tags.push(e.to_string());
        }
        let login = match (r["logged_in"].as_bool(), r["email"].as_str()) {
            (Some(false), _) => "not logged in".to_string(),
            (_, Some(e)) => e.to_string(),
            (_, None) => "not logged in".to_string(),
        };
        let tags = if tags.is_empty() { String::new() } else { format!("({})", tags.join(", ")) };
        println!("{:width$}  {login:34} {tags}", r["name"].as_str().unwrap_or_default());
    }
    Ok(ExitCode::SUCCESS)
}

pub fn default(env: &Env, profile: Option<String>, json: bool, interactive: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let profile = match profile {
        Some(p) => Some(p),
        // In a terminal, `default` alone opens the picker on the current one:
        // Enter keeps it, so it doubles as "show".
        None if interactive && !cfg.profiles.is_empty() => {
            let cur = cfg.default_name();
            let p = ui::pick_profile(env, &cfg, "Default profile", cur.as_deref()).ok_or_else(aborted)?;
            if Some(&p) == cur.as_ref() && cfg.default.is_some() {
                ui::done(&format!("Default unchanged: {p}"));
                return Ok(ExitCode::SUCCESS);
            }
            Some(p)
        }
        None => None,
    };
    match profile {
        None => {
            let d = cfg.default_name();
            if json {
                println!("{}", json!({ "default": d }));
            } else {
                println!("{}", d.unwrap_or_default());
            }
        }
        Some(p) => {
            env.update(|c| {
                require(c, &p)?;
                c.default = Some(p.clone());
                Ok(())
            })?;
            ui::done(&format!("Default for folders with no profile: {p}"));
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------------ new

pub struct AddArgs {
    pub name: Option<String>,
    pub same_as: Option<String>,
    pub base: bool,
    pub dir: Option<PathBuf>,
    pub login: bool,
    /// Passed to `claude auth login`, e.g. --sso.
    pub login_args: Vec<OsString>,
}

pub fn add(env: &Env, args: AddArgs, prompt: bool) -> Result<ExitCode> {
    // Login options (--sso, --email) only make sense for an account of its own.
    let ask_kind = prompt && args.login_args.is_empty();
    let name = create_as(env, args.name, args.same_as, args.base, args.dir, prompt, ask_kind)?;
    let cfg = env.load()?;
    ui::done(&format!("Profile {name}: {}", ui::describe(env, &cfg, &name)));
    let dir = cfg.config_dir(&name)?;
    let owns_login = cfg.alias_target(&name).is_none();
    if owns_login && claude::cached_email(env, &dir).is_none() && args.login {
        if prompt && ui::confirm(&format!("Log {name} in now?"), true) == Some(true) {
            return login(env, Some(name), args.login_args, true);
        }
        if args.login_args.is_empty() {
            ui::hint(&format!("log it in: claude-switcher login {name}   (add --sso for SSO accounts)"));
        } else {
            let extra: Vec<String> = args.login_args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
            ui::hint(&format!("log it in: claude-switcher login {name} {}", extra.join(" ")));
        }
        return Ok(ExitCode::SUCCESS);
    }
    hint_use(&cfg, &name);
    Ok(ExitCode::SUCCESS)
}

/// Set by flows (setup) that map folders themselves, where the hint is noise.
pub static QUIET_USE_HINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// After a profile becomes usable: how to start using it, unless it already is.
fn hint_use(cfg: &Config, name: &str) {
    if !QUIET_USE_HINT.load(std::sync::atomic::Ordering::Relaxed) && !cfg.map.values().any(|p| p == name) {
        ui::hint(&format!("use it: run claude in a project and pick {name}, or: claude-switcher use {name} <folder>"));
    }
}

/// Create a profile and return its name. Shared by `new` and `setup`.
pub fn create(
    env: &Env,
    name: Option<String>,
    same_as: Option<String>,
    base: bool,
    dir: Option<PathBuf>,
    prompt: bool,
) -> Result<String> {
    create_as(env, name, same_as, base, dir, prompt, prompt)
}

/// `create`, where `ask_kind` false skips asking what the profile is (its own login by default).
fn create_as(
    env: &Env,
    name: Option<String>,
    same_as: Option<String>,
    base: bool,
    dir: Option<PathBuf>,
    prompt: bool,
    ask_kind: bool,
) -> Result<String> {
    let cfg = env.load()?;
    let check = |n: &String| -> std::result::Result<(), String> {
        valid_name(n)?;
        if cfg.exists(n) { Err(format!("{n} already exists")) } else { Ok(()) }
    };
    let name = match name {
        Some(n) => n,
        None if prompt => ui::input("Profile name (e.g. work, personal)", None, false, check).ok_or_else(aborted)?,
        None => return Err("usage: claude-switcher add <name> [--same-as <profile> | --base | --dir <path>]".into()),
    };
    check(&name).map_err(|e| format!("invalid profile name {name:?}: {e}"))?;

    let (mut same_as, mut base) = (same_as, base);
    if ask_kind && same_as.is_none() && !base && dir.is_none() {
        let base_free = env.base_canonical().is_some_and(|b| cfg.profiles_for_dir(&b).is_empty());
        let mut kinds = vec![("own", "A Claude account of its own (you log in to it next)".to_string())];
        if base_free {
            let who = env
                .base_canonical()
                .and_then(|b| claude::cached_email(env, &b))
                .map(|e| format!(" ({e})"))
                .unwrap_or_default();
            kinds.push(("base", format!("The login Claude Code already has in {}{who}", env.tilde(&env.base_dir))));
        }
        if !cfg.profiles.is_empty() {
            kinds.push(("alias", "Another name for an existing profile's login".to_string()));
        }
        let labels: Vec<String> = kinds.iter().map(|k| k.1.clone()).collect();
        let default = if base_free && cfg.profiles.is_empty() { 1 } else { 0 };
        match kinds[ui::select(&format!("What is {name}?"), &labels, default).ok_or_else(aborted)?].0 {
            "base" => base = true,
            "alias" => {
                same_as = Some(ui::pick_profile(env, &cfg, "Same login as", None).ok_or_else(aborted)?);
            }
            _ => {}
        }
    }

    let profile = if let Some(other) = &same_as {
        require(&cfg, other)?;
        Profile { config_dir: None, same_as: Some(other.clone()) }
    } else if base || dir.is_some() {
        let d = dir.clone().unwrap_or_else(|| env.base_dir.clone());
        let d = paths::canonical(&d).ok_or_else(|| format!("no such folder: {}", d.display()))?;
        if let Some(owner) = cfg.profiles_for_dir(&d).first() {
            return Err(format!("{} already belongs to {owner}; use --same-as {owner}", env.tilde(&d)));
        }
        Profile { config_dir: Some(d), same_as: None }
    } else {
        let wanted = env.managed_dir(&name);
        if let Some(owner) = paths::canonical(&wanted).and_then(|d| cfg.profiles_for_dir(&d).into_iter().next()) {
            return Err(format!(
                "{} is the config dir of {owner} (renamed from {name}?); pick another name",
                env.tilde(&wanted)
            ));
        }
        Profile { config_dir: Some(create_account_dir(env, &name)?), same_as: None }
    };
    // Roll back only a dir made by this very call, never an adopted one.
    let created = (same_as.is_none() && !base && dir.is_none()).then(|| profile.config_dir.clone()).flatten();
    let result = env.update(|c| {
        if c.exists(&name) {
            return Err(format!("{name} already exists"));
        }
        // The first profile is the default until someone picks another.
        if c.profiles.is_empty() && c.default.is_none() {
            c.default = Some(name.clone());
        }
        c.profiles.insert(name.clone(), profile);
        Ok(())
    });
    if let (Err(_), Some(d)) = (&result, created) {
        let _ = std::fs::remove_dir_all(d);
    }
    result.map(|_| name)
}

/// A fresh config dir that shares the base dir's customisation through symlinks
/// and starts from a trimmed copy of its `.claude.json`.
fn create_account_dir(env: &Env, name: &str) -> Result<PathBuf> {
    let dir = env.managed_dir(name);
    if dir.exists() {
        return Err(format!(
            "{} already exists (left by a removed profile?). Reuse it with: claude-switcher add {name} --dir {}",
            env.tilde(&dir),
            env.tilde(&dir)
        ));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("cannot create {}: {e}", env.tilde(&dir)))?;
    let result = (|| {
        link_shared(env, &dir)?;
        let seed: serde_json::Map<String, Value> = std::fs::read_to_string(env.home.join(".claude.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Map<String, Value>>(&t).ok())
            .map(|m| m.into_iter().filter(|(k, _)| SEED_KEYS.contains(&k.as_str())).collect())
            .unwrap_or_default();
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(dir.join(".claude.json"))
            .map_err(|e| format!("cannot create .claude.json: {e}"))?;
        std::io::Write::write_all(&mut f, Value::Object(seed).to_string().as_bytes()).map_err(|e| e.to_string())?;
        paths::canonical(&dir).ok_or_else(|| "the new folder vanished".to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Link the shared entries of the base dir into an account dir. Returns what it
/// could not fix (an entry that exists but is not a link).
pub fn link_shared(env: &Env, acct: &Path) -> Result<Vec<PathBuf>> {
    let mut created = vec![];
    for d in SHARED_DIRS {
        let p = env.base_dir.join(d);
        if !p.exists() {
            std::fs::create_dir_all(&p).map_err(|e| format!("cannot create {d}: {e}"))?;
            created.push(d.to_string());
        }
    }
    if !created.is_empty() {
        env.update(|c| {
            for d in created {
                if !c.created_in_base.contains(&d) {
                    c.created_in_base.push(d);
                }
            }
            Ok(())
        })?;
    }
    let mut conflicts = vec![];
    for e in SHARED_DIRS.iter().chain(SHARED_FILES) {
        let src = env.base_dir.join(e);
        let dst = acct.join(e);
        if !src.exists() {
            continue;
        }
        match std::fs::symlink_metadata(&dst) {
            Ok(m) if m.file_type().is_symlink() => {
                if std::fs::read_link(&dst).ok().as_deref() != Some(src.as_path()) && !dst.exists() {
                    std::fs::remove_file(&dst).map_err(|err| format!("cannot relink {e}: {err}"))?;
                    symlink(&src, &dst).map_err(|err| format!("cannot link {e}: {err}"))?;
                }
            }
            Ok(_) => conflicts.push(dst),
            Err(_) => symlink(&src, &dst).map_err(|err| format!("cannot link {e}: {err}"))?,
        }
    }
    Ok(conflicts)
}

// ------------------------------------------------------------------ login / logout

fn ask_profile(env: &Env, cfg: &Config, profile: Option<String>, prompt: bool, question: &str) -> Result<String> {
    match profile {
        Some(p) => {
            require(cfg, &p)?;
            Ok(p)
        }
        None if cfg.profiles.is_empty() => Err(crate::no_profiles()),
        None if prompt => ui::pick_profile(env, cfg, question, None).ok_or_else(aborted),
        None => Err("usage: claude-switcher logout <profile> (see: claude-switcher list)".into()),
    }
}

/// The profile `login` acts on. An unknown name, or "+ Add an account" in the picker,
/// creates the profile first (asking what kind), so a new account is one command.
fn login_target(env: &Env, cfg: &Config, profile: Option<String>, prompt: bool) -> Result<(String, bool)> {
    let create_new = |name: Option<String>| profiles_create(env, name, prompt).map(|n| (n, true));
    match profile {
        Some(p) if cfg.exists(&p) => Ok((p, false)),
        Some(p) => {
            valid_name(&p).map_err(|e| format!("invalid profile name {p:?}: {e}"))?;
            if !prompt {
                return Err(format!("no such profile: {p}. Add it with: claude-switcher add {p}"));
            }
            match ui::confirm(&format!("There is no account named {p}. Add it now?"), true) {
                Some(true) => create_new(Some(p)),
                _ => Err(aborted()),
            }
        }
        None if cfg.profiles.is_empty() && prompt => create_new(None),
        None if cfg.profiles.is_empty() => Err(crate::no_profiles()),
        None if prompt => match ui::pick(env, cfg, "Log in which profile?", None, &["+ Add an account"]) {
            Some(ui::Choice::Profile(p)) => Ok((p, false)),
            Some(ui::Choice::Extra(_)) => create_new(None),
            None => Err(aborted()),
        },
        None => Err("usage: claude-switcher login <profile> (see: claude-switcher list)".into()),
    }
}

fn profiles_create(env: &Env, name: Option<String>, prompt: bool) -> Result<String> {
    let name = create(env, name, None, false, None, prompt)?;
    let cfg = env.load()?;
    eprintln!("Profile {name}: {}", ui::describe(env, &cfg, &name));
    Ok(name)
}

pub fn login(env: &Env, profile: Option<String>, args: Vec<OsString>, prompt: bool) -> Result<ExitCode> {
    let (name, created) = login_target(env, &env.load()?, profile, prompt)?;
    let cfg = env.load()?;
    let owner = cfg.owner(&name)?.to_string();
    if owner != name {
        eprintln!("{name} uses the login of {owner}; logging in {owner}.");
    }
    let dir = cfg.config_dir(&owner)?;
    if prompt
        && let Ok(a) = claude::auth_status(env, &dir)
        && a.logged_in
    {
        let who = a.email.unwrap_or_else(|| "an account".into());
        let again = ui::confirm(&format!("{owner} is already logged in as {who}. Log in again?"), false);
        if again != Some(true) {
            return Ok(ExitCode::SUCCESS);
        }
    }
    eprintln!(
        "Logging in {owner}. Claude Code opens your browser to sign in.\n\
         The browser signs in with whatever claude.ai account it is already using:\n\
         if that is not the one for {owner}, switch accounts there (or use a private window)."
    );
    if !std::io::stdin().is_terminal() {
        eprintln!(
            "No terminal on stdin: if the browser cannot hand the login back and shows a code,\n\
             there is nowhere to paste it. If that happens, run this in a terminal instead."
        );
    }
    let retry = if created {
        format!("; {owner} was created but is not logged in. Retry: claude-switcher login {owner}")
    } else {
        String::new()
    };
    if !claude::login(env, &dir, &args)? {
        return Err(format!("login for {owner} did not complete{retry}"));
    }
    let auth = claude::auth_status(env, &dir)?;
    if !auth.logged_in {
        return Err(format!("{owner} is still not logged in{retry}"));
    }
    let email = auth.email.clone().unwrap_or_else(|| "unknown account".into());
    ui::done(&format!("{owner} is logged in as {email}{}", auth.org.map(|o| format!(" ({o})")).unwrap_or_default()));
    // Two profiles on one account is legal but usually a slip in the browser.
    let dup = cfg.profiles.keys().filter(|n| **n != owner && cfg.alias_target(n).is_none()).find(|n| {
        auth.email.is_some() && cfg.config_dir(n).ok().and_then(|d| claude::cached_email(env, &d)) == auth.email
    });
    if let Some(other) = dup {
        eprintln!(
            "Note: {other} is logged in to the same account. If you meant another account, run\n\
             `claude-switcher login {owner}` again after switching accounts in the browser.\n\
             If it is on purpose, `claude-switcher add <name> --same-as {other}` shares one login instead."
        );
    }
    hint_use(&cfg, &name);
    Ok(ExitCode::SUCCESS)
}

pub fn logout(env: &Env, profile: Option<String>, prompt: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let name = ask_profile(env, &cfg, profile, prompt, "Log out which profile?")?;
    let owner = cfg.owner(&name)?.to_string();
    let sharing: Vec<String> =
        cfg.profiles.keys().filter(|n| cfg.owner(n).ok() == Some(owner.as_str()) && **n != name).cloned().collect();
    if !sharing.is_empty() {
        eprintln!("This also logs out {}, which share the login.", sharing.join(", "));
    }
    let dir = cfg.config_dir(&owner)?;
    if !claude::logout(env, &dir)? {
        return Err(format!("logout for {owner} failed"));
    }
    ui::done(&format!("{owner} is logged out"));
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------------ rename / remove

pub fn rename(env: &Env, old: Option<String>, new: Option<String>, prompt: bool) -> Result<ExitCode> {
    let cfg = env.load()?;
    let old = match old {
        Some(o) => o,
        None if prompt && !cfg.profiles.is_empty() => {
            ui::pick_profile(env, &cfg, "Rename which profile?", None).ok_or_else(aborted)?
        }
        None => return Err("usage: claude-switcher rename <old> <new>".into()),
    };
    require(&cfg, &old)?;
    let new = match new {
        Some(n) => n,
        None if prompt => ui::input(&format!("New name for {old}"), None, false, |n| {
            valid_name(n)?;
            if cfg.exists(n) { Err(format!("{n} already exists")) } else { Ok(()) }
        })
        .ok_or_else(aborted)?,
        None => return Err("usage: claude-switcher rename <old> <new>".into()),
    };
    let (old, new) = (old.as_str(), new.as_str());
    valid_name(new).map_err(|e| format!("invalid profile name {new:?}: {e}"))?;
    env.update(|c| {
        require(c, old)?;
        if c.exists(new) {
            return Err(format!("{new} already exists"));
        }
        let p = c.profiles.remove(old).expect("checked");
        c.profiles.insert(new.to_string(), p);
        for p in c.profiles.values_mut() {
            if p.same_as.as_deref() == Some(old) {
                p.same_as = Some(new.to_string());
            }
        }
        for v in c.map.values_mut() {
            if v == old {
                *v = new.to_string();
            }
        }
        if c.default.as_deref() == Some(old) {
            c.default = Some(new.to_string());
        }
        Ok(())
    })?;
    // The config dir keeps its path: the login is keyed to it.
    ui::done(&format!("Renamed {old} to {new}"));
    eprintln!("{} files that name {old} are not rewritten; update them by hand.", crate::state::LOCAL_FILE);
    Ok(ExitCode::SUCCESS)
}

pub fn remove(env: &Env, profile: Option<String>, force: bool, purge: bool, mode: crate::Mode) -> Result<ExitCode> {
    let (yes, prompt) = (mode.yes, mode.prompt);
    let cfg = env.load()?;
    let profile = match profile {
        Some(p) => p,
        None if prompt && !cfg.profiles.is_empty() => {
            ui::pick_profile(env, &cfg, "Remove which profile?", None).ok_or_else(aborted)?
        }
        None => return Err("usage: claude-switcher remove <profile> [--force] [--purge]".into()),
    };
    let profile = profile.as_str();
    require(&cfg, profile)?;
    let aliases = cfg.aliases_of(profile);
    if !aliases.is_empty() {
        return Err(format!("{} share {profile}'s login; remove or re-point them first", aliases.join(", ")));
    }
    let mapped = cfg.map.values().filter(|p| *p == profile).count();
    if mapped > 0 && !force {
        if !prompt && !yes {
            return Err(format!(
                "usage: {profile} is mapped to {mapped} folders; pass --force to drop those mappings too"
            ));
        }
        let q = format!("{profile} is mapped to {mapped} folders. Drop those mappings too?");
        if !yes && ui::confirm(&q, false) != Some(true) {
            return Err(aborted());
        }
    }
    let own_dir = cfg.profiles[profile].config_dir.as_ref().and_then(|d| paths::canonical(d));
    let deletable = own_dir.clone().filter(|d| env.is_managed(d));
    // Interactive: offer the purge instead of expecting the flag to be known.
    let purge = purge
        || (prompt
            && !yes
            && deletable.is_some()
            && ui::confirm(&format!("Also log {profile} out and delete its folder?"), false).ok_or_else(aborted)?);
    if prompt && !yes && !purge && ui::confirm(&format!("Remove the profile {profile}?"), false) != Some(true) {
        return Err(aborted());
    }
    if purge && deletable.is_none() {
        eprintln!(
            "Note: --purge only logs out and deletes config dirs this tool created; {}'s dir and login are kept.",
            profile
        );
    }
    if purge
        && !yes
        && let Some(d) = &deletable
    {
        let what = format!("log {profile} out and delete {}", env.tilde(d));
        if !prompt {
            return Err(format!("usage: --purge would {what}; pass --yes to confirm"));
        }
        if ui::confirm(&format!("This will {what}. Continue?"), false) != Some(true) {
            return Err(aborted());
        }
    }
    // Deleting the dir of a live login would orphan its Keychain entry: log out
    // first, and keep everything if that does not work.
    if purge
        && let Some(d) = &deletable
        && ensure_logged_out(env, profile, d)?
    {
        ui::done(&format!("Logged out {profile}"));
    }
    env.update(|c| {
        c.map.retain(|_, p| p != profile);
        if c.default.as_deref() == Some(profile) {
            c.default = None;
        }
        c.profiles.remove(profile);
        Ok(())
    })?;
    ui::done(&format!("Removed {profile}"));
    match (&deletable, purge) {
        (Some(d), true) => {
            std::fs::remove_dir_all(d).map_err(|e| format!("cannot delete {}: {e}", env.tilde(d)))?;
            ui::done(&format!("Deleted {}", env.tilde(d)));
        }
        (Some(d), false) => {
            eprintln!("Its config dir and login are kept in {} (use --purge to delete them).", env.tilde(d))
        }
        (None, _) => {}
    }
    Ok(ExitCode::SUCCESS)
}

/// Log a config dir out, or explain why nothing was deleted.
pub fn ensure_logged_out(env: &Env, name: &str, dir: &Path) -> Result<bool> {
    let keep = format!("nothing was deleted; log {name} out (claude-switcher logout {name}) and retry");
    let status = claude::auth_status(env, dir).map_err(|e| format!("cannot check {name}'s login ({e}); {keep}"))?;
    if status.logged_in {
        let ok = claude::logout(env, dir).unwrap_or(false);
        if !ok || claude::auth_status(env, dir).map(|a| a.logged_in).unwrap_or(true) {
            return Err(format!("could not log {name} out; {keep}"));
        }
        return Ok(true);
    }
    Ok(false)
}

pub struct LinkState {
    /// Shared entries the account dir does not link yet.
    pub missing: Vec<String>,
    /// Entries that exist in the account dir as real files, shadowing the base.
    pub conflicts: Vec<String>,
}

/// What `link_shared` would change, without changing anything.
pub fn link_shared_dry(env: &Env, acct: &Path) -> LinkState {
    let mut s = LinkState { missing: vec![], conflicts: vec![] };
    for e in SHARED_DIRS.iter().chain(SHARED_FILES) {
        let src = env.base_dir.join(e);
        if !src.exists() && !SHARED_DIRS.contains(e) {
            continue;
        }
        match std::fs::symlink_metadata(acct.join(e)) {
            Ok(m) if m.file_type().is_symlink() && acct.join(e).exists() => {}
            Ok(m) if !m.file_type().is_symlink() => s.conflicts.push(e.to_string()),
            _ => s.missing.push(e.to_string()),
        }
    }
    s
}
