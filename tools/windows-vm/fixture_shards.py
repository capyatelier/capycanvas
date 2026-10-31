"""Balanced fixture assignments and complete merged result coverage."""

import math
from statistics import median


def balance(selectors, runners, durations):
    if type(runners) is not int or runners < 1:
        raise ValueError("Fixture runner count must be positive")
    if not selectors or len(selectors) != len(set(selectors)):
        raise ValueError("Fixture selectors must be nonempty and distinct")
    known = {name: value for name, value in durations.items()
             if type(value) in (int, float) and math.isfinite(value) and value > 0}
    fallback = median(known.values()) if known else 1
    buckets = [[] for _ in range(min(runners, len(selectors)))]
    loads = [0] * len(buckets)
    order = {name: index for index, name in enumerate(selectors)}
    for name in sorted(selectors, key=lambda name: -known.get(name, fallback)):
        index = min(range(len(buckets)), key=lambda index: (loads[index], index))
        buckets[index].append(name)
        loads[index] += known.get(name, fallback)
    return [sorted(bucket, key=order.__getitem__) for bucket in buckets]


def merge(expected, shards):
    if not expected or len(expected) != len(set(expected)):
        raise ValueError("Merged fixture plan must be nonempty and distinct")
    found = {}
    errors = []
    assigned = []
    for shard in shards:
        name = shard["vm"]
        selection = shard["selectors"]
        assigned.extend(selection)
        rows = shard.get("results", [])
        if shard.get("failure"):
            errors.append(f"{name}: {shard['failure']}")
        if not shard.get("complete"):
            errors.append(f"{name}: missing shard completion")
        actual = [row.get("name") for row in rows]
        if actual != selection:
            errors.append(f"{name}: expected {selection}, received {actual}")
        for row in rows:
            selector = row.get("name")
            if selector not in selection or selector in found:
                errors.append(f"{name}: unexpected or duplicate selector {selector}")
                continue
            found[selector] = row
    if sorted(assigned) != sorted(expected):
        errors.append("Shard assignments do not cover the fixture plan exactly once")
    merged = []
    for selector in expected:
        row = found.get(selector)
        if row is None:
            row = {"name": selector, "exit": "missing", "seconds": 0,
                   "message": "The assigned shard did not return this selector"}
        if row.get("exit") != 0:
            errors.append(f"{selector}: {row.get('message') or row.get('exit', 'missing exit')}")
        merged.append(row)
    return merged, errors
