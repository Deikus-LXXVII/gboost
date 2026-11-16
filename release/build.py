#!/usr/bin/env python3
"""Build, rehearse and package a GBOOST release on the maintainer's computer (plan 6.7 item 10).

  python3 release/build.py <version> [--targets linux-x86_64,macos-arm64,...] [--linux-test-host <ssh host>]

Run it from a clean checkout of the tag v<version> (it refuses uncommitted changes or another commit), so
the packages are exactly the tagged code; the commit is written to dist/<version>/BUILD_COMMIT and
publish.py checks it against the tag on GitHub. Builds the core and the release tool for each target: the host's own target natively, Linux targets from
macOS with cargo-zigbuild. Runs the update and rollback rehearsals (node and client, spikes/node-net/test_*update.py) on this
computer, and for Linux binaries this computer cannot run, on --linux-test-host over ssh (a fresh temporary
folder there, removed afterwards). Then packages every target and writes the manifest into
dist/<version>/. Sign and publish that folder with spikes/release/publish.py --local dist/<version>.
Default targets: the host's own, plus linux-x86_64 and linux-arm64 when cargo-zigbuild is installed.
"""
import argparse, os, platform, re, shlex, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATES = {"core": ("spikes/bot-client/core", "gboost-s9-core"), "tool": ("spikes/release", "gboost-release")}
TRIPLES = {"linux-x86_64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu",
           "macos-arm64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin",
           "windows-x86_64": "x86_64-pc-windows-msvc", "windows-arm64": "aarch64-pc-windows-msvc"}
REHEARSALS = ("test_update.py", "test_client_update.py")  # node update/rollback, client self-update
CPU = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x86_64", "amd64": "x86_64"}


def die(msg):
    sys.exit("error: " + msg)


def run(*args, **kw):
    print("+ " + " ".join(args), flush=True)
    if subprocess.run(list(args), **kw).returncode != 0:
        die("%s failed" % args[0])


def host_target():
    osname = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system())
    cpu = CPU.get(platform.machine().lower())
    if not osname or not cpu:
        die("unknown host %s %s" % (platform.system(), platform.machine()))
    return "%s-%s" % (osname, cpu)


def binary(target, crate):
    path, name = CRATES[crate]
    return os.path.join(ROOT, path, "target", TRIPLES[target], "release", name + (".exe" if target.startswith("windows") else ""))


def build(target, host):
    triple = TRIPLES[target]
    for crate in CRATES.values():
        manifest = os.path.join(ROOT, crate[0], "Cargo.toml")
        if target == host:
            run("cargo", "build", "--release", "--locked", "--manifest-path", manifest, "--target", triple)
        elif target.startswith("linux") and shutil.which("cargo-zigbuild"):
            run("rustup", "target", "add", triple, stdout=subprocess.DEVNULL)
            # .2.17: link against an old glibc so the binary runs on any current Linux.
            run("cargo", "zigbuild", "--release", "--locked", "--manifest-path", manifest, "--target", triple + ".2.17")
        else:
            die("cannot build %s here (Linux targets need cargo-zigbuild: cargo install --locked cargo-zigbuild, and zig)" % target)


def rehearse_local(target):
    for test in REHEARSALS:
        run(sys.executable, "-I", os.path.join(ROOT, "spikes/node-net", test), binary(target, "core"), binary(target, "tool"))


def remote(host, cmd):
    r = subprocess.run(["ssh", "-o", "BatchMode=yes", host, cmd], capture_output=True, text=True)
    lines = r.stdout.strip().splitlines()
    if r.returncode != 0 or not lines:
        die("ssh %s %r: %s" % (host, cmd, r.stderr.strip()))
    return lines[-1].strip()  # the last line: a login banner may come first


def rehearse_remote(target, host):
    m = re.fullmatch(r"Linux (\S+)", remote(host, "uname -sm"))
    if not m or "linux-" + CPU.get(m.group(1).lower(), m.group(1)) != target:
        return False
    tmp = remote(host, "mktemp -d")
    if not re.fullmatch(r"/[\w./-]+", tmp) or ".." in tmp:
        die("unexpected temporary folder from %s: %r" % (host, tmp))
    q = shlex.quote(tmp)
    try:
        run("scp", "-q", binary(target, "core"), binary(target, "tool"), host + ":" + tmp + "/")
        run("scp", "-q", "-r", os.path.join(ROOT, "spikes/node-net"), host + ":" + tmp + "/")
        for test in REHEARSALS:
            run("ssh", "-o", "BatchMode=yes", host, "cd %s && python3 -I node-net/%s ./gboost-s9-core ./gboost-release" % (q, test))
    finally:
        subprocess.run(["ssh", "-o", "BatchMode=yes", host, "rm -rf -- " + q])
    return True


def git(*args):
    return subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True).stdout.strip()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("version")
    ap.add_argument("--targets", help="comma-separated, from: " + ", ".join(TRIPLES))
    ap.add_argument("--linux-test-host", help="ssh host that runs the rehearsal for Linux binaries built here")
    ap.add_argument("--skip-rehearsal", action="store_true", help="package without the rehearsal (not for releases)")
    a = ap.parse_args()
    host = host_target()
    if a.targets:
        targets = a.targets.split(",")
    else:
        targets = [host] + [t for t in ("linux-x86_64", "linux-arm64") if t != host and shutil.which("cargo-zigbuild")]
    for t in targets:
        if t not in TRIPLES:
            die("unknown target " + t)
    if not re.fullmatch(r"\d{1,6}\.\d{1,6}\.\d{1,6}", a.version):
        die("version must look like 1.2.3")
    if a.linux_test_host and a.linux_test_host.startswith("-"):
        die("bad ssh host")
    if git("status", "--porcelain", "--untracked-files=no"):
        die("uncommitted changes; build from a clean checkout of the tag")
    commit = git("rev-parse", "HEAD")
    tag_commit = git("rev-parse", "--verify", "-q", "v%s^{commit}" % a.version)
    if not commit or commit != tag_commit:
        die("HEAD is not the tag v%s; tag the commit first and build from it" % a.version)
    pub = os.path.join(ROOT, "release", "release.pub")
    schema = open(os.path.join(ROOT, "release", "SCHEMA")).read().strip()
    min_client = (open(os.path.join(ROOT, "release", "MIN_CLIENT")).read().strip()
                  if os.path.exists(os.path.join(ROOT, "release", "MIN_CLIENT")) else "0.1.0")
    if not os.path.exists(pub):
        die("release/release.pub is missing: create the release key first")
    out = os.path.join(ROOT, "dist", a.version)
    if os.path.exists(out):
        die(out + " exists; remove it or choose another version")

    for t in dict.fromkeys(targets + [host]):  # the host's own tool writes the manifest
        build(t, host)
    untested = []
    if not a.skip_rehearsal:
        for t in targets:
            if t == host:
                rehearse_local(t)
            elif not (a.linux_test_host and t.startswith("linux") and rehearse_remote(t, a.linux_test_host)):
                untested.append(t)

    os.makedirs(os.path.dirname(out), exist_ok=True)
    work = tempfile.mkdtemp(prefix=".%s-" % a.version, dir=os.path.dirname(out))
    try:
        for t in targets:
            run(sys.executable, "-I", os.path.join(ROOT, "spikes/node-net/package.py"), "--version", a.version, "--schema", schema,
                "--target", t, "--core", binary(t, "core"), "--release-tool", binary(t, "tool"), "--pubkey", pub, "--out", work)
        run(binary(host, "tool"), "manifest", work, a.version, schema, min_client)
        open(os.path.join(work, "BUILD_COMMIT"), "w").write(commit + "\n")
        os.rename(work, out)  # dist/<version> appears only when complete
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print("BUILT %s: %s" % (a.version, ", ".join(targets)))
    if a.skip_rehearsal:
        print("NOT REHEARSED (--skip-rehearsal)")
    elif untested:
        print("NOT REHEARSED on a real machine: " + ", ".join(untested))
    print("Next: python3 spikes/release/publish.py v%s --local %s --tool %s --repo <owner>/<repo> --key <release.key>"
          % (a.version, out, binary(host, "tool")))


if __name__ == "__main__":
    main()
