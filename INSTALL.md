# Installing GBOOST (instructions for Grok Bot)

The owner gives you, the Bot, a link to this repository and asks you to install GBOOST. You do the whole
install yourself. You stop and ask the owner only at the points marked **Ask the owner**, or when a step fails
and the "If it fails" note says to.

> **Stage.** GBOOST is in Phase 0 (spikes). This installs the **S9 core**, the first test of how a Grok Bot
> reaches GBOOST: a Remote MCP connector with a client key, plus a session that opens only after the owner's
> routine webhook hands back a code. It is a test install. It has no signed releases and no auto-start yet.

## Rules while installing

- Never print, paste or log a secret: the console token, the client key, or the webhook URL and key. Secrets go
  into files that only the owner's account can read. You refer to them by file path.
- Do not edit the code. If a build or test fails, report it; do not patch it.
- Do not change the owner's network rules (Tailscale ACLs, firewall) and do not turn on public exposure such as
  Tailscale Funnel or port forwarding unless the owner says so.
- Work on the computer the owner names. If they name none, use the one you are controlling now.
- Any OS works: Windows, macOS, Linux, a home computer or a VPS. Use the commands for that OS below.

## 1. Look at the computer

Find the OS and the CPU, and check the tools:

| | macOS, Linux | Windows (PowerShell) |
|---|---|---|
| OS, CPU | `uname -sm` | `[Environment]::OSVersion; $env:PROCESSOR_ARCHITECTURE` |
| Tools | `git --version; cargo --version; python3 --version; tailscale version` | `git --version; cargo --version; python --version; tailscale version` |

Needed: `git` and Rust (current stable `cargo`). Optional: Python 3.9+ (for the self-test) and Tailscale (one way
to give Grok an address, see step 6).

**Ask the owner** before installing missing tools. Name each one and the official installer you will use:
Rust from https://rustup.rs, git from the OS package manager or https://git-scm.com. On Windows, Rust needs
the Visual Studio C++ Build Tools; rustup offers to install them.

## 2. Choose the folders

| | macOS, Linux | Windows |
|---|---|---|
| Code | `~/gboost` | `%USERPROFILE%\gboost` |
| State (secrets, logs) | `~/.gboost/s9` | `%USERPROFILE%\.gboost\s9` |

The state folder must be inside the owner's home folder. The core makes its files readable by the owner only:
on macOS and Linux with file modes, on Windows through the home folder's default permissions. Create it now:

| | Command |
|---|---|
| macOS, Linux | `mkdir -p ~/.gboost && mkdir -p -m 700 ~/.gboost/s9` |
| Windows | `New-Item -ItemType Directory -Force "$env:USERPROFILE\.gboost\s9"` |

## 3. Get the code and build it

macOS, Linux:

    git clone https://github.com/Deikus-LXXVII/gboost ~/gboost      # or: git -C ~/gboost pull
    cd ~/gboost/spikes/bot-client/core
    cargo test --locked
    cargo build --release --locked

Windows (PowerShell):

    git clone https://github.com/Deikus-LXXVII/gboost "$env:USERPROFILE\gboost"      # or: git -C "$env:USERPROFILE\gboost" pull
    Set-Location "$env:USERPROFILE\gboost\spikes\bot-client\core"
    cargo test --locked
    cargo build --release --locked

The program is `target/release/gboost-s9-core` (`gboost-s9-core.exe` on Windows).

If Python is present, also run the end-to-end self-test. It starts its own core on free ports with a fake
routine, so it does not touch the real install:

    cd ~/gboost/spikes/bot-client && python3 -I scripts/e2e_remote.py "$PWD"     # Windows: python

It must end with `RESULT: ALL PASS`.

**If it fails:** send the owner the OS, the command, and the last 30 lines of output. Stop.

## 4. Pick two local ports

The core has two listeners, both on this computer only:
- the agent listener, which Grok reaches through step 6;
- the owner listener, for the owner's console. It is always loopback-only.

Use `127.0.0.1:8790` and `127.0.0.1:8791` if they are free. If either is taken, pick free ports instead: try
8790–8799, then 18790–18799. Do not stop or change whatever program holds a port.

Check a port:

| | Command |
|---|---|
| macOS | `lsof -nP -iTCP:<port> -sTCP:LISTEN` |
| Linux | `ss -ltn "sport = :<port>"` |
| Windows | `Get-NetTCPConnection -State Listen -LocalPort <port>` |

Write down the two ports you chose; later steps call them `<agent port>` and `<owner port>`.

## 5. Connect the owner's routine

GBOOST opens a session for the Bot only after it sends a one-time code to the owner's Grok routine webhook, and
the routine hands the code back. This proves the caller is the owner's own Bot.

1. Create a Grok routine with a webhook trigger, if you can do that yourself. Otherwise **ask the owner** to
   create it. Its instruction text:

   > When woken by GBOOST with kind `session_check`, call the gboost connector's tool `gboost_deliver_code`
   > with the code from the payload, unchanged.

2. **Ask the owner** for the routine's webhook URL and key through Grok's secure input field, never in chat.
3. Write them to `routine.toml` in the state folder with your file-writing tool, not with a shell command
   (a shell command would leave the key in the shell history). On macOS and Linux, run `chmod 600` on the
   file afterwards; the state folder is already closed to other users (step 2).

       url = "https://..."
       key = "..."

   The URL must be `https`.

## 6. Start the core

macOS, Linux:

    cd ~/gboost/spikes/bot-client/core
    S9_STATE_DIR=~/.gboost/s9 S9_AGENT_BIND=127.0.0.1:<agent port> S9_OWNER_BIND=127.0.0.1:<owner port> \
      nohup ./target/release/gboost-s9-core > ~/.gboost/s9/core.out 2>&1 &

Windows (PowerShell):

    $env:S9_STATE_DIR="$env:USERPROFILE\.gboost\s9"; $env:S9_AGENT_BIND="127.0.0.1:<agent port>"; $env:S9_OWNER_BIND="127.0.0.1:<owner port>"
    Start-Process -WindowStyle Hidden -FilePath "$env:USERPROFILE\gboost\spikes\bot-client\core\target\release\gboost-s9-core.exe" `
      -RedirectStandardOutput "$env:S9_STATE_DIR\core.out" -RedirectStandardError "$env:S9_STATE_DIR\core.err"

Check: `core.out` shows `Agent listener: http://127.0.0.1:<agent port>`, and there is no `NOTE` line about
`routine.toml`. The core keeps running after your session ends. It does not start again after a reboot yet.

## 7. Give Grok an HTTPS address

Grok's Remote connector needs an HTTPS URL that Grok can reach.

**With Tailscale**, when the owner's tailnet is connected to Grok:
1. Run `tailscale serve status` and pick an HTTPS port it does not use yet. Try 8790, then 8791–8799.
2. Publish the agent listener on the tailnet only (not Funnel):

       tailscale serve --bg --https=<https port> http://127.0.0.1:<agent port>

3. The URL is `https://<this computer's MagicDNS name>:<https port>/mcp`. Get the name from
   `tailscale status --json`: it is `Self.DNSName` without the trailing dot.

**Without Tailscale:** **ask the owner** how Grok should reach this computer, for example through a reverse
proxy with HTTPS that they already run. Do not open public ports on your own.

## 8. Make the client key

The client key goes to a file, not to the screen.

The console token must not appear in a command line, where other users of the computer could see it in the
process list. These commands read it inside the shell:

macOS, Linux (bash or zsh):

    curl -s -XPOST -H @<(printf 'x-console-token: %s' "$(cat ~/.gboost/s9/console.token)") \
      "http://127.0.0.1:<owner port>/local/clients/new?to_file=1"

Windows (PowerShell):

    Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:<owner port>/local/clients/new?to_file=1" `
      -Headers @{ "x-console-token" = (Get-Content -Raw "$env:USERPROFILE\.gboost\s9\console.token").Trim() }

The answer shows the client id, its fingerprint and the file path. The key is in `new-client.txt` in the state
folder.

## 9. Owner adds the connector

**Ask the owner** to do the following in Grok's connector settings, and give them the URL from step 7 and the
file path from step 8:

1. Add a custom connector: type **Remote HTTPS**, name `gboost`, the URL from step 7.
2. Add the header `x-gboost-client`. Its value is the full content of `new-client.txt`. No OAuth.
3. Turn the `gboost` connector on for the routine from step 5 too.

Once the owner confirms, delete `new-client.txt`.

## 10. Check that it works

1. Call `gboost_status`. It needs only the key and should answer.
2. Call `gboost_hello`. The first call answers that a session check was sent. The routine runs and delivers
   the code.
3. Wait about half a minute and call `gboost_hello` again. It should greet the owner. Then call `gboost_whoami`
   and `gboost_pending_request`.
4. Read `requests.log` and `security.log` in the state folder. You should see the check, the code delivered,
   and your calls, and no errors.

"GBOOST could not reach your routine webhook" means the URL or key in `routine.toml` is wrong, or the routine
is off. **If it fails:** send the owner the step, the tool answer, and the last 20 lines of both logs. These logs
hold no secrets. Stop.

## 11. Report to the owner

Tell the owner:
- the OS;
- the code and state folders;
- both local ports and the HTTPS URL;
- the client id and fingerprint;
- the check results;
- how to stop GBOOST (below).

## Stop or remove

1. Stop the process: `pkill -f gboost-s9-core` on macOS and Linux, `Stop-Process -Name gboost-s9-core` on
   Windows.
2. Remove the address: `tailscale serve --https=<https port> off`.
3. To remove everything, delete the code folder and the state folder, then remove the `gboost` connector in
   Grok.
