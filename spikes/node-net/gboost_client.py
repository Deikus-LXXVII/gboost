#!/usr/bin/env python3
"""gboost-client (spike S10): the one `gboost` connector on the Bot's computer.

Grok runs it as a Command (stdio) connector. It forwards each MCP message to the first GBOOST node that
answers, over GBOOST's own userspace Tailscale device, with the client key header. If a node fails (no
connection, 5xx, 401/403, or a lost session) it moves to the next node and replays `initialize`.
Python 3.9+ stdlib only. Only JSON-RPC goes to stdout; logs go to stderr and never contain the key.

Config folder: $GBOOST_CLIENT_HOME, else ~/.gboost-client. Files:
  config.json  {"nodes": ["https://gboost-node-a.<tailnet>.ts.net/mcp", ...],
                "tailscale": {"enabled": true, "bin_dir": null, "hostname": "gboost-client"}}
  client.key   the header value "<client id>:<secret>" (owner-only)
  ts/          the Tailscale device state (owner-only)

Commands: `gboost_client.py` (serve stdio), `gboost_client.py login`, `gboost_client.py status`.
"""
import atexit, http.client, json, os, signal, sys, time, urllib.error, urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gboost_net  # noqa: E402

TIMEOUT_S = float(os.environ.get("GBOOST_TIMEOUT_S", "20"))
RETRY_PRIMARY_S = float(os.environ.get("GBOOST_RETRY_PRIMARY_S", "60"))
FAIL_STATUSES = {401, 403, 502, 503, 504}


def home():
    return os.environ.get("GBOOST_CLIENT_HOME") or os.path.join(os.path.expanduser("~"), ".gboost-client")


def log(msg):
    sys.stderr.write("gboost-client: %s\n" % msg)
    sys.stderr.flush()


def load():
    with open(os.path.join(home(), "config.json"), encoding="utf-8") as f:
        cfg = json.load(f)
    with open(os.path.join(home(), "client.key"), encoding="utf-8") as f:
        key = f.read().strip()
    if not cfg.get("nodes"):
        raise SystemExit("config.json has no nodes")
    ts = cfg.get("tailscale") or {}
    if ts.get("enabled") and any(not u.startswith("https://") for u in cfg["nodes"]):
        raise SystemExit("with Tailscale on, every node URL must be https://")
    return cfg, key


def device(cfg):
    ts = cfg.get("tailscale") or {}
    bin_dir = os.path.expanduser(ts["bin_dir"]) if ts.get("bin_dir") else None
    return gboost_net.Device(os.path.join(home(), "ts"), ts.get("hostname") or "gboost-client", bin_dir=bin_dir)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None


class NodeFailed(Exception):
    pass


def parse_body(raw, ctype):
    """JSON, or the last JSON-RPC message of an SSE stream."""
    if not raw:
        return None
    if "text/event-stream" in (ctype or ""):
        msgs = [l[5:].strip() for l in raw.decode("utf-8", "replace").splitlines() if l.startswith("data:")]
        msgs = [m for m in msgs if m]
        if not msgs:
            raise ValueError("empty event stream")
        return json.loads(msgs[-1])
    return json.loads(raw)


class Client:
    def __init__(self, cfg, key, dev=None):
        self.nodes = list(cfg["nodes"])
        self.key = key
        self.active = None
        self.session = {}  # node url -> mcp-session-id
        self.init_msg = None  # the host's initialize request, replayed on a new node
        self.down_since = {}
        self.dev = dev
        handlers = [NoRedirect()]
        if dev:
            handlers.append(urllib.request.ProxyHandler({"https": dev.proxy_url(), "http": dev.proxy_url()}))
        else:
            handlers.append(urllib.request.ProxyHandler({}))
        self.opener = urllib.request.build_opener(*handlers)

    def login_hint(self):
        if not self.dev:
            return ""
        try:
            state, url, _ = self.dev.state()
        except Exception:
            return " GBOOST's tailnet device is not running."
        if state != "Running" and url:
            return " GBOOST's tailnet device needs a login: ask the owner to open " + url
        return ""

    def post(self, url, msg):
        """One POST. Returns (status, body). Raises NodeFailed when the node should be skipped."""
        h = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream",
             "x-gboost-client": self.key}
        if self.session.get(url):
            h["mcp-session-id"] = self.session[url]
        req = urllib.request.Request(url, data=json.dumps(msg).encode(), headers=h, method="POST")
        try:
            with self.opener.open(req, timeout=TIMEOUT_S) as r:
                sid = r.headers.get("mcp-session-id")
                if sid:
                    self.session[url] = sid
                return r.status, parse_body(r.read(), r.headers.get("Content-Type"))
        except urllib.error.HTTPError as e:
            if e.code in FAIL_STATUSES:
                raise NodeFailed("HTTP %d" % e.code)
            if e.code in (400, 404) and self.session.get(url):
                raise NodeFailed("session lost (HTTP %d)" % e.code)
            try:
                return e.code, parse_body(e.read(), e.headers.get("Content-Type"))
            except ValueError:
                return e.code, None
        except (OSError, urllib.error.URLError, http.client.HTTPException, ValueError) as e:
            raise NodeFailed(type(e).__name__)

    def attach(self, i):
        """Make node i active: a fresh MCP session there, with the host's initialize replayed."""
        url = self.nodes[i]
        self.session.pop(url, None)
        if self.init_msg is not None:
            st, _ = self.post(url, self.init_msg)
            if st != 200:
                raise NodeFailed("initialize answered %d" % st)
            self.post(url, {"jsonrpc": "2.0", "method": "notifications/initialized"})
        if self.active is not None and i != self.active:
            log("switched to node %d" % i)
        self.active = i

    def order(self):
        """List order (the primary first); nodes that failed recently go last."""
        now = time.time()
        fresh = [i for i in range(len(self.nodes)) if now - self.down_since.get(i, 0) >= RETRY_PRIMARY_S]
        return fresh + [i for i in range(len(self.nodes)) if i not in fresh]

    def send(self, msg):
        """Forward one message; returns the JSON-RPC response, or None for a notification."""
        is_init = msg.get("method") == "initialize"
        if is_init:
            self.init_msg = msg
            self.session.clear()
            self.active = None
        last = "no nodes"
        for i in self.order():
            url = self.nodes[i]
            try:
                if not is_init and i != self.active:
                    self.attach(i)
                st, body = self.post(url, msg)
                if st == 200 and is_init:
                    self.active = i
                self.down_since.pop(i, None)
                if "id" not in msg:
                    return None
                return body if body is not None else error(msg, -32000, "GBOOST node answered %d with no body" % st)
            except NodeFailed as e:
                last = str(e)
                self.down_since[i] = time.time()
                self.session.pop(url, None)
                if self.active == i:
                    self.active = None
                log("node %d failed: %s" % (i, last))
        if "id" not in msg:
            return None
        return error(msg, -32000, "No GBOOST node is reachable (%s).%s" % (last, self.login_hint()))


def error(msg, code, text):
    return {"jsonrpc": "2.0", "id": msg.get("id"), "error": {"code": code, "message": text}}


def utf8_stdio():
    for s, kw in ((sys.stdin, {}), (sys.stdout, {"newline": "\n"})):
        try:
            s.reconfigure(encoding="utf-8", **kw)
        except (AttributeError, ValueError):
            pass


def serve():
    utf8_stdio()
    cfg, key = load()
    dev = None
    if (cfg.get("tailscale") or {}).get("enabled"):
        dev = device(cfg)
        with gboost_net.Lock(os.path.join(home(), "ts.lock")):
            dev.start()
        atexit.register(dev.stop)
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
        state, info = dev.bring_up(wait_s=10)
        if state == "NeedsLogin":
            log("tailnet login needed; the link is shown in tool errors")
    c = Client(cfg, key, dev)
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        msg = None
        try:
            msg = json.loads(line)
            resp = c.send(msg) if isinstance(msg, dict) else error({}, -32600, "one message per line")
        except ValueError:
            resp = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}}
        except Exception as e:  # never die on one message
            log("internal error: %s" % type(e).__name__)
            resp = error(msg if isinstance(msg, dict) else {}, -32603, "gboost-client internal error")
        if resp is not None:
            sys.stdout.write(json.dumps(resp, ensure_ascii=False) + "\n")
            sys.stdout.flush()


def cmd_login():
    cfg, _ = load()
    if not (cfg.get("tailscale") or {}).get("enabled"):
        print("tailscale is off in config.json")
        return
    dev = device(cfg)
    with gboost_net.Lock(os.path.join(home(), "ts.lock")):
        dev.start()
    atexit.register(dev.stop)
    state, info = dev.bring_up(wait_s=20)
    if state == "Running":
        print("RUNNING " + (info or ""), flush=True)
        return
    if state != "NeedsLogin":
        print("ERROR tailnet device state %s" % state, flush=True)
        sys.exit(1)
    print("LOGIN " + info, flush=True)
    end = time.time() + 600
    while time.time() < end:
        s, _, dns = dev.state()
        if s == "Running":
            print("RUNNING " + (dns or ""), flush=True)
            return
        time.sleep(2)
    print("TIMEOUT the login was not completed in 10 minutes", flush=True)
    sys.exit(1)


def main(argv):
    if len(argv) > 1 and argv[1] == "login":
        return cmd_login()
    if len(argv) > 1 and argv[1] == "status":
        cfg, _ = load()
        print(json.dumps({"nodes": cfg["nodes"], "tailscale": cfg.get("tailscale")}, indent=2))
        return
    serve()


if __name__ == "__main__":
    main(sys.argv)
