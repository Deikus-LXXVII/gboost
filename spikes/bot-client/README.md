# S9: gboost-client on the Bot's VM

To install it the supported way, see the repository's [`INSTALL.md`](../../INSTALL.md). The notes below are for development.

Spike for decision D16 (see `docs/plans/phase-0.md`, S9). The Bot runs `gboost_client.py` as a Command (stdio)
connector. The client enrols once (owner approves its fingerprint), and each session is opened only after the
core posts a random code to the owner's routine webhook and the routine run hands it back through
`gboost_deliver_code`.

- `core/` Rust/axum, two listeners: agent (`S9_AGENT_BIND`, default 127.0.0.1:8790)
  and owner (`S9_OWNER_BIND`, loopback only, default 127.0.0.1:8791, header `x-console-token` from `state/console.token`).
- `client/gboost_client.py` Python 3.9+ stdlib only.
- `scripts/e2e.sh` full run with a fake routine receiver.

Other env: `S9_STATE_DIR` (default `state`), `S9_SESSION_TTL_S` (default 14400), `S9_CODE_TTL_S` (default 300).
Client env: `GBOOST_CLIENT_HOME`, `GBOOST_WAIT_S` (default 90).

## Run and test

    cd core && cargo test                 # integration tests
    scripts/e2e.sh                        # builds, runs everything end to end
    S9_AGENT_BIND=<tailnet-ip>:8790 cargo run --manifest-path core/Cargo.toml

Owner console (token in `state/console.token`):

    T=$(cat state/console.token)
    curl -H "x-console-token: $T" localhost:8791/local/clients
    curl -XPOST -H "x-console-token: $T" localhost:8791/local/clients/<id>/approve
    curl -XPOST -H "x-console-token: $T" localhost:8791/local/clients/<id>/revoke
    curl -XPOST -H "x-console-token: $T" localhost:8791/local/sessions/revoke-all

## Owner steps for the real test

1. In Grok, create a routine with a webhook trigger and the instruction: "When woken by GBOOST with kind
   session_check, call the gboost tool gboost_deliver_code with the code from the payload unchanged."
2. Write the routine's webhook URL and key by hand into `state/routine.toml`:

       url = "https://..."
       key = "..."

3. Start the core with `S9_AGENT_BIND=<host tailnet ip>:8790`.
4. On the Bot's VM: download `http://<host tailnet ip>:8790/client/gboost_client.py`, run
   `python3 gboost_client.py enroll http://<host tailnet ip>:8790`, and note the fingerprint.
5. Approve that fingerprint through the owner console (above).
6. Add a Command connector in Grok: `python3 ~/gboost_client.py`, and make sure the routine can use the same
   connector (and the same `~/.gboost-client/` home on the VM).
7. Ask the Bot to call `gboost_hello`; the first call waits for the webhook round trip (up to 90 s).

## Remote HTTPS connector (owner decision 2026-11-13; supersedes the Command client above for the Bot)

The Bot uses a Remote MCP connector in Grok. Its URL is served tailnet-only by `tailscale serve`; there is no
client on the Bot's VM and no OAuth. The core answers `POST /mcp` (MCP Streamable HTTP, plain JSON responses;
`GET /mcp` is 405) on the agent listener. Auth is the header `x-gboost-client: <id>:<secret>`; failures are a
generic 401 with a JSON-RPC error body and no `WWW-Authenticate`. Each client has one server-side session
(no token to hold): `gboost_hello`, `gboost_whoami` and `gboost_pending_request` need it. Without one, the
first call sends the webhook check and returns an error result telling the caller to retry once the routine
has called `gboost_deliver_code`. `initialize` and `tools/list` need only the key. Tool `gboost_status` shows
session and pending state. Revoking a client (or revoke-all) ends its session.

Owner steps:

1. Run the core with the agent listener on loopback and publish it on the tailnet only (not Funnel):

       cargo run --manifest-path core/Cargo.toml        # agent 127.0.0.1:8790
       tailscale serve --bg --https=8790 http://127.0.0.1:8790

2. Make the routine as in "Owner steps for the real test" (steps 1 and 2 above), but its instruction goes
   through the gboost connector: "When woken by GBOOST with kind session_check, call the gboost connector's
   tool gboost_deliver_code with the code from the payload unchanged."
3. Create the client key; it goes to a 0600 file, not to the terminal:

       T=$(cat state/console.token)
       curl -XPOST -H "x-console-token: $T" "localhost:8791/local/clients/new?to_file=1"
       # prints id, fingerprint and the file path; the key is in state/new-client.txt

   Without `?to_file=1` the JSON response carries `header_name` and `header_value`.
4. In Grok: add a custom connector, type Remote HTTPS, URL `https://<mac>.<tailnet>.ts.net:8790/mcp`, add
   header `x-gboost-client` with the content of `state/new-client.txt`. Delete the file afterwards.
5. Ask the Bot to call `gboost_hello`. The first call returns the "session check sent" message; the routine
   run delivers the code; the next call answers.

Test: `cd core && cargo test` and `scripts/e2e_remote.sh` (core, fake routine receiver, an MCP-host-like driver).
