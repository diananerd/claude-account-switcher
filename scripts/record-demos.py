#!/usr/bin/env python3
"""Record the terminal demos on the docs site as asciicast v2 files.

Every scene runs the real claude-switcher binary in a real pseudo-terminal, in a
throwaway HOME with a clean environment: no rc files, no real accounts. Only
`claude` is a stand-in (a small script answering `auth` the way Claude Code
does), because a real one would show real logins. The install scene runs the
public one-line installer, so it needs the network.

    scripts/record-demos.py            all scenes, into docs/public/demos/
    scripts/record-demos.py use        one scene

Output is checked for anything from the machine it was recorded on (home
paths, temp dirs, the recording user's name) and refused if found.
"""

import json
import os
import pty
import random
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "docs", "public", "demos")
WIDTH, HEIGHT = 80, 20
MAX_IDLE = 1.4  # seconds; longer silences (network, a slow build) are cut down
# Demos show placeholder versions, so they neither age nor echo a real machine.
DEMO_CSW_VERSION = "1.0.0"
DEMO_CLAUDE_VERSION = "2.0.0"  # also hardcoded in FAKE_CLAUDE

FAKE_CLAUDE = r"""#!/bin/bash
# Stand-in for Claude Code in the demos: the same auth commands and output
# shapes, and a small TUI (header, prompt box, status line) that takes /exit.
# The status line is the real one: `claude-switcher statusline`, fed the JSON
# Claude Code sends.
dir="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
json="$HOME/.claude.json"; [ -n "${CLAUDE_CONFIG_DIR:-}" ] && json="$CLAUDE_CONFIG_DIR/.claude.json"
email=$(cat "$dir/.demo-login" 2>/dev/null)
case "${1:-} ${2:-}" in
  "--version "*) echo "2.0.0 (Claude Code)"; exit ;;
  "--help "*) printf 'Usage: claude [options] [command] [prompt]\n\nCommands:\n  auth  Manage authentication\n  mcp  Configure MCP servers\n  update  Check for updates\n'; exit ;;
  "auth status")
    if [ -z "$email" ]; then echo '{"loggedIn":false,"authMethod":"none"}'
    elif [ "${3:-}" = "--text" ]; then printf 'Login method: Claude Max account\nEmail: %s\n' "$email"
    else printf '{"loggedIn":true,"authMethod":"claude.ai","email":"%s"}\n' "$email"; fi
    exit ;;
  "auth login")
    echo "Opening your browser to sign in..."
    sleep 1.6
    email="${DEMO_NEXT_LOGIN:-you@company.com}"
    echo "$email" > "$dir/.demo-login"
    printf '{"oauthAccount":{"emailAddress":"%s"}}\n' "$email" > "$json"
    echo "Login successful."
    exit ;;
esac

cols=$(stty size 2>/dev/null | cut -d' ' -f2); cols=${cols:-80}
here=${PWD/#$HOME/\~}
dim=$'\033[38;5;245m' acc=$'\033[38;5;173m' bold=$'\033[1m' off=$'\033[0m'
rule() { printf '%s%s%s\n' "$dim" "$(printf '%*s' "$cols" '' | tr ' ' '-' | sed 's/-/─/g')" "$off"; }
box() { # one line inside the header box, padded to its width
  local text="$1" plain="$2" w=$((cols > 62 ? 60 : cols - 2))
  printf '%s│%s %s%*s%s│%s\n' "$acc" "$off" "$text" $((w - 1 - ${#plain})) '' "$acc" "$off"
}
w=$((cols > 62 ? 60 : cols - 2))
line=$(printf '%*s' "$w" '' | sed 's/ /─/g')
status=$(printf '{"cwd":"%s","workspace":{"current_dir":"%s"},"model":{"display_name":"Opus"}}' "$PWD" "$PWD" \
  | claude-switcher statusline 2>/dev/null)

# Full screen, like the real TUI: drawn on the alternate screen, so leaving it
# gives the shell back exactly as it was.
printf '\033[?1049h\033[H\033[2J'
printf '\n%s╭%s╮%s\n' "$acc" "$line" "$off"
box "${acc}✻${off} ${bold}Claude Code${off} v2.0.0" "✻ Claude Code v2.0.0"
box "${dim}${email:-not logged in} · Claude Max${off}" "${email:-not logged in} · Claude Max"
box "${dim}${here}${off}" "$here"
printf '%s╰%s╯%s\n\n' "$acc" "$line" "$off"
rule; printf '\n'; rule
printf '  %s%s%s %s@%s%s\n' "$dim" "$here" "$off" "$acc" "${status:-?}" "$off"
printf '\033[3A\r%s❯%s ' "$bold" "$off"
IFS= read -r input
printf '\033[?1049l'
"""

ZSHRC = r"""unsetopt PROMPT_SP
# Window title, as terminals show it: the folder, and the command while it runs.
precmd() { print -Pn '\e]0;%~\a' }
preexec() { print -n "\e]0;${(%):-%~} \u2014 ${1%% *}\a" }
PROMPT='%F{245}%~%f %F{173}$%f '
export PATH="$HOME/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
"""


class Home:
    """A throwaway HOME with a fake Claude Code logged in to you@example.com."""

    def __init__(self, with_switcher):
        self.root = os.path.realpath(tempfile.mkdtemp(prefix="csw-demo."))
        self.home = os.path.join(self.root, "home")
        bin_dir = os.path.join(self.home, ".local", "bin")
        os.makedirs(os.path.join(self.home, ".claude"))
        os.makedirs(bin_dir)
        write(os.path.join(bin_dir, "claude"), FAKE_CLAUDE, 0o755)
        write(os.path.join(self.home, ".claude", ".demo-login"), "you@example.com\n")
        write(os.path.join(self.home, ".claude.json"), '{"oauthAccount":{"emailAddress":"you@example.com"}}\n')
        write(os.path.join(self.home, ".zshrc"), ZSHRC)
        latest = os.path.join(self.root, "latest.json")
        write(latest, json.dumps({"tag_name": "v" + version()}))
        self.env = {
            "HOME": self.home,
            "PATH": bin_dir + ":/usr/bin:/bin:/usr/sbin:/sbin",
            "TERM": "xterm-256color",
            "LANG": "en_US.UTF-8",
            "SHELL": "/bin/zsh",
            "USER": "you",
            "CLAUDE_SWITCHER_UPDATE_URL": "file://" + latest,
        }
        if with_switcher:
            shutil.copy(binary(), os.path.join(bin_dir, "claude-switcher"))
            os.symlink("claude-switcher", os.path.join(bin_dir, "csw"))

    def run(self, *args, extra_env=None):
        env = dict(self.env, **(extra_env or {}))
        subprocess.run(args, env=env, cwd=self.home, check=True, stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def mkdir(self, *rel):
        for r in rel:
            os.makedirs(os.path.join(self.home, r), exist_ok=True)

    def close(self):
        shutil.rmtree(self.root, ignore_errors=True)


class Recorder:
    """A zsh in a pty; keystrokes typed at a human pace, output timestamped."""

    def __init__(self, home, cwd="~"):
        self.home = home
        self.rng = random.Random(7)
        self.events = []
        self.clock = 0.0
        self.buffer = ""
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(cwd.replace("~", home.home, 1))
            os.execve("/bin/zsh", ["zsh", "-i"], dict(home.env, ZDOTDIR=home.home))
        self.pid, self.fd = pid, fd
        import fcntl
        import struct
        import termios
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", HEIGHT, WIDTH, 0, 0))
        self.last = time.monotonic()
        self.expect(r"\$ ", 10)
        # The shell's first prompt starts the recording on a clean screen.
        self.events = [[0.0, "o", self.buffer[self.buffer.rfind("\n") + 1:]]]
        self.clock = 0.0

    def _read(self, timeout):
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return False
        try:
            data = os.read(self.fd, 65536).decode("utf-8", "replace")
        except OSError:
            return False
        now = time.monotonic()
        self.clock += min(now - self.last, MAX_IDLE)
        self.last = now
        self.events.append([round(self.clock, 3), "o", data])
        self.buffer += data
        return True

    def expect(self, pattern, timeout=15):
        """Wait until the output since the last expect matches `pattern`."""
        if os.environ.get("DEMO_DEBUG"):
            print(f"  expect {pattern!r}", file=sys.stderr)
        deadline = time.monotonic() + timeout
        start = len(self.buffer) if hasattr(self, "_mark") else 0
        while not re.search(pattern, strip(self.buffer[start:])):
            if time.monotonic() > deadline:
                sys.exit(f"timeout waiting for {pattern!r}; screen:\n{strip(self.buffer[-1500:])}")
            self._read(0.1)
        self._mark = len(self.buffer)

    def pause(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self._read(max(0.0, min(0.05, end - time.monotonic())))

    def type(self, text, enter=True):
        """Type like a person: steady, a little uneven, never instant."""
        self._mark = len(self.buffer)
        self.pause(0.5)
        for ch in text:
            os.write(self.fd, ch.encode())
            self.pause(self.rng.uniform(0.045, 0.11))
        if enter:
            self.pause(0.25)
            os.write(self.fd, b"\r")

    def key(self, name, after=0.35):
        codes = {"enter": b"\r", "down": b"\x1b[B", "up": b"\x1b[A", "y": b"y", "n": b"n"}
        self._mark = len(self.buffer)
        self.pause(after)
        os.write(self.fd, codes.get(name, name.encode()))

    def finish(self, path, title):
        self.pause(2.5)
        keep = len(self.events)
        os.write(self.fd, b"exit\r")
        self.pause(1.0)
        try:
            if os.waitpid(self.pid, os.WNOHANG) == (0, 0):
                os.kill(self.pid, 9)
                os.waitpid(self.pid, 0)
        except ChildProcessError:
            pass
        # The scene ends on its last prompt, without the typed `exit`.
        del self.events[keep:]
        real = version()
        for e in self.events:
            e[2] = e[2].replace(real, DEMO_CSW_VERSION)
            e[2] = re.sub(r"macOS \d+(\.\d+)*", "macOS 15.0", e[2])
        header = {"version": 2, "width": WIDTH, "height": HEIGHT, "title": title,
                  "env": {"TERM": "xterm-256color", "SHELL": "/bin/zsh"}}
        text = json.dumps(header) + "\n" + "".join(json.dumps(e) + "\n" for e in self.events)
        leaks(text, self.home)
        write(path, text)
        print(f"{os.path.relpath(path, REPO)}: {self.clock:.1f}s, {len(self.events)} events")


# ------------------------------------------------------------------ scenes

def scene_install():
    home = Home(with_switcher=False)
    try:
        t = Recorder(home)
        t.type("curl -fsSL https://switcher.diananerd.com | sh")
        t.expect(r"Cancel", 60)
        t.key("enter", after=1.2)
        t.expect(r"Set up your accounts now", 90)
        t.type("n")  # the installer reads a whole line
        t.expect(r"Docs: ", 30)
        t.finish(os.path.join(OUT, "install.cast"), "Install")
    finally:
        home.close()


def scene_setup():
    home = Home(with_switcher=True)
    home.mkdir("work", "personal")
    try:
        t = Recorder(home)
        t.type("csw setup")
        t.expect(r"Name for this account")
        t.type("personal")
        t.expect(r"Add another Claude account")
        t.key("y", after=0.8)
        t.expect(r"Profile name")
        t.type("work")
        t.expect(r"What is work")
        t.key("enter", after=0.9)
        t.expect(r"Log work in now")
        t.key("y", after=0.8)
        t.expect(r"Add one more")
        t.key("n", after=0.9)
        t.expect(r"Default profile")
        t.key("enter", after=0.9)
        t.expect(r"Folder to map")
        t.type("~/work")
        t.expect(r"Profile for ~/work")
        t.type("wo", enter=False)
        t.key("enter", after=0.5)
        t.expect(r"Folder to map")
        t.type("~/personal")
        t.expect(r"Profile for ~/personal")
        t.key("enter", after=0.8)
        t.expect(r"Folder to map")
        t.key("enter", after=0.8)
        t.expect(r"Route `claude` through")
        t.key("y", after=0.9)
        t.expect(r"Next steps[\s\S]*\$ $", 20)
        t.finish(os.path.join(OUT, "setup.cast"), "Set up")
    finally:
        home.close()


def scene_use():
    home = Home(with_switcher=True)
    home.mkdir("work/api", "personal/blog", "other/new-idea")
    subprocess.run(["git", "init", "-q", os.path.join(home.home, "other", "new-idea")], check=True)
    try:
        home.run("claude-switcher", "add", "personal", "--base")
        home.run("claude-switcher", "add", "work", "--no-login")
        home.run("claude-switcher", "login", "work", extra_env={"DEMO_NEXT_LOGIN": "you@company.com"})
        home.run("claude-switcher", "default", "personal")
        home.run("claude-switcher", "use", "work", os.path.join(home.home, "work"))
        home.run("claude-switcher", "use", "personal", os.path.join(home.home, "personal"))
        home.run("claude-switcher", "shell", "install", "zsh")
        t = Recorder(home, cwd="~/work/api")
        t.type("claude")
        t.expect(r"❯ ")
        t.pause(1.6)
        t.type("/exit")
        t.expect(r"\$ $")
        t.type("cd ~/personal/blog && claude")
        t.expect(r"❯ ")
        t.pause(1.6)
        t.type("/exit")
        t.expect(r"\$ $")
        t.type("cd ~/other/new-idea && claude")
        t.expect(r"account for")
        t.key("down", after=1.1)
        t.key("enter", after=0.9)
        t.expect(r"❯ ")
        t.pause(1.6)
        t.type("/exit")
        t.expect(r"\$ $")
        t.type("csw status")
        t.expect(r"Source:.*\n[\s\S]*\$ $")
        t.finish(os.path.join(OUT, "use.cast"), "Use")
    finally:
        home.close()


def scene_switch():
    home = Home(with_switcher=True)
    home.mkdir("work", "personal", "code/app")
    try:
        home.run("claude-switcher", "add", "personal", "--base")
        home.run("claude-switcher", "add", "work", "--no-login")
        home.run("claude-switcher", "login", "work", extra_env={"DEMO_NEXT_LOGIN": "you@company.com"})
        home.run("claude-switcher", "default", "personal")
        home.run("claude-switcher", "use", "work", os.path.join(home.home, "work"))
        home.run("claude-switcher", "use", "personal", os.path.join(home.home, "personal"))
        home.run("claude-switcher", "use", "work", os.path.join(home.home, "code", "app"))
        home.run("claude-switcher", "shell", "install", "zsh")
        t = Recorder(home, cwd="~/code/app")
        t.type("claude")
        t.expect(r"❯ ")
        t.pause(1.6)
        t.type("/exit")
        t.expect(r"\$ $")
        t.type("csw")
        t.expect(r"account for")
        t.key("up", after=1.4)
        t.key("enter", after=0.9)
        t.expect(r"new sessions[\s\S]*\$ $")
        t.type("claude")
        t.expect(r"❯ ")
        t.pause(1.8)
        t.type("/exit")
        t.expect(r"\$ $")
        t.finish(os.path.join(OUT, "switch.cast"), "Switch")
    finally:
        home.close()


SCENES = {"use": scene_use, "switch": scene_switch, "setup": scene_setup, "install": scene_install}

# ------------------------------------------------------------------ helpers


def write(path, text, mode=0o644):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)
    os.chmod(path, mode)


def strip(s):
    return re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]|\r", "", s)


def version():
    with open(os.path.join(REPO, "Cargo.toml")) as f:
        return re.search(r'^version = "([^"]+)"', f.read(), re.M).group(1)


def binary():
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=REPO, check=True)
    return os.path.join(REPO, "target", "release", "claude-switcher")


def leaks(text, home):
    """Refuse anything that identifies the recording machine."""
    real_home = os.path.expanduser("~")
    user = os.environ.get("USER", "")
    needles = [home.root, os.path.realpath(home.root), real_home, "/var/folders", "/private/", version()]
    claude = subprocess.run(["claude", "--version"], capture_output=True, text=True).stdout.split(" ")[0]
    if claude:
        needles.append(claude)
    if user and user not in ("you", "runner"):
        needles += ["/Users/" + user, "/home/" + user, user + "@"]
    found = [n for n in needles if n and n in text]
    # The only accounts a demo may show are the example ones.
    found += sorted(set(re.findall(r"[\w.+-]+@[\w-]+\.[\w.-]+", re.sub(r"\\u001b\[[0-9;?]*[a-zA-Z]", " ", text))) - {"you@example.com", "you@company.com"})
    if found:
        sys.exit(f"refusing to write a demo that contains {found}")


if __name__ == "__main__":
    names = sys.argv[1:] or list(SCENES)
    for name in names:
        SCENES[name]()
