//! Launching claude under the right profile, and the pieces Claude Code itself
//! calls back into (status line, hooks).

use serde_json::Value;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::state::{self, Config, Env, Result};
use crate::{aborted, claude, cwd, paths, profiles, require, session_profile, ui};

/// Resolve the profile for the current directory, then exec claude with it.
/// `once` names a profile for this launch only (`run`).
pub fn launch(env: &Env, args: Vec<OsString>, once: Option<String>) -> Result<ExitCode> {
    // A broken config must never lock the user out of claude: say so, then run
    // claude exactly as if this tool were not installed.
    let cfg = match env.load() {
        Ok(c) => c,
        Err(e) if once.is_none() => {
            eprintln!("claude-switcher: {e}\nclaude-switcher: launching claude unchanged; see: claude-switcher doctor");
            let mut cmd = claude::command();
            cmd.args(&args);
            return Err(claude::exec(cmd));
        }
        Err(e) => return Err(e),
    };
    // A CLAUDE_CONFIG_DIR set by hand, outside this tool, is honoured untouched.
    if once.is_none()
        && std::env::var_os("CLAUDE_SWITCHER_PROFILE").is_none()
        && let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR")
        && paths::canonical(Path::new(&dir)).is_none_or(|c| cfg.profiles_for_dir(&c).is_empty())
    {
        let mut cmd = claude::command();
        cmd.args(&args);
        return Err(claude::exec(cmd));
    }

    let name = match once {
        Some(n) => n,
        None => {
            let here = paths::logical_cwd();
            if let Some(h) = here.as_deref() {
                for (dir, p) in cfg.ignored_pins(h) {
                    eprintln!(
                        "claude-switcher: ignoring {}/{}: no profile named {p} here",
                        env.tilde(&dir),
                        state::LOCAL_FILE
                    );
                }
            }
            match here.as_deref().and_then(|d| cfg.lookup(d)) {
                Some(hit) => hit.profile,
                None => {
                    // Freshly installed, nothing set up yet: never stand between
                    // the user and claude. Run it untouched and point at setup.
                    let Some(def) = cfg.default_name() else {
                        eprintln!(
                            "claude-switcher: no profiles yet, so claude runs as usual. Set them up: claude-switcher setup"
                        );
                        let mut cmd = claude::command();
                        cmd.args(&args);
                        return Err(claude::exec(cmd));
                    };
                    if wants_picker(&args) { pick_and_remember(env, &cfg, &def)? } else { def }
                }
            }
        }
    };
    // The picker may have just created the profile: read the config again.
    let cfg = env.load()?;
    require(&cfg, &name)?;
    let dir = cfg.config_dir(&name).map_err(|e| format!("{e}\nRun: claude-switcher doctor"))?;
    let mut cmd = claude::command_for(env, &dir);
    cmd.args(&args).env("CLAUDE_SWITCHER_PROFILE", &name);
    Err(claude::exec(cmd))
}

/// The picker only makes sense when starting an interactive session in a
/// terminal: not for `-p`, `--help` and the like, and not for claude's own
/// subcommands (`claude update`, `claude mcp list`), read from `claude --help`
/// so new ones are covered without a release of this tool.
fn wants_picker(args: &[OsString]) -> bool {
    let non_interactive =
        args.iter().any(|a| matches!(a.to_str(), Some("-p" | "--print" | "-h" | "--help" | "-v" | "--version")));
    if non_interactive || !ui::can_prompt(false) || !std::io::stdout().is_terminal() {
        return false;
    }
    match args.first().and_then(|a| a.to_str()) {
        Some(first) if !first.starts_with('-') => !claude::subcommands().iter().any(|c| c == first),
        _ => true,
    }
}

fn pick_and_remember(env: &Env, cfg: &Config, def: &str) -> Result<String> {
    let here = cwd()?;
    let root = state::project_root(&here).unwrap_or(here);
    let prompt = format!("Claude Code account for {}", env.tilde(&root));
    let name = match ui::pick(env, cfg, &prompt, Some(def), &["+ Add an account"]).ok_or_else(aborted)? {
        ui::Choice::Profile(p) => p,
        ui::Choice::Extra(_) => new_profile_here(env)?,
    };
    if state::covers_home(env, &root) {
        eprintln!("(Not remembered: {} would cover every project.)", env.tilde(&root));
    } else {
        env.update(|c| {
            c.map.insert(root.clone(), name.clone());
            Ok(())
        })?;
        eprintln!("Remembered: {} -> {name}. Change it with: claude-switcher", env.tilde(&root));
    }
    Ok(name)
}

/// "+ Add an account" in the first-launch picker: create it and, when it has a login
/// of its own, log it in before claude starts with it.
fn new_profile_here(env: &Env) -> Result<String> {
    let name = profiles::create(env, None, None, false, None, true)?;
    let cfg = env.load()?;
    eprintln!("Profile {name}: {}", ui::describe(env, &cfg, &name));
    let dir = cfg.config_dir(&name)?;
    if cfg.alias_target(&name).is_none()
        && claude::cached_email(env, &dir).is_none()
        && ui::confirm(&format!("Log {name} in now?"), true) == Some(true)
    {
        profiles::login(env, Some(name.clone()), vec![], true)?;
    }
    Ok(name)
}

/// Directory named by the hook / status line JSON on stdin, else the cwd.
fn stdin_dir(pointers: &[&str]) -> Option<PathBuf> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let v: Value = serde_json::from_reader(std::io::stdin()).ok()?;
    pointers.iter().find_map(|p| v.pointer(p).and_then(Value::as_str)).map(PathBuf::from)
}

/// The session's profile, plus the directory's when they differ.
pub fn statusline(env: &Env) -> Result<ExitCode> {
    let cfg = env.load()?;
    let dir = stdin_dir(&["/workspace/current_dir", "/cwd"]).or_else(paths::logical_cwd).unwrap_or_default();
    let cur = session_profile(env, &cfg, &dir);
    let mapped = cfg.lookup(&dir).map(|h| h.profile);
    match (cur, mapped) {
        (Some(c), Some(m)) if c != m => println!("{c} (here: {m})"),
        (Some(c), _) => println!("{c}"),
        (None, m) => println!("{}", m.unwrap_or_default()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Tells Claude (not the user) when the session runs under a different profile
/// than the one its directory resolves to, so it can mention it once.
pub fn hook_session_start(env: &Env) -> Result<ExitCode> {
    let Ok(cfg) = env.load() else { return Ok(ExitCode::SUCCESS) };
    let Some(dir) = stdin_dir(&["/cwd"]).or_else(paths::logical_cwd) else {
        return Ok(ExitCode::SUCCESS);
    };
    if let (Some(cur), Some(hit)) = (session_profile(env, &cfg, &dir), cfg.lookup(&dir))
        && cur != hit.profile
    {
        println!(
            "claude-switcher: this session runs as profile \"{cur}\", but {} resolves to \"{}\". \
             Mention it to the user once: to switch, they exit and run `claude --continue` \
             from a shell with the claude-switcher integration.",
            dir.display(),
            hit.profile
        );
    }
    Ok(ExitCode::SUCCESS)
}
