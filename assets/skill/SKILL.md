---
name: lyra
description: "hermes-lyra: typed self-verifying skill contracts over MCP."
version: 0.4.0
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [lyra, mcp, skills, contracts, verify, refine, compose]
    related_skills: [hermes-agent]
prerequisites:
  commands: [lyra]
---

# /lyra — hermes-lyra command surface

`hermes-lyra` is a typed, self-verifying skill contract layer over MCP.
It turns a SKILL.md into a cryptographically verifiable contract
(BLAKE3 + IPFS-style CIDs) and provides five composability gates:
`bind`, `verify`, `refine`, `compose`, `merge`.

When the user types `/lyra`, render the full panel below and ask what
they want to do. Be concise — the panel is self-explanatory.

## Welcome panel

```
╭──────────────────────────────────────────────────────── hermes-lyra v0.4.0 ──╮
│     .:.     .:.      Subcommands                                             │
│   .:=+=:. .:=+=:.    gates: bind, verify, refine, compose, merge             │
│     :+*#*=*#*+:      lint: lint, lint --strict                               │
│      .:+*#*+:.       address: cid, publish                                   │
│        .===.         tooling: mcp serve, self-check, demo refine             │
│       +: : :+        setup: setup, install, install --uninstall              │
│        :=+=:         codec: b64-encode, b64-decode                           │
│       *:   :*                                                                │
│        :=+=:         MCP Tools                                               │
│       +: : :+        contract: skill_bind, skill_verify                      │
│        :=+=:         evolve: skill_refine, skill_compose, skill_merge        │
│       *:   :*        lint: skill_lint                                        │
│        .===.                                                                 │
│         :::          ◇ lyra setup  →  registers MCP in ~/.hermes/config.yaml │
│                      ◇ docs: github.com/ZiriaLabs/hermes-lyra                │
╰──────────────────────────────────────────────────────────────────────────────╯
```

## Full option reference

### CLI subcommands — `lyra <cmd>`

**gates** (the five typed-contract operations)
- `lyra bind <SKILL.md> <descriptor.json>` — embed contract + proof into a SKILL.md
- `lyra bind <descriptor.json>` — scaffold a fresh SKILL.md from a descriptor
- `lyra verify <SKILL.md>` — re-derive the proof; status=valid | mismatch | no_proof
- `lyra refine <parent> <child>` — Liskov refinement gate (R1–R5); status=promote | rollback
- `lyra compose <skill_a> <skill_b> [...]` — type-check a pipeline; status=compatible | incompatible
- `lyra merge <producer> <consumer>` — atomic fusion into a single skill

**lint** (structural sanity for SKILL.md)
- `lyra lint <SKILL.md>` — tier-0 rules only (no false positives on audited corpus)
- `lyra lint --strict <SKILL.md>` — also runs advisory Hermes-convention rules

**address** (content addressing)
- `lyra cid <SKILL.md>` — emit the envelope CIDv1+raw+blake3
- `lyra publish <SKILL.md>` — print a publishable receipt (CID + descriptor)

**tooling** (introspection)
- `lyra mcp serve` — run as MCP stdio server (the Hermes config points at this)
- `lyra self-check` — 7-case proof-of-life suite (must be 7/7 PASS)
- `lyra demo refine` — narrated walkthrough of the refinement gate

**setup**
- `lyra setup` — register this binary as `mcp_servers.lyra` in ~/.hermes/config.yaml (idempotent)
- `lyra install` — same as setup, alternate name
- `lyra install --uninstall` — remove the MCP registration

**codec** (envelope/proof debugging helpers)
- `lyra b64-encode` — base64 encode stdin
- `lyra b64-decode` — base64 decode stdin

### MCP tools — exposed to any Hermes agent

These are what an LLM agent calls directly via the MCP bridge once
`lyra setup` has run (no shell required):

- `skill_bind` — bind a descriptor + proof into a SKILL.md
- `skill_verify` — re-derive a SKILL.md's proof against its descriptor
- `skill_refine` — Liskov refinement check (parent → child)
- `skill_compose` — pipeline type-check (producer → consumer chain)
- `skill_merge` — atomic categorical fusion (producer + consumer → fused skill)
- `skill_lint` — structural lint (tier-0 or strict)

### How to verify a skill end-to-end

```bash
# 1. Bind a contract
lyra bind ./my-skill/SKILL.md ./my-skill/descriptor.json

# 2. Verify the proof re-derives
lyra verify ./my-skill/SKILL.md   # → status=valid

# 3. Address it (envelope CIDv1+raw+blake3)
lyra cid ./my-skill/SKILL.md      # → bafkr4i...
```

### Where things live

- Binary: `~/.local/bin/lyra`
- Source: github.com/ZiriaLabs/hermes-lyra (MIT licensed)
- MCP registration: `~/.hermes/config.yaml` → `mcp_servers.lyra`
- Spec & changelog: `docs/specification.md`, `docs/CHANGELOG.md`
- Runtime ID: `hermes-lyra/0.3.0+uor-foundation/0.4.2`

## Response style

When the user invokes `/lyra` with no further question, print the
welcome panel (copy it verbatim from above) and offer one of:
- "Run a quick demo? (`lyra self-check` or `lyra demo refine`)"
- "Verify a skill you've got? (paste the path)"
- "Bind a contract to a new SKILL.md? (need: descriptor.json)"

When they ask a specific question (e.g. "what does refine do?"),
answer directly from the option reference above — don't re-print
the full panel unless they ask for it.
