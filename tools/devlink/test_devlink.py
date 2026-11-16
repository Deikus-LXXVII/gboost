#!/usr/bin/env python3
"""End-to-end check of devlink with a fake routine webhook (loopback only). Run: python3 -I test_devlink.py"""
import json, os, socket, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
DL = [sys.executable, "-I", os.path.join(HERE, "devlink.py")]
fails = 0


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else f"  ({detail})"))


def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p


got = []
class Hook(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        got.append((self.headers.get("Authorization"), json.loads(self.rfile.read(int(self.headers["Content-Length"])))))
        self.send_response(200); self.send_header("Content-Length", "0"); self.end_headers()

hook = HTTPServer(("127.0.0.1", 0), Hook)
threading.Thread(target=hook.serve_forever, daemon=True).start()

tmp = tempfile.mkdtemp()
env = dict(os.environ, DEVLINK_HOME=os.path.join(tmp, "home"))
open(os.path.join(tmp, "u"), "w").write("http://127.0.0.1:%d/hook" % hook.server_port)
open(os.path.join(tmp, "k"), "w").write("hookkey123")

r = subprocess.run(DL + ["setup", "--url-file", os.path.join(tmp, "u"), "--key-file", os.path.join(tmp, "k")], env=env, capture_output=True, text=True, encoding="utf-8")
step("setup refuses a plain-http webhook", r.returncode != 0 and "https" in r.stderr, r.stderr)
# The fake hook is http; write the routine file directly, as setup would for https.
os.makedirs(env["DEVLINK_HOME"], exist_ok=True)
r = subprocess.run(DL + ["setup"], env=env, capture_output=True, text=True, encoding="utf-8")
step("setup makes a connector key", r.returncode == 0 and os.path.exists(os.path.join(env["DEVLINK_HOME"], "connector.key")), r.stderr)
with open(os.path.join(env["DEVLINK_HOME"], "routine.json"), "w") as f:
    json.dump({"url": "http://127.0.0.1:%d/hook" % hook.server_port, "key": "hookkey123"}, f)
key = open(os.path.join(env["DEVLINK_HOME"], "connector.key")).read().strip()
if os.name != "nt":
    step("connector.key is 0600", oct(os.stat(os.path.join(env["DEVLINK_HOME"], "connector.key")).st_mode & 0o777) == "0o600")

r = subprocess.run(DL + ["send", "Привет, это тест"], env=env, capture_output=True, text=True, encoding="utf-8")
mid = r.stdout.strip()
step("send posts to the webhook with Bearer key", r.returncode == 0 and got and got[-1][0] == "Bearer hookkey123", r.stderr)
step("payload has kind, id and text", got and got[-1][1]["kind"] == "dev_message" and got[-1][1]["id"] == mid and got[-1][1]["text"] == "Привет, это тест")
step("send output has no secret", "hookkey123" not in r.stdout + r.stderr)

port = free_port()
srv = subprocess.Popen(DL + ["serve", "--bind", "127.0.0.1:%d" % port], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
for _ in range(100):
    try:
        socket.create_connection(("127.0.0.1", port), 1).close(); break
    except OSError:
        if srv.poll() is not None:
            break
        time.sleep(0.2)
else:
    pass
if srv.poll() is not None:
    print("serve exited:", srv.poll(), srv.stderr.read()[-2000:])
    sys.exit(1)

NOPROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def rpc(body, k=key):
    h = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
    if k: h["x-devlink-key"] = k
    req = urllib.request.Request("http://127.0.0.1:%d/mcp" % port, data=json.dumps(body).encode(), headers=h, method="POST")
    try:
        with NOPROXY.open(req, timeout=15) as resp:
            raw = resp.read(); return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        return e.code, None

st, _ = rpc({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}, k=None)
step("no key -> 401", st == 401, st)
st, _ = rpc({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}, k="wrong")
step("wrong key -> 401", st == 401, st)
st, b = rpc({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})
step("initialize", st == 200 and b["result"]["serverInfo"]["name"] == "devlink", b)
st, b = rpc({"jsonrpc": "2.0", "method": "notifications/initialized"})
step("notification -> 202", st == 202, st)
st, b = rpc({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
step("tools/list", st == 200 and [t["name"] for t in b["result"]["tools"]] == ["devlink_send", "devlink_history"], b)

w = subprocess.Popen(DL + ["watch", "--interval", "0.2"], env=env, stdout=subprocess.PIPE, text=True, encoding="utf-8")
time.sleep(0.5)
st, b = rpc({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "devlink_send", "arguments": {"text": "Ответ от бота", "reply_to": mid}}})
step("devlink_send delivers", st == 200 and not b["result"]["isError"], b)
line = w.stdout.readline(); w.kill()
m = json.loads(line)
step("watch prints the new message", m["text"] == "Ответ от бота" and m["reply_to"] == mid, line)
st, b = rpc({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "devlink_send", "arguments": {"text": ""}}})
step("empty text is an error result", b["result"]["isError"], b)
st, b = rpc({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "devlink_history", "arguments": {}}})
txt = b["result"]["content"][0]["text"]
step("history shows both directions", "agent " + mid in txt and "bot bm_" in txt, txt)
if os.name != "nt":
    step("inbox.jsonl is 0600", oct(os.stat(os.path.join(env["DEVLINK_HOME"], "inbox.jsonl")).st_mode & 0o777) == "0o600")
r = subprocess.run(DL + ["inbox", "--last", "1"], env=env, capture_output=True, text=True, encoding="utf-8")
step("inbox prints the last message", "Ответ от бота" in r.stdout, r.stdout)
r = subprocess.run(DL + ["serve", "--bind", "0.0.0.0:%d" % free_port()], env=env, capture_output=True, text=True, encoding="utf-8", timeout=5)
step("serve refuses a non-loopback bind", r.returncode != 0, r.stderr)

srv.kill()
print("RESULT:", "ALL PASS" if fails == 0 else f"{fails} FAILED")
sys.exit(1 if fails else 0)
