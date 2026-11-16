import json, os, re, shutil, subprocess, sys, tempfile, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

def mode_is(path, want):
    """Unix file mode check; Windows has no modes (the user-profile ACL applies), so it passes there."""
    return os.name == "nt" or oct(os.stat(path).st_mode & 0o777) == want


ROOT = sys.argv[1]
CLIENT = os.path.join(ROOT, "client", "gboost_client.py")
BIN = os.path.join(ROOT, "core", "target", "debug", "gboost-s9-core" + (".exe" if os.name == "nt" else ""))
KEY = "e2e_webhook_key_not_a_real_one"
tmp = tempfile.mkdtemp(prefix="s9e2e_")
state, chome = os.path.join(tmp, "state"), os.path.join(tmp, "client-home")
os.makedirs(state)
fails = []
webhook_hits = []


def step(name, ok, detail=""):
    print("%s  %s%s" % ("PASS" if ok else "FAIL", name, ("  [" + detail + "]") if detail and not ok else ""))
    sys.stdout.flush()
    if not ok:
        fails.append(name)


class Hook(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        webhook_hits.append((self.headers.get("authorization"), body))
        self.send_response(200); self.end_headers(); self.wfile.write(b"ok")
        # the "routine run": a separate client process calls gboost_deliver_code
        threading.Thread(target=routine_run, args=(body["code"], body.get("check_id")), daemon=True).start()


def routine_run(code, check_id):
    time.sleep(0.5)
    p = subprocess.Popen([sys.executable, CLIENT], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                         env=dict(os.environ, GBOOST_CLIENT_HOME=chome))
    msgs = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "gboost_deliver_code", "arguments": {"code": code, "check_id": check_id}}}]
    p.communicate("\n".join(json.dumps(m) for m in msgs) + "\n", timeout=20)


hook = HTTPServer(("127.0.0.1", 0), Hook)
threading.Thread(target=hook.serve_forever, daemon=True).start()
with open(os.path.join(state, "routine.toml"), "w") as f:
    f.write('url = "http://127.0.0.1:%d/hook"\nkey = "%s"\n' % (hook.server_port, KEY))

core = subprocess.Popen([BIN], env=dict(os.environ, S9_AGENT_BIND="127.0.0.1:0", S9_OWNER_BIND="127.0.0.1:0", S9_STATE_DIR=state),
                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
agent = owner = None
for _ in range(2):
    line = core.stdout.readline()
    m = re.search(r"(Agent|Owner) listener: (http://\S+)", line)
    if m:
        if m.group(1) == "Agent": agent = m.group(2)
        else: owner = m.group(2)
step("core started (agent %s, owner %s)" % (agent, owner), bool(agent and owner))
if not (agent and owner):
    core.kill(); sys.exit(1)
token = open(os.path.join(state, "console.token")).read().strip()


def owner_call(method, path):
    req = urllib.request.Request(owner + path, method=method, headers={"x-console-token": token})
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())


env = dict(os.environ, GBOOST_CLIENT_HOME=chome, GBOOST_WAIT_S="30")
try:
    # download the client file like the Bot would
    with urllib.request.urlopen(agent + "/client/gboost_client.py", timeout=10) as r:
        step("client file downloadable", r.read().decode() == open(CLIENT).read())

    r = subprocess.run([sys.executable, CLIENT, "enroll", agent], env=env, capture_output=True, text=True)
    m = re.search(r"client_id:\s+(gbc_\S+)", r.stdout)
    fp = re.search(r"fingerprint:\s+([0-9a-f]{16})", r.stdout)
    step("enroll via CLI prints id, fingerprint, ask-owner line", r.returncode == 0 and m and fp and "Ask the owner to approve" in r.stdout, r.stdout + r.stderr)
    cid = m.group(1)
    mode = oct(os.stat(os.path.join(chome, "config.json")).st_mode & 0o777)
    step("config.json is 0600, home is 0700", mode_is(os.path.join(chome, "config.json"), "0o600") and mode_is(chome, "0o700"), mode)
    clients = owner_call("GET", "/local/clients")["clients"]
    step("owner sees pending client with same fingerprint", clients[0]["state"] == "pending" and clients[0]["fingerprint"] == fp.group(1))

    host = subprocess.Popen([sys.executable, CLIENT], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env, bufsize=1)
    nid = [0]

    def rpc(method, params=None):
        nid[0] += 1
        host.stdin.write(json.dumps({"jsonrpc": "2.0", "id": nid[0], "method": method, "params": params or {}}) + "\n")
        host.stdin.flush()
        resp = json.loads(host.stdout.readline())
        assert resp["id"] == nid[0]
        return resp

    def call(tool, args=None):
        res = rpc("tools/call", {"name": tool, "arguments": args or {}})["result"]
        return res["isError"], res["content"][0]["text"]

    r = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}})
    step("initialize echoes protocolVersion", r["result"]["protocolVersion"] == "2025-06-18" and r["result"]["serverInfo"]["name"] == "gboost-client")
    host.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"); host.stdin.flush()
    step("ping", rpc("ping")["result"] == {})
    names = [t["name"] for t in rpc("tools/list")["result"]["tools"]]
    step("tools/list has all five tools", sorted(names) == sorted(["gboost_hello", "gboost_whoami", "gboost_pending_request", "gboost_deliver_code", "gboost_status"]), str(names))

    err, txt = call("gboost_status")
    step("gboost_status before approval: enrolled, no session", not err and "no valid session" in txt, txt)

    t0 = time.time()
    err, txt = call("gboost_hello", {"name": "Bot"})
    step("hello before approval is refused", err and "not approved" in txt, txt)
    ap = owner_call("POST", "/local/clients/%s/approve" % cid)
    step("owner approved client", ap == {"id": cid, "state": "approved"}, str(ap))
    step("owner list shows approved", owner_call("GET", "/local/clients")["clients"][0]["state"] == "approved")

    t0 = time.time()
    err, txt = call("gboost_hello", {"name": "Bot"})
    dt = time.time() - t0
    step("hello succeeds after webhook round-trip (%.1fs)" % dt, not err and txt == "Hello, Bot!", txt)
    step("webhook got bearer key and only allowed fields", len(webhook_hits) == 1 and webhook_hits[0][0] == "Bearer " + KEY
         and sorted(webhook_hits[0][1]) == ["check_id", "client_fingerprint", "code", "kind", "source"] and webhook_hits[0][1]["client_fingerprint"] == fp.group(1), str(webhook_hits))
    step("session.json is 0600, code file consumed", mode_is(os.path.join(chome, "session.json"), "0o600") and not os.path.exists(os.path.join(chome, "code")))

    err, txt = call("gboost_whoami")
    w = json.loads(txt) if not err else {}
    step("whoami", not err and w.get("client_id") == cid and w.get("calls_used") == 2, txt)
    err, txt = call("gboost_pending_request")
    step("pending_request", not err and "req_demo" in txt, txt)
    err, txt = call("gboost_status")
    step("gboost_status shows valid session", not err and "UTC" in txt and "no valid" not in txt, txt)
    step("only one webhook so far (session reused)", len(webhook_hits) == 1)

    owner_call("POST", "/local/clients/%s/revoke" % cid)
    err, txt = call("gboost_whoami")
    step("after revoke the next call fails", err and ("401" in txt or "revoked" in txt), txt)

    out = subprocess.run([sys.executable, CLIENT, "status"], env=env, capture_output=True, text=True).stdout
    step("status CLI works", cid in out)
    host.stdin.close()
    host.wait(timeout=5)
    secret = json.load(open(os.path.join(chome, "config.json")))["secret"]
    logs = "".join(open(os.path.join(state, n)).read() for n in ("requests.log", "security.log")) + host.stderr.read() + out
    sec = open(os.path.join(state, "security.log")).read()
    step("security.log has enrol, approved, session_created, revoked", all(k in sec for k in ('"enrol"', '"approved"', '"session_created"', '"revoked"')), sec)
    step("security.log is 0600", mode_is(os.path.join(state, "security.log"), "0o600"))
    step("no secrets in core log, client stderr or status output", secret not in logs and KEY not in logs and token not in logs)
finally:
    core.kill()
    core.wait()
    hook.shutdown()
    shutil.rmtree(tmp, ignore_errors=True)
print("\n%s (%d failed)" % ("E2E PASS" if not fails else "E2E FAIL", len(fails)))
sys.exit(1 if fails else 0)
