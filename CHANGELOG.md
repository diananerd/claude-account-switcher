# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-09-26

### Fixed

- `add` accepts `--sso` and `--email`, as the docs and `--help` show, and
  passes them to `claude auth login`. With either, it no longer asks what the
  profile is: login options mean an account of its own.
- Docs: the picker example shows the "+ Add an account" row, and the build
  from source command is two commands again.

## [0.1.0] - 2026-09-26

First public release.

### Added

- `claude-switcher`, with the short command `csw`: each folder decides which
  Claude Code account `claude` uses. The nearest mapped folder wins,
  identically through symlinks, letter case and git worktrees; a machine map
  plus optional `.claude-switcher` pin files.
- Accounts: `add` (its own login, an alias with `--same-as`, or an adopted
  dir), `login` again, `logout`, `rename`, `remove [--purge]`. Every account
  keeps its own login and uses your one local setup; sessions with different
  accounts run in parallel.
- In a folder with no account yet, `claude` asks once and remembers; Enter
  takes the default.
- `setup` for the first run, `doctor [--fix]`, `status`, `list`, `map`,
  `prune`, `update`; every command interactive in a terminal and scriptable
  (`-y`, `--json`, usage errors exit 2).
- Installer at `curl -fsSL https://switcher.diananerd.com | sh`: shows what it
  found and what it will change, checks every dependency, changes nothing until
  the download is verified, explains every failure, converges when run again,
  and uninstalls cleanly.
- Update notice by release type, stable releases only.
- A Claude Code plugin: the `/claude-switcher:switch` skill and a SessionStart
  hook; a status line segment; docs at <https://switcher.diananerd.com>.
