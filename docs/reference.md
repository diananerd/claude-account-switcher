# Reference

## Commands

Every command works in two modes.

- **Interactive**: a terminal on stdin and stderr, and no `--no-input`. Only what
  was not passed is asked, with the likely answer preselected. Confirmations
  default to no when they destroy something, to yes when they build something.
- **Headless**: no terminal, or `--no-input` (also `CLAUDE_ACCOUNT_NO_INPUT=1`).
  Nothing is asked. A missing answer is a usage error (exit 2) naming the
  argument or flag to pass; confirmations need `-y`.

Global flags: `--no-input`, `-y/--yes` (answer yes to confirmations), `--json`
(machine-readable output where supported).

| Command | Does | JSON |
|---|---|---|
| `claude-account` | Terminal: pick this project's profile (current preselected), or create one, or more actions. No terminal: same as `status`. | via `status` |
| `setup [--name N] [--no-shell]` | Interactive: five steps (name the existing login, add accounts and log them in, pick the default, map folders, add the shell integration). Headless: `--name` adopts `~/.claude` under that name and adds the shell integration (unless `--no-shell`). Both end with the next steps. | |
| `use [profile] [dir] [--local]` | Map `dir` (default: the current project root) to a profile. Asks for the profile when omitted and a terminal is present. `--local` writes `dir/.claude-account` instead. `claude-account <profile> [dir]` is a shorthand. | |
| `forget [dir] [--local]` | Remove the mapping (or the `.claude-account` file) of `dir`, which may no longer exist. Nothing to forget (it only inherits, or has no mapping) is a no-op that says what applies instead. | |
| `status [dir]` | Profile that applies, where it comes from, and the running session's profile when it differs. | yes |
| `resolve [dir]` | Only the profile name (empty when none applies). | yes |
| `map` | All mappings; flags deleted and unreachable folders. | yes |
| `prune [--all]` | Drop mappings to deleted folders (gone while their parent exists). Unreachable ones (parent gone too: unmounted volume, moved parent) are kept unless `--all`. Interactive: lists them and confirms. | |
| `list [--check]` | Profiles, their account and flags. `--check` asks `claude auth status` for each login (slower, exact). | yes |
| `default [profile]` | Show or set the profile offered first in a folder with no profile. Interactive without a profile: a picker on the current one (Enter keeps it). The first profile created is the default until another is picked. | yes |
| `new [name] [--same-as P \| --base \| --dir D] [--no-login]` | Create a profile. No flag: a new account (offers to log in). `--same-as`: share P's login. `--base`: adopt `~/.claude`. `--dir`: adopt an existing config dir. Asks for missing parts in a terminal. | |
| `login [profile] [claude auth login args]` | Hands the terminal to `claude auth login` for that profile, then verifies with `claude auth status`. An unknown name (or **+ New profile** in the picker) creates the profile first, in a terminal; without one it fails with the `new` command to run. Warns when two profiles end up on one account, and when there is no terminal to paste a code into. For an alias, logs in its owner. `claude auth login` or `/login` run in a mapped folder log in that folder's profile too. | |
| `logout [profile]` | `claude auth logout` for that profile (and every alias sharing it). | |
| `rename [old] [new]` | Rename (asks for what is missing); mappings, aliases and the default follow. The config dir keeps its path. `.claude-account` files are not rewritten. | |
| `remove [profile] [--force] [--purge]` | Unregister. Refuses while aliases use it. Interactive: picks the profile, asks before dropping its mappings and whether to purge, then confirms (default no). Headless: `--force` (or `-y`) drops its mappings. `--purge` logs it out and deletes its config dir, only for dirs this tool created (needs `--yes` without a terminal); if the logout fails nothing is deleted. An adopted dir such as `~/.claude` keeps its files and login. | |
| `run [profile] [args]` | Run claude with that profile once (asks which when omitted); the map is untouched. Everything after the profile goes to claude. | |
| `doctor [--fix]` | Checks claude, the config, each profile's dir, shared links and login, duplicate accounts, deleted or unreachable mappings, ignored pins, PATH, shell integration. `--fix` relinks and prunes; interactive, it offers to. Exit 1 on errors. | yes |
| `init zsh\|bash\|fish` | Print the `claude` shell function. | |
| `shell install [shell] [--path-dir D] [--dry-run]` | Add the marked block to the rc file (idempotent). Asks for the shell when `$SHELL` does not say. `--dry-run` only reports the file it would change. | |
| `shell uninstall` | Remove the block from every rc file. | |
| `shell status` | Where the block is installed. | yes |
| `completions <shell>` | Shell completions (bash, zsh, fish, elvish, powershell). | |
| `statusline` | Status line segment; reads Claude Code's status line JSON on stdin. | |
| `hook session-start` | SessionStart hook: prints a note for Claude when the session's profile differs from its folder's. | |
| `self-update [--version TAG]` | Runs the official installer into this binary's folder: latest stable release, or TAG. Interactive in a terminal, `-y` otherwise. A copy owned by Homebrew or cargo (or a development build) gets that tool's command instead. | |
| `uninstall [--purge]` | Shows the plan and confirms (default no; `-y` headless). Removes the shell integration and the binary. `--purge`: also log out and delete the profiles it created and its config file (other files next to it stay); stops before changing anything if a logout fails. | |

## Updates

When a newer stable release exists, commands run in a terminal end with a
notice on stderr: `info:` for a patch release, `warning:` for a minor one,
`danger:` for a major one, with the command to update. Not shown without a
terminal, in CI, with `--json`, for `statusline`, `hook`, `init`,
`completions`, `resolve`, `run` and `claude` itself. The latest version is asked
of GitHub at most once a day by a detached background process and cached in
`$XDG_CACHE_HOME/claude-account/update.json` (default `~/.cache/...`), so no
command waits on the network. `doctor` checks it live. Only stable releases
count: GitHub's latest release excludes pre-releases, and release tags with a
suffix (`v0.2.0-rc.1`) are published as pre-releases.

Exit codes: `0` success, `1` error, `2` usage error (also for a missing argument
without a terminal to ask for it), `130` cancelled at a prompt.

## Resolution

For a directory, the first match wins across these chains, each walked from the
directory up to `/`:

1. the path as reached (symlinks kept), each ancestor canonicalised;
2. the canonical path (symlinks resolved, on-disk letter case);
3. the canonical path of the repository's main worktree, if the directory is in
   a git repository.

At each folder, a mapping in `config.toml` beats a `.claude-account` file. A file
naming a profile that does not exist on this machine is ignored, with a warning
at launch and in `doctor`.

If `config.toml` cannot be read, `claude` still starts, exactly as without this
tool, after a warning; `claude-account doctor` shows what is wrong.

With no profiles at all (just installed), `claude` runs untouched and prints a
pointer to `claude-account setup`.

When `claude` runs in a directory where nothing matches: with a terminal, the
picker asks (its **+ New profile** entry creates and logs in a new account on the
spot) and remembers the answer at the project root (the main worktree, or
the folder itself outside git; never `$HOME` or above); without one (or with `-p`,
`--print`, `--help`, `--version`) the default profile is used and nothing is
remembered.

## Files

| Path | What |
|---|---|
| `$XDG_CONFIG_HOME/claude-account/config.toml` (default `~/.config/...`) | Profiles, default, mappings. Mode 0600. |
| `$XDG_DATA_HOME/claude-account/profiles/<name>` (default `~/.local/share/...`) | Config dirs created for new accounts. Mode 0700. |
| `<folder>/.claude-account` | Optional pin: first non-blank, non-`#` line is a profile name. |
| rc files | A block between `# >>> claude-account >>>` and `# <<< claude-account <<<`, edited in place; removal restores the file byte for byte. A start marker without its end marker is never touched. An existing `alias claude=<path>` is replaced by the function, and `<path>` becomes the claude it runs. Fish: `conf.d/claude-account.fish`. |

`config.toml`:

```toml
version = 1
default = "personal"

[profiles.personal]
config_dir = "/Users/you/.claude"

[profiles.work]
config_dir = "/Users/you/.local/share/claude-account/profiles/work"

[profiles.writing]
same_as = "personal"

[map]
"/Users/you/work" = "work"
"/Users/you/work/side-project" = "personal"
"/Users/you/personal" = "personal"
```

Each profile has exactly one of `config_dir` or `same_as`. Map keys are canonical
paths. Writes are atomic and locked, so concurrent launches do not lose changes.

A new account's config dir links these from `~/.claude`: the user-level
configuration Claude Code documents (`settings.json`, `CLAUDE.md`,
`keybindings.json`, `agents/`, `commands/`, `skills/`, `output-styles/`,
`plugins/`), `hooks/`, and what lets a conversation continue under another
account (`projects/`, `history.jsonl`, `file-history/`, `plans/`). Files your
settings point to by absolute path (a status line script, hook scripts) work
from every account without linking. Its `.claude.json` starts with onboarding
state, user MCP servers and project trust copied once from `~/.claude.json`.
Credentials are never read or copied.

## Environment

| Variable | Effect |
|---|---|
| `CLAUDE_ACCOUNT` | Set by `launch` for the session: the profile name. |
| `CLAUDE_CONFIG_DIR` | Set by `launch` for non-base profiles. If already set to a dir that is no profile's, `launch` leaves it alone. |
| `CLAUDE_ACCOUNT_CONFIG` | Use another config file. |
| `CLAUDE_ACCOUNT_BASE_DIR` | Dir whose settings new accounts share and that `--base` adopts (default `~/.claude`). Only `~/.claude` itself runs with `CLAUDE_CONFIG_DIR` unset. |
| `CLAUDE_ACCOUNT_CLAUDE` | Path of the real claude binary (default: `claude` on PATH). |
| `CLAUDE_ACCOUNT_NO_INPUT` | Never prompt. |
| `CLAUDE_ACCOUNT_NO_UPDATE_CHECK` | No update notice, no update check in `doctor`. |
| `CLAUDE_ACCOUNT_UPDATE_URL`, `CLAUDE_ACCOUNT_INSTALLER_URL` | Where the latest release and the installer are fetched from (mirrors, tests). |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` | Standard locations. |

Installer: interactive when a terminal is available (answers are read from
`/dev/tty`, so `curl | sh` works): it downloads and verifies, shows the plan,
offers Proceed / Customize (folder, rc file) / Cancel, and ends with the next
steps, offering `setup` on a fresh install. Headless with `-y`, without a
terminal, or with `CI` set. Flags: `-y`, `--bin-dir`, `--version`,
`--no-modify-rc`, `--uninstall`, `--purge`. Variables: `CLAUDE_ACCOUNT_BIN_DIR`,
`CLAUDE_ACCOUNT_VERSION`, `CLAUDE_ACCOUNT_DOWNLOAD_URL` (mirror of the release
assets). `NO_COLOR` turns colour off, here and in the binary.

The installer's guarantees, each covered by the acceptance suite:

- It shows what it found first: system and target, Claude Code, the installed
  version, another copy earlier on PATH, shell, PATH.
- It checks all dependencies at once and names every missing one with how to
  install it; `wget` stands in for `curl`, `sha256sum` or `openssl` for `shasum`.
- It refuses `sudo` (it would install into root's home), macOS older than 11,
  unsupported systems (Windows: use WSL) and folders it cannot write to, before
  touching anything.
- Nothing changes until the binary is downloaded, matches its SHA-256 and runs.
  Errors say what happened, what to do, and "Nothing was changed" or how to finish.
- Running it again converges: identical files, leftovers of an interrupted run
  cleared, one shell block.

Release assets: `claude-account-<target>.tar.gz` and `.sha256` for
`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`.
