---
map: bastion-context
level: context
color_meaning: trust zone
links:
  - { from: w66a0k, to: pfbm19, label: approves }
  - { from: 3es1gd, to: 22d4x0, label: calls GBOOST via connector }
  - { from: 7kz9sv, to: iubdze, label: installs from }
  - { from: 22d4x0, to: 92oqnw, label: "one connector, active node" }
---
# GBOOST <!-- id:nruof5 -->
- Owner <!-- id:yukfhw color:4 -->
  - Phone approver <!-- id:w66a0k note:"Installed web app (PWA) on iOS or Android with passkey and Web Push. Served by the primary node; calls the standby when the primary is down (gboost-method.md 6.5-6.6)." -->
  - Desktop <!-- id:ae35p0 -->
  - GBOOST nodes <!-- id:92oqnw note:"State encrypted under a passkey-released key on every node. Any OS (Windows, macOS, Linux) or a VPS. Standby enters limited mode by itself when the primary drops; full takeover after one biometric approval; hands back when the primary returns (gboost-method.md 6.5-6.6)." -->
    - Primary (owner's computer) <!-- id:cnxk1n -->
    - Standby (VPS) <!-- id:802w34 -->
    - Signed updates <!-- id:0jpudu -->
- Grok Bot <!-- id:7kz9sv color:1 -->
  - Bot VM <!-- id:bu1k30 -->
  - Grok chat <!-- id:3es1gd -->
  - Routine webhook <!-- id:ii6d42 -->
- Cursor / xAI platform <!-- id:kv2q5j color:2 -->
  - Connector backend <!-- id:22d4x0 -->
  - Model API <!-- id:cdy1m1 -->
- Owner's services <!-- id:3nzvvl color:5 -->
  - Mail <!-- id:i339j0 -->
  - Calendar <!-- id:yphzcw -->
  - Notes vault <!-- id:xpzijq -->
- Plugins <!-- id:u9kx5t color:3 -->
  - First-party <!-- id:5i3swe -->
  - Community catalog <!-- id:iwticj -->
  - GitHub repo <!-- id:iubdze note:"INSTALL.md at the repo root. Owner gives Grok Bot the repo link; the Bot detects the OS, builds, starts the core, adds the gboost connector itself and asks the owner only for missing tools and secrets it cannot read (secure input). Works on Windows, macOS, Linux or a VPS (gboost-method.md 6.2)." -->
- Approval points <!-- id:t3ywi7 color:6 -->
  - Phone approval only <!-- id:pfbm19 -->
  - Chat cards never approve <!-- id:d9qeq5 -->
  - Audit log <!-- id:zzebqr -->
