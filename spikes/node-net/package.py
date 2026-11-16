#!/usr/bin/env python3
"""Build one GBOOST release package (spike S11):

  package.py --version 1.2.3 --schema 1 --target linux-x86_64 --core <core bin> --release-tool <gboost-release bin>
             --pubkey <release.pub> --out <folder>

Writes <out>/gboost-<version>-<target>.tar.gz with: VERSION, SCHEMA, release.pub, bin/, node-net/.
"""
import argparse, io, os, tarfile

HERE = os.path.dirname(os.path.abspath(__file__))
PY = ["gboost_net.py", "gboost_node.py", "gboost_client.py", "gboost_update.py"]


def main():
    ap = argparse.ArgumentParser()
    for k in ("version", "schema", "target", "core", "release_tool", "pubkey", "out"):
        ap.add_argument("--" + k.replace("_", "-"), required=True)
    a = ap.parse_args()
    exe = ".exe" if a.target.startswith("windows") else ""
    top = "gboost-%s-%s" % (a.version, a.target)
    os.makedirs(a.out, exist_ok=True)
    out = os.path.join(a.out, top + ".tar.gz")

    def add_bytes(t, name, data, mode=0o644):
        info = tarfile.TarInfo(top + "/" + name)
        info.size, info.mode, info.mtime = len(data), mode, 0
        t.addfile(info, io.BytesIO(data))

    with tarfile.open(out, "w:gz") as t:
        add_bytes(t, "VERSION", (a.version + "\n").encode())
        add_bytes(t, "SCHEMA", (a.schema + "\n").encode())
        add_bytes(t, "release.pub", open(a.pubkey, "rb").read())
        add_bytes(t, "bin/gboost-s9-core" + exe, open(a.core, "rb").read(), 0o755)
        add_bytes(t, "bin/gboost-release" + exe, open(a.release_tool, "rb").read(), 0o755)
        for f in PY:
            add_bytes(t, "node-net/" + f, open(os.path.join(HERE, f), "rb").read(), 0o755)
    print(out)


if __name__ == "__main__":
    main()
