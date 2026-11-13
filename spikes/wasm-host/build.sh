#!/bin/sh
# Builds the guest components into artifacts/. Needs: rustup targets wasm32-unknown-unknown, wasm-tools 1.263.0, npm ci in guest-ts.
set -e
cd "$(dirname "$0")"; mkdir -p artifacts
(cd guest-rust && cargo build --release --target wasm32-unknown-unknown --locked &&
  wasm-tools component new target/wasm32-unknown-unknown/release/guest_rust.wasm -o ../artifacts/rust.wasm)
J="npx jco componentize plugin.js --wit ../wit --world-name plugin"
(cd guest-ts && $J --disable all -o ../artifacts/ts-sm.wasm &&           # StarlingMonkey, no WASI
  $J -o ../artifacts/ts-sm-fullwasi.wasm &&                              # default: drags in WASI
  $J --backend qjs --backend-qjs-stub-wasi -o ../artifacts/ts-qjs.wasm)  # QuickJS, no WASI
