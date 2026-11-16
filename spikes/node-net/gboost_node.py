#!/usr/bin/env python3
"""GBOOST node launcher (spike S10): GBOOST's own Tailscale device plus the S9 core.

  gboost_node.py --name <node name> --core <path to gboost-s9-core> --state <state folder> \
                 [--agent-port 8790] [--owner-port 8791] [--ts-bin-dir <folder>]

Starts a userspace `tailscaled` as the tailnet device `gboost-node-<name>` (separate from any Tailscale the
computer runs), prints `LOGIN <url>` if the owner must log it in and waits for that, publishes the core's
agent listener at `https://gboost-node-<name>.<tailnet>.ts.net/mcp` (tailnet only), then runs the core.
Python 3.9+ stdlib only.
"""
import argparse, os, signal, socket, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gboost_net  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--name", required=True)
    p.add_argument("--core", required=True)
    p.add_argument("--state", required=True)
    p.add_argument("--agent-port", type=int, default=8790)
    p.add_argument("--owner-port", type=int, default=8791)
    p.add_argument("--ts-bin-dir")
    p.add_argument("--login-wait-s", type=int, default=900)
    p.add_argument("--no-tailscale", action="store_true", help="tests and local use: no tailnet device, loopback only")
    p.add_argument("--stop-file", help="exit cleanly when this file appears (used by gboost_update.py on every OS)")
    a = p.parse_args()

    os.makedirs(a.state, exist_ok=True)
    ts_bin = os.path.expanduser(a.ts_bin_dir) if a.ts_bin_dir else None
    dev = None if a.no_tailscale else gboost_net.Device(os.path.join(a.state, "ts"), "gboost-node-" + a.name, bin_dir=ts_bin)
    core = None
    here = os.path.dirname(os.path.abspath(__file__))
    vfile = os.path.join(os.path.dirname(here), "VERSION")
    version = open(vfile).read().strip() if os.path.exists(vfile) else "dev"

    def stop(*_):
        sys.exit(0)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        if dev is None:
            state, info = "Running", "127.0.0.1:%d" % a.agent_port
        else:
            with gboost_net.Lock(os.path.join(a.state, "ts.lock")):
                dev.start()
            state, info = dev.bring_up(wait_s=30)
        if dev is not None and state == "NeedsLogin":
            print("LOGIN " + info, flush=True)
            end = time.time() + a.login_wait_s
            while time.time() < end:
                state, _, dns = dev.state()  # noqa: F841
                if state == "Running":
                    info = dns
                    break
                time.sleep(2)
        if state != "Running" or not info:
            sys.exit("error: the tailnet device is %s (login not completed?)" % state)
        print("TAILNET " + info, flush=True)

        env = dict(os.environ, GBOOST_VERSION=version, S9_STATE_DIR=a.state, S9_AGENT_BIND="127.0.0.1:%d" % a.agent_port,
                   S9_OWNER_BIND="127.0.0.1:%d" % a.owner_port)
        core = subprocess.Popen([a.core], env=env)
        for _ in range(100):
            if core.poll() is not None:
                sys.exit("error: the core exited; see its output above")
            try:
                socket.create_connection(("127.0.0.1", a.agent_port), 0.2).close()
                break
            except OSError:
                time.sleep(0.1)
        else:
            sys.exit("error: the core did not open its agent port")
        if dev is not None:
            try:
                dev.serve_https(a.agent_port)
            except RuntimeError as e:
                if str(e).startswith("ENABLE "):
                    print(str(e), flush=True)
                    sys.exit("error: the owner must turn on HTTPS for the tailnet (link above), then start again")
                raise
            print("URL https://%s/mcp" % info, flush=True)
        else:
            print("URL http://%s/mcp" % info, flush=True)
        print("VERSION " + version, flush=True)
        while core.poll() is None:
            if a.stop_file and os.path.exists(a.stop_file):
                break
            time.sleep(0.5)
    finally:
        if core and core.poll() is None:
            core.terminate()
            try:
                core.wait(5)
            except subprocess.TimeoutExpired:
                core.kill()
        if dev is not None:
            dev.stop()


if __name__ == "__main__":
    main()
