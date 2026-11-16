# Security invariants

Source: `docs/plans/gboost-method.md`, sections 3 (principles) and 8 (how we build). Each row is one
executable claim. A row counts only once its test exists **and** its break has been seen to turn it red.

The table format is fixed so CI can parse it (Phase 1):

- `Id` is `I<principle>.<n>`, and the test function is named `p<principle as two digits>_<slug>`. For example,
  row I1.2 is proven by `p01_chat_card_cannot_decide`.
- `Break` is the smallest change to the code that disables the control. CI keeps it as a patch in
  `security/breaks/<Id>.patch` and requires the test to fail with it applied.
- `Phase` is the build phase (plan section 10) by which the test must exist.
- `Status`: `planned` → `tested` (the test exists and is green) → `proven` (the break was seen to fail it).
  CI fails if a row for a phase that is already built is not `proven`, or if any principle has no row.

| Id | Invariant | Test | Break | Phase | Status |
|---|---|---|---|---|---|
| I1.1 | No sequence of agent-gateway calls executes a T3 or T4 effect without a valid owner signature over that effect's digest | `p01_no_t3_without_owner_proof` (model-based) | Executor skips the proof check | 1 | planned |
| I1.2 | No agent-gateway call can approve, decline or cancel a request, its own included | `p01_chat_card_cannot_decide` | Gateway routes a `decide` method | 1 | planned |
| I1.3 | No Bot-facing output (tool result, card, wake payload, error) carries a secret, token, nonce or link that grants anything. The one exception is the session code (I6.3), which grants only the Bot's own authority and only with the enrolled client key | `p01_bot_output_has_no_authority` (property, over all output types) | Approval nonce included in the card | 1 | planned |
| I1.4 | An approval executes at most once and is void after its expiry | `p01_approval_single_use_and_expiry` | Nonce not marked spent | 1 | planned |
| I1.5 | The enrolment or join code is accepted only on the owner API, once, inside its window, and never appears in Bot-facing output or logs | `p01_enrol_code_owner_channel_only` | Gateway route accepts the code | 1 | planned |
| I1.6 | The routine webhook key never appears in Bot-facing output, in the client package, or in anything the agent listener serves; it is set only through the owner API | `p01_webhook_key_never_bot_facing` (property, over all output types) | Echo the webhook URL and key in `gboost_status` | 1 | planned |
| I2.1 | No trust decision reads a path, variable or file outside the core's own state directory | `p02_trust_inputs_are_core_owned` (static: allow-list of read sites) | Read policy from `$HOME` | 1 | planned |
| I2.2 | The owner API and the agent API are separate listeners. A credential valid on one is rejected on the other | `p02_listener_separation` | Shared auth middleware | 1 | planned |
| I2.3 | Path checks resolve first and then compare: symlinks, `..`, case variants, and a different Unicode normalisation of the same name | `p02_resolve_then_compare` (property) | Compare the raw string | 1 | planned |
| I3.1 | The digest the phone signs is computed over exactly the effect the core will execute: the same canonical bytes | `p03_signed_equals_executed` | Executor re-reads the params from the plugin | 1 | planned |
| I3.2 | A field too long to display in full is refused at propose, never clipped | `p03_overlong_refused` | Renderer truncates | 1 | planned |
| I3.3 | The phone's and the core's canonical encoders agree on a shared corpus | `p03_cross_encoder_corpus` | Change the key order in one encoder | 1 | planned |
| I3.4 | Plugin code is not on the execution path: `prepare` has no write host functions | `p03_prepare_has_no_write_imports` | Link `fetch-write` into `prepare` | 1 | planned |
| I3.5 | Node id and epoch are in the signed context; an approval for one node or epoch fails on another | `p03_approval_bound_to_node_epoch` | Drop the epoch from the digest | 1 | planned |
| I4.1 | The tier comes from the core's rule table. A manifest can raise it, and can lower it only with a proven inverse | `p04_tier_derived` | Read the tier from the manifest | 1 | planned |
| I4.2 | An unknown effect type or target gets the highest tier | `p04_unknown_is_highest` | Default arm returns T1 | 1 | planned |
| I5.1 | A plugin can call only the host functions its manifest names; anything else fails to link | `p05_imports_equal_manifest` | Link all host functions | 1 | planned |
| I5.2 | `fetch` refuses any origin not in the manifest allow-list: IDN, trailing dot, userinfo, IP literal, suffix tricks, redirects | `p05_egress_allowlist` (property and fuzz) | `starts_with` host match | 1 | planned |
| I5.3 | The install screen is generated from the same parsed manifest that configures the sandbox | `p05_install_screen_from_manifest` | Screen built from the manifest's display fields | 5 | planned |
| I6.1 | Each inbound message is decoded once. Unknown fields, duplicate keys and case-variant keys are errors | `p06_strict_single_parse` (fuzz) | Case-insensitive decode | 1 | planned |
| I6.2 | Caller, account and task come from the connection, never from arguments | `p06_identity_out_of_band` | Read `account` from the params | 1 | planned |
| I6.3 | A session opens only for a code the core sent to the routine webhook, presented once, before its expiry, and signed by the enrolled client key that requested it | `p06_session_needs_fresh_webhook_code` | Accept a code a second time, or from another key | 1 | planned |
| I6.4 | Tailscale identity headers and source addresses never grant anything: no session, caller identity, owner identity or budget is derived from them | `p06_network_identity_grants_nothing` | Treat `Tailscale-User-Login` as the owner | 1 | planned |
| I7.1 | No read without a live grant that covers the account and scope | `p07_read_needs_grant` | Grant check returns true | 1 | planned |
| I7.2 | Withheld items do not appear in any representation: list, search hit, count, summary, error | `p07_withheld_everywhere` (property) | Search ignores the filter | 2 | planned |
| I8.1 | Outside text reaches the Bot only inside the untrusted wrapper | `p08_outside_text_wrapped` | Raw body in a tool result | 2 | planned |
| I8.2 | A task that read untrusted content cannot use standing grants | `p08_taint_suspends_grants` | Taint flag ignored | 3 | planned |
| I9.1 | No state is allocated for an unauthenticated peer: no replay entry and no rate-limit key | `p09_auth_before_state` | Record the nonce before verifying | 1 | planned |
| I9.2 | Stop, decline, undo and revoke succeed while every agent budget is spent | `p09_owner_controls_never_starve` | Owner API shares the agent's limiter | 1 | planned |
| I10.1 | Release, catalog and revocation signatures are domain-separated: a signature for one role never verifies for another | `p10_domain_separation` | Shared signing context | 5 | planned |
| I10.2 | The core refuses a version lower than the highest it has seen. The phone refuses before using any key | `p10_no_downgrade` | Version check removed | 5 | planned |
| I10.3 | No CI job holds a write token while it runs code that is not in the reviewed commit | `p10_workflow_lint` | Add `contents: write` to the build job | 0 | planned |
| I10.4 | A standby serves no approver code: every approver asset route answers 404 unless the node is the primary | `p10_approver_code_primary_only` | Serve assets on any node | 4 | planned |
| I11.1 | Plugins cannot register approval, filter, grant or limit logic: the SDK has no such host function | `p11_mechanisms_core_only` | Export an `approve` host function | 5 | planned |
| I12.1 | A widening (new grant, scope, host or plugin permission) is T4 and is shown as a diff | `p12_widening_is_t4` | Widen through a settings call | 3 | planned |
| I12.2 | Every grant has an expiry and a use cap; a grant without them is rejected | `p12_grants_expire` | Optional expiry | 1 | planned |
| I12.3 | If a check cannot run (store unreadable, revocation list stale beyond its limit), the action is refused | `p12_fail_closed` | `unwrap_or(true)` on error | 1 | planned |
| I12.4 | A skipped security test fails the build | `p12_no_skipped_security_tests` (CI) | Mark a `p*` test as ignored | 0 | planned |
| I12.5 | A node in standby role rejects every widening, grant creation, install and node change | `p12_standby_never_widens` | Role check removed | 4 | planned |
| I12.6 | A session has an expiry of hours and a call budget; a session without them is rejected, and one revoked from the phone fails its next call | `p12_session_short_budgeted_revocable` | Session without expiry | 1 | planned |
| I13.1 | No sequence of messages makes two nodes execute in the same epoch, and no node executes unless the highest epoch it has seen names it | `p13_one_active_per_epoch` (model-based, with partitions) | Compare epochs with `>=` | 4 | planned |
| I13.2 | A takeover statement is valid only with an owner signature; a hand-back only with the holder's node signature | `p13_takeover_needs_owner` | Accept a node signature for takeover | 4 | planned |
| I13.3 | On every node the state key is never written to disk: it exists only in memory after an owner passkey released it, and a node that has not received it cannot decrypt any sealed-set record | `p13_state_key_never_at_rest` | Cache the key in the state directory | 1 | planned |
| I13.4 | Records of accounts not marked for standby never enter the replication log | `p13_scope_limits_replication` | Ship the whole vault | 4 | planned |
| I13.5 | A node replicates only to, and accepts a log only from, nodes in the owner-signed list | `p13_nodes_are_enrolled` | Skip the descriptor check | 4 | planned |
| I13.6 | A standby never executes an effect approved or queued in an earlier epoch, and never uses a standing grant | `p13_no_carry_over_execution` | Replay the outbox on takeover | 4 | planned |
| I13.7 | A node in passive or limited mode holds no state key and no usable credential and makes no outbound service call; a standby drops the state key at hand-back | `p13_limited_mode_is_inert` (egress recorded) | Unseal the vault on heartbeat loss | 4 | planned |
| I13.8 | The only agent path is `gboost-client`: every node accepts the same enrolled client key and serves the same tool list, a session is bound to the node and epoch that opened it, and no node registers or answers as a connector of its own (no Remote HTTPS or OAuth route) | `p13_single_client_path` | Accept a session opened on the other node | 4 | planned |

## Not covered by a test

These are recorded in the open and reviewed at G4. Each needs the owner or the outside world to check it:

- Out of scope: the Bot's own behaviour on its VM, a compromised phone OS, root on the core's host.
- The platform facts F1–F25 (plan section 12). Phase 0 spikes check them by hand, and the results go into
  `docs/plans/phase-0.md`.
- What a node's host reads from memory while the node runs unlocked; on a VPS standby, during a failover.
- The owner's accepted risk (plan 6.2, D10): a Bot with command access on the Mac can use the owner's root
  SSH key to reach root on the VPS node at any time. The only guard is Grok's "ask every time".
- The owner's accepted risk (plan section 2, owner rule): the Bot's VM has full reach in the owner's
  tailnet. GBOOST trusts no network identity (I6.4); other tailnet services must authenticate on their own.
- The client key and session sit on the Bot's VM (plan 6.1, D16). An injected Bot can use them inside its
  grants and relay a code to another process for one short session; approvals (P1) are the guard.
