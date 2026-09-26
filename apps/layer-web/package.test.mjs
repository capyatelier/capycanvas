import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readFileSync, readdirSync, symlinkSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, extname, join, posix } from "node:path";
import { runInNewContext } from "node:vm";
import { fileURLToPath } from "node:url";
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
    "proof-worker.js": 'import init from "./pkg/layer_web.js";',
    "proof.js": `import {importProfile} from './export-controls.js'; new Worker(new URL("./proof-worker.js",import.meta.url));`,
    "raster-worker.js": 'import init from "./pkg/layer_web.js";',
    "raster-worker-client.js": 'new Worker(new URL("./raster-worker.js", import.meta.url));',
    "drawing-tabs.js": "export const tabs = {};",
    "document-recovery.js": "export const recovery = {};",
    "document-storage.js": "export const storage = {};",
    "workspace-store.js": "export const store = {};",
    "workspace-preload.js": 'import {store} from "./workspace-store.js"; new URL("./workspace-worker.js", import.meta.url); new URL("./pkg/layer_web_bg.wasm", import.meta.url);',
    "workspace-switcher.js": "export const switcher = {};",
    "workspace-manager.js": 'import {switcher} from "./workspace-switcher.js"; export const manager = {};',
    "workspace-worker.js": 'import init from "./pkg/layer_web.js"; import {store} from "./workspace-store.js";',
    "app.js": 'import {storage} from "./document-storage.js"; import {createRasterWorker} from "./raster-worker-client.js"; import {store} from "./workspace-preload.js"; import {manager} from "./workspace-manager.js"; import init from "./pkg/layer_web.js";\nimport {createSystemStatus} from "./system-status.js";\nimport {createHeader} from "./header.js";\nimport {createSelectionUi} from "./selection-masks.js";\nimport {createEditorPanels} from "./editor-panels.js";\nimport {createWorkspaceChrome} from "./workspace-chrome.js";\nimport {createGlass} from "./glass.js";\nimport {createDocuments} from "./documents.js";\nimport {createPreferences} from "./preferences.js";\nimport {showGpuNotice} from "./gpu.js";\nimport {createCustomization} from "./customization.js";\nimport {createNumberField} from "./numeric.js";\nimport {createLayerPanel} from "./layers.js";\nimport {createEffectPanels} from "./effects.js";\nimport {installTooltips} from "./tooltips.js";\nimport {installPenScrolling} from "./pen-scroll.js";\nimport {createPalettes} from "./palettes.js";\nconst assetPaths = {};',
    "system-status.js": "export const status = true;",
    "header.js": "import {switcher} from './workspace-switcher.js'; import {pickerButtonAction} from './color-controls.js'; export const header = true;",
    "editor-panels.js": "import {createRasterWorker} from './raster-worker-client.js'; import {chooseColor} from './color-controls.js'; import {createRangeControl} from './range-control.js'; export function createEditorPanels() {}",
    "workspace-chrome.js": "export function createWorkspaceChrome() {}",
    "glass.js": "export function createGlass() {}",
    "image-import.js": "export function createImageImport() {}",
    "selection-masks.js": "export function createSelectionUi() {}",
    "documents.js": "import {tabs} from './drawing-tabs.js'; import {recovery} from './document-recovery.js'; import {createProof} from './proof.js'; import {createImageImport} from './image-import.js'; import {chooseDocumentColor} from './document-color.js'; import {createHistogram} from './histogram.js'; import {chooseExport} from './export-controls.js'; export function createDocuments() {}",
    "document-color.js": "import {importProfile} from './export-controls.js'; export function chooseDocumentColor() {}",
    "histogram.js": "export function createHistogram() {}",
    "export-controls.js": "export function chooseExport() {}",
    "numeric.js": "export function createNumberField() {}",
    "layers.js": "export function createLayerPanel() {}",
    "color-controls.js": "export function colorButton() {}",
    "filter-previews.js": "export function filterPreviewView() {}",
    "stroke-recording.js": "export function strokeRecordingControl() {}",
    "effects.js": "import {strokeRecordingControl} from './stroke-recording.js'; import {colorButton} from './color-controls.js'; import {filterPreviewView} from './filter-previews.js'; export function createEffectPanels() {}",
    "tooltips.js": "export function installTooltips() {}",
    "pen-scroll.js": "export function installPenScrolling() {}",
    "palettes.js": "export function createPalettes() {}",
    "preferences.js": "import {chooseProfileLibrary} from './export-controls.js'; export function createPreferences() {}",
    "gpu.js": "export function showGpuNotice() {}",
    "toolbar-components.js": "import {createNumberField} from './numeric.js'; import {createRangeControl} from './range-control.js';",
    "range-control.js": "import {createNumberField} from './numeric.js'; export function createRangeControl() {}",
    "customization.js": 'import {createToolbarComponent} from "./toolbar-components.js";' + "import {colorButton} from './color-controls.js'; export function createCustomization() {}",
    "pkg/layer_web.js": "export default new URL('layer_web_bg.wasm', import.meta.url);",
    "pkg/layer_web_bg.wasm": Buffer.from([0, 97, 115, 109]),
    "style.css": 'body { color: black; mask: url("icons/pen.svg"); }',
    "icons/pen.svg": "<svg/>",
    "icons.svg": '<svg><svg data-asset="pen"/></svg>',
    "brush-previews/1-dark.png": Buffer.from([137, 80, 78, 71]),
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
  for (const path of ["icons/pen.svg", "icons.svg", "brush-previews/1-dark.png"])
    assert.ok(app.includes(JSON.stringify(names[path])));
  for (const path of ["workspace-manager.js", "header.js"])
    assert.ok(readFileSync(join(dir, names[path]), "utf8").includes(`from "./${names["workspace-switcher.js"]}"`));
  assert.ok(readFileSync(join(dir, names["header.js"]), "utf8").includes(`from "./${names["color-controls.js"]}"`));
  assert.ok(readFileSync(join(dir, names["editor-panels.js"]), "utf8").includes(`from "./${names["raster-worker-client.js"]}"`));
  assert.ok(readFileSync(join(dir, names["style.css"]), "utf8").includes(`url("${names["icons/pen.svg"]}")`));
  assert.ok(readFileSync(join(dir, names["pkg/layer_web.js"]), "utf8").includes(names["pkg/layer_web_bg.wasm"].slice(4)));
  assert.deepEqual(fingerprintAssets(runtimeFixture(t)), names, "An identical rebuild keeps every URL stable");
});

test("production module imports and worker URLs resolve to packaged files", (t) => {
  const modules = readdirSync(new URL("./", import.meta.url)).filter(path => path.endsWith(".js") && path !== "sw.js");
  const dir = runtimeFixture(t, Object.fromEntries(modules.map(path =>
    [path, readFileSync(new URL(path, import.meta.url), "utf8")])));
  const names = fingerprintAssets(dir), files = new Set(filesIn(dir));
  for (const name of Object.values(names).filter(path => path.endsWith(".js"))) {
    const source = readFileSync(join(dir, name), "utf8");
    for (const [, dependency] of source.matchAll(/\bfrom\s+["'](\.[^"']+)["']/g))
      assert.ok(files.has(posix.join(posix.dirname(name), dependency)), `${name} imports missing ${dependency}`);
    for (const [, dependency] of source.matchAll(/new URL\(["']([^"']+)["'],\s*import\.meta\.url\)/g))
      assert.ok(files.has(posix.join(posix.dirname(name), dependency)), `${name} references missing ${dependency}`);
  }
});

test("changed assets propagate to their consumers and worker version, not unrelated assets", (t) => {
  const source = runtimeFixture(t), original = runtimeFixture(t), names = fingerprintAssets(original), first = writeWorker(original);
  for (const path of ["drawing-tabs.js", "document-recovery.js", "document-storage.js", "image-import.js", "app.js", "workspace-store.js", "workspace-switcher.js", "workspace-manager.js", "workspace-worker.js", "system-status.js","header.js", "style.css", "gpu.js", "numeric.js", "range-control.js", "pkg/layer_web_bg.wasm", "icons/pen.svg", "brush-previews/1-dark.png"]) {
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
  const vendored = { ...license, used_by: [{ crate: { name: "rust_h265", version: "0.1.0", source: null,
    manifest_path: fileURLToPath(new URL("../../vendor/rust_h265/Cargo.toml", import.meta.url)) } }] };
  assert.ok(dependencyNotices([vendored]).includes("rust_h265"));
  assert.throws(() => dependencyNotices([{ ...vendored, source_path: null }]), /Missing original/);
});

test("the pinned zune-core notice preserves its complete alternative and rejects version drift", () => {
  const crate = { name:"zune-core", version:"0.4.12", source:"registry", license:"MIT OR Apache-2.0 OR Zlib" };
  const notice = { name:"MIT", source_path:null, text:"Copyright <year> <copyright holders>", used_by:[{crate}] };
  const html = dependencyNotices([notice]);
  assert.ok(html.includes("Zlib License") && html.includes("zune-core 0.4.12") && html.includes("f8fbb123d5ed04441e8324a555bfcda0cb1bd28f"));
  assert.ok(html.includes("This notice may not be removed"));
  assert.throws(() => dependencyNotices([{...notice,used_by:[{crate:{...crate,version:"0.4.13"}}]}]), /Revalidate/);
});

// Exercise the worker's event contract with separate HTTP and Cache Storage
// responses. pwa.test.mjs complements these failure/race cases in a real browser.
function workerFixture(t) {
  const dir = fixture(t), {version} = writeWorker(dir);
  const scope = "https://example.test/draw/", prefix = `capycanvas:${scope}:`, current = prefix + version;
  const stores = new Map(), handlers = {}, deleted = [], requests = [], installed = [];
  const state = {clients: [], skipped: false, claimed: false, failInstall: false,
    network: async () => new Response("fresh HTML", {headers: {"Content-Type": "text/html"}})};
  const put = (key, path, body) => {
    if (!stores.has(key)) stores.set(key, new Map());
    stores.get(key).set(new URL(path, scope).href, body);
  };
  runInNewContext(readFileSync(join(dir, "sw.js"), "utf8"), {
    URL, Request, Response, AbortController, setTimeout, clearTimeout,
    fetch: async (request, options) => { requests.push({request, options}); return state.network(request, options); },
    self: {registration: {scope}, addEventListener: (name, fn) => {handlers[name] = fn;},
      skipWaiting: async () => {state.skipped = true;},
      clients: {claim: async () => {state.claimed = true;}, matchAll: async () => state.clients}},
    caches: {
      keys: async () => [...stores.keys()],
      delete: async key => {deleted.push(key); return stores.delete(key);},
      match: async (url, {cacheName}) => {
        const body = stores.get(cacheName)?.get(url);
        return body === undefined ? undefined : new Response(body);
      },
      open: async key => {
        if (!stores.has(key)) stores.set(key, new Map());
        return {addAll: async requests => {
          installed.push(...requests);
          if (state.failInstall) throw Error("incomplete deployment");
          for (const request of requests) put(key, request.url, "complete package");
        }};
      },
    },
  });
  const lifetime = async name => {
    let pending;
    handlers[name]({waitUntil: promise => {pending = promise;}});
    await pending;
  };
  async function request(path, options = {}) {
    let response; const pending = [];
    const request = {url: new URL(path, scope).href, method: "GET", mode: "cors", ...options};
    handlers.fetch({request, resultingClientId: "arriving", respondWith: promise => {response = promise;},
      waitUntil: promise => pending.push(promise)});
    await Promise.all(pending);
    return response;
  }
  return {scope, prefix, current, state, stores, put, request, lifetime, deleted, requests, installed};
}

test("complete installation activates automatically without deleting open tabs' assets", async t => {
  const w = workerFixture(t);
  w.put(w.prefix + "old", "index.html", "old");
  w.state.clients = [{id: "drawing", url: w.scope}];
  await w.lifetime("install"); await w.lifetime("activate");
  assert.ok(w.state.skipped && w.state.claimed);
  assert.deepEqual(w.deleted, []);
  assert.ok(w.installed.every(r => r.url.startsWith(w.scope) && r.cache === "reload" && r.integrity));
});

test("startup and refresh revalidate HTML without replacing the complete offline fallback", async t => {
  const w = workerFixture(t);
  w.put(w.current, "index.html", "complete offline HTML");
  for (const path of ["./", "index.html", "./?refresh=123"]) {
    assert.equal(await (await w.request(path, {mode: "navigate"})).text(), "fresh HTML");
  }
  assert.ok(w.requests.every(({options}) => options.cache === "no-cache"));
  assert.equal(w.stores.get(w.current).get(w.scope + "index.html"), "complete offline HTML");
  for (const network of [async () => {throw Error("offline");},
    async () => new Response("down", {status: 503}), async () => new Response("missing", {status: 404}),
    async () => new Response("not HTML", {headers: {"Content-Type": "text/plain"}})]) {
    w.state.network = network;
    assert.equal(await (await w.request("./", {mode: "navigate"})).text(), "complete offline HTML");
  }
  w.stores.delete(w.current);
  assert.equal((await w.request("./", {mode: "navigate"})).status, 503);
});

test("a stalled network navigation times out to the offline package", async t => {
  const w = workerFixture(t);
  w.put(w.current, "index.html", "offline");
  w.state.network = (_, {signal}) => new Promise((resolve, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), {once: true});
  });
  assert.equal(await (await w.request("./", {mode: "navigate"})).text(), "offline");
  assert.ok(w.requests[0].options.signal.aborted);
});

test("old and new fingerprinted resources coexist, including before a new worker installs", async t => {
  const w = workerFixture(t), old = "assets/old.11111111111111111111.js", fresh = "assets/new.22222222222222222222.js";
  w.put(w.prefix + "old", old, "old bytes");
  w.put(w.current, "index.html", "complete HTML");
  // A neighboring installation must never satisfy this worker's requests.
  w.put("capycanvas:" + w.scope + "nested/:other", fresh, "neighbor bytes");
  w.state.network = async () => new Response("new network bytes");
  assert.equal(await (await w.request(old)).text(), "old bytes");
  assert.equal(await (await w.request(fresh)).text(), "new network bytes");
  assert.equal(w.requests.length, 1);
  assert.equal(await w.request("user-project.json"), undefined);
  assert.equal(await w.request("assets/unversioned.js"), undefined);
  assert.equal(await w.request("https://example.test/other/assets/x.11111111111111111111.js"), undefined);
  assert.equal(await w.request(old, {method: "POST"}), undefined);
});

test("obsolete releases are collected only at a cold navigation, never a newer install or neighboring app", async t => {
  const w = workerFixture(t), old = w.prefix + "old", neighbor = "capycanvas:" + w.scope + "nested/:old", newer = w.prefix + "installing";
  for (const key of [old, neighbor, w.current, newer]) w.put(key, "index.html", key);
  w.state.clients = [{id: "old-page", url: w.scope}];
  await w.request("./", {mode: "navigate"});
  assert.deepEqual(w.deleted, []);
  w.state.clients = [{id: "arriving", url: w.scope}];
  await w.request("./", {mode: "navigate"});
  assert.deepEqual(w.deleted, [old]);
  assert.ok(w.stores.has(neighbor) && w.stores.has(w.current) && w.stores.has(newer));
});

test("failed installation neither activates nor deletes a previously usable cache", async t => {
  const w = workerFixture(t), old = w.prefix + "old";
  w.put(old, "index.html", "old");
  w.state.failInstall = true;
  await assert.rejects(w.lifetime("install"), /incomplete deployment/);
  assert.deepEqual(w.deleted, [w.current]);
  assert.ok(w.stores.has(old));
  assert.equal(w.state.skipped, false);
  // Reinstalling a retained release (rollback) must not destroy its old cache.
  w.put(w.current, "index.html", "retained rollback");
  await assert.rejects(w.lifetime("install"), /incomplete deployment/);
  assert.equal(w.stores.get(w.current).get(w.scope + "index.html"), "retained rollback");
});
