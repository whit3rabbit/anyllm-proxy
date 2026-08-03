#!/usr/bin/env python3
"""
Compare local provider catalog data with LiteLLM provider/model metadata.

Default:
  python3 scripts/check_litellm_providers.py --provider anthropic

Provider-wide drift:
  python3 scripts/check_litellm_providers.py --all

Use --check to exit nonzero on drift, --json for machine-readable output, and
--write-report PATH to write the computed diff without mutating source files.
Use --write-rust-snapshot PATH to regenerate the Rust LiteLLM compatibility
catalog from the same upstream JSON.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any


LITELLM_URL = (
    "https://raw.githubusercontent.com/BerriAI/litellm/main/"
    "model_prices_and_context_window.json"
)

DEFAULT_MODES = ("all",)
TOKEN_PRICING_MODES = {"chat", "completion", "embedding"}

REPO_ROOT = Path(__file__).parent.parent
PROVIDERS_DIR = REPO_ROOT / "crates" / "providers" / "src" / "providers"
SNAPSHOT_PATH = PROVIDERS_DIR / "litellm_snapshot.rs"
REGISTRY_PATH = REPO_ROOT / "crates" / "providers" / "src" / "registry.rs"
PRICING_FILES = [
    REPO_ROOT / "assets" / "model_pricing.json",
    REPO_ROOT / "crates" / "proxy" / "assets" / "model_pricing.json",
]

# Hand-maintained: Anthropic models that reject
# `thinking: {"type": "enabled", "budget_tokens": N}` with a 400 (adaptive-only).
# NOT derivable from LiteLLM flags -- `extended_thinking` is true for every
# thinking-capable model, so it cannot separate "accepts budget_tokens"
# (Opus 4.6 / Sonnet 4.6, deprecated) from "rejects it" (4.7+). Source of truth:
# Anthropic docs, "Configurations each model rejects" table. Add new adaptive-only
# releases here (Opus 4.7+, Sonnet 5+, Fable 5, Mythos 5).
ANTHROPIC_ADAPTIVE_ONLY_THINKING: list[str] = [
    "claude-fable-5",
    "claude-opus-4-7",
    "claude-opus-4-7-20260416",
    "claude-opus-4-8",
    "claude-opus-5",
    "claude-sonnet-5",
]

SOURCE_PROVIDER_ALIASES = {
    "aiml": "ai_ml_api",
    "amazon_nova": "bedrock",
    "azure_text": "azure",
    "bedrock_converse": "bedrock",
    "bedrock_mantle": "bedrock",
    "cohere": "cohere_chat",
    "exa_ai": "exa",
    "fireworks_ai-embedding-models": "fireworks_ai",
    "github_copilot": "github",
    "gmi": "gmi_cloud",
    "jina_ai": "jina",
    "palm": "gemini",
    "publicai": "public_ai",
    "stability": "stability_ai",
    "text-completion-codestral": "codestral",
    "text-completion-openai": "openai",
    "vertex_ai-ai21_models": "vertex_ai",
    "vertex_ai-anthropic_models": "vertex_ai",
    "vertex_ai-deepseek_models": "vertex_ai",
    "vertex_ai-embedding-models": "vertex_ai",
    "vertex_ai-image-models": "vertex_ai",
    "vertex_ai-language-models": "vertex_ai",
    "vertex_ai-llama_models": "vertex_ai",
    "vertex_ai-minimax_models": "vertex_ai",
    "vertex_ai-mistral_models": "vertex_ai",
    "vertex_ai-moonshot_models": "vertex_ai",
    "vertex_ai-openai_models": "vertex_ai",
    "vertex_ai-qwen_models": "vertex_ai",
    "vertex_ai-text-models": "vertex_ai",
    "vertex_ai-video-models": "vertex_ai",
    "vertex_ai-zai_models": "vertex_ai",
    "zai": "zhipuai",
}


def fetch_litellm() -> dict[str, Any]:
    req = urllib.request.Request(
        LITELLM_URL,
        headers={"User-Agent": "anyllm-provider-check/1.0"},
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def mode_set(raw_modes: str) -> set[str] | None:
    modes = [mode.strip() for mode in raw_modes.split(",") if mode.strip()]
    if modes == ["all"]:
        return None
    return set(modes)


def extract_struct_blocks(text: str, name: str) -> list[str]:
    blocks = []
    needle = f"{name} {{"
    search_at = 0
    while True:
        start = text.find(needle, search_at)
        if start == -1:
            return blocks
        brace_start = text.find("{", start)
        if brace_start == -1:
            return blocks

        depth = 0
        in_string = False
        escaped = False
        for idx in range(brace_start, len(text)):
            char = text[idx]
            if in_string:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
                continue
            if char == '"':
                in_string = True
            elif char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    blocks.append(text[brace_start + 1 : idx])
                    search_at = idx + 1
                    break
        else:
            raise ValueError(f"unterminated {name} block")


def find_string_field(block: str, field: str) -> str | None:
    match = re.search(rf'{field}:\s*"([^"]*)"', block)
    return match.group(1).strip() if match else None


def find_variant_field(block: str, field: str, enum_name: str) -> str | None:
    match = re.search(rf"{field}:\s*{enum_name}::([A-Za-z0-9_]+)", block)
    return match.group(1) if match else None


def find_bool_field(block: str, field: str) -> bool | None:
    match = re.search(rf"{field}:\s*(true|false)", block)
    if not match:
        return None
    return match.group(1) == "true"


def find_string_array_field(block: str, field: str) -> list[str]:
    match = re.search(rf"{field}:\s*&\[(.*?)\]", block, re.S)
    if not match:
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def find_status(block: str) -> str | None:
    match = re.search(r"status:\s*ModelStatus::([A-Za-z]+)", block)
    return match.group(1) if match else None


def parse_int(value: str | None) -> int | None:
    if value is None:
        return None
    return int(value.replace("_", ""))


def find_int_field(block: str, field: str) -> int | None:
    match = re.search(rf"{field}:\s*([0-9_]+)", block)
    return parse_int(match.group(1)) if match else None


def parse_registered_provider_ids() -> set[str]:
    text = REGISTRY_PATH.read_text()
    if "litellm_snapshot::ALL_MODELS" in text and SNAPSHOT_PATH.exists():
        return set(parse_local_catalogs(paths=[SNAPSHOT_PATH], mark_registered=False))

    explicit = set(
        re.findall(r'\(\s*"([^"]+)"\s*,\s*providers::[A-Za-z0-9_]+::MODELS', text)
    )
    return explicit


def provider_source_paths(include_snapshot: bool) -> list[Path]:
    paths = [path for path in sorted(PROVIDERS_DIR.glob("*.rs")) if path.name != "mod.rs"]
    if not include_snapshot:
        return [path for path in paths if path != SNAPSHOT_PATH]
    if SNAPSHOT_PATH.exists():
        return [SNAPSHOT_PATH]
    return paths


def parse_local_catalogs(
    paths: list[Path] | None = None, mark_registered: bool = True
) -> dict[str, dict[str, Any]]:
    providers: dict[str, dict[str, Any]] = {}
    models_by_provider: dict[str, dict[str, Any]] = defaultdict(dict)

    for path in paths or provider_source_paths(include_snapshot=True):
        text = path.read_text()
        provider_blocks = extract_struct_blocks(text, "ProviderDef")
        for provider_block in provider_blocks:
            provider_id = find_string_field(provider_block, "id")
            if not provider_id:
                continue
            litellm_prefix = find_string_field(provider_block, "litellm_prefix") or ""
            providers[provider_id] = {
                "id": provider_id,
                "display_name": find_string_field(provider_block, "display_name") or provider_id,
                "file": str(path.relative_to(REPO_ROOT)),
                "default_base_url": find_string_field(provider_block, "default_base_url") or "",
                "protocol": find_variant_field(
                    provider_block, "protocol", "ProviderProtocol"
                )
                or "OpenAICompat",
                "auth": find_variant_field(provider_block, "auth", "AuthKind") or "Bearer",
                "status": find_variant_field(provider_block, "status", "ProviderStatus")
                or "Stub",
                "env_vars": find_string_array_field(provider_block, "env_vars"),
                "litellm_prefix": litellm_prefix,
                "litellm_provider": litellm_prefix.rstrip("/") or provider_id,
                "capabilities": {
                    "chat_completions": find_bool_field(provider_block, "chat_completions"),
                    "streaming": find_bool_field(provider_block, "streaming"),
                    "tool_use": find_bool_field(provider_block, "tool_use"),
                    "tool_choice": find_bool_field(provider_block, "tool_choice"),
                    "embeddings": find_bool_field(provider_block, "embeddings"),
                    "vision": find_bool_field(provider_block, "vision"),
                    "batch": find_bool_field(provider_block, "batch"),
                },
                "models": {},
            }

        for block in extract_struct_blocks(text, "ModelDef"):
            model_id = find_string_field(block, "id")
            if not model_id:
                continue
            model_provider = find_string_field(block, "provider_id") or provider_id
            models_by_provider[model_provider][model_id] = {
                "context_window": find_int_field(block, "context_window"),
                "max_output_tokens": find_int_field(block, "max_output_tokens"),
                "status": find_status(block),
                "file": str(path.relative_to(REPO_ROOT)),
            }

    for provider_id, models in models_by_provider.items():
        if provider_id not in providers:
            providers[provider_id] = {
                "id": provider_id,
                "file": None,
                "litellm_prefix": "",
                "litellm_provider": provider_id,
                "models": {},
            }
        providers[provider_id]["models"] = models

    registered = set(providers) if not mark_registered else parse_registered_provider_ids()
    for provider_id, provider in providers.items():
        provider["registered"] = provider_id in registered

    return providers


def local_by_litellm_provider(local: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
    by_key = {}
    for provider in local.values():
        key = provider["litellm_provider"]
        if key:
            by_key[key] = provider
    return by_key


def normalize_litellm_model_id(model: str, provider: str) -> str:
    prefix = f"{provider}/"
    if model.startswith(prefix):
        return model[len(prefix) :]
    return model


def litellm_provider_rows(
    raw: dict[str, Any], provider: str, allowed_modes: set[str] | None
) -> dict[str, dict[str, Any]]:
    rows = {}
    for model, data in raw.items():
        if not isinstance(data, dict):
            continue
        if data.get("litellm_provider") != provider:
            continue
        mode = data.get("mode")
        if allowed_modes is not None and mode not in allowed_modes:
            continue
        normalized = normalize_litellm_model_id(model, provider)
        input_cost = data.get("input_cost_per_token")
        rows[normalized] = {
            "context_window": data.get("max_input_tokens") or 0,
            "max_output_tokens": data.get("max_output_tokens") or 0,
            "mode": mode,
            "litellm_model": model,
            "pricing_eligible": mode in TOKEN_PRICING_MODES
            and input_cost is not None
            and input_cost > 0,
            "tool_use": bool(
                data.get("supports_function_calling") or data.get("supports_tool_choice")
            ),
            "tool_choice": bool(data.get("supports_tool_choice")),
            "vision": bool(data.get("supports_vision")),
            "extended_thinking": bool(
                data.get("supports_reasoning") or data.get("supports_reasoning_content")
            ),
            "supports_adaptive_thinking": bool(data.get("supports_adaptive_thinking")),
            "supports_max_reasoning_effort": bool(data.get("supports_max_reasoning_effort")),
            "supports_xhigh_reasoning_effort": bool(data.get("supports_xhigh_reasoning_effort")),
            "deprecated": bool(data.get("deprecation_date") or data.get("is_deprecated")),
        }
    return rows


def litellm_provider_counts(
    raw: dict[str, Any], allowed_modes: set[str] | None
) -> dict[str, int]:
    counts: dict[str, int] = defaultdict(int)
    for _model, data in raw.items():
        if not isinstance(data, dict):
            continue
        provider = data.get("litellm_provider")
        if not provider or provider.startswith("one of "):
            continue
        mode = data.get("mode")
        if allowed_modes is not None and mode not in allowed_modes:
            continue
        counts[provider] += 1
    return dict(sorted(counts.items()))


def pricing_rows(provider: str) -> dict[str, dict[str, Any]]:
    rows_by_file = {}
    for path in PRICING_FILES:
        entries = json.loads(path.read_text())
        rows_by_file[str(path.relative_to(REPO_ROOT))] = {
            entry["model_pattern"]: entry
            for entry in entries
            if entry.get("provider") == provider
        }
    return rows_by_file


def pricing_managed(provider: str, pricing: dict[str, dict[str, Any]]) -> bool:
    return any(rows for rows in pricing.values())


def diff_provider(
    provider: str,
    raw: dict[str, Any],
    local_by_prefix: dict[str, dict[str, Any]],
    allowed_modes: set[str] | None,
) -> dict[str, Any]:
    litellm = litellm_provider_rows(raw, provider, allowed_modes)
    local_provider = local_by_prefix.get(provider)
    catalog = local_provider["models"] if local_provider else {}
    pricing = pricing_rows(provider)
    managed_pricing = pricing_managed(provider, pricing)

    litellm_ids = set(litellm)
    litellm_pricing_ids = {
        model for model, row in litellm.items() if row.get("pricing_eligible")
    }
    catalog_ids = set(catalog)
    result: dict[str, Any] = {
        "provider": provider,
        "local_provider": {
            "id": local_provider["id"],
            "file": local_provider["file"],
            "litellm_prefix": local_provider["litellm_prefix"],
            "registered": local_provider["registered"],
        }
        if local_provider
        else None,
        "litellm_count": len(litellm),
        "catalog_count": len(catalog),
        "missing_in_catalog": sorted(litellm_ids - catalog_ids),
        "extra_in_catalog": sorted(catalog_ids - litellm_ids),
        "metadata_mismatches": [],
        "pricing_managed": managed_pricing,
        "pricing": {},
    }

    for model in sorted(litellm_ids & catalog_ids):
        expected = litellm[model]
        actual = catalog[model]
        fields = {}
        for field in ("context_window", "max_output_tokens"):
            if expected.get(field) != actual.get(field):
                fields[field] = {
                    "litellm": expected.get(field),
                    "catalog": actual.get(field),
                }
        if fields:
            result["metadata_mismatches"].append({"model": model, "fields": fields})

    for path, rows in pricing.items():
        pricing_ids = set(rows)
        result["pricing"][path] = {
            "count": len(rows),
            "managed": managed_pricing,
            "missing_litellm_models": sorted(litellm_pricing_ids - pricing_ids)
            if managed_pricing
            else [],
            "extra_pricing_models": sorted(pricing_ids - litellm_pricing_ids)
            if managed_pricing
            else [],
        }

    return result


def provider_alignment(
    raw: dict[str, Any],
    local: dict[str, dict[str, Any]],
    allowed_modes: set[str] | None,
) -> dict[str, Any]:
    litellm_counts = litellm_provider_counts(raw, allowed_modes)
    local_prefixes = {
        provider["litellm_provider"]: provider
        for provider in local.values()
        if provider["litellm_provider"]
    }
    registered = parse_registered_provider_ids()
    local_registered_prefixes = {
        provider["litellm_provider"]: provider
        for provider in local.values()
        if provider["registered"] and provider["litellm_provider"]
    }

    litellm_ids = set(litellm_counts)
    local_ids = set(local_registered_prefixes)
    source_ids = set(local_prefixes)

    return {
        "litellm_provider_count": len(litellm_ids),
        "local_registered_provider_count": len(local_ids),
        "local_source_provider_count": len(source_ids),
        "missing_local_providers": sorted(litellm_ids - local_ids),
        "extra_local_providers": sorted(local_ids - litellm_ids),
        "source_not_registered": sorted(
            provider_id for provider_id in local if provider_id not in registered
        ),
        "registered_without_source": sorted(
            provider_id for provider_id in registered if provider_id not in local
        ),
        "provider_counts": litellm_counts,
    }


def diff_one(provider: str, allowed_modes: set[str] | None) -> dict[str, Any]:
    raw = fetch_litellm()
    local = parse_local_catalogs()
    return diff_provider(provider, raw, local_by_litellm_provider(local), allowed_modes)


def diff_all(allowed_modes: set[str] | None) -> dict[str, Any]:
    raw = fetch_litellm()
    local = parse_local_catalogs()
    local_by_prefix = local_by_litellm_provider(local)
    alignment = provider_alignment(raw, local, allowed_modes)

    providers = {
        provider: diff_provider(provider, raw, local_by_prefix, allowed_modes)
        for provider in sorted(set(alignment["provider_counts"]) | set(local_by_prefix))
    }

    return {
        "scope": "all",
        "modes": sorted(allowed_modes) if allowed_modes is not None else ["all"],
        "provider_alignment": alignment,
        "providers": providers,
    }


def has_provider_drift(result: dict[str, Any]) -> bool:
    if result["local_provider"] is None:
        return True
    if result["missing_in_catalog"] or result["extra_in_catalog"]:
        return True
    if result["metadata_mismatches"]:
        return True
    for pricing in result["pricing"].values():
        if pricing.get("managed") and (
            pricing["missing_litellm_models"] or pricing["extra_pricing_models"]
        ):
            return True
    return False


def has_drift(result: dict[str, Any]) -> bool:
    if result.get("scope") == "all":
        alignment = result["provider_alignment"]
        if (
            alignment["missing_local_providers"]
            or alignment["extra_local_providers"]
            or alignment["source_not_registered"]
            or alignment["registered_without_source"]
        ):
            return True
        return any(has_provider_drift(provider) for provider in result["providers"].values())
    return has_provider_drift(result)


def print_values(label: str, values: list[str], limit: int, indent: str = "") -> None:
    print(f"{indent}{label}: {len(values)}")
    for value in values[:limit]:
        print(f"{indent}  {value}")
    if len(values) > limit:
        print(f"{indent}  ... and {len(values) - limit} more")


def print_provider_human(result: dict[str, Any], limit: int = 50) -> None:
    local = result["local_provider"]
    local_label = (
        f"{local['id']} ({local['litellm_prefix'] or 'no prefix'})"
        if local
        else "missing"
    )
    print(
        f"{result['provider']}: LiteLLM={result['litellm_count']} "
        f"catalog={result['catalog_count']} local={local_label}"
    )
    for key in ("missing_in_catalog", "extra_in_catalog"):
        print_values(key, result[key], limit)
    print(f"metadata_mismatches: {len(result['metadata_mismatches'])}")
    for item in result["metadata_mismatches"][:limit]:
        print(f"  {item['model']}: {item['fields']}")
    if len(result["metadata_mismatches"]) > limit:
        print(f"  ... and {len(result['metadata_mismatches']) - limit} more")
    for path, pricing in result["pricing"].items():
        managed = "managed" if pricing["managed"] else "not managed"
        print(f"{path}: {pricing['count']} pricing entries ({managed})")
        for key in ("missing_litellm_models", "extra_pricing_models"):
            print_values(key, pricing[key], limit, indent="  ")


def print_all_human(result: dict[str, Any], limit: int) -> None:
    alignment = result["provider_alignment"]
    print(
        "providers: "
        f"LiteLLM={alignment['litellm_provider_count']} "
        f"local_registered={alignment['local_registered_provider_count']} "
        f"local_source={alignment['local_source_provider_count']}"
    )
    for key in (
        "missing_local_providers",
        "extra_local_providers",
        "source_not_registered",
        "registered_without_source",
    ):
        print_values(key, alignment[key], limit)

    drifted = [
        provider
        for provider, provider_result in result["providers"].items()
        if has_provider_drift(provider_result)
    ]
    print_values("providers_with_model_or_pricing_drift", drifted, limit)
    for provider in drifted[:limit]:
        provider_result = result["providers"][provider]
        print(
            f"  {provider}: missing={len(provider_result['missing_in_catalog'])} "
            f"extra={len(provider_result['extra_in_catalog'])} "
            f"metadata={len(provider_result['metadata_mismatches'])}"
        )


def rust_string(value: str) -> str:
    return json.dumps(value)


def rust_ident(value: str) -> str:
    ident = re.sub(r"[^A-Za-z0-9_]", "_", value).upper()
    if not ident or ident[0].isdigit():
        ident = f"P_{ident}"
    return ident


def rust_string_slice(values: list[str]) -> str:
    if not values:
        return "&[]"
    return "&[" + ", ".join(rust_string(value) for value in values) + "]"


def display_name(provider: str) -> str:
    known = {
        "ai21": "AI21",
        "aws_polly": "AWS Polly",
        "bedrock": "AWS Bedrock",
        "bedrock_converse": "AWS Bedrock Converse",
        "bedrock_mantle": "AWS Bedrock Mantle",
        "github_copilot": "GitHub Copilot",
        "gmi": "GMI Cloud",
        "oci": "Oracle Cloud Infrastructure",
        "xai": "xAI",
        "zai": "Z.ai",
    }
    if provider in known:
        return known[provider]
    return " ".join(part.upper() if len(part) <= 3 else part.title() for part in re.split(r"[_-]+", provider))


def guessed_env_vars(provider: str, protocol: str, auth: str) -> list[str]:
    if auth == "AwsSigV4" or protocol == "BedrockNative":
        return ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"]
    if protocol in {"VertexAI", "GeminiOpenAI", "GeminiNative"}:
        return ["GEMINI_API_KEY"]
    if protocol == "AzureOpenAI":
        return ["AZURE_OPENAI_API_KEY"]
    env_name = re.sub(r"[^A-Za-z0-9]+", "_", provider).strip("_").upper()
    return [f"{env_name}_API_KEY"] if env_name else []


def default_protocol(provider: str) -> str:
    if provider == "anthropic":
        return "AnthropicNative"
    if provider in {"gemini", "palm"}:
        return "GeminiOpenAI"
    if provider == "vertex_ai" or provider.startswith("vertex_ai-"):
        return "VertexAI"
    if provider == "azure" or provider.startswith("azure"):
        return "AzureOpenAI"
    if provider == "bedrock" or provider.startswith("bedrock") or provider == "amazon_nova":
        return "BedrockNative"
    return "OpenAICompat"


def default_auth(protocol: str) -> str:
    if protocol == "AzureOpenAI":
        return "AzureApiKey"
    if protocol in {"GeminiOpenAI", "GeminiNative", "VertexAI"}:
        return "GoogleApiKey"
    if protocol == "BedrockNative":
        return "AwsSigV4"
    return "Bearer"


def source_for_provider(
    provider: str,
    sources_by_id: dict[str, dict[str, Any]],
    sources_by_litellm: dict[str, dict[str, Any]],
) -> dict[str, Any] | None:
    if provider in sources_by_id:
        return sources_by_id[provider]
    if provider in sources_by_litellm:
        return sources_by_litellm[provider]
    alias = SOURCE_PROVIDER_ALIASES.get(provider)
    if alias:
        return sources_by_id.get(alias) or sources_by_litellm.get(alias)
    return None


def provider_metadata(
    provider: str,
    rows: dict[str, dict[str, Any]],
    sources_by_id: dict[str, dict[str, Any]],
    sources_by_litellm: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    source = source_for_provider(provider, sources_by_id, sources_by_litellm)
    protocol = source["protocol"] if source else default_protocol(provider)
    auth = source["auth"] if source else default_auth(protocol)
    modes = {row["mode"] for row in rows.values()}
    tool_use = any(row["tool_use"] for row in rows.values())
    tool_choice = any(row["tool_choice"] for row in rows.values())
    vision = any(row["vision"] for row in rows.values())
    return {
        "id": provider,
        "display_name": source["display_name"] if source else display_name(provider),
        "default_base_url": source["default_base_url"] if source else "",
        "protocol": protocol,
        "auth": auth,
        "status": source["status"] if source else "Stub",
        "env_vars": (source.get("env_vars") or guessed_env_vars(provider, protocol, auth))
        if source
        else guessed_env_vars(provider, protocol, auth),
        "capabilities": {
            "chat_completions": "chat" in modes or "responses" in modes,
            "streaming": "chat" in modes or "completion" in modes or "responses" in modes,
            "tool_use": tool_use,
            "tool_choice": tool_choice,
            "embeddings": "embedding" in modes,
            "vision": vision,
            "batch": bool(source and source["capabilities"].get("batch")),
        },
    }


def write_rust_snapshot(path: Path, raw: dict[str, Any], allowed_modes: set[str] | None) -> None:
    source_catalogs = parse_local_catalogs(
        paths=provider_source_paths(include_snapshot=False), mark_registered=False
    )
    sources_by_litellm = local_by_litellm_provider(source_catalogs)
    provider_counts = litellm_provider_counts(raw, allowed_modes)
    rows_by_provider = {
        provider: litellm_provider_rows(raw, provider, allowed_modes)
        for provider in sorted(provider_counts)
    }

    lines: list[str] = [
        "// Generated by scripts/check_litellm_providers.py --write-rust-snapshot.",
        "// Source: LiteLLM model_prices_and_context_window.json.",
        "// Do not edit provider/model rows by hand; update the script or source aliases.",
        "",
        "use crate::model::{ModelCapabilities, ModelDef, ModelStatus};",
        "use crate::provider::{",
        "    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,",
        "};",
        "",
    ]

    provider_const_names = []
    model_const_names = []
    for provider, rows in rows_by_provider.items():
        ident = rust_ident(provider)
        provider_const = f"PROVIDER_{ident}"
        model_const = f"MODELS_{ident}"
        provider_const_names.append(provider_const)
        model_const_names.append((provider, model_const))
        metadata = provider_metadata(provider, rows, source_catalogs, sources_by_litellm)
        caps = metadata["capabilities"]

        lines.extend(
            [
                f"pub const {provider_const}: ProviderDef = ProviderDef {{",
                f"    id: {rust_string(provider)},",
                f"    display_name: {rust_string(metadata['display_name'])},",
                f"    default_base_url: {rust_string(metadata['default_base_url'])},",
                f"    protocol: ProviderProtocol::{metadata['protocol']},",
                f"    auth: AuthKind::{metadata['auth']},",
                f"    status: ProviderStatus::{metadata['status']},",
                f"    env_vars: {rust_string_slice(metadata['env_vars'])},",
                f"    litellm_prefix: {rust_string(provider + '/')},",
                "    capabilities: ProviderCapabilities {",
                f"        chat_completions: {str(caps['chat_completions']).lower()},",
                f"        streaming: {str(caps['streaming']).lower()},",
                f"        tool_use: {str(caps['tool_use']).lower()},",
                f"        tool_choice: {str(caps['tool_choice']).lower()},",
                f"        embeddings: {str(caps['embeddings']).lower()},",
                f"        vision: {str(caps['vision']).lower()},",
                f"        batch: {str(caps['batch']).lower()},",
                "    },",
                "};",
                "",
                f"pub const {model_const}: &[ModelDef] = &[",
            ]
        )

        for model, row in sorted(rows.items()):
            streaming = row["mode"] in {"chat", "completion", "responses"}
            status = "Deprecated" if row["deprecated"] else "Available"
            lines.extend(
                [
                    "    ModelDef {",
                    f"        id: {rust_string(model)},",
                    f"        provider_id: {rust_string(provider)},",
                    f"        context_window: {int(row['context_window'])},",
                    f"        max_output_tokens: {int(row['max_output_tokens'])},",
                    "        capabilities: ModelCapabilities {",
                    f"            streaming: {str(streaming).lower()},",
                    f"            tool_use: {str(row['tool_use']).lower()},",
                    f"            tool_choice: {str(row['tool_choice']).lower()},",
                    f"            vision: {str(row['vision']).lower()},",
                    f"            extended_thinking: {str(row['extended_thinking']).lower()},",
                    "        },",
                    f"        status: ModelStatus::{status},",
                    "    },",
                ]
            )
        lines.extend(["];", ""])

    anthropic_rows = rows_by_provider.get("anthropic", {})
    support_tables = [
        (
            "ANTHROPIC_ADAPTIVE_THINKING_MODELS",
            "supports_adaptive_thinking",
        ),
        (
            "ANTHROPIC_MAX_REASONING_EFFORT_MODELS",
            "supports_max_reasoning_effort",
        ),
        (
            "ANTHROPIC_XHIGH_REASONING_EFFORT_MODELS",
            "supports_xhigh_reasoning_effort",
        ),
    ]
    for const_name, flag in support_tables:
        lines.append(f"pub static {const_name}: &[&str] = &[")
        for model, row in sorted(anthropic_rows.items()):
            if row[flag]:
                lines.append(f"    {rust_string(model)},")
        lines.extend(["];", ""])

    # ANTHROPIC_ADAPTIVE_ONLY_THINKING_MODELS is hand-maintained (above), not
    # data-driven. Emitted verbatim so the generated file is self-contained and
    # the constant survives every regeneration.
    lines.extend(
        [
            "/// Anthropic models where `thinking: {\"type\": \"enabled\", \"budget_tokens\": N}`",
            "/// is rejected with a 400 (adaptive-only). Opus 4.6 and Sonnet 4.6 still",
            "/// accept it as a deprecated transitional escape hatch, so they are NOT here",
            "/// even though they are in `ANTHROPIC_ADAPTIVE_THINKING_MODELS`. Hand-maintained",
            "/// in check_litellm_providers.py (ANTHROPIC_ADAPTIVE_ONLY_THINKING); not",
            "/// derivable from LiteLLM flags.",
            "pub static ANTHROPIC_ADAPTIVE_ONLY_THINKING_MODELS: &[&str] = &[",
        ]
    )
    for model in ANTHROPIC_ADAPTIVE_ONLY_THINKING:
        lines.append(f"    {rust_string(model)},")
    lines.extend(["];", ""])

    lines.append("pub static ALL_PROVIDERS: &[&ProviderDef] = &[")
    for provider_const in provider_const_names:
        lines.append(f"    &{provider_const},")
    lines.extend(["];", ""])

    lines.append("pub static ALL_MODELS: &[(&str, &[ModelDef])] = &[")
    for provider, model_const in model_const_names:
        lines.append(f"    ({rust_string(provider)}, {model_const}),")
    lines.extend(["];", ""])

    path.write_text("\n".join(lines))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", default="anthropic", help="LiteLLM provider id to compare.")
    parser.add_argument("--all", action="store_true", help="Compare all LiteLLM/local providers.")
    parser.add_argument(
        "--modes",
        default=",".join(DEFAULT_MODES),
        help='Comma-separated LiteLLM modes to include, or "all".',
    )
    parser.add_argument("--json", action="store_true", help="Print JSON instead of text.")
    parser.add_argument("--check", action="store_true", help="Exit nonzero on drift.")
    parser.add_argument("--limit", type=int, default=50, help="Max items per text section.")
    parser.add_argument(
        "--write-report",
        help="Write the computed diff JSON to this path. Does not edit source files.",
    )
    parser.add_argument(
        "--write-rust-snapshot",
        help="Regenerate a Rust LiteLLM provider/model snapshot at this path.",
    )
    args = parser.parse_args()

    allowed_modes = mode_set(args.modes)
    raw = fetch_litellm()

    if args.write_rust_snapshot:
        write_rust_snapshot(Path(args.write_rust_snapshot), raw, allowed_modes)

    if args.all or args.provider == "all":
        local = parse_local_catalogs()
        local_by_prefix = local_by_litellm_provider(local)
        alignment = provider_alignment(raw, local, allowed_modes)
        result = {
            "scope": "all",
            "modes": sorted(allowed_modes) if allowed_modes is not None else ["all"],
            "provider_alignment": alignment,
            "providers": {
                provider: diff_provider(provider, raw, local_by_prefix, allowed_modes)
                for provider in sorted(set(alignment["provider_counts"]) | set(local_by_prefix))
            },
        }
    else:
        local = parse_local_catalogs()
        result = diff_provider(
            args.provider, raw, local_by_litellm_provider(local), allowed_modes
        )
    output = json.dumps(result, indent=2, sort_keys=True) + "\n"

    if args.write_report:
        Path(args.write_report).write_text(output)

    if args.json:
        print(output, end="")
    elif result.get("scope") == "all":
        print_all_human(result, args.limit)
    else:
        print_provider_human(result, args.limit)

    return 1 if args.check and has_drift(result) else 0


if __name__ == "__main__":
    sys.exit(main())
