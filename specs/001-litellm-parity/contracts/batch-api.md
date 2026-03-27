# Contract: Batch Processing API

OpenAI-compatible batch API. Supported backends: `openai`, `azure`. All other backends return 501.

## Endpoints

### POST /v1/files

Upload a JSONL file for batch processing.

**Request**: `multipart/form-data`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `file` | binary | yes | JSONL content |
| `purpose` | string | yes | Must be `batch` |

**JSONL line format** (each line):
```json
{"custom_id": "req-1", "method": "POST", "url": "/v1/chat/completions", "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}}
```

**Validation**:
- `custom_id`: required, string, max 64 chars, unique within the file
- `body.model`: required string
- File size: max 100MB (413 if exceeded)
- Line count: max 50,000 (400 if exceeded)

**Response 200**:
```json
{
  "id": "file-abc123",
  "object": "file",
  "bytes": 12345,
  "created_at": 1711471533,
  "filename": "requests.jsonl",
  "purpose": "batch"
}
```

**Errors**:
- `400` — invalid JSONL, missing `custom_id`, unsupported `purpose`
- `413` — file too large
- `415` — unsupported content type

---

### POST /v1/batches

Create a batch job from an uploaded file.

**Request body**:
```json
{
  "input_file_id": "file-abc123",
  "endpoint": "/v1/chat/completions",
  "completion_window": "24h",
  "metadata": {}
}
```

| Field | Required | Notes |
|-------|----------|-------|
| `input_file_id` | yes | Must reference an uploaded file owned by the caller's key |
| `endpoint` | yes | Must be `/v1/chat/completions` (only supported endpoint for v1) |
| `completion_window` | yes | Must be `24h` |
| `metadata` | no | Forwarded to backend |

**Response 200**:
```json
{
  "id": "batch-xyz789",
  "object": "batch",
  "endpoint": "/v1/chat/completions",
  "status": "validating",
  "input_file_id": "file-abc123",
  "completion_window": "24h",
  "created_at": 1711471533,
  "request_counts": {"total": 0, "completed": 0, "failed": 0},
  "metadata": {}
}
```

**Errors**:
- `400` — `input_file_id` not found, unsupported `endpoint`
- `501` — backend does not support batch processing

---

### GET /v1/batches/{id}

Retrieve current status of a batch.

**Path params**: `id` — batch ID (e.g. `batch-xyz789`)

**Response 200**: Same shape as POST /v1/batches response, with current status and counts.

Additional fields present when completed:
```json
{
  "output_file_id": "file-out-123",
  "error_file_id": "file-err-456",
  "completed_at": 1711485533,
  "expires_at": 1711571933
}
```

**Errors**:
- `404` — batch not found (or belongs to a different key)

---

### GET /v1/batches

List recent batches for the authenticated key.

**Query params**:
| Param | Default | Notes |
|-------|---------|-------|
| `limit` | `20` | Max 100 |
| `after` | (none) | Cursor-based pagination (batch ID) |

**Response 200**:
```json
{
  "object": "list",
  "data": [...],
  "first_id": "batch-xyz789",
  "last_id": "batch-abc001",
  "has_more": false
}
```

---

### POST /v1/messages/batches

Replaces the existing 400 stub. Delegates to the Anthropic batch API when `BACKEND=anthropic`.
For all other backends, returns 501. (Anthropic passthrough backend only; not a translation target.)

---

## Notes

- Batch output is retrieved by fetching the `output_file_id` via `GET /v1/files/{id}/content`
  (standard OpenAI Files API, not currently implemented — tracked as a follow-on if needed; for v1
  clients can use the backend's own file retrieval endpoint directly).
- The proxy does not run inference locally; all batch processing is delegated to the backend.
- Rate limiting applies to the batch create and status endpoints (same RPM as LLM endpoints).
