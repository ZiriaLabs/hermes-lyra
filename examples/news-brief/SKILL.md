---
name: news-brief
contract: {"content_hash":"609da164c8501de0db10b53db9529b1a8b9485b61a4ce700ade6638ae2128fdb","effects":["web_read", "llm"],"input_shape":{
    "type": "structured",
    "fields": [
      {"name": "sources", "shape": {
        "type": "list",
        "max_items": 8,
        "item": {"type": "string", "max_bytes": 256}
      }},
      {"name": "hours_back", "shape": {"type": "u8", "max_bytes": 1}}
    ]
  },"name":"news-brief","output_shape":{
    "type": "structured",
    "fields": [
      {"name": "items", "shape": {
        "type": "list",
        "max_items": 8,
        "item": {
          "type": "structured",
          "fields": [
            {"name": "title",   "shape": {"type": "string", "max_bytes": 64}},
            {"name": "url",     "shape": {"type": "string", "max_bytes": 128}},
            {"name": "urgency", "shape": {"type": "u8",     "max_bytes": 1}},
            {"name": "summary", "shape": {"type": "string", "max_bytes": 256}}
          ]
        }
      }}
    ]
  },"references":[
    "url-fetch@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "llm-classify@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  ],"version":"0.1.0"}
proof:    {"protocol":"hermes-lyra/0.2","spec_uri":"https://github.com/ZiriaLabs/hermes-lyra","output_hash":"4d9dd671bb3aa265a6c3897ca3ece66be86e8c3c83bb5dcc33108c6680e84ae5","runtime":"hermes-lyra/0.2.0+uor-foundation/0.4.2"}
---

# news-brief

*The exchange string.* Hermes gave the lyre to Apollo as proof of fair exchange. This skill pins what it depends on by content hash so the exchange between sub-skills stays fair across machines.

A daily cron typically wires three skills: a URL fetcher, an LLM classifier that scores urgency, and a router that posts to a channel. The pain is that any of the three can be silently upgraded — and on the morning that happens, every channel gets a differently-shaped brief and there is no record of why.

`news-brief` is the Lyra-typed parent skill that **pins** its dependencies.

## What it does

Takes a list of source URLs and a look-back window in hours. Returns a list of items, each with `title`, `url`, `urgency` (0–255), and a one-paragraph summary. The shape is locked in [`skill.lyra.json`](skill.lyra.json).

## Why a Lyra contract

The load-bearing field is `references`. Each entry has the form `<name>@<64-hex-content-hash>`:

```json
"references": [
  "url-fetch@aaaaaa...aaaa",
  "llm-classify@bbbbbb...bbbb"
]
```

Three properties this gives the multi-channel pipeline:

1. **Reproducibility across machines.** A teammate cloning the workflow gets the *same brief* because both sub-skills are pinned by content hash. Name-only matches are rejected by `skill_reference_resolve` (R-S4 in the protocol).
2. **Opt-in upgrades.** When the user wants to move to `llm-classify@cccc...`, they update one line in `references` and the brief's `content_hash` changes — a visible event in any Git diff or audit log.
3. **Manifest-rooted snapshot.** `lyra score merkle_manifest` over `[news-brief, url-fetch, llm-classify]` produces a single 32-byte Merkle root that summarizes the entire skill graph for a given day. Two users on different continents who share the root are running the same brief; they can compare without comparing each skill.

## How Hermes uses it

Cron triggers Hermes, Hermes invokes `news-brief`. Before the post-to-channel step, the gateway calls:

```bash
lyra score skill_reference_resolve "$(cat brief-with-manifest.json)"
```

If any pinned sub-skill is missing or its hash has drifted, the brief is not sent. The channel gets nothing rather than getting a wrongly-shaped message — exactly the failure mode the user stories complain about.

A v0.2 of `news-brief` that wants to add a `language` field to each item bumps to `0.2.0`, declares its parent, and mints a `next_generation` receipt. R4 (output narrows) requires the new field be additive; dropping `title` would be rejected.
