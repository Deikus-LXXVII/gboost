#!/usr/bin/env python3
"""Client cold start and device self-heal (S7 findings). No Tailscale: a fake device stands in for it.

  python3 -I test_client_start.py

- initialize is answered at once even while the tailnet device is still starting;
- tools/list comes from the cache when the device is not up yet, and from a node (refreshing the cache)
  once it is;
- when every node fails without a connection, a hung or offline device is restarted once and the call
  succeeds; within a minute a second failure does not restart it again.
"""
import json, os, sys, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, HTTPServer

HOME = tempfile.mkdtemp(prefix="s7c_")
os.environ.update(GBOOST_CLIENT_HOME=HOME, GBOOST_WARMUP_S="5", GBOOST_LIST_WAIT_S="0.5", GBOOST_HEAL_EVERY_S="60",
                  GBOOST_TIMEOUT_S="2")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gboost_client  # noqa: E402

fails = 0


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else "  (%s)" % str(detail)[-600:]), flush=True)


TOOLS = [{"name": "gboost_hello", "inputSchema": {"type": "object"}}]


class Node(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def do_POST(self):
        m = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if "id" not in m:
            self.send_response(202); self.send_header("Content-Length", "0"); self.end_headers(); return
        res = {"tools": TOOLS} if m["method"] == "tools/list" else \
            {"protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": {"name": "n", "version": "0"}} \
            if m["method"] == "initialize" else {"content": [{"type": "text", "text": "ok"}]}
        body = json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": res}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)


class NodeCtl:
    """A node whose port answers only while 'up' (the tailnet path to it works)."""
    def __init__(self):
        probe = HTTPServer(("127.0.0.1", 0), Node); self.port = probe.server_port; probe.server_close()
        self.srv = None

    def up(self):
        if not self.srv:
            self.srv = HTTPServer(("127.0.0.1", self.port), Node)
            threading.Thread(target=self.srv.serve_forever, daemon=True).start()

    def down(self):
        if self.srv:
            self.srv.shutdown(); self.srv.server_close(); self.srv = None


class FakeDevice:
    proxy_port = None  # no proxy: requests go straight to the loopback node

    def __init__(self, node, start_delay=0.0):
        self.node, self.start_delay, self.restarts, self.starts, self.state_, self.proc = node, start_delay, 0, 0, "online", None

    def start(self):
        self.starts += 1
        time.sleep(self.start_delay)

    def bring_up(self, wait_s=15):
        return "Running", "gboost-client.test"

    def health(self):
        return self.state_

    def online(self):
        return self.state_ == "online"

    def restart(self):
        self.restarts += 1
        self.state_ = "online"
        self.node.up()  # a fresh tailscaled reaches the node again

    def state(self):
        return "Running", None, "gboost-client.test"


def call(c, method, params=None, id_=1):
    return c.send({"jsonrpc": "2.0", "id": id_, "method": method, "params": params or {}})


node = NodeCtl()
node.up()
cfg = {"nodes": ["http://127.0.0.1:%d/mcp" % node.port], "tailscale": {"enabled": False}}
INIT = {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}

# 1. Cold start with a slow device and no cache: initialize at once; tools/list waits for the device.
dev = FakeDevice(node, start_delay=2.0)
c = gboost_client.Client(cfg, "gbc_x:secret", dev)
threading.Thread(target=c.bring_up_device, daemon=True).start()
t0 = time.time(); r = call(c, "initialize", INIT); dt = time.time() - t0
step("initialize answered at once while the device starts", dt < 0.2 and r["result"]["serverInfo"]["name"] == "gboost", (dt, r))
step("notifications/initialized is not forwarded", c.send({"jsonrpc": "2.0", "method": "notifications/initialized"}) is None)
t0 = time.time(); r = call(c, "tools/list", id_=2); dt = time.time() - t0
step("no cache: tools/list waits for the device and comes from the node", r.get("result", {}).get("tools") == TOOLS
     and c.ready.is_set() and dev.starts == 1 and dt >= 1.8, (dt, c.ready.is_set(), r))
step("the node's tool list is cached", json.load(open(os.path.join(HOME, "tools.json"))) == TOOLS)

# 2. Cold start with a cache: tools/list within LIST_WAIT_S.
dev = FakeDevice(node, start_delay=3.0)
c = gboost_client.Client(cfg, "gbc_x:secret", dev)
threading.Thread(target=c.bring_up_device, daemon=True).start()
call(c, "initialize", INIT)
t0 = time.time(); r = call(c, "tools/list", id_=3); dt = time.time() - t0
step("with a cache: tools/list in under 1 s while the device starts", dt < 1.0 and r.get("result", {}).get("tools") == TOOLS
     and r.get("id") == 3, (dt, r))
t0 = time.time(); r = call(c, "tools/call", {"name": "gboost_hello", "arguments": {}}, id_=4); dt = time.time() - t0
step("the first tool call waits for the device and reaches the node", r.get("result", {}).get("content", [{}])[0].get("text") == "ok"
     and c.ready.is_set() and dt >= 1.5, (dt, r))

# 3. A healthy device or one waiting for the owner's login is never restarted; an offline one is, once.
c.started -= 10  # past the warm-up
node.down()
for state in ("online", "needs_login"):
    dev.state_ = state
    r = call(c, "tools/call", {"name": "gboost_hello", "arguments": {}}, id_=50)
    step("a device that is %s is not restarted" % state, dev.restarts == 0 and "No GBOOST node is reachable" in r.get("error", {}).get("message", ""), (dev.restarts, r))
dev.state_ = "offline"
r = call(c, "tools/call", {"name": "gboost_hello", "arguments": {}}, id_=5)
step("hung device restarted once; the call succeeds", dev.restarts == 1 and r.get("result", {}).get("content", [{}])[0].get("text") == "ok",
     (dev.restarts, r))
log = open(os.path.join(HOME, "client.log"), encoding="utf-8").read()
step("the restart is in client.log", "the tailnet device is offline; restarting it" in log, log[-400:])

# 4. Within a minute, another outage does not restart the device again (rate limit across processes).
node.down()
dev.state_ = "offline"
starts = dev.starts
r = call(c, "tools/call", {"name": "gboost_hello", "arguments": {}}, id_=6)
step("no second restart within a minute; clear error", dev.restarts == 1 and dev.starts == starts + 1
     and "No GBOOST node is reachable" in r.get("error", {}).get("message", ""), (dev.restarts, r))

# 5. A device that did not start gives a clear error, not a hang.
class DeadDevice(FakeDevice):
    def start(self):
        raise RuntimeError("tailscaled exited")
c = gboost_client.Client(cfg, "gbc_x:secret", DeadDevice(node))
threading.Thread(target=c.bring_up_device, daemon=True).start()
call(c, "initialize", INIT)
r = call(c, "tools/call", {"name": "gboost_hello", "arguments": {}}, id_=7)
step("a device that cannot start gives a clear error", "tailnet device did not start" in r.get("error", {}).get("message", ""), r)

# 6. The self-test finds Tailscale the way the client does: a relative bin_dir is inside the client folder,
#    whatever the current directory.
import subprocess
h2 = tempfile.mkdtemp(prefix="s7h_")
os.makedirs(os.path.join(h2, "bin"))
for b in ("tailscaled", "tailscale"):
    f = os.path.join(h2, "bin", b + (".exe" if os.name == "nt" else "")); open(f, "w").close(); os.chmod(f, 0o755)
json.dump({"nodes": ["https://n.example/mcp"], "tailscale": {"enabled": True, "bin_dir": "bin"}}, open(os.path.join(h2, "config.json"), "w"))
open(os.path.join(h2, "client.key"), "w").write("gbc_x:secret")
r = subprocess.run([sys.executable, "-I", gboost_client.__file__, "selftest"], capture_output=True, text=True,
                   cwd=os.path.dirname(os.path.abspath(os.sep)), env=dict(os.environ, GBOOST_CLIENT_HOME=h2))
step("self-test resolves a relative bin_dir in the client folder, from any cwd", r.returncode == 0 and r.stdout.startswith("SELFTEST OK"), r.stdout + r.stderr)

node.down()
print("RESULT:", "ALL PASS" if fails == 0 else "%d FAILED" % fails)
sys.exit(1 if fails else 0)
