// Test launcher discovery without building Wasm or starting a server.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const launcher = fileURLToPath(new URL("run.sh", import.meta.url));
for (const [name, installed, customHome, override, expected] of [
  ["Cargo default directory outside PATH", ["home"], false, null, "home"],
  ["custom CARGO_HOME outside PATH", ["cargo"], true, null, "cargo"],
  ["PATH takes precedence", ["path", "home"], false, null, "path"],
  [
    "explicit executable takes precedence",
    ["path", "tool"],
    false,
    "tool",
    "tool",
  ],
  ["explicit command name", ["path"], false, "wasm-bindgen", "path"],
  ["missing tool", [], false, null, null],
  [
    "invalid override does not silently fall back",
    ["path"],
    false,
    "missing",
    null,
  ],
]) {
  test(name, (t) => {
    const root = mkdtempSync(join(tmpdir(), "layer-launcher-"));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const bin = join(root, "bin");
    const home = join(root, "user home");
    const cargo = join(root, "custom cargo");
    const locations = {
      path: join(bin, "wasm-bindgen"),
      home: join(home, ".cargo/bin/wasm-bindgen"),
      cargo: join(cargo, "bin/wasm-bindgen"),
      tool: join(root, "explicit tool/wasm-bindgen"),
    };
    function executable(path, message) {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, `#!/bin/sh\nprintf '%s\\n' '${message}'\n`, {
        mode: 0o755,
      });
    }
    executable(join(bin, "cargo"), "build");
    executable(join(bin, "python3"), "serve");
    symlinkSync("/usr/bin/dirname", join(bin, "dirname"));
    for (const tool of installed) executable(locations[tool], tool);
    const env = { HOME: home, PATH: bin };
    if (customHome) env.CARGO_HOME = cargo;
    if (override) env.LAYER_WASM_BINDGEN = locations[override] || override;
    const result = spawnSync("/bin/bash", [launcher], {
      env,
      encoding: "utf8",
    });
    assert.ifError(result.error);
    assert.equal(result.status, expected ? 0 : 1, result.stderr);
    if (expected) {
      assert.equal(result.stdout, `build\n${expected}\nserve\n`);
      assert.equal(result.stderr, "");
    } else {
      assert.equal(result.stdout, "", "must stop before building or serving");
      assert.match(
        result.stderr,
        override ? /LAYER_WASM_BINDGEN/ : /cargo install wasm-bindgen-cli/,
      );
    }
  });
}
