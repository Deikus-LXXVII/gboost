# Spike S3: approver (passkey over an action digest, plus Web Push)

Answers F8 and D3 (`docs/plans/phase-0.md`). Throwaway code. Rust core (axum 0.8.9, webauthn-rs 0.6.2,
web-push 0.11.0, all pinned, `Cargo.lock` kept) serving one static page with no CDN and no external script.

## What it proves

- The core draws the action from the effect, derives `challenge = SHA-256("bastion/approval/v1\0" ||
  canonical(effect) || nonce || expiry)` and the phone signs that challenge (WebAuthn, `userVerification:
  required`, so the platform biometric every time).
- The core verifies the assertion, requires the UV flag, checks the challenge is the digest of the effect it
  holds, marks the nonce spent, and only then "executes" (prints `EXECUTE ...`). Replay and expiry are refused.
- Web Push: content-free ("1 request waiting"), VAPID keys generated and stored in `state/` (git-ignored, 0700).

## Run

```sh
cd spikes/approver
cargo test                      # 10 tests
cargo run --release             # binds 127.0.0.1:8787 only
```

Config (env, defaults in brackets): `APPROVER_RP_ID` (required, the HTTPS host name, e.g. `<host>.<tailnet>.ts.net`),
`APPROVER_ORIGIN` (required, `https://<host>[:<port>]`), `APPROVER_PORT` [8787], `APPROVER_STATE_DIR` [state],
`APPROVER_VAPID_SUB` [the origin; the VAPID `sub` claim, Apple wants a real-looking https or mailto value].

The process keeps `state/` (passkey, VAPID key, push subscriptions, console token). Requests live in memory:
they survive a page refresh but not a server restart. To enrol again, stop the server and delete
`state/passkeys.json`.

Create a demo `mail.send` action: type `n` + Enter in the server console, or
`curl -X POST -H "x-console-token: $(cat state/console.token)" http://127.0.0.1:8787/local/demo`.
`l` lists requests. A request expires after 300 s.

The parent session fronts it with `tailscale serve --https=<port> http://127.0.0.1:8787`. The origin must be
exactly the address the phone uses, port included, or WebAuthn refuses.

## Script for the owner on the phone

1. Start the server (above). Note the `ENROLMENT CODE` printed on the console.
2. On the phone, open `https://<host>.<tailnet>.ts.net:<port>` in its browser (Tailscale on).
3. Enrol: type the code, tap "Enrol this phone", accept the passkey prompt (biometric prompt). Expect "Passkey enrolled."
4. On the server console type `n` + Enter. In the browser the inbox shows 1 pending request; tap it.
5. Read the screen. Scroll the body to its end; Approve is disabled until then. Tap "Approve".
   Expect the biometric prompt, then "Approved and executed", and `EXECUTE mail.send ... digest=...` on the console.
   Check the printed digest equals the "Digest signed" shown on the screen.
6. Try Decline on a second request (`n` again): plain button, no biometric.
7. Add to Home Screen: Share, "Add to Home Screen", Add. Close the browser. Open "Bastion" from the new icon
   (push only works from the installed app). The page should say `Standalone: true`.
8. Tap "Enable notifications", allow. Expect "Notifications enabled" and `push: subscription added, endpoint
   host ...` on the console.
9. Push test: close the app (swipe it away). Type `n` in the server console. Expect a notification
   "Bastion: 1 request waiting" and `push: sent to <host>` on the console. Repeat with the phone locked.
10. Tap the notification: the app opens on the inbox. Approve again from there.

## Report back (F8)

- Did the biometric prompt appear on every approval (steps 5, 10, and a third one)? Did any approval go through without it?
- Did the passkey prompt offer anything other than the biometric (passcode fallback)?
- Did the push arrive (a) with the app closed, (b) with the phone locked? How long after `n`? Any failure line
  on the console (`push: send to ... failed`)?
- Any step that did not work as written, and browser and OS version.

## Known limits of the spike

- The server computes the digest and the page shows it; the phone does not recompute it from the fields
  (that needs the shared canonical encoder, I3.3, Phase 1). So here the signature binds the challenge the core
  issued, which the core checks against its own copy of the effect.
- No session (accepted for the spike): after enrolment anyone who can reach the URL can read the inbox
  (before enrolment `/api/state` returns nothing). Approval itself needs the passkey. Product code needs the
  pinned-identity unlock (P2).
- Any tailnet device can still burn the 5 enrolment tries (lockout; restart the server to reset, the console
  prints this and logs the source) and, with 10 approval starts a minute allowed globally, can briefly delay
  an approval. Push subscriptions are limited to Apple, FCM and Mozilla hosts, at most 5, 10 s per send.
- `webauthn-rs` has no API for a caller-chosen challenge; `set_challenge` in `src/core.rs` edits the
  serialisable authentication state (feature `danger-allow-state-serialisation`). A library update that
  renames the field fails closed (`state layout changed`), and the honest-approval test would catch it.
