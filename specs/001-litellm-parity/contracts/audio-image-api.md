# Contract: Audio and Image Passthrough Endpoints

Transparent passthroughs to the configured backend. No translation; no model mapping.

## Audio Transcription

### POST /v1/audio/transcriptions

**Auth**: Required (any valid key, developer or admin role).

**Request**: `multipart/form-data` — forwarded byte-for-byte to the backend.

Standard OpenAI fields:
| Field | Notes |
|-------|-------|
| `file` | Audio file (mp3, mp4, mpeg, mpga, m4a, wav, webm) |
| `model` | e.g. `whisper-1` |
| `language` | optional |
| `prompt` | optional |
| `response_format` | optional |
| `temperature` | optional |

**Response**: Backend response forwarded unchanged (status code, headers, body).

**Backend restrictions**: Not mounted for `BACKEND=anthropic` or `BACKEND=bedrock`.
Returns 501 if called when those backends are configured.

---

## Text-to-Speech

### POST /v1/audio/speech

**Auth**: Required (any valid key).

**Request**: `application/json` — forwarded byte-for-byte.

Standard OpenAI fields:
| Field | Notes |
|-------|-------|
| `model` | e.g. `tts-1`, `tts-1-hd` |
| `input` | Text to synthesize |
| `voice` | `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer` |
| `response_format` | optional |
| `speed` | optional |

**Response**: Audio bytes streamed from backend unchanged.

**Backend restrictions**: Same as `/v1/audio/transcriptions`.

---

## Image Generation

### POST /v1/images/generations

**Auth**: Required (any valid key).

**Request**: `application/json` — forwarded byte-for-byte.

Standard OpenAI fields:
| Field | Notes |
|-------|-------|
| `prompt` | Required |
| `model` | e.g. `dall-e-3`, `dall-e-2` |
| `n` | optional |
| `size` | optional |
| `quality` | optional |
| `style` | optional |
| `response_format` | optional |

**Response**: Backend response forwarded unchanged.

**Backend restrictions**: Not mounted for `BACKEND=anthropic` or `BACKEND=bedrock`.

---

## Common Passthrough Behavior

- The proxy copies the original `Content-Type` header from the request to the forwarded request.
- The proxy forwards the backend's `Content-Type` response header to the client.
- No `x-anyllm-cache` header is set on passthrough responses (caching does not apply).
- No `x-anyllm-degradation` header is set.
- Rate limiting (RPM) applies to these endpoints using the authenticated key's limits.
- TPM limits do not apply (no token counting for audio/image).
- Cost tracking does not apply for v1 (no token counts returned by audio/image endpoints).
