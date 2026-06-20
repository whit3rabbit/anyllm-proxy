#!/usr/bin/env bash
# scripts/docker-smoke-test.sh
#
# Smoke-tests a running proxy + admin stack.
# Requires: curl, jq
#
# Environment (all optional, defaults shown):
#   PROXY_URL   http://localhost:3000
#   ADMIN_URL   http://localhost:3001
#   ADMIN_TOKEN test-admin-token-docker-smoke-0000
#
# Usage:
#   bash scripts/docker-smoke-test.sh
#   PROXY_URL=http://localhost:3000 bash scripts/docker-smoke-test.sh

set -euo pipefail

PROXY_URL="${PROXY_URL:-http://localhost:3000}"
ADMIN_URL="${ADMIN_URL:-http://localhost:3001}"
ADMIN_TOKEN="${ADMIN_TOKEN:-test-admin-token-docker-smoke-0000}"

PASS=0
FAIL=0

# Color output if terminal supports it.
if [ -t 1 ]; then
  GREEN='\033[0;32m'; RED='\033[0;31m'; RESET='\033[0m'
else
  GREEN=''; RED=''; RESET=''
fi

pass() { echo -e "${GREEN}PASS${RESET}: $1"; PASS=$((PASS + 1)); }
fail() { echo -e "${RED}FAIL${RESET}: $1"; FAIL=$((FAIL + 1)); }

# check <description> <expected-substring> <actual>
check() {
  local desc="$1" expected="$2" actual="$3"
  if echo "$actual" | grep -qF "$expected"; then
    pass "$desc"
  else
    fail "$desc — expected substring '$expected', got: $actual"
  fi
}

# check_status <description> <expected-http-code> <url> [curl-args...]
check_status() {
  local desc="$1" expected="$2" url="$3"
  shift 3
  local code
  code=$(curl -s -o /dev/null -w "%{http_code}" "$@" "$url")
  if [ "$code" = "$expected" ]; then
    pass "$desc (HTTP $code)"
  else
    fail "$desc — expected HTTP $expected, got $code"
  fi
}

echo "Smoke-testing proxy at $PROXY_URL, admin at $ADMIN_URL"
echo "---"

# Cookie jar for CSRF double-submit cookie pattern.
COOKIEJAR=$(mktemp)
trap 'rm -f "$COOKIEJAR"' EXIT

# 1. Proxy health (no auth)
check "proxy health" '"status":"ok"' "$(curl -sf "$PROXY_URL/health")"

# 2. Admin health (no auth, public route)
check "admin health" '"status":"ok"' "$(curl -sf "$ADMIN_URL/admin/health")"

# 3. Auth enforcement — missing key must return 401
check_status "missing auth key returns 401" "401" "$PROXY_URL/v1/messages" \
  -X POST \
  -H "content-type: application/json" \
  -d '{"model":"claude-3-5-sonnet-20241022","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'

# 4. /v1/models — no backend call needed (hardcoded list)
check "/v1/models returns data array" '"data"' \
  "$(curl -sf "$PROXY_URL/v1/models" -H "x-api-key: smoke-test-key")"

# 5. CSRF token — GET /admin/csrf-token requires admin auth, sets a cookie,
#    and returns the token body.
#    Both must be replayed on state-mutating admin requests (double-submit pattern).
CSRF_RESP=$(curl -sf -c "$COOKIEJAR" "$ADMIN_URL/admin/csrf-token" \
  -H "Authorization: Bearer $ADMIN_TOKEN")
CSRF=$(echo "$CSRF_RESP" | jq -r '.csrf_token // .token // empty')
if [ -n "$CSRF" ]; then
  pass "csrf token fetch"
else
  fail "csrf token fetch — response: $CSRF_RESP"
  CSRF="invalid"
fi

# 6. Create virtual key via admin API
CREATE_RESP=$(curl -sf -b "$COOKIEJAR" -X POST "$ADMIN_URL/admin/api/keys" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "X-CSRF-Token: $CSRF" \
  -H "content-type: application/json" \
  -d '{"description":"docker-smoke-test-key"}')
check "create virtual key returns sk-vk prefix" '"sk-vk' "$CREATE_RESP"

KEY_ID=$(echo "$CREATE_RESP" | jq -r '.id // empty')
VK=$(echo "$CREATE_RESP"    | jq -r '.key // empty')

# 7. List keys — created key description must appear
LIST_RESP=$(curl -sf "$ADMIN_URL/admin/api/keys" \
  -H "Authorization: Bearer $ADMIN_TOKEN")
check "list keys includes smoke-test-key" 'docker-smoke-test-key' "$LIST_RESP"

# 8. Virtual key auth — verify the created key works against the proxy.
#    Auth via /v1/models (no backend call needed).
if [ -n "$VK" ]; then
  check "/v1/models accessible with virtual key" '"data"' \
    "$(curl -sf "$PROXY_URL/v1/models" -H "x-api-key: $VK")"
else
  fail "virtual key auth — could not extract key from create response"
fi

# 9. Delete virtual key — CSRF tokens are one-time-use; fetch a fresh one.
if [ -n "$KEY_ID" ]; then
  CSRF_DEL_RESP=$(curl -sf -c "$COOKIEJAR" "$ADMIN_URL/admin/csrf-token" \
    -H "Authorization: Bearer $ADMIN_TOKEN")
  CSRF_DEL=$(echo "$CSRF_DEL_RESP" | jq -r '.csrf_token // .token // empty')
  DEL_CODE=$(curl -s -o /dev/null -w "%{http_code}" -b "$COOKIEJAR" -X DELETE \
    "$ADMIN_URL/admin/api/keys/$KEY_ID" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "X-CSRF-Token: $CSRF_DEL")
  if [ "$DEL_CODE" = "200" ] || [ "$DEL_CODE" = "204" ]; then
    pass "delete virtual key (HTTP $DEL_CODE)"
  else
    fail "delete virtual key — expected 200 or 204, got $DEL_CODE"
  fi
else
  fail "delete virtual key — could not extract key id from create response"
fi

echo "---"
echo "Results: ${PASS} passed, ${FAIL} failed"
[ "$FAIL" -eq 0 ]
