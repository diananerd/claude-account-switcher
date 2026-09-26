---
name: switch
description: Show or change which Claude Code account (claude-switcher profile) the current project uses. Use when the user asks which account or profile this session or folder uses, wants to switch accounts for a project, map a folder to an account, create an account profile, or log one in.
argument-hint: "[profile]"
allowed-tools: Bash(claude-switcher status *), Bash(claude-switcher list *), Bash(claude-switcher use *), Bash(claude-switcher resolve *), Bash(claude-switcher map *), Bash(claude-switcher doctor *), Bash(command -v claude-switcher)
---

# Switch the Claude Code account of this project

`claude-switcher` maps directories to Claude Code accounts (profiles). Each profile
is its own Claude Code config dir and login; `claude`, launched from a shell with
the claude-switcher integration, picks the profile its directory resolves to.

A running session can never change account: the account is fixed when `claude`
starts. Switching means changing the mapping, then relaunching.

## Steps

1. Check the CLI exists: `command -v claude-switcher`. If it does not, tell the
   user to install it and stop:
   `curl -fsSL https://switcher.diananerd.com | sh`
   then `claude-switcher setup` in a terminal.

2. Read the current state:
   - `claude-switcher status --json` (this directory: `profile`, `source`,
     `matched`, `session_profile`)
   - `claude-switcher list --json` (all profiles: `name`, `email`, `same_as`,
     `default`)

3. Decide the target profile:
   - If the user named one (`$ARGUMENTS` or their message), use it. If it does not
     exist, say so and list the existing ones.
   - Otherwise ask with AskUserQuestion: one option per profile (label = name,
     description = its email, "same login as X" for aliases), the current one
     first and marked "(current)".

4. Apply it for this project: `claude-switcher use <profile>`. It maps the
   repository root (or the current folder outside git), so every subfolder and
   worktree follows. To pin the choice in a file that travels with the repository
   instead, use `claude-switcher use <profile> --local` (writes `.claude-switcher`).

5. Tell the user, briefly:
   - which folder now maps to which profile;
   - that this session still runs as the previous profile, and to switch they
     exit (`/exit`) and run `claude --continue` from a terminal: the conversation
     resumes under the new account.

## Things you must not do

- Do not run `claude-switcher login`, `logout`, `add`, `remove`, `setup` or
  `uninstall`. Login opens a browser for OAuth and needs the user's terminal;
  the others change accounts or delete data. Give the exact command for the user
  to run in their own terminal (for a new account: `claude-switcher add <name>`,
  which logs it in). After relaunching, `/login` inside the new session
  also logs that profile in.
- Do not edit `~/.config/claude-switcher/config.toml` or any Claude Code config
  dir by hand.
- Do not set `CLAUDE_CONFIG_DIR` yourself.
