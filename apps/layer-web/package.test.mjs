import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync, readFileSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import test from "node:test";
import { checkRuntime, dependencyNotices, filesIn, writeWorker } from "./package.mjs";
import { gpuProblem } from "./gpu.js";

test("GPU help uses short, plain messages without exposing technical errors", () => {
  assert.match(gpuProblem({ secure: false, api: false })[0], /secure link/);
  assert.match(gpuProblem({ secure: true, api: false })[1], /Update your browser/);
  assert.equal(gpuProblem({ secure: true, api: true })[0], "Drawing isn’t available");
  for (const api of [true, false]) {
    const text = gpuProblem({ secure: true, api }).join(" ");
    assert.ok(text.split(/\s+/).length < 25);
    assert.doesNotMatch(text, /GPU|adapter|API|driver|renderer/);
  }
});

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), "capy-package-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "index.html"), "drawing");
  writeFileSync(join(dir, "app.js"), "export const app = true;");
  return dir;
}

test("worker version covers every file and is stable across repeat builds", (t) => {
  const dir = fixture(t);
  const first = writeWorker(dir);
  assert.deepEqual(first.files.map((f) => f.path), ["app.js", "index.html"]);
  assert.equal(writeWorker(dir).version, first.version);
  const template = readFileSync(new URL("sw.js", import.meta.url), "utf8");
  assert.notEqual(writeWorker(dir, template + "\n// worker-only update").version, first.version);
  assert.ok(first.files.every((file) => /^sha256-/.test(file.integrity)));
  writeFileSync(join(dir, "app.js"), "different");
  assert.notEqual(writeWorker(dir).version, first.version);
  assert.ok(!readFileSync(join(dir, "sw.js"), "utf8").includes("__CAPY_"));
});

test("publication rejects symlinks and embedded private runtime paths", (t) => {
  const dir = fixture(t);
  symlinkSync(join(dir, "index.html"), join(dir, "link"));
  assert.throws(() => filesIn(dir), /Not a regular/);
  for (const text of ["/home/example/code/lib.rs", "/Users/example/src", "C:\\Users\\example\\src", "-----BEGIN PRIVATE KEY-----"])
    assert.throws(() => checkRuntime(Buffer.from(text), "fixture"), /Private path or key/);
  checkRuntime(Buffer.from("/cargo/registry/library.rs /capycanvas/crates/lib.rs"), "public");
});

test("dependency notices require original text and exclude private metadata", () => {
  const license = { name: "MIT", text: "Copyright <owner> & contributors", source_path: "/private/LICENSE",
    used_by: [{ crate: { name: "example", version: "1.0", source: "registry", manifest_path: "/private/Cargo.toml" } }] };
  const html = dependencyNotices([license]);
  assert.ok(html.includes("Copyright &lt;owner&gt; &amp; contributors"));
  assert.ok(!html.includes("/private"));
  assert.throws(() => dependencyNotices([{ ...license, source_path: null }]), /Missing original/);
  assert.throws(() => dependencyNotices([{ ...license, text: "Copyright <year> <copyright holders>" }]), /Missing original/);
  assert.equal(dependencyNotices([{ ...license, source_path: null, used_by: [{ crate: { source: null } }] }]), "");
});

test("worker stays in scope and deletes only its own obsolete caches", async (t) => {
  const dir = fixture(t), { version } = writeWorker(dir);
  const scope = "https://example.test/draw/", prefix = `capycanvas:${scope}:`;
  const handlers = {}, deleted = [], fetched = [], cached = [];
  let claimed = false;
  runInNewContext(readFileSync(join(dir, "sw.js"), "utf8"), {
    URL, Request, Response,
    self: { registration: { scope }, addEventListener: (name, fn) => { handlers[name] = fn; },
      clients: { claim: async () => { claimed = true; } } },
    caches: {
      keys: async () => [prefix + "old", prefix + version, "unrelated", "capycanvas:https://example.test/draw/nested/:old"],
      delete: async (key) => { deleted.push(key); },
      open: async () => ({ addAll: async (requests) => { cached.push(...requests); },
        match: async (url) => { fetched.push(url); return new Response("cached"); } }),
    },
  });
  let pending;
  handlers.install({ waitUntil: (promise) => { pending = promise; } });
  await pending;
  assert.ok(cached.every((request) => request.url.startsWith(scope) && request.cache === "reload" && request.integrity));
  handlers.activate({ waitUntil: (promise) => { pending = promise; } });
  await pending;
  assert.deepEqual(deleted, [prefix + "old"]);
  assert.ok(claimed);
  function request(url, method = "GET") {
    let response;
    handlers.fetch({ request: { url, method }, respondWith: (promise) => { response = promise; } });
    return response;
  }
  assert.equal(await (await request(scope + "?installed")).text(), "cached");
  assert.deepEqual(fetched, [scope + "index.html"]);
  assert.equal(request("https://example.test/other/app.js"), undefined);
  assert.equal(request(scope + "user-project.json"), undefined);
  assert.equal(request(scope + "app.js", "POST"), undefined);
});

test("failed precache installation removes only the incomplete new version", async (t) => {
  const dir = fixture(t), { version } = writeWorker(dir), handlers = {}, deleted = [];
  const scope = "https://example.test/";
  runInNewContext(readFileSync(join(dir, "sw.js"), "utf8"), {
    URL, Request, self: { registration: { scope }, addEventListener: (name, fn) => { handlers[name] = fn; } },
    caches: { open: async () => ({ addAll: async () => { throw new Error("incomplete deployment"); } }),
      delete: async (name) => { deleted.push(name); } },
  });
  let pending;
  handlers.install({ waitUntil: (promise) => { pending = promise; } });
  await assert.rejects(pending, /incomplete deployment/);
  assert.deepEqual(deleted, [`capycanvas:${scope}:${version}`]);
});
