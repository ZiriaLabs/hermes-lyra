# Lineage walk-through

This file shows what an auditor sees when they replay the v0.1.0 → v0.1.1 transition.

## The refinement check (R1–R5)

| Rule | What it says | v0.1.0 → v0.1.1 |
|---|---|---|
| R1 | `name` unchanged | both `code-review-evolve` ✓ |
| R2 | `version` strictly increases on `(major, minor, patch)` | `0.1.0 < 0.1.1` ✓ |
| R3 | child input ⊆ parent input | both `{path, diff}` — equal, so trivially a subset ✓ |
| R4 | child output ⊇ parent output | child adds `category` on top of `{severity, file, line, message}` ✓ |
| R5 | child effects ⊆ parent effects | both `["llm_call"]` ✓ |

Refinement holds. A `next_generation` receipt can be minted.

## What an auditor runs

```bash
# 1. Mint and replay the parent's interface receipt.
lyra score  skill_interface_hash "$(cat v0.1.0.lyra.json)" v0.1.0-receipt.json
lyra verify skill_interface_hash "$(cat v0.1.0.lyra.json)" v0.1.0-receipt.json

# 2. Mint the lineage receipt. The parent enters as a sealed base64 envelope
#    (its output_hash is folded in) and the child enters as inline JSON.
PR_B64=$(base64 -w0 < v0.1.0-receipt.json)
CHILD=$(cat v0.1.1.lyra.json)
lyra score next_generation \
  "{\"parent_receipt\":\"$PR_B64\",\"child_descriptor\":$CHILD}" \
  lineage.json

# 3. Replay the lineage. Byte-identical on any machine with the same runtime.
lyra verify next_generation \
  "{\"parent_receipt\":\"$PR_B64\",\"child_descriptor\":$CHILD}" \
  lineage.json
```

If any step exits non-zero, the chain is broken and the new version is not promoted.

## What a malicious "self-improvement" looks like

Suppose the cron job tries to ship a v0.2.0 that drops `severity`:

```json
"fields": [
  {"name": "file",    "shape": {"type": "string", "max_bytes": 1024}},
  {"name": "line",    "shape": {"type": "u32",    "max_bytes": 4}},
  {"name": "message", "shape": {"type": "string", "max_bytes": 4096}}
]
```

R4 fails: parent promised `severity`, child no longer delivers it. `next_generation` returns `RefinementError::OutputWidened` and refuses to mint a receipt. Self-modification is contained.

## What refinement does **not** prove

The receipt says the new version is **type-substitutable** for the old one. It does not say the new model gives better answers. Behavioral evaluation is a separate layer; v0.1 lineage receipts gate type drift, not semantic drift. See [`docs/specification.md`](../../docs/specification.md) for the precedence rules.
