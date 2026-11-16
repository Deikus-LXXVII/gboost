#!/usr/bin/env python3
"""GBOOST updater for a node (spike S11, plan 6.7). Python 3.9+ stdlib only.

Layout under --root (default ~/.gboost/app), all owner-only:
  versions/<version>/   unpacked release packages (VERSION, SCHEMA, bin/, node-net/, release.pub)
  current               the running version
  pending.json          a switch in progress (finished or undone by the next command after a crash)
  history.json          [{"version", "time", "failed"?, "rolled_back"?}], oldest first
  snapshots/<name>/     state copies taken (with the node stopped) before a schema change
  update.json           {"source": {"github": "<owner>/<repo>"} | {"dir": "<folder>"}, "target": "<os>-<cpu>",
                         "node": {"name", "state", "agent_port", "owner_port", "ts_bin_dir", "no_tailscale"}}

Commands:
  install <package.tar.gz>   first install of a trusted package (from the release the Bot verified)
  start | stop | status
  check                      is a newer signed release available? (never installs)
  apply                      install the newest signed release: side by side, switch, health check,
                             automatic rollback if the new version fails
  rollback [--to V]          go back to a version this node already ran (never downloads)
  list                       installed versions and history

Rules: only a manifest signed by the release key of the version that runs now; only a version above the
running one that has not failed or been rolled back here; every package must match its SHA-256; rollback
only to versions on disk. Every command that changes anything holds update.lock.
"""
import argparse, datetime, hashlib, json, os, platform, re, shutil, subprocess, sys, tarfile, time
import urllib.error, urllib.request

KEEP = 3
HEALTH_S = float(os.environ.get("GBOOST_HEALTH_S", "30"))
MAX_DOWNLOAD = 200 * 1024 * 1024
VERSION_RE = re.compile(r"^\d{1,6}\.\d{1,6}\.\d{1,6}$")


class UpdateError(Exception):
    pass


def check_version(v):
    if not isinstance(v, str) or not VERSION_RE.match(v):
        raise UpdateError("not a version: %r" % (v,))
    return v


def vtuple(v):
    return tuple(int(x) for x in check_version(v).split("."))


def target():
    osn = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system(), platform.system().lower())
    cpu = platform.machine().lower()
    cpu = {"amd64": "x86_64", "aarch64": "arm64"}.get(cpu, cpu)
    return "%s-%s" % (osn, cpu)


def exe(name):
    return name + (".exe" if os.name == "nt" else "")


def private_dir(path):
    os.makedirs(path, exist_ok=True)
    if os.name != "nt":
        os.chmod(path, 0o700)


def write_file_atomic(path, data, mode=0o600):
    tmp = "%s.%d.tmp" % (path, os.getpid())
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode)
    with os.fdopen(fd, "wb") as f:
        f.write(data if isinstance(data, bytes) else data.encode("utf-8"))
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


class Lock:
    def __init__(self, path):
        self.path, self.f = path, None

    def __enter__(self):
        self.f = open(self.path, "a+")
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(self.f.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(self.f.fileno(), fcntl.LOCK_EX)
        return self

    def __exit__(self, *a):
        if os.name == "nt":
            import msvcrt
            try:
                msvcrt.locking(self.f.fileno(), msvcrt.LK_UNLCK, 1)
            except OSError:
                pass
        self.f.close()


class Updater:
    def __init__(self, root):
        self.root = os.path.abspath(os.path.expanduser(root))
        private_dir(self.root)
        for d in ("versions", "snapshots", "downloads"):
            private_dir(self.p(d))
        self.cfg = self.read_json("update.json", {})
        self.proc = None  # the launcher we started in this process, if any

    def p(self, *parts):
        return os.path.join(self.root, *parts)

    def read_json(self, name, default):
        path = self.p(name)
        if not os.path.exists(path):
            return default
        try:
            with open(path, encoding="utf-8") as f:
                v = json.load(f)
        except (OSError, ValueError):
            raise UpdateError("%s is unreadable; fix or remove it by hand" % name)
        if type(v) is not type(default):
            raise UpdateError("%s has an unexpected shape" % name)
        return v

    def write_json(self, name, v):
        write_file_atomic(self.p(name), json.dumps(v, indent=1))

    def say(self, msg):
        print(msg, flush=True)

    # --- versions and history ---

    def current(self):
        try:
            v = open(self.p("current"), encoding="utf-8").read().strip()
        except OSError:
            return None
        return check_version(v) if v else None

    def installed(self):
        out = []
        for v in os.listdir(self.p("versions")):
            if VERSION_RE.match(v) and os.path.exists(self.p("versions", v, "VERSION")):
                out.append(v)
        return sorted(out, key=vtuple)

    def schema_of(self, v):
        """From the package itself (covered by the signed hash), not from editable history."""
        try:
            return int(open(self.p("versions", v, "SCHEMA")).read().strip())
        except (OSError, ValueError):
            return None

    def history(self):
        return self.read_json("history.json", [])

    def mark(self, version, **flags):
        h = [x for x in self.history() if x.get("version") != version]
        old = next((x for x in self.history() if x.get("version") == version), {})
        old.update({"version": version, "time": datetime.datetime.now(datetime.timezone.utc).isoformat()}, **flags)
        h.append(old)
        self.write_json("history.json", h)

    def blocked(self):
        return {x["version"] for x in self.history() if x.get("failed") or x.get("rolled_back")}

    # --- the launcher process ---

    def pid_info(self):
        try:
            d = json.load(open(self.p("launcher.pid")))
            return int(d["pid"]), d.get("tag")
        except (OSError, ValueError, KeyError, TypeError):
            return None, None

    def alive(self):
        """Is our launcher running? Checks that the PID still belongs to it (stop file path on its line)."""
        if self.proc is not None:
            return self.proc.poll() is None
        pid, tag = self.pid_info()
        if not pid:
            return False
        if os.name == "nt":
            r = subprocess.run(["powershell", "-NoProfile", "-Command",
                                "(Get-CimInstance Win32_Process -Filter 'ProcessId=%d').CommandLine" % pid],
                               capture_output=True, text=True)
            return bool(tag) and tag in r.stdout
        try:
            os.kill(pid, 0)
        except OSError:
            return False
        r = subprocess.run(["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True)
        return bool(tag) and tag in r.stdout

    def start(self):
        v = self.current()
        if not v:
            raise UpdateError("nothing installed")
        if self.alive():
            raise UpdateError("the node is already running")
        n = self.cfg.get("node") or {}
        vdir = self.p("versions", v)
        stop_file = self.p("stop")
        args = [sys.executable, "-I", os.path.join(vdir, "node-net", "gboost_node.py"),
                "--name", n["name"], "--core", os.path.join(vdir, "bin", exe("gboost-s9-core")),
                "--state", os.path.expanduser(n["state"]), "--agent-port", str(n["agent_port"]),
                "--owner-port", str(n["owner_port"]), "--stop-file", stop_file]
        if n.get("ts_bin_dir"):
            args += ["--ts-bin-dir", n["ts_bin_dir"]]
        if n.get("no_tailscale"):
            args.append("--no-tailscale")
        try:
            os.remove(stop_file)
        except FileNotFoundError:
            pass
        fd = os.open(self.p("launcher.out"), os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
        # Windows: DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, so closing the console does not end the node.
        kw = {"creationflags": 0x00000008 | 0x00000200} if os.name == "nt" else {"start_new_session": True}
        with os.fdopen(fd, "ab") as out:
            self.proc = subprocess.Popen(args, stdout=out, stderr=out, stdin=subprocess.DEVNULL, **kw)
        self.write_json("launcher.pid", {"pid": self.proc.pid, "tag": stop_file})

    def stop(self):
        if not self.alive():
            self.clear_pid()
            return True
        open(self.p("stop"), "w").close()  # the launcher stops the core and its Tailscale device cleanly
        end = time.time() + 20
        while time.time() < end and self.alive():
            time.sleep(0.3)
        if self.alive():
            pid, _ = self.pid_info()
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)], capture_output=True)
            else:
                try:
                    os.killpg(pid, 9)
                except OSError:
                    pass
            time.sleep(1)
        if self.proc is not None:
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                pass
        gone = not self.alive()
        if gone:
            self.clear_pid()
        return gone

    def clear_pid(self):
        self.proc = None
        try:
            os.remove(self.p("launcher.pid"))
        except FileNotFoundError:
            pass

    def healthy(self, want_version):
        """The launcher is alive and the core answers /healthz with the expected version."""
        port = (self.cfg.get("node") or {}).get("owner_port")
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        end = time.time() + HEALTH_S
        while time.time() < end:
            if not self.alive():
                return False, "the launcher exited"
            try:
                with opener.open("http://127.0.0.1:%d/healthz" % port, timeout=2) as r:
                    h = json.load(r)
                if h.get("ok") and h.get("version") == want_version:
                    # The agent listener must also serve: a request without a key gets 401.
                    agent = (self.cfg.get("node") or {}).get("agent_port")
                    req = urllib.request.Request("http://127.0.0.1:%d/mcp" % agent, data=b"{}", method="POST",
                                                 headers={"content-type": "application/json"})
                    try:
                        opener.open(req, timeout=2)
                    except urllib.error.HTTPError as e:
                        if e.code == 401:
                            return True, "ok"
            except (OSError, ValueError):
                pass
            time.sleep(0.5)
        return False, "no healthy answer within %ds" % HEALTH_S

    # --- state snapshots ---

    def state_dir(self):
        return os.path.expanduser(self.cfg["node"]["state"])

    def snapshot(self, label):
        """Copy the state files (node stopped). Owner-only; pruned with the versions."""
        dest = self.p("snapshots", label)
        private_dir(dest)
        for n in os.listdir(self.state_dir()):
            if n.endswith((".json", ".toml")) and os.path.isfile(os.path.join(self.state_dir(), n)):
                with open(os.path.join(self.state_dir(), n), "rb") as f:
                    write_file_atomic(os.path.join(dest, n), f.read())
        return dest

    # Files a schema rollback may put back. Secrets and settings (routine.toml, console.token, keys) are
    # never taken from a snapshot: the live ones stay. Sessions end (a new check opens them).
    SCHEMA_FILES = ("clients.json",)

    def live_clients(self):
        path = os.path.join(self.state_dir(), "clients.json")
        if not os.path.exists(path):
            return {}
        try:
            return {c["id"]: c for c in json.load(open(path, encoding="utf-8"))}
        except (OSError, ValueError, TypeError, KeyError):
            raise UpdateError("the live clients.json is unreadable; a schema rollback would lose revocations, so it stops here")

    def restore_plan(self, snap):
        live = self.live_clients()
        try:
            old = {c["id"] for c in json.load(open(os.path.join(snap, "clients.json")))}
        except (OSError, ValueError, TypeError, KeyError):
            old = set()
        gone = sorted(i for i, c in live.items() if i not in old and c.get("state") == "approved")
        return ("clients enrolled since: " + ", ".join(gone)) if gone else "nothing"

    def restore(self, snap):
        """Put the schema files of a snapshot back without undoing anything decided since it was taken:
        for a client that exists now, its current secret hash and state win (revoked stays revoked)."""
        state = self.state_dir()
        live = self.live_clients()
        for n in self.SCHEMA_FILES:
            src = os.path.join(snap, n)
            if not os.path.exists(src):
                continue
            clients = json.load(open(src, encoding="utf-8"))
            for c in clients:
                now = live.get(c.get("id"))
                if now:
                    c["secret_hash"] = now.get("secret_hash", c.get("secret_hash"))
                    c["state"] = now.get("state", c.get("state"))
                    c["revoked_ms"] = now.get("revoked_ms", c.get("revoked_ms"))
            write_file_atomic(os.path.join(state, n), json.dumps(clients))
        try:
            os.remove(os.path.join(state, "msessions.json"))
        except FileNotFoundError:
            pass

    # --- switching versions ---

    def switch(self, new, reason, restore_first=None):
        """Stop, (snapshot), point `current` at `new`, start, check. Any failure puts the old version back."""
        old = self.current()
        if not old:
            raise UpdateError("nothing is running to switch from")
        self.write_json("pending.json", {"old": old, "new": new, "reason": reason})
        try:
            if not self.stop():
                raise UpdateError("could not stop the running node")
            snap = None
            if restore_first or self.schema_of(old) != self.schema_of(new):
                snap = self.snapshot("%d-from-%s-to-%s" % (int(time.time()), old, new))
                p = self.read_json("pending.json", {})
                p["snap"] = snap
                self.write_json("pending.json", p)
            if restore_first:
                self.restore(restore_first)
                self.say("RESTORED state snapshot %s" % os.path.basename(restore_first))
            write_file_atomic(self.p("current"), new)
            self.start()
            ok, why = self.healthy(new)
        except Exception as e:  # noqa: BLE001 - any failure here must end in the old version running
            ok, why = False, "%s: %s" % (type(e).__name__, e)
        if ok:
            os.remove(self.p("pending.json"))
            self.say("OK %s %s -> %s" % (reason, old, new))
            return True
        self.say("FAILED %s %s: %s; going back to %s" % (reason, new, why, old))
        self.mark(new, failed=True)
        self.undo_pending()
        return False

    def undo_pending(self):
        """Bring back the version recorded in pending.json (after a failure or a crash mid-switch)."""
        p = self.read_json("pending.json", {})
        old = check_version(p["old"])
        if old not in self.installed():
            raise UpdateError("the version to go back to (%s) is not on this node; the owner must look" % old)
        self.stop()
        if p.get("snap") and os.path.isdir(p["snap"]):
            self.restore(p["snap"])
        write_file_atomic(self.p("current"), old)
        self.start()
        ok, why = self.healthy(old)
        os.remove(self.p("pending.json"))
        if not ok:
            self.stop()
            self.say("STOPPED %s failed its check too (%s); the owner must look" % (old, why))
            raise SystemExit(3)
        self.say("ROLLED BACK to %s" % old)

    def recover(self):
        """A crash during a switch leaves pending.json. If the new version is current and passes its check,
        the switch had finished: keep it. Otherwise undo it. True if a node was (re)started here."""
        if not os.path.exists(self.p("pending.json")):
            return False
        p = self.read_json("pending.json", {})
        self.say("RECOVERING an interrupted switch")
        if self.current() == p.get("new"):
            if not self.alive():
                self.start()
            ok, _ = self.healthy(p["new"])
            if ok:
                os.remove(self.p("pending.json"))
                self.say("KEPT %s (the switch had finished)" % p["new"])
                return True
        self.undo_pending()
        return True

    def prune(self):
        # Keep the running version and the newest earlier ones that worked; failed versions are useless.
        keep = {self.current()}
        failed = {x["version"] for x in self.history() if x.get("failed")}
        for v in sorted(self.installed(), key=vtuple, reverse=True):
            if len(keep) >= KEEP:
                break
            if v not in failed and vtuple(v) < vtuple(self.current()):
                keep.add(v)
        for v in self.installed():
            if v not in keep:
                shutil.rmtree(self.p("versions", v), ignore_errors=True)
        for s in os.listdir(self.p("snapshots")):
            m = re.match(r"^\d+-from-(.+)-to-(.+)$", s)
            if not m or m.group(1) not in keep or m.group(2) not in keep:
                shutil.rmtree(self.p("snapshots", s), ignore_errors=True)
        shutil.rmtree(self.p("downloads"), ignore_errors=True)
        private_dir(self.p("downloads"))

    # --- releases ---

    def source(self):
        """(folder with manifest + signature, function that fetches a named file into a path)."""
        src = self.cfg.get("source") or {}
        dl = self.p("downloads", "incoming")
        shutil.rmtree(dl, ignore_errors=True)
        private_dir(dl)
        if "dir" in src:
            def get(name, dest):
                shutil.copy(os.path.join(src["dir"], name), dest)
        elif src.get("github"):
            api = "https://api.github.com/repos/%s/releases/latest" % src["github"]
            with urllib.request.urlopen(urllib.request.Request(api, headers={"Accept": "application/vnd.github+json"}), timeout=30) as r:
                assets = {a["name"]: a["browser_download_url"] for a in json.load(r).get("assets", [])}

            def get(name, dest):
                if name not in assets:
                    raise UpdateError("the release has no " + name)
                with urllib.request.urlopen(assets[name], timeout=300) as r, open(dest, "wb") as f:
                    n = 0
                    while True:
                        b = r.read(1 << 20)
                        if not b:
                            break
                        n += len(b)
                        if n > MAX_DOWNLOAD:
                            raise UpdateError(name + " is too large")
                        f.write(b)
        else:
            raise UpdateError("update.json has no source")
        for n in ("manifest.json", "manifest.json.minisig"):
            get(n, os.path.join(dl, n))
        return dl, get

    def verify(self, manifest_path):
        """Check the signature with the tool and key of the version that runs now."""
        cur = self.current()
        if not cur:
            raise UpdateError("nothing installed to verify with")
        vdir = self.p("versions", cur)
        r = subprocess.run([os.path.join(vdir, "bin", exe("gboost-release")), "verify", manifest_path,
                            os.path.join(vdir, "release.pub")], capture_output=True, text=True)
        if r.returncode != 0:
            raise UpdateError("signature check failed: " + (r.stderr or r.stdout).strip())
        with open(manifest_path, encoding="utf-8") as f:
            m = json.load(f)
        if m.get("product") != "gboost" or not isinstance(m.get("files"), dict):
            raise UpdateError("not a GBOOST manifest")
        check_version(m.get("version"))
        return m

    def check(self):
        dl, get = self.source()
        m = self.verify(os.path.join(dl, "manifest.json"))
        newer = vtuple(m["version"]) > vtuple(self.current()) and m["version"] not in self.blocked()
        return m, newer, dl, get

    def unpack(self, tarball, version):
        dest = self.p("versions", version)
        if os.path.exists(dest):
            raise UpdateError("version %s is already on this node" % version)
        tmp = self.p("downloads", "unpack-" + version)
        shutil.rmtree(tmp, ignore_errors=True)
        private_dir(tmp)
        with tarfile.open(tarball) as t:
            if hasattr(tarfile, "data_filter"):
                t.extractall(tmp, filter="data")
            else:
                for mem in t.getmembers():
                    parts = mem.name.replace("\\", "/").split("/")
                    if mem.name.startswith(("/", "\\")) or ":" in mem.name or ".." in parts or not (mem.isfile() or mem.isdir()):
                        raise UpdateError("unsafe entry in package: " + mem.name)
                    mem.mode &= 0o755
                t.extractall(tmp)
        inner = [d for d in os.listdir(tmp) if os.path.isdir(os.path.join(tmp, d))]
        root = os.path.join(tmp, inner[0]) if len(inner) == 1 and not os.path.exists(os.path.join(tmp, "VERSION")) else tmp
        if open(os.path.join(root, "VERSION")).read().strip() != version:
            raise UpdateError("package VERSION does not match the manifest")
        if os.name != "nt":
            for b in os.listdir(os.path.join(root, "bin")):
                os.chmod(os.path.join(root, "bin", b), 0o755)
        shutil.move(root, dest)
        shutil.rmtree(tmp, ignore_errors=True)

    # --- commands ---

    def apply(self):
        self.recover()
        m, newer, dl, get = self.check()
        if not newer:
            self.say("UP TO DATE %s" % self.current())
            return True
        v = m["version"]
        name = "gboost-%s-%s.tar.gz" % (v, self.cfg.get("target") or target())
        if name not in m["files"]:
            raise UpdateError("the release has no package for " + (self.cfg.get("target") or target()))
        tarball = os.path.join(dl, name)
        get(name, tarball)
        h = hashlib.sha256()
        with open(tarball, "rb") as f:
            for b in iter(lambda: f.read(1 << 20), b""):
                h.update(b)
        if h.hexdigest() != m["files"][name]["sha256"]:
            raise UpdateError("package hash does not match the signed manifest")
        if os.path.exists(self.p("versions", v)):
            shutil.rmtree(self.p("versions", v))  # an earlier, unfinished unpack of the same signed version
        self.unpack(tarball, v)
        self.mark(v)
        ok = self.switch(v, "update")
        self.prune()
        return ok

    def rollback(self, to=None, confirm=False):
        self.recover()
        cur = self.current()
        kept = self.installed()
        older = [v for v in kept if vtuple(v) < vtuple(cur) and v not in self.blocked()]
        if to is None:
            if not older:
                raise UpdateError("no earlier version is kept on this node")
            to = max(older, key=vtuple)
        if to not in kept:
            raise UpdateError("version %s is not on this node; rollback never downloads" % to)
        if to not in older:
            raise UpdateError("%s is not an earlier version that worked here" % to)
        restore = None
        if self.schema_of(to) != self.schema_of(cur):
            snaps = sorted(d for d in os.listdir(self.p("snapshots")) if d.endswith("-from-%s-to-%s" % (to, cur)))
            if not snaps:
                raise UpdateError("%s uses another state schema and no snapshot from that update is kept" % to)
            restore = self.p("snapshots", snaps[-1])
            lost = self.restore_plan(restore)
            self.say("PLAN state goes back to the snapshot %s. Lost: %s. Kept: current secrets, the routine "
                     "webhook, revocations. Open sessions end and need a new check." % (snaps[-1], lost))
            if not confirm:
                self.say("CONFIRM run again with --confirm to roll back")
                return False
        ok = self.switch(to, "rollback", restore_first=restore)
        if ok:
            self.mark(cur, rolled_back=True)  # the next apply does not put it straight back
        return ok

    def install(self, tarball, manifest, tool, pubkey):
        """First install. The package must be listed in a manifest signed by `pubkey`, checked with `tool`
        (both from the repository the owner named, built and pinned there), and carry that same key."""
        if self.current():
            raise UpdateError("already installed (%s); use apply" % self.current())
        r = subprocess.run([tool, "verify", manifest, pubkey], capture_output=True, text=True)
        if r.returncode != 0:
            raise UpdateError("signature check failed: " + (r.stderr or r.stdout).strip())
        m = json.load(open(manifest, encoding="utf-8"))
        name = os.path.basename(tarball)
        h = hashlib.sha256(open(tarball, "rb").read()).hexdigest()
        if m.get("product") != "gboost" or (m.get("files") or {}).get(name, {}).get("sha256") != h:
            raise UpdateError("the package is not the one the signed manifest lists")
        v = check_version(m.get("version"))
        self.unpack(tarball, v)
        if open(self.p("versions", v, "release.pub"), "rb").read() != open(pubkey, "rb").read():
            shutil.rmtree(self.p("versions", v), ignore_errors=True)
            raise UpdateError("the package carries a different release key")
        self.mark(v)
        write_file_atomic(self.p("current"), v)
        self.say("INSTALLED %s" % v)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=os.path.join("~", ".gboost", "app"))
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("install"); s.add_argument("package"); s.add_argument("--manifest", required=True)
    s.add_argument("--tool", required=True, help="gboost-release built from the repository")
    s.add_argument("--pubkey", required=True, help="release/release.pub from the repository")
    for c in ("start", "stop", "status", "check", "apply", "list"):
        sub.add_parser(c)
    s = sub.add_parser("rollback"); s.add_argument("--to"); s.add_argument("--confirm", action="store_true")
    a = ap.parse_args()
    try:
        u = Updater(a.root)
        if a.cmd in ("status", "list", "check"):
            if a.cmd == "status":
                u.say(json.dumps({"current": u.current(), "installed": u.installed(), "running": u.alive(),
                                  "pending": os.path.exists(u.p("pending.json"))}))
            elif a.cmd == "list":
                u.say(json.dumps(u.history(), indent=1))
            else:
                m, newer, _, _ = u.check()
                u.say(("NEW %s" % m["version"]) if newer else ("UP TO DATE %s" % u.current()))
            return
        with Lock(u.p("update.lock")):
            if a.cmd == "install":
                u.install(a.package, a.manifest, a.tool, a.pubkey)
            elif a.cmd == "start":
                if not u.recover():
                    u.start()
                ok, why = u.healthy(u.current())
                u.say(("RUNNING " if ok else "UNHEALTHY ") + str(u.current()) + ("" if ok else " (%s)" % why))
                sys.exit(0 if ok else 1)
            elif a.cmd == "stop":
                u.say("STOPPED" if u.stop() else "STILL RUNNING")
            elif a.cmd == "apply":
                sys.exit(0 if u.apply() else 1)
            elif a.cmd == "rollback":
                sys.exit(0 if u.rollback(a.to, a.confirm) else 1)
    except (UpdateError, OSError, ValueError, KeyError, TypeError) as e:
        print("ERROR %s" % e, flush=True)
        sys.exit(2)


if __name__ == "__main__":
    main()
