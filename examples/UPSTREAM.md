# Upstream attribution

Six of the nine example skills in this directory mirror skills shipped in the
canonical [NousResearch/hermes-agent][hermes-repo] repository under
`skills/github/`. Their authoring (frontmatter metadata, procedural bodies,
bash recipes) is the work of the upstream Hermes Agent maintainers and is
included here verbatim under the MIT license they ship with.

Each affected `SKILL.md` keeps the upstream `name`, `description`, `version`,
`author`, `license`, `platforms`, `metadata`, and `prerequisites` fields
unchanged so the file is a drop-in replacement for the upstream — a Hermes
Agent installation can load it as a normal skill with no special handling.

The only addition is two lines inside the YAML frontmatter, both inline JSON:

```
contract: { ... typed Lyra contract ... }
proof:    { protocol, output_cid, runtime }
```

These declare the Lyra-typed boundary of the skill and seal the entire file
under a single envelope CID. Lyra-aware tooling can verify the typed
contract; Lyra-unaware tooling (the Hermes Agent loader) sees them as
opaque YAML keys and ignores them.

## Mirrored skills

| Lyra example                | Upstream                                                              |
|-----------------------------|-----------------------------------------------------------------------|
| `codebase-inspection/`      | [`skills/github/codebase-inspection/`][src-codebase]                  |
| `github-auth/`              | [`skills/github/github-auth/`][src-auth]                              |
| `github-code-review/`       | [`skills/github/github-code-review/`][src-review]                     |
| `github-issues/`            | [`skills/github/github-issues/`][src-issues]                          |
| `github-pr-workflow/`       | [`skills/github/github-pr-workflow/`][src-pr]                         |
| `github-repo-management/`   | [`skills/github/github-repo-management/`][src-repo]                   |

Only the top-level `SKILL.md` of each upstream skill is mirrored. Sub-files
that some upstream skills ship (`scripts/`, `templates/`, `references/`) are
not copied; if you need them, fetch them directly from the upstream repo.

Bidirectional compatibility is verified at every build:

- **Hermes side** — a strict YAML parser reads every required field
  (`name`, `description`, `version`, `author`, `license`, `platforms`,
  `prerequisites`) correctly. Hermes loads the skill as it would any other.
- **Lyra side** — `lyra verify SKILL.md` returns `{"status":"valid"}`. A
  single byte flipped anywhere in the file (frontmatter, procedural body,
  bash recipes) outside the `proof:` line surfaces as `{"status":"mismatch"}`.

The remaining three examples (`inbox-triage`, `news-brief`,
`code-review-evolve`) are Lyra-originated demonstrations of the contract
layer and are MIT-licensed by this repo.

[hermes-repo]:   https://github.com/NousResearch/hermes-agent
[src-codebase]:  https://github.com/NousResearch/hermes-agent/tree/main/skills/github/codebase-inspection
[src-auth]:      https://github.com/NousResearch/hermes-agent/tree/main/skills/github/github-auth
[src-review]:    https://github.com/NousResearch/hermes-agent/tree/main/skills/github/github-code-review
[src-issues]:    https://github.com/NousResearch/hermes-agent/tree/main/skills/github/github-issues
[src-pr]:        https://github.com/NousResearch/hermes-agent/tree/main/skills/github/github-pr-workflow
[src-repo]:      https://github.com/NousResearch/hermes-agent/tree/main/skills/github/github-repo-management
