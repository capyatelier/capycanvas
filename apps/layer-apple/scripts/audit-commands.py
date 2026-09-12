#!/usr/bin/env python3
"""Detect catalog drift without mistaking inventory coverage for acceptance."""
import argparse
from collections import Counter
import json
from pathlib import Path

APP = Path(__file__).resolve().parents[1]


def objects(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from objects(child)
    elif isinstance(value, list):
        for child in value:
            yield from objects(child)


def signature(action):
    return json.dumps(action, sort_keys=True)


def shared_properties(coverage, platform, model):
    scenarios = model["property_scenarios"]
    expected = ["paint", "paper", "group"] + ["filter:" + item["id"]
                for item in model["initial"]["state"]["adjustments"]]
    if Counter(item["name"] for item in scenarios) != Counter(expected):
        raise ValueError(f"{platform} missing or duplicated property scenarios")
    kinds = set()
    count = 0
    for scenario in scenarios:
        name = scenario["name"]
        if scenario["error"] or scenario["lock_error"]:
            raise ValueError(f"{platform} failed property setup: {name}")
        view, locked = scenario["properties"], scenario["locked_properties"]
        if not view["enabled"]:
            raise ValueError(f"{platform} initial properties unavailable: {name}")
        if scenario["lock_action"] is None:
            if name != "paper" or locked is not None or scenario["locked_edit"] is not None:
                raise ValueError(f"{platform} property locking capability drift: {name}")
        elif locked["enabled"] or locked["controls"] != view["controls"]:
            raise ValueError(f"{platform} property lock changed values/schema or remained editable: {name}")
        elif view["controls"] and (not scenario["locked_edit"]["error"] or not scenario["locked_edit"]["values_unchanged"]):
            raise ValueError(f"{platform} locked property edit was not rejected intact: {name}")
        controls = {item["key"]: item for item in view["controls"]}
        if len(controls) != len(view["controls"]) or Counter(item["key"] for item in scenario["edits"]) != Counter(controls.keys()):
            raise ValueError(f"{platform} missing or duplicated property edits: {name}")
        for edit in scenario["edits"]:
            control = controls[edit["key"]]
            kinds.add(control["kind"]["kind"])
            result = edit["result"]
            if edit["error"] or not result:
                raise ValueError(f"{platform} property edit failed: {name}/{edit['key']}: {edit['error']}")
            action = result["action"]
            if (action["type"] != "effect" or action["action"]["op"] != "set"
                    or action["action"]["key"] != edit["key"] or action["action"]["layer"] != view["layer"]
                    or action["action"]["value"]["kind"] != control["kind"]["kind"]):
                raise ValueError(f"{platform} property action routing mismatch: {name}/{edit['key']}")
            if (result["before"] != control["value"] or result["before"] == result["edited"] or result["reset"] != control["default"]
                    or not result["undo_restored_all_properties"] or not result["redo_restored_all_properties"]):
                raise ValueError(f"{platform} property edit/history/reset mismatch: {name}/{edit['key']}")
            count += 1
    if kinds != set(coverage["property_kinds"]):
        raise ValueError(f"{platform} property kind drift: {sorted(kinds)}")
    if not coverage["property_workflows"].get(platform):
        raise ValueError(f"Missing {platform} property workflow review")
    for path in [*coverage["property_kinds"].values(), *coverage["property_workflows"]["checks"]]:
        if not (APP / path).is_file():
            raise ValueError(f"Missing property handler reference: {path}")
    print(f"{platform}: {len(scenarios)} property scenarios, {count} edit/undo/redo/reset routes, "
          f"{len(kinds)} property kinds; locked schemas retained")


def shared_controls(inventory, coverage, platform, model):
    routes = coverage["workspace_commands"]
    if len(routes["commands"]) != len(set(routes["commands"])):
        raise ValueError("Duplicated workspace command reviews")
    workspace_commands = {node["command"]["type"] for node in objects(model)
                          if node.get("type") in ["workspace_manager", "workspace"]
                          and isinstance(node.get("command"), dict)}
    if workspace_commands != set(routes["commands"]):
        raise ValueError(f"{platform} workspace command drift: {sorted(workspace_commands)}")
    for path in [routes["handler"], *routes["checks"]]:
        if not (APP / path).is_file():
            raise ValueError(f"Missing workspace review reference: {path}")
    if not routes.get(platform):
        raise ValueError(f"Missing {platform} workspace service review")
    panels = [panel for workspace in model["workspace_scenarios"] for panel in workspace["panels"]]
    controls = {item["control"] for panel in panels for item in panel["controls"]}
    kinds = {row["kind"]["type"] for preferences in model["preferences"]
             for page in preferences["pages"] for group in page["groups"] for row in group["rows"]}
    for label, observed in [("panel_controls", controls), ("preference_kinds", kinds)]:
        if observed != set(coverage[label]):
            raise ValueError(f"{platform} {label} drift: {sorted(observed)}")
        for path in coverage[label].values():
            if not (APP / path).is_file():
                raise ValueError(f"Missing {label} handler reference: {path}")
    tools = model["tool_scenarios"]
    selections = [signature(tool["select"]) for tool in tools]
    if len(selections) != len(set(selections)):
        raise ValueError(f"{platform} duplicated tool choices")
    unresolved = [tool["select"] for tool in tools if tool["error"]]
    if unresolved:
        raise ValueError(f"{platform} unresolved tool choices: {unresolved}; use inventory --gpu")
    required = [{"type": "invoke", "command": command} for command in inventory["catalog"]["tool_commands"]]
    required += [{"type": "select_brush", "id": brush["id"]}
                 for category in inventory["catalog"]["brush_categories"] for brush in category["brushes"]]
    required += [item["action"] for tool in tools for group in ["groups", "subtools"]
                 for item in tool["tool_set"][group]]
    missing = set(map(signature, required)) - set(selections)
    if missing:
        raise ValueError(f"{platform} unvisited tool choices: {sorted(missing)}")
    settings = {setting["id"] for tool in tools for setting in tool["settings"]}
    for tool in tools:
        if len({setting["id"] for setting in tool["settings"]}) != len(tool["settings"]):
            raise ValueError(f"{platform} ambiguous tool setting IDs for {tool['select']}")
        if any(item["command"]["id"] not in inventory["commands"] for item in tool["actions"]):
            raise ValueError(f"{platform} unknown tool action for {tool['select']}")
    failed = [command["id"] for command in model["commands"] if command["invocation"]["error"]]
    if failed:
        raise ValueError(f"{platform} initially enabled commands rejected: {failed}")
    print(f"{platform}: {len(tools)} resolved tool choices, {len(settings)} setting IDs, "
          f"{len(controls)} panel control types, {len(kinds)} preference kinds, "
          f"{len(workspace_commands)} workspace service commands")


def audit(inventory, coverage):
    if inventory.get("schema") not in [2, 3, 4] or coverage.get("schema") != 3:
        raise ValueError("Unsupported inventory or command-coverage schema")
    if inventory["schema"] == 2:
        print("Schema 2 input: dynamic controls and workspace service routes are not checked.")
    if inventory["schema"] < 4:
        print("Older inventory input: filter/layer property schemas and edit routes are not checked.")
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
        if inventory["schema"] >= 3:
            shared_controls(inventory, coverage, platform, model)
        if inventory["schema"] >= 4:
            shared_properties(coverage, platform, model)
        print(f"{platform}: {len(commands)} commands, {len(model['panels'])} panels, "
              f"{len(model['preferences'])} settings pages; unavailable={sorted(unavailable)}")
    print(f"PASS: all {len(catalog)} commands classified in {len(coverage['groups'])} groups for both Apple hosts.")
    print("This checks inventory coverage only. Behavioral, visual, and performance acceptance remain open.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inventory", type=Path, help="JSON from cargo run -p layer-host --example inventory -- --gpu")
    args = parser.parse_args()
    try:
        audit(json.loads(args.inventory.read_text()), json.loads((APP / "command-coverage.json").read_text()))
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"FAIL: {error}\n")


if __name__ == "__main__":
    main()
