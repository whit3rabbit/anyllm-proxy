---
name: openai-schema
description: This skill queries the OpenAI OpenAPI spec (~26k lines, 148 endpoints, 477 schemas) without loading the full file into context. Use when implementing or modifying Anthropic-to-OpenAI translation logic, checking OpenAI API field names/types, verifying schema shapes, or comparing request/response structures. Triggers on questions about OpenAI API structure, field types, endpoint parameters, or schema definitions.
---

# OpenAI API Schema Query

Query the OpenAI OpenAPI spec (v2.3.0, 148 endpoints, 477 schemas) for specific endpoints, schemas, and fields. The full spec is ~1.4MB/26k lines; this skill provides targeted lookups without loading it all into context.

## Setup

The query script requires PyYAML via the project venv:

```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py <command>
```

If the venv does not exist:
```bash
python3 -m venv scripts/.venv && scripts/.venv/bin/pip install pyyaml
```

The spec is auto-downloaded and cached to `scripts/.cache/openapi.yaml` on first run.

## Commands

### List available API tag groups
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py tags
```

### List endpoints (optionally filter by tag or path)
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py endpoints --filter chat
```

### Show endpoint detail by operationId or path
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py endpoint createChatCompletion
```
Returns: method, path, tags, summary, parameters, request body ref, response refs.

### Show schema detail with configurable depth
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py schema CreateChatCompletionRequest --depth 2
```
- `--depth 0`: schema composition only (allOf/oneOf refs)
- `--depth 1` (default): properties with types and refs
- `--depth 2`: expand referenced schemas one level deeper

Supports case-insensitive substring matching: `schema chatcompletion` finds all matching schemas.

### Search by regex pattern
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py search "tool" --deep
```
Searches schema names, endpoint paths, and operationIds. `--deep` also searches property names within schemas.

### Update cached spec
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py update
```
Re-downloads the spec from GitHub, replacing the local cache. Reports version, endpoint count, and schema count.

### Compare schema fields
```bash
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py diff CreateChatCompletionResponse
scripts/.venv/bin/python3 .Codex/skills/openai-schema/scripts/query-openai-schema.py diff CreateChatCompletionRequest --compare CreateChatCompletionResponse
```
Without `--compare`: lists all fields. With `--compare`: shows shared, source-only, and target-only fields.

## Condensed spec generation

For bulk output (e.g., to include in context or save to file), use the condenser script:

```bash
# Chat endpoints only (~700 lines)
scripts/.venv/bin/python3 scripts/condense-openapi.py --tag Chat -o docs/openai-spec-condensed.yaml

# Chat + Responses (~2000 lines)
scripts/.venv/bin/python3 scripts/condense-openapi.py --tag Chat --tag Responses -o docs/openai-spec-condensed.yaml

# Full spec (~11k lines)
scripts/.venv/bin/python3 scripts/condense-openapi.py -o docs/openai-spec-condensed.yaml
```

## Workflow

When working on translation logic in this project:

1. **Identify the OpenAI type** to look up (check `crates/translator/src/openai/` for the Rust type name)
2. **Query the schema** to get the canonical field list, types, and required fields
3. **Compare** with the Rust struct to find gaps or mismatches
4. **Check the endpoint** if parameter or request body structure is unclear

For translation gap analysis, use `diff --compare` to compare request vs response schemas, or compare the OpenAI schema fields against the Anthropic types in `crates/translator/src/anthropic/`.

## Available tags

Assistants, Audio, Audit Logs, Batch, Certificates, Chat, Completions, Embeddings, Evals, Files, Fine-tuning, Images, Invites, Models, Moderations, Projects, Realtime, Responses, Uploads, Usage, Users, Vector stores.
