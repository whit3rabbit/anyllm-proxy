#!/usr/bin/env python3
"""Query the OpenAI OpenAPI spec for specific endpoints, schemas, or fields.

Designed for LLM consumption: outputs concise, structured information about
specific parts of the OpenAI API spec without loading the entire ~26k line file.

Requires: pip install pyyaml
Setup: python3 -m venv scripts/.venv && scripts/.venv/bin/pip install pyyaml
"""

import argparse
import json
import re
import sys
from pathlib import Path
from urllib.request import urlopen

try:
    import yaml
except ImportError:
    print("PyYAML required: pip install pyyaml", file=sys.stderr)
    sys.exit(1)

# Resolve repo root via git or fallback to cwd
import subprocess
try:
    _root = subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"],
        stderr=subprocess.DEVNULL, text=True
    ).strip()
    REPO_ROOT = Path(_root)
except (subprocess.CalledProcessError, FileNotFoundError):
    REPO_ROOT = Path.cwd()
CACHE_DIR = REPO_ROOT / "scripts" / ".cache"
SPEC_FILE = CACHE_DIR / "openapi.yaml"

DEFAULT_URL = (
    "https://raw.githubusercontent.com/openai/openai-openapi/"
    "refs/heads/manual_spec/openapi.yaml"
)


def ensure_spec() -> dict:
    """Load spec from cache, downloading if needed."""
    if not SPEC_FILE.exists():
        print(f"Downloading spec to {SPEC_FILE}...", file=sys.stderr)
        CACHE_DIR.mkdir(parents=True, exist_ok=True)
        with urlopen(DEFAULT_URL, timeout=60) as resp:
            data = resp.read().decode("utf-8")
        SPEC_FILE.write_text(data, encoding="utf-8")
        print(f"Cached ({len(data):,} bytes)", file=sys.stderr)
        return yaml.safe_load(data)

    return yaml.safe_load(SPEC_FILE.read_text(encoding="utf-8"))


def short_ref(ref: str) -> str:
    if ref.startswith("#/components/schemas/"):
        return "$" + ref.split("/")[-1]
    return ref


def resolve_refs(obj, depth=0):
    """Replace $ref with short form, limit depth."""
    if depth > 2:
        return "..."
    if isinstance(obj, dict):
        if "$ref" in obj:
            return short_ref(obj["$ref"])
        return {k: resolve_refs(v, depth + 1) for k, v in obj.items()
                if k not in ("example", "examples", "x-oaiMeta", "x-oaiTypeLabel")}
    if isinstance(obj, list):
        return [resolve_refs(v, depth + 1) for v in obj]
    return obj


def cmd_tags(spec: dict, _args):
    """List all tags with endpoint counts."""
    tag_counts = {}
    for _path, methods in spec.get("paths", {}).items():
        for m in ("get", "post", "put", "patch", "delete"):
            op = methods.get(m)
            if op:
                for t in op.get("tags", ["untagged"]):
                    tag_counts[t] = tag_counts.get(t, 0) + 1
    for tag in sorted(tag_counts):
        print(f"  {tag}: {tag_counts[tag]} endpoints")


def cmd_endpoints(spec: dict, args):
    """List endpoints, optionally filtered by tag or path pattern."""
    paths = spec.get("paths", {})
    tag_filter = args.filter.lower() if args.filter else None

    for path, methods in sorted(paths.items()):
        for m in ("get", "post", "put", "patch", "delete"):
            op = methods.get(m)
            if not op:
                continue

            tags = op.get("tags", [])

            # Filter by tag (case-insensitive) or path substring
            if tag_filter:
                tag_match = any(tag_filter in t.lower() for t in tags)
                path_match = tag_filter in path.lower()
                if not tag_match and not path_match:
                    continue

            op_id = op.get("operationId", "")
            summary = op.get("summary", "")
            # Truncate summary
            if len(summary) > 80:
                summary = summary[:77] + "..."
            print(f"  {m.upper():6s} {path}")
            print(f"         id: {op_id}")
            if summary:
                print(f"         {summary}")
            if tags:
                print(f"         tags: {', '.join(tags)}")
            print()


def cmd_endpoint(spec: dict, args):
    """Show detailed info for a specific endpoint by operationId or path."""
    paths = spec.get("paths", {})
    target = args.target.lower()

    for path, methods in paths.items():
        for m in ("get", "post", "put", "patch", "delete"):
            op = methods.get(m)
            if not op:
                continue

            op_id = (op.get("operationId") or "").lower()
            if target != op_id and target != path.lower() and target != f"{m} {path}".lower():
                continue

            # Found it
            print(f"  {m.upper()} {path}")
            print(f"  operationId: {op.get('operationId', 'N/A')}")
            print(f"  tags: {', '.join(op.get('tags', []))}")
            if op.get("summary"):
                print(f"  summary: {op['summary']}")
            if op.get("description"):
                desc = op["description"].strip()
                if len(desc) > 300:
                    desc = desc[:297] + "..."
                print(f"  description: {desc}")

            # Parameters
            params = op.get("parameters", [])
            if params:
                print(f"\n  parameters:")
                for p in params:
                    if "$ref" in p:
                        print(f"    - {short_ref(p['$ref'])}")
                    else:
                        req = " (required)" if p.get("required") else ""
                        ptype = p.get("schema", {}).get("type", "?")
                        print(f"    - {p.get('name', '?')} ({p.get('in', '?')}, {ptype}){req}")

            # Request body
            rb = op.get("requestBody", {})
            if "$ref" in rb:
                print(f"\n  request_body: {short_ref(rb['$ref'])}")
            else:
                content = rb.get("content", {})
                for ct, ct_val in content.items():
                    schema = ct_val.get("schema", {})
                    if "$ref" in schema:
                        print(f"\n  request_body ({ct}): {short_ref(schema['$ref'])}")
                    else:
                        print(f"\n  request_body ({ct}): inline schema")

            # Responses
            responses = op.get("responses", {})
            if responses:
                print(f"\n  responses:")
                for status, resp in responses.items():
                    if "$ref" in resp:
                        print(f"    {status}: {short_ref(resp['$ref'])}")
                    else:
                        content = resp.get("content", {})
                        desc = resp.get("description", "")
                        for ct, ct_val in content.items():
                            schema = ct_val.get("schema", {})
                            if "$ref" in schema:
                                print(f"    {status}: {short_ref(schema['$ref'])} - {desc}")
                            else:
                                print(f"    {status}: inline - {desc}")
                        if not content:
                            print(f"    {status}: {desc}")
            return

    print(f"  Not found: {args.target}", file=sys.stderr)
    print(f"  Try: query-openai-schema.py endpoints --filter {args.target}", file=sys.stderr)
    sys.exit(1)


def cmd_schema(spec: dict, args):
    """Show a specific schema with properties, types, and refs."""
    schemas = spec.get("components", {}).get("schemas", {})
    target = args.name

    # Case-insensitive lookup
    match = None
    for name in schemas:
        if name.lower() == target.lower():
            match = name
            break

    if not match:
        # Try substring match
        candidates = [n for n in schemas if target.lower() in n.lower()]
        if not candidates:
            print(f"  Schema not found: {target}", file=sys.stderr)
            sys.exit(1)
        if len(candidates) == 1:
            match = candidates[0]
        else:
            print(f"  Multiple matches for '{target}':")
            for c in sorted(candidates):
                print(f"    {c}")
            return

    schema = schemas[match]
    depth = args.depth if hasattr(args, "depth") else 1

    print(f"  {match}:")
    _print_schema(schema, schemas, indent=4, depth=depth, max_depth=depth)


def _print_schema(schema, all_schemas, indent=4, depth=1, max_depth=1):
    """Recursively print schema details."""
    pad = " " * indent

    if schema.get("type"):
        print(f"{pad}type: {schema['type']}")

    if schema.get("description"):
        desc = schema["description"].strip().split("\n")[0]
        if len(desc) > 120:
            desc = desc[:117] + "..."
        print(f"{pad}description: {desc}")

    if schema.get("enum"):
        vals = schema["enum"]
        print(f"{pad}enum: {vals}")

    if schema.get("required"):
        print(f"{pad}required: {schema['required']}")

    # Composition
    for combo in ("oneOf", "anyOf", "allOf"):
        if combo in schema:
            print(f"{pad}{combo}:")
            for v in schema[combo]:
                if "$ref" in v:
                    ref_name = v["$ref"].split("/")[-1]
                    print(f"{pad}  - {short_ref(v['$ref'])}")
                    if depth > 0 and ref_name in all_schemas:
                        _print_schema(all_schemas[ref_name], all_schemas,
                                      indent=indent + 6, depth=depth - 1, max_depth=max_depth)
                elif v.get("type"):
                    print(f"{pad}  - {v['type']}")
                else:
                    print(f"{pad}  - (inline)")

    # Properties
    props = schema.get("properties", {})
    if props:
        print(f"{pad}properties:")
        for pname, pval in props.items():
            _print_property(pname, pval, all_schemas, indent + 2, depth)

    # Array items
    if schema.get("type") == "array" and "items" in schema:
        items = schema["items"]
        if "$ref" in items:
            print(f"{pad}items: {short_ref(items['$ref'])}")
        elif items.get("type"):
            print(f"{pad}items: {items['type']}")

    # additionalProperties
    addl = schema.get("additionalProperties")
    if isinstance(addl, dict):
        if "$ref" in addl:
            print(f"{pad}additionalProperties: {short_ref(addl['$ref'])}")
        elif addl.get("type"):
            print(f"{pad}additionalProperties: {addl['type']}")


def _print_property(name, prop, all_schemas, indent, depth):
    """Print a single property."""
    pad = " " * indent

    if "$ref" in prop:
        ref_name = prop["$ref"].split("/")[-1]
        line = f"{pad}{name}: {short_ref(prop['$ref'])}"
        if prop.get("description"):
            desc = prop["description"].strip().split("\n")[0][:60]
            line += f"  # {desc}"
        print(line)
        if depth > 0 and ref_name in all_schemas:
            _print_schema(all_schemas[ref_name], all_schemas,
                          indent=indent + 4, depth=depth - 1, max_depth=depth)
        return

    parts = []
    ptype = prop.get("type", "")
    if ptype:
        parts.append(ptype)

    if ptype == "array" and "items" in prop:
        items = prop["items"]
        if "$ref" in items:
            parts[-1] = f"array<{short_ref(items['$ref'])}>"
        elif items.get("type"):
            parts[-1] = f"array<{items['type']}>"

    if prop.get("enum"):
        vals = prop["enum"]
        if len(str(vals)) < 60:
            parts.append(f"enum={vals}")
        else:
            parts.append(f"enum=[{len(vals)} values]")

    if prop.get("nullable"):
        parts.append("nullable")

    if prop.get("default") is not None:
        parts.append(f"default={prop['default']}")

    # oneOf/anyOf at property level
    for combo in ("oneOf", "anyOf"):
        if combo in prop:
            variants = []
            for v in prop[combo]:
                if "$ref" in v:
                    variants.append(short_ref(v["$ref"]))
                elif v.get("type"):
                    variants.append(v["type"])
            parts.append(f"{combo}=[{', '.join(variants)}]")

    line = f"{pad}{name}: {', '.join(parts) if parts else '?'}"

    # Short description
    if prop.get("description"):
        desc = prop["description"].strip().split("\n")[0][:50]
        line += f"  # {desc}"

    print(line)


def cmd_search(spec: dict, args):
    """Search schemas and endpoints by name pattern."""
    pattern = re.compile(args.pattern, re.IGNORECASE)

    schemas = spec.get("components", {}).get("schemas", {})
    paths = spec.get("paths", {})

    # Search schemas
    schema_matches = [n for n in schemas if pattern.search(n)]
    if schema_matches:
        print("  Schemas:")
        for name in sorted(schema_matches):
            stype = schemas[name].get("type", "")
            desc = (schemas[name].get("description") or "")[:60]
            print(f"    {name} ({stype}) {desc}")
        print()

    # Search endpoints by operationId and path
    ep_matches = []
    for path, methods in paths.items():
        for m in ("get", "post", "put", "patch", "delete"):
            op = methods.get(m)
            if not op:
                continue
            op_id = op.get("operationId", "")
            if pattern.search(op_id) or pattern.search(path):
                ep_matches.append((m.upper(), path, op_id))

    if ep_matches:
        print("  Endpoints:")
        for method, path, op_id in ep_matches:
            print(f"    {method:6s} {path}  ({op_id})")
        print()

    # Search property names within schemas
    if args.deep:
        prop_matches = []
        for sname, sval in schemas.items():
            for pname in sval.get("properties", {}):
                if pattern.search(pname):
                    prop_matches.append((sname, pname))

        if prop_matches:
            print("  Properties:")
            for sname, pname in sorted(prop_matches):
                print(f"    {sname}.{pname}")
            print()

    if not schema_matches and not ep_matches and (not args.deep or not prop_matches):
        print(f"  No matches for: {args.pattern}")


def cmd_update(_spec: dict, _args):
    """Re-download the spec, replacing the cached copy."""
    if SPEC_FILE.exists():
        old_size = SPEC_FILE.stat().st_size
        print(f"  Existing cache: {SPEC_FILE} ({old_size:,} bytes)", file=sys.stderr)
    else:
        old_size = 0

    print(f"  Downloading: {DEFAULT_URL}", file=sys.stderr)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    with urlopen(DEFAULT_URL, timeout=60) as resp:
        data = resp.read().decode("utf-8")
    SPEC_FILE.write_text(data, encoding="utf-8")

    new_size = len(data)
    spec = yaml.safe_load(data)
    version = spec.get("info", {}).get("version", "?")
    n_endpoints = sum(
        1 for methods in spec.get("paths", {}).values()
        for m in ("get", "post", "put", "patch", "delete") if methods.get(m)
    )
    n_schemas = len(spec.get("components", {}).get("schemas", {}))

    print(f"  Updated: {new_size:,} bytes (was {old_size:,})", file=sys.stderr)
    print(f"  Version: {version}")
    print(f"  Endpoints: {n_endpoints}")
    print(f"  Schemas: {n_schemas}")


def cmd_diff_fields(spec: dict, args):
    """Show fields of a schema not present in another (for translation gap analysis)."""
    schemas = spec.get("components", {}).get("schemas", {})

    source = schemas.get(args.schema)
    if not source:
        print(f"  Schema not found: {args.schema}", file=sys.stderr)
        sys.exit(1)

    source_props = set(source.get("properties", {}).keys())

    if args.compare:
        compare = schemas.get(args.compare)
        if not compare:
            print(f"  Comparison schema not found: {args.compare}", file=sys.stderr)
            sys.exit(1)
        compare_props = set(compare.get("properties", {}).keys())

        only_source = source_props - compare_props
        only_compare = compare_props - source_props
        shared = source_props & compare_props

        print(f"  Shared ({len(shared)}): {sorted(shared)}")
        print(f"  Only in {args.schema} ({len(only_source)}): {sorted(only_source)}")
        print(f"  Only in {args.compare} ({len(only_compare)}): {sorted(only_compare)}")
    else:
        print(f"  {args.schema} fields ({len(source_props)}):")
        for p in sorted(source_props):
            pval = source["properties"][p]
            ptype = pval.get("type", "")
            if "$ref" in pval:
                ptype = short_ref(pval["$ref"])
            print(f"    {p}: {ptype}")


def main():
    p = argparse.ArgumentParser(
        description="Query the OpenAI OpenAPI spec.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
commands:
  tags                       List all API tags with endpoint counts
  endpoints [--filter X]     List endpoints (filter by tag or path)
  endpoint <target>          Show endpoint detail (by operationId or path)
  schema <name> [--depth N]  Show schema detail (default depth=1)
  search <pattern> [--deep]  Search by regex (--deep includes property names)
  diff <schema> [--compare]  Show/compare schema fields
  update                     Re-download the spec from GitHub
        """,
    )
    sub = p.add_subparsers(dest="command")

    sub.add_parser("tags", help="List all API tags")

    ep_list = sub.add_parser("endpoints", help="List endpoints")
    ep_list.add_argument("--filter", "-f", help="Filter by tag or path substring")

    ep_detail = sub.add_parser("endpoint", help="Show endpoint detail")
    ep_detail.add_argument("target", help="operationId or path (e.g. createChatCompletion)")

    schema_cmd = sub.add_parser("schema", help="Show schema detail")
    schema_cmd.add_argument("name", help="Schema name (case-insensitive, substring match)")
    schema_cmd.add_argument("--depth", "-d", type=int, default=1,
                            help="Ref expansion depth (default: 1)")

    search_cmd = sub.add_parser("search", help="Search by regex")
    search_cmd.add_argument("pattern", help="Regex pattern")
    search_cmd.add_argument("--deep", action="store_true",
                            help="Also search property names within schemas")

    diff_cmd = sub.add_parser("diff", help="Show/compare schema fields")
    diff_cmd.add_argument("schema", help="Source schema name")
    diff_cmd.add_argument("--compare", "-c", help="Schema to compare against")

    sub.add_parser("update", help="Re-download the spec from GitHub")

    args = p.parse_args()
    if not args.command:
        p.print_help()
        sys.exit(0)

    # update runs before loading the cached spec
    if args.command == "update":
        cmd_update(None, args)
        return

    spec = ensure_spec()

    commands = {
        "tags": cmd_tags,
        "endpoints": cmd_endpoints,
        "endpoint": cmd_endpoint,
        "schema": cmd_schema,
        "search": cmd_search,
        "diff": cmd_diff_fields,
    }
    commands[args.command](spec, args)


if __name__ == "__main__":
    main()
