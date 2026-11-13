# GBOOST

## Working mode: semi-autonomous (owner's decision, 2026-11-12)

Work through the current plan without asking about each step. Make the ordinary choices yourself (names,
formats, library versions, test layout, fixing review findings, local commits), record them in the plan in
`docs/plans/`, and keep going until a real stop point. Then report once: what was done, what was decided on
the way, and what needs the owner.

Stop and ask the owner only when:
- the action is outward-facing or hard to undo: push, PR, publishing, sending to external services,
  deleting data the owner made, touching files outside this repo and the vault `lxxvii-obsidian/GBOOST/`;
- it is a security or trust decision (secrets, permissions, what agents may do without approval);
- it is a product choice with no sensible default, or the plan's premise turns out wrong;
- the owner must act or look (log in, a device, a visual check only they can judge);
- a check fails in a way you cannot fix after two honest attempts.

Map changes in the Obsidian vault need no approval (see `docs/plans/mindmap-mcp.md`).

## Conventions

- Plans: `docs/plans/<topic>.md`, updated as work goes.
- Every change: tests and typecheck green, a reviewer pass, then a local commit on a branch (not main
  after the initial commit).
