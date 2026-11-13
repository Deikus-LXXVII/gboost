# GBOOST method: how GBOOST works

Status: **approved by the owner** (2026-11-12), including every recommendation in section 11; D8 and D9
added 2026-11-13. Nothing is
built yet. Phase 0 (section 10) is next; section 12 facts can still reopen D2 and D3. Drafted by Fable 5.2
(high effort), reviewed by Opus 5.5: audit ids checked against the audit; section 4.8 and the D2/D3 note added.
Revised the same day after the owner's correction: GBOOST exists to extend Grok Bot.
Renamed 2026-11-13 (owner): Bastion → **GBOOST, Grok Bot Operations & Superpowers Toolkit**. Map ids
(`bastion-context`) and spike code under `spikes/` keep the old name. The vault folder is `lxxvii-obsidian/GBOOST/` (renamed the same day).

Inputs: the Rubi audit of 2026-11-12, Rubi's design notes, `docs/maps/bastion-context.md`, and a Grok
web-research report of 2026-11-12 on Grok Bot (docs.x.ai pages dated 9 to 11 November). Rubi is a source of
lessons only. No Rubi code is reused.

Not checked: statements marked "(xAI docs)" come from that report, not from a test. Claims about tool
versions and phone behaviour come from memory. Section 12 lists what Phase 0 must verify.

Reading: sections 1 to 8 and 10 to 12 are the document. Section 9 is a lookup table.

## 1. GBOOST in short

GBOOST extends what **Grok Bot** can do for its owner: mail, calendar, notes, smart home, messengers, code.
The owner talks to Grok Bot in the Grok app. Grok Bot calls GBOOST's tools through a connector (Grok's
"Remote HTTPS" MCP). A plugin turns each call into a typed effect. The **core** decides, shows, executes
and logs. The owner approves, in GBOOST's own approver on the phone, only what cannot be undone.

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
(`box-secrets.json`), reusable by the Bot without the owner. So:

1. Nothing GBOOST trusts lives on that VM.
2. No GBOOST secret passes through Grok's Vault or through chat.
3. What the Bot has read, it can send anywhere. GBOOST cannot contain a leak after the read; it can only
   limit the read (P7).

| Actor | Trust | What we assume |
|---|---|---|
| Owner | Trusted, fallible | Tired, in a hurry, can be phished, taps through a prompt that looks routine |
| Grok Bot | Untrusted | Well-meaning but wrong, or steered by injected text in mail, web pages and notes. Root on its VM, other connectors, memory across tasks |
| Grok platform | Honest, outside our control | Model, VM host, connector backend. Sees every tool call and result; holds the connector's token |
| Plugin | Untrusted | Author careless at best, malicious at worst. Updates can turn hostile |
| External attacker | Hostile | Network, stolen links, stolen phone, look-alike pages, poisoned dependencies and CI |
| Builders | Fallible | The team and its coding agents, which can be injected like any other |

| # | Trust boundary | Enforcement point |
|---|---|---|
| B1 | Grok Bot (through the connector) ↔ core | Agent gateway: OAuth, strict parse, grants, budgets, read filter |
| B2 | Core ↔ plugin | WASM sandbox with host functions from the manifest |
| B3 | Core ↔ outside services | Egress proxy and effect executors. Credentials are attached here |
| B4 | Core ↔ owner's phone | Owner API: requests signed by an enrolled device key |
| B5 | Grok Bot ↔ everything else it reaches | Not ours. Limited only by what B1 lets the Bot read |
| B6 | Source ↔ release | Build without secrets, offline signing, pinned verification |

**Out of scope, and said so to the owner.** Root on the core's host. A compromised phone OS. What Grok Bot
does with tools that are not GBOOST's: its shell, other connectors, desktop commands through the Grok app.

## 3. Principles

Each principle is a rule the code enforces and a test proves (section 8).

- **P1. No agent-mintable authority.** Owner authority is a signature from a key in the phone's secure
  hardware over an action digest. Nothing else approves. No Bot-facing response carries a secret, ticket or
  link that grants anything. A chat card is display only. The Bot cannot approve, decline or cancel any
  request. (Rubi audit C1, C11, P-M3)
- **P2. The core shares nothing with the Bot.** It never runs on the Bot's VM, and it has its own OS
  account. No trust decision reads a file, variable or path the Bot can write. The owner API is a separate
  listener. Unlock never starts from a link or a typed password: the phone talks only to the core identity
  it pinned at enrolment. (Rubi audit C2, C3, C4, C27, C28)
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
  comes from the owner's signed core, not a public project site. Versions only go up.
  (Rubi audit C7, C8, C9, C24, C26)
- **P11. Each security mechanism exists once, in the core.** Approvals, filters, watches, grants and limits
  are core code that plugins cannot replace or skip. No guarantee rests on Grok's Ask-first rules, tool
  switches, Vault or skill text. (Rubi audit C16, C23, P-X2, 3.3)
- **P12. No silent widening; grants expire; failure means refusal.** Anything that lets the Bot or a plugin
  do more is a T4 action shown as a diff. Every grant has an end date and a use cap. Policy has one store,
  read at use. If a check cannot run, the action is refused. (Rubi audit C4, C20, C25, P-A4)

## 4. Actions and approvals

### 4.1 Life of an action

```
Grok Bot ── tool call ──▶ core: OAuth, strict parse, grant, budget                (B1)
                          plugin.prepare(params) ─▶ Effect                        (B2, no write capability)
                          core: tier(effect)
                            T0/T1 run now · T2 batch · T3 phone · T4 step-up
                          chat: card "waiting for you in GBOOST" (display only)
                          phone: draws the effect, signs digest(effect, context)  (B4)
                          core: verify ─▶ hold window ─▶ execute ─▶ log, keep undo (B3)
```

`context` is caller, plugin and version, account, nonce and expiry. An approval past its expiry is void; a
queued item that is overdue needs a new approval (Rubi audit P-M6).

### 4.2 Tiers

| Tier | What it covers | Examples | Path | Proof |
|---|---|---|---|---|
| T0 Read | No change. Data goes to the Bot | Search mail, read calendar | Auto inside a read grant | None; logged |
| T1 Reversible, private | Change in the owner's own data, with undo | Draft, label, archive, new note, trash | Auto inside budget; in the feed | None; undo for 30 days |
| T2 Reversible, heavy | Bulk T1 (over 20 items); overwriting what the owner wrote | Archive 400 mails, edit the owner's note | Batched: one card per task | Tap, device key |
| T3 Irreversible or outward | Leaves the owner's control or cannot be undone | Send, invite, post, permanent delete, push, pay | Per action, shown in full | Biometric, device key |
| T4 Control and safety | Changes what GBOOST allows, or physical safety | Install a plugin, add a grant, connect the Bot or a phone, door lock | Step-up | Fresh biometric, never batched, cool-off for widenings |

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
[ Decline ]                     [ Send · Face ID ]
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
- **Pause Grok Bot** is on the approver's first screen and on the core's console. It has no budget.

### 4.7 Audit log

Append-only and hash-chained. One entry per call, decision, execution and undo: caller, plugin and version,
effect type, digest, tier, how it was allowed (auto, grant id, owner proof), result. It holds digests, not
content. The chain head is signed and sent to the phone daily, so truncation shows. The Bot can read only
the state of its own requests.

### 4.8 Secrets and data at rest

- Service credentials live only in the core's vault, encrypted with a key sealed to the box's hardware (TPM
  or Secure Enclave) where it has one. They are typed in the approver and sent to the core encrypted to its
  pinned key; no agent, plugin or log ever sees them (P5, Bot rule 5). None is typed into chat or put in
  Grok's Vault.
- The undo journal, plugin storage and the audit log are encrypted at rest under keys derived from the vault
  key. Plugin storage is per plugin; uninstall deletes it after the undo window.
- Backups are encrypted to a recovery key the owner keeps offline (printed or in a password manager). A
  backup without that key is useless. Restore is a T4 action at the core's console.
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
| **A. An always-on box at home** (Mac mini, NAS, small Linux), own OS account | Remote HTTPS connector, over Grok's Tailscale plugin or a tunnel whose TLS ends on the box | OAuth token issued by the core, kept on the platform's connector backend. The Bot never receives it (xAI docs) | Yes | Installer, scan a code, add the connector |
| B. A VPS the owner rents | Remote HTTPS connector, public address | Same | Yes against the Bot. The VPS host can read data and credentials | Same installer; monthly cost |
| C. The owner's desktop | As A | Same | Only while awake. The Grok desktop app can run commands there | Same |
| D. The Bot's VM (Rubi's choice) | Command connector (stdio) | None: all is local to the Bot | No | Easiest: the Bot installs it |

**What D loses.** The Bot has root beside the core. It can read service credentials in use and act without
GBOOST; read stored mail, notes, the undo journal and everything the filter withheld; rewrite policy,
grants, the audit log and the binary; and send all of it out through open egress. The phone's signature
still verifies, but it guards nothing once the credentials are readable. Rubi's threat model set aside a
Bot that goes looking; the owner's threat (b) is that Bot. D is not offered, even as a trial mode.

**Recommendation: A for this owner; B supported from the same build** for users with no always-on machine.
The rule in one line: the core runs on a machine Grok Bot cannot log in to.

**Network path.** Prefer Grok's Tailscale plugin, so the core has no public address, if Phase 0 confirms F3.
Otherwise use a tunnel whose TLS ends on the box; one that ends TLS at its provider adds that provider to
the trusted set.

**What Grok Bot holds.** No GBOOST credential. The connector backend holds an OAuth token for the agent API.
It carries the Bot's authority only: read inside grants, propose effects. The owner revokes it in the
approver. Static header keys are not used; their protection is unknown. The routine webhook key that wakes
the Bot lives in the core's vault. All Bots of one owner share a VM, so one connector is one caller.

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
   the Grok desktop app's command access (F11). It refuses its own VM (D2): `INSTALL.md` says so and the
   installer checks for the Grok VM and exits. Both are advisory (the Bot controls its VM); D2 rests on the
   owner naming the right machine. A VPS install over SSH is phase 6 work: the Bot will have had root there,
   so it needs a fresh host, the Bot's key removed and every credential generated after that.
3. **What it runs.** A short bootstrap from the repository's latest signed release. The bootstrap checks the
   release signature against the key embedded in it and refuses anything else, so a mangled or injected
   command fails instead of installing other code. The installer then creates the core's own macOS account,
   installs the service, sets up HTTPS (Tailscale serve; Funnel when the connector needs a public address),
   registers the connector in Grok (the Bot proposes, the owner confirms Grok's own card; seen in S4) and
   prints only progress text to the Bot.
4. **Owner steps** (the Bot asks, gives a link or says where to look, and waits):
   - the macOS admin password, typed into the Mac's own system dialog, never into chat (needed once, to
     create the core's account);
   - Tailscale sign-in, and turning Funnel on in the Tailscale admin page if the tailnet has it off;
   - scanning the QR code shown on the Mac's screen with the iPhone: the approver opens, Face ID enrols the
     passkey, Add to Home Screen, notifications on;
   - confirming Grok's own card that adds the connector, then Face ID on the phone to confirm its first
     connection (T4);
   - switching the Grok desktop command access back to "ask every time" (or off) when the install ends.
5. **When something fails.** The Bot shows the failing step and its output, applies the fix `INSTALL.md`
   gives for it, and retries once. Then it stops and asks the owner. It never improvises commands outside
   `INSTALL.md`, never asks for a secret in chat, and never touches the core's account. These are
   instructions to the Bot; what enforces them is the core's separate account and the owner's confirmation.

Why this does not weaken the trust model: the trust anchors stay the release signature, the QR scan and the
phone's Face ID, and the Bot can produce none of them. The Bot gets no credential from installing: secrets
are generated inside the core's account and never printed (the S4 spike lesson: its console output held the
client secret), and the enrolment code appears only on the Mac's screen and in the QR.

Funnel exposes only the agent gateway listener; the owner API stays on the tailnet (P2).

**Enrolment while the Bot has desktop access (review 2026-11-13; owner chose (b) the same day).** A Bot that
can run commands on the Mac, or capture its screen, could read the QR's one-time code and enrol its own
passkey before the owner. Option (a), a `.pkg` the owner opens with Grok denied Screen Recording, was
declined to keep the install fully Bot-driven. Chosen (b): the core accepts exactly one passkey per
enrolment window; the Mac and the phone each show a short code derived from the enrolment and the owner
compares them before the approver is usable; a second enrolment attempt in the window is refused and shows
an alert on the Mac and on the enrolled phone. This detects a Bot that enrolled first (the owner's phone
fails to enrol and the codes do not match); it does not prevent it. Recovery: reinstall from the console.

Residual risks, stated in `INSTALL.md` for the owner:
- The link is the root of trust for the first install. A look-alike repository brings its own signing key.
  The installer and the approver both show the release key fingerprint; the official README publishes it.
- During install the Bot runs commands on the Mac as the owner. With Grok's "ask every time" the owner sees
  each one; if the owner switches it to automatic for the install, the Bot can run anything as the owner for
  that time. `INSTALL.md` recommends switching it back afterwards, and the core cannot enforce it (6.3).
- The owner's macOS account is an admin, so a Bot with desktop command access and the admin password could
  reach the core's account later. Hence: the password goes only into the system dialog, never into chat.

After that, "connect my iCloud mail" makes the Bot call `setup`; the owner opens the approver from the card,
types the app password there and approves.

**Daily use.** T0 and T1 calls just work. For T2 to T4 the tool returns a request id and a state, drawn in
chat as an interactive card (xAI docs): title, state, expiry and one button, "Open GBOOST", a pointer with
no secret. The card is built from what the Bot itself submitted, never from derived or withheld content
(Rubi audit P-M2).

**A card must never approve, decline or cancel.** Chat is the Bot's surface: the Bot can draw or alter a
card. A press returns through the connector with the Bot's authority, so the core cannot tell a person from
the Bot. And the card shows a summary, not the full effect. Rubi's chat-level approval failed this way
(Rubi audit C11, P-M3). The Grok app has no approval API for third parties, so the proof must come from
GBOOST's approver.

**Waking the Bot.** The core POSTs to a routine webhook when a request is decided or a watch fires. Each run
spends plan usage, and the platform allows 300 runs an hour (xAI docs). Rules:

- Coalesce: one wake per task for decisions, one per window for events ("5 new items"), under a daily cap
  the owner sets. Over the cap, events ride on the Bot's next tool result, which costs nothing.
- The payload carries ids, counts and fixed core-written words: no outside text, no Bot-written text, no
  secret. The Bot fetches content through tools, where it is labelled and counted (P7, P8).
- Whoever holds the webhook key can prompt the Bot. It is typed in the approver and stays in the vault.
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

### 6.4 What Grok Bot can never do without the owner

1. approve, decline or cancel any request;
2. run a T3 or T4 action, or act outside its grants and budgets;
3. create, widen or extend a grant, change a tier, budget or filter;
4. install, widen or remove a plugin, or turn on developer mode;
5. connect itself, a phone or an account, or see or enter a secret;
6. read the audit log, or text the filter withheld;
7. write text that later reaches a Bot as trusted: skill text, tool text, wake payloads, watch notes;
8. reach the core's files, process or owner API.

**Other agents.** Claude Code or a DeepSeek worker could connect later, each with its own credential and
grants. Nothing is built for them now; the caller identity in P6 is the only provision.

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
| One phone vs losing it | Recovery at the core's console, optional second device | No remote recovery while travelling |

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
| C3 | A second component accepted everything on a bearer token | P1, P3 | One authority type: the owner's signature over the digest. The connector's token carries the Bot's authority only | C |
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
1. **First slice, used from Grok chat: notes.** Install, scan, add the connector. Then in chat: "save this to
   my notes" and "find my note about X" just work (T0, T1, undo in the feed). "Rewrite my note Y" shows a
   card and asks in the approver with a diff (T2). Built: agent gateway with OAuth, plugin host, an Obsidian
   vault plugin, audit log, undo journal, approver, skill text, and the install of 6.2: signed release,
   bootstrap, `INSTALL.md`. Every boundary runs once.
2. **Mail.** Read with the filter and budgets (P7), drafts (T1), send (T3) with the outbox hold, untrusted
   labels (P8). This proves P3 on the case that failed worst in Rubi.
3. **Few prompts, and events.** Task cards, standing grants, budgets, pause rules. Watches and coalesced
   wakes through a routine webhook. Calendar plugin.
4. **Plugin SDK.** The `gboost plugin` commands, the conformance suite, docs. Rewrite one first-party plugin
   using only the public SDK. Catalog signing and revocation go live here (release signing is phase 1).
5. **Adapters.** Smart home and one messenger, with the T4 safety class.
6. **Public release.** Linux install, a container image, the VPS guide (6.2 item 2), updates.
   Any Grok Bot user can install GBOOST with first-party plugins.
7. **Community catalog.** Review process, publisher onboarding, catalog CI open to outside pull requests.

Not planned: agents running code on the owner's machines (D7). Optional later: other agents (6.4).

## 11. Decisions

**Decided 2026-11-13:** D8 (the Bot installs from a repository link) and D9 (the name GBOOST).

**Decided 2026-11-12:** the owner accepted every recommendation below (D1 Rust; D2 home box, VPS supported,
never the Bot's VM; D3 core-served web app; D4 T0 and T1 unattended at first; D5 metadata by default;
D6 first-party until the SDK phase; D7 forge plugin only). The table stays as the record of options.


| # | Decision | Options | Recommendation | Trade-off |
|---|---|---|---|---|
| D1 | Core language | Rust; Go; TypeScript | **Rust** for the core, TypeScript and Rust plugin SDKs | Rust has the best WASM host and lets "owner proof" be a type nothing else can build, but is slower to write. Go is weaker for WASM components. TypeScript has the largest dependency risk |
| D2 | Where the core runs (6.1) and how the Bot and the phone reach it | Home box; VPS; desktop; the Bot's VM. Tailscale plugin or a tunnel | **Home box, own OS account. VPS supported from the same build. Never the Bot's VM** | Every user needs a second always-on machine, which Rubi did not ask for. In return the guarantees hold against an injected Bot. A VPS host can read everything |
| D2+D3 | Note | These two interact | | If the owner API stays on a private network, the phone needs the VPN app on to approve. Content-free push still arrives (it goes through the platform's push service), but opening it must reach the core. Phase 0 tries a private-only owner API and a public address whose TLS ends on the box, and checks how each feels before committing |
| D3 | Approver form and recovery | A web app served by the core, installed to the home screen, with a passkey; a native app | **Core-served web app.** Native only if Phase 0 shows its push or key handling falls short. Recovery at the core's console, optional second device | The web app needs no store, works on any phone and pushes without a project server. A native app has stronger key handling, but its push needs a relay the project would have to run |
| D4 | What Grok Bot may do unattended | T0 only; T0 and T1 within budgets; plus standing grants for narrow T3 | **T0 and T1 at first; T3 grants opt-in, one at a time, from phase 3** | More done while the owner sleeps, and more to undo after a bad night |
| D5 | Default read scope | Everything in connected accounts; metadata only, bodies by grant | **Metadata by default, bodies by grant per account and folder** | Tighter reads mean more asking and weaker help. Looser reads hand more private text to a VM with open egress |
| D6 | Community plugins | Sideloading for all; catalog only; developer mode only | **First-party only until phase 4, then catalog; sideloading in developer mode only** | Slower growth; the owner is the review bottleneck at first |
| D7 | Running code on the owner's machines | Build it (Rubi Home's role); forge plugin only (pull requests, T3 push); nothing | **Forge plugin only.** Leave desktop commands to the Grok app's own ask-every-time | Rubi's largest cluster of findings lived here (Rubi audit C3, C4, C18 to C22). Leaving it out removes a feature the owner used |
| D8 | Install (owner, 2026-11-13) | Owner runs an installer; Bot runs a fixed command; Bot installs from a repository link | **Decided: the Bot installs from a link to the GitHub repository and asks the owner only at owner steps or on a repeated failure (6.2)** | Easiest for the owner. The Bot runs commands as the owner during install, and the link is the root of trust for the first install |
| D9 | Name (owner, 2026-11-13) | Bastion; GBOOST | **Decided: GBOOST, Grok Bot Operations & Superpowers Toolkit** | None in the design |

## 12. Facts to verify in Phase 0

Each is per xAI docs or from memory. "If wrong" says what changes.

| # | Fact the design depends on | If wrong |
|---|---|---|
| F1 | A Remote HTTPS connector can reach an owner-hosted MCP server, through the Tailscale plugin or a tunnel | Placement A fails; B (VPS) becomes the default |
| F2 | The connector's OAuth token stays on the platform's backend, never on the VM | The token can be copied off the VM. P1 still holds; shorter token life and tighter budgets |
| F3 | With the Tailscale plugin, F2 still holds and the Bot's tailnet reach can be limited to one port | Use the tunnel path only |
| F4 | A connector result can render a card with a link button; whether a press returns as a tool call | Cards become plain text with a link |
| F5 | Routine webhook: POST with a bearer key and JSON body, cost per run, 300 runs an hour | Retune the wake budget; without webhooks the Bot polls on a schedule |
| F6 | The Grok app has no approval API for third parties | If one appears, use it only if it yields a proof the core can verify |
| F7 | Whether a connector can be tied to one Bot, and whether one MCP session is one Bot run | GBOOST cannot tell Bots apart; P8's "task" becomes a time window |
| F8 | iPhone: Web Push from an installed web app on the owner's own address; a passkey check per signature | D3 moves to a native app, which needs a push relay |
| F9 | WASM component tooling for TypeScript and Rust plugins | Plugins start in Rust only, or 5.1 is reopened |
| F10 | Grok's custom connector supports OAuth dynamic client registration (only a URL to paste) | The owner copies a client id and secret once (as in the S4 spike) |
| F11 | The Grok desktop app lets the Bot run commands on the owner's Mac, with a per-command confirmation the owner can switch on | Install falls back to the owner running one command or opening a .pkg |
| F12 | Grok Bot can read a public GitHub repository from a link and follow a long install guide to the end, stopping where told | `INSTALL.md` shrinks to one command plus owner steps |
