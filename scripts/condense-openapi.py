#!/usr/bin/env python3
"""Condense an OpenAPI spec into an LLM-readable summary.

Downloads the full spec, strips examples/metadata/verbose descriptions,
and outputs a compact YAML or JSON with endpoints and schema outlines.
Supports filtering by tag to get only relevant sections.

Requires: pip install pyyaml
"""

import argparse
import json
import os
import sys
from collections import OrderedDict
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import urlopen

try:
    import yaml
except ImportError:
    print("PyYAML required: pip install pyyaml", file=sys.stderr)
    sys.exit(1)

DEFAULT_URL = (
    "https://raw.githubusercontent.com/openai/openai-openapi/"
    "refs/heads/manual_spec/openapi.yaml"
)


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Condense an OpenAPI spec into LLM-readable format."
    )
    p.add_argument(
        "--tag",
        action="append",
        dest="tags",
        help="Filter endpoints by tag (repeatable, case-insensitive).",
    )
    p.add_argument("--json", action="store_true", help="Output JSON instead of YAML.")
    p.add_argument("--output", "-o", type=str, help="Output file (default: stdout).")
    p.add_argument(
        "--cache-dir",
        type=str,
        default=str(Path(__file__).parent / ".cache"),
        help="Cache directory for downloaded specs.",
    )
    p.add_argument("--no-cache", action="store_true", help="Force re-download.")
    p.add_argument("--url", type=str, default=DEFAULT_URL, help="OpenAPI spec URL.")
    return p.parse_args()


def download_spec(url: str, cache_dir: str, no_cache: bool) -> str:
    """Download spec with local file caching."""
    cache_path = Path(cache_dir)
    # Derive a simple filename from the URL
    filename = url.rstrip("/").rsplit("/", 1)[-1]
    cached = cache_path / filename

    if not no_cache and cached.exists():
        print(f"Using cached spec: {cached} ({cached.stat().st_size:,} bytes)", file=sys.stderr)
        return cached.read_text(encoding="utf-8")

    print(f"Downloading: {url}", file=sys.stderr)
    with urlopen(url, timeout=60) as resp:
        data = resp.read().decode("utf-8")

    cache_path.mkdir(parents=True, exist_ok=True)
    cached.write_text(data, encoding="utf-8")
    print(f"Cached: {cached} ({len(data):,} bytes)", file=sys.stderr)
    return data


def short_ref(ref: str) -> str:
    """#/components/schemas/Foo -> $Foo, #/components/parameters/Bar -> $params.Bar"""
    if not ref:
        return ref
    if ref.startswith("#/components/schemas/"):
        return "$" + ref.split("/")[-1]
    if ref.startswith("#/components/parameters/"):
        return "$params." + ref.split("/")[-1]
    # Fallback: just the last segment
    return "$" + ref.split("/")[-1]


def extract_ref(obj: dict) -> str | None:
    """Pull $ref from a dict, return short form or None."""
    ref = obj.get("$ref")
    return short_ref(ref) if ref else None


def raw_ref(obj: dict) -> str | None:
    """Pull raw schema name from $ref."""
    ref = obj.get("$ref", "")
    if ref.startswith("#/components/schemas/"):
        return ref.split("/")[-1]
    return None


def truncate_desc(desc: str | None, max_len: int = 120) -> str | None:
    """Keep first sentence, cap at max_len."""
    if not desc:
        return None
    # First sentence: split on ". " or ".\n"
    for sep in (". ", ".\n"):
        idx = desc.find(sep)
        if idx != -1:
            desc = desc[: idx + 1]
            break
    desc = desc.strip()
    if len(desc) > max_len:
        desc = desc[: max_len - 3] + "..."
    return desc


def condense_property(prop: dict) -> dict:
    """Condense a single schema property to 1-level summary."""
    # Direct $ref
    ref = extract_ref(prop)
    if ref:
        return {"$ref": ref}

    result = {}

    # oneOf / anyOf / allOf at property level
    for combo in ("oneOf", "anyOf", "allOf"):
        if combo in prop:
            variants = []
            for v in prop[combo]:
                r = extract_ref(v)
                if r:
                    variants.append(r)
                elif v.get("type"):
                    variants.append(v["type"])
                else:
                    variants.append("?")
            result[combo] = variants
            return result

    ptype = prop.get("type")
    if ptype:
        result["type"] = ptype

    # Array items
    if ptype == "array" and "items" in prop:
        items = prop["items"]
        iref = extract_ref(items)
        if iref:
            result["items"] = iref
        elif items.get("type"):
            result["items"] = items["type"]
        elif "oneOf" in items:
            variants = []
            for v in items["oneOf"]:
                r = extract_ref(v)
                variants.append(r if r else v.get("type", "?"))
            result["items"] = {"oneOf": variants}

    # Enum values (compact, important for translation work)
    if "enum" in prop:
        result["enum"] = prop["enum"]

    # Nullable
    if prop.get("nullable"):
        result["nullable"] = True

    # Default (only if simple scalar)
    default = prop.get("default")
    if default is not None and isinstance(default, (str, int, float, bool)):
        result["default"] = default

    return result


def condense_schema(name: str, schema: dict) -> dict:
    """Condense a schema to type, description, required, 1-level properties."""
    result = {}

    # Handle composition schemas (no own type, just oneOf/anyOf/allOf)
    for combo in ("oneOf", "anyOf", "allOf"):
        if combo in schema:
            variants = []
            for v in schema[combo]:
                r = extract_ref(v)
                if r:
                    variants.append(r)
                elif v.get("type"):
                    variants.append(v["type"])
                else:
                    variants.append("?")
            result[combo] = variants

    stype = schema.get("type")
    if stype:
        result["type"] = stype

    desc = truncate_desc(schema.get("description"))
    if desc:
        result["description"] = desc

    if "enum" in schema:
        result["enum"] = schema["enum"]

    required = schema.get("required")
    if required:
        result["required"] = required

    # Properties (1-level)
    props = schema.get("properties", {})
    if props:
        condensed_props = {}
        for pname, pval in props.items():
            condensed_props[pname] = condense_property(pval)
        result["properties"] = condensed_props

    # Array items at schema level
    if stype == "array" and "items" in schema:
        items = schema["items"]
        iref = extract_ref(items)
        if iref:
            result["items"] = iref
        elif items.get("type"):
            result["items"] = items["type"]

    # additionalProperties
    addl = schema.get("additionalProperties")
    if isinstance(addl, dict):
        r = extract_ref(addl)
        if r:
            result["additionalProperties"] = r
        elif addl.get("type"):
            result["additionalProperties"] = addl["type"]

    return result


def extract_body_ref(operation: dict) -> str | None:
    """Extract request body schema ref from an operation."""
    rb = operation.get("requestBody", {})

    # Direct $ref on requestBody
    ref = extract_ref(rb)
    if ref:
        return ref

    content = rb.get("content", {})
    for media_type in ("application/json", "multipart/form-data"):
        mt = content.get(media_type, {})
        schema = mt.get("schema", {})
        ref = extract_ref(schema)
        if ref:
            return ref
    return None


def extract_response_refs(operation: dict) -> dict[str, str]:
    """Extract response schema refs from an operation."""
    responses = operation.get("responses", {})
    result = {}
    for status, resp in responses.items():
        # Response-level $ref
        ref = extract_ref(resp)
        if ref:
            result[str(status)] = ref
            continue
        content = resp.get("content", {})
        for media_type in ("application/json",):
            mt = content.get(media_type, {})
            schema = mt.get("schema", {})
            ref = extract_ref(schema)
            if ref:
                result[str(status)] = ref
    return result


def condense_endpoint(path: str, method: str, operation: dict) -> dict:
    """Condense a single endpoint."""
    ep = {
        "method": method.upper(),
        "path": path,
    }

    op_id = operation.get("operationId")
    if op_id:
        ep["operation_id"] = op_id

    summary = truncate_desc(operation.get("summary"))
    if summary:
        ep["summary"] = summary

    tags = operation.get("tags", [])
    if tags:
        ep["tags"] = tags

    # Parameters
    params = operation.get("parameters", [])
    if params:
        condensed_params = []
        for p in params:
            # Could be a $ref to a shared parameter
            ref = extract_ref(p)
            if ref:
                condensed_params.append(ref)
            else:
                cp = {"name": p.get("name", "?"), "in": p.get("in", "?")}
                schema = p.get("schema", {})
                if schema.get("type"):
                    cp["type"] = schema["type"]
                if p.get("required"):
                    cp["required"] = True
                condensed_params.append(cp)
        ep["parameters"] = condensed_params

    body_ref = extract_body_ref(operation)
    if body_ref:
        ep["request_body"] = body_ref

    resp_refs = extract_response_refs(operation)
    if resp_refs:
        ep["responses"] = resp_refs

    return ep


def collect_refs_from_schema(schema: dict) -> set[str]:
    """Collect all raw schema names referenced by a schema dict."""
    refs = set()

    def _walk(obj):
        if isinstance(obj, dict):
            r = raw_ref(obj)
            if r:
                refs.add(r)
            for v in obj.values():
                _walk(v)
        elif isinstance(obj, list):
            for item in obj:
                _walk(item)

    _walk(schema)
    return refs


def collect_reachable_schemas(roots: set[str], all_schemas: dict) -> set[str]:
    """BFS from root schema names to find all transitively referenced schemas."""
    visited = set()
    frontier = set(roots)

    while frontier:
        current = frontier.pop()
        if current in visited:
            continue
        visited.add(current)
        schema = all_schemas.get(current, {})
        child_refs = collect_refs_from_schema(schema)
        frontier.update(child_refs - visited)

    return visited


def collect_endpoint_schema_roots(endpoint: dict) -> set[str]:
    """Extract raw schema names from a condensed endpoint's refs."""
    roots = set()

    def _extract(val):
        if isinstance(val, str) and val.startswith("$"):
            name = val.lstrip("$")
            if not name.startswith("params."):
                roots.add(name)
        elif isinstance(val, dict):
            for v in val.values():
                _extract(v)
        elif isinstance(val, list):
            for v in val:
                _extract(v)

    _extract(endpoint.get("request_body"))
    _extract(endpoint.get("responses"))
    _extract(endpoint.get("parameters"))
    return roots


def condense_spec(raw_yaml: str, tags: list[str] | None) -> dict:
    """Main orchestrator: parse, filter, condense."""
    spec = yaml.safe_load(raw_yaml)

    info = spec.get("info", {})
    all_schemas = spec.get("components", {}).get("schemas", {})
    paths = spec.get("paths", {})

    # Normalize tag filter to lowercase
    tag_filter = None
    if tags:
        tag_filter = {t.lower() for t in tags}

    # Condense endpoints
    endpoints = []
    for path, methods in paths.items():
        for method in ("get", "post", "put", "patch", "delete", "head", "options"):
            operation = methods.get(method)
            if operation is None:
                continue

            # Tag filter
            if tag_filter:
                op_tags = [t.lower() for t in operation.get("tags", [])]
                if not tag_filter.intersection(op_tags):
                    continue

            endpoints.append(condense_endpoint(path, method, operation))

    # Collect schema roots from filtered endpoints
    schema_roots = set()
    for ep in endpoints:
        schema_roots.update(collect_endpoint_schema_roots(ep))

    # If no tag filter, include all schemas
    if tag_filter:
        reachable = collect_reachable_schemas(schema_roots, all_schemas)
    else:
        reachable = set(all_schemas.keys())

    # Condense schemas (sorted for stable output)
    schemas = {}
    for name in sorted(reachable):
        if name in all_schemas:
            schemas[name] = condense_schema(name, all_schemas[name])

    total_endpoints = sum(
        1
        for methods in paths.values()
        for m in ("get", "post", "put", "patch", "delete", "head", "options")
        if methods.get(m)
    )

    result = {
        "meta": {
            "source": "openai/openai-openapi (manual_spec branch)",
            "title": info.get("title", "?"),
            "version": info.get("version", "?"),
            "generated": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "endpoints_total": total_endpoints,
            "endpoints_included": len(endpoints),
            "schemas_total": len(all_schemas),
            "schemas_included": len(schemas),
        },
        "endpoints": endpoints,
        "schemas": schemas,
    }

    if tag_filter:
        result["meta"]["filtered_by_tags"] = sorted(tag_filter)

    return result


# Custom YAML representer to keep output clean
def _str_representer(dumper, data):
    """Use literal block style for multiline strings, plain otherwise."""
    if "\n" in data:
        return dumper.represent_scalar("tag:yaml.org,2002:str", data, style="|")
    return dumper.represent_scalar("tag:yaml.org,2002:str", data)


def _ordered_dict_representer(dumper, data):
    return dumper.represent_mapping("tag:yaml.org,2002:map", data.items())


class CleanDumper(yaml.SafeDumper):
    pass


CleanDumper.add_representer(str, _str_representer)
CleanDumper.add_representer(OrderedDict, _ordered_dict_representer)


def format_output(data: dict, as_json: bool) -> str:
    """Format the condensed spec as YAML or JSON."""
    if as_json:
        return json.dumps(data, indent=2, ensure_ascii=False)

    meta = data["meta"]
    header_lines = [
        f"# {meta['title']} - Condensed Spec",
        f"# Source: {meta['source']}",
        f"# Version: {meta['version']}",
        f"# Generated: {meta['generated']}",
        f"# Endpoints: {meta['endpoints_total']} total, {meta['endpoints_included']} included",
        f"# Schemas: {meta['schemas_total']} total, {meta['schemas_included']} included",
    ]
    if "filtered_by_tags" in meta:
        header_lines.append(f"# Filtered by tags: {', '.join(meta['filtered_by_tags'])}")
    header_lines.append("")

    body = yaml.dump(
        {"endpoints": data["endpoints"], "schemas": data["schemas"]},
        Dumper=CleanDumper,
        default_flow_style=False,
        sort_keys=False,
        width=120,
        allow_unicode=True,
    )

    return "\n".join(header_lines) + body


def main():
    args = parse_args()

    raw = download_spec(args.url, args.cache_dir, args.no_cache)
    condensed = condense_spec(raw, args.tags)
    output = format_output(condensed, args.json)

    if args.output:
        Path(args.output).write_text(output, encoding="utf-8")
        print(f"Written to {args.output}", file=sys.stderr)
        # Print size stats
        lines = output.count("\n")
        print(f"Output: {lines} lines, {len(output):,} bytes", file=sys.stderr)
    else:
        sys.stdout.write(output)


if __name__ == "__main__":
    main()
