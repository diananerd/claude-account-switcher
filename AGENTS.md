# AGENTS.md

Guide for coding agents (and humans) changing this code, typically in a fork:
the project is provided as-is and takes no contributions. Users of the tool want
[README.md](README.md) and [docs/reference.md](docs/reference.md).

## What it is

A Rust CLI, `claude-account`, that picks a Claude Code config dir
(`CLAUDE_CONFIG_DIR`) per directory, plus a POSIX installer and an optional
Claude Code plugin. Unix-like only.

## Map

| Path | Role |
|---|---|
| `src/main.rs` | CLI definition (clap), dispatch, shared helpers |
| `src/state.rs` | Model: `Env` (locations), `Config` (config.toml), profiles, **resolution** |
| `src/paths.rs` | Canonical and logical paths, git main worktree |
| `src/launch.rs` | `launch`/`run` (exec claude), picker on first launch, status line, hook |
| `src/mapping.rs` | `use`, `forget`, `status`, `resolve`, `map`, `prune` |
| `src/profiles.rs` | `new`, `login`, `logout`, `rename`, `remove`, `list`, `default`, shared links |
| `src/setup.rs` | `setup` wizard, bare-command switcher, `shell`, `uninstall` |
| `src/doctor.rs` | `doctor` |
| `src/shell.rs` | Shell function and marked rc blocks |
| `src/claude.rs` | Every call to the real `claude` binary |
| `src/update.rs` | Update notice (cached, background), `update` (runs the installer) |
| `scripts/check-release.sh` | Versions, lock file, changelog and tag agree |
| `src/ui.rs` | Prompts (dialoguer), all on stderr |
| `install.sh` | One-line installer / uninstaller (POSIX sh) |
| `plugin/` | Claude Code plugin; `.claude-plugin/marketplace.json` lists it |
| `tests/acceptance.sh` | Black-box end-to-end suite |

## Rules the code keeps

1. Paths are compared only in canonical form (`std::fs::canonicalize`). Map keys
   are canonical. Never compare a logical path with a canonical one.
2. Resolution is one general rule (identity chains, nearest match first; see
   `Config::lookup`). Do not add special cases for particular layouts.
3. The profile whose config dir is the base dir runs with `CLAUDE_CONFIG_DIR`
   unset. Others get the canonical dir: the Keychain entry is keyed to that
   string, so a different spelling would be a different login.
4. Credentials are never read, copied or written by this tool; logins go through
   `claude auth login|logout|status`, with the terminal inherited.
5. Only dirs under the data dir may be deleted. `~/.claude` is never modified,
   except creating the shared subdirectories new accounts link to.
6. `config.toml` is written only through `Env::update` (flock + atomic rename).
7. Rc files are edited only between the markers, in place; install is idempotent
   and uninstall restores the file byte for byte.
8. Every command and flow has two modes. Interactive (terminal on stdin and
   stderr, no `--no-input`): ask only for what is missing, preselect the likely
   answer, destructive confirmations default to no. Headless: never ask; use a
   documented default or fail with a `usage:` error (exit 2) naming the flag;
   `-y` accepts confirmations. Never hang. Flows end with numbered next steps.
   The installer follows the same rules (answers from `/dev/tty`).
9. `launch` and `run <profile>` pass every following argument to claude verbatim,
   bypassing clap.
10. Code, comments and messages in English.

## Testing

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
tests/acceptance.sh              # builds release, runs everything (~1 min)
tests/acceptance.sh <section>    # profiles inherit local symlink worktree env forget
                                 # lifecycle doctor shell integration picker setup install
```

The acceptance suite uses a throwaway `HOME` and a fake `claude` (prints its
environment; implements `auth status|login|logout`). Interactive flows run in a
real pty through `expect`. Every behaviour change needs a case there; a bug fix
needs the case that failed before the fix.

## CI

`.github/workflows/ci.yml`, on every push to main and every pull request, on
macOS and Linux: `scripts/check-release.sh`, rustfmt, clippy, unit tests and the
acceptance suite, plus shellcheck and dash on the shell scripts, and a build with
the `rust-version` Cargo.toml declares (the MSRV; raise it when the code needs a
newer Rust, never leave it claiming less).

## Releasing

1. Bump `version` in `Cargo.toml` and `plugin/.claude-plugin/plugin.json`, run
   `cargo build` (updates `Cargo.lock`), add `## [X.Y.Z] - YYYY-MM-DD` to
   `CHANGELOG.md`. `scripts/check-release.sh` must pass.
2. Commit, push, wait for CI to pass.
3. `git tag -a vX.Y.Z -m "claude-account X.Y.Z" && git push origin vX.Y.Z`.

`.github/workflows/release.yml` then checks the tag with
`scripts/check-release.sh vX.Y.Z`, builds the four targets, publishes the
release (notes from the changelog entry) and smoke-tests the public one-liner,
install and uninstall, on each of the four binaries.

`install.sh` is served from `main`, not from a release: a change there reaches
users as soon as it is pushed (after up to five minutes of CDN cache), so it must
keep working with every published release.

## Docs site and short install address

`https://switcher.diananerd.com` is one Cloudflare Worker on a Custom Domain
(`deploy/wrangler.jsonc`, `deploy/worker.js`): `curl`/`wget` asking for `/`,
and anyone asking for `/install.sh`, get `install.sh` from `main`; everything
else is the VitePress site in `docs/`, served as Workers Static Assets.
Workers only (never Pages), Custom Domains only (never routes).

```sh
cd docs && npm install
npm run dev       # local preview
npm run deploy    # build and deploy; needs CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID
```

The token needs Workers Scripts: Edit on the account and Workers Routes: Edit on
the zone. `guide.md` includes `README.md`; `reference.md` is the reference; the
site gets `llms.txt` at build time. Redeploy after changing them.

A bad release: do not move or reuse its tag. Delete the GitHub release (the tag
can stay), fix, and publish the next patch version; `latest` then points to it.
Users on the bad one update by running the installer again.
