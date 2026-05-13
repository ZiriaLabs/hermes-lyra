# Changelog

All notable changes to `hermes-lyra` (the reference implementation living at `lyra-ref`) are recorded here.

The protocol identifier in proofs (`proof.protocol`) maps to a particular wire-format/canonical-bytes contract. Bumping the protocol minor invalidates older proofs unless the new version explicitly lists the older one in `COMPATIBLE_RUNTIMES`.

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
- `docs/specification.md` updated: protocol identifier, spec URI, MCP tool names.
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
