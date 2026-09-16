import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readFileSync, readdirSync, symlinkSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, extname, join, posix } from "node:path";
import { runInNewContext } from "node:vm";
import test from "node:test";
import { checkRuntime, dependencyNotices, filesIn, fingerprintAssets, writeWorker } from "./package.mjs";
import { gpuEnvironment, gpuProblem } from "./gpu.js";

test("filter icons declare theme paint without GTK's symbolic CSS", () => {
  for (const name of ["exposure", "vibrance", "black_white", "gradient_map", "posterize"]) {
    const svg = readFileSync(new URL(`./icons/layer-${name}-symbolic.svg`, import.meta.url), "utf8");
    // The shared icon audit replaced several outlines with filled silhouettes.
    // Either geometry is valid; browsers must not depend on GTK's CSS to paint it.
    assert.match(svg, /<svg\b[^>]*fill="(?:none|currentColor)"/);
    assert.match(svg, /(?:fill|stroke)="currentColor"/);
  }
});

test("GPU help distinguishes missing support, insecure access and no adapter", () => {
  assert.match(gpuProblem({ secure: false, api: false })[1], /secure connection/);
  assert.match(gpuProblem({ secure: true, api: false })[1], /WebGPU is not available/);
  assert.deepEqual(gpuProblem({ secure: true, api: true, stage: "adapter" }), [
    "Could not initialize canvas", "Your browser could not find a GPU adapter.",
  ]);
  assert.match(gpuProblem({ secure: true, api: true, stage: "device" })[1], /found a GPU but could not start/);
  for (const stage of ["renderer", undefined])
    assert.match(gpuProblem({ secure: true, api: true, stage })[1], /canvas renderer/);
});

test("GPU help identifies platforms without using browser identity to decide support", () => {
  for (const [userAgent, system, browser] of [
    ["Windows NT 10.0 Chrome/150 Safari/537.36", "windows", "chromium"],
    ["Windows NT 10.0 Chrome/150 Safari/537.36 Edg/150", "windows", "edge"],
    ["Windows NT 10.0 Firefox/150", "windows", "firefox"],
    ["Macintosh Chrome/150 Safari/537.36", "mac", "chromium"],
    ["Macintosh Version/26.0 Safari/605.1.15", "mac", "safari"],
    ["Macintosh Firefox/150", "mac", "firefox"],
    ["X11; Linux x86_64 Chrome/150", "linux", "chromium"],
    ["X11; Linux x86_64 Firefox/150", "linux", "firefox"],
    ["X11; CrOS x86_64 Chrome/150", "chromeos", "chromium"],
    ["Linux; Android 12 Chrome/150 Mobile Safari/537.36", "android", "chromium"],
    ["Android 14 Firefox/150", "android", "firefox"],
    ["iPhone; CPU iPhone OS 26_0 Version/26 Mobile Safari/605.1", "ios", "webkit"],
    ["iPad; CPU OS 26_0 Version/26 Mobile Safari/605.1", "ios", "webkit"],
    ["Macintosh CriOS/150 Version/26 Safari/605.1", "ios", "webkit"],
    ["iPhone FxiOS/150 Mobile Safari/605.1", "ios", "webkit"],
    ["iPhone EdgiOS/150 Mobile Safari/605.1", "ios", "webkit"],
    ["Unknown browser", "other", "other"],
  ]) assert.deepEqual(gpuEnvironment({ userAgent }), { system, browser }, userAgent);
  assert.deepEqual(gpuEnvironment({ userAgent: "Macintosh Version/26 Safari/605.1", platform: "MacIntel", maxTouchPoints: 5 }),
    { system: "ios", browser: "webkit" }, "Desktop-mode iPad is not a Mac");
  assert.deepEqual(gpuEnvironment({ userAgent: "X11; Linux x86_64 Chrome/150", userAgentData: { platform: "Android" } }),
    { system: "android", browser: "chromium" }, "Android desktop mode respects client hints");
});

test("the source page opts out of Dark Reader before loading app styles", () => {
  const html = readFileSync(new URL("index.html", import.meta.url), "utf8");
  assert.match(html, /<meta name="darkreader-lock"/);
  assert.ok(html.indexOf('name="darkreader-lock"') < html.indexOf('rel="stylesheet"'));
  assert.match(html, /name="color-scheme" content="dark light"/);
  assert.match(html, /name="theme-color" content="#333333"/);
});

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), "capy-package-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "index.html"), "drawing");
  writeFileSync(join(dir, "app.js"), "export const app = true;");
  return dir;
}

function runtimeFixture(t, changes = {}) {
  const dir = mkdtempSync(join(tmpdir(), "capy-assets-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  for (const [path, data] of Object.entries({
    "raster-worker.js": 'import init from "./pkg/layer_web.js";',
    "raster-worker-client.js": 'new Worker(new URL("./raster-worker.js", import.meta.url));',
    "workspace-store.js": "export const store = {};",
    "workspace-switcher.js": "export const switcher = {};",
    "workspace-manager.js": 'import {switcher} from "./workspace-switcher.js"; export const manager = {};',
    "workspace-worker.js": 'import init from "./pkg/layer_web.js"; import {store} from "./workspace-store.js";',
    "app.js": 'import {createRasterWorker} from "./raster-worker-client.js"; import {store} from "./workspace-store.js"; import {manager} from "./workspace-manager.js"; import init from "./pkg/layer_web.js";\nimport {createSystemStatus} from "./system-status.js";\nimport {createHeader} from "./header.js";\nimport {createEditorPanels} from "./editor-panels.js";\nimport {createWorkspaceChrome} from "./workspace-chrome.js";\nimport {createDocuments} from "./documents.js";\nimport {createPreferences} from "./preferences.js";\nimport {showGpuNotice} from "./gpu.js";\nimport {createCustomization} from "./customization.js";\nimport {createNumberField} from "./numeric.js";\nimport {createLayerPanel} from "./layers.js";\nimport {createEffectPanels} from "./effects.js";\nimport {installTooltips} from "./tooltips.js";\nconst assetPaths = {};',
    "system-status.js": "export const status = true;",
    "header.js": "import {switcher} from './workspace-switcher.js'; export const header = true;",
    "editor-panels.js": "import {chooseColor} from './color-controls.js'; export function createEditorPanels() {}",
    "workspace-chrome.js": "export function createWorkspaceChrome() {}",
    "documents.js": "import {chooseDocumentColor} from './document-color.js'; import {createHistogram} from './histogram.js'; import {chooseExport} from './export-controls.js'; export function createDocuments() {}",
    "document-color.js": "export function chooseDocumentColor() {}",
    "histogram.js": "export function createHistogram() {}",
    "export-controls.js": "export function chooseExport() {}",
    "numeric.js": "export function createNumberField() {}",
    "layers.js": "export function createLayerPanel() {}",
    "color-controls.js": "export function colorButton() {}",
    "effects.js": "import {colorButton} from './color-controls.js'; export function createEffectPanels() {}",
    "tooltips.js": "export function installTooltips() {}",
    "preferences.js": "export function createPreferences() {}",
    "gpu.js": "export function showGpuNotice() {}",
    "customization.js": "import {colorButton} from './color-controls.js'; export function createCustomization() {}",
    "pkg/layer_web.js": "export default new URL('layer_web_bg.wasm', import.meta.url);",
    "pkg/layer_web_bg.wasm": Buffer.from([0, 97, 115, 109]),
    "style.css": 'body { color: black; mask: url("icons/pen.svg"); }',
    "icons/pen.svg": "<svg/>",
    "brush-previews/1-dark.png": Buffer.from([137, 80, 78, 71]),
    "filters/manifest.json": '{"format":1,"filters":[]}',
    "filters/example.wgsl": "fn example() {}",
    ...changes,
  })) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), data);
  }
  return dir;
}

test("every runtime filename hashes its final bytes and all dependency references follow it", (t) => {
  const dir = runtimeFixture(t), names = fingerprintAssets(dir);
  assert.deepEqual(filesIn(dir).sort(), Object.values(names).sort(), "No unversioned runtime copies remain");
  for (const [original, name] of Object.entries(names)) {
    const hash = createHash("sha256").update(readFileSync(join(dir, name))).digest("hex").slice(0, 20);
    const extension = extname(original);
    assert.equal(name, `${original.slice(0, -extension.length)}.${hash}${extension}`);
  }
  const app = readFileSync(join(dir, names["app.js"]), "utf8");
  for (const path of ["preferences.js", "gpu.js", "customization.js", "numeric.js", "layers.js", "pkg/layer_web.js"])
    assert.ok(app.includes(`from "./${names[path]}"`));
  for (const path of ["icons/pen.svg", "brush-previews/1-dark.png", "filters/manifest.json", "filters/example.wgsl"])
    assert.ok(app.includes(JSON.stringify(names[path])));
  for (const path of ["workspace-manager.js", "header.js"])
    assert.ok(readFileSync(join(dir, names[path]), "utf8").includes(`from "./${names["workspace-switcher.js"]}"`));
  assert.ok(readFileSync(join(dir, names["style.css"]), "utf8").includes(`url("${names["icons/pen.svg"]}")`));
  assert.ok(readFileSync(join(dir, names["pkg/layer_web.js"]), "utf8").includes(names["pkg/layer_web_bg.wasm"].slice(4)));
  assert.deepEqual(fingerprintAssets(runtimeFixture(t)), names, "An identical rebuild keeps every URL stable");
});

test("production runtime module imports resolve to packaged files", (t) => {
  const modules = readdirSync(new URL("./", import.meta.url)).filter(path => path.endsWith(".js") && path !== "sw.js");
  const dir = runtimeFixture(t, Object.fromEntries(modules.map(path =>
    [path, readFileSync(new URL(path, import.meta.url), "utf8")])));
  const names = fingerprintAssets(dir), files = new Set(filesIn(dir));
  for (const name of Object.values(names).filter(path => path.endsWith(".js"))) {
    const source = readFileSync(join(dir, name), "utf8");
    for (const [, dependency] of source.matchAll(/\bfrom\s+["'](\.[^"']+)["']/g))
      assert.ok(files.has(posix.join(posix.dirname(name), dependency)), `${name} imports missing ${dependency}`);
  }
});

test("changed assets propagate to their consumers and worker version, not unrelated assets", (t) => {
  const source = runtimeFixture(t), original = runtimeFixture(t), names = fingerprintAssets(original), first = writeWorker(original);
  for (const path of ["app.js", "workspace-store.js", "workspace-switcher.js", "workspace-manager.js", "workspace-worker.js", "system-status.js","header.js", "style.css", "gpu.js", "numeric.js", "pkg/layer_web_bg.wasm", "icons/pen.svg", "brush-previews/1-dark.png", "filters/manifest.json", "filters/example.wgsl"]) {
    const dir = runtimeFixture(t, { [path]: Buffer.concat([readFileSync(join(source, path)), Buffer.from("\n/* changed */")]) });
    const next = fingerprintAssets(dir);
    assert.notEqual(next[path], names[path], path);
    assert.equal(next["preferences.js"], names["preferences.js"], "Unchanged dependencies retain their URL");
    assert.equal(next["style.css"] === names["style.css"], !["style.css", "icons/pen.svg"].includes(path));
    assert.equal(next["app.js"] === names["app.js"], path === "style.css", "Module/artwork changes invalidate their consumer");
    for (const consumer of ["workspace-manager.js", "header.js"])
      assert.equal(next[consumer] === names[consumer], ![consumer, "workspace-switcher.js"].includes(path), `${consumer} follows switcher changes`);
    assert.equal(next["pkg/layer_web.js"] === names["pkg/layer_web.js"], path !== "pkg/layer_web_bg.wasm");
    assert.notEqual(writeWorker(dir).version, first.version, "Every content change updates the PWA cache version");
  }
});

test("new modules or changed rewrite anchors fail packaging rather than shipping stale references", (t) => {
  assert.throws(() => fingerprintAssets(runtimeFixture(t, { "extra.js": "export const extra = true;" })), /new module/);
  assert.throws(() => fingerprintAssets(runtimeFixture(t, { "app.js": "changed module layout" })), /Missing package reference/);
  assert.throws(() => fingerprintAssets(runtimeFixture(t, { "style.css": 'body { mask: url("missing.svg"); }' })), /Missing CSS asset/);
});

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
