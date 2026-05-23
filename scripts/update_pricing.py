#!/usr/bin/env python3
"""
Update assets/model_pricing.json from LiteLLM's canonical pricing file.

Usage:
  python scripts/update_pricing.py              # write assets/model_pricing.json and crate copy
  python scripts/update_pricing.py --dry-run    # print diff, no write
  python scripts/update_pricing.py --output /path/to/file.json
"""

import argparse
import json
import sys
import urllib.request
from pathlib import Path

LITELLM_URL = (
    "https://raw.githubusercontent.com/BerriAI/litellm/main/"
    "model_prices_and_context_window.json"
)

# Maps litellm_provider -> our provider name. Only these providers are included.
PROVIDER_MAP = {
    "openai": "openai",
    "anthropic": "anthropic",
    # vertex_ai-language-models: clean base names (e.g. "gemini-2.5-pro") — highest priority
    "vertex_ai-language-models": "google",
    # gemini: uses "gemini/" prefix in model key — we strip the prefix
    "gemini": "google",
    # vertex_ai: other vertex entries (lower priority, deduplicated below)
    "vertex_ai": "google",
}

ALLOWED_MODES = {"chat", "embedding", "completion"}

# Repo root is one level up from this script.
REPO_ROOT = Path(__file__).parent.parent
DEFAULT_OUTPUT = REPO_ROOT / "assets" / "model_pricing.json"
PACKAGE_OUTPUT = REPO_ROOT / "crates" / "proxy" / "assets" / "model_pricing.json"


def fetch_litellm_pricing() -> dict:
    req = urllib.request.Request(
        LITELLM_URL,
        headers={"User-Agent": "anyllm-pricing-updater/1.0"},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def transform(raw: dict) -> list[dict]:
    """
    Convert LiteLLM's dict-keyed pricing into our array format.

    Provider priority for deduplication (highest first):
      vertex_ai-language-models -> clean base names, e.g. "gemini-2.5-pro"
      openai / anthropic        -> direct provider names
      gemini                    -> has "gemini/" prefix we strip; covered by v_a_l above
      vertex_ai                 -> lower-priority fallback

    The "gemini/" prefix in LiteLLM model keys is a routing hint, not a real
    model name. We strip it so "gemini/gemini-2.5-pro" becomes "gemini-2.5-pro".
    """
    # Process in priority order so first-writer wins on duplicate model_pattern.
    PRIORITY = [
        "vertex_ai-language-models",
        "openai",
        "anthropic",
        "gemini",
        "vertex_ai",
    ]

    entries: dict[str, dict] = {}  # model_pattern -> entry

    for litellm_provider in PRIORITY:
        for model_name, data in raw.items():
            if not isinstance(data, dict):
                continue
            if data.get("litellm_provider", "") != litellm_provider:
                continue

            mode = data.get("mode", "")
            if mode not in ALLOWED_MODES:
                continue

            input_cost = data.get("input_cost_per_token")
            if input_cost is None or input_cost <= 0:
                continue

            # Strip routing prefix from gemini provider model keys.
            effective_name = model_name
            if litellm_provider == "gemini" and model_name.startswith("gemini/"):
                effective_name = model_name[len("gemini/"):]

            # Skip if a higher-priority entry already claimed this name.
            if effective_name in entries:
                continue

            output_cost = data.get("output_cost_per_token", 0.0)
            entries[effective_name] = {
                "model_pattern": effective_name,
                "input_cost_per_token": input_cost,
                "output_cost_per_token": output_cost,
                "provider": PROVIDER_MAP[litellm_provider],
            }

    result = sorted(entries.values(), key=lambda e: (e["provider"], e["model_pattern"]))
    return result


def diff_summary(old: list[dict], new: list[dict]) -> str:
    old_patterns = {e["model_pattern"] for e in old}
    new_patterns = {e["model_pattern"] for e in new}
    added = new_patterns - old_patterns
    removed = old_patterns - new_patterns
    changed = []
    for e in new:
        if e["model_pattern"] in old_patterns:
            old_entry = next(o for o in old if o["model_pattern"] == e["model_pattern"])
            if (
                old_entry["input_cost_per_token"] != e["input_cost_per_token"]
                or old_entry["output_cost_per_token"] != e["output_cost_per_token"]
            ):
                changed.append(e["model_pattern"])

    lines = [f"Total: {len(old)} -> {len(new)} entries"]
    if added:
        lines.append(f"Added ({len(added)}): {', '.join(sorted(added)[:20])}")
        if len(added) > 20:
            lines.append(f"  ... and {len(added) - 20} more")
    if removed:
        lines.append(f"Removed ({len(removed)}): {', '.join(sorted(removed))}")
    if changed:
        lines.append(f"Price changed ({len(changed)}): {', '.join(sorted(changed)[:10])}")
        if len(changed) > 10:
            lines.append(f"  ... and {len(changed) - 10} more")
    if not added and not removed and not changed:
        lines.append("No changes.")

    providers = {}
    for e in new:
        providers[e["provider"]] = providers.get(e["provider"], 0) + 1
    lines.append("Providers: " + ", ".join(f"{p}={c}" for p, c in sorted(providers.items())))
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Update model pricing from LiteLLM.")
    parser.add_argument("--dry-run", action="store_true", help="Print diff, do not write.")
    parser.add_argument("--output", default=str(DEFAULT_OUTPUT), help="Output file path.")
    args = parser.parse_args()

    output_path = Path(args.output)

    print(f"Fetching pricing from LiteLLM...", file=sys.stderr)
    try:
        raw = fetch_litellm_pricing()
    except Exception as e:
        print(f"ERROR: Failed to fetch LiteLLM pricing: {e}", file=sys.stderr)
        sys.exit(1)

    new_entries = transform(raw)
    if not new_entries:
        print("ERROR: Transform produced zero entries. Aborting.", file=sys.stderr)
        sys.exit(1)

    old_entries: list[dict] = []
    if output_path.exists():
        try:
            old_entries = json.loads(output_path.read_text())
        except Exception:
            pass

    print(diff_summary(old_entries, new_entries))

    if args.dry_run:
        print("\n--dry-run: no file written.")
        return

    output = json.dumps(new_entries, indent=2) + "\n"
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(output)
    print(f"Written: {output_path}", file=sys.stderr)

    if output_path.resolve() == DEFAULT_OUTPUT.resolve():
        PACKAGE_OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        PACKAGE_OUTPUT.write_text(output)
        print(f"Written: {PACKAGE_OUTPUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
