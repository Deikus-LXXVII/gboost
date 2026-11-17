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
  client.log   which node each client process uses and why it switched (owner-only, no secrets)
  ts/          the Tailscale device state (owner-only)

Commands: `gboost_client.py` (serve stdio), `gboost_client.py login`, `gboost_client.py status`,
`gboost_client.py selftest` (offline: code, config, key and Tailscale binary; the updater's health check).
"""
import http.client, json, os, signal, sys, threading, time, urllib.error, urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gboost_net  # noqa: E402

TIMEOUT_S = float(os.environ.get("GBOOST_TIMEOUT_S", "20"))
RETRY_PRIMARY_S = float(os.environ.get("GBOOST_RETRY_PRIMARY_S", "60"))
FAIL_STATUSES = {401, 403, 502, 503, 504}
WARMUP_S = float(os.environ.get("GBOOST_WARMUP_S", "30"))
LIST_WAIT_S = float(os.environ.get("GBOOST_LIST_WAIT_S", "3"))  # then tools/list comes from the cache
HEAL_EVERY_S = float(os.environ.get("GBOOST_HEAL_EVERY_S", "60"))  # at most one tailscaled restart per minute
PROTOCOLS = ("2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05")


def home():
    return os.environ.get("GBOOST_CLIENT_HOME") or os.path.join(os.path.expanduser("~"), ".gboost-client")


LOG_MAX = 1 << 20


def log(msg):
    """stderr (the host may hide it) and <home>/client.log, owner-only, one older copy kept. Never the key."""
    line = "gboost-client: %s\n" % msg
    sys.stderr.write(line)
    sys.stderr.flush()
    path = os.path.join(home(), "client.log")
    try:
        if os.path.exists(path) and os.path.getsize(path) > LOG_MAX:
            os.replace(path, path + ".1")
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
        with os.fdopen(fd, "a", encoding="utf-8") as f:
            f.write("%s pid %d %s" % (time.strftime("%Y-%m-%dT%H:%M:%S%z"), os.getpid(), line))
    except OSError:
        pass


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


def ts_bin_dir(cfg):
    """tailscale.bin_dir: `~` expanded; a relative path is relative to the client folder (never the cwd)."""
    d = (cfg.get("tailscale") or {}).get("bin_dir")
    return os.path.join(home(), os.path.expanduser(d)) if d else None


def device(cfg):
    ts = cfg.get("tailscale") or {}
    return gboost_net.Device(os.path.join(home(), "ts"), ts.get("hostname") or "gboost-client", bin_dir=ts_bin_dir(cfg))


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None


class NodeFailed(Exception):
    def __init__(self, msg, conn=False):
        super().__init__(msg)
        self.conn = conn  # no connection at all (as opposed to an HTTP answer)


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
        self.started = time.time()
        self.ready = threading.Event()  # the tailnet device is up (or failed to come up)
        self.dev_error = None
        self.last_conn_fail = False
        self.opener, self.opener_port = None, None
        self.dev_lock = threading.Lock()  # the background bring-up and heal never run at once
        if not dev:
            self.ready.set()

    def get_opener(self):
        port = self.dev.proxy_port if self.dev else None
        if self.opener is None or port != self.opener_port:
            proxies = {"https": self.dev.proxy_url(), "http": self.dev.proxy_url()} if port else {}
            self.opener = urllib.request.build_opener(NoRedirect(), urllib.request.ProxyHandler(proxies))
            self.opener_port = port
        return self.opener

    def bring_up_device(self):
        """In a background thread: the host gets its initialize answer without waiting for the tailnet."""
        try:
            with self.dev_lock:
                with gboost_net.Lock(os.path.join(home(), "ts.lock")):
                    self.dev.start()
                state, _ = self.dev.bring_up(wait_s=10)
            if state == "NeedsLogin":
                log("tailnet login needed; the link is shown in tool errors")
        except Exception as e:  # noqa: BLE001 - reported in tool errors
            self.dev_error = "%s: %s" % (type(e).__name__, e)
            log("tailnet device did not start: " + self.dev_error)
        finally:
            self.ready.set()

    def heal(self):
        """Every node failed without a connection. If GBOOST's device is hung or offline, restart it (one
        process restarts it at most once a minute; the others pick up the new one)."""
        if not self.ready.is_set():
            return False  # the background bring-up still owns the device
        mark = os.path.join(home(), "ts.heal")
        with self.dev_lock, gboost_net.Lock(os.path.join(home(), "ts.lock")):
            before = self.dev.proxy_port
            health = self.dev.health()
            recent = os.path.exists(mark) and time.time() - os.path.getmtime(mark) < HEAL_EVERY_S
            if health in ("online", "needs_login") or recent:
                # Nothing to restart (or another process just did): adopt what runs now. If its port moved,
                # the caller retries at once.
                self.dev.proc = None
                self.dev.start()
                return self.dev.proxy_port != before
            log("no node reachable and the tailnet device is %s; restarting it" % health)
            open(mark, "w").close()
            try:
                self.dev.restart()
            except Exception:
                os.remove(mark)  # the restart did not happen: let the next call try again
                raise
            self.dev.bring_up(wait_s=10)
        return True

    def tools_cache(self):
        return os.path.join(home(), "tools.json")

    def cached_tools(self, msg):
        try:
            with open(self.tools_cache(), encoding="utf-8") as f:
                tools = json.load(f)
        except (OSError, ValueError):
            return None
        log("tools/list answered from the cache (no node yet)")
        return {"jsonrpc": "2.0", "id": msg.get("id"), "result": {"tools": tools}}

    def save_tools(self, resp):
        tools = (resp or {}).get("result", {}).get("tools") if isinstance(resp, dict) else None
        if not isinstance(tools, list):
            return
        tmp = self.tools_cache() + ".tmp"
        try:
            fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                json.dump(tools, f)
            os.replace(tmp, self.tools_cache())
        except OSError:
            pass

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
            with self.get_opener().open(req, timeout=TIMEOUT_S) as r:
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
        except (OSError, urllib.error.URLError) as e:
            raise NodeFailed(type(e).__name__, conn=True)
        except (http.client.HTTPException, ValueError) as e:
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
        if i != self.active:
            log("using node %d (%s)" % (i, url))
        self.active = i

    def order(self):
        """List order (the primary first); nodes that failed recently go last."""
        now = time.time()
        fresh = [i for i in range(len(self.nodes)) if now - self.down_since.get(i, 0) >= RETRY_PRIMARY_S]
        return fresh + [i for i in range(len(self.nodes)) if i not in fresh]

    def send(self, msg):
        """Forward one message; returns the JSON-RPC response, or None for a notification.

        `initialize` is answered here at once (and replayed to a node on first use), so the host never waits
        for the tailnet. tools/list falls back to the last list a node gave if no node answers within
        LIST_WAIT_S. Right after start the device may report Running before it can reach peers; during the
        first WARMUP_S seconds a message that no node took is retried. If every node fails without a
        connection, the device is checked and restarted if it is hung or offline, then the message is retried."""
        method = msg.get("method")
        if method == "initialize":
            self.init_msg = msg
            self.session.clear()
            self.active = None
            want = (msg.get("params") or {}).get("protocolVersion")
            return {"jsonrpc": "2.0", "id": msg.get("id"), "result": {
                "protocolVersion": want if want in PROTOCOLS else "2025-06-18", "capabilities": {"tools": {}},
                "serverInfo": {"name": "gboost", "version": version()},
                "instructions": "GBOOST. Tools need an open session; the first call triggers a session check "
                                "through the owner's routine."}}
        if method == "notifications/initialized":
            return None  # sent to each node when the client attaches to it
        if method == "tools/list" and not self.ready.wait(LIST_WAIT_S):
            cached = self.cached_tools(msg)
            if cached:
                return cached
        self.ready.wait(WARMUP_S)
        if self.dev_error:
            return error(msg, -32000, "GBOOST's tailnet device did not start (%s)." % self.dev_error)
        healed = False
        while True:
            resp, delivered = self.send_once(msg)
            if delivered:
                if method == "tools/list":
                    self.save_tools(resp)
                return resp
            if self.dev and self.last_conn_fail and not healed and time.time() - self.started >= WARMUP_S:
                healed = True
                try:
                    if self.heal():
                        self.down_since.clear()
                        continue
                except Exception as e:  # noqa: BLE001
                    log("device restart failed: %s" % e)
            if not self.dev or time.time() - self.started >= WARMUP_S:
                if method == "tools/list":
                    return self.cached_tools(msg) or resp
                return resp
            self.down_since.clear()
            time.sleep(1.5)

    def send_once(self, msg):
        last = "no nodes"
        self.last_conn_fail = True
        for i in self.order():
            url = self.nodes[i]
            try:
                if i != self.active:
                    self.attach(i)
                st, body = self.post(url, msg)
                self.down_since.pop(i, None)
                if "id" not in msg:
                    return None, True
                return (body if body is not None else error(msg, -32000, "GBOOST node answered %d with no body" % st)), True
            except NodeFailed as e:
                last = str(e)
                self.last_conn_fail = self.last_conn_fail and e.conn
                self.down_since[i] = time.time()
                self.session.pop(url, None)
                if self.active == i:
                    self.active = None
                log("node %d failed: %s" % (i, last))
        if "id" not in msg:
            return None, False
        return error(msg, -32000, "No GBOOST node is reachable (%s).%s" % (last, self.login_hint())), False


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
        # The device is shared: the host may run several client processes, and it outlives each of them
        # (the next start adopts it), so no process stops it on exit. It comes up in the background.
        dev = device(cfg)
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    c = Client(cfg, key, dev)
    if dev:
        threading.Thread(target=c.bring_up_device, daemon=True).start()
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


def version():
    p = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "VERSION")
    try:
        with open(p, encoding="utf-8") as f:
            return f.read().strip()
    except OSError:
        return "dev"


def cmd_selftest():
    """No network: a new version must load, read this computer's config and key, and find Tailscale."""
    try:
        cfg, key = load()
        if ":" not in key:
            raise ValueError("client.key is not <id>:<secret>")
        for u in cfg["nodes"]:
            if not isinstance(u, str) or not u.startswith(("https://", "http://")):
                raise ValueError("bad node URL in config.json")
        ts = cfg.get("tailscale") or {}
        if ts.get("enabled"):
            gboost_net.find_bin("tailscaled", ts_bin_dir(cfg))  # exactly as device() looks for them
            gboost_net.find_bin("tailscale", ts_bin_dir(cfg))
    except (OSError, ValueError, KeyError, TypeError, SystemExit) as e:
        print("SELFTEST FAILED %s" % e, flush=True)
        sys.exit(1)
    print("SELFTEST OK " + version(), flush=True)


def main(argv):
    if len(argv) > 1 and argv[1] == "selftest":
        return cmd_selftest()
    if len(argv) > 1 and argv[1] == "login":
        return cmd_login()
    if len(argv) > 1 and argv[1] == "status":
        cfg, _ = load()
        print(json.dumps({"nodes": cfg["nodes"], "tailscale": cfg.get("tailscale")}, indent=2))
        return
    serve()


if __name__ == "__main__":
    main(sys.argv)
