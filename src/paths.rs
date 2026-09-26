//! Path identity.
//!
//! Paths are only ever compared in canonical form: every symlink resolved and, on
//! case-insensitive filesystems, the letter case as stored on disk.
//! `std::fs::canonicalize` gives exactly that on macOS and Linux.

use std::env;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// Canonical form of an existing directory.
pub fn canonical(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    c.is_dir().then_some(c)
}

/// Absolute form without touching the filesystem: `.` and `..` are folded
/// lexically, the way a shell's `cd -L` does, so symlinks are kept.
pub fn lexical_absolute(p: &Path, base: &Path) -> PathBuf {
    let joined = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The current directory as the user reached it. The shell's `$PWD` keeps the
/// symlinks that were walked through; it is trusted only while it still names the
/// same directory as the process cwd.
pub fn logical_cwd() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    if let Some(pwd) = env::var_os("PWD").map(PathBuf::from)
        && pwd.is_absolute()
        && canonical(&pwd).is_some()
        && canonical(&pwd) == canonical(&cwd)
    {
        return Some(pwd);
    }
    Some(cwd)
}

/// A directory argument in logical form (relative to the logical cwd).
pub fn logical_arg(p: &Path) -> Option<PathBuf> {
    let base = logical_cwd().unwrap_or_else(|| PathBuf::from("/"));
    let abs = lexical_absolute(p, &base);
    canonical(&abs).map(|_| abs)
}

/// Canonical form of a path that may not exist (any more): the nearest existing
/// ancestor canonicalised, plus the missing tail as written.
pub fn canonical_lenient(p: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = std::fs::canonicalize(cur) {
            return tail.iter().rev().fold(c, |acc: PathBuf, part| acc.join(part));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Main worktree of the git repository containing `dir`. The first entry of
/// `git worktree list` is the main worktree whatever the layout (linked worktrees
/// anywhere on disk, bare repositories). None outside a repository or without git.
pub fn main_worktree(dir: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["worktree", "list", "--porcelain"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let first = text.lines().next()?.strip_prefix("worktree ")?;
    canonical(Path::new(first))
}

/// `~`-abbreviated form for display.
pub fn tilde(p: &Path, home: &Path, home_canonical: &Path) -> String {
    for h in [home_canonical, home] {
        if let Ok(rest) = p.strip_prefix(h) {
            return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
        }
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_absolute_folds_dots_without_resolving() {
        let base = Path::new("/a/link");
        assert_eq!(lexical_absolute(Path::new("../b/./c"), base), PathBuf::from("/a/b/c"));
        assert_eq!(lexical_absolute(Path::new("/x/../../y"), base), PathBuf::from("/y"));
        assert_eq!(lexical_absolute(Path::new("."), base), PathBuf::from("/a/link"));
    }

    #[test]
    fn canonical_lenient_resolves_the_existing_part() {
        let tmp = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(tmp.path()).unwrap();
        std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
        assert_eq!(canonical_lenient(&real.join("link/gone/deeper")), real.join("gone/deeper"));
        assert_eq!(canonical_lenient(&real), real);
    }

    #[test]
    fn tilde_prefers_the_canonical_home() {
        let home = Path::new("/var/h");
        let canon = Path::new("/private/var/h");
        assert_eq!(tilde(Path::new("/private/var/h/p"), home, canon), "~/p");
        assert_eq!(tilde(Path::new("/var/h"), home, canon), "~");
        assert_eq!(tilde(Path::new("/var/hx"), home, canon), "/var/hx");
    }
}
