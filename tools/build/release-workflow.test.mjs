import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const workflow = name => readFileSync(root + `.github/workflows/${name}.yml`, "utf8").replace(/\r\n/g, "\n");
const release = workflow("release");
const checks = workflow("release-checks");
const ci = workflow("ci");
const job = (source, name) => source.match(new RegExp(`^  ${name}:\\n([\\s\\S]*?)(?=^  [\\w-]+:|$(?![\\s\\S]))`, "m"))?.[1];
const condition = (expression, event, ref) => Function("github", `return (${expression.replace(/^\$\{\{\s*|\s*\}\}$/g, "")});`)({ event_name: event, ref_type: ref });
const signed = release.match(/^  SIGNED: (.+)$/m)?.[1];
const publicRelease = release.match(/^  CAPY_PUBLIC_RELEASE: (.+)$/m)?.[1];
const platforms = ["web", "flatpak", "android", "windows", "apple"];
const publishers = ["android-publish", "apple-publish", "release"];

test("only a tag push enables signing, publication and the public About version", () => {
  for (const event of ["workflow_dispatch", "push", "pull_request"])
    for (const ref of ["branch", "tag"]) {
      const expected = event === "push" && ref === "tag";
      assert.equal(condition(signed, event, ref), expected, `${event}/${ref}: signing`);
      assert.equal(condition(publicRelease, event, ref), expected, `${event}/${ref}: public About version`);
      for (const name of publishers)
        assert.equal(condition(job(release, name).match(/^    if: (.+)$/m)[1], event, ref), expected, `${event}/${ref}: ${name}`);
    }
});

test("stores and the GitHub Release wait for every package, not the test suites", () => {
  for (const name of platforms) assert.match(job(release, name), /^    needs: version$/m);
  for (const name of ["android-publish", "apple-publish"]) {
    const needs = job(release, name).match(/^    needs: \[(.+)\]$/m)[1].split(", ");
    for (const prerequisite of platforms) assert.ok(needs.includes(prerequisite), `${name}: ${prerequisite}`);
    assert.ok(!needs.includes("checks"), `${name}: checks`);
  }
  assert.match(job(release, "release"), /^    needs: \[version, summary\]$/m);
});

test("a release tag may sit on main or on its release branch", () => {
  const gate = job(release, "version");
  assert.match(gate, /branch="v\$\{VERSION%\.\*\}"/);
  assert.match(gate, /for base in main "\$branch"; do/);
  assert.match(gate, /compare\/\$base\.\.\.\$GITHUB_SHA/);
});

test("a patch release that leaves saved-data code alone reuses the series' fixtures", () => {
  const step = job(release, "version").split("      - name: Require saved-data fixtures\n")[1];
  const script = step.split("        run: |\n")[1].split("\n").filter(line => line.startsWith("          ") || !line.trim()).map(line => line.slice(10)).join("\n");
  const repo = mkdtempSync(join(tmpdir(), "capy-fixtures-"));
  const git = (...args) => assert.equal(spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", ...args], { cwd: repo }).status, 0, args.join(" "));
  const fixtures = version => { for (const crate of ["layer-workspace", "layer-core", "layer-ui", "layer-host"]) for (const dir of [version, `expected/${version}`]) { mkdirSync(join(repo, `crates/${crate}/tests/compat/${dir}`), { recursive: true }); writeFileSync(join(repo, `crates/${crate}/tests/compat/${dir}/data`), version); } };
  const change = (path, text) => { mkdirSync(dirname(join(repo, path)), { recursive: true }); writeFileSync(join(repo, path), text); git("add", "-A"); git("commit", "-qm", path); };
  const run = version => spawnSync("bash", ["-c", script], { cwd: repo, env: { ...process.env, VERSION: version }, encoding: "utf8" }).status;
  try {
    git("init", "-q");
    fixtures("1.1.0"); change("crates/layer-render-wgpu/src/lib.rs", "a"); git("tag", "v1.1.0");
    change("crates/layer-render-wgpu/src/lib.rs", "b");
    assert.equal(run("1.1.1"), 0, "renderer-only patch");
    git("tag", "v1.1.1");
    assert.equal(run("1.1.2"), 0, "second renderer-only patch");
    change("crates/layer-ui/src/settings.rs", "c");
    assert.equal(run("1.1.2"), 1, "patch that changes a saved-data crate");
    fixtures("1.1.2"); git("add", "-A"); git("commit", "-qm", "fixtures");
    assert.equal(run("1.1.2"), 0, "patch with its own fixtures");
    assert.equal(run("1.2.0"), 1, "minor release without fixtures");
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("private archives and reports never enter GitHub Release assets", () => {
  assert.match(job(release, "release"), /pattern: '\*-package'/);
  assert.match(job(release, "apple"), /platform: \[ipad, mac, mac-store\]/);
  assert.match(job(release, "apple"), /name: \$\{\{ matrix.platform \}\}-archive/);
  assert.match(job(release, "summary"), /pattern: '\*-archive'/);
  assert.match(job(release, "summary"), /name: release-report/);
  assert.doesNotMatch(job(release, "release"), /name: (ipad-archive|mac-store-archive|release-report)/);
});

test("the test suites run on their own, outside the release workflow", () => {
  assert.match(checks, /^on:\n  workflow_call:/m);
  assert.doesNotMatch(checks, /^  (push|pull_request|schedule):/m);
  assert.doesNotMatch(ci, /^  arch:/m);
  assert.doesNotMatch(ci, /^  windows:/m);
  assert.doesNotMatch(release, /uses: \.\/.github\/workflows\/release-checks.yml/);
  for (const name of platforms) assert.doesNotMatch(job(release, name), /\b(?:lint\w*|assembleDebug\w*|cargo (?:test|nextest|clippy)|xcodebuild test|npm test|node --test)\b/, name);
  assert.doesNotMatch(ci, /uses: \.\/.github\/workflows\/release-checks.yml/);
  assert.match(ci, /group: ci-\$\{\{ github.workflow \}\}-\$\{\{ github.event_name \}\}-/);
});

test("logging cannot hide a failed validation command", () => {
  assert.match(checks, /^defaults:\n  run:\n    shell: bash$/m);
  assert.match(job(checks, "windows"), /    defaults:\n      run:\n        shell: pwsh/);
  const native = job(checks, "windows");
  assert.match(native, /\.\/apps\/layer-windows\/tests\/native-test-process\.ps1/);
  assert.match(native, /\$exitCode = Invoke-NativeTestProcess \$launch\n +if \(\$exitCode -ne 0\) \{ throw/);
});

test("hosted Windows jobs reserve architecture-specific build time", () => {
  const native = job(checks, "windows");
  assert.match(native, /- architecture: x64\n +target: x86_64-pc-windows-msvc\n +runner: windows-2025\n +timeout_minutes: 120\b/);
  assert.match(native, /- architecture: arm64\n +target: aarch64-pc-windows-msvc\n +runner: windows-11-arm\n +timeout_minutes: 180\b/);
  assert.match(native, /timeout-minutes: \$\{\{ matrix\.timeout_minutes \}\}/);
  assert.match(native, /name: Windows script regressions\n +timeout-minutes: 5\b/);
});

test("hosted CPU checks keep model tests while excluding native GPU cases", () => {
  const renderer = job(checks, "renderer");
  const selected = (crate, name) => {
    const args = renderer.split(`cargo test --locked --release -p ${crate} --lib --`)[1].split("2>&1")[0].match(/[^\s\\]+/g);
    const skips = args.flatMap((arg, i) => arg === "--skip" ? [args[i + 1]] : []);
    const filters = args.filter((arg, i) => !arg.startsWith("--") && args[i - 1] !== "--skip");
    return filters.some(filter => name.includes(filter)) && !skips.some(skip => name.includes(skip));
  };
  const models = "layer-render-wgpu";
  assert.ok(selected(models, "attached::renderer_replacement_keeps_native_stack_usage_bounded"));
  assert.ok(selected(models, "paint_transform::snapshot::tests::transforms_the_shared_page_check_admits_always_plan"));
  assert.ok(!selected(models, "attached::attached_renderer_forwards_fill_preview_and_reveal"));
  assert.ok(!selected(models, "paint_transform::snapshot::tests::captured_mask_aliases_reject_other_storage_and_overwritten_history"));
  const bridge = "layer-apple";
  assert.ok(selected(bridge, "tests::toolbar_component::apple_phone_queries_round_trip_without_a_session"));
  assert.ok(!selected(bridge, "tests::toolbar_component::apple_stamp_bytes_match_the_canonical_mask_and_known_receipts_omit_pixels"));
  const gtk = job(checks, "arch").match(/^ +cargo test --locked -p layer-linux --lib --test command_line -- (.+)$/m)?.[1];
  assert.equal(gtk, "--skip render_thread::tests::gpu_worker_first_preview_stroke");
});

test("the GitHub Release waits for every package and never for store uploads", () => {
  const gate = job(release, "summary").split("      - name: Require successful packages")[1]
    .match(/python3 - <<'PYCODE'\n([\s\S]*?)\n          PYCODE/)[1].replace(/^          /gm, "");
  const results = Object.fromEntries(["version", ...platforms, "android-publish", "apple-publish"].map(name => [name, { result: "success" }]));
  const run = (signed, windowsOnly = false) => spawnSync(process.env.PYTHON ?? "python3", ["-c", gate], { env: { ...process.env, SIGNED: String(signed), WINDOWS_PACKAGES_ONLY: String(windowsOnly), RESULTS: JSON.stringify(results) }, encoding: "utf8" });
  assert.equal(run(true).status, 0);
  for (const result of ["failure", "cancelled", "skipped"]) {
    results["android-publish"].result = results["apple-publish"].result = result;
    assert.equal(run(true).status, 0, `store uploads ${result}`);
    assert.equal(run(false).status, 0, `dry run with store uploads ${result}`);
  }
  for (const result of ["failure", "cancelled", "skipped"]) {
    results.android.result = result;
    assert.equal(run(false).status, 1, result);
    results.android.result = "success";
  }
  for (const name of platforms.filter(name => name !== "windows")) results[name].result = "skipped";
  assert.equal(run(false, true).status, 0);
  assert.equal(run(false).status, 1);
  for (const name of ["version", "windows"])
    for (const result of ["failure", "cancelled", "skipped"]) {
      results[name].result = result;
      assert.equal(run(false, true).status, 1, `${name}: ${result}`);
      results[name].result = "success";
    }
});

test("Apple Store jobs reuse both retained archives without touching DMG packaging", () => {
  const publish = job(release, "apple-publish");
  assert.match(publish, /platform: \[ipad, mac-store\]/);
  assert.match(publish, /name: \$\{\{ matrix.platform \}\}-archive/);
  assert.match(publish, /release\.sh \$\{\{ matrix.platform \}\} publish/);
  assert.doesNotMatch(publish, /release\.sh.* build|DEVELOPER_ID|notarytool/);
  assert.match(job(release, "apple"), /if: matrix.platform != 'mac'\n        run: ditto/);
  assert.match(job(release, "apple"), /if: matrix.platform == 'mac'\n        uses: actions\/upload-artifact/);
});
