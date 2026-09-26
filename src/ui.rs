//! Interactive pieces. Everything is drawn on stderr so stdout stays clean for
//! whatever the command prints.

use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, FuzzySelect, Input, Select};
use std::io::IsTerminal;

use crate::claude;
use crate::state::{Config, Env};

/// Interactive only with a terminal on both ends and no opt-out.
pub fn can_prompt(no_input: bool) -> bool {
    !no_input
        && std::env::var_os("CLAUDE_ACCOUNT_NO_INPUT").is_none()
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}

/// Honour NO_COLOR (https://no-color.org) on top of console's own tty checks.
pub fn init_colors() {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
}

/// `error: <msg>` on stderr, the same shape as clap's own errors.
pub fn error(msg: &str) {
    eprintln!("{} {msg}", console::style("error:").red().bold().for_stderr());
}

/// `warning: <msg>` on stderr.
pub fn warning(msg: &str) {
    eprintln!("{} {msg}", console::style("warning:").yellow().bold().for_stderr());
}

/// `info: <msg>` on stderr.
pub fn info(msg: &str) {
    eprintln!("{} {msg}", console::style("info:").cyan().bold().for_stderr());
}

/// `danger: <msg>` on stderr, for what needs attention now.
pub fn danger(msg: &str) {
    eprintln!("{} {msg}", console::style("danger:").red().bold().for_stderr());
}

/// `hint: <msg>` on stderr: the next thing to do.
pub fn hint(msg: &str) {
    eprintln!("{} {msg}", console::style("hint:").cyan().bold().for_stderr());
}

/// A result line on stdout; marked with a check only on a terminal so piped
/// output stays plain.
pub fn done(msg: &str) {
    if std::io::stdout().is_terminal() {
        println!("{} {msg}", console::style("✔").green());
    } else {
        println!("{msg}");
    }
}

/// Section title for multi-step flows: "[2/5] Title".
pub fn step(n: usize, total: usize, title: &str) {
    eprintln!(
        "\n{} {}",
        console::style(format!("[{n}/{total}]")).dim().for_stderr(),
        console::style(title).bold().for_stderr()
    );
}

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

/// One line describing a profile's login (cached, offline).
pub fn describe(env: &Env, cfg: &Config, name: &str) -> String {
    let mut s = match cfg.config_dir(name) {
        Ok(d) => claude::cached_email(env, &d).unwrap_or_else(|| "not logged in".into()),
        Err(_) => "config dir missing".into(),
    };
    if let Some(t) = cfg.alias_target(name) {
        s.push_str(&format!(", same login as {t}"));
    }
    s
}

pub enum Choice {
    Profile(String),
    Extra(usize),
}

/// Fuzzy picker over the profiles (default first), with optional extra entries
/// after them. Typing filters, arrows move, Enter takes the highlighted entry
/// (initially `preselect`). None when the user backs out with Esc.
pub fn pick(env: &Env, cfg: &Config, prompt: &str, preselect: Option<&str>, extras: &[&str]) -> Option<Choice> {
    let names = cfg.ordered();
    let width = names.iter().map(String::len).max().unwrap_or(0);
    let mut items: Vec<String> = names.iter().map(|n| format!("{n:width$}  {}", describe(env, cfg, n))).collect();
    items.extend(extras.iter().map(|e| e.to_string()));
    let default = preselect.and_then(|p| names.iter().position(|n| n == p)).unwrap_or(0);
    let idx =
        FuzzySelect::with_theme(&theme()).with_prompt(prompt).items(&items).default(default).interact_opt().ok()??;
    Some(if idx < names.len() { Choice::Profile(names[idx].clone()) } else { Choice::Extra(idx - names.len()) })
}

pub fn pick_profile(env: &Env, cfg: &Config, prompt: &str, preselect: Option<&str>) -> Option<String> {
    match pick(env, cfg, prompt, preselect, &[])? {
        Choice::Profile(p) => Some(p),
        Choice::Extra(_) => None,
    }
}

pub fn select(prompt: &str, items: &[String], default: usize) -> Option<usize> {
    Select::with_theme(&theme()).with_prompt(prompt).items(items).default(default).interact_opt().ok()?
}

pub fn input(
    prompt: &str,
    initial: Option<&str>,
    allow_empty: bool,
    validate: impl Fn(&String) -> Result<(), String>,
) -> Option<String> {
    let theme = theme();
    let mut i = Input::<String>::with_theme(&theme).with_prompt(prompt).allow_empty(allow_empty);
    if let Some(v) = initial {
        i = i.with_initial_text(v);
    }
    i.validate_with(move |s: &String| if allow_empty && s.is_empty() { Ok(()) } else { validate(s) })
        .interact_text()
        .ok()
}

pub fn confirm(prompt: &str, default: bool) -> Option<bool> {
    Confirm::with_theme(&theme()).with_prompt(prompt).default(default).interact_opt().ok().flatten()
}
