import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const workflow = name => readFileSync(root + `.github/workflows/${name}.yml`, "utf8").replace(/\r\n/g, "\n");
const release = workflow("release");
const checks = workflow("release-checks");
const ci = workflow("ci");
const job = (source, name) => source.match(new RegExp(`^  ${name}:\\n([\\s\\S]*?)(?=^  [\\w-]+:|$(?![\\s\\S]))`, "m"))?.[1];
const condition = (expression, event, ref) => Function("github", `return (${expression.replace(/^\$\{\{\s*|\s*\}\}$/g, "")});`)({ event_name: event, ref_type: ref });
const signed = release.match(/^  SIGNED: (.+)$/m)?.[1];
const platforms = ["web", "flatpak", "android", "windows", "apple"];
const publishers = ["android-publish", "ipad-publish", "release"];

test("only a tag push enables signing and publication", () => {
  for (const event of ["workflow_dispatch", "push", "pull_request"])
    for (const ref of ["branch", "tag"]) {
      const expected = event === "push" && ref === "tag";
      assert.equal(condition(signed, event, ref), expected, `${event}/${ref}: signing`);
      for (const name of publishers)
        assert.equal(condition(job(release, name).match(/^    if: (.+)$/m)[1], event, ref), expected, `${event}/${ref}: ${name}`);
    }
});

test("packages build alongside checks and stores wait for every result", () => {
  for (const name of platforms) assert.match(job(release, name), /^    needs: version$/m);
  for (const name of ["android-publish", "ipad-publish"]) {
    const needs = job(release, name).match(/^    needs: \[(.+)\]$/m)[1].split(", ");
    for (const prerequisite of ["checks", ...platforms]) assert.ok(needs.includes(prerequisite), `${name}: ${prerequisite}`);
  }
  assert.match(job(release, "release"), /^    needs: \[version, summary\]$/m);
});

test("private archives and reports never enter GitHub Release assets", () => {
  assert.match(job(release, "release"), /pattern: '\*-package'/);
  assert.match(job(release, "apple"), /name: ipad-archive/);
  assert.match(job(release, "summary"), /name: release-report/);
  assert.doesNotMatch(job(release, "release"), /name: (ipad-archive|release-report)/);
});

test("heavy validation is called only by the release workflow", () => {
  assert.match(checks, /^on:\n  workflow_call:/m);
  assert.doesNotMatch(checks, /^  (push|pull_request|schedule):/m);
  assert.doesNotMatch(ci, /^  arch:/m);
  assert.doesNotMatch(ci, /^  windows:/m);
  assert.match(job(release, "checks"), /uses: \.\/.github\/workflows\/release-checks.yml/);
  assert.doesNotMatch(ci, /uses: \.\/.github\/workflows\/release-checks.yml/);
  assert.match(ci, /group: ci-\$\{\{ github.workflow \}\}-\$\{\{ github.event_name \}\}-/);
});

test("logging cannot hide a failed validation command", () => {
  assert.match(checks, /^defaults:\n  run:\n    shell: bash$/m);
  assert.match(job(checks, "windows"), /    defaults:\n      run:\n        shell: pwsh/);
});

test("release success permits only the jobs skipped by the selected mode", () => {
  const gate = job(release, "summary").split("      - name: Require successful checks and packages")[1]
    .match(/python3 - <<'PYCODE'\n([\s\S]*?)\n          PYCODE/)[1].replace(/^          /gm, "");
  const results = Object.fromEntries(["version", "checks", ...platforms, "android-publish", "ipad-publish"].map(name => [name, { result: "success" }]));
  const run = (signed, windowsOnly = false) => spawnSync(process.env.PYTHON ?? "python3", ["-c", gate], { env: { ...process.env, SIGNED: String(signed), WINDOWS_PACKAGES_ONLY: String(windowsOnly), RESULTS: JSON.stringify(results) }, encoding: "utf8" });
  assert.equal(run(true).status, 0);
  results["android-publish"].result = results["ipad-publish"].result = "skipped";
  assert.equal(run(false).status, 0);
  assert.equal(run(true).status, 1);
  for (const result of ["failure", "cancelled", "skipped"]) {
    results.checks.result = result;
    assert.equal(run(false).status, 1, result);
  }
  for (const name of ["checks", ...platforms.filter(name => name !== "windows")]) results[name].result = "skipped";
  assert.equal(run(false, true).status, 0);
  assert.equal(run(false).status, 1);
  for (const name of ["version", "windows"])
    for (const result of ["failure", "cancelled", "skipped"]) {
      results[name].result = result;
      assert.equal(run(false, true).status, 1, `${name}: ${result}`);
      results[name].result = "success";
    }
});
