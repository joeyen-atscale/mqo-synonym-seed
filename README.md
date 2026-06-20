# mqo-synonym-seed

Turn cryptic measure names into the words people actually ask for, so an AI agent can find them.

## Why it exists

A semantic model names a measure `ORDER_AMT`. A user asks for "total sales" or "revenue." Those are the same number, but nothing in the model says so, and an agent retrieving over column names has no way to bridge the gap. The description field is where that bridge lives — and on most models it's empty.

Writing those synonyms by hand across a whole model is tedious enough that it doesn't get done. But the names themselves carry most of the signal: `_AMT` means amount, `AVG_` means average, `ROLLING_7D_` means a trailing weekly window. `mqo-synonym-seed` reads those conventions and proposes the natural-language phrases a person would use, leaving you to approve them rather than invent them.

It seeds; it does not decide. Generation and application are separate steps with a human in between.

## Install

Requires Rust 1.85+.

```sh
cargo install --path .
```

Or build in place:

```sh
cargo build --release   # binary at target/release/mqo-synonym-seed
```

## Quickstart

The input is a `describe_model` JSON file — a list of columns, each with a `name` and an optional `description`:

```json
{
  "columns": [
    {"name": "ORDER_AMT",       "type": "measure", "description": ""},
    {"name": "AVG_ORDER_VALUE", "type": "measure", "description": ""},
    {"name": "DISCOUNT_PCT",    "type": "measure", "description": "Discount applied at checkout"}
  ]
}
```

**Generate candidates** for the columns that still lack a description:

```sh
mqo-synonym-seed generate --model model.json
```

```json
{
  "candidates": [
    {
      "name": "ORDER_AMT",
      "existing_description": "",
      "synonyms": ["order amt", "order amount", "order total", "order sum", "gross order", "total sales", "revenue"]
    },
    {
      "name": "AVG_ORDER_VALUE",
      "existing_description": "",
      "synonyms": ["avg order value", "average order value", "mean order value", "typical order value", "avg order amount", "avg order total", "average avg order size", "mean avg order value"]
    }
  ]
}
```

`DISCOUNT_PCT` is skipped — it already has a description. Pass `--all` to generate for every column regardless, and `--format table` for a scannable view:

```sh
mqo-synonym-seed generate --model model.json --all --format table
```

```
NAME             SYNONYMS
-----------------------------------------------------------------------------
ORDER_AMT        order amt; order amount; order total; order sum; gross order; total sales; revenue
AVG_ORDER_VALUE  avg order value; average order value; mean order value; ...
DISCOUNT_PCT     discount pct; discount percentage; discount rate; discount ratio; discount share
```

**Review, then apply.** Keep the synonyms you want in an approval file keyed by column name:

```json
{ "ORDER_AMT": ["total sales", "revenue"] }
```

```sh
mqo-synonym-seed apply --model model.json --approved approved.json --out model.enriched.json
```

`apply` writes the approved phrases into each column's `description`. An empty description is set to the synonyms; a non-empty one keeps its text and gets the synonyms appended after a `;`. Columns not in the approval file are left untouched. Omit `--out` to write to stdout.

## How it works

The engine is a set of naming-convention rules over the column name. It splits on underscores, recognizes common measure suffixes and prefixes, and emits the phrases a person would use for each:

| Pattern | Examples it recognizes | Phrases it adds |
| --- | --- | --- |
| `_AMT` | `ORDER_AMT` | amount, total, sum, gross, total sales, revenue |
| `_QTY` | `LINE_QTY` | quantity, count, volume, units |
| `_PCT` / `_RATE` | `DISCOUNT_PCT` | percentage, rate, ratio, share |
| `_USD` / `_COST` | `SHIPPING_COST` | cost, spend, in dollars, dollar amount |
| `_FLAG` / `IS_` | `IS_ACTIVE` | indicator, flag, boolean, yes/no |
| `AVG_` / `_AVG` | `AVG_BASKET_SIZE` | average, mean, typical |
| `_VALUE` | `AVG_ORDER_VALUE` | value, amount, total, average size |
| `_SALES` | `NET_SALES` | sales, revenue, bookings |
| `ROLLING_<window>_` | `ROLLING_7D_AVG_SALES` | N-day / weekly rolling (trailing) average |

Output is deduplicated and whitespace-normalized. The rules compose: `ROLLING_7D_AVG_SALES` picks up the rolling-window, average, and sales phrasings together.

This is deterministic and offline. The `--planner-brain` flag on `generate` is a placeholder for an LLM-backed path that would produce richer phrasing; that integration is not implemented yet. The flag is accepted, prints a warning, and falls back to the rule-based path, so a pipeline that already passes it keeps working.

## Serve mode

`mqo-synonym-seed serve` runs a JSON-RPC 2.0 server over stdio for embedding in a pipeline. It speaks two methods, `generate` and `apply`, with the same shapes as the CLI:

```sh
echo '{"jsonrpc":"2.0","id":1,"method":"generate","params":{"columns":[{"name":"ORDER_AMT","description":""}]}}' \
  | mqo-synonym-seed serve
```

```json
{"id":1,"jsonrpc":"2.0","result":{"candidates":[{"name":"ORDER_AMT","existing_description":"","synonyms":["order amt","order amount","order total","order sum","gross order","total sales","revenue"]}]}}
```

`generate` over the wire processes all columns (it has no `--all` toggle); `apply` takes `{"model": ..., "approved": ...}`.

## Where it fits

This is an MQO-family tool. The AI-retrieval problem it addresses — agents finding the right governed measure from a natural-language ask — is the same one the MQO query pipeline (`mqo-mcp`) works downstream. Synonyms seeded here land in the model descriptions those tools read.

## Status

v0.1.0. The rule-based generate/apply/serve paths are complete and tested; `--planner-brain` is a declared-but-unimplemented stub, as noted above.
