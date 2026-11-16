"""GBOOST's own Tailscale device (spike S10).

Starts a userspace `tailscaled` with its own state folder and socket, so it never touches a Tailscale the
computer already runs and needs no root. Used by the node launcher and by the client on the Bot's VM.
Python 3.9+ stdlib only.
"""
import json, os, shutil, socket, subprocess, sys, tempfile, time


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


class Lock:
    """Exclusive lock file, so two GBOOST processes never run one Tailscale state at once."""

    def __init__(self, path):
        self.path = path
        self.f = None

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


def find_bin(name, bin_dir=None):
    exe = name + (".exe" if os.name == "nt" else "")
    if bin_dir:
        p = os.path.join(bin_dir, exe)
        if os.path.exists(p):
            return p
        raise FileNotFoundError(f"{exe} not found in {bin_dir}")
    p = shutil.which(exe)
    if not p:
        raise FileNotFoundError(f"{exe} not found on PATH; set the Tailscale bin folder")
    return p


class Device:
    """One GBOOST tailnet device. `state_dir` holds its keys; keep it owner-only."""

    def __init__(self, state_dir, hostname, bin_dir=None, log_path=None):
        self.state_dir = state_dir
        self.hostname = hostname
        self.tailscaled = find_bin("tailscaled", bin_dir)
        self.tailscale = find_bin("tailscale", bin_dir)
        self.log_path = log_path or os.path.join(state_dir, "tailscaled.log")
        self.proxy_port = None
        self.proc = None
        self.up_proc = None
        self.adopted = False
        os.makedirs(state_dir, exist_ok=True)
        if os.name != "nt":
            os.chmod(state_dir, 0o700)
        # Unix socket paths are limited to ~104 bytes; keep it short when the state path is long.
        sock = os.path.join(state_dir, "ts.sock")
        if os.name != "nt" and len(sock) > 100:
            base = os.environ.get("XDG_RUNTIME_DIR") or os.path.join(tempfile.gettempdir(), "gboost-%d" % os.getuid())
            os.makedirs(base, mode=0o700, exist_ok=True)
            os.chmod(base, 0o700)
            sock = os.path.join(base, "ts-%s.sock" % hostname)
        self.socket = sock if os.name != "nt" else r"\\.\pipe\gboost-ts-" + hostname

    def cli(self, *args, timeout=20, check=True):
        r = subprocess.run([self.tailscale, "--socket=" + self.socket, *args],
                           capture_output=True, text=True, timeout=timeout)
        if check and r.returncode != 0:
            raise RuntimeError("tailscale %s: %s" % (args[0], (r.stderr or r.stdout).strip()[-300:]))
        return r.stdout

    def start(self):
        """Start tailscaled with an HTTP proxy for outbound tailnet traffic. Call under a Lock."""
        if self.proc and self.proc.poll() is None:
            return
        port_file = os.path.join(self.state_dir, "proxy.port")
        try:
            # A device left running by an earlier GBOOST process (crash, restart): adopt it.
            self.status()
            self.proxy_port = int(open(port_file).read().strip())
            self.adopted = True
            return
        except (RuntimeError, OSError, ValueError, subprocess.TimeoutExpired):
            pass
        self.adopted = False
        self.proxy_port = free_port()
        with open(port_file, "w") as f:
            f.write(str(self.proxy_port))
        with open(self.log_path, "ab") as log:
            self.proc = subprocess.Popen(
                [self.tailscaled, "--tun=userspace-networking", "--statedir=" + self.state_dir,
                 "--socket=" + self.socket, "--port=0",
                 "--outbound-http-proxy-listen=127.0.0.1:%d" % self.proxy_port],
                stdout=log, stderr=log, stdin=subprocess.DEVNULL)
        for _ in range(100):
            if self.proc.poll() is not None:
                raise RuntimeError("tailscaled exited; see " + self.log_path)
            try:
                self.status()
                return
            except Exception:
                time.sleep(0.1)
        raise RuntimeError("tailscaled did not start; see " + self.log_path)

    def status(self):
        return json.loads(self.cli("status", "--json", timeout=10))

    def state(self):
        """(backend state, login URL or None, own DNS name or None)."""
        st = self.status()
        dns = (st.get("Self") or {}).get("DNSName") or ""
        return st.get("BackendState"), st.get("AuthURL") or None, dns.rstrip(".") or None

    def bring_up(self, wait_s=15):
        """Ask to join. Returns ("Running", dns), ("NeedsLogin", url) or (state, None) after wait_s."""
        backend, url, dns = self.state()
        if backend == "Running":
            return "Running", dns
        if self.up_proc is None or self.up_proc.poll() is not None:
            self.up_proc = subprocess.Popen(
                [self.tailscale, "--socket=" + self.socket, "up", "--hostname=" + self.hostname,
                 "--accept-dns=false", "--accept-routes=false"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
        end = time.time() + wait_s
        while time.time() < end:
            backend, url, dns = self.state()
            if backend == "Running":
                return "Running", dns
            if url:
                return "NeedsLogin", url
            time.sleep(0.3)
        return backend or "Unknown", url

    def serve_https(self, local_port, https_port=443):
        """Publish http://127.0.0.1:<local_port> on this device's own name, tailnet only.

        If the tailnet has HTTPS certificates or Serve turned off, `tailscale serve` prints a link to turn
        them on and waits; that link is raised so the caller can pass it to the owner."""
        try:
            self.cli("serve", "--bg", "--https=%d" % https_port, "http://127.0.0.1:%d" % local_port, timeout=30)
        except subprocess.TimeoutExpired as e:
            out = (e.stdout or b"") + (e.stderr or b"")
            out = out.decode("utf-8", "replace") if isinstance(out, bytes) else out
            links = [w for w in out.split() if w.startswith("https://login.tailscale.com/")]
            raise RuntimeError("ENABLE " + (links[0] if links else "HTTPS and Serve in the tailnet admin console"))

    def proxy_url(self):
        return "http://127.0.0.1:%d" % self.proxy_port

    def stop(self):
        for p in (self.up_proc, self.proc):
            if p and p.poll() is None:
                p.terminate()
                try:
                    p.wait(5)
                except subprocess.TimeoutExpired:
                    p.kill()
