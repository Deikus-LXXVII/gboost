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
| S5 | Android approver: passkey check per signature and Web Push from the installed web app; passkey sync on iPhone and Android; WebAuthn PRF on both (plan 6.5, 4.8) | F13, F14, F15 | Yes: an Android phone and an iPhone; open the approver, Face ID or fingerprint, report push; check whether the passkey appears on a second device of the same account | planned |
| S6 | Phone-only install: a fresh VPS created in the provider's web panel with the install block and an owner-made enrolment code pasted as user data, then enrolment from the Tailscale app's machine list (plan 6.2, D10) | F17; parts of F18 (auth-key join, Funnel from the phone) | Yes: rent a small server (not an existing VPS that runs other services) and do every step from the phone | planned |
| S7 | Two nodes behind **one** connector: one Grok connector reaches whichever node is active, before and after a switch. First FD2 (one Tailscale service name), then FD3 (owner's domain, DNS switched, token scoped to one record) if FD2 fails. Also the approver across two nodes: cached start with the primary down, cross-origin call, push from the second node (plan 6.6) | F18, F19, F16 | Yes: the second node (the S6 server can serve), add the connector in Grok and run prompts across a switch; for FD3 a domain and a scoped DNS token. If neither FD2 nor FD3 works: owner stop point (D13) | planned |
| S8 | Windows service under a virtual account with TPM sealing, the WASM host and Tailscale serve; Grok desktop command access on Windows and Linux; a macOS daemon using a Secure Enclave key with nobody logged in (plan 6.5) | F20, F21 | Yes: a Windows machine; the Mac admin password in the system dialog for the daemon test | planned |

S1 and S2 run now. S3 to S8 are stop points: the agent prepares everything, then the owner acts.

## Decisions on the way

- Spikes are Rust workspaces separate from `mcp/mindmap` (which stays TypeScript). Product Rust code will
  start in Phase 1 in its own workspace.
- S1: wasmtime 49.0.5 (50 needs rustc 1.97). The origin gate is stricter than WHATWG (non-canonical text is
  refused). TS plugins: StarlingMonkey (12 MB, mature) vs QuickJS (1.5 MB, needs the async component flag)
  is left to the SDK phase; Rust is the first-party default.

- Home box (owner, 2026-11-12): an always-on macOS computer with Tailscale. Tailnet HTTPS works
  (`*.ts.net` certificate). The tailnet root `/` on this node was already in use by another app, so
  spikes use their own HTTPS ports. Funnel is not enabled on this node.
  Caveat to carry into Phase 1: this computer is also the owner's desktop, where Claude Code and the Grok desktop
  app run (plan 6.1 option C). The product core therefore needs its own OS account (P2), and Grok desktop
  command access to this computer stays on "ask every time".
- A VPS is available to the owner over SSH. It is not used in Phase 0. It also runs other services,
  so anything installed there needs the owner's OK first.
- S4 network path, still open: (a) the Tailscale plugin puts the Bot's VM into the tailnet, which needs an ACL
  limiting it to one port on this computer (otherwise the Bot reaches every tailnet device, the VPS SSH included);
  (b) Funnel gives a public address whose TLS ends on this computer. Both change the owner's tailnet config.
- S3/S4 review (2026-11-12): first pass found that S4 let anyone mint a token (open `/register`, unguarded
  consent); fixed with DCR off by default and a console PIN on consent. Also fixed: unbounded wakes, log and
  maps (S4), push-endpoint SSRF and stall (S3). Phase 1 notes: global lockouts and rate limits are
  availability nuisances; the S3 inbox is readable by any tailnet device after enrolment.

- 2026-11-13 (owner): renamed to GBOOST (Grok Bot Operations & Superpowers Toolkit); install is "give the Bot
  the repository link" (plan 6.2, D8). Vault folder renamed to `GBOOST/` in the owner's Obsidian vault.
- 2026-11-13 (owner): public repository https://github.com/Deikus-LXXVII/gboost, started from a fresh
  single commit (author: GitHub noreply) so the local history, which holds the owner's email and network
  addresses, stays private. Until this repository moves to GitHub as its origin, updates are exported the
  same way (`git archive` into the public clone), with a scan for private data before each push.
- 2026-11-13 (owner, second round): plan revised for phone first, platforms and nodes (merged from
  `gboost-nodes-phone.md`; plan D10 to D15). Phone-only install by an owner-pasted cloud-init block and an
  owner-made enrolment code; the Bot never logs in there. The owner's own nodes: the Bot installs the primary
  node through Grok desktop and a standby VPS through the owner's root SSH key from the primary computer
  (accepted risk: a Bot with command access there can reach root on the VPS). All state encrypted on every
  node under a key the owner's passkey releases; limited mode automatic, full takeover on one biometric approval.
  **One connector** for all nodes (fixed requirement): FD2, else FD3; S7 must prove one of them. Recovery
  by passkey sync plus a second authenticator. Phases renumbered: nodes are phase 4, SDK phase 5. H3'
  (non-root SSH installer) withdrawn.

## S4 results (2026-11-13, owner ran the prompts in Grok)

- **F1 holds.** The connector reached the server on the test host through Funnel. The OAuth flow ran: discovery
  from the Cursor backend (AWS addresses, `Cursor/1.0.0`), consent on the host with the PIN, then
  `/token` from `undici`. `hello`, `whoami` and `pending_request` worked.
- **F10, partly answered.** Grok acts as a public PKCE client: it sends no client secret at `/token`. The
  spike now allows that only for the exact redirect `https://www.cursor.com/agents/mcp/oauth/callback`.
  DCR was not tried.
- **F2 looks false (to confirm).** Discovery, `initialize` and `tools/list` came from the Cursor backend.
  The three `tools/call` requests came from the tailnet node `grokbot-user-<id>` (the Bot's VM, joined
  through the Grok Tailscale plugin under the owner's login), and each carried the bearer token. So the
  access token reaches the VM, at least in transit, and the Bot, being root there, can probably read it.
  Plan consequence (section 12): P1 still holds, since the token carries only the Bot's authority. The
  token must be short-lived and bound to a budget. Next step: check whether a token read on the VM works
  from outside, and whether the VM keeps it.
- **F3: the Bot's VM is in the owner's tailnet with the owner's identity.** Tailscale serve tagged its
  requests with the owner's login. Without an ACL, it can reach every tailnet device (the host's other
  services, the VPS SSH). An ACL that limits it to the GBOOST port is needed (owner decision: tailnet
  config).
- **F4, partial.** `pending_request` returned the `ui://` card resource. The chat the owner used did not
  draw cards from connectors, so the Bot showed the link instead. Rendering is still unchecked.
- **F7, open.** On connect the backend opened four MCP sessions; all three tool calls used one of them.
  Still to do: `whoami` from a new chat, to see whether a new chat gets a new session or token.

## S9: GBOOST client on the Bot's VM, identified through the routine webhook (owner, 2026-11-13)

Decision D16 replaces the Remote HTTPS connector, OAuth and Funnel on the agent path. The Bot reaches GBOOST
through `gboost-client`, a local Command (stdio) connector on its VM. The client talks to the active node
over the tailnet, which the VM is already part of, and picks the node itself, so there is one connector by
construction.

Identity, in two parts:
- A client key made on the VM and enrolled once by the owner. In the spike, enrolment is approved on the
  owner console; in the product it is a T4 approval on the phone that shows the key's fingerprint.
- A session opened only after a webhook check. The core POSTs a random code to the owner's routine
  webhook. Only xAI can deliver it, and only to this owner's Bot. The routine run hands the code to the
  client (tool `gboost_deliver_code`), and the client trades it for a short session.

The webhook key lives only in the core's state; the Bot never holds it. This stops a stolen client key from
being used anywhere except through the owner's own Bot. It does not stop an injected Bot; biometric
approvals still guard T3 and T4.

Spike layout, `spikes/bot-client/`:
- `core/`: Rust and axum. The agent listener binds an address given by the owner (the host's tailnet IP,
  plain HTTP over WireGuard, no Tailscale config change). A separate owner listener on 127.0.0.1 carries a
  console token. Endpoints: enrol, session start and finish, tool calls, and the client file download.
- `client/gboost_client.py`: Python 3 standard library only, so the Bot can install it with no toolchain.
  It is a stdio MCP server with tools `gboost_hello`, `gboost_whoami`, `gboost_pending_request`,
  `gboost_deliver_code` and `gboost_status`.
- An end-to-end script that drives the client like the Bot would, with a fake routine receiver standing in
  for xAI.

Facts this answers (plan section 12): whether Grok runs a Command connector and draws its cards; whether a
routine run can reach the local client; the cost and delay of one check.
- S9, first finding (2026-11-13): the Bot's VM cannot open plain TCP to tailnet IPs (`100.x`). Over the
  tailnet it reaches only HTTPS names that `tailscale serve` provides, which also matches the S4 tool calls.
  So the agent listener binds to loopback behind `tailscale serve --https=<port>`, tailnet only, not Funnel.
  The client's `core_url` is `https://<host>.<tailnet>.ts.net:<port>`. Product consequence: each node
  publishes its agent listener through `tailscale serve`.
- S9, second finding (2026-11-13; this corrects the F2 reading in "S4 results"): the Bot's VM is not in the
  tailnet. It has no `tailscale` and sends all egress through a platform proxy on a local loopback port, which
  cannot reach tailnet names. The node `grokbot-user-<id>` belongs to the Grok platform and lives outside
  the VM; Remote URL connectors run there. So in S4 the bearer token was used by the platform service, not
  on the VM, and F2 probably holds after all. Consequence for D16: a Command client on the VM reaches the
  core only through a public address (Funnel). Over the tailnet, only a Remote connector can reach it.
  Owner decision needed.
- S9, owner decision (2026-11-13): use a **Remote connector over the tailnet** (URL on `tailscale serve`, no
  Funnel). The local Command client is dropped. Auth: a client key the owner creates on the owner side and
  types into the connector's header field in Grok, so it stays on the platform. A server-side session is
  opened per client key only after a webhook code: the routine run calls `gboost_deliver_code` through the
  same connector. No OAuth. Open question to test: does the Grok backend reach a tailnet-only URL for
  `initialize` and `tools/list` (in S4 those came from public AWS addresses)?
