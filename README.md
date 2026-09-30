# Claude Account Switcher

Keep your Claude Code accounts apart, for work, personal or a side project, and
let each folder decide which one `claude` uses. Every account stays logged in;
you never `/logout` and `/login` again.

> Unix-like systems only (macOS, Linux; Windows through WSL). **Tested on macOS.**
>
> Docs: <https://switcher.diananerd.com>

## Install

```sh
curl -fsSL https://switcher.diananerd.com | sh
```

It shows what it will change and asks first. It also adds **`csw`**, a short
name for the `claude-switcher` command; the examples below use it.

## Set up

```sh
csw setup
```

Short steps: name the account Claude Code is already logged in to, add your
other accounts (each logs in once in the browser), pick the default, and map
your folders, for example `~/work` to your work account.

## Use

Run `claude` as always. It uses the account of the folder you are in:

```text
~/work                -> work       (your work account)
~/work/side-project   -> personal   (an override for one folder)
~/personal            -> personal
~/other               -> asks once, then remembers
```

In a folder with no account yet, it asks once and remembers the answer:

```text
? Claude Code account for ~/other/new-idea ›
❯ personal  you@example.com
  work      you@company.com
  + Add an account
```

Enter takes the default; type to filter; **+ Add an account** adds one on the
spot.

| Command | Does |
| --- | --- |
| `csw` | switch this project's account (asks which) |
| `csw use work` | use `work` for this project; add a folder to map that one instead |
| `csw status` | which account applies here, and why |
| `csw add client` | add an account and log it in |
| `csw list` | your accounts and their logins |
| `csw login client` | log an account in again, e.g. when its login expired |
| `csw remove client` | remove an account (keeps its login unless `--purge`) |
| `csw doctor` | check everything (`--fix` repairs) |
| `csw update` | update to the latest release |

Sessions with different accounts run side by side, each in its own terminal;
`csw run work` starts one with a given account without changing the mapping.
Switching the account of the folder a session runs in moves that session
too: ask Claude to switch (`/claude-switcher:switch`) or run `csw use <account>`
from inside it, and when Claude's reply ends the session restarts under the new
account and carries on with the same conversation. Sessions started before
0.2.0, or outside the shell integration, keep their account: exit and run
`claude --continue`.

Every command also works in scripts: pass everything as arguments, `-y` to
accept confirmations, `--json` for machine-readable output.

## Update and uninstall

```sh
csw update              # latest release, accounts kept
csw uninstall           # removes the tool, keeps accounts
csw uninstall --purge   # also removes the accounts it added
```

When a new release is out, commands you run in a terminal end with a notice.
Each command shows what it will do and asks first; `-y` skips the question.

## Claude Code plugin

Tells Claude which account each session runs as, and when that is not the
folder's, and lets Claude show or switch the project's account
(`/claude-switcher:switch`). Moving a running session to another account needs
it (0.2.0 or later); everything else works without it. One install serves every
account.

```text
/plugin marketplace add diananerd/claude-account-switcher
/plugin install claude-switcher@claude-account-switcher
```

To update or remove it:

```text
/plugin marketplace update claude-account-switcher
/plugin update claude-switcher@claude-account-switcher
/plugin uninstall claude-switcher@claude-account-switcher
```

## Status line

To show the active account in Claude Code's status line, add this to your
status line script:

```sh
account=$(echo "$input" | claude-switcher statusline)   # "work" or "work (here: personal)"
```

## Limitations

- Only launches through your shell are routed. The desktop app and IDE
  extensions use `~/.claude` unless started from a shell with the integration.
- Only sessions launched through the shell integration (0.2.0 or later) switch
  account live, and only between accounts that share the conversation history
  (every account `csw add` creates does). Any other session relaunches with
  `claude --continue`.
- Logging in uses whatever claude.ai account your browser is signed in to;
  switch there (or use a private window) before `csw add` or `csw login`.

## More

- [Reference](docs/reference.md): every command, file, variable and exit code.
- [How it works](docs/how-it-works.md): profiles, which account applies,
  the installer, building from source.
- [Comparison](docs/comparison.md): other tools, and when to pick them.

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
