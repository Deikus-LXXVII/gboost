# Phase 0: facts and spikes

Parent: `docs/plans/gboost-method.md` (approved 2026-11-12), section 10 step 0 and section 12.
Spikes live in `spikes/<name>/`. They are throwaway code that answers one question each; nothing in
`spikes/` is reused as product code without a rewrite.

## Steps

| # | What | Answers | Needs the owner | Status |
|---|---|---|---|---|
| S1 | WASM plugin host: a Rust host (wasmtime, component model) loads a plugin component whose only import is `fetch`, gated by a manifest allow-list. Guests in Rust and TypeScript | F9; P5 "manifest is the sandbox" works in practice; what WASI imports a TS component drags in | No | done: `spikes/wasm-host/FINDINGS.md`. F9 holds: Rust 27 KB; TS 1.5–12 MB, zero WASI with `--disable all`, ms start when precompiled |
| S2 | `docs/security/invariants.md`: principle → test table, the skeleton CI will enforce | Section 8 | No | done |
| S3 | Approver signing: the core issues a digest, the phone signs it with a passkey (user verification each time), the core verifies. Core side in Rust, page served by the core | F8 (passkey per signature), D3 | Yes: open the page on the iPhone, Face ID, report what happened; also whether Web Push from the installed page arrives | built, reviewed twice (OK to run); waiting for the owner. `spikes/approver/README.md` |
| S4 | Grok Bot connector: a hello MCP server on the home box with OAuth, added in Grok as a Remote HTTPS connector, over the Tailscale plugin and over a tunnel; a card in chat; a routine wake | F1–F7 | Yes: add the connector in Grok, run a few prompts, create a routine | built, reviewed twice (OK to run); waiting for the owner. `spikes/mcp-connector/README.md` |

S1 and S2 run now. S3 and S4 are a stop point: the agent prepares everything, then the owner acts.

## Decisions on the way

- Spikes are Rust workspaces separate from `mcp/mindmap` (which stays TypeScript). Product Rust code will
  start in Phase 1 in its own workspace.
- S1: wasmtime 49.0.5 (50 needs rustc 1.97). The origin gate is stricter than WHATWG (non-canonical text is
  refused). TS plugins: StarlingMonkey (12 MB, mature) vs QuickJS (1.5 MB, needs the async component flag)
  is left to the SDK phase; Rust is the first-party default.

- Home box (owner, 2026-11-12): the owner's MacBook, always on, with Tailscale. Tailnet HTTPS works
  (`*.ts.net` certificate). The tailnet root `/` on this node is already served (another app on :3773), so
  spikes use their own HTTPS ports. Funnel is not enabled on this node.
  Caveat to carry into Phase 1: this Mac is also the owner's desktop, where Claude Code and the Grok desktop
  app run (plan 6.1 option C). The product core therefore needs its own macOS account (P2), and Grok desktop
  command access to this Mac stays on "ask every time".
- VPS (vps-usa) is available to the owner over SSH. It is not used in Phase 0. It also runs other services,
  so anything installed there needs the owner's OK first.
- S4 network path, still open: (a) the Tailscale plugin puts the Bot's VM into the tailnet, which needs an ACL
  limiting it to one port on this Mac (otherwise the Bot reaches every tailnet device, the VPS SSH included);
  (b) Funnel gives a public address whose TLS ends on this Mac. Both change the owner's tailnet config.
- S3/S4 review (2026-11-12): first pass found that S4 let anyone mint a token (open `/register`, unguarded
  consent); fixed with DCR off by default and a console PIN on consent. Also fixed: unbounded wakes, log and
  maps (S4), push-endpoint SSRF and stall (S3). Phase 1 notes: global lockouts and rate limits are
  availability nuisances; the S3 inbox is readable by any tailnet device after enrolment.

