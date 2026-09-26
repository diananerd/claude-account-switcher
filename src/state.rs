//! Profiles, the directory map, and how a directory resolves to a profile.
//!
//! Claude Code keys its login (the macOS Keychain entry, `.claude.json`) to
//! `CLAUDE_CONFIG_DIR`, so every config dir is an independent session and they can
//! all stay logged in at once. A profile is a name for a config dir, either its own
//! (`config_dir`) or borrowed from another profile (`same_as`). The profile whose
//! dir is the base dir (`~/.claude`) runs with `CLAUDE_CONFIG_DIR` unset, so the
//! pre-existing login, and anything launched outside this tool, keep working.
//!
//! Files (XDG Base Directory layout):
//!   $XDG_CONFIG_HOME/claude-account/config.toml    profiles, default, map
//!   $XDG_DATA_HOME/claude-account/profiles/<name>  config dirs this tool created
//!   <any dir>/.claude-account                       local pin: a profile name

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::paths;

pub type Result<T> = std::result::Result<T, String>;

/// Name of the per-directory pin file.
pub const LOCAL_FILE: &str = ".claude-account";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_as: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    /// Canonical directory -> profile.
    #[serde(default)]
    pub map: BTreeMap<PathBuf, String>,
}

impl Default for Config {
    fn default() -> Self {
        Config { version: 1, default: None, profiles: BTreeMap::new(), map: BTreeMap::new() }
    }
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The machine-wide map in config.toml.
    Map,
    /// A `.claude-account` file in the directory.
    File,
}

/// Where a directory's profile came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub profile: String,
    /// The directory that matched: the directory itself or an ancestor.
    pub key: PathBuf,
    pub source: Source,
}

/// Machine-wide locations. Everything is overridable through the environment,
/// so tests (and unusual setups) never touch the real ones.
pub struct Env {
    pub home: PathBuf,
    pub home_canonical: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub base_dir: PathBuf,
}

fn xdg(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(fallback))
}

impl Env {
    pub fn from_process() -> Result<Env> {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
        let config_file = std::env::var_os("CLAUDE_ACCOUNT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| xdg("XDG_CONFIG_HOME", &home, ".config").join("claude-account/config.toml"));
        let data_dir = xdg("XDG_DATA_HOME", &home, ".local/share").join("claude-account");
        let base_dir =
            std::env::var_os("CLAUDE_ACCOUNT_BASE_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
        let home_canonical = paths::canonical(&home).unwrap_or_else(|| home.clone());
        Ok(Env { home, home_canonical, config_file, data_dir, base_dir })
    }

    pub fn managed_dir(&self, name: &str) -> PathBuf {
        self.data_dir.join("profiles").join(name)
    }

    /// Whether a config dir was created by this tool (and so may be deleted by it).
    /// Whether a config dir was created by this tool, and so may be deleted by it:
    /// a direct child of the profiles dir, never that dir itself.
    pub fn is_managed(&self, dir: &Path) -> bool {
        let root = self.data_dir.join("profiles");
        let root = paths::canonical(&root).unwrap_or(root);
        dir.parent() == Some(root.as_path())
    }

    /// The dir whose settings new accounts share, and that `--base` adopts.
    pub fn base_canonical(&self) -> Option<PathBuf> {
        paths::canonical(&self.base_dir)
    }

    /// Whether claude uses `dir` when CLAUDE_CONFIG_DIR is unset (`~/.claude`,
    /// whatever CLAUDE_ACCOUNT_BASE_DIR says). Only that dir may run unset.
    pub fn is_base(&self, dir: &Path) -> bool {
        paths::canonical(&self.home.join(".claude")).as_deref() == Some(dir)
    }

    pub fn tilde(&self, p: &Path) -> String {
        paths::tilde(p, &self.home, &self.home_canonical)
    }

    /// `.claude.json` of a config dir: the base keeps it in `$HOME`.
    pub fn claude_json(&self, config_dir: &Path) -> PathBuf {
        if self.is_base(config_dir) { self.home.join(".claude.json") } else { config_dir.join(".claude.json") }
    }

    pub fn load(&self) -> Result<Config> {
        match fs::read_to_string(&self.config_file) {
            Ok(text) => {
                let cfg: Config = toml::from_str(&text)
                    .map_err(|e| format!("{} is not valid:\n{e}", self.tilde(&self.config_file)))?;
                if cfg.version != 1 {
                    return Err(format!(
                        "{} has version {}; this claude-account understands version 1",
                        self.tilde(&self.config_file),
                        cfg.version
                    ));
                }
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("cannot read {}: {e}", self.tilde(&self.config_file))),
        }
    }

    /// Read-modify-write under an exclusive lock, written atomically, so two
    /// terminals remembering different directories at once cannot lose either.
    pub fn update<T>(&self, f: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
        let dir = self.config_file.parent().ok_or("config file has no parent directory")?;
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", self.tilde(dir)))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(self.config_file.with_extension("lock"))
            .map_err(|e| format!("cannot open the config lock: {e}"))?;
        // SAFETY: flock on a descriptor we own; released when `lock` is dropped.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err("cannot lock the configuration".into());
        }
        let mut cfg = self.load()?;
        let out = f(&mut cfg)?;
        let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
        let text = format!(
            "# claude-account configuration, rewritten on every change: values edited by hand\n\
             # are kept, comments are not. `claude-account doctor` checks it.\n\
             # Map keys are canonical paths (symlinks resolved).\n\n{body}"
        );
        // Write through a symlinked config (dotfile managers) instead of replacing the link.
        let target = fs::canonicalize(&self.config_file).unwrap_or_else(|_| self.config_file.clone());
        let tmp = target.with_extension(format!("tmp.{}", std::process::id()));
        let write = || -> std::io::Result<()> {
            let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&tmp, &target)
        };
        write().map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("cannot write {}: {e}", self.tilde(&self.config_file))
        })?;
        drop(lock);
        Ok(out)
    }
}

/// What happened to a mapped directory that may no longer be there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Present,
    /// Gone while its parent is still there: deleted or renamed.
    Deleted,
    /// Its parent is gone too, or it cannot be read: an unmounted volume, a moved
    /// parent, a permission problem. Kept, since it may come back.
    Unreachable,
}

pub fn reach(dir: &Path) -> Reach {
    match fs::metadata(dir) {
        Ok(m) if m.is_dir() => Reach::Present,
        Ok(_) => Reach::Deleted,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if dir.parent().is_some_and(Path::is_dir) {
                Reach::Deleted
            } else {
                Reach::Unreachable
            }
        }
        Err(_) => Reach::Unreachable,
    }
}

/// The profile named by a `.claude-account` file: its first line that is not
/// blank or a `#` comment, trimmed.
pub fn read_local(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(LOCAL_FILE)).ok()?;
    text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned)
}

impl Config {
    pub fn exists(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }

    /// The configured default when it still exists, else the first profile.
    pub fn default_name(&self) -> Option<String> {
        self.default.clone().filter(|d| self.exists(d)).or_else(|| self.profiles.keys().next().cloned())
    }

    /// Profile names with the default first, the rest alphabetically.
    pub fn ordered(&self) -> Vec<String> {
        let def = self.default_name();
        let mut v: Vec<String> = def.iter().cloned().collect();
        v.extend(self.profiles.keys().filter(|k| Some(*k) != def.as_ref()).cloned());
        v
    }

    /// The profile that owns the config dir `name` ends up using (follows
    /// `same_as`, detecting cycles).
    pub fn owner<'a>(&'a self, name: &'a str) -> Result<&'a str> {
        let mut cur = name;
        for _ in 0..=self.profiles.len() {
            let p = self.profiles.get(cur).ok_or_else(|| format!("no such profile: {cur}"))?;
            match (&p.config_dir, &p.same_as) {
                (Some(_), None) => return Ok(cur),
                (None, Some(next)) => cur = next,
                (Some(_), Some(_)) => return Err(format!("profile {cur} sets both config_dir and same_as")),
                (None, None) => return Err(format!("profile {cur} sets neither config_dir nor same_as")),
            }
        }
        Err(format!("profile {name}: same_as forms a cycle"))
    }

    /// Canonical config dir of a profile.
    pub fn config_dir(&self, name: &str) -> Result<PathBuf> {
        let owner = self.owner(name)?;
        let dir = self.profiles[owner].config_dir.as_ref().expect("owner has config_dir");
        paths::canonical(dir).ok_or_else(|| format!("the config dir of {owner} is missing: {}", dir.display()))
    }

    /// The profile `name` borrows its login from, if it is an alias.
    pub fn alias_target(&self, name: &str) -> Option<String> {
        let owner = self.owner(name).ok()?;
        (owner != name).then(|| owner.to_string())
    }

    /// Profiles that borrow `name`'s login directly.
    pub fn aliases_of(&self, name: &str) -> Vec<String> {
        self.profiles.iter().filter(|(_, p)| p.same_as.as_deref() == Some(name)).map(|(n, _)| n.clone()).collect()
    }

    /// Profiles using `dir`, owners before aliases.
    pub fn profiles_for_dir(&self, dir: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            self.ordered().into_iter().filter(|n| self.config_dir(n).ok().as_deref() == Some(dir)).collect();
        v.sort_by_key(|n| self.alias_target(n).is_some());
        v
    }

    /// First candidate directory that is mapped or pinned. At one directory the
    /// machine map wins over a `.claude-account` file, so a committed pin can
    /// always be overridden locally.
    pub fn first_mapped(&self, candidates: impl IntoIterator<Item = PathBuf>) -> Option<Hit> {
        candidates.into_iter().find_map(|c| {
            if let Some(p) = self.map.get(&c) {
                return Some(Hit { profile: p.clone(), key: c, source: Source::Map });
            }
            // A pin naming a profile this machine does not have (a committed file
            // from someone else's setup) is skipped, never allowed to block claude.
            read_local(&c).filter(|p| self.exists(p)).map(|p| Hit { profile: p, key: c, source: Source::File })
        })
    }

    /// `.claude-account` files on the way up from `logical` that name a profile
    /// that does not exist here (and so are ignored by `lookup`).
    pub fn ignored_pins(&self, logical: &Path) -> Vec<(PathBuf, String)> {
        let mut seen = Vec::new();
        let dirs = logical.ancestors().filter_map(paths::canonical).chain(
            paths::canonical(logical)
                .into_iter()
                .flat_map(|c| c.ancestors().map(Path::to_path_buf).collect::<Vec<_>>()),
        );
        for d in dirs {
            if seen.iter().any(|(p, _)| *p == d) {
                continue;
            }
            if let Some(p) = read_local(&d).filter(|p| !self.exists(p)) {
                seen.push((d, p));
            }
        }
        seen
    }

    /// Resolution.
    ///
    /// A directory has several identity chains, each a list of ancestors ordered
    /// nearest first:
    ///   1. the path as the user reached it (symlinks kept), each ancestor
    ///      canonicalised;
    ///   2. the canonical path;
    ///   3. the canonical path of the repository's main worktree, when git says the
    ///      directory belongs to one.
    ///
    /// The first ancestor, in chain order, that is mapped or pinned wins. The
    /// nearest mapping therefore beats an inherited one, and a directory resolves
    /// the same way however it was reached. No particular layout is special-cased.
    pub fn lookup(&self, logical: &Path) -> Option<Hit> {
        let chain1 = logical.ancestors().filter_map(paths::canonical);
        let chain2 = paths::canonical(logical)
            .map(|c| c.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
            .unwrap_or_default();
        self.first_mapped(chain1.chain(chain2)).or_else(|| {
            let main = paths::main_worktree(logical)?;
            self.first_mapped(main.ancestors().map(Path::to_path_buf))
        })
    }
}

/// Where a choice made in a directory is remembered: the main worktree when inside
/// a repository (every subdirectory and linked worktree then shares it), else the
/// directory itself.
pub fn project_root(logical: &Path) -> Option<PathBuf> {
    paths::main_worktree(logical).or_else(|| paths::canonical(logical))
}

/// True when `root` is `$HOME` or one of its ancestors: a mapping there would
/// silently cover every project.
pub fn covers_home(env: &Env, root: &Path) -> bool {
    env.home_canonical.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.profiles.insert("a".into(), Profile { config_dir: Some("/tmp".into()), same_as: None });
        c.profiles.insert("b".into(), Profile { config_dir: None, same_as: Some("a".into()) });
        c.profiles.insert("c".into(), Profile { config_dir: None, same_as: Some("b".into()) });
        c
    }

    #[test]
    fn owner_follows_alias_chains() {
        let c = cfg();
        assert_eq!(c.owner("c").unwrap(), "a");
        assert_eq!(c.alias_target("c").as_deref(), Some("a"));
        assert_eq!(c.alias_target("a"), None);
        assert_eq!(c.aliases_of("a"), vec!["b"]);
    }

    #[test]
    fn owner_detects_cycles_and_invalid_profiles() {
        let mut c = cfg();
        c.profiles.get_mut("a").unwrap().config_dir = None;
        c.profiles.get_mut("a").unwrap().same_as = Some("c".into());
        assert!(c.owner("a").unwrap_err().contains("cycle"));
        c.profiles.insert("d".into(), Profile::default());
        assert!(c.owner("d").unwrap_err().contains("neither"));
    }

    #[test]
    fn default_falls_back_to_the_first_profile() {
        let mut c = cfg();
        assert_eq!(c.default_name().as_deref(), Some("a"));
        c.default = Some("gone".into());
        assert_eq!(c.default_name().as_deref(), Some("a"));
        c.default = Some("c".into());
        assert_eq!(c.ordered(), vec!["c", "a", "b"]);
    }

    #[test]
    fn nearest_candidate_wins_and_map_beats_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = paths::canonical(tmp.path()).unwrap();
        let (outer, inner) = (root.join("o"), root.join("o/i"));
        fs::create_dir_all(inner.join("deep")).unwrap();
        fs::write(inner.join(LOCAL_FILE), "# pinned\n\n  b  \n").unwrap();
        fs::write(inner.join("deep").join(LOCAL_FILE), "nobody\n").unwrap();
        let mut c = cfg();
        c.map.insert(outer.clone(), "a".into());
        let hit = c.first_mapped(inner.join("deep").ancestors().map(Path::to_path_buf)).unwrap();
        assert_eq!(hit, Hit { profile: "b".into(), key: inner.clone(), source: Source::File });
        assert_eq!(c.ignored_pins(&inner.join("deep")), vec![(inner.join("deep"), "nobody".to_string())]);
        c.map.insert(inner.clone(), "c".into());
        let hit = c.first_mapped(inner.ancestors().map(Path::to_path_buf)).unwrap();
        assert_eq!(hit.source, Source::Map);
        assert_eq!(hit.profile, "c");
    }

    #[test]
    fn config_round_trips_through_toml() {
        let mut c = cfg();
        c.default = Some("b".into());
        c.map.insert("/a b/ñ".into(), "a".into());
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.profiles, c.profiles);
        assert_eq!(back.map, c.map);
        assert!(toml::from_str::<Config>("bogus = 1").is_err());
    }
}
