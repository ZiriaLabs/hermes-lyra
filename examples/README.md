# hermes-lyra examples

Nine reference skill packs. Three originals (`inbox-triage`, `news-brief`, `code-review-evolve`) demonstrate one property of the contract layer each; six GitHub-flavored examples (`codebase-inspection`, `github-auth`, `github-code-review`, `github-issues`, `github-pr-workflow`, `github-repo-management`) mirror common Hermes Agent skills, showing how their typed boundary expresses in Lyra.

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

## SKILL.md format (v0.3 envelope)

A skill carries its contract and self-sealing proof directly in YAML frontmatter:

```
---
name: my-skill
contract: {"name":"my-skill","version":"0.1.0","schema":"bafkr4ie...",
           "input_shape":{...},"output_shape":{...},
           "effects":["llm"],"references":[],"content_hash":"..."}
proof:    {"protocol":"hermes-lyra/0.3","output_cid":"bafkr4i...",
           "runtime":"hermes-lyra/0.3.0+uor-foundation/0.4.2"}
---

# my-skill
Prose Hermes reads as before.
```

Standard YAML frontmatter, standard markdown body. The `contract:` and `proof:` keys are valid YAML (inline JSON values), readable by any YAML parser, ignored by anything that doesn't care about them. Single file, both layers.

The `output_cid` is the envelope CID — a CIDv1 over every byte of the file outside the `proof:` line. `kubo` with `--raw-leaves --cid-version=1 --hash=blake3` computes the same value.

### The bridge in commands

```bash
lyra bind    SKILL.md descriptor.json    # embed contract + proof into frontmatter
lyra verify  SKILL.md                    # re-derive embedded proof
lyra refine  parent.md child.md          # gates auto-detect .md vs .json
lyra compose s1.md s2.md [s3.md ...]
lyra merge   producer.md consumer.md
```

The high-level gates accept either a SKILL.md path or a descriptor JSON transparently; the bridge routes correctly.

## The nine packs

```
inbox-triage              a typed pipe a downstream consumer can trust
news-brief                pinned CID references; reruns are reproducible
code-review-evolve        self-improvement that has to prove itself
codebase-inspection       LOC + language stats as a typed receipt
github-auth               auth bootstrap with a non-secret-leaking output
github-code-review        PR review verdict + inline comments, type-locked
github-issues             create/triage/label issues under a fixed surface
github-pr-workflow        PR lifecycle pinned at the "open + CI" boundary
github-repo-management    clone/create/fork with an effect budget
```

| Pack | What it locks down | Mechanism |
|---|---|---|
| [`inbox-triage`](inbox-triage/) | Cron summarizers drift when the model is swapped. | A typed output shape. The downstream consumer fails closed at compose time, not in production. |
| [`news-brief`](news-brief/) | Multi-channel digests reshape when a sub-skill quietly upgrades. | Pinned CID references. Upgrades are visible Git diffs, not silent. |
| [`code-review-evolve`](code-review-evolve/) | Self-improving skills need proof, not vibes. | Liskov-substitutable lineage receipts. R1–R5 hold or the new version stays in staging. |
| [`codebase-inspection`](codebase-inspection/) | LOC counters drift across machines. | A determinstic typed output. CI can refuse a PR that silently moves the LOC needle. |
| [`github-auth`](github-auth/) | Credential-bootstrap audit logs accidentally leak secrets. | Output shape pins `status + scopes + remaining` — never the token itself. |
| [`github-code-review`](github-code-review/) | Reviewer-agent comments drift in shape after a model swap. | Inline comment surface (`file/line/body`) is type-locked. |
| [`github-issues`](github-issues/) | An auto-filer bug can spam a repo with bad data. | Issue-creation surface (`title/body/labels`) is byte-bounded; refinements that rename a field fail compose-check before any new issue is filed. |
| [`github-pr-workflow`](github-pr-workflow/) | Multi-effect PR skills accumulate scope over time. | `effects` budget makes scope explicit; a refinement that adds `llm` cannot mint a lineage receipt under R5. |
| [`github-repo-management`](github-repo-management/) | Swiss-army-knife skills hide their real effect surface. | Effect set enumerated up front; stricter sub-skills (e.g. `github-readonly-clone`) pass R5 by *shrinking* effects, never adding. |

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
lyra self-check (runtime: hermes-lyra/0.3.0+uor-foundation/0.4.2)
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
