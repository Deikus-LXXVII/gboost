#!/usr/bin/env python3
"""Update and rollback rehearsal (spike S11, plan 6.7 item 15). No Tailscale; loopback only.

  python3 -I test_update.py <gboost-s9-core binary> <gboost-release binary>

Builds real packages, signs them with a throwaway key, and runs: install, session check, update (session
kept), refused tampered and older releases, schema change, broken release (automatic rollback), manual
rollback across the schema change, update again, stop.
"""
import json, os, shutil, socket, subprocess, sys, tempfile, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))
CORE, TOOL = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
PY = [sys.executable, "-I"]
NOPROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))
fails = 0
tmp = tempfile.mkdtemp(prefix="s11_")
ROOT, STATE, REL = os.path.join(tmp, "app"), os.path.join(tmp, "state"), os.path.join(tmp, "releases")
TARGET = "windows-test" if os.name == "nt" else "test-target"


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else "  (%s)" % str(detail)[-600:]), flush=True)


def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p


def run(*args):
    r = subprocess.run(list(args), capture_output=True, text=True, encoding="utf-8", timeout=180)
    return r.returncode, (r.stdout + r.stderr).strip()


def upd(*args):
    return run(*PY, os.path.join(HERE, "gboost_update.py"), "--root", ROOT, *args)


run(TOOL, "keygen", os.path.join(tmp, "key"), "--unencrypted")
PUB, KEY = os.path.join(tmp, "key", "release.pub"), os.path.join(tmp, "key", "release.key")
BROKEN = TOOL  # as a "core" it prints usage and exits: a release that cannot start


def package(version, schema, core=CORE):
    out = os.path.join(tmp, "pkg", version)
    run(*PY, os.path.join(HERE, "package.py"), "--version", version, "--schema", str(schema), "--target", TARGET,
        "--core", core, "--release-tool", TOOL, "--pubkey", PUB, "--out", out)
    return os.path.join(out, "gboost-%s-%s.tar.gz" % (version, TARGET))


def publish(version, schema, core=CORE, tamper=False):
    shutil.rmtree(REL, ignore_errors=True)
    os.makedirs(REL)
    shutil.copy(package(version, schema, core), REL)
    run(TOOL, "manifest", REL, version, str(schema), "0.1.0")
    run(TOOL, "sign", os.path.join(REL, "manifest.json"), KEY)
    if tamper:
        m = json.load(open(os.path.join(REL, "manifest.json")))
        m["min_client"] = "0.0.1"
        json.dump(m, open(os.path.join(REL, "manifest.json"), "w"))


agent, owner = free_port(), free_port()
os.makedirs(ROOT); os.makedirs(STATE)
json.dump({"source": {"dir": REL}, "target": TARGET,
           "node": {"name": "t", "state": STATE, "agent_port": agent, "owner_port": owner, "no_tailscale": True}},
          open(os.path.join(ROOT, "update.json"), "w"))

# Fake routine webhook: delivers the session code back through the node's MCP endpoint.
box = {"hits": 0}


class Hook(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        box["hits"] += 1
        self.send_response(200); self.send_header("Content-Length", "0"); self.end_headers()
        threading.Thread(target=lambda: mcp("tools/call", {"name": "gboost_deliver_code", "arguments": {"code": body["code"]}}), daemon=True).start()


hook = HTTPServer(("127.0.0.1", 0), Hook)
threading.Thread(target=hook.serve_forever, daemon=True).start()
open(os.path.join(STATE, "routine.toml"), "w").write('url = "http://127.0.0.1:%d/hook"\nkey = "k"\n' % hook.server_port)


def health():
    try:
        with NOPROXY.open("http://127.0.0.1:%d/healthz" % owner, timeout=3) as r:
            return json.load(r)
    except OSError:
        return None


def mcp(method, params=None):
    msg = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params or {}}
    req = urllib.request.Request("http://127.0.0.1:%d/mcp" % agent, data=json.dumps(msg).encode(), method="POST",
                                 headers={"content-type": "application/json", "x-gboost-client": box["key"]})
    with NOPROXY.open(req, timeout=10) as r:
        return json.load(r)


def tool_text(name):
    return mcp("tools/call", {"name": name, "arguments": {}})["result"]["content"][0]["text"]


try:
    # 1. First install and start.
    publish("0.1.0", 1)
    pkg = os.path.join(REL, "gboost-0.1.0-%s.tar.gz" % TARGET)
    other = os.path.join(tmp, "other.json")
    shutil.copy(os.path.join(REL, "manifest.json"), other)
    code, out = upd("install", package("0.1.1", 1), "--manifest", os.path.join(REL, "manifest.json"), "--tool", TOOL, "--pubkey", PUB)
    step("first install of a package the manifest does not list is refused", code != 0 and "not the one" in out, out)
    code, out = upd("install", pkg, "--manifest", os.path.join(REL, "manifest.json"), "--tool", TOOL, "--pubkey", PUB)
    step("install 0.1.0 (signed)", code == 0 and "INSTALLED 0.1.0" in out, out)
    code, out = upd("start")
    step("start: healthy 0.1.0", code == 0 and (health() or {}).get("version") == "0.1.0", out)

    # 2. A client and a session opened through the (fake) routine.
    tok = open(os.path.join(STATE, "console.token")).read().strip()
    req = urllib.request.Request("http://127.0.0.1:%d/local/clients/new" % owner, method="POST", headers={"x-console-token": tok})
    box["key"] = json.load(NOPROXY.open(req))["header_value"]
    tool_text("gboost_hello")
    for _ in range(40):
        if json.loads(tool_text("gboost_status")).get("session_active"):
            break
        time.sleep(0.25)
    who = json.loads(tool_text("gboost_whoami"))
    step("session opened through the routine", who.get("session", "").startswith("gbs_") and box["hits"] == 1, who)

    # 3. Update to 0.2.0: the session survives the restart.
    publish("0.2.0", 1)
    t0 = time.time()
    code, out = upd("apply")
    dt = time.time() - t0
    step("update to 0.2.0", code == 0 and "OK update 0.1.0 -> 0.2.0" in out and (health() or {}).get("version") == "0.2.0", out)
    step("update took under 15 s", dt < 15, "%.1fs" % dt)
    who2 = json.loads(tool_text("gboost_whoami"))
    step("session kept across the update, no new check", who2.get("session") == who["session"] and box["hits"] == 1, (who2, box))

    # 4. Refusals: same version, tampered manifest, an older signed release.
    code, out = upd("apply")
    step("nothing newer: up to date", code == 0 and "UP TO DATE 0.2.0" in out, out)
    publish("0.9.0", 1, tamper=True)
    code, out = upd("apply")
    step("tampered manifest refused", code != 0 and "signature" in out and (health() or {}).get("version") == "0.2.0", out)
    publish("0.1.5", 1)
    code, out = upd("apply")
    step("older signed release not installed", "UP TO DATE 0.2.0" in out, out)

    # 5. Schema change 1 -> 2 takes a snapshot.
    publish("0.3.0", 2)
    code, out = upd("apply")
    snaps = os.listdir(os.path.join(ROOT, "snapshots")) if os.path.isdir(os.path.join(ROOT, "snapshots")) else []
    step("update to 0.3.0 (schema 2) with a snapshot", code == 0 and any("from-0.2.0-to-0.3.0" in s for s in snaps), (out, snaps))

    # 6. A broken release rolls back by itself.
    publish("0.4.0", 2, core=BROKEN)
    code, out = upd("apply")
    step("broken 0.4.0 rolls back to 0.3.0", code != 0 and "ROLLED BACK to 0.3.0" in out and (health() or {}).get("version") == "0.3.0", out)
    code, out = upd("apply")
    step("a release that failed here is not retried", "UP TO DATE 0.3.0" in out, out)

    # 7. Manual rollback across the schema change restores the snapshot.
    code, out = upd("rollback")
    step("cross-schema rollback shows the plan and waits for --confirm", code != 0 and "PLAN" in out and "CONFIRM" in out
         and (health() or {}).get("version") == "0.3.0", out)
    cid = box["key"].split(":")[0]
    tok = open(os.path.join(STATE, "console.token")).read().strip()
    NOPROXY.open(urllib.request.Request("http://127.0.0.1:%d/local/clients/%s/revoke" % (owner, cid), method="POST",
                                        headers={"x-console-token": tok}))
    routine_before = open(os.path.join(STATE, "routine.toml")).read()
    code, out = upd("rollback", "--confirm")
    step("rollback 0.3.0 -> 0.2.0 with snapshot restore", code == 0 and "RESTORED state snapshot" in out
         and (health() or {}).get("version") == "0.2.0", out)
    states = {c["id"]: c["state"] for c in json.load(open(os.path.join(STATE, "clients.json")))}
    step("a client revoked after the snapshot stays revoked", states.get(cid) == "revoked", states)
    step("live secrets are not taken from the snapshot", open(os.path.join(STATE, "routine.toml")).read() == routine_before)
    code, out = upd("rollback", "--to", "0.3.0")
    step("rollback to a newer or rejected version is refused", code != 0 and "not an earlier version" in out, out)
    code, out = upd("rollback", "--to", "0.0.1")
    step("rollback to a version not on disk is refused", code != 0 and "never downloads" in out, out)

    # 8. The rolled-back release is not put straight back; a fixed release installs.
    publish("0.3.0", 2)
    code, out = upd("apply")
    step("rolled-back 0.3.0 is not re-applied", "UP TO DATE 0.2.0" in out, out)
    publish("0.3.1", 2)
    code, out = upd("apply")
    step("update again to a fixed 0.3.1", code == 0 and (health() or {}).get("version") == "0.3.1", out)

    # 9. A crash in the middle of a switch is undone by the next command.
    # (the journal survived a switch that had passed its check)
    json.dump({"old": "0.2.0", "new": "0.3.1", "reason": "update"}, open(os.path.join(ROOT, "pending.json"), "w"))
    code, out = upd("stop")
    code, out = upd("start")
    step("a switch that had finished is kept on recovery", "KEPT 0.3.1" in out and (health() or {}).get("version") == "0.3.1", out)
    if os.name != "nt":
        modes = {d: oct(os.stat(os.path.join(ROOT, d)).st_mode & 0o777) for d in ("", "versions", "snapshots", "downloads")}
        step("updater folders are owner-only", all(m == "0o700" for m in modes.values()), modes)
    kept = json.loads(upd("status")[1])["installed"]
    step("at most 3 versions kept", len(kept) <= 3, kept)

    # 10. Stop.
    code, out = upd("stop")
    time.sleep(0.5)
    step("stop", "STOPPED" in out and health() is None, out)
finally:
    upd("stop")
    hook.shutdown()
    shutil.rmtree(tmp, ignore_errors=True)

print("RESULT:", "ALL PASS" if fails == 0 else "%d FAILED" % fails)
sys.exit(1 if fails else 0)
