#!/usr/bin/env bash
# End-to-end: core + fake routine receiver + the client driven over stdio like an MCP host.
set -euo pipefail
cd "$(dirname "$0")/.."
(cd core && cargo build --quiet)
python3 -I scripts/test_client.py "$PWD"
exec python3 -I scripts/e2e.py "$PWD"
