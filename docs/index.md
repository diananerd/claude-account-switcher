---
layout: home
hero:
  name: Claude Account Switcher
  tagline: Several Claude Code accounts on one machine. Every login stays live, and each folder decides which one <code>claude</code> uses.
  actions:
    - theme: brand
      text: Get started
      link: /guide
    - theme: alt
      text: Reference
      link: /reference
    - theme: alt
      text: GitHub
      link: https://github.com/diananerd/claude-account-switcher
features:
  - title: Per folder
    details: Map <code>~/work</code> to your work account and <code>~/personal</code> to yours. Subfolders inherit and can override; symlinks and git worktrees resolve the same way.
  - title: No more logging out
    details: Each account keeps its own login side by side, and all of them use your one local setup, so switching never means configuring again.
  - title: Asks once
    details: In a new folder, <code>claude</code> asks which account to use and remembers it. Enter takes the default.
  - title: Interactive or headless
    details: In a terminal, commands ask only for what is missing. In scripts they take arguments, <code>-y</code> and <code>--json</code>.
---

<!-- markdownlint-disable-next-line MD041 -- the hero above is the title -->
## Quick start

1. Install with the command above. It also adds `csw`, a short name for
   `claude-switcher`.
2. Run `csw setup`: name the account you are logged in to, add your other
   accounts, and map your folders.
3. Run `claude` in any project. It uses that folder's account, and asks once
   in a folder that has none.

Switch a project later with `csw`. [Read the guide](/guide) for daily use, or
[how it works](/how-it-works) for the details.
