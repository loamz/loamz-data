#!/usr/bin/env python3
"""Checks that every (method, path) in specs/data/v1/openapi.yaml appears in docs-site/docs/api/."""

import glob
import os
import sys
import yaml

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(ROOT)
OPENAPI_PATH = os.path.join(REPO_ROOT, "specs", "data", "v1", "openapi.yaml")
API_DOCS_DIR = os.path.join(ROOT, "docs", "api")

HTTP_METHODS = ("get", "post", "put", "patch", "delete", "options", "head")


def main() -> None:
    if not os.path.exists(OPENAPI_PATH):
        sys.exit(f"OpenAPI spec not found at {OPENAPI_PATH}")

    with open(OPENAPI_PATH, "r", encoding="utf-8") as f:
        spec = yaml.safe_load(f)

    expected: set[tuple[str, str]] = set()
    for path, path_item in spec.get("paths", {}).items():
        for m in HTTP_METHODS:
            if m in path_item:
                expected.add((m.upper(), path))

    # Read all generated docs
    all_content = ""
    for md_file in glob.glob(os.path.join(API_DOCS_DIR, "*.md")):
        with open(md_file, "r", encoding="utf-8") as f:
            all_content += f.read() + "\n"

    missing: list[tuple[str, str]] = []
    for method, path in sorted(expected):
        needle = f"`{method}` {path}"
        if needle not in all_content:
            missing.append((method, path))

    if missing:
        print(f"Error: {len(missing)} endpoints missing from documentation:")
        for m, p in missing:
            print(f"  - {m} {p}")
        sys.exit(1)

    print(f"Success: All {len(expected)} endpoints verified in docs-site/docs/api/")


if __name__ == "__main__":
    main()
