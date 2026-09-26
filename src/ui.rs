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
        && std::env::var_os("CLAUDE_SWITCHER_NO_INPUT").is_none()
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

/// The name this binary was run as: `csw` through the installer's short
/// command, `claude-switcher` otherwise.
fn invoked_name() -> String {
    std::env::args_os()
        .next()
        .and_then(|a| std::path::Path::new(&a).file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "claude-switcher".into())
}

/// Commands suggested in messages use the name the user typed: "claude-switcher
/// add" becomes "csw add" when run as csw. Only a command followed by a
/// subcommand or flag is rewritten; paths such as ~/.config/claude-switcher/
/// are left alone.
pub fn cmd(msg: &str) -> String {
    rename_commands(msg, &invoked_name())
}

fn rename_commands(msg: &str, name: &str) -> String {
    if name == "claude-switcher" {
        return msg.to_string();
    }
    let needle = "claude-switcher ";
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(i) = rest.find(needle) {
        let before = rest[..i].chars().last();
        let after = rest[i + needle.len()..].chars().next();
        let standalone = before.is_none_or(|c| c.is_whitespace() || "`(\"'".contains(c));
        let is_command = after.is_some_and(|c| c.is_ascii_lowercase() || c == '-');
        out.push_str(&rest[..i]);
        if standalone && is_command {
            out.push_str(name);
            out.push(' ');
        } else {
            out.push_str(needle);
        }
        rest = &rest[i + needle.len()..];
    }
    out.push_str(rest);
    out
}

/// `error: <msg>` on stderr, the same shape as clap's own errors.
pub fn error(msg: &str) {
    eprintln!("{} {}", console::style("error:").red().bold().for_stderr(), cmd(msg));
}

/// `warning: <msg>` on stderr.
pub fn warning(msg: &str) {
    eprintln!("{} {}", console::style("warning:").yellow().bold().for_stderr(), cmd(msg));
}

/// `info: <msg>` on stderr.
pub fn info(msg: &str) {
    eprintln!("{} {}", console::style("info:").cyan().bold().for_stderr(), cmd(msg));
}

/// `danger: <msg>` on stderr, for what needs attention now.
pub fn danger(msg: &str) {
    eprintln!("{} {}", console::style("danger:").red().bold().for_stderr(), cmd(msg));
}

/// `hint: <msg>` on stderr: the next thing to do.
pub fn hint(msg: &str) {
    eprintln!("{} {}", console::style("hint:").cyan().bold().for_stderr(), cmd(msg));
}

/// A result line on stdout; marked with a check only on a terminal so piped
/// output stays plain.
pub fn done(msg: &str) {
    if std::io::stdout().is_terminal() {
        println!("{} {}", console::style("✔").green(), cmd(msg));
    } else {
        println!("{}", cmd(msg));
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

#[cfg(test)]
mod tests {
    use super::rename_commands;

    #[test]
    fn suggested_commands_use_the_invoked_name() {
        assert_eq!(rename_commands("run: claude-switcher add work", "csw"), "run: csw add work");
        assert_eq!(
            rename_commands("`claude-switcher login x` or claude-switcher --help", "csw"),
            "`csw login x` or csw --help"
        );
        assert_eq!(
            rename_commands("~/.config/claude-switcher/config.toml", "csw"),
            "~/.config/claude-switcher/config.toml"
        );
        assert_eq!(
            rename_commands("claude-switcher 0.1.0 is the latest", "csw"),
            "claude-switcher 0.1.0 is the latest"
        );
        assert_eq!(
            rename_commands("cargo uninstall claude-account-switcher", "csw"),
            "cargo uninstall claude-account-switcher"
        );
        assert_eq!(
            rename_commands("run: claude-switcher add work", "claude-switcher"),
            "run: claude-switcher add work"
        );
    }
}
