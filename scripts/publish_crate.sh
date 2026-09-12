#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <package> [cargo publish args...]" >&2
  exit 2
fi

package="$1"
shift
expected_owner="${CRATES_IO_OWNER:-whit3rabbit}"

if cargo publish -p "$package" "$@"; then
  exit 0
else
  publish_ec=$?
fi

version="$({
  cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json,sys
pkg=sys.argv[1]
for package in json.load(sys.stdin)["packages"]:
    if package["name"] == pkg:
        print(package["version"])
        break
else:
    raise SystemExit(f"package {pkg!r} not found in cargo metadata")
' "$package"
})"

if ! python3 - "$package" "$version" <<'PY'
import json
import sys
import urllib.error
import urllib.request

package, version = sys.argv[1:]
url = f"https://crates.io/api/v1/crates/{package}/{version}"
request = urllib.request.Request(url, headers={"User-Agent": "anyllm-proxy publish workflow"})
try:
    with urllib.request.urlopen(request, timeout=30) as response:
        json.load(response)
except urllib.error.HTTPError as exc:
    if exc.code == 404:
        raise SystemExit(1)
    raise
PY
then
  echo "cargo publish failed for ${package} ${version}, and that exact version is not present on crates.io" >&2
  exit "$publish_ec"
fi

owners="$(cargo owner --list "$package")"
if ! printf '%s\n' "$owners" | grep -Eq "(^|[^[:alnum:]_-])${expected_owner}([^[:alnum:]_-]|$)"; then
  echo "cargo publish failed for ${package} ${version}; crate exists but is not owned by ${expected_owner}" >&2
  printf '%s\n' "$owners" >&2
  exit "$publish_ec"
fi

echo "${package} ${version} is already published and owned by ${expected_owner}"
