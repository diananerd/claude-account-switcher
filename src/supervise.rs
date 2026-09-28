//! Live switching. An interactive claude runs as a child of `launch` instead of
//! replacing it, so `claude-switcher use` from inside the session can move the
//! conversation to another profile without the user restarting anything: the
//! Stop hook tells this supervisor when Claude's reply is over, the supervisor
//! ends claude and resumes the same session under the new profile.
//!
//! Everything that is not an interactive session in a terminal still execs
//! claude directly, and so does every launch with CLAUDE_SWITCHER_NO_SUPERVISE=1.
//!
//! The supervisor and the processes inside the session (hooks, `use`) talk
//! through two files named after the supervisor's pid, in `<data dir>/run`:
//! `<pid>.session` (the session id, written by the Stop hook) and `<pid>.switch`
//! (the profile to move to, written by `use`), plus SIGUSR1 to say "look now".

use libc::{c_int, pid_t};
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::claude;
use crate::state::{Config, Env, Result};

/// Set in claude's environment: the pid of the supervisor that runs it.
pub const SUPERVISOR_VAR: &str = "CLAUDE_SWITCHER_SUPERVISOR";
const OPT_OUT_VAR: &str = "CLAUDE_SWITCHER_NO_SUPERVISE";

/// How long claude gets to exit after SIGTERM before SIGKILL.
const GRACE_SECS: u32 = 5;

/// Whether this launch should be supervised: an interactive session in a
/// terminal. Print mode, background sessions, --help/--version and claude's own
/// subcommands exec claude as before.
pub fn wanted(args: &[OsString]) -> bool {
    if std::env::var_os(OPT_OUT_VAR).is_some_and(|v| !v.is_empty() && v != "0") {
        return false;
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return false;
    }
    let one_shot = args.iter().any(|a| {
        matches!(a.to_str(), Some("-p" | "--print" | "-h" | "--help" | "-v" | "--version" | "--bg" | "--background"))
    });
    if one_shot {
        return false;
    }
    // Only a leading bare word can be a subcommand; `claude --help` is read only then.
    match args.first().and_then(|a| a.to_str()) {
        Some(first) if !first.starts_with('-') => !claude::subcommands().iter().any(|c| c == first),
        _ => true,
    }
}

/// Bit per signal caught since the last look (signal numbers are all below 64).
static CAUGHT: AtomicU64 = AtomicU64::new(0);

extern "C" fn on_signal(sig: c_int) {
    CAUGHT.fetch_or(1 << sig, Ordering::SeqCst);
}

fn caught(bits: u64, sig: c_int) -> bool {
    bits & (1 << sig) != 0
}

/// Catch without SA_RESTART, so a signal interrupts waitpid; ignore what the
/// terminal sends the whole foreground group (claude gets those itself).
fn install_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as extern "C" fn(c_int) as libc::sighandler_t;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        for sig in [libc::SIGUSR1, libc::SIGTERM, libc::SIGHUP, libc::SIGALRM] {
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
        for sig in [libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP] {
            libc::signal(sig, libc::SIG_IGN);
        }
    }
}

/// The run files of one supervisor.
pub struct Run {
    session: PathBuf,
    switch: PathBuf,
}

impl Run {
    pub fn of(env: &Env, pid: u32) -> Run {
        let dir = env.data_dir.join("run");
        Run { session: dir.join(format!("{pid}.session")), switch: dir.join(format!("{pid}.switch")) }
    }

    fn write(path: &Path, text: &str) -> Result<()> {
        let dir = path.parent().expect("run files live in a directory");
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)
            .and_then(|_| std::fs::rename(&tmp, path))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    fn read(path: &Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub fn session(&self) -> Option<String> {
        Run::read(&self.session)
    }
    pub fn record_session(&self, id: &str) -> Result<()> {
        Run::write(&self.session, id)
    }
    pub fn switch_target(&self) -> Option<String> {
        Run::read(&self.switch)
    }
    pub fn request_switch(&self, profile: &str) -> Result<()> {
        Run::write(&self.switch, profile)
    }
    pub fn cancel_switch(&self) {
        let _ = std::fs::remove_file(&self.switch);
    }
    fn clear(&self) {
        let _ = std::fs::remove_file(&self.session);
        let _ = std::fs::remove_file(&self.switch);
    }
}

/// The live supervisor of the session this process runs in, if any.
pub fn supervisor() -> Option<u32> {
    let pid: u32 = std::env::var(SUPERVISOR_VAR).ok()?.parse().ok()?;
    (pid > 1 && unsafe { libc::kill(pid as pid_t, 0) } == 0).then_some(pid)
}

/// Why a session cannot move from `from` to `to` live, if it cannot.
pub fn obstacle(cfg: &Config, from: &str, to: &str) -> Option<String> {
    if supervisor().is_none() {
        return Some("this session was not started by claude-switcher 0.2 or later".into());
    }
    // `--resume` finds the conversation only where the new profile keeps its own.
    let projects = |p: &str| cfg.config_dir(p).ok().and_then(|d| crate::paths::canonical(&d.join("projects")));
    match (projects(from), projects(to)) {
        (Some(a), Some(b)) if a == b => None,
        _ => Some(format!("{from} and {to} keep their conversations in different folders")),
    }
}

/// Tell the supervisor to look at its run files now.
pub fn nudge(pid: u32) {
    unsafe {
        libc::kill(pid as pid_t, libc::SIGUSR1);
    }
}

struct Terminal(Option<libc::termios>);

impl Terminal {
    fn save() -> Terminal {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        Terminal((unsafe { libc::tcgetattr(0, &mut t) } == 0).then_some(t))
    }

    /// Put the terminal back the way it was before claude, which may have been
    /// killed with raw mode, a hidden cursor or extended key reporting still on.
    fn restore(&self) {
        if let Some(t) = &self.0 {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, t);
            }
        }
        if std::io::stdout().is_terminal() {
            // Bracketed paste, focus events and mouse reporting off; kitty
            // keyboard flags popped; cursor shown; a fresh line.
            let mut out = std::io::stdout();
            let _ = out.write_all(b"\x1b[?2004l\x1b[?1004l\x1b[?1000l\x1b[?1006l\x1b[<u\x1b[?25h\r\n");
            let _ = out.flush();
        }
    }
}

/// Run claude as a child under `name`, and again under another profile each time
/// the session asks to move. Returns claude's own exit status.
pub fn run(env: &Env, mut name: String, mut args: Vec<OsString>) -> Result<ExitCode> {
    let me = std::process::id();
    let files = Run::of(env, me);
    files.clear();
    install_handlers();
    let terminal = Terminal::save();
    loop {
        let cfg = env.load()?;
        crate::require(&cfg, &name)?;
        let dir = cfg.config_dir(&name).map_err(|e| format!("{e}\nRun: claude-switcher doctor"))?;
        let mut cmd = claude::command_for(env, &dir);
        cmd.args(&args).env("CLAUDE_SWITCHER_PROFILE", &name).env(SUPERVISOR_VAR, me.to_string());
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                for sig in [libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP] {
                    libc::signal(sig, libc::SIG_DFL);
                }
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(claude::spawn_error)?;
        let (status, moving) = wait(child.id() as pid_t, &files);
        let next = if moving { files.switch_target().zip(files.session()) } else { None };
        match next {
            Some((target, session)) => {
                files.cancel_switch();
                terminal.restore();
                eprintln!("claude-switcher: resuming this session as {target}");
                let arities = claude::option_arities();
                args = if arities.is_empty() {
                    // Without claude's option list an option's value cannot be told
                    // from the prompt: carry nothing over rather than guess.
                    eprintln!("claude-switcher: could not read claude's options; resuming without the original ones");
                    vec!["--resume".into(), session.into()]
                } else {
                    claude::resume_args(&args, &session, &arities)
                };
                name = target;
            }
            None => {
                files.clear();
                return Ok(exit_code(status));
            }
        }
    }
}

/// Wait for claude to end, passing on stops and termination requests. Returns
/// its wait status and whether it ended because this supervisor moved it.
fn wait(pid: pid_t, files: &Run) -> (c_int, bool) {
    let mut moving = false;
    loop {
        let mut status: c_int = 0;
        let r = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
        if r == pid {
            if libc::WIFSTOPPED(status) {
                // Suspended (Ctrl-Z): suspend with it so the shell gets the
                // terminal back, then wake it when the shell wakes us.
                unsafe {
                    libc::raise(libc::SIGSTOP);
                    libc::kill(pid, libc::SIGCONT);
                }
                continue;
            }
            return (status, moving);
        }
        if r == -1 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return (status, moving);
        }
        let bits = CAUGHT.swap(0, Ordering::SeqCst);
        for sig in [libc::SIGTERM, libc::SIGHUP] {
            if caught(bits, sig) {
                unsafe {
                    libc::kill(pid, sig);
                }
            }
        }
        if caught(bits, libc::SIGUSR1) && !moving && files.switch_target().is_some() && files.session().is_some() {
            moving = true;
            unsafe {
                libc::kill(pid, libc::SIGTERM);
                libc::alarm(GRACE_SECS);
            }
        }
        if caught(bits, libc::SIGALRM) && moving {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn exit_code(status: c_int) -> ExitCode {
    if libc::WIFEXITED(status) {
        ExitCode::from(libc::WEXITSTATUS(status) as u8)
    } else if libc::WIFSIGNALED(status) {
        ExitCode::from((128 + libc::WTERMSIG(status)) as u8)
    } else {
        ExitCode::FAILURE
    }
}
