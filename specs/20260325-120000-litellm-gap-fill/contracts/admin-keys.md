# Contract: Admin Virtual Key Management

All endpoints require admin token auth (`Authorization: Bearer {admin_token}`). Admin server is localhost-only.

## POST /admin/api/keys

Create a new virtual API key.

### Request
```json
{
  "description": "Team Alpha dev key",
  "expires_at": "2026-06-01T00:00:00Z",
  "rpm_limit": 60,
  "tpm_limit": 100000,
  "spend_limit": 50.00
}
```

All fields optional.

### Response (201 Created)
```json
{
  "id": 1,
  "key": "sk-vkA1B2C3D4...full-key-shown-once",
  "key_prefix": "sk-vkA1B",
  "description": "Team Alpha dev key",
  "created_at": "2026-03-25T12:00:00Z",
  "expires_at": "2026-06-01T00:00:00Z",
  "rpm_limit": 60,
  "tpm_limit": 100000,
  "spend_limit": 50.00
}
```

The `key` field contains the raw API key. It is shown exactly once at creation time and is not stored or retrievable afterward.

## GET /admin/api/keys

List all virtual keys (active, expired, and revoked).

### Response (200 OK)
```json
{
  "keys": [
    {
      "id": 1,
      "key_prefix": "sk-vkA1B",
      "description": "Team Alpha dev key",
      "created_at": "2026-03-25T12:00:00Z",
      "expires_at": "2026-06-01T00:00:00Z",
      "revoked_at": null,
      "rpm_limit": 60,
      "tpm_limit": 100000,
      "spend_limit": 50.00,
      "total_spend": 2.34,
      "total_requests": 142,
      "total_tokens": 53200,
      "status": "active"
    }
  ]
}
```

`status` is computed: `"active"`, `"expired"`, or `"revoked"`.

## DELETE /admin/api/keys/{id}

Revoke a virtual key. Takes effect immediately (no restart).

### Response (200 OK)
```json
{
  "id": 1,
  "revoked_at": "2026-03-25T14:00:00Z",
  "status": "revoked"
}
```

### Error (404)
```json
{
  "error": "Key not found"
}
```

## Auth check order (proxy middleware)

1. Check `PROXY_API_KEYS` env-var hashes (existing behavior, backward-compatible)
2. SHA-256 hash the incoming key, look up in DashMap
3. If found: check `revoked_at`, `expires_at`, rate limits
4. If not found in either: reject 401
