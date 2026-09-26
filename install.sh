#!/bin/sh
# claude-account installer
#
#   curl -fsSL https://switcher.diananerd.com | sh
#
# How it behaves:
#   - It inspects the machine first and shows what it found.
#   - It checks every dependency up front and names all that are missing.
#   - Nothing changes until the binary is downloaded, verified (SHA-256) and seen
#     to run. Every error says what happened, the likely cause and what to do.
#   - Running it again always converges on the same end state: the binary is
#     replaced atomically, leftovers of an interrupted run are cleared, and the
#     shell block is replaced in place. An interrupted run is finished by
#     running it again.
#   - Interactive when a terminal is available (answers come from /dev/tty, so
#     `curl | sh` works). Headless with -y/--yes, without a terminal, or when CI is set.
# Unix-like systems only (macOS, Linux; on Windows use WSL). Tested on macOS.

set -eu

REPO="diananerd/claude-account-switcher"
DOCS="https://switcher.diananerd.com"
ISSUES="https://github.com/$REPO/issues"
RELEASES="https://github.com/$REPO/releases"
CLAUDE_DOCS="https://code.claude.com/docs/en/setup"
BIN_NAME="claude-account"
BIN_DIR="${CLAUDE_ACCOUNT_BIN_DIR:-${HOME:-}/.local/bin}"
VERSION="${CLAUDE_ACCOUNT_VERSION:-latest}"
# Base URL holding the release assets; overridable for mirrors and tests.
DOWNLOAD_URL="${CLAUDE_ACCOUNT_DOWNLOAD_URL:-}"
MODIFY_RC=1
ACTION=install
PURGE=""
YES=0
# Set once the first change is made: errors after it explain how to finish.
CHANGED=0

usage() {
  cat <<'EOF'
claude-account installer

Usage:
  curl -fsSL https://switcher.diananerd.com | sh
  curl -fsSL https://switcher.diananerd.com | sh -s -- [options]

Options:
  -y, --yes         no questions: install with the defaults (also without a terminal, or with CI set)
  --bin-dir DIR     where to put the binary (default: ~/.local/bin)
  --version TAG     a specific release, e.g. v0.1.0 (default: latest)
  --no-modify-rc    do not add the shell integration to your rc file
  --uninstall       remove the binary and the shell integration
  --purge           with --uninstall: also log out and delete the profiles it created
  -h, --help        this help

Running it again is safe: it upgrades, reinstalls or finishes an interrupted install.
EOF
}

# ------------------------------------------------------------------ output

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  B=$(printf '\033[1m'); DIM=$(printf '\033[2m'); G=$(printf '\033[32m'); Y=$(printf '\033[33m')
  R=$(printf '\033[31m'); C=$(printf '\033[36m'); N=$(printf '\033[0m')
else
  B=""; DIM=""; G=""; Y=""; R=""; C=""; N=""
fi
say() { printf '%s\n' "$*"; }
ok() { printf '%s✔%s %s\n' "$G" "$N" "$*"; }
info() { printf '%s%s%s\n' "$DIM" "$*" "$N"; }
warn() { printf '%swarning:%s %s\n' "$Y" "$N" "$*" >&2; }
# fail "what happened" ["what to do" ...]: says whether anything changed, exits 1.
fail() {
  printf '%serror:%s %s\n' "$R" "$N" "$1" >&2
  shift
  for h in "$@"; do printf '  %s->%s %s\n' "$C" "$N" "$h" >&2; done
  if [ "$CHANGED" = 0 ]; then
    printf '%sNothing was changed.%s\n' "$DIM" "$N" >&2
  else
    printf '%sRun the installer again to finish: it picks up where it stopped.%s\n' "$DIM" "$N" >&2
  fi
  exit 1
}
# ~-abbreviated path, whether it is spelled through $HOME or its resolved form.
HOME_REAL=$(cd "${HOME:-/}" 2>/dev/null && pwd -P || printf '%s' "${HOME:-}")
tilde() {
  for h in "$HOME" "$HOME_REAL"; do
    case "$1" in "$h") printf '~'; return ;; "$h"/*) printf '~%s' "${1#"$h"}"; return ;; esac
  done
  printf '%s' "$1"
}

# ------------------------------------------------------------------ arguments

while [ $# -gt 0 ]; do
  case "$1" in
    -y|--yes) YES=1; shift ;;
    --bin-dir) [ $# -ge 2 ] || fail "--bin-dir needs a folder"; BIN_DIR="$2"; shift 2 ;;
    --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
    --version) [ $# -ge 2 ] || fail "--version needs a tag, e.g. v0.1.0"; VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --no-modify-rc) MODIFY_RC=0; shift ;;
    --uninstall) ACTION=uninstall; shift ;;
    --purge) PURGE="--purge"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; fail "unknown option: $1" ;;
  esac
done
if [ -n "$PURGE" ] && [ "$ACTION" != uninstall ]; then
  fail "--purge only goes with --uninstall"
fi

# Interactive only with a terminal to ask on. Under `curl | sh` stdin is the
# script itself, so answers are read from /dev/tty.
INTERACTIVE=0
if [ "$YES" = 0 ] && [ -z "${CI:-}" ] && [ -t 1 ] && (: </dev/tty) 2>/dev/null; then
  INTERACTIVE=1
fi

# ask "Question" Y|N -> status 0 for yes. Enter takes the capitalised default.
ask() {
  if [ "$2" = Y ]; then hint="[Y/n]"; else hint="[y/N]"; fi
  printf '%s%s%s %s ' "$B" "$1" "$N" "$hint"
  IFS= read -r answer </dev/tty || answer=""
  case "$answer" in
    [Yy]|[Yy][Ee][Ss]) return 0 ;;
    [Nn]|[Nn][Oo]) return 1 ;;
    "") [ "$2" = Y ] ;;
    *) ask "$1" "$2" ;;
  esac
}

# ask_value "Question" default -> prints the answer (the default on Enter).
ask_value() {
  printf '%s%s%s %s[%s]%s ' "$B" "$1" "$N" "$DIM" "$2" "$N" >/dev/tty
  IFS= read -r answer </dev/tty || answer=""
  printf '%s\n' "${answer:-$2}"
}

# ------------------------------------------------------------------ environment

have() { command -v "$1" >/dev/null 2>&1; }

OS=$(uname -s 2>/dev/null || echo unknown)

# How to get missing tools on this system, as one line.
install_hint() {
  case "$OS" in
    Darwin) printf 'install them with Homebrew (brew install %s) or the Xcode command line tools (xcode-select --install)' "$*" ;;
    Linux)
      if have apt-get; then printf 'sudo apt-get install -y %s' "$*"
      elif have dnf; then printf 'sudo dnf install -y %s' "$*"
      elif have apk; then printf 'sudo apk add %s' "$*"
      elif have pacman; then printf 'sudo pacman -S %s' "$*"
      else printf 'install %s with your package manager' "$*"; fi ;;
    *) printf 'install %s' "$*" ;;
  esac
}

# Every dependency at once, so a missing tool is never discovered halfway.
check_deps() {
  missing=""
  # gzip too: GNU tar runs it to read .tar.gz (bsdtar on macOS has it built in).
  for t in uname tar gzip mktemp mkdir cp mv chmod rm dirname sed tr cut sort head tail; do
    have "$t" || missing="$missing $t"
  done
  if have curl; then FETCH=curl; elif have wget; then FETCH=wget; else missing="$missing curl"; fi
  if have shasum; then HASH=shasum; elif have sha256sum; then HASH=sha256sum
  elif have openssl; then HASH=openssl; else missing="$missing coreutils"; fi
  if [ -n "$missing" ]; then
    # shellcheck disable=SC2086 # the list is meant to split
    fail "missing tools:$missing" "$(install_hint $missing)" "then run the installer again"
  fi
}

sha256_of() {
  case "$HASH" in
    shasum) shasum -a 256 "$1" | cut -d' ' -f1 ;;
    sha256sum) sha256sum "$1" | cut -d' ' -f1 ;;
    openssl) openssl dgst -sha256 "$1" | sed 's/.*= *//' ;;
  esac
}

detect_target() {
  if [ -n "${CLAUDE_ACCOUNT_TARGET:-}" ]; then
    TARGET="$CLAUDE_ACCOUNT_TARGET"
    return
  fi
  arch=$(uname -m)
  case "$OS" in
    Darwin) os_part="apple-darwin"
      # A shell running under Rosetta reports x86_64 on Apple silicon.
      if [ "$arch" = "x86_64" ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
        arch=arm64
      fi ;;
    Linux) os_part="unknown-linux-musl" ;;
    MINGW*|MSYS*|CYGWIN*) fail "Windows is not supported directly." "use WSL: https://learn.microsoft.com/windows/wsl/install" ;;
    *) fail "unsupported system: $OS." "claude-account runs on macOS and Linux" ;;
  esac
  case "$arch" in
    arm64|aarch64) arch_part="aarch64" ;;
    x86_64|amd64) arch_part="x86_64" ;;
    *) fail "unsupported processor: $arch." "published builds: Apple silicon and Intel Macs, Linux x86_64 and ARM64" ;;
  esac
  TARGET="$arch_part-$os_part"
}

# Anything that would make the install fail or land in the wrong place.
preflight() {
  if [ -z "${HOME:-}" ] || [ ! -d "$HOME" ]; then
    fail "HOME is not set to an existing folder."
  fi
  if [ "$(id -u 2>/dev/null || echo 1)" = 0 ] && [ -n "${SUDO_USER:-}" ]; then
    fail "do not run the installer with sudo: it would install into root's home." \
      "run it as yourself; it only writes inside your home"
  fi
  case "$BIN_DIR" in
    /*) ;;
    *) fail "--bin-dir must be an absolute path (got: $BIN_DIR)." ;;
  esac
  if [ "$OS" = Darwin ] && [ -z "${CLAUDE_ACCOUNT_TARGET:-}" ]; then
    major=$(sw_vers -productVersion 2>/dev/null | cut -d. -f1)
    if [ -n "$major" ] && [ "$major" -lt 11 ] 2>/dev/null; then
      fail "macOS $(sw_vers -productVersion) is too old." "claude-account needs macOS 11 (Big Sur) or newer"
    fi
  fi
  # The install folder, or its nearest existing parent, must be writable.
  probe="$BIN_DIR"
  while [ ! -d "$probe" ]; do probe=$(dirname "$probe"); done
  if [ ! -w "$probe" ]; then
    fail "cannot write to $(tilde "$probe")." \
      "choose a folder you own: --bin-dir ~/bin (or Customize, interactively)"
  fi
}

on_path() { case ":$PATH:" in *":$1:"*) return 0 ;; *) return 1 ;; esac; }

# version_cmp A B -> prints lt, eq or gt, in semver order: x.y.z compared as
# numbers, and a pre-release (x.y.z-rc.1) before its own release.
version_cmp() {
  if [ "$1" = "$2" ]; then echo eq; return; fi
  a=${1%%-*}; b=${2%%-*}
  if [ "$a" = "$b" ]; then
    # Same numbers: the one without a suffix is the release, so it is greater.
    case "$1" in *-*) echo lt ;; *) echo gt ;; esac
    return
  fi
  lowest=$(printf '%s\n%s\n' "$a" "$b" | sort -t. -k1,1n -k2,2n -k3,3n | head -1)
  if [ "$lowest" = "$a" ]; then echo lt; else echo gt; fi
}

# What is on this machine now, shown before anything happens.
show_state() {
  say ""
  say "${B}Found${N}"
  os_name="$OS"
  [ "$OS" != Darwin ] || os_name="macOS $(sw_vers -productVersion 2>/dev/null || true)"
  say "  System         $os_name, $(uname -m) -> $TARGET"
  if have claude; then
    say "  Claude Code    $(claude --version 2>/dev/null | head -1 | cut -d' ' -f1) at $(tilde "$(command -v claude)")"
  else
    say "  Claude Code    ${Y}not found${N} (needed to use claude-account: $CLAUDE_DOCS)"
  fi
  OLD_VERSION=""
  if [ -x "$BIN_DIR/$BIN_NAME" ]; then
    OLD_VERSION=$("$BIN_DIR/$BIN_NAME" --version 2>/dev/null | cut -d' ' -f2) || OLD_VERSION="unknown"
    say "  claude-account $OLD_VERSION in $(tilde "$BIN_DIR")"
  else
    say "  claude-account not installed in $(tilde "$BIN_DIR")"
  fi
  other=$(command -v "$BIN_NAME" 2>/dev/null || true)
  if [ -n "$other" ] && [ "$other" != "$BIN_DIR/$BIN_NAME" ]; then
    say "                 ${Y}another copy comes first on PATH:${N} $(tilde "$other")"
  fi
  say "  Shell          ${SHELL:-unknown}"
  if on_path "$BIN_DIR"; then
    say "  PATH           includes $(tilde "$BIN_DIR")"
  else
    say "  PATH           does not include $(tilde "$BIN_DIR") (the shell integration adds it)"
  fi
}

expand_dir() { # ~ and relative paths typed at a prompt -> absolute
  # shellcheck disable=SC2088 # a literal ~ typed by the user, expanded here
  case "$1" in
    "~") printf '%s\n' "$HOME" ;;
    "~/"*) printf '%s/%s\n' "$HOME" "${1#"~/"}" ;;
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$(pwd)" "$1" ;;
  esac
}

# fetch <url> <file>: explains the failure instead of printing a transfer code.
fetch() {
  if [ "$FETCH" = curl ]; then
    rc=0
    curl -fsSL --proto '=https,file' --retry 3 --connect-timeout 15 -o "$2" "$1" 2>"$TMP/fetch.err" || rc=$?
    case "$rc" in
      0) return 0 ;;
      22|37) fail "not found: $1" \
        "check the version (releases: $RELEASES), or leave --version out for the latest" ;;
      5|6|7|28|35|56) fail "could not reach $(printf '%s' "$1" | cut -d/ -f1-3) ($(tail -1 "$TMP/fetch.err"))." \
        "check your internet connection or proxy (HTTPS_PROXY), then run the installer again" ;;
      *) fail "download failed: $1 ($(tail -1 "$TMP/fetch.err"))." "run the installer again; if it keeps failing, see $ISSUES" ;;
    esac
  else
    wget -q -T 15 -O "$2" "$1" 2>"$TMP/fetch.err" || {
      rc=$?
      [ "$rc" != 8 ] || fail "not found: $1" "check the version (releases: $RELEASES)"
      fail "could not download $1." "check your internet connection or proxy (HTTPS_PROXY), then run the installer again"
    }
  fi
}

# What the shell step would do, in the binary's own words (it owns rc editing).
rc_plan() { # <binary>
  if [ "$MODIFY_RC" = 0 ]; then
    printf 'leave your shell rc files alone\n'
    return
  fi
  if on_path "$BIN_DIR"; then
    line=$("$1" shell install --dry-run 2>/dev/null | tail -1) || line=""
  else
    line=$("$1" shell install --dry-run --path-dir "$BIN_DIR" 2>/dev/null | tail -1) || line=""
  fi
  # shellcheck disable=SC2016 # the literal text $SHELL is meant
  case "$line" in
    "Would add the shell integration to "*)
      extra=""; on_path "$BIN_DIR" || extra=" (and put $(tilde "$BIN_DIR") on PATH)"
      printf 'add the shell integration to %s%s, so `claude` picks the account per folder\n' \
        "${line#Would add the shell integration to }" "$extra" ;;
    "Would update the shell integration in "*) printf 'update the shell integration in %s\n' "${line#Would update the shell integration in }" ;;
    "Shell integration already in "*) printf 'keep the shell integration already in %s\n' "${line#Shell integration already in }" ;;
    *) printf 'skip the shell integration (cannot tell your shell from $SHELL=%s)\n' "${SHELL:-unset}" ;;
  esac
}

# ------------------------------------------------------------------ install

install() {
  check_deps
  detect_target
  preflight
  asset="$BIN_NAME-$TARGET.tar.gz"
  if [ -n "$DOWNLOAD_URL" ]; then
    base="${DOWNLOAD_URL%/}"
  elif [ "$VERSION" = "latest" ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    base="https://github.com/$REPO/releases/download/$VERSION"
  fi

  TMP=$(mktemp -d 2>/dev/null || mktemp -d -t claude-account)
  # Always leave no temp files, and no half-copied binary next to the real one.
  trap 'rm -rf "$TMP"; rm -f "$BIN_DIR/.$BIN_NAME.new"' EXIT
  trap 'exit 130' INT TERM

  say "${B}claude-account installer${N}"
  show_state

  # 1. Download, verify, and see it run: nothing on the machine changes yet.
  say ""
  info "Downloading $asset ($VERSION)..."
  fetch "$base/$asset" "$TMP/$asset"
  fetch "$base/$asset.sha256" "$TMP/$asset.sha256"
  expected=$(cut -d' ' -f1 < "$TMP/$asset.sha256")
  actual=$(sha256_of "$TMP/$asset")
  if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    fail "the download does not match its checksum (corrupted or tampered with)." "run the installer again"
  fi
  tar -xzf "$TMP/$asset" -C "$TMP" 2>"$TMP/tar.err" \
    || fail "cannot unpack the download: $(tail -1 "$TMP/tar.err")" "run the installer again; if it keeps failing, see $ISSUES"
  [ -f "$TMP/$BIN_NAME" ] || fail "the archive does not contain $BIN_NAME." "report it: $ISSUES"
  chmod 755 "$TMP/$BIN_NAME"
  new_version=$("$TMP/$BIN_NAME" --version 2>/dev/null | cut -d' ' -f2) \
    || fail "the downloaded binary does not run on this machine ($TARGET)." "report it with your system details: $ISSUES"
  ok "Verified claude-account $new_version for $TARGET"

  # 2. The plan; interactive runs choose Proceed, Customize or Cancel.
  while :; do
    OLD_VERSION=""
    if [ -x "$BIN_DIR/$BIN_NAME" ]; then
      OLD_VERSION=$("$BIN_DIR/$BIN_NAME" --version 2>/dev/null | cut -d' ' -f2) || OLD_VERSION="0"
    fi
    verb=install
    if [ -n "$OLD_VERSION" ]; then
      case "$(version_cmp "$OLD_VERSION" "$new_version")" in
        lt) verb=upgrade ;; eq) verb=reinstall ;; *) verb=downgrade ;;
      esac
    fi
    say ""
    say "This will:"
    case "$verb" in
      install) say "  - install claude-account $new_version to $(tilde "$BIN_DIR")" ;;
      reinstall) say "  - reinstall claude-account $new_version in $(tilde "$BIN_DIR")" ;;
      *) say "  - $verb claude-account $OLD_VERSION -> $new_version in $(tilde "$BIN_DIR")" ;;
    esac
    say "  - $(rc_plan "$TMP/$BIN_NAME")"
    [ "$INTERACTIVE" = 1 ] || break
    say ""
    say "  ${B}1)${N} Proceed ${DIM}(default)${N}"
    say "  ${B}2)${N} Customize"
    say "  ${B}3)${N} Cancel"
    choice=$(ask_value ">" 1)
    case "$choice" in
      1) break ;;
      2)
        answer=$(ask_value "Install folder" "$(tilde "$BIN_DIR")")
        BIN_DIR=$(expand_dir "$answer")
        preflight
        if ask "Add the shell integration, so \`claude\` picks the account per folder?" Y; then MODIFY_RC=1; else MODIFY_RC=0; fi
        ;;
      3) say "Cancelled. Nothing was changed."; exit 130 ;;
      *) say "Type 1, 2 or 3." ;;
    esac
  done

  # 3. Apply. The binary is replaced by an atomic rename; the rc block is
  #    replaced in place by the binary. Both converge when run again.
  CHANGED=1
  mkdir -p "$BIN_DIR" || fail "cannot create $(tilde "$BIN_DIR")." "choose another folder with --bin-dir"
  cp "$TMP/$BIN_NAME" "$BIN_DIR/.$BIN_NAME.new" || fail "cannot write to $(tilde "$BIN_DIR")." "check the free space and permissions there"
  mv -f "$BIN_DIR/.$BIN_NAME.new" "$BIN_DIR/$BIN_NAME" || fail "cannot replace $(tilde "$BIN_DIR/$BIN_NAME")."
  case "$verb" in
    install) ok "Installed claude-account $new_version to $(tilde "$BIN_DIR")" ;;
    reinstall) ok "Reinstalled claude-account $new_version in $(tilde "$BIN_DIR")" ;;
    upgrade) ok "Upgraded claude-account $OLD_VERSION -> $new_version in $(tilde "$BIN_DIR")" ;;
    downgrade) ok "Downgraded claude-account $OLD_VERSION -> $new_version in $(tilde "$BIN_DIR")" ;;
  esac
  rc_changed=0
  rc_file=""
  if [ "$MODIFY_RC" = 1 ]; then
    if on_path "$BIN_DIR"; then set -- shell install; else set -- shell install --path-dir "$BIN_DIR"; fi
    if out=$("$BIN_DIR/$BIN_NAME" "$@" 2>"$TMP/shell.err"); then
      printf '%s\n' "$out" | sed "s/^[^A-Za-z]*//" | while IFS= read -r l; do ok "$l"; done
      case "$out" in *"added to"*|*"updated in"*) rc_changed=1 ;; esac
      rc_file=$(printf '%s\n' "$out" | tail -1 | sed 's/.* \([^ ]*\)$/\1/')
    else
      warn "the shell integration was not added: $(sed 's/^[^:]*: //' "$TMP/shell.err" | tail -1)"
      warn "claude-account is installed; add the integration later with: claude-account shell install"
    fi
  fi

  # 4. Check the result the way the user will meet it.
  first=$(command -v "$BIN_NAME" 2>/dev/null || true)
  if [ -n "$first" ] && [ "$first" != "$BIN_DIR/$BIN_NAME" ]; then
    warn "$(tilde "$first") comes before $(tilde "$BIN_DIR") on your PATH and will be used instead."
    warn "remove it (e.g. cargo uninstall claude-account-switcher), or put $(tilde "$BIN_DIR") first on PATH."
  fi

  # 5. What now. Fresh installs can run setup right away.
  has_profiles=0
  if [ "$("$BIN_DIR/$BIN_NAME" list --json 2>/dev/null | tr -d ' \n')" != "[]" ] \
     && "$BIN_DIR/$BIN_NAME" list --json >/dev/null 2>&1; then
    has_profiles=1
  fi
  ran_setup=0
  if [ "$INTERACTIVE" = 1 ] && [ "$has_profiles" = 0 ] && have claude; then
    say ""
    if ask "Set up your accounts now (claude-account setup)?" Y; then
      # setup prints the single "Next steps"; tell it what happened to the shell.
      # (just added -> open a new terminal first; skipped by choice -> say how to add it;
      # already there -> setup sees it itself).
      set --
      if [ "$rc_changed" = 1 ]; then set -- --shell-ready; elif [ "$MODIFY_RC" = 0 ]; then set -- --no-shell; fi
      if "$BIN_DIR/$BIN_NAME" setup "$@" </dev/tty; then
        ran_setup=1
      else
        warn "setup did not finish; run it again with: claude-account setup"
      fi
    fi
  fi

  if [ "$ran_setup" = 1 ]; then
    if [ "$MODIFY_RC" = 0 ] && ! on_path "$BIN_DIR"; then
      warn "$(tilde "$BIN_DIR") is not on your PATH; add it so the claude-account command is found."
    fi
    say ""
    info "Help: claude-account --help · Docs: $DOCS"
    return
  fi

  say ""
  say "${B}Next steps${N}"
  n=1
  if ! have claude; then
    say "  $n. Install Claude Code: $CLAUDE_DOCS"; n=$((n + 1))
  fi
  if [ "$rc_changed" = 1 ]; then
    say "  $n. Open a new terminal (or run: source $rc_file)"; n=$((n + 1))
  elif [ "$MODIFY_RC" = 0 ]; then
    say "  $n. Add to your shell rc file: eval \"\$(claude-account init zsh)\"   ${DIM}(or bash, fish)${N}"; n=$((n + 1))
    if ! on_path "$BIN_DIR"; then say "  $n. Put $(tilde "$BIN_DIR") on your PATH"; n=$((n + 1)); fi
  fi
  if [ "$has_profiles" = 1 ]; then
    say "  $n. Your profiles and mappings are unchanged. Check everything: ${C}claude-account doctor${N}"
  else
    say "  $n. Run ${C}claude-account setup${N} to name your accounts, log them in and map folders"; n=$((n + 1))
    say "  $n. Run ${C}claude${N} in any project: it uses that folder's account, or asks once"
  fi
  say ""
  info "Help: claude-account --help · Docs: $DOCS"
}

# ------------------------------------------------------------------ uninstall

uninstall() {
  bin=""
  if [ -x "$BIN_DIR/$BIN_NAME" ]; then
    bin="$BIN_DIR/$BIN_NAME"
  elif have "$BIN_NAME"; then
    bin=$(command -v "$BIN_NAME")
  fi
  if [ -z "$bin" ]; then
    say "claude-account is not installed in $(tilde "$BIN_DIR") nor on PATH; nothing to do."
    exit 0
  fi
  # The binary knows every file it touched: it shows the plan, asks (default
  # no) and removes them, then itself.
  if [ "$INTERACTIVE" = 1 ]; then
    "$bin" uninstall $PURGE </dev/tty
  else
    "$bin" uninstall --yes $PURGE
  fi
}

case "$ACTION" in
  install) install ;;
  uninstall) uninstall ;;
esac
