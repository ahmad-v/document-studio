#!/usr/bin/env python3
"""Create a review-only license metadata inventory from Cargo metadata."""

import argparse
import json
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path, help="cargo metadata JSON file")
    parser.add_argument("output", type=Path, help="output JSON inventory file")
    args = parser.parse_args()

    metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
    packages = sorted(metadata["packages"], key=lambda package: (package["name"], package["version"]))
    root_id = metadata.get("resolve", {}).get("root")
    root = next((package for package in packages if package["id"] == root_id), None)
    if root is None:
        member_ids = set(metadata.get("workspace_members", []))
        root = next((package for package in packages if package["id"] in member_ids), packages[0])
    components = [
        {
            "name": package["name"],
            "version": package["version"],
            "source": package.get("source"),
            "license_expression": package.get("license"),
            "license_file_declared": bool(package.get("license_file")),
        }
        for package in packages
    ]
    missing = [
        {"name": component["name"], "version": component["version"]}
        for component in components
        if not component["license_expression"] and not component["license_file_declared"]
    ]
    report = {
        "schema_version": 1,
        "project": root["name"],
        "inventory_only": True,
        "license_clearance": "not performed by this metadata inventory",
        "component_count": len(components),
        "components_missing_license_metadata": missing,
        "components": components,
    }
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
