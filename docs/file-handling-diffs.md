# File and Document Handling Differences

## Anthropic Document Blocks

Anthropic supports inline documents in message content:
```json
{
  "type": "document",
  "source": {
    "type": "base64",
    "media_type": "application/pdf",
    "data": "<base64>"
  },
  "title": "report.pdf"
}
```

Also supports image blocks with base64 or URL sources.

## OpenAI File Handling

### Chat Completions
- Supports `image_url` content parts (URL or data URI)
- No inline document/PDF support
- Files must be uploaded separately via `/v1/files` API

### Responses API
- Supports `input_file` with `file_data` (base64), `file_id`, or `file_url`
- This would be the ideal target for document translation

## Current Translation

| Anthropic Block | OpenAI Translation | Fidelity |
|---|---|---|
| Image (base64) | `image_url` with data URI | Full |
| Image (URL) | `image_url` with URL | Full |
| Document (base64 PDF) | Text note placeholder | Lossy |

### Document Translation Detail

Since the proxy targets Chat Completions (not Responses), documents are converted to a text note:
```
[Attached report.pdf: application/pdf (12345 bytes base64)]
```

This is a known limitation. Full document support would require targeting the Responses API with `input_file.file_data`.

## Size Limits

| Provider | Limit |
|---|---|
| Anthropic Messages | 32 MB |
| Anthropic Batch | 256 MB |
| Proxy | 32 MB (enforced at edge) |
