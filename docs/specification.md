# hermes-lyra v0.1

**A deterministic skill-interface standard for AI agents.**

Reference implementation: the `lyra_ref` crate in the repository root.

## Purpose

A single, minimal, deterministic standard for declaring the interface contract of an AI agent skill. Every skill that claims Lyra compliance MUST expose a descriptor matching this exact schema. The descriptor is validated to produce a sealed receipt attesting to structural correctness.

## Design principles

1. **One way to declare.** No optional extensions, no profiles, no vendor prefixes.
2. **Deterministic canonicalization.** Two semantically identical descriptors MUST canonicalize to identical bytes.
3. **Typed contracts.** Every shape and effect comes from a fixed, closed vocabulary.
4. **Content-addressed identity.** The skill body is identified by its BLAKE3 hash, not its name.
5. **Minimal surface.** Only fields necessary for interoperability, composability, and audit.

## Descriptor schema

A Lyra descriptor is a JSON object with exactly these seven fields, in canonical alphabetical order:

```json
{
  "content_hash": "<hex: 64 lowercase hex chars>",
  "effects": [<effect>],
  "input_shape": <shape>,
  "name": "<kebab-case: 1-64 chars, [a-z0-9-]>",
  "output_shape": <shape>,
  "references": ["<name>"],
  "version": "<semver: major.minor.patch>"
}
```

### Field definitions

| Field | Type | Constraints |
|---|---|---|
| `content_hash` | string | Exactly 64 lowercase hex characters. BLAKE3-256 of the skill body. **Caller-attested**: Lyra binds the proof to the declared hash but never recomputes it from a SKILL.md body. See [content_hash binding](#content_hash-binding). |
| `effects` | array | Zero or more effect tags from the closed vocabulary. |
| `input_shape` | shape | The shape of the single input value the skill accepts. |
| `name` | string | 1–64 ASCII characters: lowercase letters, digits, hyphens. No leading/trailing hyphen. No consecutive hyphens. Matches the [agentskills.io](https://agentskills.io/specification) `name` rules. |
| `output_shape` | shape | The shape of the single output value the skill produces. |
| `references` | array | Zero or more CIDv1 strings naming dependencies. |
| `schema` | string | CIDv1 of the schema this descriptor instantiates. v1 recognizes exactly one value, [`LYRA_SKILL_SCHEMA_V1_CID`](#schemas-first-v030). Anything else is rejected as `unsupported_schema`. See [Schemas-first](#schemas-first-v030). |
| `version` | string | SemVer `major.minor.patch`, each component a 32-bit unsigned integer. |

#### content_hash binding

`content_hash` is the BLAKE3-256 digest of the skill body the descriptor describes. In Lyra v0.1 it is **caller-attested**, not server-verified:

- `skill_bind` accepts the declared `content_hash` as input and binds it into the proof. It does **not** hash the SKILL.md body to confirm the value.
- `skill_verify` re-derives the proof from the embedded descriptor and checks the descriptor against the SKILL.md's frontmatter contract. It does **not** hash the body and compare.
- An attacker who controls a SKILL.md can therefore embed a body that contradicts the declared hash. The proof remains internally consistent — but it attests to *the descriptor*, not to the body.

The integrity gap is closed at the layer that *fetched* the SKILL.md: an out-of-band hash check (registry index, signed manifest, distribution CDN) is responsible for proving that the body the consumer reads is the body the producer published. Lyra binds the typed contract; the distribution layer binds the body.

A future protocol revision MAY add a `lyra_attest_body` gate that hashes the body and compares to the declared `content_hash`, raising the integrity bound from "caller-attested" to "substrate-verified". v0.1 deliberately stays out of the bytes-of-the-body business so the trust model is small and inspectable.

### Shape grammar

```
shape ::= scalar | structured | list

scalar ::= { "type": "u8",  "max_bytes": uint }
         | { "type": "u16", "max_bytes": uint }
         | { "type": "u32", "max_bytes": uint }
         | { "type": "u64", "max_bytes": uint }
         | { "type": "string", "max_bytes": uint }
         | { "type": "bytes",  "max_bytes": uint }

structured ::= { "type": "structured", "fields": [field] }
field ::= { "name": ident, "shape": shape }

list ::= { "type": "list", "item": shape, "max_items": uint }

uint  ::= integer in range [1, 16777216]  // 16 MiB cap
ident ::= string matching [A-Za-z_][A-Za-z0-9_]{0,63}
```

Bounds on `max_bytes` per scalar type:

| Lyra type | `max_bytes` upper bound |
|---|---|
| `u8` | 1 |
| `u16` | 2 |
| `u32` | 4 |
| `u64` | 8 |
| `string` | 16777216 (16 MiB) |
| `bytes` | 16777216 (16 MiB) |
| `structured` | product of field `max_bytes` ≤ 16777216 |
| `list` | `item.max_bytes * max_items` ≤ 16777216 |

The `16777216` cap (16 MiB) is the single universal capacity bound for v0.1.

#### Shape position: root vs nested

The grammar is uniform — *every* shape variant (scalar, `structured`, `list`) is legal both as a top-level `input_shape`/`output_shape` and as a nested element inside a container. There is no separate "root shape" vocabulary.

Consequences worth stating explicitly so authors don't second-guess them:

- A skill MAY take a single scalar as its top-level input (`input_shape: {"type":"string","max_bytes":256}`) — there is no requirement to wrap it in a `structured` with a single field.
- A skill MAY return a top-level `list` (`output_shape: {"type":"list","item":...,"max_items":...}`) — there is no requirement to wrap it in a `structured` with a single field named `items`.
- A `structured` field's `shape` MAY itself be `structured` or `list`. Nesting depth is bounded only by the 16 MiB capacity cap, which compounds multiplicatively through containers.
- A `list`'s `item` MAY itself be `list` or `structured`. Same depth/capacity rule.

The only positional constraint anywhere in the grammar is the capacity rule: at every level, `max_bytes` (for scalars) or the multiplicative product (for containers) must fit within the 16 MiB universal cap. The constraint is on *size*, not on *position*.

### Effect vocabulary (closed set)

| Tag | Meaning |
|---|---|
| `none` | No observable side effects. Pure function. |
| `file_read` | Reads from the filesystem. |
| `file_write` | Writes to the filesystem. |
| `network_read` | Makes an outbound network request. |
| `network_write` | Sends data over the network. |
| `subprocess` | Spawns an external process. |
| `llm_call` | Calls an LLM API. |

A skill MAY declare `none` alongside other effects. If `none` is the only element, the skill claims to be pure.

## Canonicalization rules

Two descriptors are equivalent iff their canonical forms are byte-identical. Canonicalization:

1. Object keys appear in alphabetical order: `content_hash`, `effects`, `input_shape`, `name`, `output_shape`, `references`, `version`.
2. No whitespace outside of string values.
3. Arrays are ordered lexicographically by their canonical string representation.
4. `structured.fields` are ordered lexicographically by their field `name`.
5. Numbers have no leading zeros, no sign, no decimal point, no exponent.
6. Strings use `"` delimiters with minimal escaping (`\`, `"`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX` for control chars).
7. `content_hash` is lowercase hex.
8. `name` is lowercase (enforced by the field constraint, not canonicalization).

## Strict parsing (v0.2.1+)

Canonicalization defines the **output** of a parse. Strict parsing defines the **acceptance set** — the set of byte sequences a conforming implementation may parse. Two implementations must agree on both. Where canonicalization tells you that `{"a":1,"b":2}` and `{"b":2,"a":1}` produce the same hash, strict parsing tells you which byte sequences are accepted at all.

The protocol pins one acceptance set. Any conforming implementation MUST reject the following byte sequences with an error and MUST NOT silently treat them as equivalent to a valid form:

1. **Trailing commas** — `{"a":1,}` or `["a",]`. Different lenient parsers absorb trailing commas at different points; pinning a single acceptance set means one CID per file across every implementation.
2. **Unquoted object keys** — `{name:"x"}`. JSON keys MUST be quoted strings; JavaScript identifier keys are out of scope.
3. **Leading byte-order mark** — `U+FEFF` at the start of input. Some editors silently inject a BOM; the receipt envelope rejects it so a re-saved file does not change its CID.
4. **Bare carriage returns** — a `\r` byte anywhere in the input. Canonical JSON uses `\n` for newlines; `\r\n` and lone `\r` are Windows-line-ending artefacts and produce different bytes for the same logical document.
5. **Excessive nesting depth** — any object or array nested more than **32 levels** deep. Unbounded nesting is both a DOS vector and an acceptance-set ambiguity (different implementations blow their stacks at different depths). The cap is global across objects and arrays combined.
6. **Duplicate object keys** — `{"a":1,"a":2}`. Already-rejected; restated here for completeness.

These six rules are independent of canonicalization: they tighten what bytes are *accepted as input*, but they do not change the canonical *output* for any input that was already valid under v0.2.0. The same v0.2.0 pinned hashes therefore continue to verify under v0.2.1 — strict mode is a pure tightening of the acceptance set.

The `MAX_NESTING_DEPTH` constant lives in `src/computations.rs`; conforming implementations MUST use the same value.

## Content addressing (v0.3.0+)

A SKILL.md is a **self-sealing envelope**. The entire file — frontmatter, contract, prose, whitespace — except for a single `proof:` line participates in one CID that is embedded back into the proof line. Any byte change anywhere in the file changes the CID; tampering with the proof line itself is detected because the (stripped) bytes still hash to the original CID.

There is exactly one CID per file. The protocol identifier becomes `hermes-lyra/0.3`. v0.2 receipts return `unsupported_protocol`.

### The envelope rule

```
envelope_bytes   =  SKILL.md  minus the single frontmatter line beginning with `proof:`
                                (and the single trailing newline that line consumes)
output_cid       =  CIDv1(codec=raw, hash=blake3-256,
                          digest=BLAKE3-256(envelope_bytes))
```

That's the only rule. No protocol framing inside the hash, no domain separators, no canonicalization beyond what already lives in the SKILL.md on disk. A Python or Go verifier needs three things: read the file, strip the proof line, BLAKE3 the rest.

**Canonical position of the proof line.** Exactly one line, at column 0 inside the frontmatter (between the opening `---` and closing `---`), beginning with literal bytes `proof:`. Multi-line proof values are rejected at parse time.

**Why raw codec (0x55).** A SKILL.md is markdown-with-frontmatter, not JSON. Raw is the multiformats-correct codec for opaque file content. A kubo node accepts the same bytes via `ipfs add --raw-leaves --cid-version=1 --hash=blake3` and returns the same CID.

### Frontmatter proof block (v0.3)

```yaml
proof: {"protocol":"hermes-lyra/0.3","output_cid":"bafy…","runtime":"hermes-lyra/0.3.1+uor-foundation/0.4.2"}
```

Exactly three fields, in that order, on one line. `spec_uri` from v0.2 is dropped — informational duplication of `protocol`, which is the authoritative identifier.

### Verification flow

```
1. Parse: locate the frontmatter proof line (between `---` separators,
   column 0, prefix `proof:`). MUST be exactly one. MUST be single-line JSON.
2. Strip: remove that line + its trailing `\n` → envelope_bytes.
3. Hash: expected_cid = CIDv1(raw, blake3, BLAKE3(envelope_bytes)).
4. Compare: expected_cid == proof.output_cid → valid; else mismatch.
5. Runtime gate: proof.runtime ∈ {LYRA_RUNTIME_IDENT} ∪ COMPATIBLE_RUNTIMES
                 else → unsupported_protocol.
```

Steps 2-4 are the seal. Step 5 is the runtime compatibility check.

### What is sealed

Every byte of the file outside the single proof line:
- frontmatter keys (`name:`, `version:`, the `contract:` JSON, any author-added keys)
- the closing `---` separator
- the prose body
- trailing whitespace and newlines

### What is not sealed

- The proof line itself (it's what's stripped).
- The `contract:` value is hashed *as text inside the envelope* — its canonical form is not enforced. The author's frontmatter pretty-printing is preserved verbatim, and the CID seals exactly the byte sequence the author wrote. Two SKILL.md files with semantically equivalent but textually different `contract:` JSON have different CIDs. This is the right behaviour for a file-level envelope.

### Receipt schema (unchanged from earlier v0.3 sketches)

```json
{
  "computation_id": "skill_interface_hash",
  "input":          "<exact bytes the computation consumed>",
  "output_cid":     "bafy…",
  "runtime":        "hermes-lyra/0.3.1+uor-foundation/0.4.2"
}
```

For receipts emitted by primitive computations (e.g., direct `lyra score`), `output_cid` is the CID over the framed canonical computation input, identical to the previous v0.3 design. For SKILL.md envelopes, `output_cid` is the file-level CID. The two contexts are disjoint — a receipt has a `computation_id`, a SKILL.md is just bytes.

### IPFS interoperability

```
$ ipfs add --raw-leaves --cid-version=1 --hash=blake3 examples/inbox-triage/SKILL.md
added bafkr... 1234 bytes

$ lyra cid examples/inbox-triage/SKILL.md
bafkr...
```

The two CIDs match — the file-level CID is exactly what kubo computes when configured with `raw-leaves + cid-version=1 + blake3`. No custom multicodec, no protocol-specific wrapper.

### What this enables

1. **Drop-in IPFS publication.** `ipfs add` a SKILL.md, get back a CID that `lyra verify` accepts without modification.
2. **Single-CID identity.** A skill is one byte sequence, one CID. No duality between file-CID and contract-CID.
3. **Forks are different files with different CIDs.** Editing the contract, the body prose, or even a single character of frontmatter produces a different CID. Curators publish a registry as `[{name, cid}, ...]`; users follow whichever curator's CID they trust.
4. **Cross-implementation determinism.** Any future Python/Go implementation that strips the proof line and BLAKE3s the rest produces an identical CID. Multiformats-standard, no UOR runtime needed at the verifier.

### UOR anchoring

The CID itself is a multiformats-standard value, not a UOR-shaped value. UOR anchoring lives one layer down:

- The descriptor inside the `contract:` field uses `ConstrainedTypeShape`-projected leaf types (`LyraU8`, …, `LyraString`, `LyraBytes` from `src/shape.rs`).
- The BLAKE3 hasher used to compute the CID implements `uor_foundation::enforcement::Hasher` — registered with UOR even though BLAKE3 itself is not a UOR construct.
- The runtime ident declares its UOR substrate version (`+uor-foundation/<version>`) at compile time, visible in the proof line.

The CID seals UOR-typed content via a UOR-registered hasher. The address itself is multiformats. This is the correct factoring: UOR governs the types and the hashing primitive; multiformats governs the addressing.

## JSON-LD `@context`

For the JSON-LD interchange format, Lyra binds two namespaces:

```
schema: https://schema.org/                       (universal vocabulary)
lyra:   https://lyra-protocol.org/ontology/v0.1/  (Lyra-specific vocabulary)
```

A skill descriptor renders as a `schema:SoftwareApplication`. Universal fields (`schema:name`, `schema:softwareVersion`) borrow from schema.org so search-engine ingestors, RDF triplestores, and JSON-LD-aware retrieval pipelines understand the output natively. Lyra-specific fields (`lyra:schema`, `lyra:contentHash`, `lyra:inputShape`, `lyra:outputShape`, `lyra:effects`, `lyra:references`) stay under the `lyra:` namespace because schema.org has no native vocabulary for them.

```json
{
  "@context": { "schema": "https://schema.org/", "lyra": "https://lyra-protocol.org/ontology/v0.1/" },
  "@type": "schema:SoftwareApplication",
  "schema:name": "inbox-triage",
  "schema:softwareVersion": "0.1.0",
  "lyra:schema":      "bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju",
  "lyra:contentHash": "…",
  "lyra:inputShape":  { "type": "structured", "fields": [...] },
  "lyra:outputShape": { "type": "structured", "fields": [...] },
  "lyra:effects":     ["llm"],
  "lyra:references":  []
}
```

This is **Depth-1 schema.org alignment** — vocabulary borrow at the descriptor boundary. The typed shape grammar (everything under `lyra:inputShape` / `lyra:outputShape`) is not aliased; it's Lyra's own type system, not a schema.org subgraph. Depth 3 (schema.org as our meta-schema) is deliberately not done — that would trade the compile-time guarantees of `ConstrainedTypeShape` for schema.org's loose `expectedType` model.

Shape IRIs (under `lyra:inputShape` / `lyra:outputShape`) are namespaced under `https://lyra-protocol.org/shapes/v0.1/{u8,u16,u32,u64,string,bytes,structured,list}`.

JSON-LD is an **edge format**. The typed descriptor is the source of truth, the envelope CID is the trust path. JSON-LD never enters the trust path; it exists only for cross-framework interchange and search-engine discoverability.

## Schemas-first (v0.3.0+)

A schema in Lyra is a CID-addressed object. Every skill descriptor names the schema it instantiates by embedding that schema's CID in its `schema` field. Schemas are not magic: they are themselves UOR-typed declarations addressable as canonical bytes.

### The v1 root schema

`lyra-skill/v1` is the root schema for all v0.3 skill descriptors. It is declared in Rust as a `ConstrainedTypeShape` with a discriminating `Affine` rule over per-slot indicators (sum of 8 coefficients = 8, bias = −8), which the UOR kernel grinds to `Ok(Grounded<T>)` at preflight. An adversary publishing a "schema" whose constraint system is inconsistent — e.g. `0 = bias≠0` — is rejected with `PipelineFailure::ShapeViolation` and cannot stand in for a real schema.

The schema's identity is its CID over the canonical bytes:

```
canonical_bytes = {
  "constraints": [{"bias":-8,"coefficient_count":8,"coefficients":[1,1,1,1,1,1,1,1],"type":"affine"}],
  "iri":         "https://lyra-protocol.org/schemas/v0.1/skill",
  "kind":        "lyra-skill",
  "site_count":  8,
  "version":     "0.1.0"
}
schema_cid      = CIDv1(codec=raw, hash=blake3-256, BLAKE3-256(canonical_bytes))
```

For v1 the pinned value is:

```
LYRA_SKILL_SCHEMA_V1_CID = "bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju"
```

### Two-layer validation

Schema enforcement is intentionally split:

1. **UOR kernel attests *constraint-system consistency*.** When a schema declaration is ground through `pipeline::run::<LyraSkillSchemaV1, _, LyraHasher>`, the kernel walks the `Affine` constraints via `preflight_feasibility` and `run_reduction_stages`. A self-contradicting schema cannot ground; a well-formed one does. This is the load-bearing UOR-anchored half.

2. **The Rust binding layer attests *value satisfaction*.** When a `SkillDescriptor` is built, the builder requires its `schema` field to equal a recognized CID (currently exactly one: the v1 CID). Mismatches surface as `DescriptorBuildError::UnsupportedSchema` at build time and as `unsupported_schema` at verify time (typed outcome, not a hard error).

Separation of concerns is honest: the kernel grinds the rules, the binding layer grinds the values.

### Why the schema field is in the envelope

Adding `schema` participates in canonicalization and therefore in the envelope CID. This is intentional. A SKILL.md that silently changed schemas (or claimed a schema it does not satisfy) would compute a different envelope CID; verifiers detect the drift at the same gate that catches body or descriptor mutations. The schema declaration is **transitively sealed** by the same envelope rule as everything else outside the `proof:` line.

### Future schemas

`lyra-skill/v1` is pinned. Future schemas (e.g. `lyra-skill/v2`) will introduce new CIDs and new accepted values in the builder. Old SKILL.md files continue to validate against `v1`; new schemas are additive, not replacements. Schema upgrades are version-aware and CID-addressed, not implicit.

## Proof strategies (v0.3.0+)

Lyra's computations classify under UOR foundation's `ProofStrategy` ontology. Each `computation_id` maps to exactly one strategy IRI:

| `computation_id`            | UOR `ProofStrategy` | IRI                                          |
|-----------------------------|---------------------|----------------------------------------------|
| `skill_interface_hash`      | `Computation`       | `https://uor.foundation/proof/Computation`   |
| `skill_reference_resolve`   | `Computation`       | `https://uor.foundation/proof/Computation`   |
| `compose_interfaces`        | `Composition`       | `https://uor.foundation/proof/Composition`   |
| `next_generation`           | `Computation`       | `https://uor.foundation/proof/Computation`   |

`compose_interfaces` is the only Lyra computation that proves by *composition of sub-identities* (producer's output shape composes with consumer's input shape — categorical composition). The other three are decidable runtime checks; `Computation` is the honest UOR classification.

`next_generation` is deliberately **not** classified as `Composition` even though a *chain* of refinements would compose categorically. A single refinement step is one decidable R1–R5 check, not a composition of two sub-proofs.

These IRIs are **informational, not trust-bearing**. The envelope CID is the trust path. The strategy IRIs exist for cross-framework JSON-LD consumers who want to grep "which UOR strategy backs this computation?" against a stable URI. Lookups are exposed via [`proof_strategy::proof_strategy_iri`](https://github.com/ZiriaLabs/hermes-lyra) in the reference implementation; unknown IDs default to `Computation` (the conservative classification).

## Relationship to agentskills.io

[Agent Skills](https://agentskills.io) is the dominant open format for distributing AI agent skills as `SKILL.md` bundles (YAML frontmatter + Markdown + optional scripts/references/assets). Lyra and agentskills.io solve **different problems** and are designed to compose:

- **agentskills.io** answers *"how does an agent discover and load a skill?"* It's a packaging and discovery format. Trust is human: the user reads the prose and decides to install.
- **hermes-lyra** answers *"how do we prove a skill ran with a declared interface?"* It's a typed verification substrate. Trust is cryptographic: anyone can validate a receipt offline.

### Field alignment

Lyra's `name` field matches agentskills.io's `name` rules exactly: 1–64 ASCII characters, `[a-z0-9-]`, no leading/trailing hyphen, no consecutive hyphens. **Any valid Lyra `name` is a valid agentskills `name`.** This lets a single string identify the same skill in both layers.

| agentskills.io frontmatter | Lyra descriptor | Notes |
|---|---|---|
| `name` (required) | `name` (required) | Same rules. Direct cross-reference key. |
| `description` (required) | — | Discovery prose lives in `SKILL.md`. Lyra is the verification layer below it. |
| `license`, `compatibility`, `metadata` | — | Distribution metadata, not part of the trust contract. |
| `allowed-tools` (experimental, free-form string) | `effects` (closed vocabulary) | Related but distinct: `allowed-tools` lists tools a skill *may use*; `effects` declares categories of side effects the skill *will perform* (`network_read`, `file_write`, etc.). |
| — | `content_hash`, `version`, `input_shape`, `output_shape`, `references` | Lyra-specific. These carry the verification contract. |

A skill author can keep the full agentskills `SKILL.md` for discovery and emit a sidecar Lyra descriptor (e.g. `skill.lyra.json-ld`) for verification — the two layers reference each other through `name`.

## Compliance levels

A skill is **Lyra-Compliant** if and only if:
1. Its descriptor passes structural validation (all fields present, all types in vocabulary, all bounds within limits).
2. Its `content_hash` matches the BLAKE3-256 of the actual skill body.
3. Its `references` all resolve against the registry manifest.
4. It carries a valid receipt from `skill_interface_hash`.

A skill is **Lyra-Composable** with another skill if and only if:
1. Both are Lyra-Compliant.
2. `compose_interfaces` returns `COMPATIBLE` for `(producer.output_shape, consumer.input_shape)`.

## Receipt architecture

A Lyra **attestation** is two layers that travel together:

1. **`content_hash`** — BLAKE3-256 over `(computation_label ‖ 0x00 ‖ canonical_bytes)`. Reproducible by anyone with the same inputs. Proves *this exact content was attested to*.
2. **`seal`** — a sealed certificate produced by the sanctioned pipeline. Cannot be forged outside it. Proves *this content passed structural validation*.

Verifying an attestation means checking **both** layers: recompute `content_hash` from the descriptor and confirm equality, then replay the `seal`. Either layer alone is insufficient; together they prove "*this exact descriptor, validated by the pipeline, with this exact content.*"

The two layers are deliberately orthogonal. The seal is a structural attestation — it captures the type shape of what was validated. The `content_hash` is a value attestation — it captures the specific bytes. Designs that fold value content into the seal exist (and are common in proof systems), but they require a custom output-shape per computation; Lyra v0.1 uses a generic typed input and layers a separate BLAKE3 over canonical bytes. The result is the same end-to-end binding with simpler, auditable primitives.

For deterministic computations the combination is forgery-resistant: an attacker cannot replace either layer without verify detecting it. The `content_hash` ties the receipt to its exact inputs; the `seal` ties the receipt to a genuine pipeline run.

### CLI envelope (on-the-wire form)

The CLI renders attestations as a JSON envelope, used for storage and interchange:

```json
{
  "computation_id": "skill_interface_hash",
  "input": "<canonicalized descriptor string>",
  "output_hash": "<32-byte BLAKE3 hex, == content_hash>",
  "trace_b64": "<base64-encoded seal>"
}
```

`output_hash` is the wire form of `content_hash`; `trace_b64` is the wire form of `seal`. `verify` rebuilds both, replays the seal, and re-runs the computation to confirm the output hash — a triple check.

### Embedded proof in a `SKILL.md` (self-verifying form)

When a typed contract is bound into a `SKILL.md` via the fenced `lyra` block, the block carries a tiny self-verifying proof alongside the descriptor:

```
```lyra
{"descriptor":{...}, "proof":{
  "protocol":    "hermes-lyra/0.2",
  "spec_uri":    "https://github.com/ZiriaLabs/hermes-lyra",
  "output_hash": "<32-byte BLAKE3 hex over canonical descriptor bytes>",
  "runtime":     "lyra-ref/0.1.0+uor-foundation/0.4.2"
}}
```
```

Four fields:

| Field | What it pins | Why a verifier needs it |
|---|---|---|
| `protocol`    | Which rule set governs canonicalization + hashing. | Tells the verifier *which spec to apply*. The value is a content identifier; verifiers select rules locally by name. |
| `spec_uri`    | Canonical repository URI for the protocol's rules and reference implementation. | **Cold-start hint.** A verifier with zero prior knowledge of Lyra can fetch the spec + implementation from this URI to bootstrap. Informational only — verifiers do **not** consult it during the verify path; the authoritative identifier is `protocol`. Any mirror that serves the same repository content is equally valid. |
| `output_hash` | BLAKE3-256 over the canonical descriptor bytes. | The proof artifact. The verifier re-derives it and compares byte-for-byte. |
| `runtime`     | Which implementation produced this proof. | Lets a verifier confirm byte-exact reproducibility against the same substrate, and reject proofs from incompatible runtimes. |

Verifying an embedded proof requires only the SKILL.md itself, the verifier's own `lyra` binary, and one BLAKE3 hash computation. No registry, no signing authority, no network.

**Verify outcomes.** Each is distinct and routed separately by callers:

| Outcome | Meaning | CLI exit |
|---|---|---|
| `valid`                  | proof re-derives, protocol known, substrate compatible | 0 |
| `mismatch`               | re-derived `output_hash` ≠ proof's: descriptor or proof tampered | 1 |
| `no_proof`               | SKILL.md has no embedded proof (legacy / bare descriptor) | 2 |
| `unsupported_protocol`   | proof's `protocol` is unknown to this verifier; not a forgery, just a future-version proof | 2 |
| `substrate_incompatible` | proof's `runtime` is not in the verifier's `COMPATIBLE_RUNTIMES` set | 2 |

**Forward-compat note.** Newly-minted proofs MUST include `protocol` and `spec_uri`. Implementations MAY tolerate extra unrecognized fields in the proof object for forward compatibility — unknown fields are ignored, not rejected. v0.2 does NOT accept v0.1 proofs (`protocol: "lyra/0.1"`); `COMPATIBLE_RUNTIMES` is empty.

### Gate-as-loader: verification is the load path

The `valid` outcome carries two additional fields that turn the verify call into the agent's mandatory load path:

```json
{
  "status":     "valid",
  "descriptor": { ... typed contract ... },
  "body":       "...SKILL.md prose with the lyra fenced block removed...",
  "cached":     false
}
```

- `body` — the SKILL.md prose minus the embedded `lyra` block. This is what the agent's LLM reads as the procedure. Because the body is returned **only** on `valid`, an agent that wants to execute a skill has to verify first; there is no parallel "read body" path in the protocol's MCP surface. Verification stops being a polite convention and becomes the only way to load.

- `cached` — `true` iff the result was served from a process-local memo keyed by `BLAKE3(skill_md_bytes)`, `false` if freshly re-derived. The cache is scoped per process (a fresh CLI invocation always misses; a long-running MCP server cache-hits on repeated loads of the same SKILL.md bytes).

**Latency budget.** First load of a SKILL.md pays the full verify cost (~1 ms — parse fence, parse descriptor, BLAKE3 the canonical bytes, four equality checks). Repeated loads of the same content within a process cost the BLAKE3 of the input string plus a `HashMap` lookup — under 50 µs in steady state. Agents running thousands of skill invocations per minute see negligible verification overhead after the first encounter with each skill.

**Cache correctness.** Identical input bytes deterministically produce identical outcomes, so the cache key (input BLAKE3) uniquely determines the value and never goes stale. The cache lives in process memory; it is never shared across processes (no cross-process trust); it has no eviction logic in v0.1 (a server holding 10k unique SKILL.md cache entries uses well under 10 MB).

### Registry snapshots and hash chains

`registry_snapshot(manifest, prev_digest)` produces an attestation whose `content_hash` includes both the manifest bytes **and** the previous snapshot's `content_hash`. Chaining snapshots — `prev_digest := previous.content_hash` — gives append-only audit: any retroactive edit to a manifest forks the chain at the point of edit, and every downstream snapshot's `content_hash` changes.

## Lineage receipts (v0.1.x)

**A mutation that does not typecheck against its parent cannot mint a `next_generation` receipt.**

The `next_generation` computation seals a parent → child evolution link in a skill's lineage. The receipt proves only that the child descriptor is a structural refinement of the parent under the rules below. It does not carry an execution trace, a diff, or a rationale.

### Refinement rules

A child descriptor *refines* a parent iff it is **Liskov-substitutable** for the parent: anywhere the parent's signature is accepted, the child can drop in safely. By set semantics:

- A structured **input** shape is a *requirement*: the set of valid inputs is the set of records satisfying every required field. So requiring **fewer** fields means accepting **more** inputs.
- A structured **output** shape is a *promise*: downstream consumers depend on those fields existing. So promising **more** fields means satisfying **more** consumers.

A child descriptor refines a parent iff **all** of the following hold:

- **R1. Name.** `child.name == parent.name`.
- **R2. Version.** Both `parent.version` and `child.version` MUST parse as SemVer (`InvalidVersion(_)` on failure). The refinement check then compares **only the numeric `(major, minor, patch)` tuple** and requires the child's tuple to be strictly greater. Prerelease and build metadata are validated as SemVer but contribute **nothing** to ordering — `1.0.0+a → 1.0.0+b`, `1.0.0-alpha.1 → 1.0.0-alpha.2`, and `1.0.0-rc.1 → 1.0.0` are all rejected with `VersionNotIncreased`. This closes the unbounded prerelease/build-metadata refinement-chain pollution vector.
- **R3. Input widens.** Child accepts a superset of parent inputs:
  - Same `Shape` variant.
  - Sized shapes (`string`, `bytes`): `child.max_bytes >= parent.max_bytes`.
  - Numeric shapes (`u8`/`u16`/`u32`/`u64`): same variant; bound `>=`.
  - `structured`: **`child.fields ⊆ parent.fields`** — child requires no fields the parent didn't already require. Each shared field is recursively input-widening.
  - `list`: child item shape input-widens parent item shape AND `child.max_items >= parent.max_items`.
- **R4. Output narrows.** Child output is substitutable for parent output (parent consumers stay satisfied):
  - Same `Shape` variant.
  - Sized/numeric: bound `<=`.
  - `structured`: **`child.fields ⊇ parent.fields`** — every field the parent promised is still produced by the child. Each shared field is recursively output-narrowing.
  - `list`: child item shape output-narrows parent item shape AND `child.max_items <= parent.max_items`.
- **R5. Effects.** `child.effects ⊆ parent.effects`. A child may not declare any side effect its parent did not declare.
- **R6. References.** Not gated. References are advisory.

#### Precedence: validation before refinement

Implementations **MUST** run descriptor validation before refinement. A child descriptor that fails validation is rejected as malformed; refinement is not consulted. The two checks are co-load-bearing: validation guards the type-system perimeter, refinement guards Liskov substitutability inside it.

Consequently, a malformed child surfaces as `MalformedDescriptor`, never as `NotARefinement`, even when both would apply.

#### Capacity overflow during refinement

One special case interacts with validation precedence: a child that **adds** a field to a structured output (legitimate under R4) may push the field-product past the universal 16 MiB capacity cap. Validation correctly rejects such a descriptor — but the underlying mutation is type-substitutable, not format-broken. Implementations **MUST** distinguish these:

- A capacity-overflowing child descriptor surfaces as `Rollback { rule_fired: "CapacityExceeded" }` — same shape as R1–R5 rejections, so agents route it like a refinement decision (don't promote) rather than a format error.
- Other validation failures (zero `max_bytes`, invalid identifier, unknown shape kind) still surface as `MalformedDescriptor`.

The recognizer is a stable error-message marker (`CAPACITY_EXCEEDED_TAG = "shape capacity"`) emitted by `validate_shape`; gates pattern-match on it and re-route accordingly. Fusion uses the same distinction: a fused descriptor that exceeds capacity surfaces as `FuseResult::CapacityExceeded` rather than `MalformedDescriptor`.

### Computation input

```json
{
  "parent_receipt":   "<base64 of the canonical receipt envelope>",
  "child_descriptor": <canonical-form child descriptor JSON object>
}
```

### Behavior

1. Decode `parent_receipt`; parse the envelope.
2. Re-verify the parent receipt (re-run its computation, compare `output_hash`, replay its seal). Failure → `ParentReceiptInvalid`.
3. Require `parent.computation_id ∈ {skill_interface_hash, next_generation}`. Else `InvalidParentComputation`.
4. Extract the parent descriptor from `parent.input` (root) or `parent.input.child_descriptor` (chain link).
5. Parse the child descriptor via the typed builder.
6. Call `is_refinement(&parent, &child)`. Failure → `NotARefinement(<reason>)`.
7. `child_interface_hash := skill_interface_hash(child_descriptor)` — the **raw 32 output bytes** of `blake3::hash` on the child's canonical bytes (never the hex form).
8. `parent.output_hash` is decoded from its 64-character hex representation to **32 raw bytes** before being fed to the hasher.
9. Output := `BLAKE3(b"EVOLVED" ‖ 0x00 ‖ parent_output_hash_bytes[32] ‖ 0x00 ‖ child_interface_hash_bytes[32])`.

### Non-claims

- **No execution trace.** The receipt proves type-shape refinement only, not behavioral equivalence.
- **No provenance.** Root authenticity is out of scope. A chain proves internal consistency, not that it descends from a trusted party.
- **Forks are permitted at the protocol level.** Two parties may independently mint distinct children of the same parent. Fork detection is a registry concern — `registry_snapshot` already chains snapshots via `prev_digest`.

## Three load-bearing guarantees

The protocol's value rests on three properties of the substrate. Each is provable by code or by measurement, not assertion.

### (1) Sealed construction proofs

`Certified<T>` from `uor-foundation` has private fields and no public constructor. The only way to obtain one is through the sanctioned pipeline (`run::<...>`) or its replay equivalent (`certify_from_trace`). The Rust compiler enforces this. `Attestation::seal` carries a `Certified<GroundingCertificate>`; user code cannot fabricate it, and therefore cannot fabricate an `Attestation` with a forged seal, even via the struct's public fields. **The gate is uncircumventable by construction, not by check.**

### (2) Deterministic canonicalization, substrate-bound

Every Lyra seal is `BLAKE3(runtime_ident ‖ 0x00 ‖ computation_label ‖ 0x00 ‖ canonical_bytes)` where `runtime_ident` is the implementation + substrate version string (e.g. `lyra-ref/0.1.0+uor-foundation/0.4.2`). This folds the substrate into the seal itself:

- **Same substrate, same inputs → bit-identical seal.** Verified by `tests/substrate.rs::same_process_two_seals_are_byte_identical` and by spawning a fresh `lyra` binary in `tests/substrate.rs::cross_process_seal_matches_in_process_seal` — independent processes produce byte-equal `output_hash` values.
- **Different substrate → visibly distinct seal.** Verification by a verifier with a different `runtime_ident` returns `SubstrateVersionMismatch`, not a silent failure. Verified by `tests/substrate.rs::receipt_with_foreign_runtime_is_rejected`.

The receipt envelope carries `runtime` as an explicit field, so the substrate identity is auditable on disk.

### (3) Type-decidability in microseconds

Refinement (R1–R6) and structural validation are decidable in microseconds because the substrate represents them as type-shape relationships, not as semantic equivalence checks.

Measured on `lyra-ref/0.1.0`, release build:

| Operation | Cost |
|---|---|
| `score skill_interface_hash` (root) | ~4.5 µs |
| `verify skill_interface_hash` (root) | ~2.9 µs |
| `score next_generation` (single link) | ~37 µs |
| `verify` of a length-1 chain | ~3.9 µs |
| `verify` of a length-5 chain | ~71 µs |
| `verify` of a length-10 chain | ~945 µs |
| `verify` of a length-25 chain | ~152 ms |

**Single-operation cost is solidly in microseconds**, fast enough to put in the inner loop of an LLM-driven evolution agent. Chain verification is currently O(N²) because v0.1 has no memoization — each link recursively re-verifies the chain back to the root. Memoization is on the v0.2 roadmap; until it lands, applications verifying long chains should cache verified parents themselves.

Reproduce with `cargo bench --bench lyra_bench`.

## What receipts DO and do NOT prove

This section is normative. Implementations and consumers must treat it as the contract.

### What a receipt proves

A valid receipt `{computation_id, input, output_hash, runtime, trace_b64}` proves:

1. The byte sequence in `input` was processed by an implementation matching `runtime` (or a `COMPATIBLE_RUNTIMES` predecessor).
2. The implementation produced exactly `output_hash` as the computation's output.
3. A sealed `Certified<...>` was minted by the sanctioned UOR pipeline over the *full 32 bytes* of `BLAKE3(runtime ‖ 0x00 ‖ computation_id ‖ 0x00 ‖ input)`.
4. The implementation's gate-level structural validation passed (shape vocabulary, effect vocabulary, name rules, content_hash format, capacity caps, refinement rules where applicable).

### What a receipt does **not** prove

These are out of scope by design. Implementations must not imply otherwise.

- **The skill body exists.** `SkillDescriptor.content_hash` is a **claim** by the descriptor author about the BLAKE3-256 of the skill body. The protocol treats it as opaque bytes: it goes into the canonical form and into the seal, but the protocol does *not* fetch the body or verify the hash matches anything. Body-integrity verification is the consumer's responsibility, layered above the descriptor.
- **The skill is correct.** The seal attests to type-shape conformance; it says nothing about whether the skill returns useful values. Two skills with identical shapes and different semantics (e.g., a Celsius-to-string formatter and a Fahrenheit-to-string formatter) are indistinguishable to the protocol.
- **A reference's named skill is current.** v0.1 pins references as `<name>@<64-hex-content_hash>`. A reference resolves only when both the name and the content_hash match a manifest entry; name-only matches are rejected. *However*, the protocol does not verify the resolved manifest entry's body exists.
- **The publisher is who they claim to be.** Receipts are signature-free. Authorship attestation is a separate layer (sign the receipt envelope with an external key).

### Receipt format and trust layers

The CLI wire-format `Receipt` is intentionally minimal — exactly four fields:

```json
{"computation_id": "...", "input": "...", "output_hash": "...", "runtime": "..."}
```

Forgery resistance rests on **BLAKE3-256 over the canonical input** (`output_hash`) plus the **deterministic computation** (anyone with the same runtime re-derives the same `output_hash`). The wire format intentionally does **not** carry a separate "seal" field. An earlier version did (`trace_b64`, a serialized UOR pipeline trace), but under the pipeline configuration this protocol uses, that trace's content fingerprint depends only on the term-arena *structure*, not on input values — every receipt produced the same trace bytes. Carrying it on the wire was misleading without adding security. The wire form now reflects the actual trust story.

**In-process Rust callers** who want a sealed UOR witness still get one. `gate::validate_skill` (and `check_composable`, `registry_snapshot`) returns:

```rust
pub struct Attestation {
    pub content_hash: [u8; 32],                    // BLAKE3 over canonical bytes
    pub seal:         Certified<GroundingCertificate>,  // sealed UOR type
}
```

The `Certified<...>` type cannot be constructed outside the sanctioned pipeline (the Rust compiler enforces this via sealed traits). It is a **type-level structural witness** — proof at the type system that the descriptor was admitted by the pipeline. It is *not* an input-bound cryptographic seal, and the wire form does not pretend it is.

**Two trust surfaces, made explicit:**

- **CLI / wire path** (`score`, `verify`): trust rests on BLAKE3 over the canonical input plus re-execution. A receipt verifies iff the same `(computation_id, input)` produces the same `output_hash` under a compatible `runtime`. No type-level witness crosses the wire.
- **Typed Rust API** (`gate::validate_skill`, `check_composable`, `registry_snapshot`): trust additionally includes a compile-time `Certified<GroundingCertificate>` value that a caller can only obtain through the sanctioned pipeline. This is useful inside a single process; it does not serialize.

**Parser strictness.** The descriptor JSON parser is intentionally narrow:

- Duplicate keys at any nesting level are rejected.
- List elements must be properly JSON-quoted strings; bare tokens (`[network_read]`) error rather than being coerced.
- Whitespace-only or empty references fail the pinned `name@<64-hex>` rule before name validation runs.
- Shape `max_bytes` must be ≥ 1 and ≤ `LYRA_MAX_BYTES`; structured-field product capacities are bounded via saturating `u128` arithmetic.

These rules close the smuggling paths that a permissive JSON reader would leave open — a producer cannot rely on lenient coercion to land a value that the typed API would later reject.

### Refinement and Liskov

The refinement rules R1–R6 (in the [Lineage receipts](#lineage-receipts-v01x) section) enforce **type substitutability** in the Liskov sense:

- **Inputs widen** (covariant on the parent's set of valid inputs): child accepts everything parent accepts.
- **Outputs narrow** (contravariant on the consumer's set of expected outputs): every value child produces satisfies parent's promised type.

Output narrowing is the *correct* Liskov rule. If parent promised `string max_bytes ≤ 1000` and child returns `string max_bytes ≤ 500`, every child output still satisfies parent's promise (a 500-byte value is ≤ 1000). A consumer that reads parent's output as a bounded string sees only valid values from child. This is what substitutability means.

What refinement does *not* enforce:

- **Capability stability.** If a consumer benchmarked against a 1000-byte parent output and depends on values often being near that maximum, switching to a 500-byte child changes observable distributions. That's a behavioral property; refinement is a type property. Operators who need capability stability must layer their own check (e.g., conformance test vectors bundled with the descriptor).
- **Semantic equivalence.** A child can preserve types and silently change algorithm. Refinement allows this; behavioral conformance is a separate layer.

### Composability

`check_composable` returns `COMPATIBLE` iff a producer's `output_shape` is type-substitutable for a consumer's `input_shape`. It is a **shape check**, not a semantic check. Two skills with type-compatible shapes can still be semantically incompatible (Celsius vs Fahrenheit). Operators must not infer semantic safety from a `COMPATIBLE` result.

#### `at_step` semantics on chain failures

When `skill_compose` reports a failure on an N-skill chain, the `at_step` field indexes the failure differently depending on the failure mode:

- `status: "incompatible"` — `at_step` is the **transition index**, i.e. the edge `skills[at_step] → skills[at_step + 1]` failed. The producer's array position is `at_step`; the consumer's is `at_step + 1`. For a 2-element chain this is always `0`. For a 3-element chain, the second edge (`skills[1] → skills[2]`) reports `at_step = 1`, not `2`.
- `status: "malformed_descriptor"` — `at_step` is the **array index of the bad element itself**. A parse failure has no edge to attribute to, so the value points at the offending skill directly.

Agents consuming `at_step` to render an error should branch on `status` to choose the right interpretation.

### Substrate upgrades

A receipt's `runtime` field identifies the implementation + substrate that produced it. Verifiers reject runtimes that are not the current build's `LYRA_RUNTIME_IDENT` and not in `COMPATIBLE_RUNTIMES`. The `COMPATIBLE_RUNTIMES` list is the migration path: when a substrate update is byte-equivalent for canonical bytes and pipeline output, the new build adds the old ident to the list, so older receipts continue to verify. When a substrate update *is not* byte-equivalent, old receipts are explicitly rejected with `SubstrateVersionMismatch` — silent divergence is not possible.

## Known limitations (v0.1)

- **CLI and typed paths share validation but not encoders.** The typed Rust API and the CLI's JSON path are two implementations of the same protocol. They share `validate_name` and field-level rules, but the CLI keeps its own canonical-form encoder. v0.2 may collapse them into one path.
- **Not yet on crates.io.** Install via `cargo install --git https://github.com/ZiriaLabs/hermes-lyra` until a stable release is cut.
- **JSON-LD `@context` is minimal.** `to_jsonld` emits `{"@context": {"type": "lyra:SkillDescriptor"}}` — enough to mark a document as a Lyra descriptor, but not yet a full vocabulary mapping under `https://lyra-protocol.org/ontology/v0.1/`.

## Versioning

The protocol is versioned independently of any underlying substrate. Future versions may widen the type vocabulary or relax bounds only via explicit standard revision and a `@context` IRI bump.
