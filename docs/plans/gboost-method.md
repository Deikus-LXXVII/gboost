# GBOOST method: how GBOOST works

Status: **approved by the owner** (2026-11-12), including every recommendation in section 11; D8 and D9
added 2026-11-13. **Merged revision 2026-11-13:** phone first, platforms, nodes and failover (P1, P2, P10,
P12 amended, P13 new, 6.5, 6.6, phases, D10 to D15, F13 to F22), from the draft `gboost-nodes-phone.md`
(Fable 5.2) after the owner's decisions of 2026-11-13; merged by Opus 5.5. Nothing is built yet. Phase 0
(section 10) is next; section 12 facts can still reopen D2, D3 and D16. **D16 (2026-11-13):** the Bot
reaches GBOOST through `gboost-client` on its VM; it supersedes D13 (6.1, 6.6, S9). Drafted by Fable 5.2
(high effort), reviewed by Opus 5.5: audit ids checked against the audit; section 4.8 and the D2/D3 note added.
Revised 2026-11-12 after the owner's correction: GBOOST exists to extend Grok Bot.
Renamed 2026-11-13 (owner): Bastion → **GBOOST, Grok Bot Operations & Superpowers Toolkit**. Map ids
(`bastion-context`) and spike code under `spikes/` keep the old name. The vault folder is `GBOOST/` in the owner's Obsidian vault (renamed the same day).

Inputs: the Rubi audit of 2026-11-12, Rubi's design notes, `docs/maps/bastion-context.md`, and a Grok
web-research report of 2026-11-12 on Grok Bot (docs.x.ai pages dated 9 to 11 November). Rubi is a source of
lessons only. No Rubi code is reused.

Not checked: statements marked "(xAI docs)" come from that report, not from a test. Claims about tool
versions and phone behaviour come from memory. Section 12 lists what Phase 0 must verify.

Reading: sections 1 to 8 and 10 to 12 are the document. Section 9 is a lookup table.

## 1. GBOOST in short

GBOOST extends what **Grok Bot** can do for its owner: mail, calendar, notes, smart home, messengers, code.
The owner talks to Grok Bot in the Grok app. Grok Bot calls GBOOST's tools through `gboost-client`, a local
Command (stdio) connector on the Bot's VM that talks to the active node over the owner's tailnet (D16). A
plugin turns each call into a typed effect. The **core** decides, shows, executes and logs. The owner
approves, in GBOOST's own approver on the phone, only what cannot be undone.

GBOOST is a public project for any Grok Bot user. Each owner runs their own core. The project runs no
servers.

| Term | Meaning |
|---|---|
| Grok Bot | The agent and the owner's main surface. Untrusted and fallible |
| Core | The trusted part: policy, approvals, vault, effect executors, plugin host, audit log |
| Plugin | Untrusted, sandboxed code that maps tool calls to effects and service data to typed records |
| Effect | A typed description of one change (`mail.send`, `file.write`), from a closed set the core defines |
| Adapter | First-party native code, signed with the core, for what a sandbox cannot reach (Apple Shortcuts) |
| Approver | GBOOST's app on the owner's phone. Holds the only keys that can approve |
| Grant | A scoped, expiring permission the owner gave to the Bot or a plugin |

Choices this draft makes (the owner can overturn any of them):

1. The core never runs on Grok Bot's computer (6.1).
2. Chat is for asking and showing. Approval happens only in GBOOST's approver.
3. Plugins compute, the core acts. Plugins are WASM components and never perform a write.
4. Approval follows reversibility. Undoable actions run; irreversible ones ask.
5. One kind of owner authority: a signature from the phone over an action digest.
6. No model sits in the trust path. No LLM classifies risk or writes approval text.

## 2. Threat model

**Assets.** A1 the owner's content. A2 service credentials. A3 the power to act as the owner (send, delete,
unlock a door, push code). A4 approval keys and the audit log. A5 availability: the owner can always see,
stop and recover. A6 GBOOST's own release and catalog channel.

**Grok Bot's computer is agent territory.** Per xAI docs the Bot runs on a hosted cloud VM (run by Cursor):
one per user, shared by all of that user's Bots, with sudo and open egress by default. There is no
self-hosted runner. Anything placed there (a stdio MCP server, files, Grok Vault secrets) is readable and
writable by the Bot. Rubi's test in October found Vault secrets in plaintext on the VM
(`box-secrets.json`), reusable by the Bot without the owner. `gboost-client` (D16) lives there too, with its
client key and session: they carry only the Bot's own authority, so the Bot reading them changes nothing. So:

1. Nothing GBOOST trusts lives on that VM.
2. No GBOOST secret passes through Grok's Vault or through chat.
3. What the Bot has read, it can send anywhere. GBOOST cannot contain a leak after the read; it can only
   limit the read (P7).

| Actor | Trust | What we assume |
|---|---|---|
| Owner | Trusted, fallible | Tired, in a hurry, can be phished, taps through a prompt that looks routine |
| Grok Bot | Untrusted | Well-meaning but wrong, or steered by injected text in mail, web pages and notes. Root on its VM, other connectors, memory across tasks |
| Grok platform | Honest, outside our control | Model, VM host, routines. Sees every tool call and result; delivers the routine webhook only to this owner's Bot |
| Plugin | Untrusted | Author careless at best, malicious at worst. Updates can turn hostile |
| External attacker | Hostile | Network, stolen links, stolen phone, look-alike pages, poisoned dependencies and CI |
| Builders | Fallible | The team and its coding agents, which can be injected like any other |

| # | Trust boundary | Enforcement point |
|---|---|---|
| B1 | Grok Bot (through `gboost-client`) ↔ core | Agent gateway: enrolled client key, webhook-checked session, strict parse, grants, budgets, read filter |
| B2 | Core ↔ plugin | WASM sandbox with host functions from the manifest |
| B3 | Core ↔ outside services | Egress proxy and effect executors. Credentials are attached here |
| B4 | Core ↔ owner's phone | Owner API: requests signed by an enrolled device key |
| B5 | Grok Bot ↔ everything else it reaches | Not ours. Limited only by what B1 lets the Bot read |
| B6 | Source ↔ release | Build without secrets, offline signing, pinned verification |

**Out of scope, and said so to the owner.** Root on the core's host. A compromised phone OS. What Grok Bot
does with tools that are not GBOOST's: its shell, other connectors, desktop commands through the Grok app.

**Accepted, by owner rule (`CLAUDE.md`, 2026-11-13).** GBOOST never restricts the Bot's own capabilities.
Through the Grok Tailscale plugin the Bot's VM is in the owner's tailnet under the owner's login, with full
reach to every device on it (S4); no ACL limits it. GBOOST's guarantees must hold with that reach. Hence
Tailscale identity headers (they name the owner for the Bot's requests too) and source addresses never
count as identity or grant anything: the core knows the Bot only by its enrolled client key and session.

## 3. Principles

Each principle is a rule the code enforces and a test proves (section 8).

- **P1. No agent-mintable authority.** Owner authority is a signature from a passkey on an enrolled owner
  device over an action digest. Nothing else approves. A passkey synced by the platform (iCloud Keychain,
  Google Password Manager) puts that platform account in the trust base; a device-bound FIDO2 key is the
  stricter choice (D14). No Bot-facing response carries a secret, ticket or link that grants anything. A
  chat card is display only. The Bot cannot approve, decline or cancel any request. (Rubi audit C1, C11,
  P-M3)
- **P2. The core shares nothing with the Bot.** It runs only on machines the Bot has never had a login on,
  and it has its own OS account. No trust decision reads a file, variable or path the Bot can write. The
  owner API is a separate listener. Unlock never starts from a link or a typed password: the phone pins a
  signed node list (6.6) and talks only to the node identities in it. Local recovery goes through the
  **owner channel**: a path to a node the Bot cannot use (its local screen, the VPS provider's web panel,
  the owner's own SSH session). The owner's VPS node is a recorded exception (6.2, D10).
  (Rubi audit C2, C3, C4, C27, C28)
- **P3. What the owner sees is what executes.** The core draws the approval from the effect, the phone signs
  the digest of what it drew, and the core executes that effect itself. Plugin code writes no approval text
  and is not on the execution path. A value too long to show in full is refused, not clipped.
  (Rubi audit C10, P-M1, P-A2, D-H1 to D-H3)
- **P4. The tier is derived, not claimed.** Risk tier comes from the effect type and its targets, by a rule
  table in the core. A manifest can raise a tier; it can lower one only with an inverse the conformance
  tests prove (5.2). Unknown means highest. No model or plugin judges risk at run time. (Rubi audit C18, 3.7)
- **P5. Capabilities only; the manifest is the sandbox configuration.** A plugin has no file, network,
  environment, process or secret access beyond the host functions its manifest names. The install screen is
  generated from the same manifest, so nothing is shown that is not enforced. (Rubi audit C5, C16, C19)
- **P6. One strict parse; identity out of band.** The core decodes each inbound message once against a
  schema. Unknown fields, duplicate keys and case variants are errors. Caller and account are set by the
  core from the connection, never by arguments. (Rubi audit C6, P-X2)
- **P7. The Bot reads the minimum.** This is the confidentiality boundary, because the Bot's VM has open
  egress. Reads need a grant (account, scope, expiry). Results are metadata by default; bodies are counted.
  A core filter withholds sensitive items in every representation, and search does not match withheld
  fields. Secrets are never returned. (Rubi audit P-M2, P-M4, 3.5, 3.9)
- **P8. Untrusted content lowers autonomy.** Outside text reaches the Bot in a labelled wrapper. A task that
  has read any loses its standing grants until it ends. This is not a boundary: the Bot remembers across
  tasks. The boundary for actions is P1 plus T3.
- **P9. Authenticate first; budgets per actor; owner controls never starve.** No state is kept for an
  unauthenticated peer. Stop, decline, undo and revoke have no budget. (Rubi audit C11, C12, C15)
- **P10. Trust roots are offline or in hardware, pinned, one per role.** Release, catalog and revocation
  have different keys and domain-separated signatures. The release key is never in CI. The approver's code
  comes from the owner's signed core, not a public project site, and only the primary node serves it.
  Versions only go up. (Rubi audit C7, C8, C9, C24, C26)
- **P11. Each security mechanism exists once, in the core.** Approvals, filters, watches, grants and limits
  are core code that plugins cannot replace or skip. No guarantee rests on Grok's Ask-first rules, tool
  switches, Vault or skill text. (Rubi audit C16, C23, P-X2, 3.3)
- **P12. No silent widening; grants expire; failure means refusal.** Anything that lets the Bot or a plugin
  do more is a T4 action shown as a diff. Every grant has an end date and a use cap. Policy has one store,
  read at use. If a check cannot run, the action is refused. On a standby there is no widening at all:
  policy has one writer, the primary. (Rubi audit C4, C20, C25, P-A4)
- **P13. Nodes are enrolled, scoped and fenced.** A node exists only through an owner-signed descriptor. It
  receives only the state its role and scope need. Exactly one node executes per epoch. Taking the active
  role needs the owner's signature; giving it up needs the holder's. No node holds the state key at rest:
  the key is released by the owner's passkey and kept in memory only (6.6).

## 4. Actions and approvals

### 4.1 Life of an action

```
Grok Bot ── tool call ──▶ core: client session, strict parse, grant, budget        (B1)
                          plugin.prepare(params) ─▶ Effect                        (B2, no write capability)
                          core: tier(effect)
                            T0/T1 run now · T2 batch · T3 phone · T4 step-up
                          chat: card "waiting for you in GBOOST" (display only)
                          phone: draws the effect, signs digest(effect, context)  (B4)
                          core: verify ─▶ hold window ─▶ execute ─▶ log, keep undo (B3)
```

`context` is caller, plugin and version, account, node id, epoch (6.6), nonce and expiry. An approval made
for one node or epoch is void on another. An approval past its expiry is void; a
queued item that is overdue needs a new approval (Rubi audit P-M6).

### 4.2 Tiers

| Tier | What it covers | Examples | Path | Proof |
|---|---|---|---|---|
| T0 Read | No change. Data goes to the Bot | Search mail, read calendar | Auto inside a read grant | None; logged |
| T1 Reversible, private | Change in the owner's own data, with undo | Draft, label, archive, new note, trash | Auto inside budget; in the feed | None; undo for 30 days |
| T2 Reversible, heavy | Bulk T1 (over 20 items); overwriting what the owner wrote | Archive 400 mails, edit the owner's note | Batched: one card per task | Tap, device key |
| T3 Irreversible or outward | Leaves the owner's control or cannot be undone | Send, invite, post, permanent delete, push, pay | Per action, shown in full | Biometric, device key |
| T4 Control and safety | Changes what GBOOST allows, or physical safety | Install a plugin, add a grant, connect the Bot or a phone, add a node, take over, door lock | Step-up | Fresh biometric, never batched, cool-off for widenings |

T4 actions for nodes and owner devices (6.6):

| T4 action | Shown as | Cool-off |
|---|---|---|
| Add a node | Node descriptor in full | Yes |
| Change a node's role or accounts in scope | Diff | Yes when widening |
| Take over (full mode on the standby) | Which node, which accounts, what its host can then read | No |
| Promote the standby for good; remove a node | Descriptor; credentials to revoke | No (narrowing or recovery) |
| Enrol or remove an approver device; restore from backup | Device or backup details | Yes for enrol and restore |

Hand-back and entering limited mode are not owner actions: both narrow. Each appears in the feed.

Numbers here and below are starting values, tunable by the owner. The mind-map exception already in this
repo fits the model: map edits are T1 (private, logged, reversible through git).

### 4.3 The approval screen

Drawn by the core from the effect, in the approver, from one template per effect type:

```
Grok Bot · iCloud Mail · account "personal"     ← from the connection, not from the plugin or the Bot
Send email                                      ← fixed wording per effect type
To: anna@example.com    Cc: none    Bcc: none
Subject: Friday
<full body; quoted text, HTML differences and each attachment listed>
Cannot be undone after 20 s.
Grok Bot's note (unverified): "Answering Anna's question"
[ Decline ]                     [ Send · biometric ]
```

Rules: every signed field is visible in full. The primary button enables at the end of the content. Decline
is a full-size button. Bot text appears only under its own label. Refresh and Back lose nothing: the inbox
is the approver's home and requests live in the core (Rubi audit D-H4).

### 4.4 Keeping prompts few

- **T0 and T1 never prompt.** Most daily actions land here. The owner gets a feed, not a question.
- **One card per task.** T2 items share one tap. Up to five T3 items share one screen, each shown in full
  with its own switch; one biometric signs the list of digests. T4 is never batched.
- **Standing grants.** The owner pre-approves a narrow pattern: effect type plus typed constraints on its
  fields ("mail.send where every recipient is in {me, Anna}, at most 5 a day, until 1 January"). Never
  constraints on free text. Creating a grant is T4. T4 actions cannot be granted.
- **Notices with no content.** Two channels, neither a project server: the chat card (6.2) and Web Push
  sent by the core straight to the phone. Both say only that something waits.
- **A fatigue budget.** Target: under ten approver prompts on a normal day. The core counts them and shows
  weekly which kinds caused the load.

### 4.5 Undo and quarantine

- Every T1 and T2 effect type has an inverse the core runs itself. The core keeps the prior state, encrypted,
  for 30 days. Undo is one tap in the feed.
- No effect deletes directly. "Delete" moves to a trash. Permanent delete is T3.
- Approved T3 effects wait in an outbox: 20 seconds when the owner just approved, 10 minutes with a notice
  when a standing grant approved it. Cancel is one tap.

### 4.6 Budgets, limits, stop

- Budgets per day: body reads, T1 operations, T3 requests, wakes of the Bot; at most five requests pending
  in the approver. Per plugin: calls and egress bytes. An exhausted budget pauses that tier and tells the
  owner. Three declines in a row pause the Bot until the owner resumes it.
- **Pause Grok Bot** is on the approver's first screen and through the owner channel. It has no budget.

### 4.7 Audit log

Append-only and hash-chained. One entry per call, decision, execution and undo: caller, plugin and version,
effect type, digest, tier, how it was allowed (auto, grant id, owner proof), result. It holds digests, not
content. The chain head is signed and sent to the phone daily, so truncation shows. The Bot can read only
the state of its own requests.

### 4.8 Secrets and data at rest

- Service credentials live only in the core's vault. They are typed in the approver and sent to the core
  encrypted to its pinned node key; no agent, plugin or log ever sees them (P5, Bot rule 5). None is typed
  into chat or put in Grok's Vault.
- **State key (owner decision 2026-11-13).** The vault, policy, undo journal, plugin storage and audit log
  are encrypted under one state key, on every node. The key is never at rest: the owner's passkey releases
  it (WebAuthn PRF, F15) and the node keeps it in memory only. A node therefore needs one biometric approval after each
  reboot; until then it runs in limited mode (6.6). The host cannot read the data at rest, in snapshots or
  in backups; while a node runs unlocked, its host can read process memory. The node's own key pair is
  sealed to the hardware (TPM or Secure Enclave) where there is one (6.5). Plugin storage is per plugin;
  uninstall deletes it after the undo window.
- Backups are encrypted to a recovery key the owner keeps offline (printed or in a password manager). A
  backup without that key is useless. Restore is T4: install a fresh node, enrol, then paste the recovery
  key in the approver, encrypted to the node's key. It decrypts data; it approves nothing.
- **Recovery of the approval key (D14).** R1: platform passkey sync, so a new phone on the same Apple or
  Google account has the passkey (F14); this puts that account in the trust base. R2: a second
  authenticator enrolled as T4 (a second phone or tablet, or a FIDO2 key used over NFC or USB-C), offered at
  setup and the only choice for an owner who turns sync off. No typed recovery code: it would be a second
  kind of authority (P1). Last resort: the owner channel.
- Nothing about the owner's content goes to GBOOST's builders: no telemetry, crash reports carry no content
  and are off by default.

## 5. Plugins

A plugin extends what Grok Bot can do. Each tool in its manifest becomes a connector tool the Bot can call.

### 5.1 Isolation

| Option | Isolation | Manifest enforced | For authors | Cost |
|---|---|---|---|---|
| WASM components | Strong by default: every call is an import | Yes, structurally | Typed bindings; Rust, TypeScript, Python, Go. No native libraries | Young toolchain outside Rust |
| Native process under an OS sandbox | Good when the profile is right; one per OS | Only with a generated profile plus an egress proxy | Any language, any library | Per-OS work; hard to test in CI (Rubi audit C19, C20) |
| Deno-style permissions | Medium: one runtime's checks | Yes, by flags | Very easy for TypeScript | One language; large trusted base |

**Recommendation: WASM components for all plugins.** The Grok facts do not change this: the sandbox protects
the owner's box from the plugin, wherever the Bot runs. A component cannot call what it does not import, so
Rubi audit C5 becomes a property of the format. Typed interfaces remove the argument maps behind C6. The
same sandbox runs on every OS, in CI, in dev and in production (C20). The cost: libraries with native parts
will not compile; 5.2 answers that. A native sandboxed process is kept for adapters only: few, first-party,
reviewed as core code.

### 5.2 Plugins compute, the core acts

| Entry | Does | May call |
|---|---|---|
| `read` | Fetches from the service, returns typed records with trust labels | Read-only routes declared in the manifest |
| `prepare` | Turns a tool call into an effect | The same read-only routes; nothing that writes |
| `connect` | Checks a new account | Runs only inside a setup the owner opened in the approver (Rubi audit C17) |

The core owns each effect type: its renderer, tier rule, executor and inverse. First set: `http.request`,
`mail.send`, `mail.move`, `cal.put`, `cal.delete`, `file.write`, `file.move`, `adapter.call`. Credentials are
attached by the executor or the egress proxy; plugin code never holds them. There is no plugin-executed
write. If no effect type fits, the plugin waits until one is added, and adding one is a core change.

A generic `http.request` write is T3. It drops to T1 only when the manifest pairs the route with an inverse
route, the conformance suite proves the round trip, and catalog review confirms it.

The author gets approval screens, chat cards, undo, credential handling, retries and rate limits for free.

### 5.3 Manifest

```toml
id = "icloud-mail"
version = "1.2.0"
api = "gboost:plugin@1"

[net]                                   # enforced by the egress proxy
read  = ["imaps://imap.mail.me.com:993"]
write = ["smtps://smtp.mail.me.com:587"]   # usable by core executors only
[auth]
app_password = { attach = "login", hosts = ["imap.mail.me.com", "smtp.mail.me.com"] }   # never visible to plugin code

[[tool]]
name = "send"
effect = "mail.send"                    # tier comes from the effect: T3
about = "Send an email from the owner's iCloud account."   # the only plugin text the Bot sees (5.6)
[[tool]]
name = "archive"
effect = "mail.move"                    # T1, undo for free
[[tool]]
name = "search"
read = "mail.summary"                   # T0; record fields carry trust labels
```

The core rejects at install: wildcard or public-suffix hosts, private and CGNAT addresses without a `lan`
capability, unknown keys, effects outside the set, and a tier lower than the rule table gives.

### 5.4 SDK and dev loop

Host functions: `fetch` and typed protocol reads such as IMAP and CalDAV (declared read routes only),
`store.get/put`, `log` (redacted), `emit` (typed events). Absent by design: files, environment, sockets,
processes, other plugins, the approval API, any way to set the caller. Goal: a first working plugin in under
150 lines, in TypeScript or Rust.

- `gboost plugin new` scaffolds the manifest, one tool and one test.
- `gboost dev` runs the plugin in the real sandbox against a local core, with a simulated owner that shows
  every approval screen in the terminal. HTTP is recorded and replayed. No Grok Bot is needed.
- `gboost test` runs the conformance suite: schema matches the rendered screen, undo restores state,
  over-long fields are refused, undeclared hosts fail.
- A core accepts a locally signed plugin only in developer mode, which the owner turns on in the approver
  (T4). Sideloading exists only in that mode; the sandbox is the same.

### 5.5 Signing, catalog, updates, revocation

The catalog and the revocation list are signed static files. Nothing else is hosted.

1. `gboost pack` builds and signs the component with the publisher's key, one per plugin (Rubi audit P-X3).
2. The author opens a pull request on the catalog. CI rebuilds from source, compares hashes, runs
   conformance, and prints the permission diff.
3. A human reviews a first version, any T3 or T4 effect, any new host, adapter or read scope, and any change
   to `about` text. Other updates merge after the checks.
4. The catalog is signed with the catalog key (not the release key), with a sequence number and an expiry.
5. On the owner's core, an update that widens nothing installs after a soak period. One that widens
   anything is a T4 approval showing the diff.

**Revocation (Rubi audit C25).** A signed list with its own key, sequence number and expiry, checked daily
and before a plugin starts. A revoked plugin is stopped, its data kept, the owner told. If the list is more
than seven days stale, plugins lose T3 effects until it refreshes.

### 5.6 Text that reaches the Bot

Anything a plugin writes into the Bot's context is an injection channel (Rubi audit C23). Decision: plugins
ship **no skill text**. A plugin may give each tool one `about` line: plain text, at most 300 characters,
part of the signed manifest, shown to the owner at install, a T4 diff when it changes. The Bot cannot write
or edit any of it.

## 6. Grok Bot

### 6.1 Where the core runs

This is the architecture decision (D2).

| Option | How Grok Bot reaches it | Credential, and where it lives | Holds against an injected Bot | Install |
|---|---|---|---|---|
| **A. An always-on box at home** (a desktop or mini PC, NAS, small Linux box), own OS account | `gboost-client` on the Bot's VM, over the tailnet (D16; was: Remote HTTPS connector) | Client key and short session, on the VM, readable by the Bot; the Bot's authority only | Yes | Installer, scan a code, install the client, enrol it |
| B. A VPS the owner rents | Same, over the tailnet; no public address | Same | Yes against the Bot. The VPS host can read data and credentials | Same installer; monthly cost |
| C. The owner's desktop | As A | Same | Only while awake. The Grok desktop app can run commands there | Same |
| D. The Bot's VM (Rubi's choice) | Command connector (stdio) | None: all is local to the Bot | No | Easiest: the Bot installs it |

**What D loses.** The Bot has root beside the core. It can read service credentials in use and act without
GBOOST; read stored mail, notes, the undo journal and everything the filter withheld; rewrite policy,
grants, the audit log and the binary; and send all of it out through open egress. The phone's signature
still verifies, but it guards nothing once the credentials are readable. Rubi's threat model set aside a
Bot that goes looking; the owner's threat (b) is that Bot. D is not offered, even as a trial mode.

**Recommendation: A for this owner; B supported from the same build** for users with no always-on machine.
The rule in one line: the core runs on a machine Grok Bot cannot log in to.

**Phone-only owner.** An owner with only a phone has no placement A: the only core is B, and the VPS host
can read what runs in memory there (at rest the state is encrypted, 4.8). The install says so (6.2).

**Nodes.** An owner may run more than one core (6.6). The usual shape is A as the primary and a VPS (B) as
the standby. A typical case: a computer at home as primary, a rented VPS as standby.

**Network path (D16).** Grok Bot calls `gboost-client`, a local Command (stdio) connector on its VM. The
client talks to the active node's agent listener over the tailnet: the VM joins it through Grok's Tailscale
plugin under the owner's login (S4, F3). The agent path needs no public address, no Remote HTTPS connector,
no OAuth and no Funnel. The client picks the active node itself (6.6), so there is one connector by
construction.

**What Grok Bot holds.** The client key and the current session, both on the VM and readable by the Bot
(S4 settled F2 the same way for the old path: the bearer token reached the VM on every tool call). Both
carry the Bot's authority only: read inside grants, propose effects. Identity has two parts:

- **Client key.** Made by the client on the VM and enrolled once by the owner: a T4 approval on the phone
  that shows the key's fingerprint. The owner removes it in the approver.
- **Session.** Opened only after a webhook check. On a start request signed by the enrolled key, the core
  POSTs a random code to the owner's routine webhook; the routine run hands it to the client
  (`gboost_deliver_code`); the client trades it, signed with its key, for a session. The code is single
  use, short-lived and bound to the key that asked for it. Sessions are short (hours), budgeted and
  revocable from the phone. Checks run at session start and every few hours, never per call, and have
  their own budget, since each costs a routine run (F24).

The webhook key lives only in the core's vault: the Bot and the VM never hold it. A stolen client key is
therefore useless except through the owner's own Bot. Limits: this does not stop an injected Bot (approvals
do, P1); a Bot can relay a code to another process, which gains only a short session for the same key. All
Bots of one owner share a VM, so one client is one caller.

### 6.2 Chat-first use

**Setup (owner decision 2026-11-13: install from a link).** The owner sends Grok Bot the link to the GBOOST
GitHub repository and says "install it". The Bot does the whole install and turns to the owner only at the
steps a person must do, or when a step still fails after one retry. This replaces the 2026-11-12 version (fixed command,
owner pastes the connector URL).

1. **The repository is the install guide.** `INSTALL.md` is written for an agent: preflight checks, exact
   commands, the expected output of each, how to recover from each known failure, and marked **owner
   steps** where the agent must stop, say what is needed, and wait.
   The Bot reads it from the latest signed release tag, not the default branch, and only from the exact
   repository URL the owner gave. These rules are instructions; the release signature is the real check.
2. **Where it installs.** The Bot asks which machine, and installs on the owner's always-on machine through
   the Grok desktop app's command access (F11). The core never goes on the Bot's VM (D2; only the client
   does, 3a): `INSTALL.md` says so and the core installer checks for the Grok VM and exits. Both are
   advisory (the Bot controls its VM); D2 rests on the owner naming the right machine. If the owner has only a phone, the Bot switches to the phone-only path
   below. A Bot-run VPS install over SSH is not offered as a product path (Bot as root cannot be proven gone
   afterwards); the owner's own setup is a recorded exception (below).
3. **What it runs.** A short bootstrap from the repository's latest signed release. The bootstrap checks the
   release signature against the key embedded in it and refuses anything else, so a mangled or injected
   command fails instead of installing other code. The installer then creates the core's own service account (a hidden user on macOS, Linux or Windows),
   installs the service, sets up HTTPS for the approver (Tailscale serve), binds the agent listener to the
   node's tailnet address, and prints only progress text to the Bot.
3a. **The client (D16).** The Bot installs `gboost-client` on its own VM (the core serves the file from the
   signed release; the core trusts nothing in it) and adds it in Grok as a Command connector (F25). The
   client makes its key and asks the core to enrol it; the client prints the key's fingerprint in chat.
4. **Owner steps** (the Bot asks, gives a link or says where to look, and waits):
   - the OS admin password, typed into the computer's own system dialog, never into chat (needed once, to
     create the core's account);
   - Tailscale sign-in, and adding Grok's Tailscale plugin so the Bot's VM joins the tailnet (Funnel only
     if the owner API is made public, D2+D3 note);
   - scanning the QR code shown on the primary computer's screen with the phone: the approver opens, the platform authenticator (Face ID/Touch ID, Windows Hello, Android biometrics) enrols the
     passkey, Add to Home Screen, notifications on;
   - confirming Grok's own prompt that adds the Command connector, if it shows one; then the biometric check on the
     phone to enrol the client key (T4), after checking that the fingerprint matches the one in chat;
   - creating a routine in Grok with the instruction text the approver shows (6.3), and typing its webhook
     URL and key in the approver, never in chat. The first session check runs right after;
   - switching the Grok desktop command access back to "ask every time" (or off) when the install ends.
5. **When something fails.** The Bot shows the failing step and its output, applies the fix `INSTALL.md`
   gives for it, and retries once. Then it stops and asks the owner. It never improvises commands outside
   `INSTALL.md`, never asks for a secret in chat, and never touches the core's account. These are
   instructions to the Bot; what enforces them is the core's separate account and the owner's confirmation.

Why this does not weaken the trust model: the trust anchors stay the release signature, the QR scan and the
phone's biometric check, and the Bot can produce none of them. The Bot gets no owner credential from installing
(the client key it makes carries only its own authority, and only once the owner enrols it): secrets
are generated inside the core's account and never printed (the S4 spike lesson: its console output held the
client secret), and the enrolment code appears only on the Mac's screen and in the QR.

The agent listener answers only on the tailnet; it has no public address (D16). The owner API is a separate
listener (P2). The Bot's tailnet reach is not limited (2, accepted by owner rule), so neither listener
trusts the network: the agent listener needs the client key and a session, the owner API a device-key
signature.

**Enrolment while the Bot has desktop access (review 2026-11-13; owner chose (b) the same day).** A Bot that
can run commands on the Mac, or capture its screen, could read the QR's one-time code and enrol its own
passkey before the owner. Option (a), a `.pkg` the owner opens with Grok denied Screen Recording, was
declined to keep the install fully Bot-driven. Chosen (b): the core accepts exactly one passkey per
enrolment window; the Mac and the phone each show a short code derived from the enrolment and the owner
compares them before the approver is usable; a second enrolment attempt in the window is refused and shows
an alert on the Mac and on the enrolled phone. This detects a Bot that enrolled first (the owner's phone
fails to enrol and the codes do not match); it does not prevent it. Recovery: reinstall through the owner
channel.

Residual risks, stated in `INSTALL.md` for the owner:
- The link is the root of trust for the first install. A look-alike repository brings its own signing key.
  The installer and the approver both show the release key fingerprint; the official README publishes it.
- During install the Bot runs commands on the Mac as the owner. With Grok's "ask every time" the owner sees
  each one; if the owner switches it to automatic for the install, the Bot can run anything as the owner for
  that time. `INSTALL.md` recommends switching it back afterwards, and the core cannot enforce it (6.3).
- The owner's OS account is usually an admin, so a Bot with desktop command access and the admin password could
  reach the core's account later. Hence: the password goes only into the system dialog, never into chat.

**Phone-only install (D10: H1 with an owner-made enrolment code E1).** The Bot still leads from the
repository link but only as a guide: it reads `INSTALL.md`, tells the owner each step, and checks results
from outside. It never holds a login on the host.

1. The Bot asks "which machine?"; "only my phone" selects this path.
2. Owner steps, each opened from a link to a well-known site (provider, Tailscale, GitHub): sign in to
   Tailscale and install its app, create a one-time auth key; make up an **enrolment code** (the phone's
   password generator, at least 20 characters); in the provider's web panel create the smallest server with
   a supported Linux image and paste the install block, copied from the GitHub release page (not from chat),
   into the "user data" field with the two blanks filled: auth key and enrolment code.
3. First boot (cloud-init, F17): the block downloads the bootstrap of one pinned release and checks its hash;
   the bootstrap checks the release signature (item 3 above). The installer creates the service account,
   joins the tailnet, sets up HTTPS and serves a progress page with fixed text and no secrets.
4. The Bot polls the progress page. The owner opens the node's address from the Tailscale app's machine list,
   not from chat, types the enrolment code, and the biometric check enrols the passkey. Add to Home Screen, notifications
   on. Then the client, its enrolment and the routine, as in items 3a and 4 above.

The code is single use, void after 30 minutes or the first enrolment, never accepted by the agent gateway,
never echoed in a page or log. The egress proxy blocks the cloud metadata address, where the user data stays
readable. Guard (b) on this path: one passkey per window, a second attempt refused and alerted; the code
comparison is dropped because the Bot never has the code. Residual: an owner who pastes the code into chat,
or types it on a look-alike page a hostile Bot linked, gives it away; the real enrolment then fails with an
alert, and the fix is to delete the server and start again. H2 (the owner pastes one command in a phone SSH
app or the provider's web console, code shown by `gboost enrol` in that session) is supported for existing
machines. Rejected: the Bot installing over SSH with a key the owner gives it (root once cannot be proven
gone), the Bot creating the VPS with a provider API token (lasting control of the host), codes sent by
mail or SMS (a sending service the project would run), and "first passkey wins" (unsafe on a reachable
address).

**The owner's own setup (D10, accepted risk).** The Bot installs the primary node through Grok desktop command
access (items 1 to 5 above) and a standby VPS node through the owner's existing root SSH key,
from the primary computer. Accepted risk, recorded: a Bot with command access on the primary computer can use that key, so it can
reach root on the VPS at any time; the owner declined hardware-key protection for the key (a platform authenticator or
`ed25519-sk`). The only guard is Grok's "ask every time". This is an exception to P2 for that node: what
the VPS holds is exposed to the Bot as in 6.1 option D, which for a standby means the clear set always and
the scoped accounts during a failover (6.6). A join code for this node passes through the Bot, so the check
that remains is the address and key fingerprint on the T4 "Add a node" screen, compared with the Tailscale
app's machine list. Phone-only owners keep H1.

**Managing from the phone.** The approver has a **Nodes** screen and a **Manage** screen, so nothing needs
a shell: status of each node, pause, updates, grants, plugins, accounts, backups, takeover and hand-back.
Writing plugins (`gboost dev`, 5.4) needs a computer; it is exempt from the phone-first rule (D15).

After that, "connect my iCloud mail" makes the Bot call `setup`; the owner opens the approver from the card,
types the app password there and approves.

**Daily use.** T0 and T1 calls just work. For T2 to T4 the tool returns a request id and a state, drawn in
chat as an interactive card (xAI docs; whether Grok draws cards from a Command connector is F4): title,
state, expiry and one button, "Open GBOOST", a pointer with no secret. The card is built from what the
Bot itself submitted, never from derived or withheld content (Rubi audit P-M2).

**A card must never approve, decline or cancel.** Chat is the Bot's surface: the Bot can draw or alter a
card. A press returns through the connector with the Bot's authority, so the core cannot tell a person from
the Bot. And the card shows a summary, not the full effect. Rubi's chat-level approval failed this way
(Rubi audit C11, P-M3). The Grok app has no approval API for third parties, so the proof must come from
GBOOST's approver.

**Waking the Bot.** The core POSTs to the owner's routine webhook when a request is decided or a watch
fires. The same webhook carries the session checks of 6.1 (D16). Each run spends plan usage, and the
platform allows 300 runs an hour (xAI docs). Rules:

- Coalesce: one wake per task for decisions, one per window for events ("5 new items"), under a daily cap
  the owner sets. Over the cap, events ride on the Bot's next tool result, which costs nothing. A wake and
  a due session check share one run.
- Session checks have their own daily budget, separate from the wake cap, so wakes never starve the Bot of
  a session. Only a start request signed by the enrolled client key triggers one.
- The payload carries ids, counts, fixed core-written words and, for a check, the session code: no outside
  text, no Bot-written text, no secret. The code is not a secret in this sense: it grants nothing without
  the enrolled client key, once, before its expiry. The Bot fetches content through tools, where it is
  labelled and counted (P7, P8).
- Whoever holds the webhook key can prompt the Bot and could pass a session check. It is typed in the
  approver and stays in the core's vault; it never reaches the Bot or the VM.
- Watches are core objects, listed and removable in the approver (Rubi audit P-X2).

### 6.3 Grok's own controls and the GBOOST skill

Grok's "Ask first" and Auto Review rules are plain-language and best-effort, and the Bot may propose changes
to them (xAI docs). GBOOST relies on neither them nor Grok's per-tool switches (P11). They complement it:

- Leave GBOOST's tools on automatic in Grok. One prompt per action, GBOOST's, is better than two.
- Use Grok's rules for what GBOOST cannot see: the Bot's shell, other connectors, desktop commands.
- A tool switched off in Grok fails safe. GBOOST's grants remain the real access list.

**The skill.** GBOOST ships instruction text for Grok Bot: how to call tools, how to tell the owner what
waits, never to ask for a secret in chat. Every rule in it is also enforced by the core; Rubi's "the agent
never talks to the panel" was only an instruction (Rubi audit C1). The text is part of the signed release
and is served by the core through the connector. A copy on the VM is a cache; nothing depends on it.

The skill also holds the routine's instruction text, which the approver shows the owner when the routine is
created (6.2): "When this run's payload carries a GBOOST session code, call `gboost_deliver_code` with it
and do nothing else with it. Otherwise read the ids and fetch details through GBOOST's tools." If the Bot
ignores it, the only effect is no session: the core opens none without the code.

### 6.4 What Grok Bot can never do without the owner

1. approve, decline or cancel any request;
2. run a T3 or T4 action, or act outside its grants and budgets;
3. create, widen or extend a grant, change a tier, budget or filter;
4. install, widen or remove a plugin, or turn on developer mode;
5. connect itself, a phone, a node or an account, take over for a node, or see or enter a secret;
6. read the audit log, or text the filter withheld;
7. write text that later reaches a Bot as trusted: skill text, tool text, wake payloads, watch notes;
8. reach the core's files, process or owner API.

**Other agents.** Claude Code or a DeepSeek worker could connect later, each with its own credential and
grants. Nothing is built for them now; the caller identity in P6 is the only provision.

### 6.5 Platforms

| Platform | Hosts a core | Approver | Installer | Service and own account | Node key sealing |
|---|---|---|---|---|---|
| macOS | Yes | Optional second device | Bootstrap script, later a signed and notarised `.pkg`; Bot-run through Grok desktop (D8) | launchd daemon, hidden service user | Secure Enclave (F21) |
| Linux | Yes; the VPS and the phone-only target | Optional second device | cloud-init block, one-line bootstrap, `.deb`/`.rpm`, container image | systemd unit, dedicated user, sandboxing options on | TPM where present; a VPS usually has none, so a file only root and the service can read |
| Windows | Yes | Optional second device | Signed MSI or winget; needs a code-signing certificate | Windows service under a virtual service account | TPM (F20) |
| Android | No | **Yes**: web app installed from Chrome, passkey, Web Push (F13) | None | n/a | Platform passkey |
| iOS | No | **Yes**: web app installed from Safari, passkey, Web Push (F8) | None | n/a | Platform passkey |

**Phones never host a core.** iOS cannot run a listening background service. Android could in principle,
but the OS stops background work, the address changes, and core plus approver on one device removes B4: one
compromised phone would hold both the credentials and the approval key.

**Desktops as approver.** A browser with a platform authenticator (Face ID/Touch ID, Windows Hello, Android biometrics) works as a second
device. It is off by default on any machine where the Grok desktop app has command access, because the Bot
could change what that browser shows: View and Pause are allowed there, signing T3 and T4 is not.

**Web app, not native (D3 stands).** One code base covers both phones, with no store and no project push
relay. Reasons to reopen: device-bound keys, code pinned in a store build, more reliable push; each needs a
relay or store accounts the project would have to run.

**An admin on the same machine.** On every desktop OS an administrator can reach the service account. The
rules in 6.2 (admin password only in the system dialog; Grok command access back to "ask every time") apply
to Windows and Linux desktops the same way.

### 6.6 Nodes and failover

**Terms.** A **node** is one installed core, with its own key pair, address and OS account. The **active
node** is the one node allowed to execute effects in the current **epoch**, a counter raised only by a
signed statement. **Limited mode**: a node answers the Bot and the phone but holds no state key and executes
nothing. The **state key** encrypts the vault, journal, policy and the rest of the sealed set (4.8).

**Topology.** Single node (no failover; this section applies once a second node exists), or a primary plus
one standby: a typical case is a computer at home plus a rented VPS. More than two nodes is not planned.

**Replication.** The primary ships an append-only, epoch-tagged log to the standby over the tailnet, on a
channel authenticated by both node keys.

| Class | Contents | Form on the standby |
|---|---|---|
| Clear set | Node list, epoch statements, owner device public keys, the enrolled client public key, push subscription and push sending key | Signed by the primary, readable; limited mode needs it |
| Sealed set | Vault (service credentials, webhook key), policy, grants, budgets, pending requests, outbox, undo journal, plugin storage, audit log | Encrypted under the state key |

**State key on every node (D11).** Every node, primary included, holds the sealed set encrypted and the
state key only in memory after the owner's passkey released it (PRF, F15). A standby gets the key only when
the owner approves a takeover, and drops it at hand-back; the state key rotates at every hand-back. Scope:
the owner marks which accounts are "available on standby"; the others never replicate. Where a service
allows several credentials (app-specific passwords, one OAuth grant per client), the standby gets its own,
typed by the owner in the approver, so a suspect VPS can be cut off at the service without touching the
primary. Rejected: a standby holding the key at all times (its host reads everything, every day).

**Modes (D12).**

| Mode | Entered | Does | Credentials used |
|---|---|---|---|
| Passive | Normal running | Receives the log, answers `status` | None |
| **Limited** | By itself, when the primary misses heartbeats for 5 minutes; also any node after a reboot until unlocked | Tells the Bot "main node offline", queues proposals (encrypted to the primary's key after the strict parse), pushes the owner "Main node offline. Take over?", serves Pause and status | None |
| **Full** | The owner approves takeover (T4, one biometric approval that also releases the state key) | T0 and T1, and T3 with per-action approval, for accounts in scope. No standing grants, no widening, no installs: policy has one writer, the primary | The scoped accounts' credentials, in memory |

A false alarm in limited mode costs nothing: no secret is involved and the primary is not disturbed.
Automatic full takeover is not possible by design: no node holds the state key without the owner. If the
owner is asleep, the Bot gets "main node offline" and a queue, not working tools.

**Epoch statements are the fencing token.** A takeover is `{epoch n+1, active = standby}` signed by the
owner's passkey. A hand-back is `{epoch n+2, active = primary}` signed by the node that holds the role: only
the holder can give it up, so it needs no prompt. Statements authenticate themselves, so anyone may carry
them, the Bot included; withholding one only preserves the old state. Rules:

- Node id and epoch are in every approval's `context` (4.1).
- A node executes only if the highest epoch it has seen names it.
- At takeover the approver first tries the primary itself; if it answers, the screen offers a clean
  hand-over instead.
- A primary that was unreachable past the threshold comes back in limited mode and stays there until it has
  read the current epoch from the standby or the owner's phone.
- The standby never executes anything approved in an earlier epoch. Outbox items the primary may or may not
  have sent show as "state unknown" and need a new approval; they are not resent. Pending approvals are void
  across a failover and asked again.

What is left of split-brain: the primary is online but invisible to both the standby and the phone (for
example a Tailscale outage) and the owner approves takeover. Both nodes then act until the primary sees the
new epoch. Each T3 still needs its own approval naming its node; T1 budgets can be spent twice (undoable);
standing grants run on the primary only, so their caps hold.

**One connector (owner's fixed requirement; D16 supersedes D13).** Grok has one connector for all nodes:
`gboost-client` on the Bot's VM. The client holds the signed node list, asks each node for its status and
the highest epoch it has seen, and sends calls to the node that epoch names; while the primary is down it
reaches the standby in limited mode. The client is the Bot's code, so a wrong choice is possible; it only
reaches a node that refuses to execute (epoch fencing, I13.1) or one in limited mode. Sessions are bound to
the node and epoch that opened them; after a failover the client runs a new check. The webhook key is in
the sealed set, so a standby in limited mode cannot run a check: it accepts only status and queued
proposals, signed by the enrolled client key (in the clear set). A standby in full mode opens sessions
itself. The front-door options FD2 (Tailscale service name) and FD3 (own domain, DNS switch) are no longer
needed, nor is a DNS token on the VPS.

**The approver has one origin: the primary's.** The installed web app starts from its cache when the primary
is down and calls the standby's owner API cross-origin (F16). The standby never serves approver code, so a
VPS host cannot change what the owner sees (P3, P10); it can only send effects, which the unchanged code
draws in full, with the node's name on the identity row. The standby pushes with the replicated sending
key. If F16 fails: a second installed approver per node, or a native app (D3). If the primary is gone for
good: "Promote standby" (T4); the approver is installed again from the new primary's origin and the passkey
re-enrolled there, authorised by a signature from the old one.

**Adding and removing nodes.** Adding a node is T4 with a cool-off. The new node is installed with its own
owner-made join code (as in 6.2). In the approver the owner opens Nodes, Add, and enters the address and the
code. The primary and the new node run a password-authenticated key exchange on that code, which binds
their node keys without the Bot. The approver shows the node descriptor (name, address, key fingerprint,
role, accounts in scope) and signs its digest. Nodes replicate only to nodes in the signed list; the
approver talks only to nodes in it. Removing a node is T4 without cool-off: the primary stops shipping,
rotates the state key, and lists the credentials to revoke at their services.

| Element | Actor and asset | Control | Residual risk |
|---|---|---|---|
| Sealed replica on a VPS | VPS host; A1, A2 | Encrypted at rest; key released per takeover; scope per account | During a failover the host can read the scoped accounts' data and credentials in memory |
| Clear set on a VPS | VPS host; A4, A5 | Public keys and hashes only; signed by the primary | The host learns node names, device count and timing, and can send empty notifications |
| Takeover | Bot or outside attacker; A3 | Owner's passkey signature over the epoch statement | An owner tricked into a needless takeover exposes the scoped accounts to the VPS host |
| Limited mode | VPS host; A1 | No secrets on the node; proposals sealed to the primary | The host sees what the Bot submits during the outage and can feed the Bot false results (B5) |
| Hand-back | Thief holding the home box; A3 | Node key; state key not at rest; "Remove node" fences it | A stolen box that boots comes back in limited mode; it needs a biometric approval to act |
| Split-brain | Fallible builders; A3 | Epoch in context; standing grants on the primary only | T1 budgets can double during a partition |
| Node enrolment | Bot, outside attacker; A4 | Owner-made join code, key exchange, T4 signature on the descriptor | A join code leaked into chat (or, in the owner's setup, passed through the Bot) lets another machine pair; the address and fingerprint on the T4 screen are the last check |
| Approver across nodes | VPS host; A3 | Code only from the primary; signed effects shown in full | None beyond the primary's own |
| Node choice in the client (D16) | Bot, VPS host; A3, A5 | Signed node list; epoch fencing; sessions bound to node and epoch | The Bot can send calls to the wrong node; that node refuses or queues. A VPS host in limited mode sees what the Bot submits (as in the Limited mode row) |
| Client key and session on the VM (D16) | Injected Bot, anyone it relays to; A1, A3 | Webhook check bound to the key; short, budgeted, revocable sessions; grants and approvals | An injected Bot uses the client as it likes inside grants, and can relay a code to another process for one short session of the same key. Tailscale headers and source addresses grant nothing |
| Owner's VPS node (D10) | Bot with Mac command access; A1, A2 | Grok's "ask every time" only | Root on the VPS at any time; during a failover the scoped accounts |
| Recovery by passkey sync (R1) | Outside attacker; A4 | Platform account security | Whoever takes over the Apple or Google account can approve |

## 7. Where the priorities conflict

| Conflict | Resolution | What it costs |
|---|---|---|
| Simplest install (the Bot sets GBOOST up on its own VM) vs P2 | The core runs on a second machine; the Bot installs it from a link, the owner does only the owner steps (6.2) | Every user needs an always-on box or a VPS |
| Chat-first vs approvals the Bot cannot fake | Cards show, the approver decides | One app switch per T2 to T4 action |
| A prompt for everything vs none | Prompt only where undo is impossible; make the rest undoable | Services with no clean undo land in T3 |
| A useful Bot vs minimal reads (P7) | Metadata first, scoped grants, withheld items revealed one by one | The Bot sometimes has to ask |
| Unattended work vs accidents | Standing grants: narrow, typed, capped, expiring, off after untrusted reads | Triage of incoming mail cannot send unattended |
| Timely events vs plan usage and injection | Coalesced, capped, content-free wakes | Events arrive in batches, minutes late |
| Any library, own wording, own prompts vs safe by default | WASM, core effect types, core-drawn screens, one `about` line | Some plugins wait for an effect type; no custom approval UI |
| One phone vs losing it | Passkey sync (R1) plus a second authenticator (R2), 4.8 | The Apple or Google account can approve; without sync, the owner must enrol R2 in advance |
| Phone only vs a Bot-run install | The owner pastes one block in the provider panel (6.2) | One paste with two blanks; the VPS host can read memory (placement B only) |
| Automatic failover vs a VPS that holds every secret | Limited mode is automatic; full takeover needs one biometric approval (6.6) | While the owner sleeps, the Bot gets a queue, not working tools |
| One connector vs no shared party in the middle | `gboost-client` on the Bot's VM picks the active node over the tailnet (6.6, D16) | The client's key and session are readable by the Bot; each session check costs a routine run and some delay (F24) |
| A capable Bot vs a narrow path to GBOOST | Full tailnet reach accepted (owner rule); the core trusts only the client key and session, never network identity (2) | The Bot can reach every tailnet service; each must authenticate on its own |
| State never readable at rest vs unattended restarts | Passkey-released state key on every node (4.8) | One biometric approval after each reboot of any node |
| Phones as hosts | Phones are approvers only (6.5) | A phone-only owner needs a VPS |

What the Rubi audit adds to the owner's list: prompt quality matters more than prompt count (P-M1, D-H1 to
D-H3). Reads are the leak path, and the only place to stop one. Availability is part of security (C11, C12,
C15). The builders are an actor (C7 to C9). Community plugins open only after enforcement exists: six of
Rubi's thirteen first-party plugin repositories were rated High.

## 8. How we build GBOOST

`CLAUDE.md` already sets semi-autonomous mode and the plan, test, review, local-commit loop. This section
adds what a security product needs on top.

**The kernel.** A named list of modules: digest and approval verification, tier rules, grants, agent gateway,
plugin host, egress proxy, effect executors, audit log, update verification. It has the fewest dependencies,
no unreachable code, and stricter gates.

**Invariants.** Each principle has at least one executable test named after it (`p01_…`). A table in
`docs/security/invariants.md` maps principle → tests. CI fails when a principle has none.

**Break-checked security tests.** A security test counts only if it has been seen to fail. For each invariant
we keep a recorded break: a small patch that disables the control. A CI job applies every break and requires
the matching test to go red. Mutation testing runs over kernel modules. A skipped security test fails the
build (Rubi audit C20).

**Property and fuzz focus**, in order of value:

1. Canonical encoding: one parse per input; equal actions give equal digests; the phone's and the core's
   encoders agree on a shared corpus (Rubi audit C29).
2. The approval state machine, model-based: no sequence of Bot calls executes a T3 effect without an owner
   proof over its digest; each approval executes at most once; expiry holds.
3. Grants and undo: removing a grant never allows more; effect then inverse leaves the fixture unchanged.
4. Egress matching: IDN, trailing dots, address literals, CGNAT, redirects, DNS rebinding (Rubi audit C13, C19).
5. Bot-facing output: no card, wake payload or tool result carries a secret or withheld text.

**Hostile-Bot suite.** A scripted caller replays every High scenario from the Rubi audit against the agent
gateway, and must fail each time. A nightly run gives a real model fake accounts and an injection corpus.

| Review gate | When | Who |
|---|---|---|
| G1 Boundary note | A change adds an input, effect type, host function or authority | Author: five lines on what is new and what limits it |
| G2 Review | Every change | `reviewer` |
| G3 Second model | Any kernel diff | `grok-reviewer`, a different model from the author |
| G4 Owner | Tier table, effect set, default grants, trust roots, kernel list, skill text | Owner (a stop point under `CLAUDE.md`) |
| G5 Release | Before a tagged release | Full hostile suite, all break checks, permission diff of first-party plugins |

**Supply chain.** Dependencies pinned by hash, CI actions by commit. Build jobs hold no secrets; signing
happens outside CI. A lint rejects a workflow job that has a write token and runs code not in the reviewed
commit (Rubi audit C8, C9). Coding agents never hold release keys and do not push. Every security claim
shown to users links to a test or is deleted (Rubi audit C7).

## 9. Traceability: Rubi audit → GBOOST

Kind: **C** = unrepresentable by construction. **E** = enforced by an automated check that is break-checked.

### High findings

| Finding | What went wrong in Rubi | Rule | Why it cannot recur | Kind |
|---|---|---|---|---|
| C1 | The Bot minted a link ticket the panel took for the owner | P1, P2 | No ticket type exists. The owner API accepts only device-key signatures. Cards and links are pointers | C |
| C2 | Unlock links followed a file the Bot could write | P2 | The core is not on the Bot's VM. No unlock link, origin file or password | C |
| C3 | A second component accepted everything on a bearer token | P1, P3 | One authority type: the owner's signature over the digest. The client key and session carry the Bot's authority only | C |
| C4 | Approved policy was not applied; the Bot could write the config folder | P2, P12 | One policy store, read at use, on a machine the Bot cannot log in to | C, E |
| C5 | Plugins ran unsandboxed; permissions were display text | P5 | Manifest lines are the linked imports and proxy rules, and the install screen | C |
| C6 | A case-variant argument name bypassed account checks | P6 | Typed interfaces, strict single parse, identity outside the arguments | C |
| C7 | A public web page was the root of trust for approvals | P10 | The approver's code is served by the owner's core from its signed release (D3) | C |
| C8 | One online CI key signed releases, feed and catalog | P10 | The release key is not in CI. Three keys, three roles | C |
| C9 | A workflow ran unreviewed upstream code beside a write token | P10 | Workflow lint: no write token in a job that runs unpinned code | E |
| P-X1 | Plugin pages changed state on a ticket alone | P1, P11 | Plugins have no pages that act. Every change is an effect | C |
| P-M1 | The send approval did not show the sent message | P3 | The core composes, shows and sends the same bytes | C |
| P-M2 | A hidden mail's subject leaked through the reveal summary | P7 | Cards and request states never contain withheld fields | C |
| P-M3 | Unsubscribe sent attacker-written mail on a chat-level approval | P1, P4 | One way to send mail: `mail.send`, T3. Nothing in chat approves | C |
| D-H1 | Batch items were cut to one line | P3 | Signed fields are shown in full; over-long values are refused at the call | C, E |
| D-H2 | The screen did not say which plugin or Bot asked | P3, P6 | The identity row is drawn by the core from the connection | C |
| D-H3 | Long previews were clamped with Approve enabled | P3 | No clamp; the button waits for the end | E |
| D-H4 | Refresh or Back lost the request | P1, §4.3 | Requests live in the core; the inbox is the approver's home | C |
| Design | The daemon ran on the Bot's VM; platform secrets were plaintext there | P2, §4.8 | Placement A or B; no secret in chat or Grok's Vault | C |

### Medium findings, by rule

| Rule | Findings | How |
|---|---|---|
| P1, P9 | C11, C12, C15, C17 | The Bot cannot decline; authenticate first; owner controls have no budget; `connect` only in an owner-opened setup |
| P2, P10 | C24, C26, C27, C28 | The phone pins the core's identity and highest version; installers are in the signed release manifest and verify the signature themselves (the Bot may start them, 6.2); no Bot-writable path in a trust decision |
| P3 | C10, C21, P-A3, P-M6, D-M1 to D-M11 | The effect model; push is T3 over the frozen diff; edits shown as diffs; overdue items need a new approval; one core-drawn template set |
| P4, P12 | C18, P-A1, P-A4, P-M5, notes overwrite, shortcut names | Least by default; no risk decision from shell text or names; bulk and overwriting owner text are T2; every grant ends |
| P5, P11 | C13, C14, C16, C19 | No project relay or watcher; all egress through the proxy, allow-lists only; adapter access is an owner grant |
| P6, P7 | P-X2, P-M4, calendar oracle, Telegram defaults | Watches are core objects and wakes carry no text; the filter covers every representation; no read without a grant |
| §5.5, §5.6, §6.3 | C23, C25, P-X3, P-X4 | Skill text ships signed with the core and plugins ship one `about` line; signed revocation list; one key per plugin |
| §8 | C20, C29 | Same sandbox everywhere; a skipped security test fails; break checks |
| D7 | C22 | Sessions on the owner's machines are out of scope |

## 10. Build phases

Ordered. Each phase ends with its invariants green and break-checked.

0. **Decisions, facts, spikes.** Owner decides D1 to D4. Verify section 12. Spikes: a hello MCP on a home box
   called by Grok Bot through a Remote HTTPS connector with OAuth, over both network paths; a card in chat; a
   routine wake; a phone signing a digest the core verifies; a WASM component calling `fetch` through an
   allow-list; F11 and F12 (the Bot drives a command on the Mac, and follows a guide from a repository
   link). Write `docs/security/invariants.md`. CI with the supply-chain rules from day one.
   Added 2026-11-13 (`docs/plans/phase-0.md` S5 to S8): the Android approver with passkey and Web Push, passkey
   sync on both phones, PRF (F13 to F15); the phone-only install on a fresh, separately rented VPS, done by
   the owner from the phone (F17); the approver across two nodes (F16); a Windows service and sealing, and
   macOS sealing in a daemon (F20, F21). S4 ran the Remote HTTPS connector with OAuth (F1 holds, F2 false
   in practice, F3 answered). Added with D16 (S9): `gboost-client` as a Command connector on the Bot's VM,
   client-key enrolment and the routine-webhook session check, end to end (F4, F23 to F25). The two-node
   front-door spike (F18, F19) is no longer needed: the client picks the node.
1. **First slice, used from Grok chat: notes.** Install, scan, install and enrol the client, create the
   routine. Then in chat: "save this to my notes" and "find my note about X" just work (T0, T1, undo in the
   feed). "Rewrite my note Y" shows a card and asks in the approver with a diff (T2). Built: agent gateway
   with client-key enrolment (T4) and the webhook session check (so the routine webhook arrives here, not in
   phase 3), `gboost-client`, plugin host, an Obsidian
   vault plugin, audit log, undo journal, approver, skill text, and the installs of 6.2: signed release,
   bootstrap, `INSTALL.md`, the Bot-run macOS install (D8) and the phone-only VPS install (D10). The core
   builds and is tested on macOS and Linux from the first commit. Built in from the start because
   retrofitting is expensive: the state store as an epoch-tagged append-only log, the passkey-released state
   key (4.8), node id and epoch in the approval context, the Manage screen, enrolment by owner-made code.
   Phase 1 is done when a person holding only a phone gets from the link to the first saved note. Every
   boundary runs once.
2. **Mail.** Read with the filter and budgets (P7), drafts (T1), send (T3) with the outbox hold, untrusted
   labels (P8). This proves P3 on the case that failed worst in Rubi.
3. **Few prompts, and events.** Task cards, standing grants, budgets, pause rules. Watches and coalesced
   wakes through the routine webhook (shared with session checks since phase 1). Calendar plugin.
4. **Nodes.** Node enrolment, replication with the clear and sealed sets, limited mode, takeover and
   hand-back, the Nodes screen, per-node credentials, node choice in `gboost-client` (6.6). The owner's VPS
   standby is installed here (6.2, D10). Hostile-suite additions: a hostile standby, a partition, a replayed
   epoch, a stolen primary.
5. **Plugin SDK.** The `gboost plugin` commands, the conformance suite, docs. Rewrite one first-party plugin
   using only the public SDK. Catalog signing and revocation go live here (release signing is phase 1).
6. **Adapters.** Smart home and one messenger, with the T4 safety class.
7. **Public release.** Windows installer, packages and a container image, updates. Any Grok Bot user can
   install GBOOST with first-party plugins. (The VPS install moved to phase 1.)
8. **Community catalog.** Review process, publisher onboarding, catalog CI open to outside pull requests.

Not planned: agents running code on the owner's machines (D7). Optional later: other agents (6.4).

## 11. Decisions

**Decided 2026-11-13 (third round, owner, after S4):** D16 the agent path is `gboost-client` on the Bot's
VM with a webhook-checked session; D13 is superseded.

**Decided 2026-11-13 (second round, owner):** D10 phone-only install by an owner-pasted block, the Bot never
logs in, and the owner's own VPS node installed by the Bot through the owner's root key as an accepted risk;
D11 state encrypted on every node under a passkey-released key; D12 limited mode automatic, full takeover on
one biometric approval; D13 one connector for all nodes (FD2, else FD3); D14 passkey sync plus a second authenticator;
D15 phone-only install in phase 1, nodes as phase 4, plugin authoring exempt from phone first. The draft
`gboost-nodes-phone.md` keeps the options in full.

**Decided 2026-11-13:** D8 (the Bot installs from a repository link) and D9 (the name GBOOST).

**Decided 2026-11-12:** the owner accepted every recommendation below (D1 Rust; D2 home box, VPS supported,
never the Bot's VM; D3 core-served web app; D4 T0 and T1 unattended at first; D5 metadata by default;
D6 first-party until the SDK phase; D7 forge plugin only). The table stays as the record of options.


| # | Decision | Options | Recommendation | Trade-off |
|---|---|---|---|---|
| D1 | Core language | Rust; Go; TypeScript | **Rust** for the core, TypeScript and Rust plugin SDKs | Rust has the best WASM host and lets "owner proof" be a type nothing else can build, but is slower to write. Go is weaker for WASM components. TypeScript has the largest dependency risk |
| D2 | Where the core runs (6.1) and how the Bot and the phone reach it | Home box; VPS; desktop; the Bot's VM. Tailscale plugin or a tunnel | **Home box, own OS account. VPS supported from the same build. Never the Bot's VM** | Every user needs a second always-on machine, which Rubi did not ask for. In return the guarantees hold against an injected Bot. A VPS host can read everything |
| D2+D3 | Note | These two interact | | If the owner API stays on a private network, the phone needs the VPN app on to approve. Content-free push still arrives (it goes through the platform's push service), but opening it must reach the core. Phase 0 tries a private-only owner API and a public address whose TLS ends on the box, and checks how each feels before committing |
| D3 | Approver form and recovery | A web app served by the core, installed to the home screen, with a passkey; a native app | **Core-served web app.** Native only if Phase 0 shows its push or key handling falls short. Recovery: passkey sync plus a second authenticator (D14, 4.8); the owner channel as last resort | The web app needs no store, works on any phone and pushes without a project server. A native app has stronger key handling, but its push needs a relay the project would have to run |
| D4 | What Grok Bot may do unattended | T0 only; T0 and T1 within budgets; plus standing grants for narrow T3 | **T0 and T1 at first; T3 grants opt-in, one at a time, from phase 3** | More done while the owner sleeps, and more to undo after a bad night |
| D5 | Default read scope | Everything in connected accounts; metadata only, bodies by grant | **Metadata by default, bodies by grant per account and folder** | Tighter reads mean more asking and weaker help. Looser reads hand more private text to a VM with open egress |
| D6 | Community plugins | Sideloading for all; catalog only; developer mode only | **First-party only until the SDK phase (5 since 2026-11-13), then catalog; sideloading in developer mode only** | Slower growth; the owner is the review bottleneck at first |
| D7 | Running code on the owner's machines | Build it (Rubi Home's role); forge plugin only (pull requests, T3 push); nothing | **Forge plugin only.** Leave desktop commands to the Grok app's own ask-every-time | Rubi's largest cluster of findings lived here (Rubi audit C3, C4, C18 to C22). Leaving it out removes a feature the owner used |
| D8 | Install (owner, 2026-11-13) | Owner runs an installer; Bot runs a fixed command; Bot installs from a repository link | **Decided: the Bot installs from a link to the GitHub repository and asks the owner only at owner steps or on a repeated failure (6.2)** | Easiest for the owner. The Bot runs commands as the owner during install, and the link is the root of trust for the first install |
| D9 | Name (owner, 2026-11-13) | Bastion; GBOOST | **Decided: GBOOST, Grok Bot Operations & Superpowers Toolkit** | None in the design |
| D10 | Phone-only install (amends D8 on that path) and the owner's VPS node | H1 owner pastes a block; H2 owner's SSH app; Bot over SSH; Bot with a provider token | **Decided: H1 with an owner-made enrolment code (E1), H2 supported; the Bot guides and never logs in. The owner's own VPS node: the Bot installs it through the owner's root SSH key from the Mac, accepted risk (6.2)** | One paste with two blanks for a phone-only owner. For this owner, a Bot with Mac command access can reach root on the VPS at any time; the only guard is "ask every time" |
| D11 | What a node holds | Standby holds the key always; key released per failover, per account; nothing | **Decided: all state encrypted on every node under a key only the owner's passkey releases (PRF), in memory only; a standby gets it per takeover, for accounts in scope** | Host reads nothing at rest, in snapshots or backups; a VPS host can read memory during a failover. One biometric approval after each reboot |
| D12 | Who starts a failover | Automatic; limited automatic and full on approval; owner only | **Decided: limited mode automatic, full takeover after one biometric approval** | If the owner is asleep, the Bot gets "main node offline" and a queue, not working tools |
| D13 | Front door (**superseded by D16**, 2026-11-13) | FD1 connector per node; FD2 Tailscale service name; FD3 own domain with DNS switch; FD4 VPS forwards; FD5 tunnel provider | **Decided: one connector for all nodes (owner's fixed requirement). FD2 if Phase 0 confirms it, else FD3 with a DNS token scoped to one record. FD1, FD4, FD5 rejected. Neither FD2 nor FD3 working is an owner stop point** | FD3 puts a DNS token on the VPS: its holder can redirect the Bot's traffic (6.6) |
| D14 | Recovery and the meaning of "phone key" (amends P1, D3) | R1 passkey sync; R2 second authenticator; R3 typed code with delay; R4 owner channel | **Decided: R1 by default plus R2 offered at setup; no typed recovery code** | Sync makes the Apple or Google account able to approve |
| D15 | Scope and order | (a) phone-only VPS install in phase 1, nodes as a new phase 4; (b) nodes after the SDK | **Decided: (a); plugin authoring exempt from phone first** | Phase 1 grows by a Linux install path; the SDK moves one phase later |
| D16 | Agent path (owner, 2026-11-13, after S4; supersedes D13) | (a) Remote HTTPS connector with OAuth, over Funnel or a public address, one address moved between nodes (FD2 or FD3); (b) `gboost-client`, a local Command connector on the Bot's VM, over the tailnet, with an owner-enrolled client key and sessions opened after a routine-webhook check | **Decided: (b).** The client picks the active node, so one connector holds by construction; no public address, OAuth or Funnel on the agent path (6.1, 6.6) | Credentials sit on the VM, readable by the Bot (S4 showed (a) put the token there too). Does not stop an injected Bot (approvals do); a Bot can relay a code (short sessions bound to the key); each check costs a routine run, so checks run at session start and every few hours, not per call |

## 12. Facts to verify in Phase 0

Each is per xAI docs or from memory. "If wrong" says what changes.

| # | Fact the design depends on | If wrong |
|---|---|---|
| F1 | A Remote HTTPS connector can reach an owner-hosted MCP server, through the Tailscale plugin or a tunnel | Holds (S4). No longer the agent path (D16) |
| F2 | The connector's OAuth token stays on the platform's backend, never on the VM | **False in practice (S4):** the `tools/call` requests came from the Bot's VM over the tailnet, carrying the bearer token. Credentials the Bot uses are on the VM. D16 builds on that: client key and session on the VM, the Bot's authority only, short and budgeted |
| F3 | With the Tailscale plugin, F2 still holds and the Bot's tailnet reach can be limited to one port | **Answered (S4):** the VM joins the owner's tailnet under the owner's login and reaches every device. No ACL, by owner rule (never restrict the Bot): its reach is accepted, and Tailscale identity headers never count as owner identity |
| F4 | A Command connector's result can render a card (`ui://` resource) with a link button; whether a press returns as a tool call. S4: a Remote connector returned the resource, the chat did not draw it | Cards become plain text with a link |
| F5 | Routine webhook: POST with a bearer key and JSON body, cost per run, 300 runs an hour | Retune the wake budget; without webhooks the Bot polls on a schedule |
| F6 | The Grok app has no approval API for third parties | If one appears, use it only if it yields a proof the core can verify |
| F7 | Whether a connector can be tied to one Bot, and whether one MCP session is one Bot run | GBOOST cannot tell Bots apart; P8's "task" becomes a time window |
| F8 | iPhone: Web Push from an installed web app on the owner's own address; a passkey check per signature | D3 moves to a native app, which needs a push relay |
| F9 | WASM component tooling for TypeScript and Rust plugins | Plugins start in Rust only, or 5.1 is reopened |
| F10 | Grok's custom connector supports OAuth dynamic client registration (only a URL to paste) | Partly answered (S4: Grok is a public PKCE client). No longer on the agent path (D16) |
| F11 | The Grok desktop app lets the Bot run commands on the owner's primary computer, with a per-command confirmation the owner can switch on | Install falls back to the owner running one command or opening a .pkg |
| F12 | Grok Bot can read a public GitHub repository from a link and follow a long install guide to the end, stopping where told | `INSTALL.md` shrinks to one command plus owner steps |
| F13 | Android: a web app installed from Chrome gets Web Push sent by the core with its own key, and a passkey check with user verification per signature | Android needs a native approver, which needs a push relay (D3 reopens) |
| F14 | iPhone and Android platform passkeys are synced by the platform account, not bound to the device; a FIDO2 key can be used from the phone | R1 falls away and R2 becomes required; or, if sync cannot be turned off, P1's wording must say so |
| F15 | The WebAuthn PRF extension works with platform passkeys in Safari on iOS and Chrome on Android | The phone keeps the unwrap secret in web storage as a non-extractable key (weaker), or the owner types the recovery key to unlock a node |
| F16 | An installed web app starts from its service-worker cache while its origin is unreachable, can call a second origin, and receives a push sent by another node with the same sending key; iOS does not evict the cache of an installed app | One installed approver per node, or a native app |
| F17 | Mainstream VPS providers accept pasted cloud-init user data in their mobile web panel and run it once as root at first boot; the boot log is readable there | H2 (SSH app) becomes the phone-only default |
| F18 | Tailscale: a headless node joins with a one-time auth key; Funnel can be switched on from the phone; TLS for serve and Funnel ends on the node; one service name can be served by whichever of two nodes is active (also through Funnel), so one Grok connector reaches the active node; `ts.net` names work as a passkey RP ID | The front-door parts (one service name, Funnel for the connector) are no longer needed (D16). Still needed: headless join, TLS on the node and the RP ID for the approver; if the RP ID fails the owner needs a domain |
| F19 | FD3: one Grok connector at the owner's domain keeps working when DNS moves to the other node: Grok follows the change within minutes, each node gets a certificate for the name, the connector's token is accepted by whichever node is active, and the provider's DNS token can be limited to one record | No longer needed (D16 supersedes D13) |
| F20 | Windows: a service under a virtual account, key sealed with the TPM, the WASM host and Tailscale serve all work; the Grok desktop app exists on Windows and Linux with the same command access as F11 | Windows hosting moves later; on those OSes the owner runs the installer |
| F21 | A macOS daemon account can use a Secure Enclave key with nobody logged in | macOS seals the node key with a keychain file key, as on a VPS |
| F22 | Grok's secure input field keeps a typed secret out of the model's context and the chat history, and passes it only to a tool call | Passwords go only to owner channels (provider panel, approver), never through the Bot |
| F23 | A routine run started by the webhook can call a tool of the local Command connector on the VM, so it can hand the code to `gboost-client` | Sessions cannot be checked through the webhook; D16 reopens (owner stop point) |
| F24 | One routine run costs little plan usage and delivers the code within a minute or so | Fewer checks (longer sessions, tighter budgets), or D16 reopens if the delay makes sessions unusable |
| F25 | Grok runs a Command connector as a user-installed process on the VM, and it survives VM restarts | The Bot reinstalls or restarts the client after a restart (the key must then survive on disk), or D16 reopens |
