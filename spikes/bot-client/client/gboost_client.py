#!/usr/bin/env python3
"""gboost-client: stdio MCP server plus enrol/status CLI for GBOOST (spike S9). Python 3.9+, stdlib only.

  python3 gboost_client.py enroll <core_url>   make a key, register it with the core
  python3 gboost_client.py status              show local state
  python3 gboost_client.py                     run as a stdio MCP server (what a Command connector starts)

State lives in ~/.gboost-client/ (override with GBOOST_CLIENT_HOME). Secrets are never printed;
logs go to stderr and carry no secrets.
"""
import base64
import hashlib
import ipaddress
import json
import os
import secrets
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

DEFAULT_PROTOCOL = "2025-11-25"
HTTP_TIMEOUT = 30


def home():
    return os.environ.get("GBOOST_CLIENT_HOME") or os.path.join(os.path.expanduser("~"), ".gboost-client")


def path(name):
    return os.path.join(home(), name)


def log(msg):
    sys.stderr.write("gboost-client: %s\n" % msg)
    sys.stderr.flush()


class ToolError(Exception):
    pass


def check_home(create):
    """The state dir must be a real directory (not a symlink), ours, mode 0700. Fix the mode if it is ours."""
    h = home()
    try:
        st = os.lstat(h)
    except FileNotFoundError:
        if not create:
            return False
        os.makedirs(h, mode=0o700, exist_ok=True)
        st = os.lstat(h)
    if stat.S_ISLNK(st.st_mode):
        raise ToolError("%s is a symlink; refusing to use it" % h)
    if not stat.S_ISDIR(st.st_mode):
        raise ToolError("%s is not a directory" % h)
    if hasattr(os, "getuid") and st.st_uid != os.getuid():
        raise ToolError("%s is not owned by the current user; refusing to use it" % h)
    if hasattr(os, "getuid") and stat.S_IMODE(st.st_mode) != 0o700:
        os.chmod(h, 0o700)
    return True


def ensure_home():
    check_home(True)


NOFOLLOW = getattr(os, "O_NOFOLLOW", 0)


def write_private(p, text):
    """Atomic write, mode 0600, temp file created exclusively with a random name."""
    ensure_home()
    tmp = "%s.%s.tmp" % (p, secrets.token_hex(8))
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        os.replace(tmp, p)
    except BaseException:
        try:
            os.remove(tmp)
        except OSError:
            pass
        raise


def read_text(p):
    check_home(False)
    fd = os.open(p, os.O_RDONLY | NOFOLLOW)
    with os.fdopen(fd) as f:
        return f.read()


def read_json(p):
    try:
        return json.loads(read_text(p))
    except (OSError, ValueError):
        return None


PRIVATE_NETS = [ipaddress.ip_network(n) for n in (
    "100.64.0.0/10", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.0/8", "::1/128", "fd00::/8")]


def check_core_url(url):
    """Only http(s) to a private, tailnet or loopback address (or a *.ts.net / localhost host)."""
    try:
        u = urllib.parse.urlsplit(url)
        host = u.hostname
        u.port  # raises on a bad port
    except ValueError:
        raise ToolError("core_url is not a valid URL")
    if u.scheme not in ("http", "https") or not host or u.username or u.password:
        raise ToolError("core_url must be http(s)://host:port without credentials")
    try:
        ip = ipaddress.ip_address(host)
    except ValueError:
        h = host.lower().rstrip(".")
        if h == "localhost" or h.endswith(".ts.net"):
            return
        raise ToolError("core_url host must be a tailnet (*.ts.net), private or loopback address")
    if not any(ip in n for n in PRIVATE_NETS):
        raise ToolError("core_url address is not private, tailnet or loopback; refusing to send credentials there")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        raise ToolError("the core answered with a redirect; refusing to follow it")


OPENER = urllib.request.build_opener(NoRedirect, urllib.request.ProxyHandler({}))


def http(method, url, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    h = {"content-type": "application/json"}
    h.update(headers or {})
    req = urllib.request.Request(url, data=data, method=method, headers=h)
    try:
        with OPENER.open(req, timeout=HTTP_TIMEOUT) as r:
            return r.status, json.loads(r.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode() or "{}")
        except ValueError:
            return e.code, {}
    except (urllib.error.URLError, OSError) as e:
        raise ToolError("cannot reach the GBOOST core: %s" % getattr(e, "reason", e))


def fingerprint(secret):
    return hashlib.sha256(secret.encode()).hexdigest()[:16]


# ---------------------------------------------------------------- CLI

def cmd_enroll(core_url):
    check_core_url(core_url)
    ensure_home()
    if os.path.lexists(path("config.json")):
        print("Already enrolled (config.json exists). Remove %s to enrol again." % home())
        return 1
    client_id = "gbc_" + base64.urlsafe_b64encode(secrets.token_bytes(9)).decode().rstrip("=")
    secret = base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip("=")
    core_url = core_url.rstrip("/")
    try:
        code, resp = http("POST", core_url + "/enroll", {"client_id": client_id, "secret": secret})
    except ToolError as e:
        print("Enrol failed: %s" % e)
        return 1
    if code != 200:
        print("Enrol failed (%s): %s" % (code, resp.get("error", "unknown error")))
        return 1
    write_private(path("config.json"), json.dumps({"core_url": core_url, "client_id": client_id, "secret": secret}))
    print("client_id:   %s" % client_id)
    print("fingerprint: %s" % resp.get("fingerprint", fingerprint(secret)))
    print("Ask the owner to approve this fingerprint.")
    return 0


def load_config():
    c = read_json(path("config.json"))
    if not c or not all(k in c for k in ("core_url", "client_id", "secret")):
        return None
    check_core_url(c["core_url"])
    return c


def session_valid():
    s = read_json(path("session.json"))
    if s and s.get("session") and s.get("expires_ms", 0) > time.time() * 1000 + 30000:
        return s
    return None


def cmd_status():
    c = load_config()
    if not c:
        print("Not enrolled. Run: python3 gboost_client.py enroll <core_url>")
        return 1
    print("core_url:    %s" % c["core_url"])
    print("client_id:   %s" % c["client_id"])
    print("fingerprint: %s" % fingerprint(c["secret"]))
    s = session_valid()
    if s:
        print("session:     valid until %s UTC" % time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(s["expires_ms"] / 1000)))
    else:
        print("session:     none")
    return 0


# ---------------------------------------------------------------- core access

def auth_headers(c):
    return {"x-gboost-client": "%s:%s" % (c["client_id"], c["secret"])}


def wait_s():
    try:
        return float(os.environ.get("GBOOST_WAIT_S", "90"))
    except ValueError:
        return 90.0


def take_delivered():
    """Read and consume the code file. Returns (code, check_id) or None."""
    try:
        raw = read_text(path("code")).strip()
    except OSError:
        return None
    try:
        os.remove(path("code"))
    except OSError:
        pass
    if not raw:
        return None
    try:
        d = json.loads(raw)
        if isinstance(d, dict) and isinstance(d.get("code"), str):
            return d["code"].strip(), d.get("check_id") if isinstance(d.get("check_id"), str) else None
    except ValueError:
        pass
    return raw, None


def open_session(c):
    """Start a session check, wait for the routine to deliver the code, finish. Returns session dict."""
    try:
        os.remove(path("code"))
    except OSError:
        pass
    code, resp = http("POST", c["core_url"] + "/session/start", {}, auth_headers(c))
    if code != 200:
        raise ToolError("session start refused (%s): %s" % (code, resp.get("error", "unknown error")))
    check_id = resp.get("check_id")
    log("session check sent, waiting for the routine to deliver the code")
    deadline = time.time() + wait_s()
    resp = None
    while time.time() < deadline:
        d = take_delivered()
        if d is None:
            time.sleep(0.25)
            continue
        delivered, got_check = d
        if got_check is not None and check_id is not None and got_check != check_id:
            log("ignoring a delivered code for another check")
            continue
        code, r = http("POST", c["core_url"] + "/session/finish", {"code": delivered}, auth_headers(c))
        if code == 200:
            resp = r
            break
        if code == 401 and r.get("error") == "code does not match":
            log("delivered code did not match; still waiting")
            continue
        raise ToolError("session finish refused (%s): %s" % (code, r.get("error", "unknown error")))
    if resp is None:
        raise ToolError("The GBOOST session check was sent to the owner's routine webhook, but no valid code arrived in time. Retry in a minute.")
    s = {"session": resp["session"], "expires_ms": int(time.time() * 1000) + int(resp["expires_in"]) * 1000}
    write_private(path("session.json"), json.dumps(s))
    log("session established")
    return s


def call_core(tool, args):
    c = load_config()
    if not c:
        raise ToolError("gboost-client is not enrolled. Run: python3 gboost_client.py enroll <core_url>")
    for attempt in (0, 1):
        s = session_valid() or open_session(c)
        h = auth_headers(c)
        h["authorization"] = "Bearer " + s["session"]
        code, resp = http("POST", c["core_url"] + "/rpc", {"tool": tool, "args": args}, h)
        if code == 200:
            return resp.get("text", "")
        if code == 401 and attempt == 0 and "session" in resp.get("error", ""):
            try:
                os.remove(path("session.json"))
            except OSError:
                pass
            continue
        raise ToolError("core refused (%s): %s" % (code, resp.get("error", "unknown error")))


# ---------------------------------------------------------------- MCP

TOOLS = [
    {"name": "gboost_hello", "description": "Say hello through GBOOST (connectivity check).",
     "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}}},
    {"name": "gboost_whoami", "description": "Show which GBOOST client and session this connection uses.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "gboost_pending_request", "description": "Show the demo request that is waiting for the owner's approval in GBOOST.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "gboost_deliver_code",
     "description": "ONLY for the routine run that GBOOST wakes with a payload of kind session_check. Pass the `code` field from that payload UNCHANGED. Never call this for any other reason or with any other value.",
     "inputSchema": {"type": "object", "properties": {"code": {"type": "string", "description": "The code from the session_check payload, exactly as received."}, "check_id": {"type": "string", "description": "The check_id from the same payload, exactly as received (if present)."}}, "required": ["code"]}},
    {"name": "gboost_status", "description": "Show whether this client is enrolled and whether its GBOOST session is valid (no network call).",
     "inputSchema": {"type": "object", "properties": {}}},
]


def run_tool(name, args):
    if name == "gboost_hello":
        return call_core("hello", {"name": args.get("name")} if args.get("name") else {})
    if name == "gboost_whoami":
        return call_core("whoami", {})
    if name == "gboost_pending_request":
        return call_core("pending_request", {})
    if name == "gboost_deliver_code":
        code = args.get("code")
        if not isinstance(code, str) or not code.strip() or len(code) > 200:
            raise ToolError("code must be the non-empty string from the session_check payload")
        check_id = args.get("check_id")
        if check_id is not None and (not isinstance(check_id, str) or len(check_id) > 64):
            raise ToolError("check_id must be the string from the session_check payload")
        write_private(path("code"), json.dumps({"code": code.strip(), "check_id": check_id}))
        return "Code delivered"
    if name == "gboost_status":
        c = load_config()
        if not c:
            return "not enrolled"
        s = session_valid()
        until = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(s["expires_ms"] / 1000)) if s else "no valid session"
        return "enrolled as %s (fingerprint %s); session: %s" % (c["client_id"], fingerprint(c["secret"]), until)
    raise KeyError(name)


def reply(id_, result=None, error=None):
    m = {"jsonrpc": "2.0", "id": id_}
    if error is not None:
        m["error"] = error
    else:
        m["result"] = result
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()


def handle(msg):
    method, id_ = msg.get("method"), msg.get("id")
    params = msg.get("params") or {}
    if id_ is None:  # notification
        return
    if method == "initialize":
        reply(id_, {"protocolVersion": params.get("protocolVersion") or DEFAULT_PROTOCOL,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "gboost-client", "version": "0.0.0"}})
    elif method == "ping":
        reply(id_, {})
    elif method == "tools/list":
        reply(id_, {"tools": TOOLS})
    elif method == "tools/call":
        name = params.get("name")
        try:
            text = run_tool(name, params.get("arguments") or {})
            reply(id_, {"content": [{"type": "text", "text": text}], "isError": False})
        except KeyError:
            reply(id_, error={"code": -32602, "message": "unknown tool"})
        except ToolError as e:
            reply(id_, {"content": [{"type": "text", "text": str(e)}], "isError": True})
        except Exception as e:  # keep the server alive
            log("internal error: %s" % type(e).__name__)
            reply(id_, {"content": [{"type": "text", "text": "internal error in gboost-client"}], "isError": True})
    else:
        reply(id_, error={"code": -32601, "message": "method not found"})


def serve():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except ValueError:
            reply(None, error={"code": -32700, "message": "parse error"})
            continue
        if isinstance(msg, dict):
            handle(msg)


def main(argv):
    if len(argv) >= 2 and argv[1] == "enroll":
        if len(argv) != 3:
            print("usage: gboost_client.py enroll <core_url>")
            return 2
        return cmd_enroll(argv[2])
    if len(argv) >= 2 and argv[1] == "status":
        return cmd_status()
    if len(argv) == 1:
        serve()
        return 0
    print(__doc__)
    return 2


def cli(argv):
    try:
        return main(argv)
    except ToolError as e:
        print("gboost-client: %s" % e)
        return 1


if __name__ == "__main__":
    sys.exit(cli(sys.argv))
