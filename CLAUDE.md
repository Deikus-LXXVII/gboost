# GBOOST

## Working mode: semi-autonomous (owner's decision, 2026-11-12)

Work through the current plan without asking about each step. Make the ordinary choices yourself (names,
formats, library versions, test layout, fixing review findings, local commits), record them in the plan in
`docs/plans/`, and keep going until a real stop point. Then report once: what was done, what was decided on
the way, and what needs the owner.

Stop and ask the owner only when:
- the action is outward-facing or hard to undo: push, PR, publishing, sending to external services,
  deleting data the owner made, touching files outside this repo and the owner's Obsidian vault folder `GBOOST/` (path set in the local `.mcp.json`);
- it is a security or trust decision (secrets, permissions, what agents may do without approval);
- it is a product choice with no sensible default, or the plan's premise turns out wrong;
- the owner must act or look (log in, a device, a visual check only they can judge);
- a check fails in a way you cannot fix after two honest attempts.

Map changes in the Obsidian vault need no approval (see `docs/plans/mindmap-mcp.md`).

## Product rules (owner, 2026-11-13)

- Phone first: everything in GBOOST works from a phone alone, the install included. A desktop is never
  required to set up, approve, manage or recover.
- Platforms: Android, iOS, macOS, Linux and Windows are all supported, each in the role it can play
  (see the plan, section 6).
- Nodes: GBOOST runs on more than one machine. If the owner's main computer drops off the network, a standby
  node (for example a VPS) takes over, and hands back when the main one returns.
- Never restrict Grok Bot's own capabilities for GBOOST's sake (no Tailscale ACLs on the Bot, no removing its
  access to the owner's machines): owners use them to run their computers. GBOOST's guarantees must hold with
  the Bot as capable as it is. (Owner, 2026-11-13.)
- Public project, no tailoring (owner, 2026-11-16): nothing in code, docs or install steps is fitted to one
  owner's hardware or setup (host names, tailnet names, IPs, ports chosen around their apps, "the owner's
  Mac", a named VPS, vault paths, single-OS code paths). GBOOST works in any configuration: Windows, Linux,
  macOS, a VPS and so on. The install detects the platform and asks the owner only on problems.
- These rules do not lower the security priority: each must be met without breaking the plan's principles,
  and any trade-off goes to the owner.

## Conventions

- Plans: `docs/plans/<topic>.md`, updated as work goes.
- Every change: tests and typecheck green, a practical check run by the agent, a reviewer pass, then a
  local commit on a branch (not main after the initial commit).
- Practical check (owner's rule, 2026-11-13): "compiles and unit tests pass" is not done. The agent runs the
  feature the way a user would: pipelines end to end on real inputs; services started and called; UIs opened
  in a real browser or device, every button and form actually pressed and its outcome checked; visuals
  checked against requirements (every element present, light and dark, phone width). Scripted checks live in
  the repo so they can be rerun. Report what was exercised and what was not. Only steps that truly need the
  owner (their Face ID, their accounts) go to the owner, with exact instructions.
- UX before UI (owner's rule, 2026-11-13): for every surface people use (approver, web panel, setup flow,
  CLI), first write and walk through the flows: who does what, the shortest path, wording, empty, loading,
  error and success states, recovery from mistakes. Styling comes after the flows are clear and tested.
