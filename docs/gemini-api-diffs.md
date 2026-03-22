# Gemini API Differences from OpenAI

Research deliverable for Phase 20. Documents how Google's Gemini APIs differ from OpenAI's Chat Completions API and what translation work is required.

## Recommendation

**Implement Vertex OpenAI-compatible mode first (Phase 20a).** Vertex AI exposes `projects.locations.endpoints.openapi` with Chat Completions-compatible endpoints. The existing Anthropic-to-OpenAI translation layer works as-is with different auth, base URL, and model names.

Build native Gemini translation (Phases 20c-20g) only if Vertex OpenAI-compatible mode has gaps in tool calling, streaming, or schema handling.

## Two API Surfaces

Google exposes Gemini through two surfaces:

| Surface | Base URL | Auth | Model format |
|---|---|---|---|
| Gemini Developer API | `generativelanguage.googleapis.com/v1beta` | API key (`x-goog-api-key` header or `?key=` param) | `models/{model}` |
| Vertex AI | `{region}-aiplatform.googleapis.com` | OAuth bearer token (ADC / service account) | `projects/{p}/locations/{l}/publishers/google/models/{m}` |

Vertex also has an **OpenAI-compatible endpoint** at `projects.locations.endpoints.openapi` that accepts Chat Completions format directly.

## Endpoint Mapping

| Capability | OpenAI | Gemini Dev API | Vertex AI |
|---|---|---|---|
| Non-streaming | `POST /chat/completions` | `POST /{model}:generateContent` | `POST .../models/{model}:generateContent` |
| Streaming | SSE from `/chat/completions` | `POST /{model}:streamGenerateContent?alt=sse` (SSE) | Stream of `GenerateContentResponse` |
| Embeddings | `POST /embeddings` | `POST /{model}:embedContent` | `POST .../models/*:embedContent` |
| Token counting | N/A | `POST /{model}:countTokens` | `POST .../models/*:countTokens` |
| Batch | JSONL file + batch job | `:batchGenerateContent` | Vertex batch prediction |

## Content Model

OpenAI uses `messages[]` with role + content. Gemini uses `contents[]` with `parts[]`.

**OpenAI message:**
```json
{"role": "user", "content": "Hello"}
```

**Gemini content:**
```json
{"role": "user", "parts": [{"text": "Hello"}]}
```

### Parts Union

A Gemini `Part` can carry:
- `text`: plain text
- `inlineData`: `{mimeType, data}` (base64 bytes)
- `fileData`: `{mimeType, fileUri}` (GCS URI for Vertex, Files API URI for Dev)
- `functionCall`: `{name, args}` (tool invocation)
- `functionResponse`: `{name, response}` (tool result)

OpenAI uses separate message types or structured content arrays for equivalent functionality.

## Roles and System Instructions

| Concept | OpenAI | Gemini |
|---|---|---|
| System prompt | `system` role in messages | `systemInstruction` field (separate from `contents[]`) |
| Developer instructions | `developer` role | No equivalent (fold into `systemInstruction`) |
| User | `user` | `user` |
| Assistant | `assistant` | `model` |

Gemini `Content.role` is restricted to `user` or `model`. The `systemInstruction` field only allows text parts.

**Turn alternation:** Gemini may enforce strict user/model alternation. Consecutive same-role messages must be merged.

**Translation:** Anthropic `system` -> Gemini `systemInstruction.parts[].text`. Anthropic `assistant` -> `model`. If both system and developer-style instructions exist, concatenate into `systemInstruction`.

## Tool Calling

| Aspect | OpenAI | Gemini |
|---|---|---|
| Declaration | `tools[].function.parameters` (JSON Schema) | `tools[].function_declarations[].parameters` (OpenAPI 3.0 subset) |
| Max declarations | Not documented hard limit | **128 per request** |
| Call format | `tool_calls[].function.{name, arguments}` (arguments is JSON string) | `Part.functionCall.{name, args}` (args is JSON object) |
| Result format | Tool role message with `tool_call_id` | `Part.functionResponse.{name, response}` |
| Streaming calls | Delta-based assembly of arguments string | `partialArgs[]` with `willContinue` flag |
| Choice | `tool_choice: auto/none/required/{function}` | `toolConfig.functionCallingConfig.mode: AUTO/NONE/ANY` |

### Schema Restrictions

Gemini rejects certain JSON Schema constructs. Tool parameter schemas must be sanitized:

**Must strip:** `$schema`, `$ref`, `$defs`, `definitions`, `default`, `pattern`, `examples`

**Must rewrite:** `anyOf`/`oneOf` -> flatten to first variant or `string` fallback

**Must validate:** Only Gemini-supported `format` values (e.g., `int32`, `float`, `date-time`)

This is critical: the `claude-code-proxy` project implements `clean_gemini_schema()` for exactly this reason.

## Generation Parameters

| Parameter | OpenAI | Gemini |
|---|---|---|
| Max output | `max_tokens` / `max_completion_tokens` | `maxOutputTokens` |
| Temperature | `temperature` | `temperature` (range `(0.0, 2.0]`) |
| Top-p | `top_p` | `topP` |
| Top-k | Not always present | `topK` |
| Candidates | `n` | `candidateCount` |
| Stop sequences | `stop` (string or array) | `stopSequences` (array) |
| Seed | `seed` | `seed` ("mostly deterministic") |
| Presence penalty | `presence_penalty` | `presencePenalty` |
| Frequency penalty | `frequency_penalty` | `frequencyPenalty` |
| JSON output | `response_format` | `responseMimeType` + `responseSchema` |

## Streaming

### Gemini Developer API

Uses SSE (`streamGenerateContent?alt=sse`). Each SSE event contains a full `GenerateContentResponse` with incremental content. Parse with standard SSE framing (`data:` lines separated by `\n\n`).

### Vertex AI

Documented as a "stream of `GenerateContentResponse` instances." Not explicitly described as SSE in REST reference. May use different HTTP framing.

### OpenAI

SSE with typed chunk objects. Each chunk has `choices[].delta` with incremental content.

**Key difference:** OpenAI streams deltas (partial content). Gemini streams full response objects with accumulated content. The streaming state machine must account for this (Gemini: take last part, OpenAI: accumulate deltas).

## Authentication

| Surface | Mechanism | Header/Param |
|---|---|---|
| OpenAI | API key | `Authorization: Bearer sk-...` |
| Gemini Dev API | API key | `x-goog-api-key: ...` or `?key=...` |
| Vertex AI | OAuth token | `Authorization: Bearer $(gcloud auth print-access-token)` |
| Gemini Live WS | API key | `?key=...` in WebSocket URL |

## Model Naming

Claude model names do not exist in Google's ecosystem. Mapping is required:

| Pattern | Target |
|---|---|
| Contains "haiku" | `gemini-2.5-flash` (small/fast) |
| Contains "sonnet" | `gemini-2.5-pro` (balanced) |
| Contains "opus" | `gemini-2.5-pro` (best available) |
| Unrecognized | Passthrough with warning |

Phase 15 (configurable model mapping) builds the infrastructure for this.

## Safety Settings

Gemini/Vertex supports per-request `safetySettings[]` with `{category, threshold}`. Categories: hate speech, dangerous content, harassment, sexually explicit. Thresholds: `BLOCK_LOW_AND_ABOVE`, `BLOCK_MEDIUM_AND_ABOVE`, `BLOCK_ONLY_HIGH`, `BLOCK_NONE`, `OFF`.

No direct OpenAI equivalent. Safety rejections should map to a normalized error category, not a generic 400.

## File Uploads

| Feature | OpenAI | Gemini Dev API | Vertex AI |
|---|---|---|---|
| Upload | Uploads API (64MB parts, 8GB total) | Files API (Google resumable upload) | GCS URIs |
| Reference | File ID in messages | `file.uri` in `fileData` part | GCS URI in `fileData.fileUri` |
| Headers | Standard multipart | `X-Goog-Upload-Protocol: resumable`, `X-Goog-Upload-Command` | N/A (use gsutil/GCS client) |

## Error Handling

Gemini errors are not fully documented in the same structured way as OpenAI (`error.message/type/param/code`). Rate limit errors trigger on RPM, TPM (input), and RPD dimensions (OpenAI has 5 dimensions: RPM, RPD, TPM, TPD, IPM).

## Rate Limits

| Dimension | OpenAI | Gemini Dev API |
|---|---|---|
| RPM | Yes | Yes |
| RPD | Yes | Yes |
| TPM | Yes | Yes (input only) |
| TPD | Yes | Not documented |
| IPM | Yes (images) | Not documented |

## Batch Processing

OpenAI: JSONL input file (up to 50,000 requests, 200MB), uploaded with `purpose: batch`.

Gemini: `batchGenerateContent` with inlined requests or file-backed. Outputs JSONL responses preserving order.

## References

- Gemini Developer API: `generativelanguage.googleapis.com/v1beta`
- Vertex AI Gemini: `{region}-aiplatform.googleapis.com`
- `claude-code-proxy` schema sanitizer: `clean_gemini_schema()` pattern for JSON Schema subset restrictions
- `claude-code-via-antigravity`: Gemini backend with role mapping and tool schema stripping
