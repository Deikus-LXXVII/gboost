#!/usr/bin/env python3
"""Device.restart on a real, never logged-in tailscaled (skipped when tailscaled is not installed), and the
updater's login_line() reading only what this start printed.

  python3 -I test_device_restart.py
"""
import os, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gboost_net, gboost_update  # noqa: E402

fails = 0


def step(name, ok, detail=""):
    global fails
    fails += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + ("" if ok else "  (%s)" % detail), flush=True)


# login_line: an old LOGIN line, or one written before this start, is never reported.
root = tempfile.mkdtemp(prefix="login_")
u = gboost_update.Updater(root)
open(os.path.join(root, "launcher.out"), "w").write("LOGIN https://login.tailscale.com/a/old\n")
step("no start record: no login line", u.login_line() is None)
gboost_update.write_file_atomic(os.path.join(root, "launcher.start"), str(os.path.getsize(os.path.join(root, "launcher.out"))))
step("an old line before this start is ignored", u.login_line() is None)
open(os.path.join(root, "launcher.out"), "a").write("LOGIN https://login.tailscale.com/a/new\n")
step("a line printed after this start is reported", u.login_line() == "LOGIN https://login.tailscale.com/a/new", u.login_line())
open(os.path.join(root, "launcher.out"), "a").write("URL https://n.example/mcp\n")
step("after URL the login is over", u.login_line() is None)
shutil.rmtree(root)

if os.name == "nt" or not shutil.which("tailscaled") or not shutil.which("tailscale"):
    print("SKIP  real tailscaled tests (tailscaled not on PATH or Windows)")
else:
    base = tempfile.mkdtemp(prefix="dev restart ")  # a space in the path on purpose
    state, decoy_state = os.path.join(base, "state dir"), os.path.join(base, "state dir-other")
    try:
        subprocess.run([sys.executable, "-I", "-c", "import sys;sys.path.insert(0,%r);import gboost_net;"
                        "gboost_net.Device(%r,'gboost-rt').start()" % (HERE, state)], check=True)
        d = gboost_net.Device(state, "gboost-rt"); d.start()
        pid1 = int(open(os.path.join(state, "tailscaled.pid")).read())
        step("our daemon is recognised with spaces in the path", d._our_daemon(pid1))
        dd = gboost_net.Device(decoy_state, "gboost-decoy"); dd.start()
        pid_decoy = int(open(os.path.join(decoy_state, "tailscaled.pid")).read())
        step("a daemon whose state dir only starts with ours is not ours", not d._our_daemon(pid_decoy))
        d.restart()
        pid2 = int(open(os.path.join(state, "tailscaled.pid")).read())
        step("restart replaces ours and leaves the decoy", pid2 != pid1 and not (d._alive(pid1) and d._our_daemon(pid1))
             and d._alive(pid_decoy))
        os.kill(pid2, 9); time.sleep(0.5); d.proc = None
        d.restart()
        pid3 = int(open(os.path.join(state, "tailscaled.pid")).read())
        step("a dead recorded process just gets a fresh start", pid3 not in (pid1, pid2) and d.health() == "needs_login", d.health())
        dd.stop()
    finally:
        subprocess.run(["pkill", "-f", "statedir=" + base])
        shutil.rmtree(base, ignore_errors=True)

print("RESULT:", "ALL PASS" if fails == 0 else "%d FAILED" % fails)
sys.exit(1 if fails else 0)
