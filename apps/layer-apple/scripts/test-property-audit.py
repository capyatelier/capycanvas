#!/usr/bin/env python3
"""Reject missing or contradictory property evidence in a generated inventory."""
from copy import deepcopy
import json
from pathlib import Path
import runpy
import sys

app = Path(__file__).resolve().parents[1]
audit = runpy.run_path(str(app / "scripts/audit-commands.py"))["shared_properties"]
inventory = json.loads(Path(sys.argv[1]).read_text())
coverage = json.loads((app / "command-coverage.json").read_text())

def scenario(model):
    return model["property_scenarios"][0]

probes = {
    "missing filter": lambda m, c: m["property_scenarios"].pop(),
    "duplicate filter": lambda m, c: m["property_scenarios"].append(deepcopy(m["property_scenarios"][-1])),
    "unknown property kind": lambda m, c: scenario(m)["properties"]["controls"][0]["kind"].update(kind="matrix"),
    "missing edit": lambda m, c: scenario(m)["edits"].pop(),
    "wrong action target": lambda m, c: scenario(m)["edits"][0]["result"]["action"]["action"].update(layer=999),
    "accepted locked edit": lambda m, c: scenario(m)["locked_edit"].update(error=None),
    "enabled locked controls": lambda m, c: scenario(m)["locked_properties"].update(enabled=True),
    "failed history": lambda m, c: scenario(m)["edits"][0]["result"].update(undo_restored_all_properties=False),
    "incorrect reset": lambda m, c: scenario(m)["edits"][0]["result"].update(reset={"kind":"number", "value":999}),
    "missing handler": lambda m, c: c["property_kinds"].update(number="missing-property-handler.swift"),
}
for platform in ["ios", "mac"]:
    model = inventory["platforms"][platform]
    audit(coverage, platform, model)
    for name, mutate in probes.items():
        changed, review = deepcopy(model), deepcopy(coverage)
        mutate(changed, review)
        try:
            audit(review, platform, changed)
        except ValueError:
            pass
        else:
            raise AssertionError(f"{platform} accepted invalid evidence: {name}")
    print(f"PASS: {platform} rejects all {len(probes)} property-evidence corruption probes")
