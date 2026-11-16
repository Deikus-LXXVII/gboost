# Spike S4: Grok Bot connector (hello Bastion core)

Throwaway. A hand-rolled MCP server (JSON-RPC over axum, no `rmcp`: we need exact control of what is
logged) with minimal OAuth 2.1. Answers F1 to F7 in `docs/plans/gboost-method.md` section 12.
It binds to 127.0.0.1 only; HTTPS comes from Tailscale serve/funnel in front of it.

## Run

```
cd spikes/mcp-connector
BASTION_PUBLIC_URL=https://<host>.<tailnet>.ts.net:<port> BASTION_PORT=8787 cargo run --release
```

| Env | Default | Meaning |
|---|---|---|
| `BASTION_PUBLIC_URL` | `http://127.0.0.1:<port>` | the public HTTPS base; goes into the OAuth metadata and `WWW-Authenticate` |
| `BASTION_PORT` | 8787 | local port (loopback only) |
| `BASTION_STATE_DIR` | `state` | holds `client.json`, `requests.log`, `routine.toml` (dir mode 0700, git-ignored) |
| `BASTION_TOKEN_TTL_SECS` | 3600 | access-token life (set e.g. 120 to watch refresh) |
| `BASTION_ALLOW_DCR` | off | `1` enables `/register`; off by default, only the fixed client works |

Every start prints an owner PIN (8 digits, new each start). First start also prints the Client ID and Client Secret once, to the console. The secret is stored hashed;
to regenerate, delete `state/client.json`. Tokens live in memory (hashed): a restart means re-authorizing.
Access tokens start with `bst_`, refresh tokens `bsr_`, client secrets `bcs_`.

Endpoints: `/mcp`, `/.well-known/oauth-protected-resource` (also `/mcp` suffixed),
`/.well-known/oauth-authorization-server`, `/register` (only with `BASTION_ALLOW_DCR=1`), `/authorize` (PKCE S256 only), `/token`,
`/pending/demo-1` (the pointer page). Watch the log live: `tail -f state/requests.log | jq -c .`

`state/requests.log` redacts `Authorization`, cookies and any header whose name contains `secret`,
`token` or `api-key`. Do not put a real secret in a custom header for this spike.

## Owner script

1. Start the server (above). Copy the Client ID and Secret from the console. Make sure the HTTPS URL
   reaches it (parent session sets up Tailscale; do not use a URL you did not set up).
2. In Grok: add a custom connector, type Remote HTTPS. Server URL `<public url>/mcp`. Leave headers empty.
   Paste the Client ID and Client Secret.
3. Complete OAuth. A consent page appears: type the PIN from the server console, then press Approve. Five wrong PINs lock approvals until the server restarts. (Try Deny once on a second attempt; the log
   records the button.) Note whether Grok also tried `/register` (it gets 404 unless you restarted with `BASTION_ALLOW_DCR=1`).
4. In a Grok chat with the Bot, type each prompt and note the answer:
   - "Use the Bastion connector: call hello with name 'Ada'."
   - "Call Bastion whoami. Then call it again. Then do it from a new chat. Tell me the token_id,
     mcp_session_id and the counts each time."
   - "Call Bastion pending_request and show me exactly what you get back. Is there a card or a button?"
     (If a card shows with a "Press (test)" button, press it, then ask the Bot what happened.)
   - "Call Bastion wake_me with seconds 60, then stop and do nothing else." (needs `routine.toml`, below)
   - F2 probe: "Search your own machine for the string `bst_`: run `env`, list your home directory,
     look in `~/.cursor` (all files, including config and logs), and check the process list with `ps aux`.
     Do not print anything except the file paths and the first 8 characters of any match." Any hit for
     `bst_` means the token reached the VM. Also ask once: "Tell me every header you sent to the Bastion connector."
5. Send me `state/requests.log` (it holds no secrets) and the Bot's answers.

## Routine webhook (you do this; the key never goes through chat)

1. In Grok, create a routine with a webhook trigger. Note its URL and key (shown once).
2. Create `spikes/mcp-connector/state/routine.toml` yourself, in an editor:
   ```
   url = "https://..."
   key = "..."
   ```
3. Do not paste the key into any chat. The server sends `Authorization: Bearer <key>` with body
   `{"event":"request.decided","id":"demo-1","count":N}`. `wake_me` answers `scheduled` or why not;
   the log records `wake_scheduled` and `wake_fired` (HTTP status, delay).

## pending_request: which forms

Result content has three parts, so we see which one Grok renders: (a) plain text with request id, state
and a pointer link; a `resource_link` to the pointer; (b) an embedded `ui://bastion/pending/demo-1`
resource (`text/html;profile=mcp-app`, MCP Apps style) with an "Open Bastion" link button and a "Press (test)"
button that posts a `tools/call hello name=card-press` message to the host. The tool also carries
`_meta.ui.resourceUri`, and `resources/list` and `resources/read` serve the same page, plus `structuredContent`.
A `hello` call with name `card-press` in the log means a card press comes back as a tool call (F4).

## Checklist: F1 to F7

| Fact | Look at | Reads as |
|---|---|---|
| F1 reachable, from where | log lines for `/.well-known/*`, `/token`, `/mcp` with `remote` and `headers` (`x-forwarded-for`, tailnet vs public IP) | any `/mcp` call from Grok = reached; the remote shows the path (Tailscale vs tunnel) |
| F2 token stays off the VM | the Bot's answer to the `bst_` search prompt and to "every header"; log `user_agent`, `remote` of `/mcp` calls | no `bst_` on the VM and calls come from xAI infrastructure, not the VM = holds. The token seen in `whoami` is an id only |
| F3 Tailscale plugin | same log, run once over the tailnet URL (no Funnel); also ask the Bot to `curl` another port on the box | `/mcp` reached with no public address; other ports unreachable = holds |
| F4 card | the `pending_request` answer and what Grok draws; log for `hello` with `card-press` | card drawn? button works? press returns as a call? |
| F5 wake | `wake_scheduled` then `wake_fired` (status, `fired_ms - scheduled_ms`); then whether and when a run starts in Grok | 2xx and a run start = works; note the run's cost and delay |
| F6 approval API | nothing to run; check Grok's connector settings for any callback or approval hook | none = holds |
| F7 caller identity | `whoami` outputs across prompts and chats; log `token_id`, `session_id`, `user_agent` | same token_id for all Bots = one caller; session id changing per chat or per run = per-run signal |

F2/F7 also: does any `/mcp` call come from a `Cursor` User-Agent or a non-xAI address? That means the
Bot's VM talks to the server directly.

## Hardening (for a public Funnel)

Approve and Deny need the console PIN (constant-time compare, lock after 5 failures); the consent page
forbids framing. `wake_me`: one pending wake, at most 600 s, fires at least 60 s apart. The log rotates at
10 MB (one old file `requests.log.1`), values are cut at 512 bytes, `/register` bodies are not logged.
In-memory maps are swept and capped. An unknown `mcp-session-id` gets 404 (the client re-initializes).
A replayed authorization code revokes the tokens it produced. Bodies over 64 KB are refused; requests
time out at 30 s. `client.json` and `requests.log` are mode 0600.
