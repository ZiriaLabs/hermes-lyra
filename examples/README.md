# hermes-lyra examples

Three reference skill packs, each demonstrating one property of the contract layer.

## Mounted as MCP

`hermes-lyra` runs as a Model Context Protocol server over stdio. One binary, five tools.

```bash
cargo install --path ..
```

```
~/.hermes/mcp.json

{ "mcpServers": {
    "lyra": { "command": "lyra", "args": ["mcp", "serve"] }
} }
```

The canonical MCP config template lives at the repo root: [`../mcp.json`](../mcp.json). Works in Claude Desktop, Cursor, and any other MCP client.

## Available tools

```
skill_bind:    embed a typed contract + self-verifying proof into SKILL.md frontmatter
skill_verify:  re-derive the embedded proof locally (no network)
skill_refine:  R1–R5 refinement check between parent and child skills
skill_compose: composition check (pair or chain)
skill_merge:   atomic merge of a producer + consumer into one self-contained skill
```

Receipts produced through the MCP server are byte-identical to what the CLI produces. Same input + same substrate → same bytes on every machine.

## SKILL.md format (v0.2)

A skill carries its contract and proof directly in YAML frontmatter:

```
---
name: my-skill
description: ...
version: "0.1.0"
contract: {"name":"my-skill","version":"0.1.0","input_shape":{...},
           "output_shape":{...},"effects":["llm"],"references":[],
           "content_hash":"..."}
proof:    {"protocol":"hermes-lyra/0.2","output_hash":"...",
           "runtime":"hermes-lyra/0.2.0+uor-foundation/0.4.2",
           "spec_uri":"..."}
---

# my-skill
Prose Hermes reads as before.
```

Standard YAML frontmatter, standard markdown body. The `contract:` and `proof:` keys are valid YAML (inline JSON values), readable by any YAML parser, ignored by anything that doesn't care about them. Single file, both layers.

### The bridge in commands

```bash
lyra bind    SKILL.md descriptor.json    # embed contract + proof into frontmatter
lyra verify  SKILL.md                    # re-derive embedded proof
lyra refine  parent.md child.md          # gates auto-detect .md vs .json
lyra compose s1.md s2.md [s3.md ...]
lyra merge   producer.md consumer.md
```

The high-level gates accept either a SKILL.md path or a descriptor JSON transparently; the bridge routes correctly.

## The three packs

```
inbox-triage          a typed pipe a downstream consumer can trust
news-brief            pinned <name>@<hash> refs; reruns are reproducible
code-review-evolve    self-improvement that has to prove itself
```

| Pack | What it locks down | Mechanism |
|---|---|---|
| [`inbox-triage`](inbox-triage/) | Cron summarizers drift when the model is swapped. | A typed output shape. The downstream consumer fails closed at compose time, not in production. |
| [`news-brief`](news-brief/) | Multi-channel digests reshape when a sub-skill quietly upgrades. | Pinned `<name>@<hash>` references. Upgrades are visible Git diffs, not silent. |
| [`code-review-evolve`](code-review-evolve/) | Self-improving skills need proof, not vibes. | Liskov-substitutable lineage receipts. R1–R5 hold or the new version stays in staging. |

## Demo

One command, cross-platform, no shell required:

```
$ lyra demo refine
```

Or the equivalent shell script: [`code-review-evolve/tripwire.sh`](code-review-evolve/tripwire.sh) (legacy filename, same scenario). Thirty seconds, two outcomes:

```
[1/3] mint parent receipt (v0.1.0, in production)
[2/3] cron proposes v0.1.1 (adds `category` field)      → PROMOTE
[3/3] cron proposes v0.2.0-bad (drops `severity`)       → ROLLBACK
       lineage rejected: NotARefinement(OutputWidened)
```

The moment `NotARefinement(OutputWidened)` prints is the moment the protocol becomes obvious. That line is mechanically the rule that fired — R4: outputs may not widen.

## Self-verify

Three properties any user can verify locally with no third-party tooling.

```
self-verifying:   `lyra self-check` re-derives pinned acceptance vectors
interoperable:    receipts are single-line text JSON; the wire format is open
decentralized:    no central registry, no signing authority, no upstream pin
```

Three runtime dependencies in the entire stack: `uor-foundation` (the substrate), `uor-foundation-sdk` (the sealed-type macro layer), and `blake3` (the hash). The JSON parser, base64 codec, JSON-RPC envelope, demo, and acceptance suite are all hand-rolled in the same crate.

Anyone with a `lyra` binary can prove their copy is honest:

```
$ lyra self-check
lyra self-check (runtime: hermes-lyra/0.2.0+uor-foundation/0.4.2)
  PASS  inbox-triage      / skill_interface_hash
  PASS  news-brief        / skill_interface_hash
  PASS  code-review v0.1.0/ skill_interface_hash
  PASS  code-review v0.1.1/ skill_interface_hash
  PASS  lineage v0.1.0->v0.1.1 / next_generation
  PASS  v0.2.0-bad rejected with OutputWidened (tripwire live)
  PASS  receipt JSON roundtrip
self-check: 7/7 PASS
```

If your binary disagrees with anyone else's on a single byte, `self-check` says so. That is the decentralization property in one shell line.
