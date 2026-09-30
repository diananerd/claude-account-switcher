# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-09-30

### Added

- Live switching: switching the account of the folder a session runs in
  (`use`, or the plugin's skill) moves the running session too. When Claude's
  reply ends the session restarts under the new account and resumes the same
  conversation, with the options it was started with. Interactive sessions now
  run as a child of `claude-switcher`; Ctrl-Z, signals and the exit status pass
  through. `CLAUDE_SWITCHER_NO_SUPERVISE=1` turns it off.
- `hook stop`, and a Stop hook in the plugin that calls it. The move waits until
  Claude Code records that the turn is over, so every other Stop hook finishes
  first, and none that keeps Claude working is cut short.
- `use` inside a session whose claude-switcher plugin is missing or older than
  0.2.0 says so, with the commands to update it, instead of promising a move
  that would never happen.
- Sessions of every account see and can message each other: `sessions/`, Claude
  Code's list of running sessions, is shared like `projects/`. Account dirs
  from before 0.2.0 are migrated at their next launch.

## [0.1.4] - 2026-09-28

### Added

- Docs site: `llms-full.txt`, the `llms.txt` summary followed by every doc page
  in one plain-text file, for assistants. Both are built from the docs, so they
  cannot drift.

### Fixed

- Docs site: plain-text files (`llms.txt`, `llms-full.txt`) are served as UTF-8,
  so browsers no longer garble the non-ASCII characters in them.
- `add --help` and the command examples show only the login options that are
  documented.

## [0.1.3] - 2026-09-26

### Added

- Docs site: the home page opens with terminal demos (use, switch, set up,
  install) recorded from the real binary by `scripts/record-demos.py`.

### Fixed

- Suggested commands use the name you ran, also when the command stands alone:
  after `csw setup`, "Switch a project later with: csw", not the full name.

## [0.1.2] - 2026-09-26

### Added

- Tests: the docs' output examples (folder mapping, picker, status line,
  `config.toml`) are compared with real output.

### Changed

- The plugin's SessionStart hook now always tells Claude which profile (and
  account email) the session runs as, not only when the folder maps to another
  one, so "which account am I using?" gets a direct answer.

### Fixed

- Docs: `llms.txt` names `add` for new accounts and describes the plugin;
  `AGENTS.md` names `add` and lists every test section; the reference documents
  `created_in_base` in `config.toml`.

## [0.1.1] - 2026-09-26

### Fixed

- `add` accepts `--email`, as the docs and `--help` show, and passes it to
  `claude auth login`. With it, it no longer asks what the profile is: a login
  option means an account of its own.
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
