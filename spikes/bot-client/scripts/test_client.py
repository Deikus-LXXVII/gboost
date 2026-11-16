"""Unit tests for client/gboost_client.py (no core needed). Run: python3 -I scripts/test_client.py <spike root>"""
import importlib.util, json, os, shutil, sys, tempfile, threading, time, unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

ROOT = sys.argv.pop(1) if len(sys.argv) > 1 and os.path.isdir(sys.argv[1]) else os.getcwd()
spec = importlib.util.spec_from_file_location("gc", os.path.join(ROOT, "client", "gboost_client.py"))
gc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gc)


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="s9py_")
        self.home = os.path.join(self.tmp, "home")
        os.environ["GBOOST_CLIENT_HOME"] = self.home
        os.environ["GBOOST_WAIT_S"] = "5"

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)


class CoreUrl(Base):
    def test_refuses_public_and_odd_urls(self):
        for u in ["http://8.8.8.8:8790", "https://example.com", "http://100.63.255.255", "http://172.32.0.1", "http://[2001:db8::1]/",
                  "ftp://127.0.0.1", "file:///etc/passwd", "http://user:pw@127.0.0.1/", "http://evil.ts.net.example.com", "127.0.0.1:80", "http:///x"]:
            with self.assertRaises(gc.ToolError, msg=u):
                gc.check_core_url(u)

    def test_accepts_private_tailnet_loopback(self):
        for u in ["http://127.0.0.1:8790", "http://127.5.5.5", "http://[::1]:80", "http://100.64.0.1:1", "http://100.127.255.254", "http://10.1.2.3",
                  "http://172.16.0.1", "http://172.31.255.255", "http://192.168.1.2", "http://[fd12::1]/", "https://box.tail1234.ts.net", "http://localhost:8790"]:
            gc.check_core_url(u)

    def test_enroll_cli_refuses_public_url_without_network(self):
        self.assertEqual(gc.cli(["x", "enroll", "https://example.com"]), 1)
        self.assertFalse(os.path.exists(self.home))


class HomeSafety(Base):
    def test_symlink_home_refused(self):
        os.makedirs(os.path.join(self.tmp, "real"))
        os.symlink(os.path.join(self.tmp, "real"), self.home)
        with self.assertRaises(gc.ToolError):
            gc.write_private(gc.path("config.json"), "{}")
        self.assertEqual(os.listdir(os.path.join(self.tmp, "real")), [])

    def test_mode_fixed_when_ours(self):
        os.makedirs(self.home, mode=0o755)
        os.chmod(self.home, 0o755)
        gc.write_private(gc.path("a"), "x")
        self.assertEqual(os.stat(self.home).st_mode & 0o777, 0o700)
        self.assertEqual(os.stat(gc.path("a")).st_mode & 0o777, 0o600)
        self.assertEqual([n for n in os.listdir(self.home) if n.endswith(".tmp")], [])

    def test_temp_symlink_not_followed(self):
        victim = os.path.join(self.tmp, "victim")
        open(victim, "w").write("keep")
        gc.ensure_home()
        real = gc.secrets.token_hex
        gc.secrets.token_hex = lambda n: "fixed"
        try:
            os.symlink(victim, gc.path("a") + ".fixed.tmp")
            with self.assertRaises(OSError):
                gc.write_private(gc.path("a"), "pwned")
        finally:
            gc.secrets.token_hex = real
        self.assertEqual(open(victim).read(), "keep")

    def test_read_through_symlink_refused(self):
        gc.ensure_home()
        victim = os.path.join(self.tmp, "victim.json")
        open(victim, "w").write('{"a": 1}')
        os.symlink(victim, gc.path("config.json"))
        self.assertIsNone(gc.read_json(gc.path("config.json")))


class FakeCore(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        n = int(self.headers.get("content-length") or 0)
        body = json.loads(self.rfile.read(n) or b"{}")
        srv = self.server
        srv.hits.append((self.path, body))
        if self.path == "/redirect":
            self.send_response(302); self.send_header("Location", "http://127.0.0.1:%d/trap" % srv.server_port); self.end_headers(); return
        if self.path == "/trap":
            srv.trapped += 1
        out, code = {}, 200
        if self.path == "/session/start":
            out = {"pending": True, "expires_in": 300, "check_id": "chk1"}
        elif self.path == "/session/finish":
            if body["code"] == "real":
                out = {"session": "gbs_x", "expires_in": 3600, "budget": 5}
            else:
                code, out = 401, {"error": "code does not match"}
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header("content-length", str(len(data))); self.end_headers(); self.wfile.write(data)


class WithCore(Base):
    def setUp(self):
        super().setUp()
        self.srv = HTTPServer(("127.0.0.1", 0), FakeCore)
        self.srv.hits, self.srv.trapped = [], 0
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.url = "http://127.0.0.1:%d" % self.srv.server_port

    def tearDown(self):
        self.srv.shutdown(); self.srv.server_close()
        super().tearDown()

    def test_no_redirect_followed(self):
        with self.assertRaises(gc.ToolError) as cm:
            gc.http("POST", self.url + "/redirect", {})
        self.assertIn("redirect", str(cm.exception))
        self.assertEqual(self.srv.trapped, 0)
        self.assertEqual([h[0] for h in self.srv.hits], ["/redirect"])

    def test_stale_and_foreign_codes_then_real_code_succeeds(self):
        gc.ensure_home()
        cfg = {"core_url": self.url, "client_id": "gbc_test000000", "secret": "s" * 43}

        def deliver():
            for code, chk in [("stale-a", "OLD"), ("stale-b", None), ("real", "chk1")]:
                time.sleep(0.5)
                gc.write_private(gc.path("code"), json.dumps({"code": code, "check_id": chk}))
        threading.Thread(target=deliver, daemon=True).start()
        s = gc.open_session(cfg)
        self.assertEqual(s["session"], "gbs_x")
        finishes = [h[1]["code"] for h in self.srv.hits if h[0] == "/session/finish"]
        # the code for another check is never sent; the unlabelled stale one is sent, mismatches, and waiting goes on
        self.assertEqual(finishes, ["stale-b", "real"])
        self.assertFalse(os.path.exists(gc.path("code")))

    def test_wrong_codes_only_times_out(self):
        os.environ["GBOOST_WAIT_S"] = "1.5"
        gc.ensure_home()
        cfg = {"core_url": self.url, "client_id": "gbc_test000000", "secret": "s" * 43}
        threading.Thread(target=lambda: (time.sleep(0.3), gc.write_private(gc.path("code"), json.dumps({"code": "bad"}))), daemon=True).start()
        with self.assertRaises(gc.ToolError) as cm:
            gc.open_session(cfg)
        self.assertIn("no valid code", str(cm.exception))


if __name__ == "__main__":
    unittest.main(verbosity=1)
