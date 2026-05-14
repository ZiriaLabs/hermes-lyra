---
name: inbox-triage
contract: {"content_hash":"bc72ae24c1227cdf5c74eb5a26b527569e3f91177875b322aca3cf3d8ea20ff2","effects":["llm"],"input_shape":{
    "type": "structured",
    "fields": [
      {"name": "emails", "shape": {
        "type": "list",
        "max_items": 4,
        "item": {
          "type": "structured",
          "fields": [
            {"name": "from",    "shape": {"type": "string", "max_bytes": 32}},
            {"name": "subject", "shape": {"type": "string", "max_bytes": 64}},
            {"name": "body",    "shape": {"type": "string", "max_bytes": 2048}}
          ]
        }
      }}
    ]
  },"name":"inbox-triage","output_shape":{
    "type": "structured",
    "fields": [
      {"name": "summaries", "shape": {
        "type": "list",
        "max_items": 4,
        "item": {
          "type": "structured",
          "fields": [
            {"name": "from",    "shape": {"type": "string", "max_bytes": 32}},
            {"name": "subject", "shape": {"type": "string", "max_bytes": 64}},
            {"name": "summary", "shape": {"type": "string", "max_bytes": 256}}
          ]
        }
      }},
      {"name": "priority", "shape": {"type": "u8", "max_bytes": 1}}
    ]
  },"references":[],"schema":"bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju","version":"0.1.0"}
proof:    {"protocol":"hermes-lyra/0.3","output_cid":"bafkr4icrowvqin7ssusit3stumyugd5evjcskmlazdbj66neibm6ot3vey","runtime":"hermes-lyra/0.3.0+uor-foundation/0.4.2"}
---

# inbox-triage

*The tuning string.* Two skills that share a typed contract are in tune; the one that consumes this one cannot be surprised.

A cron pipeline runs unattended: model summarizes inbox, downstream skill posts to Slack. The failure mode is silent — a model swap or a skill update renames `summary` to `body`, the poster keeps running, and the channel gets blank messages for a week before anyone notices.

`inbox-triage` is the Lyra-typed version of that summarizer.

## What it does

Takes a list of emails (`from`, `subject`, `body`) and returns a list of one-line summaries plus a single `priority` byte (0–255). That is the entire contract. The shape is fixed in [`skill.lyra.json`](skill.lyra.json) and enforced by the protocol.

## Why a Lyra contract

Three concrete properties the cron pipeline gets for free:

1. **Determinism per run.** `lyra score skill_interface_hash skill.lyra.json` is byte-identical across machines and across days. The Slack poster can cache on the hash; identical batches don't re-post.
2. **Shape lock.** The downstream skill declares `inbox-triage`'s `output_shape` as its `input_shape` and Lyra's `check_composable` rejects any future upgrade that drops `summary` or renames `priority`. Drift fails closed at compose time, not in production.
3. **Effect budget.** `effects: ["llm"]` is declared up front. A refinement that adds `network_write` cannot mint a `next_generation` receipt under R5 — so a quietly added webhook can never sneak through.

## How Hermes uses it

Drop this directory into your agent's skills folder. The agent reads `SKILL.md` as a regular skill; `skill.lyra.json` is read by whatever downstream skill wants to verify the contract — typically the Slack/Telegram/Discord poster declared as its consumer.

Receipt-on-cron:

```bash
# Inside the cron job, after the model returns its output:
lyra score skill_interface_hash "$(cat skill.lyra.json)" > /var/log/lyra/$(date +%F).json
```

The log directory is a deterministic audit trail. Any reviewer can replay a day's receipt against the day's descriptor and confirm nothing changed.
