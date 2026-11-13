#!/usr/bin/env python3
"""Generates API documentation pages from specs/data/v1/openapi.yaml."""

import os
import re
import sys
import yaml

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(ROOT)
OPENAPI_PATH = os.path.join(REPO_ROOT, "specs", "data", "v1", "openapi.yaml")
OUTPUT_DIR = os.path.join(ROOT, "docs", "api")

HTTP_METHODS = ("get", "post", "put", "patch", "delete", "options", "head")


def clean_text(text: str | None) -> str:
    if not text:
        return ""
    return text.strip().replace("\n", " ")


def resolve_ref(spec: dict, ref: str) -> dict:
    parts = ref.strip("#/").split("/")
    cur = spec
    for p in parts:
        cur = cur.get(p, {})
    return cur


def format_schema(schema: dict | None, spec: dict) -> str:
    if not schema:
        return ""
    if "$ref" in schema:
        schema = resolve_ref(spec, schema["$ref"])
    t = schema.get("type", "object")
    fmt = schema.get("format", "")
    if fmt:
        t = f"{t} ({fmt})"
    return t


def get_group(path: str) -> str:
    parts = path.strip("/").split("/")
    if len(parts) >= 3 and parts[0] == "api" and parts[1] == "v1":
        return parts[2]
    if parts:
        return parts[0]
    return "general"


def render(spec: dict) -> dict[str, str]:
    paths = spec.get("paths", {})
    grouped: dict[str, list[tuple[str, str, dict]]] = {}

    for path, path_item in paths.items():
        group = get_group(path)
        for method in HTTP_METHODS:
            if method in path_item:
                op = path_item[method]
                grouped.setdefault(group, []).append((method.upper(), path, op))

    pages: dict[str, str] = {}

    for group, endpoints in grouped.items():
        lines = []
        group_title = group.replace("-", " ").title()
        lines.append(f"# {group_title} API\n")
        lines.append(f"Endpoints under `/api/v1/{group}`.\n")

        for method, path, op in endpoints:
            summary = op.get("summary", "")
            description = op.get("description", "")
            lines.append(f"## `{method}` {path}\n")
            if summary:
                lines.append(f"**{summary}**\n")
            if description:
                lines.append(f"{description}\n")

            # Parameters
            params = op.get("parameters", [])
            if params:
                lines.append("### Parameters\n")
                lines.append("| Name | In | Required | Type | Description |")
                lines.append("|---|---|---|---|---|")
                for p in params:
                    if "$ref" in p:
                        p = resolve_ref(spec, p["$ref"])
                    p_name = p.get("name", "")
                    p_in = p.get("in", "")
                    p_req = "Yes" if p.get("required") else "No"
                    p_schema = p.get("schema", {})
                    p_type = format_schema(p_schema, spec)
                    p_desc = clean_text(p.get("description", ""))
                    lines.append(f"| `{p_name}` | `{p_in}` | {p_req} | `{p_type}` | {p_desc} |")
                lines.append("")

            # Request Body
            req_body = op.get("requestBody", {})
            if req_body:
                if "$ref" in req_body:
                    req_body = resolve_ref(spec, req_body["$ref"])
                lines.append("### Request Body\n")
                content = req_body.get("content", {})
                for ct, media in content.items():
                    schema = media.get("schema", {})
                    schema_type = format_schema(schema, spec)
                    lines.append(f"- **Content-Type**: `{ct}` (`{schema_type}`)")
                    if "$ref" in schema:
                        resolved = resolve_ref(spec, schema["$ref"])
                        props = resolved.get("properties", {})
                        if props:
                            lines.append("\n  **Fields:**\n")
                            for prop_name, prop_data in props.items():
                                pt = prop_data.get("type", "any")
                                pdesc = clean_text(prop_data.get("description", ""))
                                lines.append(f"  - `{prop_name}` (`{pt}`): {pdesc}")
                lines.append("")

            # Responses
            responses = op.get("responses", {})
            if responses:
                lines.append("### Responses\n")
                lines.append("| Status | Description |")
                lines.append("|---|---|")
                for status_code, resp_data in responses.items():
                    if "$ref" in resp_data:
                        resp_data = resolve_ref(spec, resp_data["$ref"])
                    desc = clean_text(resp_data.get("description", ""))
                    lines.append(f"| `{status_code}` | {desc} |")
                lines.append("")

            lines.append("---\n")

        pages[group] = "\n".join(lines)

    # Index page
    index_lines = [
        "# API Reference",
        "",
        "Loamz Data exposes a versioned REST API at `/api/v1` adhering strictly to RFC 9457 for error formats.",
        "",
        "## Endpoints by Resource Group",
        "",
        "| Group | Endpoints | Description |",
        "|---|---|---|",
    ]
    for group in sorted(grouped.keys()):
        count = len(grouped[group])
        index_lines.append(f"| [{group.title()}]({group}.md) | {count} | Endpoints for `{group}` |")

    pages["index"] = "\n".join(index_lines) + "\n"
    return pages


def main() -> None:
    if not os.path.exists(OPENAPI_PATH):
        sys.exit(f"OpenAPI spec not found at {OPENAPI_PATH}")

    with open(OPENAPI_PATH, "r", encoding="utf-8") as f:
        spec = yaml.safe_load(f)

    os.makedirs(OUTPUT_DIR, exist_ok=True)
    pages = render(spec)

    for group, content in pages.items():
        out_file = os.path.join(OUTPUT_DIR, f"{group}.md")
        with open(out_file, "w", encoding="utf-8") as f:
            f.write(content)
        print(f"Generated {out_file}")


if __name__ == "__main__":
    main()
