---
name: news-brief
contract: {"content_hash":"f27640ecc5f34a6107bcd6e0906026d499ff9784afe3d817c45ef823e6554a4e","effects":["web_read", "llm"],"input_shape":{
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
    "bafkr4idtjwypfi4rrrhkzciuvexfxehrv4zgvnylzxgr7lhuyimcfdmene",
    "bafkr4ihjjst6s2zemp7qmufq7c26dg67lqnm3wruko2mdnaeqtpppktbo4"
  ],"schema":"bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju","version":"0.1.0"}
proof:    {"protocol":"hermes-lyra/0.3","output_cid":"bafkr4iamifowvi5ocop6seo72v72g2evub6pgikd7q67zccgk4uiqjuv34","runtime":"hermes-lyra/0.3.0+uor-foundation/0.4.2"}
---

# news-brief

*The exchange string.* Hermes gave the lyre to Apollo as proof of fair exchange. This skill pins what it depends on by content hash so the exchange between sub-skills stays fair across machines.

A daily cron typically wires three skills: a URL fetcher, an LLM classifier that scores urgency, and a router that posts to a channel. The pain is that any of the three can be silently upgraded — and on the morning that happens, every channel gets a differently-shaped brief and there is no record of why.

`news-brief` is the Lyra-typed parent skill that **pins** its dependencies.

## What it does

Takes a list of source URLs and a look-back window in hours. Returns a list of items, each with `title`, `url`, `urgency` (0–255), and a one-paragraph summary. The shape is locked in [`skill.lyra.json`](skill.lyra.json).

## Why a Lyra contract

The load-bearing field is `references`. Each entry is the envelope CID of another SKILL.md — the same string `lyra cid <file>` emits:

```json
"references": [
  "bafkr4idtjwypfi4rrrhkzciuvexfxehrv4zgvnylzxgr7lhuyimcfdmene",
  "bafkr4ihjjst6s2zemp7qmufq7c26dg67lqnm3wruko2mdnaeqtpppktbo4"
]
```

Three properties this gives the multi-channel pipeline:

1. **Reproducibility across machines.** A teammate cloning the workflow gets the *same brief* because each sub-skill is pinned by its envelope CID. Two SKILL.md files with the same prose but different contracts have different CIDs; substitution is impossible without changing the reference.
2. **Opt-in upgrades.** When the user wants to move to a different `llm-classify` version, they paste the new file's CID into `references` and the brief's `content_hash` changes — a visible event in any Git diff or audit log.
3. **Universal addressing.** Each CID is a multiformats-standard reference. Any tool that can resolve a CID (kubo, an HTTP gateway, a content store) can resolve a Lyra reference — no dialect, no parallel naming scheme, no `@` prefix.

## How Hermes uses it

Cron triggers Hermes, Hermes invokes `news-brief`. Before the post-to-channel step, the gateway calls:

```bash
lyra score skill_reference_resolve "$(cat brief-with-manifest.json)"
```

If any pinned sub-skill is missing or its hash has drifted, the brief is not sent. The channel gets nothing rather than getting a wrongly-shaped message — exactly the failure mode the user stories complain about.

A v0.2 of `news-brief` that wants to add a `language` field to each item bumps to `0.2.0`, declares its parent, and mints a `next_generation` receipt. R4 (output narrows) requires the new field be additive; dropping `title` would be rejected.
