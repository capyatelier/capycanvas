// Verify the development entry points and explicit distribution profile override.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("../../", import.meta.url));
for (const [platform, profile, explicit, expected] of [
  ["linux", "", "", "dev-perf"],
  ["linux", "release", "", "release"],
  ["web", "", "", "dev-perf"],
  ["web", "release", "", "release"],
  ["web", "dev-perf", "web-release", "web-release"],
  ["web", "dev", "", "dev"],
]) {
  test(`${platform}: env=${profile || "unset"}, explicit=${explicit || "unset"}`, (t) => {
    const bin = mkdtempSync(join(tmpdir(), "capy-profile-"));
    t.after(() => rmSync(bin, { recursive: true, force: true }));
    const log = join(bin, "commands.jsonl");
    for (const tool of ["cargo", "wasm-bindgen", "python3", "mkdir", "cp"]) {
      writeFileSync(join(bin, tool), `#!${process.execPath}
require("node:fs").appendFileSync(process.env.COMMAND_LOG,
  JSON.stringify([${JSON.stringify(tool)}, ...process.argv.slice(2)]) + "\\n");
`, { mode: 0o755 });
    }
    const script = `apps/layer-${platform}/${platform === "linux" ? "run" : "build"}.sh`;
    const args = platform === "linux" ? ["a file.capyc"] : explicit ? ["custom pkg", explicit] : [];
    const result = spawnSync("/bin/bash", [join(root, script), ...args], {
      cwd: bin,
      env: { ...process.env, PATH: `${bin}:/usr/bin:/bin`, CAPY_RUST_PROFILE: profile,
        LAYER_WASM_BINDGEN: join(bin, "wasm-bindgen"), CARGO_TARGET_DIR: "custom target",
        COMMAND_LOG: log },
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const commands = readFileSync(log, "utf8").trim().split("\n").map(JSON.parse);
    assert.deepEqual(commands[0], platform === "linux"
      ? ["cargo", "run", "--locked", "--profile", expected, "-p", "layer-linux", "--", ...args]
      : ["cargo", "build", "--locked", "--profile", expected, "-p", "layer-web", "--target", "wasm32-unknown-unknown"]);
    if (platform === "web") {
      assert.deepEqual(commands[1], ["wasm-bindgen", "--target", "web", "--out-dir",
        explicit ? "custom pkg" : "apps/layer-web/pkg",
        `custom target/wasm32-unknown-unknown/${expected === "dev" ? "debug" : expected}/layer_web.wasm`]);
    }
  });
}
