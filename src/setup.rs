//! Guided flows: first-time setup, the bare-command switcher, the shell
//! integration and uninstalling.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::shell::{self, Change, Shell};
use crate::state::{self, Env, Result};
use crate::{Mode, aborted, claude, cwd, mapping, note_running_session, paths, profiles, ui};

fn say(s: &str) {
    eprintln!("{s}");
}

/// `~/x` and relative paths -> absolute, for paths typed at a prompt.
fn expand(env: &Env, input: &str) -> PathBuf {
    let p = match input.strip_prefix("~") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => env.home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(input),
    };
    paths::lexical_absolute(&p, &cwd().unwrap_or_else(|_| env.home.clone()))
}

// ------------------------------------------------------------------ bare command

/// `claude-switcher` in a terminal: switch this project's profile. The current
/// profile is preselected, so Enter keeps it; extra entries lead to the rest.
pub fn interactive(env: &Env) -> Result<ExitCode> {
    let cfg = env.load()?;
    if cfg.profiles.is_empty() {
        say("No profiles yet; let's set them up.\n");
        return setup(env, None, false, false, Mode { prompt: true, yes: false });
    }
    let here = cwd()?;
    let root = state::project_root(&here).unwrap_or_else(|| here.clone());
    let hit = cfg.lookup(&here);
    let current = hit.as_ref().map(|h| h.profile.clone()).or_else(|| cfg.default_name());
    let mut prompt = format!("Claude Code account for {}", env.tilde(&root));
    if let Some(h) = &hit
        && h.key != root
    {
        prompt.push_str(&format!(" (inherits {} from {})", h.profile, env.tilde(&h.key)));
    }
    let extras = ["+ Add an account", "  More actions"];
    match ui::pick(env, &cfg, &prompt, current.as_deref(), &extras) {
        Some(ui::Choice::Profile(name)) => {
            if hit.as_ref().is_some_and(|h| h.profile == name) {
                say(&format!("Unchanged: {name}."));
            } else if state::covers_home(env, &root)
                && ui::confirm(
                    &format!("{} holds all your folders; map every unmapped one to {name}?", env.tilde(&root)),
                    false,
                ) != Some(true)
            {
                say("Nothing changed. Run it inside a project folder to map just that project.");
            } else {
                env.update(|c| {
                    c.map.insert(root.clone(), name.clone());
                    Ok(())
                })?;
                ui::done(&format!("{} -> {name}", env.tilde(&root)));
                say("Applies to new sessions.");
                note_running_session(env);
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(ui::Choice::Extra(0)) => profiles::add(
            env,
            profiles::AddArgs { name: None, same_as: None, base: false, dir: None, login: true, login_args: vec![] },
            true,
        ),
        Some(ui::Choice::Extra(_)) => {
            more(env, &root, hit.is_some_and(|h| h.key == root && h.source == state::Source::Map))
        }
        None => Err(aborted()),
    }
}

fn more(env: &Env, root: &Path, mapped_here: bool) -> Result<ExitCode> {
    let mut items: Vec<(&str, String)> = vec![
        ("login", "Log a profile in".into()),
        ("logout", "Log a profile out".into()),
        ("default", "Set the default profile".into()),
        ("list", "List profiles".into()),
        ("map", "List mapped directories".into()),
        ("doctor", "Check everything (doctor)".into()),
    ];
    if mapped_here {
        items.push(("forget", format!("Forget the mapping of {}", env.tilde(root))));
    }
    let labels: Vec<String> = items.iter().map(|i| i.1.clone()).collect();
    let pick = ui::select("What now?", &labels, 0).ok_or_else(aborted)?;
    match items[pick].0 {
        "login" => profiles::login(env, None, vec![], true),
        "logout" => profiles::logout(env, None, true),
        "default" => {
            let cfg = env.load()?;
            let p =
                ui::pick_profile(env, &cfg, "Default profile", cfg.default_name().as_deref()).ok_or_else(aborted)?;
            profiles::default(env, Some(p), false, false)
        }
        "list" => profiles::list(env, true, false),
        "map" => mapping::map(env, false),
        "doctor" => crate::doctor::doctor(env, false, false, Mode { prompt: true, yes: false }),
        _ => mapping::forget(env, Some(root.to_path_buf()), false),
    }
}

// ------------------------------------------------------------------ setup

/// Symlinks in the binary's own folder that resolve to it.
pub fn short_commands(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        .filter(|p| std::fs::canonicalize(p).ok().as_deref() == Some(exe))
        .collect()
}

/// Numbered "Next steps", the same shape everywhere a flow ends.
pub fn next_steps(steps: &[String]) {
    if steps.is_empty() {
        return;
    }
    eprintln!("\n{}", console::style("Next steps").bold().for_stderr());
    for (i, s) in steps.iter().enumerate() {
        eprintln!("  {}. {}", i + 1, ui::cmd(s));
    }
}

/// First-time setup. Interactive: five short steps, each with its answer
/// preselected. Headless (`--name`): adopt ~/.claude under that name and add the
/// shell integration; everything else is a separate command.
pub fn setup(env: &Env, name: Option<String>, no_shell: bool, shell_ready: bool, mode: Mode) -> Result<ExitCode> {
    let prompt = mode.prompt;
    let Some(version) = claude::version() else {
        return Err("claude not found. Install Claude Code first: https://code.claude.com/docs/en/setup".into());
    };
    if prompt {
        say("claude-switcher runs several Claude Code accounts side by side, one per folder.");
        say(&format!("Found {version}."));
    }
    let mut todo: Vec<String> = vec![];
    // The shell step only counts when setup does it itself.
    let total = if no_shell || shell_ready { 4 } else { 5 };
    profiles::QUIET_USE_HINT.store(true, std::sync::atomic::Ordering::Relaxed);

    // 1. Give the login Claude Code already has a name.
    let cfg = env.load()?;
    let base_owner = env.base_canonical().and_then(|b| cfg.profiles_for_dir(&b).into_iter().next());
    if prompt {
        ui::step(1, total, "Your current Claude Code login");
    }
    match base_owner {
        Some(owner) => {
            if prompt || name.as_deref().is_some_and(|n| n != owner) {
                say(&format!("{} is already the profile {owner}.", env.tilde(&env.base_dir)));
            }
        }
        None => {
            std::fs::create_dir_all(&env.base_dir)
                .map_err(|e| format!("cannot create {}: {e}", env.tilde(&env.base_dir)))?;
            let base = env.base_canonical().ok_or("the base dir vanished")?;
            let who = claude::auth_status(env, &base).ok().filter(|a| a.logged_in).and_then(|a| a.email);
            let name = match name {
                Some(n) => n,
                None if prompt => {
                    match &who {
                        Some(e) => say(&format!("Claude Code is logged in as {e} (in {}).", env.tilde(&env.base_dir))),
                        None => say(&format!("Claude Code has no login yet in {}.", env.tilde(&env.base_dir))),
                    }
                    let taken: Vec<String> = cfg.profiles.keys().cloned().collect();
                    ui::input("Name for this account (e.g. personal, work)", None, false, |n| {
                        crate::valid_name(n)?;
                        if taken.contains(n) { Err(format!("{n} already exists")) } else { Ok(()) }
                    })
                    .ok_or_else(aborted)?
                }
                None => {
                    return Err("usage: claude-switcher setup --name <profile> (names the login in ~/.claude)".into());
                }
            };
            profiles::create(env, Some(name.clone()), None, true, None, false)?;
            ui::done(&format!("{name} = {} ({})", env.tilde(&env.base_dir), who.as_deref().unwrap_or("not logged in")));
            if who.is_none() {
                if prompt && ui::confirm(&format!("Log {name} in now?"), true) == Some(true) {
                    profiles::login(env, Some(name), vec![], true)?;
                } else {
                    todo.push(format!("Log it in: claude-switcher login {name}"));
                }
            }
        }
    }

    if prompt {
        // 2. More accounts.
        ui::step(2, total, "Other Claude accounts");
        loop {
            let n = env.load()?.profiles.len();
            let q = if n <= 1 { "Add another Claude account?" } else { "Add one more?" };
            if ui::confirm(q, n <= 1) != Some(true) {
                break;
            }
            let name = profiles::create(env, None, None, false, None, true)?;
            let cfg = env.load()?;
            ui::done(&format!("Profile {name}: {}", ui::describe(env, &cfg, &name)));
            let dir = cfg.config_dir(&name)?;
            if cfg.alias_target(&name).is_none() && claude::cached_email(env, &dir).is_none() {
                if ui::confirm(&format!("Log {name} in now?"), true) == Some(true) {
                    if let Err(e) = profiles::login(env, Some(name.clone()), vec![], true) {
                        ui::warning(&e);
                        todo.push(format!("Log it in: claude-switcher login {name}"));
                    }
                } else {
                    todo.push(format!("Log it in: claude-switcher login {name}"));
                }
            }
        }

        // 3. Default for unmapped folders.
        ui::step(3, total, "Default profile");
        let cfg = env.load()?;
        if cfg.profiles.len() > 1 {
            say("In a folder with no profile yet, `claude` asks which to use; the default comes first.");
            let d =
                ui::pick_profile(env, &cfg, "Default profile", cfg.default_name().as_deref()).ok_or_else(aborted)?;
            profiles::default(env, Some(d), false, false)?;
        } else {
            say("Only one profile so far: it is the default.");
        }

        // 4. Folders.
        ui::step(4, total, "Folders");
        say("Map folders to profiles; everything inside a folder inherits it. Leave empty to skip.");
        loop {
            let cfg = env.load()?;
            let answer = ui::input("Folder to map (empty to finish)", None, true, |s| {
                let p = expand(env, s);
                if paths::canonical(&p).is_some() { Ok(()) } else { Err(format!("no such folder: {}", p.display())) }
            })
            .ok_or_else(aborted)?;
            if answer.trim().is_empty() {
                break;
            }
            let dir = paths::canonical(&expand(env, answer.trim())).ok_or("that folder vanished")?;
            if state::covers_home(env, &dir)
                && ui::confirm(&format!("{} holds all your folders; map it anyway?", env.tilde(&dir)), false)
                    != Some(true)
            {
                continue;
            }
            let current = cfg.lookup(&dir).map(|h| h.profile);
            let p = ui::pick_profile(env, &cfg, &format!("Profile for {}", env.tilde(&dir)), current.as_deref())
                .ok_or_else(aborted)?;
            env.update(|c| {
                c.map.insert(dir.clone(), p.clone());
                Ok(())
            })?;
            ui::done(&format!("{} -> {p}", env.tilde(&dir)));
        }
        if total == 5 {
            ui::step(5, 5, "Shell integration");
        }
    }

    // 5. Shell integration.
    let mut reopen = shell_ready;
    if no_shell {
        todo.push("Route `claude` through claude-switcher: claude-switcher shell install".into());
    } else if !shell_ready {
        let sh = match Shell::detect() {
            Some(sh) => Some(sh),
            None if prompt => pick_shell()?,
            None => {
                ui::warning("cannot tell your shell from $SHELL; skipped the shell integration");
                todo.push("Add the shell integration: claude-switcher shell install zsh|bash|fish".into());
                None
            }
        };
        if let Some(sh) = sh {
            if shell::installed_in(env).iter().any(|(s, _)| *s == sh) {
                if prompt {
                    say(&format!("Already set up for {}.", sh.name()));
                }
            } else {
                let rc = shell::rc_files(env, sh)[0].clone();
                let q = format!("Route `claude` through claude-switcher? (adds a marked block to {})", env.tilde(&rc));
                if !prompt || ui::confirm(&q, true) == Some(true) {
                    for (file, _) in shell::install(env, sh, None, false)? {
                        ui::done(&format!("Shell integration added to {}", env.tilde(&file)));
                    }
                    reopen = true;
                } else {
                    todo.push(format!("Add it later: claude-switcher shell install {}", sh.name()));
                }
            }
        }
    }

    eprintln!("\n{}", console::style("Your profiles").bold().for_stderr());
    profiles::list(env, false, false)?;
    let mut steps = vec![];
    if reopen {
        steps.push("Open a new terminal, so `claude` goes through claude-switcher".to_string());
    }
    steps.extend(todo);
    steps.push("Run claude in any project: it uses that folder's account, or asks once".into());
    steps.push("Switch a project later with: claude-switcher   (check everything: claude-switcher doctor)".into());
    next_steps(&steps);
    Ok(ExitCode::SUCCESS)
}

fn pick_shell() -> Result<Option<Shell>> {
    let items: Vec<String> = ["zsh", "bash", "fish", "Skip"].iter().map(|s| s.to_string()).collect();
    Ok(match ui::select("Which shell do you use?", &items, 0).ok_or_else(aborted)? {
        0 => Some(Shell::Zsh),
        1 => Some(Shell::Bash),
        2 => Some(Shell::Fish),
        _ => None,
    })
}

// ------------------------------------------------------------------ shell integration

pub fn shell_install(
    env: &Env,
    sh: Option<Shell>,
    path_dir: Option<&Path>,
    dry_run: bool,
    prompt: bool,
) -> Result<ExitCode> {
    let sh = match sh.or_else(Shell::detect) {
        Some(s) => s,
        None if prompt => pick_shell()?.ok_or_else(aborted)?,
        None => return Err("usage: cannot tell your shell from $SHELL; name it: shell install zsh|bash|fish".into()),
    };
    let mut changed = false;
    for (file, change) in shell::install(env, sh, path_dir, dry_run)? {
        let what = match (change, dry_run) {
            (Change::Added, false) => "Shell integration added to",
            (Change::Updated, false) => "Shell integration updated in",
            (Change::Added, true) => "Would add the shell integration to",
            (Change::Updated, true) => "Would update the shell integration in",
            (_, _) => "Shell integration already in",
        };
        changed |= change != Change::Unchanged;
        let line = format!("{what} {}", env.tilde(&file));
        if dry_run { println!("{line}") } else { ui::done(&line) }
    }
    if changed && !dry_run {
        ui::hint("open a new terminal (or source that file) for it to take effect");
    }
    Ok(ExitCode::SUCCESS)
}

pub fn shell_uninstall(env: &Env) -> Result<ExitCode> {
    let done = shell::uninstall(env)?;
    if done.is_empty() {
        println!("No shell integration found.");
    }
    for (file, _) in done {
        println!("Removed the shell integration from {}", env.tilde(&file));
    }
    Ok(ExitCode::SUCCESS)
}

pub fn shell_status(env: &Env, json: bool) -> Result<ExitCode> {
    let found = shell::installed_in(env);
    if json {
        let v: Vec<_> = found.iter().map(|(s, f)| json!({"shell": s.name(), "file": f})).collect();
        println!("{:#}", json!(v));
    } else if found.is_empty() {
        println!("Not installed. Run: claude-switcher shell install");
    } else {
        for (s, f) in found {
            println!("{:5} {}", s.name(), env.tilde(&f));
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------------ uninstall

pub fn uninstall(env: &Env, purge: bool, mode: Mode) -> Result<ExitCode> {
    let (yes, prompt) = (mode.yes, mode.prompt);
    let cfg = env.load().unwrap_or_default();
    let managed: Vec<(String, PathBuf)> = cfg
        .profiles
        .keys()
        .filter_map(|n| {
            cfg.config_dir(n)
                .ok()
                .filter(|d| env.is_managed(d) && cfg.alias_target(n).is_none())
                .map(|d| (n.clone(), d))
        })
        .collect();
    if !yes {
        let mut plan = vec!["remove the shell integration".to_string(), "delete this binary".to_string()];
        if purge {
            plan.push(format!("delete {} and {}", env.tilde(&env.config_file), env.tilde(&env.data_dir)));
            if !managed.is_empty() {
                let names: Vec<&str> = managed.iter().map(|m| m.0.as_str()).collect();
                plan.push(format!("log out and delete the profiles {}", names.join(", ")));
            }
        }
        if !prompt {
            return Err(format!("usage: uninstall would {}; pass --yes to confirm", plan.join(", ")));
        }
        eprintln!("This will:");
        for p in &plan {
            eprintln!("  - {p}");
        }
        if ui::confirm("Continue?", false) != Some(true) {
            return Err(aborted());
        }
    }

    // Log every profile out before touching anything: a dir deleted with a live
    // login would orphan its Keychain entry, so one failure stops the purge.
    if purge {
        for (name, dir) in &managed {
            if profiles::ensure_logged_out(env, name, dir).map_err(|e| format!("uninstall stopped: {e}"))? {
                println!("Logged out {name}");
            }
        }
    }
    for (file, _) in shell::uninstall(env)? {
        println!("Removed the shell integration from {}", env.tilde(&file));
    }
    if purge {
        if env.data_dir.exists() {
            std::fs::remove_dir_all(&env.data_dir)
                .map_err(|e| format!("cannot delete {}: {e}", env.tilde(&env.data_dir)))?;
            println!("Deleted {}", env.tilde(&env.data_dir));
        }
        // Folders it created in the base dir, only while still empty.
        let emptied: Vec<String> =
            cfg.created_in_base.iter().filter(|d| std::fs::remove_dir(env.base_dir.join(d)).is_ok()).cloned().collect();
        if !emptied.is_empty() {
            println!(
                "Removed the empty folders it had created in {}: {}",
                env.tilde(&env.base_dir),
                emptied.join(", ")
            );
        }
        let cache = crate::update::cache_dir(env);
        if cache.exists() {
            std::fs::remove_dir_all(&cache).map_err(|e| format!("cannot delete {}: {e}", env.tilde(&cache)))?;
            println!("Deleted {}", env.tilde(&cache));
        }
        // Only this tool's own files; their dir goes too if nothing else is in it.
        for f in [env.config_file.clone(), env.config_file.with_extension("lock")] {
            if f.exists() {
                std::fs::remove_file(&f).map_err(|e| format!("cannot delete {}: {e}", env.tilde(&f)))?;
                println!("Deleted {}", env.tilde(&f));
            }
        }
        if let Some(d) = env.config_file.parent()
            && std::fs::remove_dir(d).is_ok()
        {
            println!("Deleted {}", env.tilde(d));
        }
    }

    // The binary last: everything above needs it.
    let exe = std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok());
    match exe {
        Some(e) if e.to_string_lossy().contains("/Cellar/") => {
            println!("Installed with Homebrew: run `brew uninstall claude-switcher`.")
        }
        Some(e) if e.starts_with(env.home.join(".cargo")) => {
            println!("Installed with cargo: run `cargo uninstall claude-account-switcher`.")
        }
        Some(e) if e.components().any(|c| c.as_os_str() == "target") && e.to_string_lossy().contains("/target/") => {
            println!("Running from a build directory ({}); left in place.", env.tilde(&e));
        }
        Some(e) => {
            // Short commands (the installer's `csw`, or any other name) are links
            // to this binary in its folder; they go with it, and nothing else does.
            for link in short_commands(&e) {
                if std::fs::remove_file(&link).is_ok() {
                    println!("Deleted {}", env.tilde(&link));
                }
            }
            std::fs::remove_file(&e).map_err(|err| format!("cannot delete {}: {err}", env.tilde(&e)))?;
            println!("Deleted {}", env.tilde(&e));
        }
        None => eprintln!("Could not locate this binary; delete it by hand."),
    }
    if !purge {
        eprintln!(
            "Profiles and logins are kept ({}, {}); `claude-switcher uninstall --purge` removes them.",
            env.tilde(&env.config_file),
            env.tilde(&env.data_dir)
        );
    }
    eprintln!(
        "{} keeps its content. {} files in your projects are left in place.",
        env.tilde(&env.base_dir),
        state::LOCAL_FILE
    );
    next_steps(&["Open a new terminal: `claude` is the plain command again".to_string()]);
    Ok(ExitCode::SUCCESS)
}
