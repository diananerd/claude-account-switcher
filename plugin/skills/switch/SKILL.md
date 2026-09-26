---
name: switch
description: Show or change which Claude Code account (claude-account profile) the current project uses. Use when the user asks which account or profile this session or folder uses, wants to switch accounts for a project, map a folder to an account, create an account profile, or log one in.
argument-hint: "[profile]"
allowed-tools: Bash(claude-account status *), Bash(claude-account list *), Bash(claude-account use *), Bash(claude-account resolve *), Bash(claude-account map *), Bash(claude-account doctor *), Bash(command -v claude-account)
---

# Switch the Claude Code account of this project

`claude-account` maps directories to Claude Code accounts (profiles). Each profile
is its own Claude Code config dir and login; `claude`, launched from a shell with
the claude-account integration, picks the profile its directory resolves to.

A running session can never change account: the account is fixed when `claude`
starts. Switching means changing the mapping, then relaunching.

## Steps

1. Check the CLI exists: `command -v claude-account`. If it does not, tell the user
   to install it and stop:
   `curl -fsSL https://switcher.diananerd.com | sh`
   then `claude-account setup` in a terminal.

2. Read the current state:
   - `claude-account status --json` (this directory: `profile`, `source`,
     `matched`, `session_profile`)
   - `claude-account list --json` (all profiles: `name`, `email`, `same_as`,
     `default`)

3. Decide the target profile:
   - If the user named one (`$ARGUMENTS` or their message), use it. If it does not
     exist, say so and list the existing ones.
   - Otherwise ask with AskUserQuestion: one option per profile (label = name,
     description = its email, "same login as X" for aliases), the current one
     first and marked "(current)".

4. Apply it for this project: `claude-account use <profile>`. It maps the
   repository root (or the current folder outside git), so every subfolder and
   worktree follows. To pin the choice in a file that travels with the repository
   instead, use `claude-account use <profile> --local` (writes `.claude-account`).

5. Tell the user, briefly:
   - which folder now maps to which profile;
   - that this session still runs as the previous profile, and to switch they
     exit (`/exit`) and run `claude --continue` from a terminal: the conversation
     resumes under the new account.

## Things you must not do

- Do not run `claude-account login`, `logout`, `new`, `remove`, `setup` or
  `uninstall`. Login opens a browser for OAuth and needs the user's terminal;
  the others change accounts or delete data. Give the exact command for the user
  to run in their own terminal (for a new profile: `claude-account new <name>`,
  which offers to log in). After relaunching, `/login` inside the new session
  also logs that profile in.
- Do not edit `~/.config/claude-account/config.toml` or any Claude Code config
  dir by hand.
- Do not set `CLAUDE_CONFIG_DIR` yourself.
