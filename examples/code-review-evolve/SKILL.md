---
name: code-review-evolve
contract: {"content_hash":"5c603a7cf565940555aeb82164adbcbb633f4587b40b0611a1bd929021b6030c","effects":["llm"],"input_shape":{
    "type": "structured",
    "fields": [
      {"name": "path", "shape": {"type": "string", "max_bytes": 256}},
      {"name": "diff", "shape": {"type": "string", "max_bytes": 65536}}
    ]
  },"name":"code-review-evolve","output_shape":{
    "type": "structured",
    "fields": [
      {"name": "findings", "shape": {
        "type": "list",
        "max_items": 8,
        "item": {
          "type": "structured",
          "fields": [
            {"name": "severity", "shape": {"type": "u8",     "max_bytes": 1}},
            {"name": "file",     "shape": {"type": "string", "max_bytes": 64}},
            {"name": "line",     "shape": {"type": "u32",    "max_bytes": 4}},
            {"name": "message",  "shape": {"type": "string", "max_bytes": 256}}
          ]
        }
      }}
    ]
  },"references":[],"version":"0.1.0"}
proof:    {"protocol":"hermes-lyra/0.3","output_cid":"bafkr4ibiicigxyop4ukv3gkg3d7skryr4e6za6yyy5gcrvfzkzjkakxoum","runtime":"hermes-lyra/0.3.0+uor-foundation/0.4.2"}
---

# code-review-evolve

*The lineage string.* A lyre passes from one player to the next only if it is still in tune. A skill passes from v0.1.0 to v0.1.1 only if the refinement holds.

A skill-audit loop runs on a cron and proposes its own mutations. Regulatory regimes (EU AI Act) want proof that v0.1.1 of a reviewer is a legitimate refinement of v0.1.0 — not a silent regression that drops a field a human reviewer was depending on.

`code-review-evolve` is the worked example.

## What it does

Takes a diff (path + unified-diff string) and returns a list of findings, each with severity, file, line, and a message. v0.1.1 adds a `category` field to each finding. The lineage receipt proves the addition is Liskov-safe.

## The lineage

| Version | Output fields | Change |
|---|---|---|
| **v0.1.0** ([`v0.1.0.lyra.json`](v0.1.0.lyra.json)) | `severity, file, line, message` | base |
| **v0.1.1** ([`v0.1.1.lyra.json`](v0.1.1.lyra.json)) | `severity, file, line, message, category` | adds `category` |

R4 (output narrows) requires child output ⊇ parent output. v0.1.1 satisfies parent's promise of `{severity, file, line, message}` and adds `category` on top. The refinement check passes.

## What would fail

A v0.2.0 that drops `severity` to save tokens — exactly the kind of "optimization" a self-improvement loop will try — is rejected. R4 fails because parent promised `severity` and child no longer delivers it. No `next_generation` receipt can be minted, so the audit trail visibly halts. The cron job's self-update notices and rolls back.

## How Hermes uses it

The audit cron does this:

```bash
# 1. Receipt for the parent (root of the lineage).
lyra score skill_interface_hash "$(cat v0.1.0.lyra.json)" v0.1.0-receipt.json

# 2. Base64-encode the parent receipt envelope (lineage takes it as a
#    sealed input, not as raw JSON, so the parent's binding is preserved).
PR_B64=$(base64 -w0 < v0.1.0-receipt.json)
CHILD=$(cat v0.1.1.lyra.json)

# 3. Mint the lineage receipt. Fails closed if R1–R5 don't hold.
lyra score next_generation \
  "{\"parent_receipt\":\"$PR_B64\",\"child_descriptor\":$CHILD}" \
  lineage.json
```

If this exits non-zero, the new version stays in staging. If it exits zero, the receipt is committed alongside the descriptor and any future auditor can replay the chain. See [`LINEAGE.md`](LINEAGE.md) for the verification walk-through.
