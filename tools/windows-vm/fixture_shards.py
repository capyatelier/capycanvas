"""Once-only shared fixture claims and complete merged result coverage."""

import json
import math
import secrets
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Lock, Thread
from statistics import median


class Queue:
    def __init__(self, selectors, runners, durations, plan):
        if not selectors or len(selectors) != len(set(selectors)):
            raise ValueError("Fixture selectors must be nonempty and distinct")
        if not runners or len(runners) != len(set(runners)):
            raise ValueError("Fixture runners must be nonempty and distinct")
        known = {name: value for name, value in durations.items()
                 if type(value) in (int, float) and math.isfinite(value) and value > 0}
        fallback = median(known.values()) if known else 1
        self.remaining = iter(sorted(selectors, key=lambda name: -known.get(name, fallback)))
        self.lock = Lock()
        self.selectors = selectors
        self.assignments = {runner: [] for runner in runners}
        self.tokens = {runner: secrets.token_urlsafe(32) for runner in runners}
        self.plan = plan
        self.save()
        queue = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                runner = self.path[1:]
                token = queue.tokens.get(runner)
                if token is None or not secrets.compare_digest(self.headers.get("Authorization", ""), "Bearer " + token):
                    self.send_error(403)
                    return
                with queue.lock:
                    selector = next(queue.remaining, None)
                    if selector is not None:
                        queue.assignments[runner].append(selector)
                        queue.save()
                body = json.dumps({"selector": selector}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = Thread(target=self.server.serve_forever, daemon=True)

    def save(self):
        temporary = self.plan.with_suffix(".tmp")
        temporary.write_text(json.dumps({"runs": self.selectors, "shards": self.assignments}, indent=2))
        temporary.replace(self.plan)

    def configuration(self, runner):
        return {"url": f"http://10.0.2.2:{self.server.server_port}/{runner}", "token": self.tokens[runner]}

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exception):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


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
