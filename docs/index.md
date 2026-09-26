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
    details: Each account keeps its own login side by side, while settings, skills, plugins and conversation history stay shared.
  - title: Asks once
    details: In a new folder, <code>claude</code> asks which account to use and remembers it. Enter takes the default.
  - title: Interactive or headless
    details: In a terminal, commands ask only for what is missing. In scripts they take arguments, <code>-y</code> and <code>--json</code>.
---

<!-- markdownlint-disable-next-line MD041 -- the hero above is the title -->
## How it works

```text
~/work                -> work       (your work account)
~/work/side-project   -> personal   (an override for one folder)
~/personal            -> personal
~/other               -> asks once, then remembers
```

Install with the command above, then run `claude-account setup`: it names the
login you already have, adds your other accounts and maps your folders. From
then on, `claude` picks the account on its own. [Read the guide](/guide).
