---
map: bastion-context
level: context
color_meaning: trust zone
links:
  - { from: own001, to: gat001, label: approves }
  - { from: agt001, to: ext001, label: "calls, read-only" }
---
# Bastion <!-- id:root01 -->
- Owner <!-- id:own000 color:1 -->
  - Phone <!-- id:own001 -->
  - Desktop <!-- id:own002 -->
- Agents <!-- id:agt000 color:2 note:"docs/notes/agents.md" -->
  - Claude Code <!-- id:agt001 -->
  - Grok sister <!-- id:agt002 -->
- Gateway <!-- id:gat000 color:3 -->
  - Approval queue <!-- id:gat001 -->
- Vault <!-- id:vlt000 color:4 -->
  - Obsidian maps <!-- id:vlt001 -->
- External services <!-- id:ext000 color:5 -->
  - Model APIs <!-- id:ext001 note:"say \"hi\" --\u003e \u003cb\u003e" -->
- Audit log <!-- id:aud000 color:6 -->
