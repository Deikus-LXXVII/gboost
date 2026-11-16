#!/usr/bin/env bash
# End-to-end for the Remote MCP connector: core + fake routine receiver + curl-like MCP host.
set -euo pipefail
cd "$(dirname "$0")/.."
(cd core && cargo build --quiet)
exec python3 -I scripts/e2e_remote.py "$PWD"
