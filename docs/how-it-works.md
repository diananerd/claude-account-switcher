# How it works

For the curious. You do not need any of this to use it.

## Accounts are config dirs

Claude Code keeps its login and settings in a config dir, `~/.claude` by
default, and uses another one when `CLAUDE_CONFIG_DIR` points to it. The login
belongs to that dir (on macOS, the Keychain entry is keyed to it), so each dir
is an independent account, and all of them stay logged in at once.

- A **profile** is a name for a config dir: its own login, or another name for
  an existing profile's login (`--same-as`).
- The login already in `~/.claude` becomes a profile as it is; nothing moves.
  That profile runs with `CLAUDE_CONFIG_DIR` unset, exactly as before.
- New profiles live in `~/.local/share/claude-account/profiles/<name>`. They
  share your settings, skills, agents, commands, hooks, plugins, memory and
  conversation history with `~/.claude` through symlinks; only the login is
  separate. That is why `claude --continue` can resume a conversation under
  another account.
- The shell integration (a marked block in your rc file) defines a `claude`
  function. It finds the profile for the current folder and runs the real
  `claude` with that profile's config dir.

## Which account applies

The nearest mapped folder wins, so a folder inherits its parent's account and
can override it:

```sh
csw use work ~/work                    # everything under ~/work
csw use personal ~/work/side-project   # except this folder
```

A folder resolves the same way however you reach it: through symlinks, in any
letter case on case-insensitive disks, and from git worktrees that live outside
their repository (they follow their repository). The exact rules are in the
[reference](reference.md#resolution).

A `.claude-account` file containing a profile name pins a folder too, and can be
committed (`csw use <profile> --local`). The machine mapping wins over a file in
the same folder, and a file naming a profile you do not have is ignored with a
warning.

## The installer

`curl -fsSL https://switcher.diananerd.com | sh` runs `install.sh` from the
repository's main branch.

- It shows what it found (system, Claude Code, any installed copy, shell, PATH)
  and checks every dependency before changing anything.
- Nothing changes until the download matches its SHA-256 and runs. Every error
  says what happened, what to do, and whether anything changed.
- Running it again is safe: it upgrades, reinstalls, downgrades on request, or
  finishes an interrupted install, and leaves the same files every time.
- Options, after `sh -s --`: `-y` for no questions, `--version TAG`,
  `--bin-dir DIR`, `--no-modify-rc`, `--alias NAME` or `--no-alias` for the
  short command, `--uninstall [--purge]`. See `sh -s -- --help`.

## Building from source

With Rust 1.88 or newer:

```sh
cargo install --locked --tag vX.Y.Z \
  --git https://github.com/diananerd/claude-account-switcher
claude-account shell install && claude-account setup
```

A build installed this way has no `csw`; add one with
`ln -s claude-account ~/.cargo/bin/csw`, or use the full name.
