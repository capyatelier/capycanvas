import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  copyFileSync,
  readFileSync,
  readdirSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { runJourney } from "./journeys.mjs";

test("browser journeys require an error check", async () => {
  await assert.rejects(runJourney([[true, async () => {}]]), TypeError);
});

test("browser journeys await their error check", async () => {
  const failure = new Error("browser error");
  await assert.rejects(runJourney([[true, async () => {}, async () => { throw failure; }]]), error => error === failure);
});

const shell = process.env.LAYER_TEST_BASH || "/bin/bash";
const shellPath = path => process.platform === "win32" ? path.replaceAll("\\", "/").replace(/^([A-Za-z]):/, (_, drive) => "/" + drive.toLowerCase()) : path;
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
    const client = join(root,"apps/layer-web");
    mkdirSync(client,{recursive:true});
    for(const file of ['run.sh','build.sh'])copyFileSync(new URL(file,import.meta.url),join(client,file));
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
      writeFileSync(path, `#!/bin/sh\n${message ? `printf '%s\\n' '${message}'` : ':'}\n`, {
        mode: 0o755,
      });
    }
    executable(join(bin, "cargo"), "build");
    writeFileSync(join(bin, "python3"), '#!/bin/sh\nif [ "$1" = "-m" ]; then printf "serve\\n"; fi\n', { mode: 0o755 });
    writeFileSync(join(bin, "dirname"), '#!/bin/sh\nexec /usr/bin/dirname "$@"\n', { mode: 0o755 });
    writeFileSync(join(bin, "bash"), '#!/bin/sh\nexec /bin/bash "$@"\n', { mode: 0o755 });
    for(const name of ['mktemp','mv','rm','chmod'])writeFileSync(join(bin,name),`#!/bin/sh
exec /usr/bin/${name} "$@"
`,{mode:0o755});
    for (const tool of installed) executable(locations[tool], tool);
    const env = { HOME: shellPath(home), PATH: shellPath(bin) };
    if (customHome) env.CARGO_HOME = shellPath(cargo);
    if (override) env.LAYER_WASM_BINDGEN = locations[override] ? shellPath(locations[override]) : override;
    const result = spawnSync(shell, [shellPath(join(client,"run.sh"))], {
      env,
      encoding: "utf8",
    });
    assert.ifError(result.error);
    assert.equal(result.status, expected ? 0 : 1, result.stderr);
    if (expected) {
      assert.equal(result.stdout, `build\n${expected}\nserve\n`);
      assert.equal(result.stderr, "");
      assert.equal(readFileSync(join(client,'launch.css'),'utf8'),'build\n');
      writeFileSync(join(bin,'cargo'),'#!/bin/sh\nif [ "$1" = run ]; then printf partial; exit 7; fi\nprintf "build\\n"\n',{mode:0o755});
      const failed=spawnSync(shell,[shellPath(join(client,'run.sh'))],{env,encoding:'utf8'});
      assert.equal(failed.status,7,failed.stderr);
      assert.equal(readFileSync(join(client,'launch.css'),'utf8'),'build\n','A failed generator preserves the last published launch styles');
      assert.deepEqual(readdirSync(client).filter(name=>name.startsWith('launch.css.')),[],'The failing build removes only its own unpublished stylesheet');
    } else {
      assert.equal(result.stdout, "", "must stop before building or serving");
      assert.match(
        result.stderr,
        override ? /LAYER_WASM_BINDGEN/ : /cargo install wasm-bindgen-cli/,
      );
    }
  });
}
