#!/usr/bin/env python3
"""Detect catalog drift without mistaking inventory coverage for acceptance."""
import argparse
from collections import Counter
import json
from pathlib import Path

APP = Path(__file__).resolve().parents[1]


def audit(inventory, coverage):
    if inventory.get("schema") != 2 or coverage.get("schema") != 1:
        raise ValueError("Unsupported inventory or command-coverage schema")
    catalog = inventory["commands"]
    covered = [command for group in coverage["groups"] for command in group["commands"]]
    for label, commands in [("catalog", catalog), ("coverage", covered)]:
        duplicates = sorted(command for command, count in Counter(commands).items() if count != 1)
        if duplicates:
            raise ValueError(f"Duplicate {label} commands: {duplicates}")
    if set(catalog) != set(covered):
        raise ValueError(f"Command coverage drift: missing={sorted(set(catalog) - set(covered))}; "
                         f"stale={sorted(set(covered) - set(catalog))}")
    for group in coverage["groups"]:
        for check in group["checks"]:
            if not (APP / check).is_file():
                raise ValueError(f"Missing check reference: {check}")
        for platform in ["ios", "mac"]:
            if not group.get(platform):
                raise ValueError(f"Missing {platform} review: {group['name']}")
    for platform in ["ios", "mac"]:
        model = inventory["platforms"][platform]
        commands = model["commands"]
        if Counter(c["id"] for c in commands) != Counter(catalog):
            raise ValueError(f"{platform} inventory omits or duplicates command entries")
        unavailable = {c["id"] for c in commands if not c["available"]}
        if unavailable != set(coverage["unavailable"][platform]):
            raise ValueError(f"{platform} capability changed: review unavailable commands {sorted(unavailable)}")
        print(f"{platform}: {len(commands)} commands, {len(model['panels'])} panels, "
              f"{len(model['preferences'])} settings pages; unavailable={sorted(unavailable)}")
    print(f"PASS: all {len(catalog)} commands classified in {len(coverage['groups'])} groups for both Apple hosts.")
    print("This checks inventory coverage only. Behavioral, visual, and performance acceptance remain open.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inventory", type=Path, help="JSON from cargo run -p layer-host --example inventory")
    args = parser.parse_args()
    try:
        audit(json.loads(args.inventory.read_text()), json.loads((APP / "command-coverage.json").read_text()))
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"FAIL: {error}\n")


if __name__ == "__main__":
    main()
