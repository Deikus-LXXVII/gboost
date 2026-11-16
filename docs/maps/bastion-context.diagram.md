---
generated_from: bastion-context.md
# Generated file. Do not edit: changes are overwritten on the next run.
---
```mermaid
mindmap
  nruof5(("GBOOST"))
    yukfhw["Owner"]
      w66a0k["Phone approver"]
      ae35p0["Desktop"]
      92oqnw["GBOOST nodes"]
        cnxk1n["Primary (owner's computer)"]
        802w34["Standby (VPS)"]
        0jpudu["Signed updates"]
    7kz9sv["Grok Bot"]
      bu1k30["Bot VM"]
      3es1gd["Grok chat"]
      ii6d42["Routine webhook"]
    kv2q5j["Cursor / xAI platform"]
      22d4x0["Connector backend"]
      cdy1m1["Model API"]
    3nzvvl["Owner's services"]
      i339j0["Mail"]
      yphzcw["Calendar"]
      xpzijq["Notes vault"]
    u9kx5t["Plugins"]
      5i3swe["First-party"]
      iwticj["Community catalog"]
      iubdze["GitHub repo"]
    t3ywi7["Approval points"]
      pfbm19["Phone approval only"]
      d9qeq5["Chat cards never approve"]
      zzebqr["Audit log"]
```

## Links

- Phone approver → Phone approval only: approves
- Grok chat → Connector backend: calls GBOOST via connector
- Grok Bot → GitHub repo: installs from
- Connector backend → GBOOST nodes: one connector, active node
