//! Shell integration: the `claude` function that routes every launch through
//! `claude-account launch`, and the marked block that loads it from rc files.
//!
//! Rc files are edited only between the markers below, in place when a block is
//! already there, so installing twice changes nothing and uninstalling leaves
//! every other line exactly as it was.

use clap::ValueEnum;
use std::fs;
use std::path::{Path, PathBuf};

use crate::state::{Env, Result};

pub const BEGIN: &str = "# >>> claude-account >>>";
pub const END: &str = "# <<< claude-account <<<";

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    pub const ALL: [Shell; 3] = [Shell::Zsh, Shell::Bash, Shell::Fish];

    pub fn name(self) -> &'static str {
        match self {
            Shell::Zsh => "zsh",
            Shell::Bash => "bash",
            Shell::Fish => "fish",
        }
    }

    /// The user's login shell, from $SHELL.
    pub fn detect() -> Option<Shell> {
        let sh = std::env::var("SHELL").ok()?;
        match Path::new(&sh).file_name()?.to_str()? {
            "zsh" => Some(Shell::Zsh),
            "bash" => Some(Shell::Bash),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }
}

/// What `claude-account init <shell>` prints.
///
/// An existing `alias claude=<path>` (older Claude Code installs add one) would
/// shadow the function, or in zsh break its definition; the alias is removed and
/// its target becomes the real claude that `launch` runs. The `function` keyword
/// is used because a name after it is never alias-expanded.
pub fn function(shell: Shell) -> &'static str {
    match shell {
        Shell::Zsh => {
            "if (( ${+aliases[claude]} )); then\n\
             \x20 [[ ${aliases[claude]} == *[[:space:]]* ]] || eval \"export CLAUDE_ACCOUNT_CLAUDE=${aliases[claude]}\"\n\
             \x20 unalias claude\n\
             fi\n\
             function claude { command claude-account launch \"$@\"; }"
        }
        Shell::Bash => {
            "if _ca_alias=$(alias claude 2>/dev/null); then\n\
             \x20 _ca_alias=${_ca_alias#alias claude=}; _ca_alias=${_ca_alias#\\'}; _ca_alias=${_ca_alias%\\'}\n\
             \x20 case $_ca_alias in *[[:space:]]*) ;; *) eval \"export CLAUDE_ACCOUNT_CLAUDE=$_ca_alias\" ;; esac\n\
             \x20 unalias claude\n\
             fi\n\
             unset _ca_alias\n\
             function claude { command claude-account launch \"$@\"; }"
        }
        Shell::Fish => "function claude --wraps claude\n    command claude-account launch $argv\nend",
    }
}

/// POSIX single-quoted literal.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Fish single-quoted literal.
fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Files that load the integration for a shell. Fish gets a file of its own in
/// conf.d; zsh and bash get a marked block in their rc file (bash on macOS also
/// in ~/.bash_profile, which login shells read instead of ~/.bashrc).
pub fn rc_files(env: &Env, shell: Shell) -> Vec<PathBuf> {
    match shell {
        Shell::Zsh => {
            let zdot = std::env::var_os("ZDOTDIR").map(PathBuf::from).unwrap_or_else(|| env.home.clone());
            vec![zdot.join(".zshrc")]
        }
        Shell::Bash => {
            let mut v = vec![env.home.join(".bashrc")];
            if cfg!(target_os = "macos") {
                v.push(env.home.join(".bash_profile"));
            }
            v
        }
        Shell::Fish => {
            let cfg = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| env.home.join(".config"));
            vec![cfg.join("fish/conf.d/claude-account.fish")]
        }
    }
}

fn block(shell: Shell, path_dir: Option<&Path>) -> String {
    let mut lines =
        vec![BEGIN.to_string(), "# Managed by claude-account; remove with `claude-account shell uninstall`.".into()];
    match shell {
        Shell::Zsh | Shell::Bash => {
            if let Some(d) = path_dir {
                let q = sh_quote(&d.to_string_lossy());
                lines.push(format!("case \":$PATH:\" in *:{q}:*) ;; *) export PATH={q}\":$PATH\" ;; esac"));
            }
            lines.push(format!(
                "if command -v claude-account >/dev/null 2>&1; then eval \"$(claude-account init {})\"; fi",
                shell.name()
            ));
        }
        Shell::Fish => {
            if let Some(d) = path_dir {
                lines.push(format!("fish_add_path --global --path {}", fish_quote(&d.to_string_lossy())));
            }
            lines.push("if type -q claude-account; claude-account init fish | source; end".into());
        }
    }
    lines.push(END.into());
    lines.join("\n") + "\n"
}

/// Byte range of the managed block: from the start of its BEGIN line to the end
/// of its END line (newline included). Markers are matched per line, ignoring
/// trailing whitespace and `\r`. A BEGIN without an END is an error: guessing
/// where the block ends could eat the rest of the user's file.
fn find_block(text: &str) -> std::result::Result<Option<(usize, usize)>, String> {
    let mut begin = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_end();
        if begin.is_none() && t == BEGIN {
            begin = Some(pos);
        } else if let Some(b) = begin
            && t == END
        {
            return Ok(Some((b, pos + line.len())));
        }
        pos += line.len();
    }
    match begin {
        Some(_) => Err(format!("has a \"{BEGIN}\" line without its \"{END}\" line; fix it by hand")),
        None => Ok(None),
    }
}

/// Separator `install` puts before a block it appends: a blank line after a file
/// that ends with a newline, a single newline after one that does not. `remove`
/// takes back exactly that, so an install/uninstall round trip is byte-exact.
fn separator(old: &str) -> &'static str {
    if old.is_empty() { "" } else { "\n" }
}

fn remove(text: &str, (b, e): (usize, usize)) -> String {
    let mut pre = &text[..b];
    if pre.ends_with('\n') {
        pre = &pre[..pre.len() - 1];
    }
    format!("{pre}{}", &text[e..])
}

fn write_preserving(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    // Follow a symlinked rc file (dotfile managers) instead of replacing the link.
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let perms = fs::metadata(&target).ok().map(|m| m.permissions());
    let tmp = target.with_extension(format!("claude-account.{}", std::process::id()));
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    if let Some(p) = perms {
        let _ = fs::set_permissions(&tmp, p);
    }
    fs::rename(&tmp, &target).map_err(|e| format!("cannot write {}: {e}", target.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Updated,
    Unchanged,
    Removed,
}

/// Add or refresh the managed block. Replaces an existing block in place.
/// With `dry_run`, only report what would change.
pub fn install(env: &Env, shell: Shell, path_dir: Option<&Path>, dry_run: bool) -> Result<Vec<(PathBuf, Change)>> {
    let new_block = block(shell, path_dir);
    let mut done = vec![];
    for rc in rc_files(env, shell) {
        if shell == Shell::Bash && rc.ends_with(".bash_profile") && !rc.exists() {
            continue;
        }
        let old = fs::read_to_string(&rc).unwrap_or_default();
        if shell == Shell::Fish {
            let change = if old == new_block {
                Change::Unchanged
            } else if old.is_empty() {
                Change::Added
            } else {
                Change::Updated
            };
            if change != Change::Unchanged && !dry_run {
                write_preserving(&rc, &new_block)?;
            }
            done.push((rc, change));
            continue;
        }
        let change;
        let found = find_block(&old).map_err(|e| format!("{} {e}", env.tilde(&rc)))?;
        let text = if let Some((b, e)) = found {
            change = if old[b..e] == new_block { Change::Unchanged } else { Change::Updated };
            format!("{}{}{}", &old[..b], new_block, &old[e..])
        } else {
            change = Change::Added;
            format!("{old}{}{new_block}", separator(&old))
        };
        if change != Change::Unchanged && !dry_run {
            write_preserving(&rc, &text)?;
        }
        done.push((rc, change));
    }
    Ok(done)
}

/// Remove the managed block from every shell's files.
pub fn uninstall(env: &Env) -> Result<Vec<(PathBuf, Change)>> {
    let mut done = vec![];
    for shell in Shell::ALL {
        for rc in rc_files(env, shell) {
            let Ok(old) = fs::read_to_string(&rc) else { continue };
            if shell == Shell::Fish {
                if old.contains(BEGIN) {
                    fs::remove_file(&rc).map_err(|e| format!("cannot remove {}: {e}", rc.display()))?;
                    done.push((rc, Change::Removed));
                }
                continue;
            }
            if let Some(range) = find_block(&old).map_err(|e| format!("{} {e}", env.tilde(&rc)))? {
                write_preserving(&rc, &remove(&old, range))?;
                done.push((rc, Change::Removed));
            }
        }
    }
    Ok(done)
}

/// Rc files that currently carry the block.
pub fn installed_in(env: &Env) -> Vec<(Shell, PathBuf)> {
    Shell::ALL
        .iter()
        .flat_map(|&s| rc_files(env, s).into_iter().map(move |f| (s, f)))
        .filter(|(_, f)| fs::read_to_string(f).is_ok_and(|t| t.contains(BEGIN)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(old: &str) -> String {
        let b = block(Shell::Zsh, None);
        let installed = format!("{old}{}{b}", separator(old));
        let range = find_block(&installed).unwrap().unwrap();
        remove(&installed, range)
    }

    #[test]
    fn install_then_remove_is_byte_exact() {
        for old in ["", "a\n", "a", "a\n\n", "a\r\nb\r\n", "a\r\nb", "\n"] {
            assert_eq!(round_trip(old), old, "{old:?}");
        }
    }

    #[test]
    fn markers_are_found_per_line_and_an_unterminated_block_is_refused() {
        let text = format!("x\n{BEGIN}  \r\nbody\n{END}\r\ny\n");
        let (b, e) = find_block(&text).unwrap().unwrap();
        assert_eq!(&text[..b], "x\n");
        assert_eq!(&text[e..], "y\n");
        assert!(find_block(&format!("x\n{BEGIN}\nrest of the file\n")).is_err());
        assert_eq!(find_block("no markers\n").unwrap(), None);
        assert_eq!(find_block(&format!("say {BEGIN} inline\n")).unwrap(), None);
    }

    #[test]
    fn quoting_survives_hostile_paths() {
        assert_eq!(sh_quote("/a b/it's $x `y`"), "'/a b/it'\\''s $x `y`'");
        assert_eq!(fish_quote("/a'b\\c"), "'/a\\'b\\\\c'");
    }

    #[test]
    fn block_only_adds_path_when_asked() {
        assert!(!block(Shell::Zsh, None).contains("PATH"));
        assert!(block(Shell::Bash, Some(Path::new("/o p/bin"))).contains("export PATH='/o p/bin'\":$PATH\""));
        assert!(block(Shell::Fish, None).contains("claude-account init fish | source"));
    }
}
