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
//! through files named after the supervisor's pid, in `<data dir>/run`:
//! `<pid>.session` and `<pid>.transcript` (the session id, and where its
//! transcript was when the reply ended, written by the Stop hook) and
//! `<pid>.switch` (the profile to move to, written by `use`), plus SIGUSR1 to
//! say "look now".
//!
//! Claude runs its Stop hooks in parallel, so the nudge comes while the others
//! may still run. The supervisor ends claude only once the transcript says the
//! turn is over (a `turn_duration` entry, written after every Stop hook, and not
//! when one keeps Claude working).

use libc::{c_int, pid_t};
use std::ffi::OsString;
use std::io::{IsTerminal, Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::claude;
use crate::state::{Config, Env, Result};

/// Set in claude's environment: the pid of the supervisor that runs it.
pub const SUPERVISOR_VAR: &str = "CLAUDE_SWITCHER_SUPERVISOR";
const OPT_OUT_VAR: &str = "CLAUDE_SWITCHER_NO_SUPERVISE";

/// How long claude gets to exit after SIGTERM before SIGKILL.
const GRACE: Duration = Duration::from_secs(5);

/// How long a pending move waits on a transcript that stopped growing without
/// saying the turn is over (a hung hook, a Claude Code that does not write it).
const QUIET_CAP: Duration = Duration::from_secs(90);

/// How often a pending move looks at the transcript.
const TICK: Duration = Duration::from_millis(200);

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
    transcript: PathBuf,
    switch: PathBuf,
    plugin: PathBuf,
}

impl Run {
    pub fn of(env: &Env, pid: u32) -> Run {
        let dir = env.data_dir.join("run");
        Run {
            session: dir.join(format!("{pid}.session")),
            transcript: dir.join(format!("{pid}.transcript")),
            switch: dir.join(format!("{pid}.switch")),
            plugin: dir.join(format!("{pid}.plugin")),
        }
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
    /// Remember the transcript and its current length: the turn's end is
    /// written after this point, once every Stop hook is done.
    pub fn record_transcript(&self, path: &str) -> Result<()> {
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Run::write(&self.transcript, &format!("{len}\t{path}"))
    }
    fn transcript(&self) -> Option<(u64, PathBuf)> {
        let text = Run::read(&self.transcript)?;
        let (len, path) = text.split_once('\t')?;
        Some((len.parse().ok()?, PathBuf::from(path)))
    }
    /// Remember which claude-switcher plugin the session loaded (its SessionStart
    /// hook runs in every version, the Stop hook only from 0.2).
    /// With two copies loaded, one that has the Stop hook wins.
    pub fn record_plugin(&self, root: &Path) -> Result<()> {
        if !has_stop_hook(root) && self.plugin().is_some_and(|p| has_stop_hook(&p)) {
            return Ok(());
        }
        Run::write(&self.plugin, &root.to_string_lossy())
    }
    fn plugin(&self) -> Option<PathBuf> {
        Run::read(&self.plugin).map(PathBuf::from)
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
        let _ = std::fs::remove_file(&self.transcript);
        let _ = std::fs::remove_file(&self.plugin);
        let _ = std::fs::remove_file(&self.switch);
    }
}

/// The live supervisor of the session this process runs in, if any.
pub fn supervisor() -> Option<u32> {
    let pid: u32 = std::env::var(SUPERVISOR_VAR).ok()?.parse().ok()?;
    (pid > 1 && unsafe { libc::kill(pid as pid_t, 0) } == 0).then_some(pid)
}

/// Why a session cannot move from `from` to `to` live, if it cannot.
pub fn obstacle(env: &Env, cfg: &Config, from: &str, to: &str) -> Option<String> {
    let Some(pid) = supervisor() else {
        return Some("this session was not started by claude-switcher 0.2 or later".into());
    };
    // The plugin's Stop hook is what tells the supervisor the reply is over.
    match Run::of(env, pid).plugin() {
        None => return Some("the claude-switcher plugin is not loaded in this session".into()),
        Some(root) if !has_stop_hook(&root) => {
            return Some(
                "the claude-switcher plugin in this session is older than 0.2; update it in Claude Code with \
                 /plugin marketplace update claude-account-switcher, then \
                 /plugin update claude-switcher@claude-account-switcher"
                    .into(),
            );
        }
        Some(_) => {}
    }
    // `--resume` finds the conversation only where the new profile keeps its own.
    let projects = |p: &str| cfg.config_dir(p).ok().and_then(|d| crate::paths::canonical(&d.join("projects")));
    match (projects(from), projects(to)) {
        (Some(a), Some(b)) if a == b => None,
        _ => Some(format!("{from} and {to} keep their conversations in different folders")),
    }
}

fn has_stop_hook(plugin_root: &Path) -> bool {
    std::fs::read_to_string(plugin_root.join("hooks/hooks.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.pointer("/hooks/Stop").is_some())
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
    let mut drain: Option<Drain> = None;
    let mut kill_at: Option<Instant> = None;
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
            ticker(false);
            return (status, kill_at.is_some());
        }
        if r == -1 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            ticker(false);
            return (status, kill_at.is_some());
        }
        let bits = CAUGHT.swap(0, Ordering::SeqCst);
        for sig in [libc::SIGTERM, libc::SIGHUP] {
            if caught(bits, sig) {
                unsafe {
                    libc::kill(pid, sig);
                }
            }
        }
        if caught(bits, libc::SIGUSR1)
            && drain.is_none()
            && kill_at.is_none()
            && files.switch_target().is_some()
            && files.session().is_some()
        {
            drain = Some(Drain::start(files.transcript()));
            ticker(true);
        }
        if let Some(d) = &mut drain {
            if files.switch_target().is_none() {
                // Switched back while the other hooks ran: stay.
                drain = None;
                ticker(false);
            } else if d.turn_over() {
                drain = None;
                kill_at = Some(Instant::now() + GRACE);
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
        if kill_at.is_some_and(|t| Instant::now() >= t) {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// A move waiting for Claude's turn to be over.
struct Drain {
    /// The transcript and how far it was when the reply ended.
    transcript: Option<(u64, PathBuf)>,
    len: u64,
    changed: Instant,
}

impl Drain {
    fn start(transcript: Option<(u64, PathBuf)>) -> Drain {
        let len = transcript.as_ref().map_or(0, |t| t.0);
        Drain { transcript, len, changed: Instant::now() }
    }

    /// Whether claude can be ended now: the transcript says the turn is over,
    /// or has been quiet for too long, or there is no transcript to watch.
    fn turn_over(&mut self) -> bool {
        let Some((from, path)) = &self.transcript else { return true };
        let Ok(tail) = read_from(path, *from) else { return true };
        let ended = tail.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).any(|v| {
            v.get("type").and_then(|t| t.as_str()) == Some("system")
                && v.get("subtype").and_then(|t| t.as_str()) == Some("turn_duration")
        });
        if ended {
            return true;
        }
        let len = from + tail.len() as u64;
        if len != self.len {
            self.len = len;
            self.changed = Instant::now();
        }
        self.changed.elapsed() >= QUIET_CAP
    }
}

fn read_from(path: &Path, from: u64) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// SIGALRM every TICK while a move is pending, so waitpid wakes to look.
fn ticker(on: bool) {
    let every = if on {
        libc::timeval { tv_sec: 0, tv_usec: TICK.as_micros() as libc::suseconds_t }
    } else {
        libc::timeval { tv_sec: 0, tv_usec: 0 }
    };
    let t = libc::itimerval { it_interval: every, it_value: every };
    unsafe {
        libc::setitimer(libc::ITIMER_REAL, &t, std::ptr::null_mut());
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
