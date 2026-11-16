#!/usr/bin/env python3
"""Sign and publish a GBOOST draft release (maintainer's computer only; spike S11, plan 6.7).

  python3 publish.py <tag> --repo <owner>/<repo> --key <release.key> [--tool <gboost-release>]
                    [--keychain <service> --yes]   (macOS: password from Keychain, no prompt)

CI built the packages and an unsigned manifest into a draft release. This script downloads them, checks
every hash and the version, shows what will be signed, asks for a yes, signs with the release key (the key
asks for its password), checks the signature against the pinned `release/release.pub`, uploads it and
publishes the release. Needs the GitHub CLI (`gh`) signed in.
"""
import argparse, json, os, shutil, subprocess, sys, tarfile, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(HERE))


def sh(*args, **kw):
    r = subprocess.run(list(args), **kw)
    if r.returncode != 0:
        sys.exit("error: %s failed" % args[0])
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tag")
    ap.add_argument("--repo", required=True)
    ap.add_argument("--key", required=True)
    ap.add_argument("--keychain", metavar="SERVICE", help="macOS: take the key password from this Keychain item instead of asking")
    ap.add_argument("--yes", action="store_true", help="do not ask before signing (owner's decision 2026-11-16: the agent may sign)")
    ap.add_argument("--export-clone", help="the public clone the tag was pushed from, if this repo is not that clone")
    ap.add_argument("--tool", default=os.path.join(HERE, "target", "release", "gboost-release" + (".exe" if os.name == "nt" else "")))
    a = ap.parse_args()
    pub = os.path.join(REPO_ROOT, "release", "release.pub")
    if not os.path.exists(a.tool):
        sys.exit("error: build the tool first: cargo build --release --manifest-path spikes/release/Cargo.toml")
    if not os.path.exists(pub):
        sys.exit("error: release/release.pub is missing")
    d = tempfile.mkdtemp(prefix="gboost-publish-")
    try:
        sh("gh", "release", "download", a.tag, "--repo", a.repo, "--dir", d)
        m = json.load(open(os.path.join(d, "manifest.json"), encoding="utf-8"))
        if "v" + m["version"] != a.tag:
            sys.exit("error: manifest version %s does not match tag %s" % (m["version"], a.tag))
        pkgs = sorted(f for f in os.listdir(d) if f.endswith(".tar.gz"))
        if sorted(m["files"]) != pkgs:
            sys.exit("error: the files in the release differ from the manifest")
        pinned = open(pub, "rb").read()
        want_schema = open(os.path.join(REPO_ROOT, "release", "SCHEMA")).read().strip()
        if str(m["schema"]) != want_schema:
            sys.exit("error: manifest schema %s differs from release/SCHEMA %s" % (m["schema"], want_schema))
        for f in pkgs:
            h = sh(a.tool, "hash", os.path.join(d, f), capture_output=True, text=True).stdout.strip()
            if h != m["files"][f]["sha256"]:
                sys.exit("error: hash mismatch for " + f)
            # Each package carries the key that will check the NEXT update: it must be the pinned one.
            with tarfile.open(os.path.join(d, f)) as t:
                top = f[: -len(".tar.gz")]
                def member(name):
                    x = t.extractfile(top + "/" + name)
                    if x is None:
                        sys.exit("error: %s has no %s" % (f, name))
                    return x.read()
                if member("release.pub") != pinned:
                    sys.exit("error: %s carries a different release key" % f)
                if member("VERSION").decode().strip() != m["version"] or member("SCHEMA").decode().strip() != want_schema:
                    sys.exit("error: %s has a wrong VERSION or SCHEMA" % f)
        # The tag must point at the commit the maintainer has locally (and has reviewed).
        remote = sh("gh", "api", "repos/%s/commits/%s" % (a.repo, a.tag), "--jq", ".sha", capture_output=True, text=True).stdout.strip()
        lr = subprocess.run(["git", "-C", REPO_ROOT, "rev-parse", "--verify", "-q", a.tag + "^{commit}"], capture_output=True, text=True)
        local = lr.stdout.strip() if lr.returncode == 0 else ""
        print("Tag %s is commit %s on GitHub; your local %s is %s." % (a.tag, remote, a.tag, local or "missing"))
        if local != remote and not a.export_clone:
            sys.exit("error: the tag on GitHub is not the commit you have locally; review it first (or pass --export-clone <dir>)")
        if a.export_clone:
            cr = subprocess.run(["git", "-C", a.export_clone, "rev-parse", "--verify", "-q", a.tag + "^{commit}"], capture_output=True, text=True)
            clone = cr.stdout.strip() if cr.returncode == 0 else ""
            if clone != remote:
                sys.exit("error: the tag in %s (%s) is not the GitHub tag" % (a.export_clone, clone or "missing"))
        print("Release %s (schema %s, lowest client %s):" % (m["version"], m["schema"], m["min_client"]))
        for f in pkgs:
            print("  %s  %s" % (m["files"][f]["sha256"][:16], f))
        if not a.yes and input("Sign and publish? Type yes: ").strip() != "yes":
            sys.exit("stopped, nothing signed")
        sign = [a.tool, "sign", os.path.join(d, "manifest.json"), os.path.expanduser(a.key)]
        if a.keychain:
            pw = subprocess.run(["security", "find-generic-password", "-s", a.keychain, "-w"], capture_output=True)
            if pw.returncode != 0:
                sys.exit("error: no Keychain item %r (add it: security add-generic-password -s %s -a gboost -w)" % (a.keychain, a.keychain))
            sh(*sign, "--password-stdin", input=pw.stdout, stdout=subprocess.DEVNULL)
            del pw
        else:
            sh(*sign)
        sh(a.tool, "verify", os.path.join(d, "manifest.json"), pub)
        sh("gh", "release", "upload", a.tag, os.path.join(d, "manifest.json.minisig"), "--repo", a.repo, "--clobber")
        sh("gh", "release", "edit", a.tag, "--repo", a.repo, "--draft=false", "--notes", "Signed release %s." % m["version"])
        print("PUBLISHED %s" % a.tag)
    finally:
        shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    main()
