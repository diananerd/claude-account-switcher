# Comparison

Several tools help you use more than one Claude Code account on one machine.
They solve different problems. This page says which problem each one solves, so
you can pick the right one, including when it is not this one.

## Who this tool is for

People who keep separate Claude accounts **for separate purposes**: work,
personal, a client, a side project. Each project always belongs to one of them,
so the account should follow the project, fixed and without thinking about it.

- **Per project, fixed.** Each folder maps to one account; subfolders inherit
  and can override. The same folder always gets the same account.
- **Deterministic.** The nearest mapped folder wins, resolved the same way
  through symlinks, letter case and git worktrees. No state that depends on
  which account you used last.
- **Good defaults.** In a folder with no account yet, `claude` asks once and
  remembers. Every login stays live, and your settings, skills, plugins and
  history are shared, so switching projects never means logging in again.

It is **not** built for pooling several accounts to stretch rate limits,
rotating accounts inside one project, or tracking usage and cost. If that is
what you need, the tools below that focus on it will serve you better.

## Approaches

| Approach | How the account is chosen | Examples |
| --- | --- | --- |
| Swap the global login | One login at a time; switching replaces it for the whole machine, running sessions included | claude-swap, cc-account-switcher, CCSwitcher, ClaudeCodeMultiAccounts |
| Rotate for limits | Moves to another account when one hits its limit, often with usage dashboards | claude-swap, clauth, the fairy-pitta fork of cc-account-switcher |
| Profile per launch | A config dir per profile; you pick it with an alias, a flag or a command each time | shell aliases, ccprofile, claude-account-switch (ftery0) |
| Profile per folder | A config dir per profile, chosen by the folder you are in | claude-account, claude-code-profiles, cpm |

## Tools

As of September 2026, from each project's own documentation.

| Tool | Mechanism | Granularity | Focus |
| --- | --- | --- | --- |
| [claude-account](https://github.com/diananerd/claude-account-switcher) | One `CLAUDE_CONFIG_DIR` per account; the `claude` shell function picks it | Per folder, central mapping (plus optional pin files); asks once where unmapped | Separate accounts by purpose |
| [claude-code-profiles](https://github.com/pegasusheavy/claude-code-profiles) | One `CLAUDE_CONFIG_DIR` per profile, set by a wrapper | Global, per shell, or per folder through a `.claude-profile` file and a cd hook | Separate accounts by purpose |
| [cpm](https://github.com/JakubKontra/claude-profile-manager) | One `CLAUDE_CONFIG_DIR` per profile, a wrapper per profile | Per command, or per folder through a `.claude-profile` file and a cd hook | Separate accounts by purpose |
| [claude-account-switch](https://github.com/ftery0/claude-account-switch) | One `CLAUDE_CONFIG_DIR` per profile | Per shell | Separate accounts by purpose |
| [ccprofile](https://github.com/stebennett/claude-code-profiles) | Sets `CLAUDE_CONFIG_DIR` at launch | Chosen at each launch | Separate accounts by purpose |
| [claude-swap](https://github.com/realiti4/claude-swap) | Swaps the default login (Keychain or credentials file); can run one session per account | Global, or per terminal | Rotation at rate limits, usage dashboard |
| [clauth](https://github.com/uwuclxdy/clauth) | Swaps tokens, or runs a session in its own config dir | Global or per session | Rotation, usage and cost |
| [cc-account-switcher](https://github.com/ming86/cc-account-switcher) | Swaps the Keychain login | Global | Switching (archived) |
| [CCSwitcher](https://github.com/XueshiQiao/CCSwitcher) | Swaps the Keychain login and `~/.claude.json` | Global | Switching, usage and cost (macOS app) |

## How the per-folder tools differ

claude-code-profiles and cpm share the idea of choosing the account by folder.
The differences that matter day to day:

- **Where the mapping lives.** They read a `.claude-profile` file in the
  project. claude-account keeps one central mapping, so nothing is added to your
  repositories; a pin file is optional, for teams that want it committed.
- **A folder with no account yet.** claude-account asks once, with the default
  preselected, and remembers the answer for the whole repository.
- **Resolution.** claude-account resolves a folder identically however you
  reach it: through symlinks, in any letter case on case-insensitive disks, and
  from git worktrees outside their repository.
- **What stays shared.** claude-account shares settings, skills, agents,
  commands, hooks, plugins and conversation history across accounts through
  symlinks; only the login is separate.
- **Operations.** An installer that shows its plan and converges when run again,
  `doctor`, `update`, and every command usable both interactively and in
  scripts.

## Without a tool

Claude Code already supports this through `CLAUDE_CONFIG_DIR`, which moves its
config dir, login included. Two common recipes:

- **An alias per account**, such as
  `alias claude-work='CLAUDE_CONFIG_DIR=~/.claude-work claude'`. You pick the
  account by hand each time, and each account starts from an empty setup.
- **direnv**, with an `.envrc` exporting `CLAUDE_CONFIG_DIR` in each project.
  It switches when you cd, but every project needs its own file and a
  `direnv allow`, and folders without one get nothing.

`CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) and `ANTHROPIC_PROFILE`
are aimed at scripts and at Console or API logins, not at keeping several Pro
or Max accounts side by side.
