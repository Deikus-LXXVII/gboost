# GBOOST — Grok Bot Operations & Superpowers Toolkit

GBOOST extends what Grok Bot can do for its owner (mail, calendar, notes, smart home, messengers) while
keeping the owner's data safe from outside attackers and from the Bot's own mistakes. The core runs on the
owner's own always-on machine. Plugins are sandboxed WASM components, and anything that cannot be undone
needs the owner's approval on their phone.

**Status: early design.** The repository holds the design and Phase 0 spikes (throwaway experiments). It runs
on Windows, macOS and Linux, on a home computer or a VPS.

**Install:** give Grok Bot the link to this repository and ask it to install GBOOST. The Bot follows
[`INSTALL.md`](INSTALL.md) and asks you only when it needs you. For now this installs the S9 test core.

- Design: [`docs/plans/gboost-method.md`](docs/plans/gboost-method.md)
- Phase 0 progress: [`docs/plans/phase-0.md`](docs/plans/phase-0.md)
- Security invariants: [`docs/security/invariants.md`](docs/security/invariants.md)
- Spikes: [`spikes/`](spikes/)
