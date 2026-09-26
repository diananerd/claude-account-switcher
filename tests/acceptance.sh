#!/bin/bash
# End-to-end acceptance tests for claude-account, black-box: they only run the
# binary (and the installer) and look at what a fake `claude` receives. Every case
# runs against a throwaway HOME, so real accounts are never touched. Interactive
# cases drive a real pseudo-terminal through expect(1).
#
#   tests/acceptance.sh            everything (builds the release binary first)
#   tests/acceptance.sh picker     only sections whose name matches
#   CA=/path/to/claude-account tests/acceptance.sh   test another build

set -u -o pipefail

REPO=$(cd -P "$(dirname "$0")/.." && pwd)
if [ -z "${CA:-}" ]; then
  (cd "$REPO" && cargo build --release --quiet) || exit 1
  CA="$REPO/target/release/claude-account"
fi
CA=$(cd -P "$(dirname "$CA")" && pwd)/$(basename "$CA")
FILTER="${1:-}"
PASS=0; FAIL=0; SKIP=0; FAILED=""

# The default TMPDIR on macOS sits under /var, itself a symlink to /private/var,
# so every path below is reached through a real symlink for free.
SANDBOX=$(mktemp -d "${TMPDIR:-/tmp}/claude-account-test.XXXXXX")
trap 'rm -rf "$SANDBOX"' EXIT

export HOME="$SANDBOX/home" SHELL=/bin/zsh
unset CLAUDE_CONFIG_DIR CLAUDE_ACCOUNT CLAUDE_ACCOUNT_NO_INPUT CLAUDE_ACCOUNT_CONFIG CLAUDE_ACCOUNT_BASE_DIR \
      CLAUDE_ACCOUNT_CLAUDE CLAUDECODE XDG_CONFIG_HOME XDG_DATA_HOME ZDOTDIR
mkdir -p "$HOME/.claude" "$SANDBOX/bin"
export GIT_CONFIG_GLOBAL="$SANDBOX/gitconfig" GIT_CONFIG_NOSYSTEM=1
git config --global user.name test; git config --global user.email test@example.com
git config --global init.defaultBranch main

# Fake claude: prints the environment it was launched with, and implements
# `auth status|login|logout` against a marker file in the config dir, updating
# .claude.json the way the real one does.
cat > "$SANDBOX/bin/claude" <<'EOF'
#!/bin/bash
dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
json="$HOME/.claude.json"; if [ -n "${CLAUDE_CONFIG_DIR:-}" ]; then json="$CLAUDE_CONFIG_DIR/.claude.json"; fi
if [ "${1:-}" = "--version" ]; then echo "9.9.9 (Claude Code)"; exit 0; fi
if [ "${1:-}" = "--help" ]; then
  echo "CFG=${CLAUDE_CONFIG_DIR-<unset>} ACCT=${CLAUDE_ACCOUNT-<unset>} ARGS=$*"
  printf 'Usage: claude [options] [command] [prompt]\n\nCommands:\n  auth  Manage auth\n  update|upgrade  Update\n  mcp  MCP\n'
  exit 0
fi
case "${1:-} ${2:-}" in
  "auth status")
    if [ -f "$dir/.fake-login" ] && [ -n "${FAKE_NO_EMAIL:-}" ]; then echo '{"loggedIn":true}'
    elif [ -f "$dir/.fake-login" ]; then printf '{"loggedIn":true,"email":"%s","orgName":"Org"}\n' "$(cat "$dir/.fake-login")"
    else echo '{"loggedIn":false,"authMethod":"none"}'; fi ;;
  "auth login")
    if [ "${FAKE_LOGIN_FAIL:-}" = 1 ]; then echo "Login cancelled" >&2; exit 1; fi
    email="${FAKE_LOGIN_EMAIL:-$(basename "$dir")@example.com}"
    echo "$email" > "$dir/.fake-login"
    t=$(mktemp); { jq --arg e "$email" '.oauthAccount.emailAddress = $e' "$json" 2>/dev/null \
      || jq -n --arg e "$email" '{oauthAccount: {emailAddress: $e}}'; } > "$t" && mv "$t" "$json"
    echo "Login successful ARGS=$*" ;;
  "auth logout")
    if [ "${FAKE_LOGOUT_FAIL:-}" = 1 ]; then echo "Logout failed" >&2; exit 1; fi
    rm -f "$dir/.fake-login"
    t=$(mktemp); jq 'del(.oauthAccount)' "$json" > "$t" 2>/dev/null && mv "$t" "$json"
    echo "Logged out" ;;
  *) echo "CFG=${CLAUDE_CONFIG_DIR-<unset>} ACCT=${CLAUDE_ACCOUNT-<unset>} ARGS=$*" ;;
esac
EOF
chmod +x "$SANDBOX/bin/claude"
export PATH="$SANDBOX/bin:$(dirname "$CA"):$PATH"
# Never ask GitHub: "the latest release" is a local file, the current version.
CURRENT_V=$("$CA" --version | cut -d' ' -f2)
latest_is() { printf '{"tag_name":"v%s"}' "$1" > "$SANDBOX/latest.json"; }
latest_is "$CURRENT_V"
export CLAUDE_ACCOUNT_UPDATE_URL="file://$SANDBOX/latest.json"

cat > "$HOME/.claude.json" <<'EOF'
{"oauthAccount":{"emailAddress":"work@example.com"},"hasCompletedOnboarding":true,
 "lastOnboardingVersion":"2.1.0","mcpServers":{"pencil":{"command":"pencil"}},
 "projects":{"/somewhere":{"hasTrustDialogAccepted":true}},"userID":"must-not-be-copied"}
EOF
echo work@example.com > "$HOME/.claude/.fake-login"
echo '{"enabledPlugins":{}}' > "$HOME/.claude/settings.json"
echo '# shared memory' > "$HOME/.claude/CLAUDE.md"

# ------------------------------------------------------------------ helpers

ok()   { PASS=$((PASS + 1)); printf '  ok    %s\n' "$1"; }
bad()  { FAIL=$((FAIL + 1)); FAILED="$FAILED
  - $1"; printf '  FAIL  %s\n        %s\n' "$1" "$2"; }
skip() { SKIP=$((SKIP + 1)); printf '  skip  %s (%s)\n' "$1" "$2"; }
want() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "expected: $2 | got: $3"; fi; }
want_has() { case "$3" in *"$2"*) ok "$1" ;; *) bad "$1" "missing: $2 | got: $3" ;; esac; }
want_not() { case "$3" in *"$2"*) bad "$1" "unexpected: $2 | got: $3" ;; *) ok "$1" ;; esac; }
fails() { if "${@:2}" >/dev/null 2>&1; then bad "$1" "succeeded"; else ok "$1"; fi; }
selected() { [ -z "$FILTER" ] || case "$1" in *"$FILTER"*) return 0 ;; *) return 1 ;; esac; }
section() { printf '\n%s\n' "$1"; }

launch_in() { (cd "$1" && "$CA" launch -p probe 2>&1); }
cfg_of()   { launch_in "$1" | sed -n 's/^CFG=\([^ ]*\) .*/\1/p'; }
acct_of()  { launch_in "$1" | sed -n 's/.* ACCT=\([^ ]*\) .*/\1/p'; }
canon()    { (cd -P "$1" && pwd -P); }
mapped_to() { "$CA" map --json | jq -r --arg p "$1" 'to_entries[] | select(.value == $p) | .key'; }
map_json() { "$CA" map --json | jq -S -c .; }
profile_json() { "$CA" list --json | jq -c --arg n "$1" '.[] | select(.name == $n)'; }

# Drive a command in a real pty: <dir> <command...> -- <keys...>. Keys are ENTER,
# DOWN, UP, ESC or literal text. Prints the transcript and "<<exit N>>".
pty_in() {
  local dir="$1"; shift
  DIR="$dir" expect -f - "$@" <<'EOF' 2>&1
set timeout 8
log_user 1
set sep [lsearch -exact $argv --]
set cmd [lrange $argv 0 [expr {$sep - 1}]]
set keys [lrange $argv [expr {$sep + 1}] end]
cd $env(DIR)
spawn -noecho {*}$cmd
if {[llength $keys] > 0} {
  expect {
    -re {account for|Profile for|What is|Profile name|Default profile|Name for this|Log .* in|already logged|There is no profile|Cancel|Continue\?|which profile\?|Which shell|Fix [0-9]|Drop these|Run claude once|New name} {}
    timeout { puts "\n<<timeout waiting for a prompt>>" }
    eof { puts "\n<<eof before a prompt>>" }
  }
}
foreach k $keys {
  sleep 0.35
  switch -- $k {
    ENTER { send -- "\r" }
    DOWN { send -- "\033\[B" }
    UP { send -- "\033\[A" }
    ESC { send -- "\033" }
    default { send -- $k }
  }
}
set timeout 4
expect {
  eof {}
  timeout { puts "\n<<still running>>"; close }
}
catch wait result
puts "\n<<exit [lindex $result 3]>>"
EOF
}
picker_in() { local dir="$1"; shift; pty_in "$dir" "$CA" launch -- "$@"; }

# ------------------------------------------------------------------ fixtures

W="$HOME/work"
mkdir -p "$W/sub/deeper" "$W/client-proj/deep" "$W/two words/ñandú" "$HOME/elsewhere" "$HOME/personal/projects/a"
ln -s "$W/client-proj" "$HOME/link-to-client"
ln -s "$HOME/elsewhere" "$W/linkdir"

git -C "$W" init -q repo && mkdir -p "$W/repo/src/lib"
git -C "$W/repo" commit -q --allow-empty -m init
git -C "$W/repo" worktree add -q "$HOME/wt/feature" -b feature
git -C "$W/repo" worktree add -q "$HOME/wt/override" -b override
mkdir -p "$HOME/wt/feature/pkg"
git init -q --bare "$HOME/bare.git"
git -C "$HOME/bare.git" worktree add -q --orphan "$HOME/bt" 2>/dev/null || true

"$CA" new work --base >/dev/null
"$CA" new client --no-login >/dev/null
"$CA" new personal --same-as work >/dev/null
"$CA" default personal >/dev/null
"$CA" use work "$W" >/dev/null 2>&1
"$CA" use client "$W/client-proj" >/dev/null 2>&1
"$CA" use client "$HOME/wt/override" >/dev/null 2>&1
"$CA" use personal "$HOME/personal" >/dev/null 2>&1
if [ -d "$HOME/bt" ]; then "$CA" use client "$HOME/bare.git" >/dev/null 2>&1; fi
CLIENT_DIR=$("$CA" list --json | jq -r '.[] | select(.name == "client") | .config_dir')
CONFIG="$HOME/.config/claude-account/config.toml"

# ------------------------------------------------------------------ cases

if selected profiles; then
  section "profiles"
  want "profiles: the config follows XDG" "yes" "$([ -f "$CONFIG" ] && echo yes)"
  want "profiles: managed dirs live under XDG data" "$(canon "$HOME/.local/share/claude-account/profiles/client")" "$CLIENT_DIR"
  want "profiles: base profile runs with CLAUDE_CONFIG_DIR unset" "<unset>" "$(cfg_of "$W")"
  want "profiles: own profile gets its canonical config dir" "$CLIENT_DIR" "$(cfg_of "$W/client-proj")"
  want "profiles: alias of the base also runs unset" "<unset>" "$(cfg_of "$HOME/personal/projects/a")"
  want "profiles: CLAUDE_ACCOUNT names the alias, not its target" "personal" "$(acct_of "$HOME/personal/projects/a")"
  want_has "profiles: list marks the default first" "personal" "$("$CA" list | head -1)"
  want_has "profiles: list shows aliases" "same login as work" "$("$CA" list | grep '^personal')"
  want "profiles: list --json" "personal,client,work" "$("$CA" list --json | jq -r '[.[].name] | join(",")')"
  want "profiles: list --check asks claude" "false" "$("$CA" list --check --json | jq -r '.[] | select(.name == "client") | .logged_in')"
  want "profiles: new account seeds only the allowed keys" \
    '["hasCompletedOnboarding","lastOnboardingVersion","mcpServers","projects"]' "$(jq -c 'keys' "$CLIENT_DIR/.claude.json")"
  want "profiles: seeded .claude.json is private" "-rw-------" "$(ls -l "$CLIENT_DIR/.claude.json" | cut -c1-10)"
  want "profiles: managed dir is private" "drwx------" "$(ls -ld "$CLIENT_DIR" | cut -c1-10)"
  want "profiles: config.toml is private" "-rw-------" "$(ls -l "$CONFIG" | cut -c1-10)"
  want "profiles: shared settings.json links to the base" "$HOME/.claude/settings.json" "$(readlink "$CLIENT_DIR/settings.json")"
  want "profiles: missing shared dirs are created in the base and linked" "$HOME/.claude/agents" "$(readlink "$CLIENT_DIR/agents")"
  for n in Upper list -dash 'a b' '' setup "$(printf 'x%.0s' $(seq 41))"; do
    fails "profiles: rejects name \"${n:0:12}\"" "$CA" new "$n" --no-login
  done
  fails "profiles: rejects duplicates" "$CA" new client --no-login
  fails "profiles: alias of a missing profile fails" "$CA" new ghost --same-as nobody
  want "profiles: a failed new leaves nothing behind" "" "$(profile_json ghost)"
  fails "profiles: a config dir cannot be adopted twice" "$CA" new other --base
  fails "profiles: new without a name and no terminal fails" "$CA" new --no-input
fi

if selected inherit; then
  section "inheritance and overrides"
  want "inherit: a subdirectory inherits its mapped ancestor" "work" "$(acct_of "$W/sub/deeper")"
  want "inherit: a nested mapping overrides the parent" "client" "$(acct_of "$W/client-proj")"
  want "inherit: below the override, the override wins" "client" "$(acct_of "$W/client-proj/deep")"
  want "inherit: spaces and non-ASCII in the path" "work" "$(acct_of "$W/two words/ñandú")"
  want "inherit: status says where it comes from" "Source:    inherited from ~/work" "$("$CA" status "$W/sub/deeper" | grep Source)"
  want "inherit: status on the mapped dir itself" "Source:    mapped here" "$("$CA" status "$W" | grep Source)"
  want "inherit: status --json" "true" "$("$CA" status --json "$W/sub" | jq -r .inherited)"
fi

if selected local; then
  section "local .claude-account files"
  mkdir -p "$W/pinned/inner" "$W/pinned-bad"
  printf '# pinned for this repo\n\nclient\n' > "$W/pinned/.claude-account"
  want "local: a .claude-account file pins its tree" "client" "$(acct_of "$W/pinned/inner")"
  want "local: status names the file" "Source:    pinned by ~/work/pinned/.claude-account" "$("$CA" status "$W/pinned/inner" | grep Source)"
  "$CA" use work "$W/pinned" >/dev/null 2>&1
  want "local: the machine map overrides a file at the same level" "work" "$(acct_of "$W/pinned/inner")"
  "$CA" forget "$W/pinned" >/dev/null
  echo nobody > "$W/pinned-bad/.claude-account"
  out=$(cd "$W/pinned-bad" && "$CA" launch -p x 2>&1)
  want_has "local: a file naming an unknown profile is ignored with a warning" "ignoring ~/work/pinned-bad/.claude-account" "$out"
  want_has "local: ...and never blocks claude (falls back to the parent)" "ACCT=work" "$out"
  want_has "local: doctor points at it" "names nobody, which does not exist here" "$(cd "$W/pinned-bad" && "$CA" doctor 2>&1)"
  rm "$W/pinned-bad/.claude-account"
  mkdir -p "$W/pin-me"
  "$CA" use client "$W/pin-me" --local >/dev/null 2>&1
  want "local: use --local writes the file" "client" "$(cat "$W/pin-me/.claude-account")"
  want "local: ...and the map is untouched" "" "$(mapped_to client | grep pin-me)"
  "$CA" forget --local "$W/pin-me" >/dev/null
  want "local: forget --local removes it" "no" "$([ -e "$W/pin-me/.claude-account" ] && echo yes || echo no)"
fi

if selected symlink; then
  section "symlinks"
  want "symlink: reaching a mapped subtree through a link" "client" "$(acct_of "$HOME/link-to-client")"
  want "symlink: a link inside work pointing outside inherits work" "work" "$(acct_of "$W/linkdir")"
  want "symlink: the link's target reached directly is unmapped" "personal" "$(acct_of "$HOME/elsewhere")"
  mkdir -p "$HOME/real/proj"; ln -s "$HOME/real" "$HOME/alias-of-real"
  "$CA" use client "$HOME/alias-of-real/proj" >/dev/null 2>&1
  want "symlink: map key is canonical whatever spelling was used" "$(canon "$HOME/real/proj")" "$(mapped_to client | grep real)"
  want "symlink: resolves through the canonical path" "client" "$(acct_of "$HOME/real/proj")"
  if [ "$(cd -L "$HOME" && pwd -L)" != "$(canon "$HOME")" ]; then
    want "symlink: a symlinked ancestor of HOME changes nothing" "work" "$(acct_of "$(canon "$W")/sub")"
  else skip "symlink: symlinked ancestor of HOME" "TMPDIR is not behind a symlink here"; fi
  up=$(printf '%s' "$W/sub" | tr '[:lower:]' '[:upper:]')
  if [ -d "$up" ]; then want "symlink: other letter case on a case-insensitive FS" "work" "$(acct_of "$up")"
  else skip "symlink: letter case" "filesystem is case-sensitive"; fi
fi

if selected worktree; then
  section "git worktrees"
  want "worktree: a repo subdir inherits work" "work" "$(acct_of "$W/repo/src/lib")"
  want "worktree: a linked worktree outside work resolves as its repo" "work" "$(acct_of "$HOME/wt/feature/pkg")"
  want "worktree: a worktree with its own mapping overrides" "client" "$(acct_of "$HOME/wt/override")"
  if [ -d "$HOME/bt" ]; then want "worktree: a worktree of a bare repo resolves as the bare repo" "client" "$(acct_of "$HOME/bt")"
  else skip "worktree: bare repo" "this git cannot add an orphan worktree"; fi
  (cd "$HOME/wt/feature/pkg" && "$CA" use client >/dev/null 2>&1)
  want "worktree: use inside a worktree maps the main worktree" "$(canon "$W/repo")" "$(mapped_to client | grep '/repo$')"
  want "worktree: ...so the whole repo follows" "client" "$(acct_of "$W/repo/src")"
  (cd "$W/repo/src" && "$CA" forget >/dev/null)
  want "worktree: forget restores inheritance" "work" "$(acct_of "$HOME/wt/feature")"
fi

if selected env; then
  section "environment and non-interactive use"
  want "env: a foreign CLAUDE_CONFIG_DIR passes through" "/tmp/foreign" \
    "$(cd "$W" && CLAUDE_CONFIG_DIR=/tmp/foreign "$CA" launch -p x | sed -n 's/^CFG=\([^ ]*\) .*/\1/p')"
  want "env: a profile dir inherited from a parent session is re-resolved" "<unset>" \
    "$(cd "$W" && CLAUDE_CONFIG_DIR="$CLIENT_DIR" "$CA" launch -p x | sed -n 's/^CFG=\([^ ]*\) .*/\1/p')"
  want "env: run uses a profile once" "$CLIENT_DIR" "$(cd "$W" && "$CA" run client -p x | sed -n 's/^CFG=\([^ ]*\) .*/\1/p')"
  before=$(map_json)
  (cd "$HOME/elsewhere" && "$CA" run client -p x >/dev/null)
  want "env: run does not touch the map" "$before" "$(map_json)"
  want "env: arguments reach claude untouched" "ARGS=-p a b  c" "$(cd "$W" && "$CA" launch -p "a b " c | sed -n 's/.*\(ARGS=.*\)/\1/p')"
  want "env: launch hands --help to claude" "ARGS=--help" "$(cd "$W" && "$CA" launch --help | sed -n 's/.*\(ARGS=.*\)/\1/p')"
  want "env: run hands flags after the profile to claude" "ARGS=--help --model x -p" \
    "$(cd "$W" && "$CA" run client --help --model x -p | sed -n 's/.*\(ARGS=.*\)/\1/p')"
  mkdir "$HOME/gone"
  want "env: a deleted cwd falls back to the default" "personal" \
    "$(cd "$HOME/gone" && rmdir "$HOME/gone" && "$CA" launch -p x 2>/dev/null | sed -n 's/.* ACCT=\([^ ]*\) .*/\1/p')"
  fails "env: default rejects unknown profiles" "$CA" default nobody
  fails "env: run with an unknown profile fails loudly" "$CA" run nobody -p x
  want "env: resolve --json" '{"matched":"'"$(canon "$W")"'","profile":"work"}' \
    "$("$CA" resolve --json "$W/sub" | jq -S -c .)"
  want_has "env: bare command without a terminal prints status" "Profile:   work" "$(cd "$W" && "$CA" </dev/null 2>&1)"
  fails "env: use without a profile and no terminal fails" "$CA" use --no-input
  (
    export HOME="$SANDBOX/headless-setup"; mkdir -p "$HOME/.claude"; echo '{}' > "$HOME/.claude.json"; : > "$HOME/.zshrc"
    "$CA" setup > "$SANDBOX/hs0.log" 2>&1; echo $? > "$SANDBOX/hs0.rc"
    "$CA" setup --name main > "$SANDBOX/hs1.log" 2>&1; echo $? > "$SANDBOX/hs1.rc"
    "$CA" setup --name main > "$SANDBOX/hs2.log" 2>&1; echo $? > "$SANDBOX/hs2.rc"
    grep -c '>>> claude-account >>>' "$HOME/.zshrc" > "$SANDBOX/hs.blocks"
    "$CA" list --json | jq -r '.[0].name + " " + (.[0].base|tostring) + " " + (.[0].default|tostring)' > "$SANDBOX/hs.list"
    mkdir -p "$HOME/p"; (cd "$HOME/p" && rm -f "$HOME/.config/claude-account/config.toml" && "$CA" launch -p x > "$SANDBOX/hs.launch" 2>&1)
  )
  want "env: headless setup without --name is a usage error" "2" "$(cat "$SANDBOX/hs0.rc")"
  want_has "env: ...naming the flag" "setup --name" "$(cat "$SANDBOX/hs0.log")"
  want "env: headless setup --name adopts ~/.claude, as the default (first profile)" "main true true" "$(cat "$SANDBOX/hs.list")"
  want_has "env: ...adds the shell integration and says what next" "Next steps" "$(cat "$SANDBOX/hs1.log")"
  want "env: headless setup is idempotent" "0 1" "$(cat "$SANDBOX/hs2.rc") $(cat "$SANDBOX/hs.blocks")"
  want_has "env: with no profiles at all, claude still runs" "ARGS=-p x" "$(cat "$SANDBOX/hs.launch")"
  want_has "env: ...and points at setup" "claude-account setup" "$(cat "$SANDBOX/hs.launch")"
  mkdir -p "$HOME/short"
  "$CA" client "$HOME/short" >/dev/null 2>&1
  want "env: claude-account <profile> [dir] is short for use" "client" "$("$CA" resolve "$HOME/short")"
  fails "env: unknown words are errors" "$CA" nosuchthing
  want_has "env: mapping HOME warns that it covers everything" "contains your home folder" "$("$CA" use personal "$HOME" 2>&1)"
  "$CA" forget "$HOME" >/dev/null
  mkdir -p "$HOME/par"; for i in $(seq 1 16); do mkdir -p "$HOME/par/$i"; done
  for i in $(seq 1 16); do "$CA" use client "$HOME/par/$i" >/dev/null 2>&1 & done; wait
  want "env: concurrent writes lose nothing" "16" "$(mapped_to client | grep -c '/par/')"
  cp "$CONFIG" "$SANDBOX/config.bak"; echo 'bogus = [' >> "$CONFIG"
  want_has "env: a broken config is reported, not overwritten" "is not valid" "$("$CA" list 2>&1)"
  out=$(launch_in "$W")
  want_has "env: with a broken config claude still launches" "ARGS=-p probe" "$out"
  want_has "env: ...unchanged" "CFG=<unset> ACCT=<unset>" "$out"
  want_has "env: ...and says why" "launching claude unchanged" "$out"
  cp "$SANDBOX/config.bak" "$CONFIG"
  rm -rf "$HOME/par/3"; "$CA" prune >/dev/null
  want "env: prune drops mappings to missing directories" "15" "$(mapped_to client | grep -c '/par/')"
fi

if selected forget; then
  section "forget"
  "$CA" forget "$W/sub" >/dev/null 2>&1; rc=$?
  want "forget: an inherited dir is a no-op (exit 0)" "0" "$rc"
  want_has "forget: ...that says where it inherits from" "inherits work from ~/work" "$("$CA" forget "$W/sub" 2>&1)"
  want "forget: ...and changes nothing" "work" "$("$CA" resolve "$W/sub")"
  "$CA" forget "$W/client-proj" >/dev/null
  want "forget: removing an override restores the parent" "work" "$(acct_of "$W/client-proj/deep")"
  "$CA" use client "$W/client-proj" >/dev/null 2>&1
fi

if selected lifecycle; then
  section "profile lifecycle"
  out=$("$CA" login client </dev/null 2>&1)
  want_has "lifecycle: login hands over to claude auth login" "Login successful" "$out"
  want_has "lifecycle: ...warns which browser account is used" "whatever claude.ai account" "$out"
  want_has "lifecycle: ...and verifies the result" "client is logged in as client@example.com" "$out"
  want "lifecycle: list shows the new login" "client@example.com" "$(profile_json client | jq -r .email)"
  want_has "lifecycle: login passes extra args (--sso)" "ARGS=auth login --sso" "$("$CA" login client --sso </dev/null 2>&1)"
  want_has "lifecycle: logging an alias in logs its owner in" "personal uses the login of work" \
    "$(FAKE_LOGIN_EMAIL=work@example.com "$CA" login personal </dev/null 2>&1)"
  "$CA" new dup --no-login >/dev/null
  want_has "lifecycle: two profiles on one account are flagged" "client is logged in to the same account" \
    "$(FAKE_LOGIN_EMAIL=client@example.com "$CA" login dup </dev/null 2>&1)"
  want_has "lifecycle: a failed login fails" "did not complete" "$(FAKE_LOGIN_FAIL=1 "$CA" login dup </dev/null 2>&1)"
  out=$("$CA" login brandnew </dev/null 2>&1)
  want_has "lifecycle: login of an unknown profile without a terminal says how to create it" "claude-account new brandnew" "$out"
  want "lifecycle: ...and creates nothing" "" "$(profile_json brandnew)"
  want_has "lifecycle: login without a terminal warns about pasting codes" "No terminal on stdin" "$("$CA" login dup </dev/null 2>&1)"
  want_has "lifecycle: a missing claude is named, with where to get it" "not found. Install Claude Code" \
    "$(CLAUDE_ACCOUNT_CLAUDE=/nonexistent/claude "$CA" run client -p x 2>&1)"
  want_has "lifecycle: ...also for login" "not found. Install Claude Code" \
    "$(CLAUDE_ACCOUNT_CLAUDE=/nonexistent/claude "$CA" login client </dev/null 2>&1)"
  "$CA" logout dup >/dev/null 2>&1
  want "lifecycle: logout" "false" "$("$CA" list --check --json | jq -r '.[] | select(.name == "dup") | .logged_in')"
  "$CA" use dup "$HOME/short" >/dev/null 2>&1
  "$CA" new dup-alias --same-as dup >/dev/null
  "$CA" rename dup twin >/dev/null 2>&1
  want "lifecycle: rename moves mappings" "twin" "$("$CA" resolve "$HOME/short")"
  want "lifecycle: rename re-points aliases" "twin" "$(profile_json dup-alias | jq -r .same_as)"
  fails "lifecycle: rename refuses an existing name" "$CA" rename twin client
  fails "lifecycle: remove refuses a profile others borrow" "$CA" remove twin
  "$CA" remove dup-alias >/dev/null
  fails "lifecycle: remove refuses a mapped profile without --force" "$CA" remove twin
  twin_dir=$(profile_json twin | jq -r .config_dir)
  fails "lifecycle: remove --purge without a terminal needs --yes" "$CA" remove twin --force --purge
  "$CA" remove twin --force >/dev/null 2>&1
  want "lifecycle: remove keeps the dir without --purge" "yes" "$([ -d "$twin_dir" ] && echo yes)"
  want "lifecycle: ...and drops its mappings" "" "$("$CA" resolve "$HOME/short")"
  # twin was renamed from dup: its dir keeps the name dup (the login is keyed to the path).
  want_has "lifecycle: new over a leftover dir suggests reusing it" "Reuse it with: claude-account new dup --dir" \
    "$("$CA" new dup --no-login 2>&1)"
  "$CA" new twin --dir "$twin_dir" --no-login >/dev/null 2>&1
  want "lifecycle: ...and --dir reuses it" "$twin_dir" "$(profile_json twin | jq -r .config_dir)"
  "$CA" remove twin >/dev/null 2>&1
  "$CA" new vanish --no-login >/dev/null; mkdir -p "$HOME/vanish-here"; "$CA" use vanish "$HOME/vanish-here" >/dev/null 2>&1
  rm -rf "$(profile_json vanish | jq -r .config_dir)"
  want_has "lifecycle: a profile whose dir was deleted points to doctor" "claude-account doctor" "$(launch_in "$HOME/vanish-here")"
  "$CA" doctor >/dev/null 2>&1; rc=$?
  want "lifecycle: ...and doctor fails on it" "1" "$rc"
  "$CA" remove vanish --force >/dev/null 2>&1
  want "lifecycle: ...and it can still be removed" "" "$(profile_json vanish)"
  "$CA" new gone --no-login >/dev/null; "$CA" login gone </dev/null >/dev/null 2>&1
  gone_dir=$(profile_json gone | jq -r .config_dir)
  "$CA" remove gone --purge --yes >/dev/null 2>&1
  want "lifecycle: remove --purge deletes a managed dir" "no" "$([ -d "$gone_dir" ] && echo yes || echo no)"
  mkdir -p "$HOME/external-cfg"
  "$CA" new adopted --dir "$HOME/external-cfg" >/dev/null 2>&1
  want "lifecycle: new --dir adopts an existing config dir" "$(canon "$HOME/external-cfg")" "$(profile_json adopted | jq -r .config_dir)"
  echo adopted@example.com > "$HOME/external-cfg/.fake-login"
  out=$("$CA" remove adopted --purge --yes 2>&1)
  want "lifecycle: --purge never deletes a dir it did not create" "yes" "$([ -d "$HOME/external-cfg" ] && echo yes)"
  want "lifecycle: ...nor logs it out" "yes" "$([ -f "$HOME/external-cfg/.fake-login" ] && echo yes)"
  want_has "lifecycle: ...and says so" "only logs out and deletes config dirs this tool created" "$out"
  want "lifecycle: the base dir is never deleted" "yes" "$([ -d "$HOME/.claude" ] && echo yes)"
fi

if selected doctor; then
  section "doctor"
  out=$("$CA" doctor 2>&1); rc=$?
  want_has "doctor: finds claude" "claude found: 9.9.9" "$out"
  want_has "doctor: checks every login" "client: logged in as client@example.com" "$out"
  want "doctor: exits 0 with only warnings" "0" "$rc"
  mv "$CLIENT_DIR/settings.json" "$SANDBOX/s.bak"
  want_has "doctor: notices a broken shared link" "client: not sharing settings.json" "$("$CA" doctor 2>&1)"
  "$CA" doctor --fix >/dev/null 2>&1
  want "doctor: --fix relinks it" "$HOME/.claude/settings.json" "$(readlink "$CLIENT_DIR/settings.json")"
  mkdir -p "$HOME/tmpmap"; "$CA" use client "$HOME/tmpmap" >/dev/null 2>&1; rmdir "$HOME/tmpmap"
  want_has "doctor: notices deleted mappings" "were deleted" "$("$CA" doctor 2>&1)"
  "$CA" doctor --fix >/dev/null 2>&1
  want_not "doctor: --fix prunes them" "tmpmap" "$("$CA" map)"
  want "doctor: --json" "ok" "$("$CA" doctor --json | jq -r '.[0].level')"
  cp "$CONFIG" "$SANDBOX/config.bak"
  printf '\n[profiles.broken]\nsame_as = "loop"\n[profiles.loop]\nsame_as = "broken"\n' >> "$CONFIG"
  "$CA" doctor >/dev/null 2>&1; rc=$?
  want "doctor: a same_as cycle is an error (exit 1)" "1" "$rc"
  cp "$SANDBOX/config.bak" "$CONFIG"
fi

if selected shell; then
  section "shell integration"
  printf 'export KEEP=1\nalias ll=ls\n' > "$HOME/.zshrc"
  "$CA" shell install zsh >/dev/null 2>&1
  "$CA" shell install zsh >/dev/null 2>&1
  want "shell: install is idempotent (one block)" "1" "$(grep -c '>>> claude-account >>>' "$HOME/.zshrc")"
  want_has "shell: keeps the rest of the rc file" "alias ll=ls" "$(cat "$HOME/.zshrc")"
  "$CA" shell install zsh --path-dir "/opt/ca bin" >/dev/null 2>&1
  want "shell: a changed block is replaced in place" "1" "$(grep -c '>>> claude-account >>>' "$HOME/.zshrc")"
  want_has "shell: --path-dir adds PATH, quoted" "export PATH='/opt/ca bin'" "$(cat "$HOME/.zshrc")"
  if command -v zsh >/dev/null; then
    want "shell: a zsh sourcing the rc routes claude" "work" \
      "$(cd "$W" && ZDOTDIR="$HOME" zsh -ic 'claude -p x' 2>/dev/null | sed -n 's/.* ACCT=\([^ ]*\) .*/\1/p')"
  else skip "shell: zsh" "zsh not installed"; fi
  want "shell: bash function routes claude" "client" \
    "$(cd "$W/client-proj" && bash -c "$("$CA" init bash); claude -p x" | sed -n 's/.* ACCT=\([^ ]*\) .*/\1/p')"
  "$CA" shell install fish >/dev/null 2>&1
  want "shell: fish gets its own conf.d file" "yes" "$([ -f "$HOME/.config/fish/conf.d/claude-account.fish" ] && echo yes)"
  mkdir -p "$HOME/dotfiles"; printf 'export A=1\n' > "$HOME/dotfiles/bashrc"; ln -s "$HOME/dotfiles/bashrc" "$HOME/.bashrc"
  "$CA" shell install bash >/dev/null 2>&1
  want "shell: a symlinked rc file stays a symlink" "$HOME/dotfiles/bashrc" "$(readlink "$HOME/.bashrc")"
  want_has "shell: ...and its target gets the block" "claude-account init bash" "$(cat "$HOME/dotfiles/bashrc")"
  want "shell: status lists every install" "3" "$("$CA" shell status --json | jq length)"
  "$CA" shell uninstall >/dev/null 2>&1
  want "shell: uninstall restores the rc file exactly" "$(printf 'export KEEP=1\nalias ll=ls')" "$(cat "$HOME/.zshrc")"
  want "shell: ...the symlinked one too" "export A=1" "$(cat "$HOME/dotfiles/bashrc")"
  want "shell: ...and removes the fish file" "no" "$([ -e "$HOME/.config/fish/conf.d/claude-account.fish" ] && echo yes || echo no)"
fi

if selected review; then
  section "review findings"
  # 1: a BEGIN without END is refused, the file untouched
  printf 'a\n# >>> claude-account >>>\nuser line 1\nuser line 2\n' > "$SANDBOX/half.rc"; cp "$SANDBOX/half.rc" "$HOME/.zshrc"
  fails "review: install refuses an rc with a start marker but no end marker" "$CA" shell install zsh
  want "review: ...and leaves it untouched" "" "$(cmp "$HOME/.zshrc" "$SANDBOX/half.rc" 2>&1)"
  "$CA" shell uninstall >/dev/null 2>&1
  want "review: uninstall does not eat the rest of such a file" "" "$(cmp "$HOME/.zshrc" "$SANDBOX/half.rc" 2>&1)"
  # 3: byte-exact round trips (no final newline, CRLF, trailing blank line)
  for kind in nonl crlf blank; do
    case $kind in nonl) printf 'export A=1' ;; crlf) printf 'export A=1\r\nexport B=2\r\n' ;; blank) printf 'x\n\n' ;; esac > "$SANDBOX/orig.rc"
    cp "$SANDBOX/orig.rc" "$HOME/.zshrc"; "$CA" shell install zsh >/dev/null 2>&1; "$CA" shell uninstall >/dev/null 2>&1
    want "review: install+uninstall is byte-exact ($kind)" "" "$(cmp "$HOME/.zshrc" "$SANDBOX/orig.rc" 2>&1)"
  done
  # 2: an existing alias claude=... (older installs) does not break the function
  mkdir -p "$SANDBOX/localclaude"; cp "$SANDBOX/bin/claude" "$SANDBOX/localclaude/claude"
  nopath=$(printf '%s' "$PATH" | tr ':' '\n' | grep -vx "$SANDBOX/bin" | paste -sd: -)
  if command -v zsh >/dev/null; then
    printf 'alias claude="%s"\n' "$SANDBOX/localclaude/claude" > "$HOME/.zshrc"; "$CA" shell install zsh >/dev/null 2>&1
    out=$(cd "$W" && PATH="$nopath" ZDOTDIR="$HOME" zsh -ic 'claude -p x' 2>&1)
    want_has "review: zsh with a claude alias still routes through the tool" "ACCT=work" "$out"
    want_not "review: ...without a parse error" "parse error" "$out"
    "$CA" shell uninstall >/dev/null 2>&1
  else skip "review: zsh alias" "zsh not installed"; fi
  out=$(cd "$W" && PATH="$nopath" bash -c "alias claude='$SANDBOX/localclaude/claude'; shopt -s expand_aliases; $("$CA" init bash)
claude -p x" 2>&1)
  want_has "review: bash with a claude alias still routes through the tool" "ACCT=work" "$out"
  # 9: hostile --path-dir
  printf '' > "$HOME/.zshrc"; "$CA" shell install zsh --path-dir "/tmp/a b/it's \$x" >/dev/null 2>&1
  if command -v zsh >/dev/null; then
    want_has "review: a path with spaces, quotes and \$ lands in PATH intact" "/tmp/a b/it's \$x" \
      "$(ZDOTDIR="$HOME" zsh -ic 'print -r -- "$PATH"' 2>/dev/null)"
  fi
  "$CA" shell uninstall >/dev/null 2>&1
  # 4 + 5: never delete a dir the tool did not create in this call, nor the profiles root
  "$CA" new keepme --no-login >/dev/null; keep_dir=$(profile_json keepme | jq -r .config_dir); "$CA" remove keepme >/dev/null 2>&1
  chmod 500 "$HOME/.config/claude-account"
  fails "review: new --dir fails when the config cannot be written" "$CA" new keepme --dir "$keep_dir" --no-login
  chmod 700 "$HOME/.config/claude-account"
  want "review: ...and the adopted dir survives" "yes" "$([ -d "$keep_dir" ] && echo yes)"
  "$CA" new rootp --dir "$HOME/.local/share/claude-account/profiles" --no-login >/dev/null 2>&1
  "$CA" remove rootp --purge --yes >/dev/null 2>&1
  want "review: the profiles root itself is never purged" "yes" "$([ -d "$CLIENT_DIR" ] && echo yes)"
  # 7: CLAUDE_ACCOUNT_BASE_DIR elsewhere than ~/.claude runs with CLAUDE_CONFIG_DIR set
  mkdir -p "$SANDBOX/altbase" "$SANDBOX/altproj"
  alt() { CLAUDE_ACCOUNT_CONFIG="$SANDBOX/alt.toml" CLAUDE_ACCOUNT_BASE_DIR="$SANDBOX/altbase" "$CA" "$@"; }
  alt new alt --base --no-login >/dev/null 2>&1; alt use alt "$SANDBOX/altproj" >/dev/null 2>&1
  want "review: a base dir other than ~/.claude gets CLAUDE_CONFIG_DIR" "$(canon "$SANDBOX/altbase")" \
    "$(cd "$SANDBOX/altproj" && alt launch -p x | sed -n 's/^CFG=\([^ ]*\) .*/\1/p')"
  # 8: unreachable folders are kept by prune, dropped by prune --all, forgettable by name
  mkdir -p "$HOME/vol/proj"; "$CA" use client "$HOME/vol/proj" >/dev/null 2>&1; rm -rf "$HOME/vol"
  out=$("$CA" prune 2>&1)
  want_has "review: prune keeps a folder whose parent is gone too" "Kept ~/vol/proj" "$out"
  want_has "review: doctor calls it unreachable" "~/vol/proj is not reachable" "$("$CA" doctor 2>&1)"
  "$CA" forget "$HOME/vol/proj" >/dev/null 2>&1
  want "review: forget accepts a mapped folder that no longer exists" "" "$(mapped_to client | grep '/vol/')"
  mkdir -p "$HOME/vol2/proj"; "$CA" use client "$HOME/vol2/proj" >/dev/null 2>&1; rm -rf "$HOME/vol2"
  "$CA" prune --all >/dev/null 2>&1
  want "review: prune --all drops it" "" "$(mapped_to client | grep '/vol2/')"
  # 10: a symlinked config.toml stays a symlink; the header is honest about comments
  mkdir -p "$SANDBOX/dots"; mv "$CONFIG" "$SANDBOX/dots/config.toml"; ln -s "$SANDBOX/dots/config.toml" "$CONFIG"
  "$CA" default client >/dev/null; "$CA" default personal >/dev/null
  want "review: a symlinked config stays a symlink" "$SANDBOX/dots/config.toml" "$(readlink "$CONFIG")"
  want_has "review: ...and its target is updated" 'default = "personal"' "$(cat "$SANDBOX/dots/config.toml")"
  want_has "review: the header says comments are not kept" "comments are not" "$(head -2 "$CONFIG")"
  rm "$CONFIG"; mv "$SANDBOX/dots/config.toml" "$CONFIG"
  # 12: no false "same account" note when claude reports no email
  "$CA" new noemail --no-login >/dev/null
  want_not "review: no false duplicate-account note without an email" "same account" \
    "$(FAKE_NO_EMAIL=1 "$CA" login noemail </dev/null 2>&1)"
  "$CA" remove noemail >/dev/null 2>&1
  "$CA" new tmpy --no-login >/dev/null; mkdir -p "$HOME/tmpy-here"; "$CA" use tmpy "$HOME/tmpy-here" >/dev/null 2>&1
  fails "review: headless remove of a mapped profile needs --force or -y" "$CA" remove tmpy
  "$CA" remove tmpy -y >/dev/null 2>&1
  want "review: -y accepts dropping its mappings" "" "$(profile_json tmpy)"
  # 14: usage errors exit 2
  "$CA" use --no-input >/dev/null 2>&1; rc=$?
  want "review: usage errors exit 2" "2" "$rc"
  # 16: rename then new with the old name
  "$CA" new r1 --no-login >/dev/null; "$CA" rename r1 r2 >/dev/null 2>&1
  want_has "review: new over a renamed profile's dir names its owner" "is the config dir of r2" "$("$CA" new r1 --no-login 2>&1)"
  # 17: purge keeps everything when logout fails
  "$CA" login r2 </dev/null >/dev/null 2>&1; r2_dir=$(profile_json r2 | jq -r .config_dir)
  out=$(FAKE_LOGOUT_FAIL=1 "$CA" remove r2 --purge --yes 2>&1)
  want_has "review: purge stops when logout fails" "nothing was deleted" "$out"
  want "review: ...keeping the profile" "r2" "$(profile_json r2 | jq -r .name)"
  want "review: ...and its dir" "yes" "$([ -d "$r2_dir" ] && echo yes)"
  "$CA" remove r2 --purge --yes >/dev/null 2>&1
  want "review: with logout working it purges" "no" "$([ -d "$r2_dir" ] && echo yes || echo no)"
  # 11b: uninstall --purge deletes only its own files from a custom config dir
  (
    export HOME="$SANDBOX/h11" CLAUDE_ACCOUNT_CONFIG="$SANDBOX/h11/dots/claude-account/config.toml"
    mkdir -p "$HOME/.claude" "$HOME/dots/claude-account"; echo keep > "$HOME/dots/claude-account/notes.txt"
    "$CA" new solo11 --no-login >/dev/null; "$CA" login solo11 </dev/null >/dev/null 2>&1
    FAKE_LOGOUT_FAIL=1 "$CA" uninstall --purge --yes > "$SANDBOX/u11a.log" 2>&1; echo $? > "$SANDBOX/u11a.rc"
    [ -f "$CLAUDE_ACCOUNT_CONFIG" ] && echo kept > "$SANDBOX/u11a.cfg"
    "$CA" uninstall --purge --yes > /dev/null 2>&1
    [ -f "$HOME/dots/claude-account/notes.txt" ] && echo kept > "$SANDBOX/u11.notes"
    [ -f "$CLAUDE_ACCOUNT_CONFIG" ] || echo gone > "$SANDBOX/u11.cfg"
  )
  want "review: uninstall --purge stops when a logout fails" "1" "$(cat "$SANDBOX/u11a.rc")"
  want "review: ...changing nothing" "kept" "$(cat "$SANDBOX/u11a.cfg" 2>/dev/null)"
  want "review: uninstall --purge deletes its config file" "gone" "$(cat "$SANDBOX/u11.cfg" 2>/dev/null)"
  want "review: ...but not other files in that dir" "kept" "$(cat "$SANDBOX/u11.notes" 2>/dev/null)"
fi

if selected release; then
  section "release consistency (scripts/check-release.sh)"
  rel_copy() { rm -rf "$SANDBOX/rc"; mkdir -p "$SANDBOX/rc/scripts" "$SANDBOX/rc/plugin/.claude-plugin"
    cp "$REPO/scripts/check-release.sh" "$SANDBOX/rc/scripts/"; cp "$REPO/Cargo.toml" "$REPO/Cargo.lock" "$REPO/CHANGELOG.md" "$SANDBOX/rc/"
    cp "$REPO/plugin/.claude-plugin/plugin.json" "$SANDBOX/rc/plugin/.claude-plugin/"; }
  v=$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)
  rel_copy; want_has "release: the tree is consistent" "$v consistent" "$(sh "$SANDBOX/rc/scripts/check-release.sh" 2>&1)"
  want_has "release: the matching tag passes" "$v consistent" "$(sh "$SANDBOX/rc/scripts/check-release.sh" "v$v" 2>&1)"
  fails "release: another tag fails" sh "$SANDBOX/rc/scripts/check-release.sh" v0.0.0
  rel_copy; sed -i.bak 's/"version": "[^"]*"/"version": "0.0.0"/' "$SANDBOX/rc/plugin/.claude-plugin/plugin.json"
  fails "release: a plugin version that differs fails" sh "$SANDBOX/rc/scripts/check-release.sh"
  rel_copy; sed -i.bak "s/^## \[$v\]/## [old]/" "$SANDBOX/rc/CHANGELOG.md"
  fails "release: a version without a changelog entry fails" sh "$SANDBOX/rc/scripts/check-release.sh"
  rel_copy; sed -i.bak "1,/^version = /s/^version = \".*\"/version = \"9.9.9\"/" "$SANDBOX/rc/Cargo.toml"
  fails "release: a Cargo.lock out of date fails" sh "$SANDBOX/rc/scripts/check-release.sh"
fi

if selected update; then
  section "updates"
  bump() { echo "$CURRENT_V" | awk -F. -v k="$1" '{ if (k=="major") print $1+1".0.0"; else if (k=="minor") print $1"."$2+1".0"; else print $1"."$2"."$3+1 }'; }
  cache="$HOME/.cache/claude-account/update.json"
  put_cache() { mkdir -p "$(dirname "$cache")"; printf '{"checked":%s,"latest":"%s"}' "$(date +%s)" "$1" > "$cache"; }
  want_has "update: doctor says when it is up to date" "is the latest stable release" "$("$CA" doctor 2>&1)"
  latest_is "$(bump minor)"
  want_has "update: doctor names a newer stable release" "$(bump minor) is available" "$("$CA" doctor 2>&1)"
  want_has "update: ...and the command to get it" "claude-account update" "$("$CA" doctor 2>&1)"
  latest_is "$CURRENT_V"
  if command -v expect >/dev/null; then
    (
      unset CI
      put_cache "$(bump patch)";  pty_in "$HOME" "$CA" list -- > "$SANDBOX/n1.log"
      put_cache "$(bump minor)";  pty_in "$HOME" "$CA" list -- > "$SANDBOX/n2.log"
      put_cache "$(bump major)";  pty_in "$HOME" "$CA" list -- > "$SANDBOX/n3.log"
      pty_in "$HOME" "$CA" list --json -- > "$SANDBOX/n4.log"
      CLAUDE_ACCOUNT_NO_UPDATE_CHECK=1 pty_in "$HOME" "$CA" list -- > "$SANDBOX/n5.log"
      put_cache "$CURRENT_V";     pty_in "$HOME" "$CA" list -- > "$SANDBOX/n6.log"
      # A day-old cache is refreshed in the background, for the next command.
      latest_is "$(bump minor)"
      printf '{"checked":1,"latest":"%s"}' "$CURRENT_V" > "$cache"
      pty_in "$HOME" "$CA" list -- > /dev/null
      for _ in 1 2 3 4 5 6 7 8 9 10; do grep -q "$(bump minor)" "$cache" && break; sleep 0.3; done
      cat "$cache" > "$SANDBOX/n7.cache"
      latest_is "$CURRENT_V"
    )
    plain() { sed 's/\x1b\[[0-9;]*m//g' "$1"; }
    want_has "update: a patch release is an info notice" "info: claude-account $(bump patch) is available" "$(plain "$SANDBOX/n1.log")"
    want_has "update: a minor release is a warning" "warning: claude-account $(bump minor) is available" "$(plain "$SANDBOX/n2.log")"
    want_has "update: a major release is flagged as danger" "danger: claude-account $(bump major) is available" "$(plain "$SANDBOX/n3.log")"
    want "update: the notice comes last" "danger:" "$(grep -v '^<<exit' "$SANDBOX/n3.log" | sed 's/\x1b\[[0-9;]*m//g' | grep -v '^\s*$' | tail -1 | cut -d' ' -f1)"
    want_not "update: no notice with --json" "is available" "$(cat "$SANDBOX/n4.log")"
    want_not "update: no notice when switched off" "is available" "$(cat "$SANDBOX/n5.log")"
    want_not "update: no notice when up to date" "is available" "$(cat "$SANDBOX/n6.log")"
    want_has "update: a stale cache is refreshed in the background" "$(bump minor)" "$(cat "$SANDBOX/n7.cache")"
  else skip "update: notices" "expect not installed"; fi
  put_cache "$(bump major)"
  want_not "update: no notice without a terminal (scripts, CI)" "is available" "$("$CA" list 2>&1)"
  rm -f "$cache"
  want_has "update: a development build points to git" "development build" "$("$CA" update 2>&1)"
  # update runs the real installer into its own folder
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) T=aarch64-apple-darwin ;; Darwin-x86_64) T=x86_64-apple-darwin ;;
    Linux-x86_64) T=x86_64-unknown-linux-musl ;; Linux-aarch64) T=aarch64-unknown-linux-musl ;; *) T="" ;;
  esac
  if [ -n "$T" ]; then
    UREL="$SANDBOX/urel"; mkdir -p "$UREL/pkg" "$SANDBOX/uhome/tools"; cp "$CA" "$UREL/pkg/claude-account"
    tar -czf "$UREL/claude-account-$T.tar.gz" -C "$UREL/pkg" claude-account
    (cd "$UREL" && shasum -a 256 "claude-account-$T.tar.gz" > "claude-account-$T.tar.gz.sha256")
    cp "$CA" "$SANDBOX/uhome/tools/claude-account"
    out=$(HOME="$SANDBOX/uhome" "$SANDBOX/uhome/tools/claude-account" update 2>&1)
    want_has "update: update when current says so and does nothing" "is the latest stable release" "$out"
    latest_is "$(bump minor)"
    out=$(HOME="$SANDBOX/uhome" CLAUDE_ACCOUNT_INSTALLER_URL="file://$REPO/install.sh" CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$UREL" \
      "$SANDBOX/uhome/tools/claude-account" update 2>&1); rc=$?
    latest_is "$CURRENT_V"
    want "update: update runs the official installer" "0" "$rc"
    want_has "update: ...into the binary's own folder" "claude-account $CURRENT_V in ~/tools" "$out"
  else skip "update: update command" "no asset naming for this platform"; fi
fi

if selected matrix; then
  section "every command: help, headless, json"
  # Run with a 5 s limit and no terminal; prints "<exit code>" (124 on hang).
  bounded() { ( "$@" </dev/null >"$SANDBOX/m.out" 2>"$SANDBOX/m.err" ) & local pid=$! i
    for i in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    if kill -0 "$pid" 2>/dev/null; then kill "$pid"; wait "$pid" 2>/dev/null; echo 124; else wait "$pid"; echo $?; fi; }
  cmds=$("$CA" --help | awk '/^Commands:/{f=1; next} /^$/{f=0} f {print $1}' | grep -v '^help$')
  want_has "matrix: the command list is read from --help" "setup" "$cmds"
  mkdir -p "$HOME/mx"
  : > "$SANDBOX/matrix.txt"
  for c in $cmds; do
    rc=$(bounded "$CA" "$c" --help)
    if [ "$rc" = 0 ] && grep -q '^Usage:' "$SANDBOX/m.out"; then ok "matrix: $c --help"; else bad "matrix: $c --help" "rc=$rc"; fi
    case "$c" in
      # These wait for input on stdin, run claude, or change the install: covered elsewhere.
      launch|run|statusline|hook|init|completions|update|uninstall|refresh-update-cache) continue ;;
    esac
    rc=$(cd "$HOME/mx" && bounded "$CA" --no-input "$c")
    printf '%-10s rc=%s  %s\n' "$c" "$rc" "$(head -1 "$SANDBOX/m.err" | sed 's/\x1b\[[0-9;]*m//g')" >> "$SANDBOX/matrix.txt"
    case "$rc" in
      0) ok "matrix: $c headless without arguments ends (0)" ;;
      2) if grep -q -E 'usage:|^Usage:' "$SANDBOX/m.err"; then ok "matrix: $c headless without arguments is a usage error naming what to pass"
         else bad "matrix: $c headless usage error" "$(cat "$SANDBOX/m.err")"; fi ;;
      124) bad "matrix: $c headless without arguments hangs" "timed out" ;;
      *) bad "matrix: $c headless without arguments" "rc=$rc $(cat "$SANDBOX/m.err")" ;;
    esac
  done
  for c in status resolve map list default doctor "shell status"; do
    # shellcheck disable=SC2086
    rc=$(bounded "$CA" $c --json)
    if [ "$rc" -le 1 ] && jq -e . "$SANDBOX/m.out" >/dev/null 2>&1; then ok "matrix: $c --json is valid JSON"
    else bad "matrix: $c --json" "rc=$rc $(head -c 200 "$SANDBOX/m.out")"; fi
  done
  cp "$SANDBOX/matrix.txt" "${MATRIX_OUT:-/dev/null}" 2>/dev/null || true
fi

if selected integration; then
  section "status line and hook"
  want "integration: statusline shows the session's profile" "work" \
    "$(echo "{\"workspace\":{\"current_dir\":\"$W\"}}" | CLAUDE_ACCOUNT=work "$CA" statusline)"
  want "integration: statusline flags a mismatch" "work (here: client)" \
    "$(echo "{\"workspace\":{\"current_dir\":\"$W/client-proj\"}}" | CLAUDE_ACCOUNT=work "$CA" statusline)"
  want "integration: statusline infers the profile without CLAUDE_ACCOUNT" "client" \
    "$(echo "{\"cwd\":\"$W/client-proj\"}" | CLAUDE_CONFIG_DIR="$CLIENT_DIR" "$CA" statusline)"
  want "integration: an alias is inferred from the map" "personal" "$(echo "{\"cwd\":\"$HOME/personal\"}" | "$CA" statusline)"
  want "integration: hook is silent when session and map agree" "" \
    "$(echo "{\"cwd\":\"$W\"}" | CLAUDE_ACCOUNT=work "$CA" hook session-start)"
  want_has "integration: hook tells Claude about a mismatch" 'runs as profile "work"' \
    "$(echo "{\"cwd\":\"$W/client-proj\"}" | CLAUDE_ACCOUNT=work "$CA" hook session-start)"
  mkdir -p "$W/sess-a" "$HOME/sess-b"
  out=$(cd "$W/sess-a" && CLAUDECODE=1 CLAUDE_ACCOUNT=work "$CA" use client "$HOME/sess-b" 2>&1)
  want_not "integration: mapping another folder from a session says nothing about the session" "This session" "$out"
  out=$(cd "$W/sess-a" && CLAUDECODE=1 CLAUDE_ACCOUNT=work "$CA" use client 2>&1)
  want_has "integration: remapping the session's own folder says how to switch" "resume it as client" "$out"
  "$CA" forget "$W/sess-a" >/dev/null
  want_has "integration: completions" "claude-account" "$("$CA" completions zsh | head -3)"
fi

if selected picker; then
  section "picker and interactive mode (real pty)"
  if ! command -v expect >/dev/null; then skip "picker" "expect not installed"
  else
    mkdir -p "$HOME/new1" "$HOME/new2" "$HOME/new3" "$HOME/new5"
    out=$(picker_in "$HOME/new1" ENTER)
    want_has "picker: shown in an unmapped directory" "Claude Code account for ~/new1" "$out"
    want_has "picker: Enter takes the default" "ACCT=personal" "$out"
    want "picker: the choice is remembered" "personal" "$("$CA" resolve "$HOME/new1")"
    want_not "picker: not shown once remembered" "account for" "$(pty_in "$HOME/new1" "$CA" launch --)"
    want_has "picker: typing filters, Enter takes the match" "ACCT=client" "$(picker_in "$HOME/new2" c l i ENTER)"
    want_has "picker: arrows move the selection" "ACCT=work" "$(picker_in "$HOME/new3" DOWN DOWN ENTER)"
    out=$(picker_in "$HOME/new5" ESC)
    want_has "picker: Esc backs out with 130" "<<exit 130>>" "$out"
    want_not "picker: ...without launching claude" "ACCT=" "$out"
    want "picker: ...and remembers nothing" "" "$("$CA" resolve "$HOME/new5")"
    git -C "$HOME" init -q proj2 && git -C "$HOME/proj2" commit -q --allow-empty -m i && mkdir -p "$HOME/proj2/a/b"
    picker_in "$HOME/proj2/a/b" c l i e n t ENTER >/dev/null
    want "picker: remembers the repo root, not the subdir" "$(canon "$HOME/proj2")" "$(mapped_to client | grep proj2)"
    before=$(map_json)
    want_has "picker: never remembers HOME" "Not remembered" "$(picker_in "$HOME" ENTER)"
    want "picker: ...and the map is unchanged" "$before" "$(map_json)"
    want_not "picker: not shown for claude -p" "account for" "$(pty_in "$HOME/new5" "$CA" launch -p x --)"
    mkdir -p "$HOME/sw"
    out=$(pty_in "$HOME/sw" "$CA" -- DOWN DOWN ENTER)
    want_has "interactive: bare command switches the project" "~/sw -> work" "$out"
    want "interactive: ...and remembers it" "work" "$("$CA" resolve "$HOME/sw")"
    want_has "interactive: Enter keeps the current profile" "Unchanged: work" "$(pty_in "$HOME/sw" "$CA" -- ENTER)"
    want_has "interactive: use without a profile asks" "~/sw -> client" "$(pty_in "$HOME/sw" "$CA" use -- c l i e n t ENTER)"
    out=$(pty_in "$HOME" "$CA" new -- s o l o ENTER DOWN ENTER w o r k ENTER)
    want_has "interactive: new asks the name and the kind" "Profile solo: work@example.com, same login as work" "$out"
    out=$(pty_in "$HOME" "$CA" new -- b a d ' ' ENTER)
    want_has "interactive: new validates the name as you type" "lowercase letters" "$out"
    out=$(pty_in "$HOME" "$CA" new -- f r e s h ENTER ENTER ENTER)
    want_has "interactive: a new own profile offers to log in and verifies it" "fresh is logged in as fresh@example.com" "$out"
    FAKE_LOGIN_EMAIL=client@example.com "$CA" login client </dev/null >/dev/null 2>&1
    out=$(pty_in "$HOME" "$CA" login client -- ENTER)
    want_has "interactive: login asks before replacing a login" "already logged in as client@example.com" "$out"
    want_not "interactive: ...and keeps it by default" "Login successful" "$out"
    before=$(map_json)
    out=$(pty_in "$HOME" "$CA" -- ENTER ENTER)
    want_has "interactive: switching in HOME asks first" "holds all your folders" "$out"
    want "interactive: ...and changes nothing by default" "$before" "$(map_json)"
    mkdir -p "$HOME/sub1"
    out=$(pty_in "$HOME/sub1" "$CA" launch update --)
    want_not "picker: not shown for claude subcommands (read from claude --help)" "account for" "$out"
    want_has "picker: ...the subcommand runs" "ARGS=update" "$out"
    want "picker: ...and nothing is remembered" "" "$("$CA" resolve "$HOME/sub1")"
    out=$(pty_in "$HOME/sub1" "$CA" launch "fix the bug" -- ESC)
    want_has "picker: still shown for a prompt argument" "account for" "$out"
    "$CA" new tmpx --no-login >/dev/null; tmpx_dir=$(profile_json tmpx | jq -r .config_dir)
    out=$(pty_in "$HOME" "$CA" remove -- t m p x ENTER ENTER y ENTER)
    want_has "interactive: remove picks, offers purge (default no) and confirms" "Removed tmpx" "$out"
    want "interactive: ...keeping the folder when purge was declined" "yes" "$([ -d "$tmpx_dir" ] && echo yes)"
    "$CA" new tmpz --no-login >/dev/null
    out=$(pty_in "$HOME" "$CA" remove tmpz -- ENTER ENTER)
    want_has "interactive: remove defaults to no" "<<exit 130>>" "$out"
    want "interactive: ...and keeps the profile" "tmpz" "$(profile_json tmpz | jq -r .name)"
    out=$(pty_in "$HOME" "$CA" rename -- t m p z ENTER t m p w ENTER)
    want_has "interactive: rename asks which and the new name" "Renamed tmpz to tmpw" "$out"
    out=$(pty_in "$HOME" "$CA" default -- ENTER)
    want_has "interactive: default opens on the current one; Enter keeps it" "Default unchanged: personal" "$out"
    out=$(pty_in "$HOME/sw" "$CA" run -- c l i e n t ENTER)
    want_has "interactive: run without a profile asks" "ACCT=client" "$out"
    mv "$CLIENT_DIR/settings.json" "$SANDBOX/s2.bak"
    out=$(pty_in "$HOME" "$CA" doctor -- ENTER)
    want_has "interactive: doctor offers to fix what it can" "Fix 1 of these now" "$out"
    want "interactive: ...and Enter fixes it" "$HOME/.claude/settings.json" "$(readlink "$CLIENT_DIR/settings.json")"
    mkdir -p "$HOME/gone-a"; "$CA" use client "$HOME/gone-a" >/dev/null 2>&1; rmdir "$HOME/gone-a"
    out=$(pty_in "$HOME" "$CA" prune -- ENTER)
    want_has "interactive: prune lists and confirms" "Dropped ~/gone-a" "$out"
    : > "$SANDBOX/shx.rc"
    out=$(SHELL=/bin/unknown ZDOTDIR="$SANDBOX/shxdir" pty_in "$HOME" "$CA" shell install -- ENTER)
    want_has "interactive: shell install asks the shell when \$SHELL says nothing" "Shell integration added to" "$out"
    "$CA" shell uninstall >/dev/null 2>&1
    cp "$CA" "$SANDBOX/ubin-claude-account"
    out=$(pty_in "$HOME" "$SANDBOX/ubin-claude-account" uninstall -- ENTER)
    want_has "interactive: uninstall shows the plan" "delete this binary" "$out"
    want "interactive: ...and defaults to no" "yes" "$([ -x "$SANDBOX/ubin-claude-account" ] && echo yes)"
    out=$(pty_in "$HOME" "$CA" login newacct -- ENTER ENTER)
    want_has "interactive: login of an unknown profile offers to create it" "There is no profile newacct" "$out"
    want_has "interactive: ...creates it and logs it in" "newacct is logged in as newacct@example.com" "$out"
    out=$(pty_in "$HOME" "$CA" login -- + ENTER v i a ENTER ENTER)
    want_has "interactive: login's picker offers a new profile" "via is logged in as via@example.com" "$out"
    mkdir -p "$HOME/new6"
    out=$(picker_in "$HOME/new6" + ENTER c l i 2 ENTER ENTER ENTER)
    want_has "picker: + New profile creates, logs in and launches" "ACCT=cli2" "$out"
    want_has "picker: ...after logging it in" "cli2 is logged in as cli2@example.com" "$out"
    want "picker: ...and remembers it" "cli2" "$("$CA" resolve "$HOME/new6")"
  fi
fi

if selected setup; then
  section "setup wizard (real pty, fresh HOME)"
  if ! command -v expect >/dev/null; then skip "setup" "expect not installed"
  else
    (
      export HOME="$SANDBOX/fresh"
      mkdir -p "$HOME/.claude" "$HOME/proj"
      echo main@example.com > "$HOME/.claude/.fake-login"
      echo '{"oauthAccount":{"emailAddress":"main@example.com"}}' > "$HOME/.claude.json"
      touch "$HOME/.zshrc"
      out=$(pty_in "$HOME" "$CA" setup -- m a i n ENTER ENTER s i d e ENTER ENTER ENTER ENTER ENTER '~/proj' ENTER s i d e ENTER ENTER ENTER)
      printf '%s\n' "$out" > "$SANDBOX/setup.log"
      "$CA" list --json > "$SANDBOX/setup.json"
      "$CA" resolve "$HOME/proj" > "$SANDBOX/setup.resolve"
      grep -c '>>> claude-account >>>' "$HOME/.zshrc" > "$SANDBOX/setup.rc" || true
    )
    out=$(cat "$SANDBOX/setup.log")
    want_has "setup: detects the existing login" "Claude Code is logged in as main@example.com" "$out"
    want "setup: names it and adds a second account" "main,side" "$(jq -r '[.[].name] | sort | join(",")' "$SANDBOX/setup.json")"
    want "setup: the existing login stays in ~/.claude" "true" "$(jq -r '.[] | select(.name == "main") | .base' "$SANDBOX/setup.json")"
    want_has "setup: logs the new account in and verifies it" "side is logged in as side@example.com" "$out"
    want "setup: maps a folder" "side" "$(cat "$SANDBOX/setup.resolve")"
    want "setup: installs the shell integration" "1" "$(cat "$SANDBOX/setup.rc")"
    want_has "setup: ends with next steps" "Open a new terminal" "$out"
    (
      export HOME="$SANDBOX/fresh2"
      mkdir -p "$HOME/.claude"; echo '{}' > "$HOME/.claude.json"
      "$CA" new taken --no-login >/dev/null
      pty_in "$HOME" "$CA" setup -- t a k e n ENTER ESC > "$SANDBOX/setup2.log"
    )
    want_has "setup: a taken name is asked again, not fatal" "taken already exists" "$(cat "$SANDBOX/setup2.log")"
  fi
fi

if selected install; then
  section "installer and uninstaller"
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) T=aarch64-apple-darwin ;; Darwin-x86_64) T=x86_64-apple-darwin ;;
    Linux-x86_64) T=x86_64-unknown-linux-musl ;; Linux-aarch64) T=aarch64-unknown-linux-musl ;; *) T="" ;;
  esac
  if [ -z "$T" ]; then skip "install" "no asset naming for this platform"
  else
    REL="$SANDBOX/release"; mkdir -p "$REL/pkg"
    cp "$CA" "$REL/pkg/claude-account"
    tar -czf "$REL/claude-account-$T.tar.gz" -C "$REL/pkg" claude-account
    (cd "$REL" && shasum -a 256 "claude-account-$T.tar.gz" > "claude-account-$T.tar.gz.sha256")
    (
      export HOME="$SANDBOX/inst" CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$REL" PATH="$SANDBOX/bin:/usr/bin:/bin:/usr/sbin:/sbin"
      mkdir -p "$HOME"; printf 'export MINE=1\n' > "$HOME/.zshrc"
      sh "$REPO/install.sh" > "$SANDBOX/i1.log" 2>&1; echo $? > "$SANDBOX/i1.rc"
      sh "$REPO/install.sh" > "$SANDBOX/i2.log" 2>&1
      printf '#!/bin/sh\necho "claude-account 0.0.1"\n' > "$HOME/.local/bin/claude-account"
      sh "$REPO/install.sh" > "$SANDBOX/iup.log" 2>&1
      printf '#!/bin/sh\necho "claude-account 99.0.0"\n' > "$HOME/.local/bin/claude-account"
      sh "$REPO/install.sh" > "$SANDBOX/idown.log" 2>&1
      printf '#!/bin/sh\necho "claude-account %s-rc.1"\n' "$("$CA" --version | cut -d' ' -f2)" > "$HOME/.local/bin/claude-account"
      sh "$REPO/install.sh" > "$SANDBOX/ipre.log" 2>&1
      grep -c '>>> claude-account >>>' "$HOME/.zshrc" > "$SANDBOX/i.blocks"
      cp "$HOME/.zshrc" "$SANDBOX/i.zshrc"
      (cd "$HOME" && ZDOTDIR="$HOME" zsh -ic 'whence -w claude; claude-account --version' > "$SANDBOX/i.shell" 2>&1)
      mkdir -p "$HOME/.claude"
      "$HOME/.local/bin/claude-account" new me --base --no-login >/dev/null
      sh "$REPO/install.sh" > "$SANDBOX/upgrade.log" 2>&1
      sh "$REPO/install.sh" --uninstall > "$SANDBOX/u1.log" 2>&1; echo $? > "$SANDBOX/u1.rc"
      [ -e "$HOME/.local/bin/claude-account" ] && echo yes > "$SANDBOX/u1.bin" || echo no > "$SANDBOX/u1.bin"
      cp "$HOME/.zshrc" "$SANDBOX/u1.zshrc"
      [ -f "$HOME/.config/claude-account/config.toml" ] && echo kept > "$SANDBOX/u1.cfg" || echo gone > "$SANDBOX/u1.cfg"
      sh "$REPO/install.sh" --no-modify-rc > "$SANDBOX/i3.log" 2>&1
      cp "$HOME/.zshrc" "$SANDBOX/i3.zshrc"
      sh "$REPO/install.sh" --uninstall --purge > "$SANDBOX/u2.log" 2>&1; echo $? > "$SANDBOX/u2.rc"
      [ -e "$HOME/.config/claude-account" ] && echo kept > "$SANDBOX/u2.cfg" || echo gone > "$SANDBOX/u2.cfg"
      [ -d "$HOME/.claude" ] || mkdir -p "$HOME/.claude"
      sh "$REPO/install.sh" --uninstall > "$SANDBOX/u3.log" 2>&1; echo $? > "$SANDBOX/u3.rc"
      echo "0000  claude-account-$T.tar.gz" > "$REL/bad.sha256"
      mkdir -p "$SANDBOX/badrel"; cp "$REL/claude-account-$T.tar.gz" "$SANDBOX/badrel/"; cp "$REL/bad.sha256" "$SANDBOX/badrel/claude-account-$T.tar.gz.sha256"
      CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$SANDBOX/badrel" sh "$REPO/install.sh" > "$SANDBOX/bad.log" 2>&1; echo $? > "$SANDBOX/bad.rc"
      [ -e "$HOME/.local/bin/claude-account" ] && echo yes > "$SANDBOX/bad.bin" || echo no > "$SANDBOX/bad.bin"
      CLAUDE_ACCOUNT_TARGET=x86_64-pc-windows-msvc sh "$REPO/install.sh" > "$SANDBOX/win.log" 2>&1; echo $? > "$SANDBOX/win.rc"
      sh "$REPO/install.sh" --purge > "$SANDBOX/purgeonly.log" 2>&1; echo $? > "$SANDBOX/purgeonly.rc"
      sh "$REPO/install.sh" --bogus > "$SANDBOX/bogus.log" 2>&1; echo $? > "$SANDBOX/bogus.rc"
    )
    want "install: exits 0" "0" "$(cat "$SANDBOX/i1.rc")"
    want_has "install: reports the version" "Installed claude-account" "$(cat "$SANDBOX/i1.log")"
    want_has "install: puts ~/.local/bin on PATH in the block" ".local/bin'" "$(cat "$SANDBOX/i.zshrc")"
    want "install: running twice leaves one block" "1" "$(cat "$SANDBOX/i.blocks")"
    want_has "install: the same version again is a reinstall" "reinstall claude-account" "$(cat "$SANDBOX/i2.log")"
    want_has "install: an older installed version is an upgrade" "upgrade claude-account 0.0.1 ->" "$(cat "$SANDBOX/iup.log")"
    want_has "install: ...and says so when done" "Upgraded claude-account 0.0.1 ->" "$(cat "$SANDBOX/iup.log")"
    want_has "install: a newer installed version is a downgrade" "downgrade claude-account 99.0.0 ->" "$(cat "$SANDBOX/idown.log")"
    want_has "install: the release after its own pre-release is an upgrade" "upgrade claude-account $CURRENT_V-rc.1 -> $CURRENT_V" "$(cat "$SANDBOX/ipre.log")"
    want_has "install: keeps the user's rc lines" "export MINE=1" "$(cat "$SANDBOX/i.zshrc")"
    want_has "install: a new zsh gets the claude function" "claude: function" "$(cat "$SANDBOX/i.shell")"
    want_has "install: ...and finds claude-account on PATH" "claude-account 0." "$(cat "$SANDBOX/i.shell")"
    want_has "install: tells what to do next" "claude-account setup" "$(cat "$SANDBOX/i1.log")"
    want_has "install: an upgrade keeps profiles and says so" "profiles and mappings are unchanged" "$(cat "$SANDBOX/upgrade.log")"
    want_not "install: ...without suggesting setup again" "claude-account setup" "$(cat "$SANDBOX/upgrade.log")"
    want "uninstall: exits 0" "0" "$(cat "$SANDBOX/u1.rc")"
    want "uninstall: deletes the binary" "no" "$(cat "$SANDBOX/u1.bin")"
    want "uninstall: restores the rc file" "export MINE=1" "$(cat "$SANDBOX/u1.zshrc")"
    want "uninstall: keeps profiles without --purge" "kept" "$(cat "$SANDBOX/u1.cfg")"
    want "install: --no-modify-rc leaves the rc alone" "export MINE=1" "$(cat "$SANDBOX/i3.zshrc")"
    want "uninstall --purge: exits 0" "0" "$(cat "$SANDBOX/u2.rc")"
    want "uninstall --purge: deletes the configuration" "gone" "$(cat "$SANDBOX/u2.cfg")"
    want "uninstall: with nothing installed is a no-op" "0" "$(cat "$SANDBOX/u3.rc")"
    want "install: a checksum mismatch fails" "1" "$(cat "$SANDBOX/bad.rc")"
    want_has "install: ...says so, and why it matters" "does not match its checksum (corrupted or tampered with)" "$(cat "$SANDBOX/bad.log")"
    want "install: ...and installs nothing" "no" "$(cat "$SANDBOX/bad.bin")"
    want "install: a target without a release fails" "1" "$(cat "$SANDBOX/win.rc")"
    want "install: unknown options fail" "1" "$(cat "$SANDBOX/bogus.rc")"
    want "install: --purge without --uninstall fails" "1" "$(cat "$SANDBOX/purgeonly.rc")"
    # Environment detection, dependencies, safe failure, convergence.
    TOOLS="sh uname tar gzip mktemp mkdir cp mv chmod rm dirname sed tr cut sort head tail cat id sysctl sw_vers grep curl shasum"
    mkbin() { local d="$1" t p; shift; rm -rf "$d"; mkdir -p "$d"
      for t in "$@"; do p=$(command -v "$t" 2>/dev/null) && ln -s "$p" "$d/$t"; done; }
    without() { local skip=" $1 " t out=""; for t in $TOOLS; do case "$skip" in *" $t "*) ;; *) out="$out $t" ;; esac; done; echo "$out"; }
    (
      export HOME="$SANDBOX/inst3" CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$REL"
      mkdir -p "$HOME"; : > "$HOME/.zshrc"
      # shellcheck disable=SC2046
      mkbin "$SANDBOX/mb1" $(without "tar gzip curl")
      PATH="$SANDBOX/mb1" /bin/sh "$REPO/install.sh" > "$SANDBOX/d1.log" 2>&1; echo $? > "$SANDBOX/d1.rc"
      if command -v openssl >/dev/null; then
        mkbin "$SANDBOX/mb2" $(without "shasum") openssl
        PATH="$SANDBOX/mb2:$SANDBOX/bin" /bin/sh "$REPO/install.sh" > "$SANDBOX/d2.log" 2>&1; echo $? > "$SANDBOX/d2.rc"
        rm -rf "$HOME/.local"; : > "$HOME/.zshrc"
      fi
      mkbin "$SANDBOX/mb3" $TOOLS; rm "$SANDBOX/mb3/id"; printf '#!/bin/sh\necho 0\n' > "$SANDBOX/mb3/id"; chmod +x "$SANDBOX/mb3/id"
      SUDO_USER=someone PATH="$SANDBOX/mb3" /bin/sh "$REPO/install.sh" > "$SANDBOX/d3.log" 2>&1; echo $? > "$SANDBOX/d3.rc"
      mkdir -p "$HOME/ro"; chmod 500 "$HOME/ro"
      sh "$REPO/install.sh" --bin-dir "$HOME/ro/bin" > "$SANDBOX/d4.log" 2>&1; echo $? > "$SANDBOX/d4.rc"; chmod 700 "$HOME/ro"
      CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$SANDBOX/no-such-release" sh "$REPO/install.sh" > "$SANDBOX/d5.log" 2>&1; echo $? > "$SANDBOX/d5.rc"
      CLAUDE_ACCOUNT_DOWNLOAD_URL="https://127.0.0.1:9" sh "$REPO/install.sh" > "$SANDBOX/d6.log" 2>&1; echo $? > "$SANDBOX/d6.rc"
      mkbin "$SANDBOX/mb7" $TOOLS; rm "$SANDBOX/mb7/uname"; printf '#!/bin/sh\n[ "$1" = -s ] && echo MINGW64_NT-10.0 || echo x86_64\n' > "$SANDBOX/mb7/uname"; chmod +x "$SANDBOX/mb7/uname"
      PATH="$SANDBOX/mb7" /bin/sh "$REPO/install.sh" > "$SANDBOX/d7.log" 2>&1; echo $? > "$SANDBOX/d7.rc"
      [ -e "$HOME/.local/bin/claude-account" ] && echo yes > "$SANDBOX/d.none" || echo no > "$SANDBOX/d.none"
      cp "$HOME/.zshrc" "$SANDBOX/d.zshrc"
      # no claude on PATH: installs, and says it is needed
      mkbin "$SANDBOX/mb8" $TOOLS
      PATH="$SANDBOX/mb8" /bin/sh "$REPO/install.sh" > "$SANDBOX/d8.log" 2>&1; echo $? > "$SANDBOX/d8.rc"
      # convergence: a second run leaves identical files
      cp "$HOME/.zshrc" "$SANDBOX/c1.zshrc"; cp "$HOME/.local/bin/claude-account" "$SANDBOX/c1.bin"
      PATH="$SANDBOX/mb8" /bin/sh "$REPO/install.sh" > /dev/null 2>&1
      cmp "$HOME/.zshrc" "$SANDBOX/c1.zshrc" > "$SANDBOX/c.rc.cmp" 2>&1; cmp "$HOME/.local/bin/claude-account" "$SANDBOX/c1.bin" > "$SANDBOX/c.bin.cmp" 2>&1
      # an interrupted run (binary gone, a half-copied leftover): run again, done
      rm "$HOME/.local/bin/claude-account"; echo partial > "$HOME/.local/bin/.claude-account.new"
      PATH="$SANDBOX/mb8" /bin/sh "$REPO/install.sh" > "$SANDBOX/c2.log" 2>&1; echo $? > "$SANDBOX/c2.rc"
      ls -A "$HOME/.local/bin" | tr '\n' ' ' > "$SANDBOX/c2.ls"; grep -c '>>> claude-account >>>' "$HOME/.zshrc" > "$SANDBOX/c2.blocks"
      # another copy first on PATH
      mkdir -p "$SANDBOX/shadow"; cp "$CA" "$SANDBOX/shadow/claude-account"
      PATH="$SANDBOX/shadow:$SANDBOX/mb8" /bin/sh "$REPO/install.sh" > "$SANDBOX/c3.log" 2>&1
    )
    want "install: missing tools fail" "1" "$(cat "$SANDBOX/d1.rc")"
    want_has "install: ...naming all of them at once" "missing tools: tar gzip curl" "$(cat "$SANDBOX/d1.log")"
    want_has "install: ...with how to get them" "then run the installer again" "$(cat "$SANDBOX/d1.log")"
    if [ -f "$SANDBOX/d2.rc" ]; then
      want "install: openssl stands in for shasum" "0" "$(cat "$SANDBOX/d2.rc")"
    else skip "install: openssl fallback" "openssl not installed"; fi
    want_has "install: refuses sudo" "do not run the installer with sudo" "$(cat "$SANDBOX/d3.log")"
    want_has "install: an unwritable folder is caught before anything" "cannot write to ~/ro" "$(cat "$SANDBOX/d4.log")"
    want_has "install: ...with the way out" "--bin-dir" "$(cat "$SANDBOX/d4.log")"
    want_has "install: a missing release is explained" "not found:" "$(cat "$SANDBOX/d5.log")"
    want_has "install: ...pointing at the releases" "releases" "$(cat "$SANDBOX/d5.log")"
    want_has "install: a network failure is explained" "could not reach https://127.0.0.1:9" "$(cat "$SANDBOX/d6.log")"
    want_has "install: Windows shells are pointed to WSL" "use WSL" "$(cat "$SANDBOX/d7.log")"
    for i in 1 3 4 5 6 7; do
      want_has "install: failure $i says nothing was changed" "Nothing was changed." "$(cat "$SANDBOX/d$i.log")"
    done
    want "install: ...and really changed nothing (binary)" "no" "$(cat "$SANDBOX/d.none")"
    want "install: ...and really changed nothing (rc file)" "" "$(cat "$SANDBOX/d.zshrc")"
    want "install: without Claude Code it still installs" "0" "$(cat "$SANDBOX/d8.rc")"
    want_has "install: ...shows what it found" "Claude Code    not found" "$(cat "$SANDBOX/d8.log")"
    want_has "install: ...and puts installing it in the next steps" "Install Claude Code:" "$(cat "$SANDBOX/d8.log")"
    want_has "install: the state shows the system and target" "System " "$(cat "$SANDBOX/d8.log")"
    want "install: a second run leaves the rc file byte-identical" "" "$(cat "$SANDBOX/c.rc.cmp")"
    want "install: ...and the binary byte-identical" "" "$(cat "$SANDBOX/c.bin.cmp")"
    want "install: an interrupted install finishes by running it again" "0" "$(cat "$SANDBOX/c2.rc")"
    want "install: ...leaving only the binary" "claude-account " "$(cat "$SANDBOX/c2.ls")"
    want "install: ...and one shell block" "1" "$(cat "$SANDBOX/c2.blocks")"
    want_has "install: warns when another copy wins on PATH" "comes before ~/.local/bin on your PATH" "$(cat "$SANDBOX/c3.log")"
    if command -v expect >/dev/null; then
      (
        unset CI
        export HOME="$SANDBOX/inst2" CLAUDE_ACCOUNT_DOWNLOAD_URL="file://$REL" PATH="$SANDBOX/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        mkdir -p "$HOME/.claude"; printf 'export MINE=1\n' > "$HOME/.zshrc"
        pty_in "$HOME" sh -c "cat '$REPO/install.sh' | sh" -- 3 ENTER > "$SANDBOX/ii0.log"
        [ -e "$HOME/.local/bin/claude-account" ] && echo yes > "$SANDBOX/ii0.bin" || echo no > "$SANDBOX/ii0.bin"
        pty_in "$HOME" sh -c "cat '$REPO/install.sh' | sh" -- ENTER n ENTER > "$SANDBOX/ii1.log"
        pty_in "$HOME" sh -c "cat '$REPO/install.sh' | sh" -- ENTER > "$SANDBOX/ii2.log"
        pty_in "$HOME" sh -c "cat '$REPO/install.sh' | sh" -- 2 ENTER '~/bin2' ENTER n ENTER 1 ENTER n ENTER > "$SANDBOX/ii3.log"
        [ -x "$HOME/bin2/claude-account" ] && echo yes > "$SANDBOX/ii3.bin" || echo no > "$SANDBOX/ii3.bin"
        pty_in "$HOME" sh -c "cat '$REPO/install.sh' | sh -s -- --uninstall" -- y ENTER > "$SANDBOX/ii4.log"
        [ -e "$HOME/.local/bin/claude-account" ] && echo yes > "$SANDBOX/ii4.bin" || echo no > "$SANDBOX/ii4.bin"
      )
      out=$(cat "$SANDBOX/ii0.log")
      want_has "install (interactive): shows the plan before changing anything" "This will:" "$out"
      want_has "install (interactive): ...naming the rc file" "add the shell integration to ~/.zshrc" "$out"
      want_has "install (interactive): Cancel exits 130" "<<exit 130>>" "$out"
      want "install (interactive): ...having changed nothing" "no" "$(cat "$SANDBOX/ii0.bin")"
      out=$(cat "$SANDBOX/ii1.log")
      want_has "install (interactive): Enter proceeds" "Installed claude-account" "$out"
      want_has "install (interactive): offers to run setup" "Set up your accounts now" "$out"
      want_has "install (interactive): ends with next steps" "Next steps" "$out"
      want_has "install (interactive): ...starting with a new terminal" "Open a new terminal" "$out"
      want_has "install (interactive): ...and setup when it was skipped" "claude-account setup" "$out"
      want_has "install (interactive): running it again says reinstall" "reinstall claude-account" "$(cat "$SANDBOX/ii2.log")"
      out=$(cat "$SANDBOX/ii3.log")
      want "install (interactive): Customize changes the folder" "yes" "$(cat "$SANDBOX/ii3.bin")"
      want_has "install (interactive): ...and without the rc file, says what to add" "eval" "$out"
      want_has "install (interactive): uninstall asks through the binary" "Continue?" "$(cat "$SANDBOX/ii4.log")"
      want "install (interactive): ...and removes it on yes" "no" "$(cat "$SANDBOX/ii4.bin")"
    else skip "install (interactive)" "expect not installed"; fi
  fi
fi

printf '\n%d passed, %d failed, %d skipped\n' "$PASS" "$FAIL" "$SKIP"
if [ "$FAIL" -gt 0 ]; then printf 'Failed:%s\n' "$FAILED"; exit 1; fi
