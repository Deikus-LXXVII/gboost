# S1 findings: WASM plugin host (wasmtime component model)

Versions: rustc 1.96.1, wasmtime 49.0.5 (50.0.1 needs rustc 1.97), wit-bindgen 0.63.0, wasm-tools 1.263.0,
@bytecodealliance/jco 1.41.0, componentize-js 0.23.0, node 20.20.2. Cargo.lock in `host/` and `guest-rust/`,
package-lock.json in `guest-ts/`. Build guests with `./build.sh` (artifacts/ is git-ignored); test: `cd host && cargo test`.

## Answers
1. **Yes.** `host/src/lib.rs` links one function, `bastion:plugin/http.fetch`, through a typed Linker. The gate
   `origin_allowed` checks it against the TOML manifest (`[net] fetch = [origins]`) before the fake network is
   touched. Refused: other hosts, `host.evil.com`, `host@evil.com`, userinfo, uppercase, trailing dot, IP literals
   (v4, hex, v6), http, other ports, backslash/tab/control tricks (24 cases). Manifest origins must be a bare
   https origin (no wildcard, IP, path); unknown manifest keys are errors. Gate is deliberately stricter than
   WHATWG: non-canonical text (uppercase host) is refused even though the origin is semantically equal.
2. **Zero WASI is possible, for all three guests.** `OFFERED_WASI` is empty; no WASI is linked. Rust guest built
   as wasm32-unknown-unknown + `wasm-tools component new`: imports only our two interfaces. TS guests: see 4.
   The default jco output imports 19 WASI interfaces (io poll/error/streams, clocks, cli stdio+terminal-*,
   filesystem types/preopens, random, http types/outgoing-handler). The host refuses to load it (test) by
   checking component imports. Denying stubs were not needed (not built).
3. WIT `wit/plugin.wit`, package `bastion:plugin@0.1.0`: import `fetch`, export
   `prepare(tool, params) -> result<effect, string>`, `effect = mail-send | note-write`. Both pass `prepare` tests.
4. **Both build and run on the same host** (same tests, all three guests):

| Guest | Size | WASI imports | Compile+instantiate | Precompiled load | Load+instantiate+first call (first call not measured alone) |
|---|---|---|---|---|---|
| Rust (wit-bindgen) | 27 KB | 0 | 16 ms | 0.2 ms | 0.2 ms |
| TS, StarlingMonkey `--disable all` | 11.99 MB | 0 | 6.7 s | 5.5 ms | 6 ms |
| TS, QuickJS `--backend qjs --backend-qjs-stub-wasi` | 1.48 MB | 0 | 0.95 s | 1.0 ms | 1 ms |
| TS, default jco | 12.03 MB | 19 (above) | refused | | |

   Pain points: (a) default jco drags in 19 WASI interfaces; `--disable all` removes them (found by trying; not
   obvious from docs). (b) The default engine is 12 MB and takes ~7 s to JIT-compile (release-optimised
   cranelift; 85 s in an unoptimised debug build, so set `opt-level=3` for deps). Install-time precompile
   (`Engine::precompile_component`) fixes start-up: ms per start. (c) The QuickJS backend is smaller and faster
   to compile but its output uses `waitable.join`, so the host needs `wasm_component_model_async(true)`
   (a wasmtime feature flag, more attack surface in the host). Maturity unknown; it is the newer backend.
   (d) `result<_, string>` Err is a JS `throw`; Ok is a plain return; variants are `{tag, val}`. (e) `npm audit`
   shows 3 moderate findings in dev-tooling deps (build time only). (f) The Rust path needs wasm-tools as an
   extra step (wasip2 target not used; it would import wasi:cli etc.). (g) wasmtime needs a newish rustc.
5. Tests: 7 pass (`cargo test`): allowed origin plus typed effect (3 guests), gate tricks, guests cannot reach
   refused origins (fake network never called), unoffered WASI in manifest fails to load, WASI-importing
   component fails to load, bad manifests, timing. Break-check: with `return true;` at the top of
   `origin_allowed`, `gate_function_refuses_tricks` and `guests_cannot_reach_refused_origins` FAILED; restored, all pass.

## Recommendation for F9
TypeScript and Rust plugins are both viable with no ambient authority. Caveats: build with `--disable all` (or
QuickJS) and let the core reject any component with a non-Bastion import (CI check on the catalog);
precompile at install and cache; budget 12 MB per TS plugin with StarlingMonkey, or decide whether to accept
QuickJS and the async flag; pin the toolchain. Rust stays the lean first-party default. Not tested here: real
network, store/log/emit host functions, memory/fuel limits (add epoch interruption and store limits in Phase 1).

## Review notes for Phase 1 (reviewer, 2026-11-12)
- The gate must return the parsed `Url` and the host must fetch exactly that, so the HTTP client does not
  re-parse the raw text. Add tests for IDN/Unicode hosts, `%61` hosts, `https:host`, `:0443`, empty port.
- Real egress needs SSRF rules beyond the gate: refuse `.localhost/.local/.internal`, resolve and refuse
  private/CGNAT addresses, pin the resolved address (DNS rebinding), re-check every redirect.
- Offered-WASI matching must be exact name equality, not a prefix. Nothing is offered today; the linker
  having only Bastion functions is the real barrier, the import check is an earlier, clearer error.
- Enable `wasm_component_model_async` only if the QuickJS backend is chosen, and only for those plugins.
- "Guests cannot reach refused origins" holds by construction (no other imports); the test exercises the gate.

