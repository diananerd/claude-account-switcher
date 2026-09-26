# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.2] - 2026-09-25

### Fixed

- `uninstall --purge` leaves nothing behind: it also deletes the update cache
  and the folders it had created in `~/.claude` (only while still empty).
- Logging a profile out reports it by name instead of passing `claude`'s own
  output through.
- When the installer runs `setup`, there is one "Next steps" list, with opening
  a new terminal first, and no empty shell step.
- The rc block puts `"$HOME/.local/bin"` on PATH instead of an absolute path.

### Added

- A comparison with other tools in the docs.
- CI validates the plugin and marketplace with `claude plugin validate`.

## [0.1.1] - 2026-09-25

### Fixed

- Pre-releases compare in semver order: `update` and `doctor` no longer
  call a pre-release "the latest stable release", the release that follows a
  pre-release is offered as an update, and the installer calls it an upgrade.
- `forget` with nothing to forget is a no-op (exit 0) that says what applies
  instead, so scripts can run it unconditionally.
- Every argument in `--help` says what it is and what happens when omitted.

### Added

- The acceptance suite runs every command's help, headless run and `--json`
  output, reading the command list from `--help`.

## [0.1.0] - 2026-09-25

First public release.

### Added

- Profiles: a Claude Code config dir with its own login, an alias of another
  profile's login (`--same-as`), or an adopted existing dir (`--base`,
  `--dir`). New accounts share settings, skills, plugins and history with
  `~/.claude`; only the login is separate.
- Per-folder resolution: the nearest mapped ancestor wins, identically through
  symlinks, letter case and git worktrees; a machine map plus optional
  `.claude-account` pin files.
- The `claude` shell function (zsh, bash, fish): picks the folder's profile,
  and asks once in a folder with none (Enter takes the default).
- Every command interactive (asks only what is missing) and headless
  (arguments, `-y`, `--json`, usage errors exit 2).
- Profile lifecycle: `setup`, `new`, `login` (verified), `logout`, `rename`,
  `remove [--purge]`, `use`, `forget`, `status`, `list`, `map`, `prune`,
  `doctor [--fix]`.
- Update notice (info, warning or danger by release type) and an update command,
  stable releases only.
- Installer at `curl -fsSL https://switcher.diananerd.com | sh`: inspects the
  machine, checks every dependency, changes nothing until the download is
  verified, explains every failure, converges when run again; `--uninstall`.
- Status line segment, a SessionStart hook and a `/claude-account:switch`
  skill in the Claude Code plugin.
- Docs site at <https://switcher.diananerd.com>.
