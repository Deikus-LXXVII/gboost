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
| I1.3 | No Bot-facing output (tool result, card, wake payload, error) carries a secret, token, nonce or link that grants anything | `p01_bot_output_has_no_authority` (property, over all output types) | Approval nonce included in the card | 1 | planned |
| I1.4 | An approval executes at most once and is void after its expiry | `p01_approval_single_use_and_expiry` | Nonce not marked spent | 1 | planned |
| I2.1 | No trust decision reads a path, variable or file outside the core's own state directory | `p02_trust_inputs_are_core_owned` (static: allow-list of read sites) | Read policy from `$HOME` | 1 | planned |
| I2.2 | The owner API and the agent API are separate listeners. A credential valid on one is rejected on the other | `p02_listener_separation` | Shared auth middleware | 1 | planned |
| I2.3 | Path checks resolve first and then compare: symlinks, `..`, case variants, and a different Unicode normalisation of the same name | `p02_resolve_then_compare` (property) | Compare the raw string | 1 | planned |
| I3.1 | The digest the phone signs is computed over exactly the effect the core will execute: the same canonical bytes | `p03_signed_equals_executed` | Executor re-reads the params from the plugin | 1 | planned |
| I3.2 | A field too long to display in full is refused at propose, never clipped | `p03_overlong_refused` | Renderer truncates | 1 | planned |
| I3.3 | The phone's and the core's canonical encoders agree on a shared corpus | `p03_cross_encoder_corpus` | Change the key order in one encoder | 1 | planned |
| I3.4 | Plugin code is not on the execution path: `prepare` has no write host functions | `p03_prepare_has_no_write_imports` | Link `fetch-write` into `prepare` | 1 | planned |
| I4.1 | The tier comes from the core's rule table. A manifest can raise it, and can lower it only with a proven inverse | `p04_tier_derived` | Read the tier from the manifest | 1 | planned |
| I4.2 | An unknown effect type or target gets the highest tier | `p04_unknown_is_highest` | Default arm returns T1 | 1 | planned |
| I5.1 | A plugin can call only the host functions its manifest names; anything else fails to link | `p05_imports_equal_manifest` | Link all host functions | 1 | planned |
| I5.2 | `fetch` refuses any origin not in the manifest allow-list: IDN, trailing dot, userinfo, IP literal, suffix tricks, redirects | `p05_egress_allowlist` (property and fuzz) | `starts_with` host match | 1 | planned |
| I5.3 | The install screen is generated from the same parsed manifest that configures the sandbox | `p05_install_screen_from_manifest` | Screen built from the manifest's display fields | 4 | planned |
| I6.1 | Each inbound message is decoded once. Unknown fields, duplicate keys and case-variant keys are errors | `p06_strict_single_parse` (fuzz) | Case-insensitive decode | 1 | planned |
| I6.2 | Caller, account and task come from the connection, never from arguments | `p06_identity_out_of_band` | Read `account` from the params | 1 | planned |
| I7.1 | No read without a live grant that covers the account and scope | `p07_read_needs_grant` | Grant check returns true | 1 | planned |
| I7.2 | Withheld items do not appear in any representation: list, search hit, count, summary, error | `p07_withheld_everywhere` (property) | Search ignores the filter | 2 | planned |
| I8.1 | Outside text reaches the Bot only inside the untrusted wrapper | `p08_outside_text_wrapped` | Raw body in a tool result | 2 | planned |
| I8.2 | A task that read untrusted content cannot use standing grants | `p08_taint_suspends_grants` | Taint flag ignored | 3 | planned |
| I9.1 | No state is allocated for an unauthenticated peer: no replay entry and no rate-limit key | `p09_auth_before_state` | Record the nonce before verifying | 1 | planned |
| I9.2 | Stop, decline, undo and revoke succeed while every agent budget is spent | `p09_owner_controls_never_starve` | Owner API shares the agent's limiter | 1 | planned |
| I10.1 | Release, catalog and revocation signatures are domain-separated: a signature for one role never verifies for another | `p10_domain_separation` | Shared signing context | 4 | planned |
| I10.2 | The core refuses a version lower than the highest it has seen. The phone refuses before using any key | `p10_no_downgrade` | Version check removed | 4 | planned |
| I10.3 | No CI job holds a write token while it runs code that is not in the reviewed commit | `p10_workflow_lint` | Add `contents: write` to the build job | 0 | planned |
| I11.1 | Plugins cannot register approval, filter, grant or limit logic: the SDK has no such host function | `p11_mechanisms_core_only` | Export an `approve` host function | 4 | planned |
| I12.1 | A widening (new grant, scope, host or plugin permission) is T4 and is shown as a diff | `p12_widening_is_t4` | Widen through a settings call | 3 | planned |
| I12.2 | Every grant has an expiry and a use cap; a grant without them is rejected | `p12_grants_expire` | Optional expiry | 1 | planned |
| I12.3 | If a check cannot run (store unreadable, revocation list stale beyond its limit), the action is refused | `p12_fail_closed` | `unwrap_or(true)` on error | 1 | planned |
| I12.4 | A skipped security test fails the build | `p12_no_skipped_security_tests` (CI) | Mark a `p*` test as ignored | 0 | planned |

## Not covered by a test

These are recorded in the open and reviewed at G4. Each needs the owner or the outside world to check it:

- Out of scope: the Bot's own behaviour on its VM, a compromised phone OS, root on the core's host.
- The platform facts F1–F8 (plan section 12). Phase 0 spikes check them by hand, and the results go into
  `docs/plans/phase-0.md`.
