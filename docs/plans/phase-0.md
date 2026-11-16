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
| S10 | Tailscale inside GBOOST: every GBOOST process (node core, client on the Bot's VM) runs its own userspace `tailscaled` with its own state, no root, separate from any system Tailscale. The install shows the owner a login link. A small stdio client on the Bot's VM is the one `gboost` connector and forwards MCP to the live node over the tailnet, failing over by itself | One connector without a domain or tailnet admin setup (front door); F18 | Yes: open the login links; the Bot adds the Command connector; a second node for failover | in progress |
| S11 | Updates (plan 6.7): CI builds the core, launcher and client per OS on a version tag and writes a manifest of hashes; the maintainer signs it offline; a node installs side by side, switches, health-checks and rolls back by itself; the client updates from the same release. Includes a deliberately broken release to prove the rollback | 6.7 | Yes: the owner signs the first release on their computer and says OK to the update in chat | planned |

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
- 2026-11-16 (owner): public project, no tailoring to one owner's machines; every install goes through Grok
  Bot from the GitHub link. `INSTALL.md` (root) is the Bot's procedure for the S9 core on Windows, macOS,
  Linux or a VPS. CI runs the tests on all three OSes (green at public commit 0ab2183). Next: the owner
  gives the Bot the link; the live S9 test (does Grok reach a tailnet-only URL) follows that install.
- 2026-11-16, S9 live result: the Bot installed the core from the GitHub link by `INSTALL.md` (macOS host)
  and added the connector itself. Grok's backend reaches a tailnet-only `tailscale serve` URL: the agent
  listener is bound to loopback, so every call came through the tailnet. Full loop worked: first call sent
  the session check to the routine webhook, the routine delivered the code through the same connector, the
  session opened, then `gboost_hello`, `gboost_whoami`, `gboost_pending_request` answered; no errors in either
  log, 11 unauthenticated or probe requests aggregated as noise. Open: auto-start after reboot (not in this
  stage), F7 (whoami from a new chat), whether the platform probes keep coming without the key.
- 2026-11-16, F7 answered (via devlink): a new routine run, a fresh model context with no memory of the last
  run, called `gboost_status` and `gboost_whoami` and got the same client and the same open session with no
  new check. Sessions belong to the client key, not to a chat or context, as designed. Calls reach the core
  from the platform's tailnet node (seen in `x-forwarded-for`), not from the Bot's VM. The Bot refused to
  install on the VPS on an approval relayed by the agent; it asked the owner in its own chat first. This is
  the intended behaviour: the Bot's own guardrail, not a GBOOST one.
- 2026-11-16, S7 research (Grok, Tailscale docs as of 2026-11): Tailscale Services (GA, all plans, host
  v1.86+, clients v1.94.1+) give one name `https://<svc>.<tailnet>.ts.net` advertised by several hosts,
  active-passive (oldest eligible host first), failover about 15 s after `tailscale down`, `tailscale serve
  drain svc:<name>` to move traffic by hand. Limits: hosts must be **tagged** nodes (a user-logged-in
  computer cannot host; tagging it removes the user identity), the service must be defined in the admin
  console or API, access is default-deny and needs a grant, Funnel does not support services. Unknown:
  whether Grok's connector resolves `svc:` names. Plan for S7: GBOOST runs its own tagged Tailscale node
  next to the core on each machine (a second `tailscaled` in userspace mode with its own state, or tsnet),
  so the owner's own login on the computer stays as it is. Needs owner admin steps: a tag, the service, a
  grant, a tagged auth key.
- 2026-11-16 (owner): front door for several nodes. Kept as the default: Tailscale Services (FD2), with
  the owner's own domain (FD3) as an option for owners who have one. A shared domain for all users was
  rejected: its owner could point any user's name at their own server and get a certificate for it, which
  makes one person a party in the middle for everyone. The owner asked to look for options that need no
  extra setup, or setup the Bot can do during install. Candidates under study:
  (a) the Bot moves the one `gboost` connector to the standby's own `ts.net` URL on failover (no tailnet
  or DNS setup; needs the Bot able to edit a connector from a routine run; asked via devlink);
  (b) FD2 with the Bot doing the admin steps in the owner's signed-in browser, the owner only confirming;
  (c) one node needs no front door at all: setup appears only when a second node is added.
- 2026-11-16, candidate (a) ruled out (Bot's answer via devlink): the Bot has no tool to change a
  connector's URL (only status, restart, rename or delete an account, delete a server, custom instructions);
  URLs change only in the Grok app's connector settings; a routine run may not change connectors without
  the owner's OK in chat (platform rule and the Bot's own rule); scheduled routines run at most every 5
  minutes and spend plan usage. So the address must stay fixed and failover happens behind it: FD2, with the
  Bot doing the tailnet admin steps during install while the owner is in the chat to confirm (candidate b).
- 2026-11-16, candidate (d): a small client on the Bot's VM (Command connector) that joins the tailnet
  itself with userspace `tailscaled` and picks the live node. Probe via devlink: the VM (Debian 13 x86_64,
  non-root, PID 1 tini, no systemd, hibernates) reaches `controlplane.tailscale.com` and DERP over HTTPS
  through the platform's transparent egress tunnel. Downloading and starting `tailscaled` was blocked by the
  platform's automatic review without the owner's OK in the Bot's chat. Persistence: no autostart, so the
  Command connector would start `tailscaled` itself, with state on disk (`--statedir`), not in memory.
  Waiting for the owner's OK in the Bot's chat to run the rest (netcheck, whether a login link appears).
- 2026-11-16, candidate (d) probe finished (owner OK in the Bot's chat): static `tailscaled` 1.104.7 runs
  without root in userspace mode with a SOCKS5 listener; netcheck: UDP works, IPv4 only, hard NAT (traffic
  likely through DERP, nearest relay about 5 ms); `tailscale up` prints a login link. Not done: joining
  (needs the owner: interactive login or an auth key). Technically feasible; next is a prototype client.
- 2026-11-16 (owner): updates must be easy, quick and reliable (plan 6.7). Release key on the owner's own
  computer, encrypted, never in CI. Default policy: nothing installs by itself; every update waits for the
  owner's OK in chat. Spike S11 builds this.
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

## S10: Tailscale inside GBOOST (owner, 2026-11-16)

Decision: GBOOST brings its own Tailscale. On each node and in the client on the Bot's VM, GBOOST starts a
userspace `tailscaled` (`--tun=userspace-networking`, own `--statedir` and socket, no root) as a separate
tailnet device named `gboost-<role>-<name>`. It never touches a Tailscale the computer already runs. The
first start prints a login link; the Bot passes it to the owner, who opens it once per device. State is kept
on disk, so restarts and VM hibernation do not need a new login.

- Node: the GBOOST tailnet device publishes the core's agent listener with its own `tailscale serve`
  (`https://gboost-node-<name>.<tailnet>.ts.net/mcp`, tailnet only).
- Client: `gboost_client.py` on the Bot's VM is a Command (stdio) connector. It forwards each MCP message
  to the first live node in its node list through the GBOOST device's SOCKS5 listener, with the client key
  header. If the node does not answer, it moves to the next one and replays `initialize`. The session check
  through the routine webhook works as in S9.
- Why: one connector that never changes, no domain, no tailnet admin setup (the Bot cannot change a
  connector's URL; Tailscale Services need tagged hosts). Cost: one login link per GBOOST device.
- Open: Windows userspace `tailscaled` as a non-admin second instance; macOS has no official static
  build (Homebrew or a build from source); epoch fencing is not in the spike.
