import json, os, re, shutil, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

def mode_is(path, want):
    """Unix file mode check; Windows has no modes (the user-profile ACL applies), so it passes there."""
    return os.name == "nt" or oct(os.stat(path).st_mode & 0o777) == want


ROOT = sys.argv[1]
BIN = os.path.join(ROOT, "core", "target", "debug", "gboost-s9-core" + (".exe" if os.name == "nt" else ""))
WH_KEY = "e2e_webhook_key_not_a_real_one"
tmp = tempfile.mkdtemp(prefix="s9remote_")
state = os.path.join(tmp, "state")
os.makedirs(state)
fails = []
hits = []
box = {}  # filled once the core runs: agent url and key


def step(name, ok, detail=""):
    print("%s  %s%s" % ("PASS" if ok else "FAIL", name, ("  [" + detail + "]") if detail and not ok else ""))
    sys.stdout.flush()
    if not ok:
        fails.append(name)


def mcp(key, method=None, params=None, id_=1, extra=None, agent=None):
    msg = {"jsonrpc": "2.0", "method": method}
    if id_ is not None:
        msg["id"] = id_
    if params is not None:
        msg["params"] = params
    h = {"content-type": "application/json", "accept": "application/json, text/event-stream"}
    if key:
        h["x-gboost-client"] = key
    h.update(extra or {})
    req = urllib.request.Request((agent or box["agent"]) + "/mcp", data=json.dumps(msg).encode(), headers=h, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            body = r.read()
            return r.status, dict(r.headers), (json.loads(body) if body else None)
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), json.loads(e.read() or b"null")


def call(key, tool, args=None, extra=None):
    st, _, b = mcp(key, "tools/call", {"name": tool, "arguments": args or {}}, extra=extra)
    r = b["result"]
    return r["isError"], r["content"][0]["text"]


class Hook(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        hits.append((self.headers.get("authorization"), body))
        self.send_response(200); self.end_headers(); self.wfile.write(b"ok")
        # the "routine run": calls gboost_deliver_code through the same connector, same header
        threading.Thread(target=routine_run, args=(body["code"], body.get("check_id")), daemon=True).start()


def routine_run(code, check_id):
    time.sleep(0.3)
    box["deliver"] = call(box["key"], "gboost_deliver_code", {"code": code, "check_id": check_id})
    box["delivered"].set()


box["delivered"] = threading.Event()
hook = HTTPServer(("127.0.0.1", 0), Hook)
threading.Thread(target=hook.serve_forever, daemon=True).start()
with open(os.path.join(state, "routine.toml"), "w") as f:
    f.write('url = "http://127.0.0.1:%d/hook"\nkey = "%s"\n' % (hook.server_port, WH_KEY))

core = subprocess.Popen([BIN], env=dict(os.environ, S9_AGENT_BIND="127.0.0.1:0", S9_OWNER_BIND="127.0.0.1:0", S9_STATE_DIR=state),
                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
agent = owner = None
for _ in range(2):
    m = re.search(r"(Agent|Owner) listener: (http://\S+)", core.stdout.readline())
    if m:
        if m.group(1) == "Agent": agent = m.group(2)
        else: owner = m.group(2)
step("core started", bool(agent and owner))
if not (agent and owner):
    core.kill(); sys.exit(1)
box["agent"] = agent
token = open(os.path.join(state, "console.token")).read().strip()
try:
    # owner creates the client; the key goes to a file, not to the terminal
    req = urllib.request.Request(owner + "/local/clients/new?to_file=1", method="POST", headers={"x-console-token": token})
    with urllib.request.urlopen(req, timeout=10) as r:
        made = json.loads(r.read())
    kf = os.path.join(state, "new-client.txt")
    key = open(kf).read().strip()
    box["key"] = key
    secret = key.split(":", 1)[1]
    step("owner created a client; key only in state/new-client.txt (0600)",
         mode_is(kf, "0o600") and secret not in json.dumps(made) and key.startswith(made["client_id"]))

    st, hd, b = mcp(None, "tools/list")
    step("no key -> 401, JSON-RPC error, no WWW-Authenticate", st == 401 and "error" in b and not any(k.lower() == "www-authenticate" for k in hd))
    st, hd, b = mcp(made["client_id"] + ":wrong", "tools/list")
    step("bad key -> 401", st == 401 and not any(k.lower() == "www-authenticate" for k in hd))

    st, hd, b = mcp(key, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}})
    sid = hd.get("mcp-session-id") or hd.get("Mcp-Session-Id")
    step("initialize echoes protocolVersion, returns mcp-session-id", st == 200 and b["result"]["protocolVersion"] == "2025-06-18" and bool(sid))
    h2 = {"mcp-session-id": sid}
    st, _, b = mcp(key, "notifications/initialized", id_=None, extra=h2)
    step("notifications/initialized -> 202", st == 202)
    st, _, b = mcp(key, "tools/list", extra=h2)
    names = [t["name"] for t in b["result"]["tools"]]
    step("tools/list shows 5 tools with the key only", st == 200 and len(names) == 5 and "gboost_deliver_code" in names and not hits)

    err, txt = call(key, "gboost_hello", extra=h2)
    step("hello without session: isError with the check message", err and "session check to your routine webhook" in txt)
    ok = box["delivered"].wait(10)
    step("routine webhook got the check (source/kind/code/check_id/fingerprint)",
         ok and len(hits) == 1 and hits[0][0] == "Bearer " + WH_KEY and sorted(hits[0][1]) == ["check_id", "client_fingerprint", "code", "kind", "source"]
         and hits[0][1]["client_fingerprint"] == made["fingerprint"])
    step("routine run delivered the code through gboost_deliver_code", box.get("deliver") == (False, "Session open"), str(box.get("deliver")))

    err, txt = call(key, "gboost_hello", {"name": "Bot"}, extra=h2)
    step("hello after the check works", (err, txt) == (False, "Hello, Bot!"), txt)
    err, txt = call(key, "gboost_whoami", extra=h2)
    w = json.loads(txt) if not err else {}
    step("whoami: client, session, budget, mcp-session-id, peer", w.get("client_id") == made["client_id"] and w.get("calls_used") == 2
         and w.get("claimed_by_caller", {}).get("mcp_session_id") == sid and str(w.get("seen", {}).get("peer", "")).startswith("127.0.0.1:"), txt)
    err, txt = call(key, "gboost_pending_request", extra=h2)
    step("pending_request works", not err and "req_demo" in txt)
    err, txt = call(key, "gboost_deliver_code", {"code": hits[0][1]["code"]})
    step("the code is single use", err)
    err, txt = call(key, "gboost_status")
    step("status: session active, nothing pending", json.loads(txt).get("session_active") is True and json.loads(txt).get("check_pending") is False)
    step("only one webhook call in total", len(hits) == 1)

    req = urllib.request.Request(owner + "/local/clients/%s/revoke" % made["client_id"], method="POST", headers={"x-console-token": token})
    urllib.request.urlopen(req, timeout=10).read()
    st, _, _ = mcp(key, "tools/list")
    step("revoked client gets 401", st == 401)
finally:
    core.kill()
    hook.shutdown()
    shutil.rmtree(tmp, ignore_errors=True)
print("RESULT: %s" % ("FAIL (%d)" % len(fails) if fails else "ALL PASS"))
sys.exit(1 if fails else 0)
