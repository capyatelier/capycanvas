// Real packaged-app checks, using the existing browser harness and a disposable
// loopback server. No deployment, browser installation or OS input injection.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { filesIn, writeWorker } from "./package.mjs";

export async function servePackage() {
  const source = resolve("dist/capycanvas");
  assert.ok(readFileSync(join(source, ".capy-package"), "utf8").trim(), "Build the package first");
  const fixture = mkdtempSync(join(tmpdir(), "capy-pwa-test-"));
  cpSync(source, join(fixture, "update"), { recursive: true });
  writeFileSync(join(fixture, "update/index.html"), readFileSync(join(source, "index.html"), "utf8").replace("</head>", "<!-- package-update-test --></head>"));
  writeWorker(join(fixture, "update"));
  const mime = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".webmanifest": "application/manifest+json", ".svg": "image/svg+xml", ".png": "image/png" };
  const state = { online: true, update: false, broken: false, requests: [] };
  const server = createServer((req, res) => {
    const pathname = decodeURIComponent(new URL(req.url, "http://local").pathname);
    const nested = pathname.startsWith("/nested/capy/");
    const path = pathname.slice(nested ? "/nested/capy/".length : 1) || "index.html";
    state.requests.push(pathname);
    if (!state.online) { res.writeHead(503); res.end(); return; }
    const directory = state.update && !nested ? join(fixture, "update") : source;
    const file = resolve(directory, path);
    if (!file.startsWith(directory + "/")) { res.writeHead(403); res.end(); return; }
    try {
      const body = state.broken && path.endsWith(".wasm") ? Buffer.from("mismatched release") : readFileSync(file);
      res.writeHead(200, { "Content-Type": mime[extname(file)] || "text/plain", "Cache-Control": "no-store" });
      res.end(body);
    } catch { res.writeHead(404); res.end(); }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { state, url: `http://127.0.0.1:${server.address().port}/`, source,
    close: async () => {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      rmSync(fixture, { recursive: true, force: true });
    } };
}

async function checkFullscreen({ call, evaluate, settle, canvasPixels }) {
  const waitFor = (condition) => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>10000)reject(Error('Fullscreen check timed out'));else setTimeout(check,25)}check()})`);
  const toggle = async () => {
    const point = await evaluate("(()=>{const r=document.querySelector('#fullscreen').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()");
    for (const type of ["mousePressed", "mouseReleased"])
      await call("Input.dispatchMouseEvent", { type, ...point, button: "left", buttons: type === "mousePressed" ? 1 : 0, clickCount: 1 });
  };
  const checkButton = async (active) => {
    // The exit promise can settle before the browser delivers fullscreenchange.
    await waitFor(`!!document.fullscreenElement === ${active} && !document.querySelector('#fullscreen').disabled && document.querySelector('#fullscreen').title === '${active ? "Exit fullscreen" : "Enter fullscreen"}'`);
    assert.equal(await evaluate("document.querySelector('#fullscreen').title"), active ? "Exit fullscreen" : "Enter fullscreen");
    assert.equal(await evaluate("document.querySelector('#fullscreen').getAttribute('aria-label')"), active ? "Exit fullscreen" : "Enter fullscreen");
    assert.equal(await evaluate("document.querySelector('#fullscreen svg').dataset.asset"), active ? "fullscreen-exit" : "fullscreen-enter");
    await settle();
  };
  await call("Emulation.clearDeviceMetricsOverride");
  await settle();
  try {
    await checkButton(false);
    assert.ok(await evaluate("(()=>{const f=document.querySelector('#fullscreen'),s=f.nextElementSibling,a=f.getBoundingClientRect(),b=s.getBoundingClientRect();return s.dataset.command==='settings'&&a.width===36&&a.height===36&&b.width===36&&b.height===36&&Math.abs(b.left-a.right-6)<1&&a.top===b.top})()"), "Fullscreen sits immediately left of Settings with matching size and spacing");
    await evaluate("window.originalFullscreenRequest=document.documentElement.requestFullscreen;document.documentElement.requestFullscreen=()=>Promise.reject(Error('test denied'))");
    await toggle();
    await waitFor("document.querySelector('#status').textContent === 'Could not change fullscreen mode.'");
    await checkButton(false);
    await evaluate("document.documentElement.requestFullscreen=window.originalFullscreenRequest;delete window.originalFullscreenRequest;document.querySelector('#status').textContent=''");
    await toggle();
    await checkButton(true);
    assert.ok(await evaluate("document.fullscreenElement === document.documentElement"), "Fullscreen includes the whole UI, not only the canvas");
    await evaluate("layerApp.dispatch({type:'open_settings',page:'appearance'})");
    assert.ok(await evaluate("document.querySelector('#settings').open && document.fullscreenElement.contains(document.querySelector('#settings'))"));
    await evaluate("layerApp.dispatch({type:'cancel_settings'});layerApp.dispatch({type:'set_theme',theme:'light'});layerApp.dispatch({type:'invoke',command:'fit_canvas'})");
    await settle();
    const before = await canvasPixels();
    const point = await evaluate("({x:innerWidth/2-100,y:innerHeight/2})");
    await call("Input.dispatchMouseEvent", { type: "mousePressed", ...point, button: "left", buttons: 1, clickCount: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x + 200, y: point.y, button: "left", buttons: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", x: point.x + 200, y: point.y, button: "left", buttons: 0, clickCount: 1 });
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled");
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50, "GPU ink renders after the fullscreen resize");
    mkdirSync("artifacts/ui/fullscreen", { recursive: true });
    for (const theme of ["light", "dark"]) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`); await settle();
      const shot = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(`artifacts/ui/fullscreen/${theme}.png`, Buffer.from(shot.data, "base64"));
    }
    await toggle();
    await checkButton(false);
    await toggle();
    await checkButton(true);
    // Browser-controlled exits (including Escape) dispatch this same event.
    await evaluate("document.exitFullscreen()");
    await checkButton(false);
  } finally {
    await evaluate("document.fullscreenElement ? document.exitFullscreen() : undefined").catch(() => {});
    await call("Emulation.setDeviceMetricsOverride", { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
    await settle();
  }
  // An unavailable Fullscreen API must not break startup or the Settings button.
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: "Object.defineProperty(document,'fullscreenEnabled',{value:false,configurable:true})" });
  const previous = await evaluate("performance.timeOrigin");
  await call("Page.reload");
  await waitFor(`performance.timeOrigin !== ${previous} && document.body.dataset.gpu === 'ready'`);
  assert.equal(await evaluate("document.querySelector('#fullscreen').disabled"), true);
  assert.equal(await evaluate("document.querySelector('#fullscreen').title"), "Fullscreen unavailable");
  await evaluate("document.querySelector('#header-end [data-command=settings]').click()");
  assert.equal(await evaluate("document.querySelector('#settings').open"), true);
  await evaluate("layerApp.dispatch({type:'cancel_settings'})");
  await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
  console.log("Fullscreen: geometry, real enter/exit, external exit, failure recovery, settings and GPU ink passed");
}

export async function checkPwa({ call, evaluate, settle, canvasPixels, host }) {
  await checkFullscreen({ call, evaluate, settle, canvasPixels });
  const ready = async (previous) => {
    const start = Date.now();
    while (Date.now() - start < 25000) {
      try {
        if (await evaluate(`performance.timeOrigin !== ${previous} && !!window.layerApp && document.body.dataset.gpu === 'ready'`)) return;
      } catch (error) {
        if (!/navigated|context|Cannot find/i.test(String(error))) throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw new Error(await evaluate("document.querySelector('#status')?.textContent || 'Packaged app did not load'"));
  };
  const navigate = async (url) => {
    const previous = await evaluate("performance.timeOrigin");
    await call("Page.navigate", { url }); await ready(previous); await settle();
  };
  const reload = async () => {
    const previous = await evaluate("performance.timeOrigin");
    await call("Page.reload", { ignoreCache: true }); await ready(previous);
  };
  const offline = async (value) => {
    host.state.online = !value;
    await call("Network.emulateNetworkConditions", { offline: value, latency: 0, downloadThroughput: -1, uploadThroughput: -1 });
  };
  await call("Network.enable");
  await call("Network.setCacheDisabled", { cacheDisabled: true });
  for (const path of ["", "nested/capy/"]) {
    await navigate(host.url + path);
    const manifest = await call("Page.getAppManifest");
    assert.deepEqual(manifest.errors, []);
    const data = JSON.parse(manifest.data);
    assert.equal(new URL(data.scope, manifest.url).href, host.url + path);
    assert.equal(new URL(data.start_url, manifest.url).href, host.url + path);
    assert.equal(data.display, "standalone");
    assert.deepEqual(data.icons.map((icon) => icon.sizes), ["192x192", "512x512"]);
    assert.ok(data.icons.every((icon) => icon.purpose === "any"), "Rounded artwork is not marked maskable");
    await evaluate(`navigator.serviceWorker.ready.then(()=>new Promise(resolve=>{if(navigator.serviceWorker.controller)resolve(true);else navigator.serviceWorker.addEventListener('controllerchange',()=>resolve(true),{once:true})}))`);
    assert.deepEqual((await call("Page.getInstallabilityErrors")).installabilityErrors, []);
    assert.ok(await evaluate(`(async()=>{const keys=await caches.keys();return keys.some(k=>k.startsWith('capycanvas:'+location.href+':'))})()`));
    await offline(true);
    await reload();
    assert.equal(await evaluate("document.querySelector('link[rel=icon]').sizes.value"), "32x32");
    const icons = await evaluate(`(async()=>{
      const manifestUrl=document.querySelector('link[rel=manifest]').href;
      const manifest=await (await fetch(manifestUrl)).json();
      const urls=[document.querySelector('link[rel=icon]').href,
        document.querySelector('link[rel=apple-touch-icon]').href,
        ...manifest.icons.map(icon=>new URL(icon.src,manifestUrl).href)];
      return Promise.all(urls.map(async url=>{
        const image=new Image();image.src=url;await image.decode();
        const canvas=document.createElement('canvas');canvas.width=canvas.height=32;
        const ctx=canvas.getContext('2d');ctx.drawImage(image,0,0,32,32);
        const pixels=ctx.getImageData(0,0,32,32).data;
        const alpha=(x,y)=>pixels[(y*32+x)*4+3];
        return {width:image.naturalWidth,height:image.naturalHeight,
          corners:[alpha(0,0),alpha(31,0),alpha(0,31),alpha(31,31)],background:[...pixels.slice(16*4,17*4)]};
      }));
    })()`);
    assert.deepEqual(icons, [32, 180, 192, 512].map(size => ({
      width: size, height: size, corners: [0, 0, 0, 0], background: [118, 118, 118, 255],
    })), "Favicon and app icons load offline with rounded corners and mid-gray backgrounds");
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'});layerApp.dispatch({type:'invoke',command:'fit_canvas'})");
    await settle();
    assert.ok(await evaluate("Promise.all([...document.querySelectorAll('.brush-preview')].map(i=>i.decode())).then(()=>true)"));
    const before = await canvasPixels();
    await call("Input.dispatchMouseEvent", { type: "mousePressed", x: 650, y: 450, button: "left", buttons: 1, clickCount: 1 });
    for (let x = 670; x <= 900; x += 20)
      await call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y: 450, button: "left", buttons: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", x: 900, y: 450, button: "left", buttons: 0, clickCount: 1 });
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50, "Real GPU ink must render after an offline cold reload");
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'dark'})");
    await settle();
    assert.ok(await evaluate("Promise.all([...document.querySelectorAll('.brush-preview')].map(i=>i.decode())).then(()=>true)"));
    await offline(false);
    console.log(`PWA installability, offline Wasm/ink/previews: /${path}`);
  }

  await navigate(host.url);
  const snapshot = "JSON.stringify(layerApp.state(),(_,v)=>typeof v==='bigint'?String(v):v)";
  const before = await evaluate(snapshot);
  const keys = await evaluate("caches.keys()");
  host.state.update = true;
  host.state.broken = true;
  // A mismatched asset returned with HTTP 200 must fail integrity checking,
  // leaving the old active version untouched.
  assert.equal(await evaluate(`(async()=>{const r=await navigator.serviceWorker.getRegistration();const seen=new Promise(resolve=>r.addEventListener('updatefound',()=>{const w=r.installing;w.addEventListener('statechange',()=>{if(w.state==='redundant')resolve(w.state)})},{once:true}));await r.update();return seen})()`), "redundant");
  assert.deepEqual(await evaluate("caches.keys()"), keys);
  host.state.broken = false;
  await evaluate(`(async()=>{const r=await navigator.serviceWorker.getRegistration();const seen=new Promise(resolve=>r.addEventListener('updatefound',()=>{const w=r.installing;w.addEventListener('statechange',()=>{if(w.state==='installed')resolve(true)})},{once:true}));await r.update();return seen})()`);
  assert.ok(await evaluate("navigator.serviceWorker.getRegistration().then(r=>!!r.waiting)"));
  assert.equal(await evaluate(snapshot), before, "Waiting update must not change the live session");
  assert.ok(!await evaluate("document.documentElement.outerHTML.includes('package-update-test')"));
  await call("Page.navigate", { url: "about:blank" });
  // The old worker has no clients now; activation occurs without forcing reloads.
  await new Promise((resolve) => setTimeout(resolve, 500));
  await navigate(host.url);
  assert.ok(await evaluate("document.documentElement.outerHTML.includes('package-update-test')"));
  const currentKeys = await evaluate("caches.keys()");
  assert.equal(currentKeys.filter((key) => key.startsWith(`capycanvas:${host.url}:`)).length, 1);
  assert.ok(currentKeys.includes(keys.find((key) => key.startsWith(`capycanvas:${host.url}nested/capy/:`))), "Root update must preserve the subpath installation");
  await offline(true);
  await reload();
  await offline(false);
  assert.ok(!filesIn(host.source).some((path) => /\.rs$|\.d\.ts$|\.map$|\.toml$/.test(path)));
  console.log("PWA failed-update recovery, deferred activation and scope isolation: passed");
}
