# Windows: GBOOST's own userspace Tailscale node (research, 2026-11-17, Grok; from Tailscale v1.104.7 source)

Verdict: possible without admin rights and without the Tailscale service, from v1.104.0 on. Not yet run
on Windows; the next step is a CI job on a Windows runner.

- `tailscaled.exe --tun=userspace-networking --windows-mode=dev --statedir=<dir> --socket=\\.\pipe\<name>
  --port=0 --outbound-http-proxy-listen=127.0.0.1:<port>`. Without `--windows-mode=dev` the default pipe is
  under `ProtectedPrefix\Administrators` (admin only). Without `--port=0` it takes UDP 41641 and clashes
  with an installed Tailscale service. Without `--statedir` it would use the system service's state.
- The CLI needs `--socket=\\.\pipe\<name>` on every call while a system service runs (no `TS_SOCKET`).
- The HTTP and SOCKS5 proxies work in userspace mode on Windows (one report: split-DNS names failed in the
  proxy; GBOOST uses MagicDNS names of peers, to check).
- `tailscale serve` terminates HTTPS inside netstack, so port 443 needs no admin; no live report found.
- Binaries: only installers are published (MSI/EXE, `.sha256` next to each; MSI Authenticode "Tailscale
  Inc."). No zip. Options: extract the MSI without installing (to verify), or build from source with Go,
  or embed with tsnet.

Sources: tailscale/tailscale v1.104.7 cmd/tailscaled/tailscaled.go (applyWindowsMode, defaultPort,
tryEngine), safesocket/pipe_windows.go, paths/paths.go, cmd/tailscale/cli/cli.go, client/local/local.go,
cmd/tailscaled/proxy.go, ipn/ipnlocal/serve.go; pkgs.tailscale.com/stable; issues 2791, 4677, 8422.
