# GBOOST revision: phone first, all platforms, nodes

Status: **merged into gboost-method.md on 2026-11-13; kept as the record of options** (drafted 2026-11-13, Fable 5.2).
Section and id references (6.1, P2, D8, F11) point into the pre-merge `gboost-method.md`; the final
decisions are in section 9 and in the plan's section 11.

Trigger: the three product rules the owner added to `CLAUDE.md` on 2026-11-13 (phone first, all platforms,
nodes with failover). Priorities are unchanged: security, then ease of use, then ease of plugins.

Not checked: nothing here was tested or looked up. Every statement about phone OSes, WebAuthn, Tailscale
and VPS providers is **from memory**; the ones the design rests on are marked "(memory)" and listed as
F13 to F21 in section 8. Design reasoning (who can read what, who can sign what) is derived from the plan.

## 1. Summary

| Rule | Answer in one line | Cost |
|---|---|---|
| 1 Phone first | The core goes on a fresh VPS that the owner creates in the provider's web panel on the phone, pasting one fixed block. The Bot guides and checks; it never holds a login on the host | One paste with two blanks, instead of a fully Bot-run install |
| 2 Platforms | Core on macOS, Linux, Windows. Approver on Android and iOS as the same core-served web app. Phones never host a core | Three installers and three service setups |
| 3 Nodes | One active node per owner-signed epoch. A VPS standby takes over **limited** service by itself and **full** service after one biometric approval, which releases a key the VPS does not hold at rest | Full failover to a VPS is not automatic by default |

New terms:

| Term | Meaning |
|---|---|
| Node | One installed core. Has its own key pair, address, OS account and connector |
| Owner channel | A path to a node that the Bot cannot use: its local screen, the VPS provider's web panel, the owner's own SSH session. Replaces "the core's console" in 4.8, 6.2 and D3 |
| Active node | The one node allowed to execute effects in the current epoch |
| Epoch | A counter naming the active node. Raised only by a signed statement (5.3) |
| Limited mode | A node answers the Bot and the phone but holds no usable service credential and executes nothing |
| State key | The key that encrypts the replicated vault, journal and policy |

## 2. Rule 1: phone only

### 2.1 Where the core goes when the owner has only a phone

| # | Option | Bot's access to the host | Verdict |
|---|---|---|---|
| H1 | **Fresh VPS, owner pastes a fixed block** into the provider's "user data" field while creating the server in the provider's web panel (cloud-init, runs once at first boot) | None, ever | **Recommended** |
| H2 | Existing machine (VPS or a home box without a Grok desktop app): the owner pastes one command in a phone SSH app or the provider's web console | None | Supported; needs an SSH app and some skill |
| H3 | The Bot installs over SSH with a key the owner gives it (the phase 6 note in 6.2) | Root during install | Not recommended; see 2.3 |
| H4 | The Bot creates the VPS with a provider API token | Can rebuild, snapshot and open the console until the token is revoked | Rejected: lasting control of the host |
| H5 | The core on the phone | n/a | Rejected (section 3) |

For a phone-only owner there is no placement A (6.1): the only core is placement B, and the VPS host can
read everything on it. This is the price of rule 1 for that owner and the install says so.

### 2.2 The phone-only install, end to end (H1)

The Bot still leads from the repository link (D8) but as a guide: it reads `INSTALL.md`, tells the owner
each step, and checks results from outside. It runs no command on the host.

1. Owner sends the Bot the repository link. The Bot asks "which machine?"; "only my phone" selects this path.
2. Owner steps, each opened from a link the Bot gives to a well-known site (provider, Tailscale, GitHub):
   - Tailscale: sign in on the phone, install the Tailscale app, create a one-time auth key.
   - Make up an **enrolment code** (the phone's password generator; at least 20 characters).
   - Provider panel: new server, smallest size, a supported Linux image. Paste the install block copied from
     the release page on GitHub (not from chat), with the two blanks filled: auth key, enrolment code.
3. First boot: the block downloads the bootstrap for one pinned release, checks its hash (the hash is in the
   block), and the bootstrap checks the release signature (6.2 item 3). The installer creates the service
   account, joins the tailnet, sets up HTTPS and serves a progress page with fixed text and no secrets.
4. The Bot polls the progress page and reports. Owner opens the node's address **from the Tailscale app's
   machine list**, not from chat, types the enrolment code, and the biometric check enrols the passkey. Add to Home
   Screen, notifications on.
5. The Bot proposes the connector, the owner confirms Grok's card, then the biometric check in the approver (T4).

**How the enrolment code avoids the Bot.** There is no second screen for a QR, so the direction is
reversed: the owner makes the code and gives it to the core through the owner channel.

| # | Option | Passes through | Verdict |
|---|---|---|---|
| E1 | **Owner-made code, pasted into the provider panel with the block, typed again on the enrolment page** | Provider (already able to read the host) | **Recommended** |
| E2 | Core-made code printed to the boot log, read in the provider's web console | Provider | Fallback; console pages are poor on a phone |
| E3 | Core-made code shown in the owner's own SSH session (`gboost enrol`) | Nobody | Used with H2 |
| E4 | Code sent by mail or SMS | A sending service the project would run | Rejected |
| E5 | No code: first passkey wins, guard (b) alerts | Nobody | Rejected on a reachable address |

Rules for E1: single use, void after 30 minutes or the first enrolment, never accepted by the agent
gateway, never echoed in any page or log. The core's egress proxy blocks the cloud metadata address, where
the user data stays readable. Guard (b) changes shape: "one passkey per window" and "second attempt is
refused and alerted" stay; "compare two codes" is dropped on this path because the Bot never has the code,
which prevents what (b) only detected. The desktop path (QR on the Mac) keeps (b) as decided.

Residual: an owner who pastes the code into chat, or types it on a look-alike page a hostile Bot linked,
gives it away; the real enrolment then fails with an alert, and the fix is to delete the server and start
again.

### 2.3 If the Bot did get root (H3)

Root once cannot be proven gone from inside the same system: any check runs on software the Bot could have
changed. Steps that bound it: fresh host only; a key used for the install alone and removed; every secret
generated after removal; host keys rotated; files compared with the signed manifest. None of them detects
persistence planted as root. The only real neutraliser is a rebuild from a clean image with the block,
which is H1. **Recommendation: do not offer H3**; drop the phase 6 note in 6.2 item 2. If the owner wants
it anyway, it ships labelled "the Bot was root here" with the 6.1 option D consequences.

### 2.4 Daily management and recovery from the phone

Management: the approver gains a **Nodes** screen and a **Manage** screen, so nothing needs a shell:
status of each node, pause, updates, grants, plugins, accounts, backups, failover and hand-back.

Recovery today is "at the core's console" (D3, 7), which for a home box means a keyboard. Options:

| # | Option | Trust added | Verdict |
|---|---|---|---|
| R1 | **Platform passkey sync** (iCloud Keychain, Google Password Manager): a new phone on the same account has the key (memory, F14) | The Apple or Google account can recover the approval key | **Recommended as the default**; on iPhone it is probably already the case |
| R2 | **Second authenticator** enrolled as T4: a second phone or tablet, or a FIDO2 key tapped on the phone (NFC, USB-C) | None; same kind of authority | **Recommended in addition**, and the only choice for an owner who turns sync off |
| R3 | Typed recovery code with a 24 h delay and an alert on every device | A typed secret becomes a second kind of authority (against P1, P2) | Not recommended |
| R4 | Owner channel only | None | Last resort. Phone-friendly on a VPS (provider panel), not on a home box |

Backups stay encrypted to the offline recovery key (4.8). Restore is: install a fresh node (2.2), enrol,
then paste the recovery key in the approver, encrypted to the node's key. It decrypts data; it approves
nothing.

Outside rule 1 unless the owner says otherwise: writing plugins (`gboost dev`, 5.4) needs a computer.

## 3. Rule 2: platforms

| Platform | Hosts a core | Approver | Installer | Service and own account | Key sealing |
|---|---|---|---|---|---|
| macOS | Yes | Optional second device (below) | Bootstrap script, later a signed and notarised `.pkg`; Bot-run through Grok desktop (D8) | launchd daemon, hidden service user | Secure Enclave (F21) |
| Linux | Yes; the VPS and the phone-only target | Optional second device | cloud-init block, one-line bootstrap, `.deb`/`.rpm`, container image | systemd unit, dedicated user, sandboxing options on | TPM where present; a VPS usually has none, so the key is a file only root and the service can read |
| Windows | Yes | Optional second device | Signed MSI or winget; needs a code-signing certificate | Windows service under a virtual service account | TPM |
| Android | No | **Yes**: installed web app in Chrome, passkey, Web Push | None | n/a | Platform passkey |
| iOS | No | **Yes**: installed web app from Safari, passkey, Web Push | None | n/a | Platform passkey |

**Phones never host a core.** iOS cannot run a listening background service. Android can in principle, but
the OS stops background work, the address changes, and core plus approver on one device removes boundary
B4: one compromised phone would hold both the credentials and the approval key. Not planned.

**Desktops as approver.** A browser with a platform authenticator (Face ID/Touch ID, Windows Hello, Android biometrics) works as a second
device. Off by default on any machine where the Grok desktop app has command access: the Bot could change
what that browser shows. View and Pause are allowed there; signing T3 and T4 is not.

**Web app or native (D3).** Stay with the core-served web app: one code base covers both phones, no store,
no project push relay. Reasons to reopen: device-bound keys, code pinned in a store build, more reliable
push. Each needs a relay or store accounts the project would have to run.

**An admin on the same machine.** On every desktop OS an administrator can reach the service account. The
note in 6.2 (admin password only in the system dialog; Grok command access back to "ask every time")
applies to Windows and Linux desktops the same way.

Phase 0 must verify (section 8): Android web app with passkey and push (F13); passkey sync on both phones
(F14); Windows service account, sealing, WASM host and Tailscale under a service (F20); Grok desktop command
access on Windows and Linux (F20); a macOS daemon using the Secure Enclave with nobody logged in (F21).

## 4. Rule 3: nodes and failover

### 4.1 Topology

| Shape | Nodes | Notes |
|---|---|---|
| Single | One (a home box, or a VPS for a phone-only owner) | No failover. Rule 3 does not apply until a second node exists |
| **Home + standby** | Primary at home, standby on a VPS | The owner's case and the rule's example |
| Two of a kind | Two VPSs, or two machines the owner controls | The standby may run full and automatic (4.4) |

One node is active per epoch. The primary is the designated home of the active role; the standby holds a
replica and takes over. More than two nodes is not planned.

### 4.2 What replicates, and what the VPS host can read

The primary ships an append-only, epoch-tagged log to the standby over the tailnet, on a channel
authenticated by both node keys. Two classes:

| Class | Contents | Form on the standby | Why |
|---|---|---|---|
| Clear set | Node list, epoch statements, owner device public keys, the hash of the standby's own connector token, push subscription and push sending key | Signed by the primary, readable | Limited mode needs it: verify the owner, notify the phone, answer the Bot |
| Sealed set | Vault (service credentials, webhook key), policy, grants, budgets, pending requests, outbox, undo journal, plugin storage, audit log | Encrypted under the state key | Needed only to act |

| State key custody | The VPS host can read | Failover | Verdict |
|---|---|---|---|
| K1 Standby holds the key at all times | Everything, always, including while the home box is healthy | Automatic | Only for a standby on the owner's own hardware, or by explicit choice (D11) |
| **K2 The phone releases the key per failover**; the standby keeps it in memory and wipes it on hand-back | Clear set always. Sealed set only while a failover is active. A host that copied the key then can read the replica until the key rotates | One Face ID | **Recommended for a VPS** |
| K3 Scope: the owner marks which accounts are "available on standby"; the rest never replicate | Only the marked accounts | As K2 | **Recommended with K2** |
| K4 No key on the standby, ever (queue only) | What the Bot submits during the outage | n/a | The floor: this is limited mode |

K2 needs the phone to hold a decryption secret. Preferred: the passkey's PRF extension (memory, F15), so the
secret needs the same biometric approval as an approval. The state key rotates at every hand-back.

**Per-node service credentials.** Where a service allows several (app-specific passwords, one OAuth grant
per client), the standby gets its own, typed by the owner in the approver when marking the account. The
owner can then cut off a suspect VPS at the service without touching the primary.

### 4.3 What a standby may do

| Mode | Entered | Does | Credentials used |
|---|---|---|---|
| Passive | Normal running | Receives the log, answers `status` | None |
| **Limited** | By itself, after the primary misses heartbeats for 5 minutes | Tells the Bot "main node offline", queues proposals (encrypted to the primary's key after the strict parse), pushes the owner "Main node offline. Take over?", serves Pause and status | None |
| **Full** | The owner approves takeover (T4) | T0 and T1, and T3 with per-action approval, for accounts marked for standby. No standing grants, no widening, no installs: policy has one writer, the primary | The marked accounts' credentials, in memory |

A false alarm in limited mode costs nothing: no secret is involved and the primary is not disturbed.
Read-only is not offered as a separate mode: most services have no read-only credential.

### 4.4 Who decides, split-brain, fencing

| Option | Speed | Split-brain | Needs | Verdict |
|---|---|---|---|---|
| Automatic lease and heartbeat for full takeover | Minutes, also while the owner sleeps | Possible in a partition: with two nodes there is no quorum | K1 | Opt-in only (D12) |
| **Limited automatic, full on the owner's approval** | Limited in minutes; full when the owner taps | Bounded (below) | K2 | **Recommended** |
| Owner only | Nothing until the owner taps | As above | K2 | Loses "the Bot is told why nothing works" |

**Epoch statements are the fencing token.** A takeover is `{epoch n+1, active = standby}` signed by the
owner's passkey. A hand-back is `{epoch n+2, active = primary}` signed by the node that holds the role: only
the holder can give it up, so it needs no prompt. Statements authenticate themselves, so anyone may carry
them: the standby, the phone, even the Bot. Withholding one only preserves the old state.

Rules:
- Node id and epoch are part of every approval's `context` (4.1). An approval made for one node or epoch is
  void on another.
- A node executes only if the highest epoch it has seen names it.
- At takeover the approver first tries the primary itself. If the phone reaches it, the screen says so and
  offers a clean hand-over instead.
- A primary that was unreachable past the threshold comes back in limited mode and stays there until it has
  read the current epoch from the standby or the owner's phone.
- The standby never executes anything approved in an earlier epoch. Outbox items the primary may or may not
  have sent are shown as "state unknown" and need a new approval; they are not resent.
- Pending approvals are void across a failover and are asked again.

What is left of split-brain: the primary is online but invisible to both the standby and the phone (for
example a Tailscale outage) and the owner approves takeover. Both nodes then act until the primary sees the
new epoch. Each T3 still needs its own approval naming its node; T1 budgets can be spent twice (undoable);
standing grants run on the primary only, so their caps hold.

### 4.5 Front door: how Grok and the phone find the active node

| # | Option | Who else sees traffic | Verdict |
|---|---|---|---|
| FD1 | **One connector per node** in Grok, each with its own address and OAuth token. A node that is not active returns a fixed "not active" result | Nobody new | **Recommended baseline**; nothing is shared and nothing extra is hosted. Cost: two tool lists in the Bot's context (F19) |
| FD2 | One Tailscale service name hosted by both nodes (memory, F18) | Tailscale, as today | Use instead of FD1 if Phase 0 confirms it, including with Funnel |
| FD3 | The owner's own domain, DNS switched to the active node | DNS provider; a DNS token sits on the VPS | Only when both nodes have public addresses and the owner has a domain |
| FD4 | The VPS as permanent front door, forwarding to the home box | The VPS host can obtain a certificate for the name and read the primary's traffic at all times | Rejected |
| FD5 | A tunnel provider with a stable name that ends TLS | That provider | Rejected (6.1 network path) |

**The approver has one origin: the primary's.** The installed web app starts from its cache when the primary
is down and calls the standby's owner API cross-origin (memory, F16). The standby never serves approver
code, so a VPS host cannot change what the owner sees (P3, P10); it can only send effects, which the
unchanged code draws in full, with the node's name on the identity row. The standby pushes with the
replicated sending key. If F16 fails: a second installed approver per node, or a native app (D3).

If the primary is gone for good: "Promote standby" (T4). The approver is installed again from the new
primary's origin and the passkey re-enrolled there, authorised by a signature from the old one.

### 4.6 How the phone trusts a new node

Adding a node is T4 with a cool-off. The new node is installed as in 2.2 with its own owner-made join code.
In the approver the owner opens Nodes, Add, and enters the address and the code. The primary and the new
node run a password-authenticated key exchange on that code, which binds their node keys with no help from
the Bot. The approver then shows the node descriptor (name, address, key fingerprint, role, key custody,
accounts in scope) and signs its digest. Nodes replicate only to nodes in the signed list; the approver
talks only to nodes in it. Removing a node is T4 without cool-off: the primary stops shipping, rotates the
state key, and lists the credentials to revoke at their services.

### 4.7 Threat model map

| Element | Actor and asset | Control | Residual risk |
|---|---|---|---|
| Sealed replica on a VPS (K2, K3) | VPS host; A1, A2 | Ciphertext at rest; key released per failover; scope per account | During a failover the host can read the marked accounts' data and credentials |
| Clear set on a VPS | VPS host; A4, A5 | Public keys and hashes only; signed by the primary | The host learns node names, device count and timing, and can send empty notifications |
| Takeover | Bot or outside attacker; A3 | Owner's passkey signature over the epoch statement | An owner tricked into approving a needless takeover exposes the marked accounts to the VPS host |
| Limited mode gateway | VPS host; A1 | No secrets on the node; proposals sealed to the primary | The host sees what the Bot submits during the outage and can feed the Bot false results (B5) |
| Hand-back | Thief holding the home box; A3 | Node key; "Remove node" fences it | A stolen box that boots unattended becomes active again until the owner removes it |
| Split-brain | Fallible builders; A3 | Epoch in context; standing grants on the primary only | T1 budgets can double during a partition |
| Node enrolment | Bot, outside attacker; A4 | Owner channel code, key exchange, T4 signature on the descriptor | A join code leaked into chat lets another machine pair; the fingerprint on the T4 screen is the last check |
| Approver across nodes | VPS host; A3 | Code only from the primary; signed effects shown in full | None beyond the primary's own |
| Phone-only install (H1, E1) | Bot; A2, A4 | No Bot login on the host; code through the owner channel | The repository link stays the first root of trust (6.2) |
| Recovery by passkey sync (R1) | Outside attacker; A4 | Platform account security | Whoever takes over the Apple or Google account can approve |

## 5. Changes to principles and tiers

- **P1**, amend: "a key in the phone's secure hardware" becomes "a passkey on an enrolled owner device". Say
  that a synced passkey puts the platform account in the trust base, and that a device-bound FIDO2 key is the
  stricter choice (D14).
- **P2**, amend: "never runs on the Bot's VM" becomes "runs only on machines the Bot has never had a login
  on". "The core's console" becomes "the owner channel". The phone pins a signed node list, not one core.
- **P10**, add: approver code is served only by the primary.
- **P12**, add: on a standby there is no widening at all; policy has one writer.
- **P13, new. Nodes are enrolled, scoped and fenced.** A node exists only through an owner-signed
  descriptor. It receives only the state its role and scope need. Exactly one node executes per epoch.
  Taking the active role needs the owner's signature; giving it up needs the holder's. A node off the
  owner's hardware never holds the state key at rest unless the owner chose that (K1).

| New T4 action | Shown as | Cool-off |
|---|---|---|
| Add a node | Node descriptor in full | Yes |
| Change a node's role, key custody (K2 to K1) or accounts in scope | Diff | Yes when widening |
| Take over (full mode on the standby) | Which node, which accounts, what the VPS host can then read | No |
| Change failover policy (to automatic) | Diff, with the K1 consequence spelled out | Yes |
| Promote the standby for good; remove a node | Descriptor; credentials to revoke | No (narrowing or recovery) |
| Enrol or remove an approver device; restore from backup | Device or backup details | Yes for enrol and restore |

Hand-back and entering limited mode are not owner actions: both narrow. Each appears in the feed.

## 6. New invariant rows

Same format as `docs/security/invariants.md`. Phase numbers use the new numbering in section 7.

| Id | Invariant | Test | Break | Phase | Status |
|---|---|---|---|---|---|
| I1.5 | The enrolment or join code is accepted only on the owner API, once, inside its window, and never appears in Bot-facing output or logs | `p01_enrol_code_owner_channel_only` | Gateway route accepts the code | 1 | planned |
| I3.5 | Node id and epoch are in the signed context; an approval for one node or epoch fails on another | `p03_approval_bound_to_node_epoch` | Drop the epoch from the digest | 1 | planned |
| I10.4 | A standby serves no approver code: every approver asset route answers 404 unless the node is the primary | `p10_approver_code_primary_only` | Serve assets on any node | 4 | planned |
| I12.5 | A node in standby role rejects every widening, grant creation, install and node change | `p12_standby_never_widens` | Role check removed | 4 | planned |
| I13.1 | No sequence of messages makes two nodes execute in the same epoch, and no node executes unless the highest epoch it has seen names it | `p13_one_active_per_epoch` (model-based, with partitions) | Compare epochs with `>=` | 4 | planned |
| I13.2 | A takeover statement is valid only with an owner signature; a hand-back only with the holder's node signature | `p13_takeover_needs_owner` | Accept a node signature for takeover | 4 | planned |
| I13.3 | A standby without the released state key cannot decrypt any sealed-set record; the key is never written to disk and is gone after hand-back | `p13_sealed_set_opaque` | Cache the key in the state directory | 4 | planned |
| I13.4 | Records of accounts not marked for standby never enter the replication log | `p13_scope_limits_replication` | Ship the whole vault | 4 | planned |
| I13.5 | A node replicates only to, and accepts a log only from, nodes in the owner-signed list | `p13_nodes_are_enrolled` | Skip the descriptor check | 4 | planned |
| I13.6 | A standby never executes an effect approved or queued in an earlier epoch, and never uses a standing grant | `p13_no_carry_over_execution` | Replay the outbox on takeover | 4 | planned |
| I13.7 | A node in limited mode holds no usable credential and makes no outbound service call | `p13_limited_mode_is_inert` (egress recorded) | Unseal the vault on heartbeat loss | 4 | planned |

Add to "Not covered by a test": what a VPS host reads from memory during a failover; the platform facts
F13 to F21.

## 7. Phases (replaces the list in section 10 where it differs)

- **0, add spikes.** S5 Android approver: passkey per signature and Web Push (F13), passkey sync on both
  phones (F14), PRF (F15). S6 phone-only install on a fresh VPS with the block and an owner-made code (F17),
  done by the owner from the phone; an existing VPS that runs other services is not used, so this needs a
  fresh small server. S7 two-node approver: cached start with the primary down, cross-origin
  call, push from the second node (F16); two connectors in Grok (F19); Tailscale service name (F18).
  S8 Windows service and sealing (F20), macOS sealing in a daemon (F21).
- **1, widen.** The core builds and is tested on macOS and Linux from the first commit. Two installs: the
  Bot-run macOS install (D8) and the phone-only VPS install (2.2). Phase 1 is done when a person holding
  only a phone gets from the link to the first saved note. Built in from the start because retrofitting is
  expensive: the state store as an epoch-tagged append-only log, node id and epoch in the approval context,
  the Manage screen, enrolment by owner-made code.
- **2, 3** unchanged.
- **4, new: nodes.** Node enrolment, replication with the clear and sealed sets, limited mode, takeover and
  hand-back, the Nodes screen, per-node credentials, one connector per node. Hostile-suite additions: a
  hostile standby, a partition, a replayed epoch, a stolen primary.
- **5** SDK (was 4). **6** adapters (was 5). **7** public release (was 6): Windows installer, packages and
  container; the VPS guide is no longer here, it is phase 1. **8** community catalog (was 7).
  Existing invariant rows marked phase 4 (I5.3, I10.1, I10.2, I11.1) move to 5.

## 8. Facts to verify (continuing section 12; all from memory)

| # | Fact the design depends on | If wrong |
|---|---|---|
| F13 | Android: a web app installed from Chrome gets Web Push sent by the core with its own key, and a passkey check with user verification per signature | Android needs a native approver, which needs a push relay (D3 reopens) |
| F14 | iPhone and Android platform passkeys are synced by the platform account, not bound to the device; a FIDO2 key can be used from the phone | R1 falls away and R2 becomes required; or, if sync cannot be turned off, P1's wording must say so |
| F15 | The WebAuthn PRF extension works with platform passkeys in Safari on iOS and Chrome on Android | The phone keeps the unwrap secret in web storage as a non-extractable key (weaker), or K2 needs the recovery key typed at takeover |
| F16 | An installed web app starts from its service-worker cache while its origin is unreachable, can call a second origin, and receives a push sent by another node with the same sending key; iOS does not evict the cache of an installed app | One installed approver per node, or a native app |
| F17 | Mainstream VPS providers accept pasted cloud-init user data in their mobile web panel and run it once as root at first boot; the boot log is readable there | H2 (SSH app) becomes the phone-only default |
| F18 | Tailscale: a headless node joins with a one-time auth key; Funnel can be switched on from the phone; TLS for serve and Funnel ends on the node; a service name can be hosted by two nodes and reached through Funnel; `ts.net` tailnet names work as a passkey RP ID | FD2 is dropped and FD1 stays; if RP ID fails the owner needs a domain |
| F19 | Grok accepts two connectors with the same tool names and the Bot uses whichever answers | Tool names get a node prefix, or FD2 or FD3 becomes necessary |
| F20 | Windows: a service under a virtual account, key sealed with the TPM, the WASM host and Tailscale serve all work; the Grok desktop app exists on Windows and Linux with the same command access as F11 | Windows hosting moves later; on those OSes the owner runs the installer |
| F21 | A macOS daemon account can use a Secure Enclave key with nobody logged in | macOS seals with a keychain file key, as on a VPS |
| F22 | Grok's secure input field keeps a typed secret out of the model's context and the chat history, and passes it only to a tool call | Passwords go only to owner channels (provider panel, approver), never through the Bot |

## 9. Decisions for the owner

**Owner answers 2026-11-13 (second round; replaces the first-round D11/D12 answer):**
- D11 + D12: **all GBOOST state is encrypted under a key that only the owner's passkey unlocks** (PRF, F15),
  on every node. A standby enters limited mode by itself; full takeover needs one biometric approval, which releases
  the key (K2, with K3 scope). The VPS host cannot read the data at rest, in snapshots or in backups.
  Residual: while a node runs unlocked, its host can read process memory; on a VPS that is during a
  failover only. A home node also needs one biometric approval after each reboot to unlock.
- D10: H3' (separate non-root user) is withdrawn. The owner keeps a root SSH key to the VPS on the primary computer and
  does not want to give it up; the Bot installs the VPS node through it, from the primary computer (Grok desktop command
  access). Accepted risk, recorded: a Bot with command access on the primary computer can use that key, so it can reach
  root on the VPS at any time; the owner declined hardware-key protection for the key (a platform authenticator or
  `ed25519-sk`). The only guard is Grok's "ask every time". Phone-only owners keep H1 (paste a block).
- D13: one connector is the goal. FD2 (one Tailscale name served by whichever node is active) if Phase 0
  confirms it; FD1 (one connector per node) only as the fallback.
- D14: R1 + R2. D15: (a). The rest of the draft accepted (phones as approvers only; macOS/Linux/Windows hosts).

**H3' (withdrawn by the owner, kept for the record): the Bot installs over SSH as a non-root user that can run only the signed installer.**
- The installer account is created on the fresh VPS with the Bot's public key and one sudo rule: it may run
  `/usr/local/sbin/gboost-bootstrap` as root, and nothing else (no password needed, so none passes the Bot).
  The bootstrap installs only a release whose signature checks out.
- Creating that account needs root once, so it is a short block the owner pastes when creating the server
  (cloud-init, F17); providers' "add an SSH key" puts the key on root, which is H3.
- The Bot then runs the whole install, reads progress, fixes known failures from `INSTALL.md`, and asks the
  owner only at owner steps. At the end the bootstrap deletes the installer account and its key.
- Residual: during install the Bot has a normal user shell on the host (a local privilege escalation bug
  would give it root); a fresh, updated image and the short window bound it.
- Secure input field: a password typed there still goes to the Bot's tools, so an injected Bot can use it.
  Fine for passwords the Bot must use anyway; not a way to hand it root. Whether the field keeps the value
  out of the model's context and chat history is F22.

| # | Decision | Options | Recommendation | Trade-off |
|---|---|---|---|---|
| D10 | Phone-only install (amends D8 on this path) | H1 owner pastes a block; H2 owner's SSH app; H3 Bot over SSH; H4 Bot with a provider token | **H1, with H2 supported. The Bot guides and never logs in. H3 not offered** | The owner does one paste with two blanks and makes up a code. In return no Bot ever had root on the core's host |
| D11 | What a VPS standby holds | K1 everything, always; K2+K3 sealed, released per failover, per account; K4 nothing | **K2 with K3.** K1 only for a standby on the owner's own hardware | The VPS host can read the marked accounts during a failover, never otherwise. With K1 it reads everything every day |
| D12 | Who starts a failover | Automatic; limited automatic and full on approval; owner only | **Limited automatic, full after one biometric approval.** Automatic full is an opt-in T4 that implies K1 | If the owner is asleep, the Bot gets "main node offline" and a queue, not working tools |
| D13 | Front door | FD1 one connector per node; FD2 Tailscale service name; FD3 own domain | **FD1 now; FD2 if Phase 0 confirms it** | Two tool lists in the Bot's context. No shared name means no party in the middle |
| D14 | Recovery and the meaning of "phone key" (amends P1, D3) | R1 passkey sync; R2 second authenticator; R3 typed code; R4 owner channel | **R1 by default plus R2 offered at setup. No typed recovery code** | Sync makes the Apple or Google account able to approve. Without it, an owner with one phone and a home box needs that box's keyboard to recover |
| D15 | Scope and order | (a) Phone-only VPS install in phase 1 and nodes as a new phase 4; (b) nodes after the SDK. Plugin authoring exempt from rule 1 or not | **(a); plugin authoring exempt** | Phase 1 grows by a Linux install path, and the SDK moves one phase later |

## 10. Where a rule cannot be met without weakening security

1. **Rule 1 and D8 together.** A fully Bot-run install on a phone-only setup needs the Bot to be root on the
   core's host, and that cannot be undone with certainty (2.3). Met by H1 at the cost of one owner paste.
2. **Rule 1 and placement.** A phone-only owner can only have placement B: the VPS host can read everything.
3. **Rule 1 and recovery.** Phone-only recovery needs either passkey sync (the platform account enters the
   trust base) or a second authenticator bought and enrolled in advance. A typed code would break P1.
4. **Rule 3 and automatic failover.** A VPS that takes over fully without the owner must hold every secret
   at all times (K1). The recommended design is automatic only up to limited mode.
5. **Rule 3 and the approver.** Depends on F16. If it fails, the owner gets a second approver icon per node,
   or the project needs a native app and a push relay.
6. **Rule 2.** Android and iOS cannot host a core; they are approvers only.
