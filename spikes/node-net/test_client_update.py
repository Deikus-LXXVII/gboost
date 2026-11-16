#!/usr/bin/env python3
"""Client self-update rehearsal (plan 6.7 item 7). No Tailscale; loopback only.

  python3 -I test_client_update.py <gboost-s9-core binary> <gboost-release binary>

A real node and a client, both installed from signed packages by gboost_update.py (role node and role
client). The connector's command is the stable entry point <client root>/gboost-client.py; the test sends
MCP through it before and after: an update, a broken client release (automatic rollback), a tampered
manifest, and a manual rollback.
"""
import json, os, shutil, socket, subprocess, sys, tempfile, threading, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CORE, TOOL = os.path.abspath(sys.argv[1]), os.path.abspath(sys.argv[2])
PY = [sys.executable, "-I"]
NOPROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))
TARGET = "windows-test" if os.name == "nt" else "test-target"
fails = 0
tmp = tempfile.mkdtemp(prefix="s11c_")
NODE, CLIENT, CHOME, STATE, REL = (os.path.join(tmp, d) for d in ("node", "client", "chome", "state", "rel"))


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else "  (%s)" % str(detail)[-600:]), flush=True)


def free_port():
    s = socket.socket(); s.bind(("127.0.0.1", 0)); p = s.getsockname()[1]; s.close(); return p


def run(*args, stdin=None, client_env=True):
    env = dict(os.environ, GBOOST_CLIENT_HOME=CHOME) if client_env else \
        {k: v for k, v in os.environ.items() if k != "GBOOST_CLIENT_HOME"}
    r = subprocess.run(list(args), capture_output=True, text=True, encoding="utf-8", timeout=180, input=stdin, env=env)
    return r.returncode, (r.stdout + r.stderr).strip()


def node_upd(*args):
    return run(*PY, os.path.join(HERE, "gboost_update.py"), "--root", NODE, *args)


def client(*args, stdin=None):  # exactly what the connector runs
    return run(*PY, os.path.join(CLIENT, "gboost-client.py"), *args, stdin=stdin)


def client_upd(*args):  # the installed updater, through its stable entry point; it finds the client folder
    return run(*PY, os.path.join(CLIENT, "gboost-update.py"), *args, client_env=False)  # in update.json, not env


run(TOOL, "keygen", os.path.join(tmp, "key"), "--unencrypted")
PUB, KEY = os.path.join(tmp, "key", "release.pub"), os.path.join(tmp, "key", "release.key")

# A client whose self-test fails: the update must roll back.
BROKEN_PY = os.path.join(tmp, "broken-py")
shutil.copytree(HERE, BROKEN_PY, ignore=shutil.ignore_patterns("__pycache__", "test_*"))
src = open(os.path.join(BROKEN_PY, "gboost_client.py"), encoding="utf-8").read()
assert 'print("SELFTEST OK " + version(), flush=True)' in src
open(os.path.join(BROKEN_PY, "gboost_client.py"), "w", encoding="utf-8").write(
    src.replace('print("SELFTEST OK " + version(), flush=True)', 'raise RuntimeError("broken on purpose")'))


def variant(name, replacement):
    """A copy of node-net whose client self-test success line is replaced."""
    d = os.path.join(tmp, name)
    shutil.copytree(HERE, d, ignore=shutil.ignore_patterns("__pycache__", "test_*"))
    path = os.path.join(d, "gboost_client.py")
    src = open(path, encoding="utf-8").read()
    open(path, "w", encoding="utf-8").write(src.replace('print("SELFTEST OK " + version(), flush=True)', replacement))
    return d


SLOW_BROKEN = variant("slow-py", 'time.sleep(3); raise RuntimeError("slow and broken")')
NEEDS_MARKER = variant("marker-py", 'print("SELFTEST FAILED needs the marker file" if not os.path.exists(os.path.join(home(), "marker")) '
                       'else "SELFTEST OK " + version(), flush=True); sys.exit(0 if os.path.exists(os.path.join(home(), "marker")) else 1)')


def publish(version, py_dir=HERE, tamper=False):
    shutil.rmtree(REL, ignore_errors=True)
    os.makedirs(REL)
    run(*PY, os.path.join(HERE, "package.py"), "--version", version, "--schema", "1", "--target", TARGET,
        "--core", CORE, "--release-tool", TOOL, "--pubkey", PUB, "--out", REL, "--py-dir", py_dir)
    run(TOOL, "manifest", REL, version, "1", "0.1.0")
    run(TOOL, "sign", os.path.join(REL, "manifest.json"), KEY)
    if tamper:
        m = json.load(open(os.path.join(REL, "manifest.json")))
        m["min_client"] = "0.0.1"
        json.dump(m, open(os.path.join(REL, "manifest.json"), "w"))
    return os.path.join(REL, "gboost-%s-%s.tar.gz" % (version, TARGET)), os.path.join(REL, "manifest.json")


def mcp_through_client():
    msgs = [{"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}]
    code, out = client(stdin="".join(json.dumps(m) + "\n" for m in msgs))
    replies = [json.loads(x) for x in out.splitlines() if x.startswith("{")]
    names = [t["name"] for r in replies if r.get("id") == 2 for t in r.get("result", {}).get("tools", [])]
    return "gboost_hello" in names, out


agent, owner = free_port(), free_port()
for d in (NODE, CLIENT, CHOME, STATE):
    os.makedirs(d)
json.dump({"source": {"dir": REL}, "target": TARGET,
           "node": {"name": "t", "state": STATE, "agent_port": agent, "owner_port": owner, "no_tailscale": True}},
          open(os.path.join(NODE, "update.json"), "w"))
json.dump({"source": {"dir": REL}, "target": TARGET, "role": "client", "client": {"home": CHOME}},
          open(os.path.join(CLIENT, "update.json"), "w"))
json.dump({"nodes": ["http://127.0.0.1:%d/mcp" % agent], "tailscale": {"enabled": False}},
          open(os.path.join(CHOME, "config.json"), "w"))

try:
    # 1. Node and client from the signed 0.1.0.
    pkg, man = publish("0.1.0")
    code, out = node_upd("install", pkg, "--manifest", man, "--tool", TOOL, "--pubkey", PUB)
    code, out = node_upd("start")
    step("node 0.1.0 running", code == 0 and "RUNNING 0.1.0" in out, out)
    tok = open(os.path.join(STATE, "console.token")).read().strip()
    req = urllib.request.Request("http://127.0.0.1:%d/local/clients/new" % owner, method="POST", headers={"x-console-token": tok})
    with open(os.path.join(CHOME, "client.key"), "w") as f:
        f.write(json.load(NOPROXY.open(req))["header_value"])
    code, out = run(*PY, os.path.join(HERE, "gboost_update.py"), "--root", CLIENT, "install", pkg, "--manifest", man,
                    "--tool", TOOL, "--pubkey", PUB)
    step("client 0.1.0 installed with its entry points", code == 0 and "INSTALLED 0.1.0" in out
         and os.path.exists(os.path.join(CLIENT, "gboost-client.py")) and os.path.exists(os.path.join(CLIENT, "gboost-update.py")), out)
    code, out = client("selftest")
    step("entry point runs client 0.1.0 (self-test)", code == 0 and out == "SELFTEST OK 0.1.0", out)
    ok, out = mcp_through_client()
    step("MCP through the entry point reaches the node", ok, out)
    code, out = client_upd("start")
    step("client role: nothing to start", code == 0 and "CLIENT nothing to start" in out, out)

    # 2. Update the client to 0.2.0 through the installed updater.
    publish("0.2.0")
    code, out = client_upd("apply")
    step("client update 0.1.0 -> 0.2.0", code == 0 and "OK update 0.1.0 -> 0.2.0" in out, out)
    code, out = client("selftest")
    step("entry point now runs 0.2.0", out == "SELFTEST OK 0.2.0", out)
    ok, out = mcp_through_client()
    step("MCP still works after the update", ok, out)

    # 3. Refusals and a broken release.
    publish("0.9.0", tamper=True)
    code, out = client_upd("apply")
    step("tampered manifest refused", code != 0 and "signature" in out and client("selftest")[1] == "SELFTEST OK 0.2.0", out)
    publish("0.3.0", py_dir=BROKEN_PY)
    code, out = client_upd("apply")
    step("broken client 0.3.0 rolls back to 0.2.0", code != 0 and "ROLLED BACK to 0.2.0" in out
         and client("selftest")[1] == "SELFTEST OK 0.2.0", out)
    code, out = client_upd("apply")
    step("the failed release is not retried", "UP TO DATE 0.2.0" in out, out)
    ok, out = mcp_through_client()
    step("MCP works after the automatic rollback", ok, out)

    # 4. A local fault (a broken key) is not blamed on the release: the update is refused, nothing marked.
    publish("0.4.0")
    good = open(os.path.join(CHOME, "client.key")).read()
    open(os.path.join(CHOME, "client.key"), "w").write("no-colon")
    code, out = client_upd("apply")
    step("a client that fails its own check is not updated", code != 0 and "fix that first" in out
         and "FAILED update" not in out, out)
    open(os.path.join(CHOME, "client.key"), "w").write(good)
    code, out = client_upd("apply")
    step("after the fix the same release installs", code == 0 and client("selftest")[1] == "SELFTEST OK 0.4.0", out)

    # 5. Manual rollback.
    code, out = client_upd("rollback")
    step("manual rollback to 0.2.0 (no schema step for a client)", code == 0 and "OK rollback 0.4.0 -> 0.2.0" in out
         and client("selftest")[1] == "SELFTEST OK 0.2.0", out)
    ok, out = mcp_through_client()
    step("MCP works after the manual rollback", ok, out)
    # 6. While the new client's self-test runs, `current` never points at it (no connector can start it).
    publish("0.5.0", py_dir=SLOW_BROKEN)
    seen, res = set(), {}
    th = threading.Thread(target=lambda: res.update(r=client_upd("apply")))
    th.start()
    while th.is_alive():
        try:
            seen.add(open(os.path.join(CLIENT, "current")).read().strip())
        except OSError:
            pass
        time.sleep(0.05)
    step("a client under test is never current", seen == {"0.2.0"} and "ROLLED BACK to 0.2.0" in res["r"][1], (seen, res))

    # 7. A local fault reported by the new self-test does not block the release; after the fix it installs.
    publish("0.6.0", py_dir=NEEDS_MARKER)
    code, out = client_upd("apply")
    step("local fault reported, release not blocked", code != 0 and "LOCAL FAULT" in out, out)
    code, out = client_upd("apply")
    step("the same release is tried again", "UP TO DATE" not in out and "LOCAL FAULT" in out, out)
    open(os.path.join(CHOME, "marker"), "w").close()
    code, out = client_upd("apply")
    step("after the fix it installs", code == 0 and client("selftest")[1] == "SELFTEST OK 0.6.0", out)

    # 8. `start` finishes an interrupted switch (client role).
    json.dump({"old": "0.2.0", "new": "0.6.0", "reason": "update"}, open(os.path.join(CLIENT, "pending.json"), "w"))
    code, out = client_upd("start")
    step("start recovers an interrupted switch", "KEPT 0.6.0" in out and not os.path.exists(os.path.join(CLIENT, "pending.json")), out)

    # 9. A schema change needs no snapshot for a client.
    shutil.rmtree(REL, ignore_errors=True); os.makedirs(REL)
    run(*PY, os.path.join(HERE, "package.py"), "--version", "0.7.0", "--schema", "2", "--target", TARGET,
        "--core", CORE, "--release-tool", TOOL, "--pubkey", PUB, "--out", REL)
    run(TOOL, "manifest", REL, "0.7.0", "2", "0.1.0"); run(TOOL, "sign", os.path.join(REL, "manifest.json"), KEY)
    code, out = client_upd("apply")
    snaps = os.listdir(os.path.join(CLIENT, "snapshots"))
    step("schema change on a client: no snapshot", code == 0 and snaps == [] and client("selftest")[1] == "SELFTEST OK 0.7.0", (out, snaps))
    code, out = client_upd("rollback")
    step("rollback across the schema needs no --confirm on a client", code == 0 and "OK rollback 0.7.0 -> 0.6.0" in out, out)

    if os.name != "nt":
        modes = {n: oct(os.stat(os.path.join(CLIENT, n)).st_mode & 0o777) for n in ("", "gboost-client.py", "gboost-update.py")}
        step("client folder and entry points are owner-only", set(modes.values()) == {"0o700"}, modes)
finally:
    node_upd("stop")
    shutil.rmtree(tmp, ignore_errors=True)

print("RESULT:", "ALL PASS" if fails == 0 else "%d FAILED" % fails)
sys.exit(1 if fails else 0)
