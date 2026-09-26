//! claude-account: several Claude Code accounts on one machine, chosen per
//! folder. `state.rs` holds the model and the resolution rules.
//!
//! Every command works two ways. Headless: everything comes from arguments and
//! flags; nothing is ever asked (no terminal, `--no-input`), a missing answer is
//! a usage error (exit 2) naming the flag to pass, and `-y/--yes` accepts
//! confirmations. Interactive (a terminal on stdin and stderr): only what is
//! missing is asked, with the likely answer preselected; destructive
//! confirmations default to no, constructive ones to yes.

mod claude;
mod doctor;
mod launch;
mod mapping;
mod paths;
mod profiles;
mod setup;
mod shell;
mod state;
mod ui;
mod update;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use state::{Config, Env, Result};

#[derive(Parser)]
#[command(
    name = "claude-account",
    version,
    about = "Run several Claude Code accounts on one machine, chosen per folder",
    long_about = "Run several Claude Code accounts on one machine, chosen per folder.\n\n\
        Each profile is a Claude Code config dir with its own login. With the shell\n\
        integration, `claude` picks the profile mapped to the folder you run it in;\n\
        in a folder with no profile yet it asks once and remembers the answer.\n\n\
        In a terminal, commands ask for whatever is missing. Without one (or with\n\
        --no-input) they never ask: pass everything as arguments.",
    after_help = "Examples:\n  \
        claude-account setup               guided first-time setup\n  \
        claude-account                     switch this project's profile\n  \
        claude-account use work ~/work     everything under ~/work uses the profile work\n  \
        claude-account login work --sso    log a profile in (creates it if it is new)\n  \
        claude-account status --json       what applies here, for scripts\n\n\
        Docs: https://github.com/diananerd/claude-account-switcher"
)]
struct Cli {
    /// Never ask anything; fail with a usage error instead (also CLAUDE_ACCOUNT_NO_INPUT=1)
    #[arg(long, global = true)]
    no_input: bool,
    /// Answer yes to confirmations
    #[arg(long, short, global = true)]
    yes: bool,
    /// Machine-readable JSON output where supported
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// First-time setup: name your accounts, log in, map folders, add the shell integration
    Setup {
        /// Name for the login Claude Code already has in ~/.claude (required without a terminal)
        #[arg(long)]
        name: Option<String>,
        /// Do not add the shell integration
        #[arg(long)]
        no_shell: bool,
    },

    /// Map this project (or DIR) to a profile
    Use {
        /// Profile name (asked in a terminal when omitted)
        profile: Option<String>,
        /// Folder (default: the project you are in, i.e. the repository root or this folder)
        dir: Option<PathBuf>,
        /// Write a .claude-account file in the folder instead of the machine map
        #[arg(long)]
        local: bool,
    },
    /// Remove this project's (or DIR's) own mapping
    Forget {
        /// Folder (default: the project you are in, i.e. the repository root or this folder)
        dir: Option<PathBuf>,
        /// Remove the .claude-account file instead of the machine mapping
        #[arg(long)]
        local: bool,
    },
    /// Which profile applies here, and why
    Status {
        /// Folder (default: the current folder)
        dir: Option<PathBuf>,
    },
    /// Print only the profile that applies here (empty when none does)
    Resolve {
        /// Folder (default: the current folder)
        dir: Option<PathBuf>,
    },
    /// List mapped folders
    Map,
    /// Drop mappings to deleted folders
    Prune {
        /// Also drop unreachable ones (unmounted volume, moved parent)
        #[arg(long)]
        all: bool,
    },

    /// List profiles and their logins
    List {
        /// Ask claude for the live login state (slower)
        #[arg(long)]
        check: bool,
    },
    /// Show or set the profile offered first in a folder with no profile
    Default {
        /// Profile to make the default (without it: show it, or pick in a terminal)
        profile: Option<String>,
    },
    /// Create a profile: its own login, an alias (--same-as) or an existing dir
    New {
        /// Name for the profile: lowercase letters, digits and dashes (asked in a terminal)
        name: Option<String>,
        /// Share the login of an existing profile
        #[arg(long, value_name = "PROFILE", conflicts_with_all = ["base", "dir"])]
        same_as: Option<String>,
        /// Adopt ~/.claude and whatever login it has
        #[arg(long, conflicts_with = "dir")]
        base: bool,
        /// Adopt an existing Claude Code config dir
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Do not offer to log in afterwards
        #[arg(long)]
        no_login: bool,
    },
    /// Log a profile in; an unknown name creates it (extra args go to `claude auth login`, e.g. --sso)
    Login {
        /// Profile to log in; a new name creates it (picked in a terminal when omitted)
        profile: Option<String>,
        /// Passed to `claude auth login`, e.g. --sso or --email you@company.com
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Log a profile out
    Logout {
        /// Profile to log out (picked in a terminal when omitted)
        profile: Option<String>,
    },
    /// Rename a profile (its login and mappings follow)
    Rename {
        /// Current name (picked in a terminal when omitted)
        old: Option<String>,
        /// New name (asked in a terminal when omitted)
        new: Option<String>,
    },
    /// Unregister a profile; its folder and login are kept unless --purge
    Remove {
        /// Profile to remove (picked in a terminal when omitted)
        profile: Option<String>,
        /// Also drop the folders mapped to it
        #[arg(long)]
        force: bool,
        /// Also log it out and delete its config dir (only dirs this tool created)
        #[arg(long)]
        purge: bool,
    },
    /// Launch claude with a profile, this time only
    Run {
        /// Profile to run as (picked in a terminal when omitted)
        profile: Option<String>,
        /// Everything after the profile goes to claude untouched
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },

    /// Check the installation, profiles, logins and mappings
    Doctor {
        /// Repair what can be repaired safely, without asking
        #[arg(long)]
        fix: bool,
    },
    /// Print the shell function (eval it in your rc file)
    Init { shell: shell::Shell },
    /// Manage the shell integration in your rc files
    #[command(subcommand)]
    Shell(ShellCmd),
    /// Print shell completions
    Completions { shell: clap_complete::Shell },
    /// Status line segment (reads the status line JSON on stdin)
    Statusline,
    /// Claude Code hook entry points
    Hook { event: HookEvent },
    /// Update to the latest stable release (runs the official installer)
    Update {
        /// A specific release instead, e.g. v0.1.0 (pre-releases too)
        #[arg(long, value_name = "TAG")]
        version: Option<String>,
    },
    /// Remove the shell integration and this binary (--purge: profiles and config too)
    Uninstall {
        /// Also log out and delete every profile this tool created, and its config
        #[arg(long)]
        purge: bool,
    },

    /// Refresh the cached "latest version" (spawned in the background)
    #[command(hide = true)]
    RefreshUpdateCache,
    /// Resolve the profile, then exec claude (what the shell function calls)
    #[command(hide = true)]
    Launch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// `claude-account <profile> [dir]` is short for `claude-account use`
    #[command(external_subcommand)]
    Other(Vec<String>),
}

#[derive(Subcommand)]
enum ShellCmd {
    /// Add the integration to your shell's rc file (idempotent)
    Install {
        /// Defaults to your login shell ($SHELL)
        shell: Option<shell::Shell>,
        /// Also put this folder on PATH (used by the installer)
        #[arg(long)]
        path_dir: Option<PathBuf>,
        /// Only say which file would change
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the integration from every rc file
    Uninstall,
    /// Show where the integration is installed
    Status,
}

#[derive(Clone, Copy, ValueEnum)]
enum HookEvent {
    SessionStart,
}

fn main() -> ExitCode {
    ui::init_colors();
    // `launch` forwards every argument to claude verbatim, --help and --version
    // included, so it bypasses the parser entirely; so does everything after
    // `run <profile>`.
    let argv: Vec<OsString> = std::env::args_os().collect();
    let is = |i: usize, s: &str| argv.get(i).is_some_and(|a| a == s);
    let result = if is(1, "launch") {
        Env::from_process().and_then(|env| launch::launch(&env, argv[2..].to_vec(), None))
    } else if is(1, "run") && argv.get(2).and_then(|a| a.to_str()).is_some_and(|p| !p.starts_with('-')) {
        let profile = argv[2].to_string_lossy().into_owned();
        Env::from_process().and_then(|env| launch::launch(&env, argv[3..].to_vec(), Some(profile)))
    } else {
        let cli = Cli::parse();
        // The update notice closes commands a person reads; never output meant
        // for machines or for claude itself.
        let notify = !cli.json
            && !matches!(
                cli.cmd,
                Some(
                    Cmd::Statusline
                        | Cmd::Hook { .. }
                        | Cmd::Init { .. }
                        | Cmd::Completions { .. }
                        | Cmd::Launch { .. }
                        | Cmd::Run { .. }
                        | Cmd::Update { .. }
                        | Cmd::RefreshUpdateCache
                        | Cmd::Resolve { .. }
                )
            );
        Env::from_process().and_then(|env| {
            let r = dispatch(&env, cli);
            if notify && r.is_ok() {
                update::notice(&env);
            }
            r
        })
    };
    match result {
        Ok(code) => code,
        Err(e) if e.is_empty() => ExitCode::from(130),
        // Same exit status as clap's own usage errors.
        Err(e) if e.starts_with("usage:") => {
            ui::error(&e);
            ExitCode::from(2)
        }
        Err(e) => {
            ui::error(&e);
            ExitCode::FAILURE
        }
    }
}

/// How a command may interact with the user.
#[derive(Clone, Copy)]
pub struct Mode {
    /// A terminal is there and asking is allowed.
    pub prompt: bool,
    /// Confirmations are pre-answered yes.
    pub yes: bool,
}

impl Mode {
    /// Ask a yes/no question, or answer it without asking: `--yes` says yes;
    /// headless, the default applies only when it is the safe one (`false`),
    /// otherwise the flag that makes it explicit is required.
    pub fn confirm(self, question: &str, default: bool, flag_hint: &str) -> Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if self.prompt {
            return ui::confirm(question, default).ok_or_else(aborted);
        }
        if default {
            return Err(format!("usage: {question} Pass {flag_hint} to confirm without a terminal"));
        }
        Ok(false)
    }
}

fn dispatch(env: &Env, cli: Cli) -> Result<ExitCode> {
    let prompt = ui::can_prompt(cli.no_input);
    let mode = Mode { prompt, yes: cli.yes };
    let json = cli.json;
    match cli.cmd {
        None if prompt => setup::interactive(env),
        None => mapping::status(env, None, json),
        Some(Cmd::Setup { name, no_shell }) => setup::setup(env, name, no_shell, mode),
        Some(Cmd::Use { profile, dir, local }) => mapping::use_profile(env, profile, dir, local, prompt),
        Some(Cmd::Forget { dir, local }) => mapping::forget(env, dir, local),
        Some(Cmd::Status { dir }) => mapping::status(env, dir, json),
        Some(Cmd::Resolve { dir }) => mapping::resolve(env, dir, json),
        Some(Cmd::Map) => mapping::map(env, json),
        Some(Cmd::Prune { all }) => mapping::prune(env, all, mode),
        Some(Cmd::List { check }) => profiles::list(env, check, json),
        Some(Cmd::Default { profile }) => profiles::default(env, profile, json, prompt && !json),
        Some(Cmd::New { name, same_as, base, dir, no_login }) => {
            profiles::new(env, profiles::NewArgs { name, same_as, base, dir, login: !no_login }, prompt)
        }
        Some(Cmd::Login { profile, args }) => profiles::login(env, profile, args, prompt),
        Some(Cmd::Logout { profile }) => profiles::logout(env, profile, prompt),
        Some(Cmd::Rename { old, new }) => profiles::rename(env, old, new, prompt),
        Some(Cmd::Remove { profile, force, purge }) => profiles::remove(env, profile, force, purge, mode),
        Some(Cmd::Run { profile, args }) => {
            let profile = match profile {
                Some(p) => p,
                None if prompt => {
                    let cfg = env.load()?;
                    if cfg.profiles.is_empty() {
                        return Err(no_profiles());
                    }
                    ui::pick_profile(env, &cfg, "Run claude once as", None).ok_or_else(aborted)?
                }
                None => return Err("usage: claude-account run <profile> [claude args]".into()),
            };
            launch::launch(env, args, Some(profile))
        }
        Some(Cmd::Doctor { fix }) => doctor::doctor(env, fix, json, mode),
        Some(Cmd::Init { shell }) => {
            println!("{}", shell::function(shell));
            Ok(ExitCode::SUCCESS)
        }
        Some(Cmd::Shell(ShellCmd::Install { shell, path_dir, dry_run })) => {
            setup::shell_install(env, shell, path_dir.as_deref(), dry_run, prompt)
        }
        Some(Cmd::Shell(ShellCmd::Uninstall)) => setup::shell_uninstall(env),
        Some(Cmd::Shell(ShellCmd::Status)) => setup::shell_status(env, json),
        Some(Cmd::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "claude-account", &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        Some(Cmd::Statusline) => launch::statusline(env),
        Some(Cmd::Hook { event: HookEvent::SessionStart }) => launch::hook_session_start(env),
        Some(Cmd::Uninstall { purge }) => setup::uninstall(env, purge, mode),
        Some(Cmd::Update { version }) => update::update(env, version, mode),
        Some(Cmd::RefreshUpdateCache) => update::refresh(env),
        Some(Cmd::Launch { args }) => launch::launch(env, args, None),
        Some(Cmd::Other(args)) => {
            let cfg = env.load()?;
            match args.as_slice() {
                [name, rest @ ..] if cfg.exists(name) && rest.len() <= 1 => {
                    mapping::use_profile(env, Some(name.clone()), rest.first().map(PathBuf::from), false, prompt)
                }
                [name, ..] => Err(format!("unknown command or profile: {name} (see: claude-account --help)")),
                [] => unreachable!("clap never yields an empty external subcommand"),
            }
        }
    }
}

// ------------------------------------------------------------------ helpers

/// Empty error: the user backed out of a prompt (exit 130, nothing printed).
pub fn aborted() -> String {
    String::new()
}

pub fn cwd() -> Result<PathBuf> {
    paths::logical_cwd().ok_or_else(|| "cannot read the current folder".into())
}

/// DIR argument (logical) or the current directory.
pub fn dir_or_cwd(dir: Option<PathBuf>) -> Result<PathBuf> {
    match dir {
        Some(d) => paths::logical_arg(&d).ok_or_else(|| format!("no such folder: {}", d.display())),
        None => cwd(),
    }
}

/// Canonical directory a command acts on: DIR itself when given, else the
/// project the current directory belongs to.
pub fn target_dir(dir: Option<PathBuf>) -> Result<PathBuf> {
    match dir {
        Some(d) => paths::logical_arg(&d)
            .and_then(|l| paths::canonical(&l))
            .ok_or_else(|| format!("no such folder: {}", d.display())),
        None => state::project_root(&cwd()?).ok_or_else(|| "cannot read the current folder".into()),
    }
}

pub fn require(cfg: &Config, name: &str) -> Result<()> {
    if cfg.exists(name) { Ok(()) } else { Err(format!("no such profile: {name} (see: claude-account list)")) }
}

pub fn valid_name(name: &str) -> std::result::Result<(), String> {
    let chars_ok = !name.is_empty()
        && name.len() <= 40
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-');
    let reserved = name == "help" || Cli::command().get_subcommands().any(|s| s.get_name() == name);
    if !chars_ok {
        Err("use up to 40 lowercase letters, digits and dashes".into())
    } else if reserved {
        Err(format!("{name} is a command name"))
    } else {
        Ok(())
    }
}

pub fn no_profiles() -> String {
    "no profiles yet; run: claude-account setup".into()
}

/// Profile of the running Claude Code session: CLAUDE_ACCOUNT when `launch` set
/// it, otherwise inferred from CLAUDE_CONFIG_DIR, preferring the profile the
/// directory resolves to when both use the same config dir.
pub fn session_profile(env: &Env, cfg: &Config, dir: &Path) -> Option<String> {
    if let Some(a) = std::env::var("CLAUDE_ACCOUNT").ok().filter(|a| cfg.exists(a)) {
        return Some(a);
    }
    let cfg_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| env.base_dir.clone());
    let cfg_dir = paths::canonical(&cfg_dir)?;
    if let Some(hit) = cfg.lookup(dir)
        && cfg.config_dir(&hit.profile).ok().as_deref() == Some(cfg_dir.as_path())
    {
        return Some(hit.profile);
    }
    cfg.profiles_for_dir(&cfg_dir).into_iter().next()
}

/// Run from inside a Claude Code session: when the change just made means this
/// session's own directory now resolves to another profile than the session
/// runs as, say how to move the conversation over. Changes elsewhere say nothing.
pub fn note_running_session(env: &Env) {
    if std::env::var_os("CLAUDECODE").is_none() {
        return;
    }
    let (Ok(cfg), Some(here)) = (env.load(), paths::logical_cwd()) else { return };
    let Some(now) = cfg.lookup(&here).map(|h| h.profile) else { return };
    if let Some(cur) = session_profile(env, &cfg, &here)
        && cur != now
    {
        ui::hint(&format!("this session still runs as {cur}; exit and run `claude --continue` to resume it as {now}"));
    }
}
