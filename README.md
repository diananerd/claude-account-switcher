# Claude Account Switcher

`claude-account` runs several Claude Code accounts on one machine and lets each
folder decide which one `claude` uses.

```text
~/work                -> work       (your work account)
~/work/side-project   -> personal   (an override for one folder)
~/personal            -> personal
~/other               -> asks once, then remembers
```

No more `/logout` and `/login` to switch: every account stays logged in, side by
side, and `claude` picks the right one from where you run it.

> Unix-like systems only (macOS, Linux; Windows through WSL). **Tested on macOS.**
>
> Docs: <https://switcher.diananerd.com>

## Install

```sh
curl -fsSL https://switcher.diananerd.com | sh
```

It downloads the binary for your machine from GitHub Releases and checks its
SHA-256, then shows what it will change (the binary in `~/.local/bin`, a marked
block in your shell rc file) and asks: **1) Proceed**, 2) Customize, 3) Cancel.
It ends with the next steps, and on a fresh install offers to run
`claude-account setup` right away.

No terminal, `CI` set, or `sh -s -- -y`: it installs with the defaults without
asking. Options: `sh -s -- --help`.

It first shows what it found (system, Claude Code, any installed copy, shell,
PATH) and checks every dependency at once. Nothing changes until the download is
verified and runs; every error says what to do and whether anything changed.
Running it again is always safe: it upgrades, reinstalls, or finishes an
interrupted install, and leaves the same files every time.

**Update**: `claude-account update` (or run the install command again). It
shows `upgrade 0.1.0 -> 0.1.1` before changing anything and keeps your profiles
and mappings. When a newer release exists, commands you run in a terminal end
with a notice (info for a patch, warning for a minor, danger for a major
release); `claude-account doctor` reports it too. Only stable releases are
offered; a specific one: `update --version v0.1.0` (older ones are shown as
a downgrade). `CLAUDE_ACCOUNT_NO_UPDATE_CHECK=1` turns the check off.

Other ways: build it with Rust 1.88 or newer, then run
`claude-account shell install` and `claude-account setup`:

```sh
cargo install --locked --tag vX.Y.Z \
  --git https://github.com/diananerd/claude-account-switcher
```

## How it works

Claude Code keeps its login and settings in a config dir, `~/.claude` by default,
and honours `CLAUDE_CONFIG_DIR` to use another one. The login is tied to that dir,
so each dir is an independent account.

- A **profile** is a name for a config dir: its own login, or another name for an
  existing profile's login (`--same-as`).
- Your current login in `~/.claude` becomes a profile as it is; nothing is moved.
- New profiles live in `~/.local/share/claude-account/profiles/<name>` and share
  your settings, skills, agents, hooks, plugins, memory and history with
  `~/.claude` through symlinks. Only the login and account identity are separate.
- The shell integration defines a `claude` function that resolves the profile for
  the current directory and runs the real `claude` with it.

## Daily use

Run `claude` as always. In a directory with no profile yet, it asks once:

```text
? Claude Code account for ~/other/new-idea
❯ personal  you@example.com
  work      you@company.com
```

Enter takes the default; type to filter. The choice is remembered for the project
(the repository root, so subfolders and worktrees follow).

Need another account? Pick **+ New profile** there (or run
`claude-account login <new-name>`): it creates the profile, opens the browser to
log in, and continues with it. `claude auth login`, or `/login` inside a session,
also logs in whichever profile the folder uses.

To change a project later, run `claude-account` in it and pick another profile.
A running session keeps its account; exit and run `claude --continue` to
resume the conversation under the new one.

## Which profile applies

The nearest mapped ancestor wins, so a folder inherits its parent's profile and
can override it:

```sh
claude-account use work ~/work                    # everything under ~/work
claude-account use personal ~/work/side-project   # except this folder
```

It resolves the same way through symlinks, in any letter case on case-insensitive
disks, and from git worktrees that live outside their repository.

A `.claude-account` file containing a profile name pins a folder too, and can
be committed (`claude-account use <profile> --local`). The machine mapping wins
over a file at the same folder.

## Commands

| | |
|---|---|
| `claude-account` | switch this project's profile (interactive) |
| `claude-account setup` | guided first-time setup |
| `claude-account status` | which profile applies here, and why |
| `claude-account use <profile> [dir]` | map a project or folder |
| `claude-account forget [dir]` | remove a mapping |
| `claude-account list` | profiles and logins |
| `claude-account new <name>` | create a profile |
| `claude-account login <profile>` | log in (opens the browser; `--sso` for SSO); creates the profile if new |
| `claude-account run <profile> [args]` | run claude with a profile once |
| `claude-account doctor` | check everything (`--fix` repairs) |
| `claude-account update` | update to the latest stable release |

Every command and flow works both ways:

- **Interactive** (in a terminal): it asks only for what you did not pass, with
  the likely answer preselected. Destructive questions default to no.
- **Headless** (scripts, CI, `--no-input`): it never asks. Pass everything as
  arguments, `-y` to accept confirmations, `--json` for machine-readable output.
  A missing answer is a usage error (exit 2) that names the flag to pass.

Full reference: [docs/reference.md](docs/reference.md).

## Status line

Show the active profile in Claude Code's status line by adding this to your
status line script:

```sh
profile=$(echo "$input" | claude-account statusline)   # "work" or "work (here: personal)"
```

## Claude Code plugin

Optional. Lets Claude show or switch the project's profile (`/claude-account:switch`)
and tells Claude when a session runs under a different profile than its folder.

```text
/plugin marketplace add diananerd/claude-account-switcher
/plugin install claude-account@claude-account-switcher
```

Update it with `/plugin marketplace update claude-account-switcher`, then
`/plugin update claude-account@claude-account-switcher`; remove it with
`/plugin uninstall claude-account@claude-account-switcher`. It needs the CLI;
without it the skill says how to install it and the hook does nothing.

## Uninstall

```sh
claude-account uninstall            # shell integration and binary; keeps profiles
claude-account uninstall --purge    # also logs out and deletes the profiles it created
```

Both show what they will remove and ask first (default no); add `-y` to skip
the question. Or through the installer:

```sh
curl -fsSL https://switcher.diananerd.com | sh -s -- --uninstall [--purge]
```

`~/.claude` is never touched.

## Limitations

- Only launches through your shell are routed. The desktop app and IDE extensions
  use `~/.claude` unless started from a shell that has the integration.
- A session cannot change account while running (relaunch with `claude --continue`).
- Logging in uses whatever claude.ai account your browser is signed in to; switch
  there (or use a private window) before `claude-account login`.

## Status

Provided as-is. There is no commitment to regular maintenance and no
contribution process: issues and pull requests may go unanswered. For changes or
customisation, fork it; [AGENTS.md](AGENTS.md) maps the code, and

```sh
cargo test                  # unit tests
tests/acceptance.sh         # end-to-end, in a throwaway HOME (needs expect, jq, git)
```

check that a fork still behaves.

## License

[MIT](LICENSE)
