# Changelog

All notable changes to `hermes-lyra` (the reference implementation living at `lyra-ref`) are recorded here.

The protocol identifier in proofs (`proof.protocol`) maps to a particular wire-format/canonical-bytes contract. Bumping the protocol minor invalidates older proofs unless the new version explicitly lists the older one in `COMPATIBLE_RUNTIMES`.

## 0.4.0 (additive — structural lint, no protocol-bytes change)

Adds a textual lint layer (`lyra lint <SKILL.md>` / `skill_lint` over MCP) that
complements the cryptographic gates. The protocol bytes are unchanged from
v0.3.0: receipts, CIDs, schema CIDs, JSON-LD output, and proof line format all
round-trip identically.

**Layering.** `lyra verify` answers "does the proof match the bytes?";
`lyra lint` answers "does the file follow the conventions other ecosystem
tooling expects?" Lint runs before bind/verify; failure does not block them.

### Two tiers, empirically calibrated

**Tier-0** (default) — six rules, hard fail with exit code 1. Each rule was
selected because it passes 100% on every audited skill we could find: the
87-skill upstream Hermes/agentskills.io corpus *and* every Lyra-native
descriptor in this repo. Zero false positives on real production content.

1. Frontmatter exists and parses as flat YAML
2. `name` is a valid Hermes slug — `[a-z0-9][a-z0-9-]*[a-z0-9]`
3. Body contains at least one H1 (`# `)
4. Body trimmed length ≥ 200 chars
5. Body contains at least one H2 (`## `)
6. Body contains a fenced code block or a list item

**Strict** (`--strict` flag / `strict: true` argument) — advisory rules for
Hermes-side and Lyra-author conventions that are *not* universal across
both ecosystems. **Advisory diagnostics never fail the build** — they
return `status: "advisory"` with exit code 0:

- `description` is present and non-empty (Hermes-side; Lyra-native
  descriptors omit it because the contract lives in `input_shape` /
  `output_shape`)
- `version` is SemVer (`N.N.N[-ident]`)
- H1 in body contains the `name` slug
- `platforms` is present and non-empty

### What was empirically *rejected* from Tier-0

The design audit measured candidate rules against 87 upstream skills.
These rules looked plausible but actually misidentify real, correct
authoring:

| Rejected rule | Upstream-skill failures | Why excluded |
|---|---:|---|
| `name == H1 (slug-normalized)` | 23/87 (26%) | `findmy` vs `# Find My (Apple)`; slug ≠ display title — by design |
| `prerequisites.commands ⊇ derived effects` | 45/87 (52%) | Upstream convention is to declare nothing; deriving from bash blocks produces noise |
| `version is SemVer` | 7/87 (8%) | Real skills use `1.0`, `2024-05-13`, etc. |
| `platforms non-empty` | 1/87 | `teams-meeting-pipeline` is platform-agnostic |
| `description non-empty` | 0/87 upstream, 3/9 Lyra-native | Hermes-side convention only; Lyra-native skills correctly omit it |

The last row is the load-bearing lesson: a rule that passes 100% on the
upstream-skill corpus can still be wrong as a hard rule, because the
Lyra-native skills in *this* repo use a deliberately different shape.
The audit data forced `description` into the advisory tier.

### Output contract

```
$ lyra lint skill.md
{"status":"clean"}                                             # exit 0

$ lyra lint broken.md
{"status":"lint_failed","rules":[{"id":"name-slug",            # exit 1
  "tier":"tier0","message":"..."}]}

$ lyra lint hermes-mirror.md --strict
{"status":"advisory","rules":[{"id":"strict-h1-matches-name",  # exit 0
  "tier":"strict","message":"H1 \"GitHub PR Workflow\" does not contain slug \"github-pr-workflow\""}]}
```

Each rule object has stable `id` (machine-readable), `tier`
(`tier0`|`strict`), and `message` (human-readable).

### MCP surface

`tools/list` now advertises six tools — the existing five gates plus
`skill_lint`. Input schema:

```json
{"skill_md": "...", "strict": true}
```

`strict` defaults to `false`. Output JSON is byte-identical to the CLI's.

### Tests

- New `tests/lint.rs`: end-to-end coverage — all 9 bundled examples
  pass Tier-0; at least one example raises an advisory under `--strict`;
  the JSON contract is stable; `--strict` diagnostics never escalate.
- Linter unit tests in `src/linter.rs`: 12 cases covering each rule's
  predicate, JSON serialization, slug parser boundaries, SemVer parser
  boundaries, and Lyra-native vs Hermes-style outcomes.
- Full suite: **295 tests pass / 0 fail**. `lyra self-check` 7/7. All
  example SKILL.md files verify under v0.3 cryptographically AND lint
  clean at Tier-0.

### No protocol break

v0.3.0 receipts, CIDs, schema CIDs, JSON-LD output, and proof-line bytes
are unchanged. `COMPATIBLE_RUNTIMES` is unchanged. A v0.4 verifier
reading a v0.3 SKILL.md returns the same result it did before.

## 0.3.0 (breaking — content addressing + schemas-first)

A SKILL.md is now a **self-sealing envelope**: every byte of the file outside a single `proof:` line participates in one CIDv1 that is embedded back into that proof line. There is exactly one CID per file, and it is recoverable by anyone with the bytes — no protocol-specific framing, no runtime version inside the hash, no descriptor/file CID duality.

**Schemas are CIDs.** Every descriptor declares which schema it instantiates by embedding the schema's CID in a `schema` field. The root schema, `lyra-skill/v1`, is itself a UOR-typed `ConstrainedTypeShape` declaration whose canonical bytes hash to `bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju`. The kernel grinds the schema's constraint set through `pipeline::run` at build time; self-contradicting schemas (`0 = bias≠0`) cannot ground and cannot pass for real schemas. The builder rejects descriptors carrying an unrecognized schema CID with `DescriptorBuildError::UnsupportedSchema`. See [spec § Schemas-first](specification.md#schemas-first-v030).

**Proof strategies classify under UOR.** Each `computation_id` maps to a `ProofStrategy` IRI from the UOR foundation ontology: `compose_interfaces` → `Composition`, the other three → `Computation`. Exposed via `proof_strategy::proof_strategy_iri`. Informational only — the envelope CID remains the trust path. See [spec § Proof strategies](specification.md#proof-strategies-v030).

**JSON-LD output aligns with schema.org (Depth 1).** Skill descriptors render as `schema:SoftwareApplication` with universal fields (`schema:name`, `schema:softwareVersion`) borrowed from schema.org and Lyra-specific fields namespaced under `lyra:`. Search-engine ingestors (Google Rich Results, RDF triplestores) recognize the output natively. The typed shape grammar inside `lyra:inputShape` / `lyra:outputShape` is intentionally not aliased — Depth 1 borrows vocabulary at the descriptor boundary, not at the type system. See [spec § JSON-LD @context](specification.md#json-ld-context).

**References are CIDs.** The descriptor's `references[]` field holds the envelope CIDs of dependent SKILL.md files — bare CIDv1 strings, the same form `lyra cid` emits. The legacy `name@<64-hex>` format is removed. A reference is exactly one CID per entry: one address, one identifier, no parallel naming scheme. The name lives inside the referenced file's frontmatter and is a free byproduct of resolution.

**Manifests use `{name, cid}`.** `skill_reference_resolve` accepts a manifest of `[{name, cid}, ...]`. Matching is by CID alone (the name is metadata for callers, not part of the match). Two manifest entries with the same CID under different names refer to the same object; entries with the same name under different CIDs refer to different objects.

### The envelope rule

```
envelope_bytes = SKILL.md  minus the single frontmatter line beginning with `proof:`
                            (and its single trailing newline)
output_cid     = CIDv1(codec=raw, hash=blake3-256, digest=BLAKE3-256(envelope_bytes))
```

A `kubo` node configured with `--raw-leaves --cid-version=1 --hash=blake3` computes the same CID over the same bytes. The address is multiformats-standard; UOR governs only the types inside the envelope and the hasher primitive.

### Proof line (three fields, on one line, inside frontmatter)

```yaml
proof: {"protocol":"hermes-lyra/0.3","output_cid":"bafkr…","runtime":"hermes-lyra/0.3.0+uor-foundation/0.4.2"}
```

- `spec_uri` from v0.2 — **removed**. Informational duplication of `protocol`.
- `output_hash` from v0.2 — **removed**. Replaced by `output_cid`.
- Field order is fixed: `protocol`, `output_cid`, `runtime`.

### Wire format

Receipt schema:

```json
{
  "computation_id": "skill_interface_hash",
  "input":          "<canonical descriptor JSON>",
  "output_cid":     "bagaaihra…",
  "runtime":        "hermes-lyra/0.3.0+uor-foundation/0.4.2"
}
```

- `output_hash` (64 hex) — **removed**. v0.2 receipts containing this field are rejected at parse time with a diagnostic pointing at `lyra bind`.
- `output_cid` (base32 CIDv1) — **new, required**. Multibase-`b` + 36-byte CIDv1 over `BLAKE3-256(LYRA_PROTOCOL_ID_PREFIX || 0x00 || label || 0x00 || canonical_bytes)`.
- `runtime` — preserved but **no longer folded into the hash**. Used as a verifier compatibility gate (`unsupported_protocol` when unknown), not part of content identity.

### The single invariant

Two implementations that produce the same canonical input bytes produce byte-identical `output_cid` strings. Forever. Across crate versions. Across language implementations.

### Type sealing

New `src/cid.rs` ships a sealed `Cid` struct with private fields and only validated constructors (`from_canonical_input`, `from_raw_blob`, `parse`, `from_binary`). Closed `Codec` (`Raw 0x55` | `Json 0x0200`) and `HashCode` (`Blake3_256 0x1e`) enums — adding a codec is a deliberate protocol bump, not a runtime decision. There is no `From<&str>` escape hatch. A `Cid` value is structurally well-formed by construction.

### Migration

Receipts are not byte-compatible with v0.2. Rebind every SKILL.md:

```
lyra bind <SKILL.md> <descriptor.json>
```

A v0.3 verifier reading a v0.2 receipt returns a typed error pointing at `lyra bind`. v0.2 frontmatter (`proof.output_hash`) is detected and rejected at parse time so partial migrations cannot silently slip through.

### Tests

- New `tests/cid_format.rs`: 22 byte-level vectors covering the multiformats spec — CIDv1 binary layout (36 bytes Raw / 37 bytes Json), multibase-`b` base32 string form (59 / 61 chars), parser rejection (wrong version, unknown codec, unknown hash code, wrong digest length, truncated, trailing bytes), domain-separation across computation labels, round-trip binary + string.
- All v0.2 pinned hashes regenerated as CIDs: `EXPECTED_INBOX_TRIAGE_CID`, `EXPECTED_NEWS_BRIEF_CID`, `EXPECTED_CR_V010_CID`, `EXPECTED_CR_V011_CID`, `EXPECTED_LINEAGE_CID`, `COMPATIBLE_CID`.
- Full suite: **251 tests pass / 0 fail**. `lyra self-check` 7/7. All three shipped examples verify under v0.3.

### Spec

New `§ Content addressing (v0.3.0+)` section in `specification.md` formalises the layout, type sealing, and verification flow.

### What is intentionally absent

No bundled IPFS client. `lyra publish` (next CLI addition) emits canonical bytes + CID; the user pins them with whatever transport they trust (`ipfs add`, web3.storage, USB stick). Decentralization means we don't pick the transport for you. No `lyra fetch` either — `curl $gateway/$cid | lyra verify -` is the one-liner.

## 0.2.1 (additive)

Strict-parse mode for the descriptor JSON. **Non-breaking for valid input**: every byte sequence that was accepted under v0.2.0 still parses to the same canonical form. The protocol identifier stays at `hermes-lyra/0.2`; the runtime identifier becomes `hermes-lyra/0.2.1+uor-foundation/0.4.2`. All v0.2.0 pinned hashes continue to verify.

### What tightens

The acceptance set narrows so that two conforming implementations agree on which byte sequences are valid input at all — a prerequisite for content-addressing (every input that hashes maps to one canonical form, every accepted byte sequence has one CID). Rejected:

- Trailing commas in objects and arrays (`{"a":1,}`, `["a",]`).
- Unquoted object keys (`{name:"x"}`).
- Leading `U+FEFF` byte-order mark.
- Bare carriage return (`\r`) bytes anywhere in input.
- Nesting deeper than 32 levels (`MAX_NESTING_DEPTH` constant in `src/computations.rs`).

Duplicate object keys were already rejected; restated in the spec for completeness.

### Spec

New section `§ Strict parsing (v0.2.1+)` in `specification.md` enumerates the acceptance-set rules formally.

### Tests

10 new tests in `tests/strict_parse.rs` pin each rejection case with adversarial fixtures. Full suite is 225 tests (215 from v0.2.0 + 10 new), all green. `lyra self-check` remains 7/7.

## 0.2.0 (breaking)

Renamed user-facing surface. Internal Rust type names (`Descriptor`, `EmbeddedProof`, the `lyra_ref` crate name) are unchanged; the binary stays `lyra`.

### One-line install

`lyra install` resolves `~/.hermes/config.yaml` (or `$HERMES_HOME/config.yaml`), splices `mcp_servers.lyra` to point at the binary that ran the command, and writes atomically. Idempotent (second run is `unchanged`). Update-in-place when the path changes. `--uninstall` removes the entry. Surgical YAML edit — never round-trips the whole file, so comments and unrelated keys are preserved. Hermes's config-file watcher auto-reloads MCP on save; no restart needed.

### Protocol

- **Protocol identifier**: `lyra/0.1` → `hermes-lyra/0.2`.
- **Runtime identifier**: `lyra-ref/<v>+uor-foundation/<v>` → `hermes-lyra/<v>+uor-foundation/<v>`.
- **Spec URI**: now points to `https://github.com/ZiriaLabs/hermes-lyra`.
- **No backward verify**: `COMPATIBLE_RUNTIMES` is empty. v0.1 proofs return `unsupported_protocol`. Re-bind any v0.1 SKILL.md with `lyra bind` to mint v0.2 proofs.
- **All pinned canonical hashes regenerated** — including the example pack hashes, the `COMPATIBLE_HASH` gate constant, and the `EXPECTED_*_HASH` constants used by `lyra self-check`. Any change to the protocol identifier or runtime ident propagates into the BLAKE3 over canonical bytes, by design.

### MCP tools

| v0.1 name        | v0.2 name        |
|------------------|------------------|
| `lyra_certify`   | `skill_bind`     |
| `lyra_verify`    | `skill_verify`   |
| `lyra_refine`    | `skill_refine`   |
| `lyra_compose`   | `skill_compose`  |
| `lyra_fuse`      | `skill_merge`    |

The legacy `lyra_*` names are **not** advertised in `tools/list` and a regression test enumerates them as a blocklist. The seven `lyra_md_*` / `lyra_tripwire` / `lyra_chain_check` / `lyra_compose_check` / `lyra_score` MCP tools from v0.1 are removed (they were never the agent-facing surface).

### CLI subcommands

| v0.1            | v0.2     | Notes                                     |
|-----------------|----------|-------------------------------------------|
| `lyra certify`  | `lyra bind`    | legacy name no longer dispatches    |
| `lyra fuse`     | `lyra merge`   | legacy name no longer dispatches    |
| `lyra demo tripwire` | `lyra demo refine` | both still accepted (one-line equivalence) |

USAGE / `--help` text rewritten to drop "Lyra Protocol" branding.

### Effects vocabulary

Closed set:

| v0.1 effect         | v0.2 effect    |
|---------------------|----------------|
| `none`              | `none`         |
| `file_read`         | `file_read`    |
| `file_write`        | `file_write`   |
| `network_read`      | `web_read`     |
| `network_write`     | `web_write`    |
| `subprocess`        | `terminal`     |
| `llm_call`          | `llm`          |

Wire strings on the receipt JSON match the v0.2 column. Refinement check R5 (`effects ⊆ parent`) catches a skill that silently widens its capability surface.

### SKILL.md format

**Frontmatter-only.** v0.1 carried the typed descriptor and proof inside a fenced ` ```lyra ` code block in the markdown body. v0.2 carries them as YAML frontmatter keys:

```
---
name: my-skill
version: "0.1.0"
contract: {"name":"my-skill",...}
proof:    {"protocol":"hermes-lyra/0.2","output_hash":"...",...}
---

# my-skill prose
```

The `contract:` and `proof:` values are valid YAML (inline JSON), readable by any YAML parser. The fenced-block code path is gone — `extract_lyra_block`, `parse_lyra_block`, `LyraBlockContent`, `FENCE_OPEN_PREFIX`, `replace_lyra_block`, and `append_lyra_block` were deleted. The body hash (over the SKILL.md with `contract:` and `proof:` stripped) is what enters canonical bytes, so re-binding the same descriptor to the same body is bit-identical.

### Documentation

- `README.md` rewritten. Drops legacy marketing and tripwire/Lyra-Protocol branding.
- `examples/README.md` rewritten.
- `specification.md` updated: protocol identifier, spec URI, MCP tool names.
- `CONTRIBUTING.md`: project owner references generalized; effect vocabulary moved to v0.2 closed set.

### Repository layout

Collapsed to a single canonical crate at the repo root (no workspace wrapper, no nested `lyra-ref/`). `uor-foundation` and `uor-foundation-sdk` are now plain crates.io dependencies (pinned to `=0.4.2`); the `uor-foundation-test-helpers` dependency was removed (it was vestigial — no source-code usage).

### Repository / package metadata

- `Cargo.toml` (workspace + `lyra-ref`): `repository` and `homepage` point at `ZiriaLabs/hermes-lyra`. Author rolled to `hermes-lyra contributors`.

### Migration

A v0.1 SKILL.md regenerates to v0.2 in one step:

```bash
lyra bind   v01-skill.md   sidecar-descriptor.json   > v02-skill.md
lyra verify v02-skill.md
```

No v0.1 → v0.2 lineage receipts. The two versions are not Liskov-substitutable for each other at the protocol level (different protocol identifier ⇒ different canonical bytes), so there is no automatic upgrade — only a re-bind.

## 0.1.0

Initial public reference implementation. Five canonical gates (`certify`, `verify`, `refine`, `compose`, `fuse`). Fenced-block ` ```lyra ` SKILL.md format. Protocol identifier `lyra/0.1`.
