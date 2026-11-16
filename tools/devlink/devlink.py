#!/usr/bin/env python3
"""devlink: a development channel between a coding agent and the owner's Grok Bot.

Agent -> Bot: `devlink.py send "text"` posts {kind: dev_message, id, text} to the owner's Grok routine webhook.
Bot -> agent: the Bot calls the `devlink_send` tool on a Remote MCP connector served by `devlink.py serve`;
each message is appended to inbox.jsonl, which the agent watches (`devlink.py watch`).

Python 3.9+ stdlib only; Windows, macOS, Linux. Not part of GBOOST itself.
Config folder: $DEVLINK_HOME, else %APPDATA%\\gboost-devlink (Windows) or ~/.config/gboost-devlink.
"""
import argparse, datetime, hmac, json, os, secrets, sys, threading, time, urllib.error, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_TEXT = 20000
MAX_BODY = 64 * 1024
MAX_PER_HOUR = 120
PROTOCOL = "2025-06-18"


def home():
    if os.environ.get("DEVLINK_HOME"):
        return os.environ["DEVLINK_HOME"]
    if os.name == "nt":
        return os.path.join(os.environ.get("APPDATA", os.path.expanduser("~")), "gboost-devlink")
    return os.path.join(os.path.expanduser("~"), ".config", "gboost-devlink")


def path(name):
    return os.path.join(home(), name)


def ensure_home():
    os.makedirs(home(), exist_ok=True)
    if os.name != "nt":
        os.chmod(home(), 0o700)


def write_private(name, text):
    """Owner-only file (0600 on Unix; the user-profile ACL on Windows)."""
    ensure_home()
    p = path(name)
    tmp = p + ".tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    os.replace(tmp, p)


def append_private(name, obj):
    ensure_home()
    fd = os.open(path(name), os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    with os.fdopen(fd, "a", encoding="utf-8") as f:
        f.write(json.dumps(obj, ensure_ascii=False) + "\n")


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def new_id(prefix):
    return prefix + secrets.token_urlsafe(6)


# ---------- setup ----------

def cmd_setup(a):
    """Store the routine webhook (from files, never argv) and make the connector key."""
    ensure_home()
    if a.url_file and a.key_file:
        url = open(a.url_file, encoding="utf-8").read().strip()
        key = open(a.key_file, encoding="utf-8").read().strip()
        if not url.startswith("https://"):
            sys.exit("error: the webhook URL must be https")
        write_private("routine.json", json.dumps({"url": url, "key": key}))
        print("routine webhook stored in", path("routine.json"))
    if not os.path.exists(path("connector.key")) or a.rotate_key:
        write_private("connector.key", secrets.token_urlsafe(32))
        print("new connector key in", path("connector.key"), "(header x-devlink-key)")
    else:
        print("connector key kept:", path("connector.key"))


# ---------- send (agent -> Bot) ----------

def cmd_send(a):
    text = sys.stdin.read() if a.text == "-" else a.text
    text = text.strip()
    if not text:
        sys.exit("error: empty message")
    if len(text) > MAX_TEXT:
        sys.exit(f"error: message longer than {MAX_TEXT} characters")
    try:
        r = json.load(open(path("routine.json"), encoding="utf-8"))
    except OSError:
        sys.exit("error: no routine webhook; run setup first")
    mid = new_id("dm_")
    payload = {
        "source": "devlink",
        "kind": "dev_message",
        "id": mid,
        "reply_to": a.reply_to,
        "text": text,
        "reply_with": "Call the devlink connector's tool devlink_send with your answer; set reply_to to this id.",
    }
    req = urllib.request.Request(
        r["url"], data=json.dumps(payload).encode(), method="POST",
        headers={"Authorization": "Bearer " + r["key"], "Content-Type": "application/json"})
    opener = urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(req, timeout=20) as resp:
            status = resp.status
    except urllib.error.HTTPError as e:
        status = e.code
    except (urllib.error.URLError, OSError) as e:
        append_private("outbox.jsonl", {"id": mid, "time": now(), "status": "transport error", "text": text})
        sys.exit(f"error: webhook not reached ({type(e).__name__})")
    append_private("outbox.jsonl", {"id": mid, "time": now(), "status": status, "reply_to": a.reply_to, "text": text})
    if not 200 <= status < 300:
        sys.exit(f"error: webhook answered {status}")
    print(mid)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


# ---------- watch / inbox (agent side) ----------

def read_inbox():
    try:
        with open(path("inbox.jsonl"), encoding="utf-8") as f:
            return [json.loads(l) for l in f if l.strip()]
    except OSError:
        return []


def fmt(m):
    head = f"[{m['time']}] {m['id']}" + (f" re {m['reply_to']}" if m.get("reply_to") else "")
    return head + "\n" + m["text"]


def cmd_inbox(a):
    msgs = read_inbox()
    for m in msgs[-a.last:]:
        print(fmt(m) + "\n")


def cmd_watch(a):
    """Print each new inbox message as one line of JSON (for a background monitor)."""
    seen = len(read_inbox())
    while True:
        msgs = read_inbox()
        for m in msgs[seen:]:
            print(json.dumps(m, ensure_ascii=False), flush=True)
        seen = len(msgs)
        time.sleep(a.interval)


# ---------- serve (Bot -> agent, MCP Streamable HTTP) ----------

TOOLS = [
    {"name": "devlink_send",
     "description": "Send a message to the coding agent that develops GBOOST. Use it to answer a dev_message "
                    "(set reply_to to its id) or to report on your own. Plain text, up to 20000 characters.",
     "inputSchema": {"type": "object", "required": ["text"],
                     "properties": {"text": {"type": "string"}, "reply_to": {"type": "string"}}}},
    {"name": "devlink_history",
     "description": "Show the last messages in both directions (agent to Bot and Bot to agent).",
     "inputSchema": {"type": "object", "properties": {"last": {"type": "integer", "minimum": 1, "maximum": 50}}}},
]


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, addr, key):
        super().__init__(addr, Handler)
        self.key = key.encode()
        self.lock = threading.Lock()
        self.sent = []


def tool_result(text, error=False):
    return {"content": [{"type": "text", "text": text}], "isError": error}


def history(last):
    rows = []
    for name, who in (("outbox.jsonl", "agent"), ("inbox.jsonl", "bot")):
        try:
            with open(path(name), encoding="utf-8") as f:
                for l in f:
                    if l.strip():
                        m = json.loads(l)
                        rows.append((m["time"], who, m))
        except OSError:
            pass
    rows.sort(key=lambda r: r[0])
    out = []
    for t, who, m in rows[-last:]:
        out.append(f"[{t}] {who} {m['id']}" + (f" re {m['reply_to']}" if m.get("reply_to") else "") + "\n" + m["text"])
    return "\n\n".join(out) or "No messages yet."


class Handler(BaseHTTPRequestHandler):
    server_version = "devlink"

    def log_message(self, *args):
        pass

    def reply(self, status, obj=None):
        body = b"" if obj is None else json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(status)
        if obj is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.reply(405, {"error": "use POST"})

    def do_POST(self):
        if self.path.split("?")[0] != "/mcp":
            return self.reply(404, {"error": "not found"})
        got = self.headers.get("x-devlink-key", "").encode()
        if not hmac.compare_digest(got, self.server.key):
            return self.reply(401, {"jsonrpc": "2.0", "id": None, "error": {"code": -32001, "message": "unauthorized"}})
        n = int(self.headers.get("Content-Length") or 0)
        if n <= 0 or n > MAX_BODY:
            return self.reply(413, {"error": "body too large or empty"})
        try:
            msg = json.loads(self.rfile.read(n))
        except ValueError:
            return self.reply(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}})
        if not isinstance(msg, dict):
            return self.reply(400, {"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "one request per POST"}})
        if "id" not in msg:
            return self.reply(202)
        mid, method, params = msg["id"], msg.get("method"), msg.get("params") or {}
        if method == "initialize":
            res = {"protocolVersion": params.get("protocolVersion", PROTOCOL), "capabilities": {"tools": {}},
                   "serverInfo": {"name": "devlink", "version": "0.1"},
                   "instructions": "Channel to the coding agent that develops GBOOST. Answer dev_message wake-ups with devlink_send."}
        elif method == "ping":
            res = {}
        elif method == "tools/list":
            res = {"tools": TOOLS}
        elif method == "tools/call":
            res = self.call(params.get("name"), params.get("arguments") or {})
        else:
            return self.reply(200, {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})
        self.reply(200, {"jsonrpc": "2.0", "id": mid, "result": res})

    def call(self, name, args):
        if name == "devlink_send":
            text = args.get("text")
            if not isinstance(text, str) or not text.strip():
                return tool_result("text is required", True)
            if len(text) > MAX_TEXT:
                return tool_result(f"text is longer than {MAX_TEXT} characters; split it", True)
            reply_to = args.get("reply_to") if isinstance(args.get("reply_to"), str) else None
            with self.server.lock:
                t = time.time()
                self.server.sent = [s for s in self.server.sent if t - s < 3600]
                if len(self.server.sent) >= MAX_PER_HOUR:
                    return tool_result("Too many messages this hour; wait.", True)
                self.server.sent.append(t)
                m = {"id": new_id("bm_"), "time": now(), "reply_to": reply_to[:64] if reply_to else None, "text": text.strip()}
                append_private("inbox.jsonl", m)
            return tool_result(f"Delivered to the coding agent as {m['id']}.")
        if name == "devlink_history":
            last = args.get("last") if isinstance(args.get("last"), int) else 10
            return tool_result(history(max(1, min(50, last))))
        return tool_result(f"unknown tool {name}", True)


def cmd_serve(a):
    try:
        key = open(path("connector.key"), encoding="utf-8").read().strip()
    except OSError:
        sys.exit("error: no connector key; run setup first")
    host, _, port = a.bind.rpartition(":")
    if host not in ("127.0.0.1", "::1", "localhost") and not a.allow_non_loopback:
        sys.exit("error: bind to loopback and publish with a proxy (e.g. tailscale serve), or pass --allow-non-loopback")
    srv = Server((host, int(port)), key)
    print(f"devlink listening on http://{a.bind}/mcp (header x-devlink-key, inbox {path('inbox.jsonl')})", flush=True)
    srv.serve_forever()


def main():
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError):
            pass
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("setup", help="store the routine webhook from files, make the connector key")
    s.add_argument("--url-file"); s.add_argument("--key-file"); s.add_argument("--rotate-key", action="store_true")
    s.set_defaults(f=cmd_setup)
    s = sub.add_parser("send", help="send a message to the Bot (text, or - for stdin)")
    s.add_argument("text"); s.add_argument("--reply-to"); s.set_defaults(f=cmd_send)
    s = sub.add_parser("inbox", help="print the last messages from the Bot")
    s.add_argument("--last", type=int, default=5); s.set_defaults(f=cmd_inbox)
    s = sub.add_parser("watch", help="print each new message from the Bot as a JSON line")
    s.add_argument("--interval", type=float, default=2.0); s.set_defaults(f=cmd_watch)
    s = sub.add_parser("serve", help="run the MCP endpoint the Bot calls")
    s.add_argument("--bind", default="127.0.0.1:8795"); s.add_argument("--allow-non-loopback", action="store_true")
    s.set_defaults(f=cmd_serve)
    a = p.parse_args()
    a.f(a)


if __name__ == "__main__":
    main()
