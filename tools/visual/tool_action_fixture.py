#!/usr/bin/env python3
"""Generate tool-button capture input from the shared inventory and theme fixture."""
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("inventory", type=Path)
parser.add_argument("theme_fixture", type=Path, help="toolbar-fixture output for the desired theme")
parser.add_argument("--column-width", type=int, choices=[120, 226], required=True)
args = parser.parse_args()
inventory = json.loads(args.inventory.read_text())
theme = json.loads(args.theme_fixture.read_text())


def actions_for(platform):
    actions = {}
    for scenario in inventory["platforms"][platform]["tool_scenarios"]:
        for item in scenario["actions"]:
            command = item["command"]
            previous = actions.setdefault(command["id"], item)
            assert previous["checkable"] == item["checkable"]
            assert previous["command"]["label"] == command["label"]
    return list(actions.values())


actions = actions_for("mac")
identity = lambda items: {(item["command"]["id"], item["command"]["label"], item["checkable"]) for item in items}
assert identity(actions) == identity(actions_for("ios")), "Apple tool-action inventories differ"
assert len(actions) == 6, "Review capture height and coverage when the shipped action set changes"
print(json.dumps({
    "schema": 1, "actions": actions, "theme": theme["theme"],
    "column_width": args.column_width, "width": 4 * args.column_width + 36,
    "height": 420, "scale": 2, "text_size": theme["text_size"], "palette": theme["palette"],
}, indent=2))
