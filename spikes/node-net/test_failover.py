#!/usr/bin/env python3
"""Failover check for gboost_client.py: two S9 cores on loopback (no Tailscale), one client key.

Run: python3 -I test_failover.py <path to gboost-s9-core binary>
"""
import json, os, shutil, socket, subprocess, sys, tempfile, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CORE = sys.argv[1]
fails = 0


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else "  (%s)" % detail), flush=True)


def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p


def wait_port(p):
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", p), 0.2).close(); return True
        except OSError:
            time.sleep(0.1)
    return False


tmp = tempfile.mkdtemp(prefix="s10_")
cores = {}


def start_core(name, agent, owner):
    st = os.path.join(tmp, name)
    os.makedirs(st, exist_ok=True)
    env = dict(os.environ, S9_STATE_DIR=st, S9_AGENT_BIND="127.0.0.1:%d" % agent, S9_OWNER_BIND="127.0.0.1:%d" % owner)
    cores[name] = subprocess.Popen([CORE], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    assert wait_port(agent) and wait_port(owner), name
    return st


pa, oa, pb, ob = free_port(), free_port(), free_port(), free_port()
sa = start_core("a", pa, oa)
tok = open(os.path.join(sa, "console.token")).read().strip()
req = urllib.request.Request("http://127.0.0.1:%d/local/clients/new" % oa, method="POST", headers={"x-console-token": tok})
made = json.load(urllib.request.urlopen(req))
key = made["header_value"]
# Node B knows the same client (hash only), as the node set will replicate it.
sb_dir = os.path.join(tmp, "b"); os.makedirs(sb_dir)
shutil.copy(os.path.join(sa, "clients.json"), os.path.join(sb_dir, "clients.json"))
start_core("b", pb, ob)

home = os.path.join(tmp, "client")
os.makedirs(home)
json.dump({"nodes": ["http://127.0.0.1:%d/mcp" % pa, "http://127.0.0.1:%d/mcp" % pb], "tailscale": {"enabled": False}},
          open(os.path.join(home, "config.json"), "w"))
open(os.path.join(home, "client.key"), "w").write(key)
env = dict(os.environ, GBOOST_CLIENT_HOME=home, GBOOST_RETRY_PRIMARY_S="2", GBOOST_TIMEOUT_S="3")
cl = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "gboost_client.py")], env=env,
                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
n = [0]


def call(method, params=None, notify=False):
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        n[0] += 1; msg["id"] = n[0]
    cl.stdin.write(json.dumps(msg) + "\n"); cl.stdin.flush()
    return None if notify else json.loads(cl.stdout.readline())


def status():
    r = call("tools/call", {"name": "gboost_status", "arguments": {}})
    return r


def served_by():
    """Which core saw the last /mcp call: compare request logs."""
    def count(name):
        try:
            return sum(1 for l in open(os.path.join(tmp, name, "requests.log")) if '"/mcp"' in l)
        except OSError:
            return 0
    return count("a"), count("b")


r = call("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}})
step("initialize through the client", "result" in r and r["result"].get("serverInfo"), r)
call("notifications/initialized", notify=True)
r = call("tools/list")
step("tools/list", "result" in r and any(t["name"] == "gboost_status" for t in r["result"]["tools"]), r)
before = served_by()
r = status()
step("status from node A", "result" in r and not r["result"]["isError"], r)
after = served_by()
step("node A served it", after[0] > before[0] and after[1] == before[1], (before, after))

cores["a"].terminate(); cores["a"].wait()
before = served_by()
t0 = time.time()
r = status()
dt = time.time() - t0
step("status after node A stops (failover)", "result" in r and not r["result"]["isError"], r)
after = served_by()
step("node B served it, initialize replayed", after[1] >= before[1] + 2, (before, after))
step("failover took under 5 s", dt < 5, "%.1fs" % dt)
r = status()
step("next call stays on B", "result" in r and not r["result"]["isError"], r)

# Node A comes back; after the back-off the client prefers it again.
start_core("a", pa, oa)
time.sleep(2.5)
before = served_by()
r = status()
after = served_by()
step("back on node A after it returns", "result" in r and after[0] > before[0], (before, after))

cores["b"].terminate(); cores["a"].terminate()
cores["a"].wait(); cores["b"].wait()
r = status()
step("no node: clear error", "error" in r and "No GBOOST node" in r["error"]["message"], r)

cl.stdin.close(); cl.wait(5)
err = cl.stderr.read()
step("client log has no key", key.split(":")[1] not in err, "")

# A node behind `tailscale serve` whose core died answers 502; an MCP server may answer with SSE.
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer


class Bad(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length") or 0))
        self.send_response(502); self.send_header("Content-Length", "0"); self.end_headers()


class Sse(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        m = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if "id" not in m:
            self.send_response(202); self.send_header("Content-Length", "0"); self.end_headers(); return
        res = {"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "sse", "version": "0"}} \
            if m["method"] == "initialize" else {"content": [{"type": "text", "text": "sse ok"}], "isError": False}
        body = ("event: message\ndata: " + json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": res}) + "\n\n").encode()
        self.send_response(200); self.send_header("Content-Type", "text/event-stream")
        self.send_header("mcp-session-id", "sse-1"); self.send_header("Content-Length", str(len(body))); self.end_headers()
        self.wfile.write(body)


bad, sse = HTTPServer(("127.0.0.1", 0), Bad), HTTPServer(("127.0.0.1", 0), Sse)
for srv in (bad, sse):
    threading.Thread(target=srv.serve_forever, daemon=True).start()
json.dump({"nodes": ["http://127.0.0.1:%d/mcp" % bad.server_port, "http://127.0.0.1:%d/mcp" % sse.server_port],
           "tailscale": {"enabled": False}}, open(os.path.join(home, "config.json"), "w"))
cl = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "gboost_client.py")], env=env,
                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
r = call("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
step("502 node is skipped, SSE answer parsed", r.get("result", {}).get("serverInfo", {}).get("name") == "sse", r)
r = call("tools/call", {"name": "x", "arguments": {}})
step("tool call over SSE", r.get("result", {}).get("content", [{}])[0].get("text") == "sse ok", r)
cl.stdin.write("not json\n"); cl.stdin.flush()
r = json.loads(cl.stdout.readline())
step("bad input gets a parse error, client stays up", r.get("error", {}).get("code") == -32700 and cl.poll() is None, r)
cl.stdin.close(); cl.wait(5)
print("RESULT:", "ALL PASS" if fails == 0 else "%d FAILED" % fails)
shutil.rmtree(tmp, ignore_errors=True)
sys.exit(1 if fails else 0)
