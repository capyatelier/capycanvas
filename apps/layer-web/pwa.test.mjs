// Real packaged-app checks, using the existing browser harness and a disposable
// loopback server. No deployment, browser installation or OS input injection.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { filesIn, writeWorker } from "./package.mjs";

export async function servePackage() {
  const source = resolve("dist/capycanvas");
  assert.ok(readFileSync(join(source, ".capy-package"), "utf8").trim(), "Build the package first");
  const fixture = mkdtempSync(join(tmpdir(), "capy-pwa-test-"));
  const update = join(fixture, "update");
  cpSync(source, update, { recursive: true });
  let html = readFileSync(join(update, "index.html"), "utf8");
  // A real changed JS/CSS release, not merely a changed HTML comment. Old URLs
  // stay immutable; only the updated HTML/worker point at the new fingerprints.
  for (const [type, suffix] of [["js", '\nglobalThis.capyTestRelease = "updated";'], ["css", "\n:root { --capy-test-release: updated; }"]]) {
    const old = html.match(new RegExp(`assets/[^"/]+\\.[0-9a-f]{20}\\.${type}`))[0];
    const data = readFileSync(join(update, old), "utf8") + suffix;
    const path = old.replace(/\.[0-9a-f]{20}\./, `.${createHash("sha256").update(data).digest("hex").slice(0, 20)}.`);
    writeFileSync(join(update, path), data);
    rmSync(join(update, old));
    html = html.replaceAll(old, path);
  }
  writeFileSync(join(update, "index.html"), html);
  writeWorker(update);
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
      res.writeHead(200, { "Content-Type": mime[extname(file)] || "text/plain",
        "Cache-Control": path.startsWith("assets/") ? "public, max-age=31536000, immutable" : "no-cache" });
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

async function checkPointerIds({ call, evaluate, settle, canvasPixels }) {
  // Synthetic events can carry Safari's negative IDs. Only DOM capture is
  // substituted; routing, Wasm deserialization, coalescing and GPU ink are real.
  await call("Input.setIgnoreInputEvents", { ignore: true });
  await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'});layerApp.dispatch({type:'invoke',command:'fit_canvas'})");
  await settle();
  const before = await canvasPixels();
  let result;
  try {
    result = await evaluate(`(async () => {
      const {app,canvas}=layerApp, input=app.input, pen=app.pen, capture=canvas.setPointerCapture;
      const routed=[], samples=[], captures=[], expectedRoutes=[], expectedSamples=[];
      const ids=[-2147483648,-1234567890,1234567890,-1,0,1,2147483647,-2];
      app.input=function(event){if(event.type==='pointer')routed.push([String(event.id),event.phase]);return input.call(this,event)};
      app.pen=function(records,revision){for(let i=0;i<records.length;i+=11)samples.push([records[i],records[i+1]]);return pen.call(this,records,revision)};
      canvas.setPointerCapture=id=>captures.push(id);
      const settle=()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const r=canvas.getBoundingClientRect(), x=r.x+r.width/2-80, y=r.y+r.height/2-160;
      const emit=(type,id,kind,px,py,coalesced=false)=>{
        const end=['pointerup','pointercancel','lostpointercapture'].includes(type);
        const init={bubbles:true,cancelable:true,pointerId:id,pointerType:kind,clientX:px,clientY:py,button:0,buttons:end?0:1,pressure:end?0:.7};
        const event=new PointerEvent(type,init);
        if(coalesced)Object.defineProperty(event,'getCoalescedEvents',{value:()=>[new PointerEvent(type,{...init,clientX:px-20}),new PointerEvent(type,init)]});
        expectedRoutes.push([String(id>>>0),type==='lostpointercapture'?'cancel':type.slice(7)]);
        canvas.dispatchEvent(event);
      };
      try {
        for(const [i,id] of ids.entries()){
          const kind=i===5?'mouse':'pen', end=i===6?'pointercancel':i===7?'lostpointercapture':'pointerup';
          emit('pointerdown',id,kind,x,y+i*12);
          emit('pointermove',id,kind,x+100,y+i*12,true);
          emit(end,id,kind,x+100,y+i*12);
          expectedSamples.push(...[1,2,2,end==='pointerup'?3:4].map(phase=>[id>>>0,phase]));
          await settle();
        }
        const zoom=app.state().camera.zoom;
        emit('pointerdown',-11,'touch',x,y);
        emit('pointerdown',-12,'touch',x+100,y);
        emit('pointermove',-12,'touch',x+180,y);
        const scale=app.state().camera.zoom/zoom;
        emit('pointerup',-11,'touch',x,y);
        emit('pointerup',-12,'touch',x+180,y);
        await settle();
        return {routed,samples,captures,expectedRoutes,expectedSamples,ids,scale,status:document.querySelector('#status').textContent};
      } finally {app.input=input;app.pen=pen;canvas.setPointerCapture=capture;}
    })()`);
    assert.deepEqual(result.routed, result.expectedRoutes, "Signed DOM IDs retain their bits at the unsigned Rust boundary");
    assert.deepEqual(result.samples, result.expectedSamples, "Brush histories use the same IDs, including cancellation and capture loss");
    assert.deepEqual(result.captures, [...result.ids, -11, -12], "DOM capture receives the original signed IDs");
    assert.ok(result.scale > 1.5, "Two negative touch IDs remain distinct and pinch zoom works");
    assert.doesNotMatch(result.status, /error|invalid|u64/i);
    await evaluate("layerApp.dispatch({type:'invoke',command:'fit_canvas'})");
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50, "Negative-ID pen strokes reach the GPU canvas");
  } finally { await call("Input.setIgnoreInputEvents", { ignore: false }); }
  console.log("Signed pointer IDs: Wasm routing, GPU ink, coalesced samples, cancel/capture loss and multitouch passed");
}

export async function checkPwa({ call, evaluate, settle, canvasPixels, host }) {
  await checkPointerIds({ call, evaluate, settle, canvasPixels });
  await checkFullscreen({ call, evaluate, settle, canvasPixels });
  const point = (selector) => evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  const background = (selector) => evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).backgroundColor`);
  const tap = async (selector) => {
    await call("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [await point(selector)] });
    await call("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await settle();
    assert.equal(await evaluate("document.documentElement.hasAttribute('data-touch')"), true);
  };
  const settings = '#header-end [data-command="settings"]', menu = ".header-menu > summary", zen = '[data-command="zen_mode"]';
  for (const theme of ["light", "dark"]) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 650, y: 450 });
    const idle = await background(settings), menuIdle = await background(menu);
    await tap(settings);
    assert.equal(await evaluate("document.querySelector('#settings').open"), true);
    await evaluate("layerApp.dispatch({type:'cancel_settings'})");
    assert.equal(await background(settings), idle, "Touch leaves no stuck Settings hover");
    for (const pointerType of ["mouse", "pen"]) {
      await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...await point(settings), pointerType });
      await settle();
      assert.equal(await evaluate("document.documentElement.hasAttribute('data-touch')"), false);
      assert.notEqual(await background(settings), idle, `${pointerType} hover returns after touch`);
      await tap(settings);
      await evaluate("layerApp.dispatch({type:'cancel_settings'})");
      assert.equal(await background(settings), idle);
    }
    await tap(menu);
    assert.equal(await background(menu), menuIdle, "Menu names do not retain touch hover");
    assert.equal(await evaluate("document.querySelector('.header-menu').open"), true);
    await tap(menu);
    assert.equal(await evaluate("document.querySelector('.header-menu').open"), false);
    await tap(zen);
    assert.equal(await evaluate(`document.querySelector('${zen}').getAttribute('aria-pressed')`), "true", "Touch preserves intentional toggle selection");
    assert.notEqual(await background(zen), idle);
    await tap(zen);
    assert.equal(await background(zen), idle, "Zen returns to its idle color when toggled off by touch");
  }
  await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 650, y: 450 });
  console.log("Touch controls: Settings, menus, Zen and mouse/pen switching passed in both themes");
  // CDP verifies the app's cursor policy, not the compositor's physical tablet
  // cursor. Chrome's Wayland tablet path can ignore CSS cursor:none; see docs.
  const center = await point("#canvas");
  assert.equal(await evaluate(`document.elementFromPoint(${center.x},${center.y}).id`), "canvas");
  for (const pointerType of ["mouse", "pen"]) {
    for (const [type, offset, buttons] of [
      ["mouseMoved", 0, 0], ["mousePressed", 0, 1],
      ["mouseMoved", 40, 1], ["mouseReleased", 40, 0],
    ]) {
      await call("Input.dispatchMouseEvent", {
        type, pointerType, x: center.x + offset, y: center.y,
        button: buttons || type === "mouseReleased" ? "left" : "none",
        buttons, clickCount: type === "mouseMoved" ? 0 : 1,
        force: buttons ? 0.7 : 0,
      });
      await settle();
      assert.equal(await evaluate("getComputedStyle(layerApp.canvas).cursor"), "none", `${pointerType} ${type} requests no browser cursor`);
      assert.ok(await evaluate("!!document.querySelector('.cursor-outline-front').getAttribute('d')"), `${pointerType} ${type} retains the brush outline`);
    }
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...await point(settings), pointerType });
    await settle();
    assert.equal(await evaluate("document.querySelector('.cursor-outline-front').getAttribute('d')"), "", `${pointerType} leaving the canvas clears the brush outline`);
  }
  console.log("Mouse/pen DOM cursor policy: hover, drawing, release and canvas exit passed (native tablet cursor not tested)");
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
  assert.ok(await evaluate("[document.querySelector('#canvas'), ...document.querySelectorAll('#canvas-cursor, #canvas-cursor *')].every(n=>{const s=getComputedStyle(n);return s.userSelect==='none'&&s.webkitUserSelect==='none'&&s.webkitUserDrag==='none'})"), "Canvas and pen-tip artwork cannot be selected or dragged");
  assert.ok(await evaluate("[...document.querySelectorAll('#canvas-cursor, #canvas-cursor *')].every(n=>getComputedStyle(n).pointerEvents==='none')"));
  assert.equal(await evaluate("getComputedStyle(document.querySelector('#document-title')).userSelect"), "text", "Document names remain copyable");
  for (const path of filesIn(join(host.source, "assets"))) {
    const hash = createHash("sha256").update(readFileSync(join(host.source, "assets", path))).digest("hex").slice(0, 20);
    assert.ok(path.includes(`.${hash}.`), `Final asset bytes match their fingerprint: ${path}`);
  }
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
    assert.equal(await evaluate("document.querySelector('link[rel=apple-touch-icon]').sizes.value"), "180x180");
    assert.equal(await evaluate("document.querySelector('meta[name=apple-mobile-web-app-title]').content"), "Capy Canvas");
    assert.match(await evaluate("document.querySelector('link[rel=apple-touch-icon]').href"), /\/apple-touch-icon\.png\?v=[0-9a-f]{20}$/);
    // The OS icon fetch is not necessarily controlled by the page's worker.
    const apple = await fetch(host.url + path + "apple-touch-icon.png");
    assert.equal(apple.status, 200);
    assert.equal(apple.headers.get("content-type"), "image/png");
    assert.equal(apple.headers.get("cache-control"), "no-cache");
    const png = Buffer.from(await apple.arrayBuffer());
    assert.equal(png.readUInt32BE(16), 180); assert.equal(png.readUInt32BE(20), 180);
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
        const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0,32,32);
        const pixels=ctx.getImageData(0,0,32,32).data;
        const alpha=(x,y)=>pixels[(y*32+x)*4+3];
        return {width:image.naturalWidth,height:image.naturalHeight,
          corners:[alpha(0,0),alpha(31,0),alpha(0,31),alpha(31,31)],background:[...pixels.slice(16*4,17*4)]};
      }));
    })()`);
    assert.deepEqual(icons, [32, 180, 192, 512].map(size => ({
      width: size, height: size, corners: Array(4).fill(size === 180 ? 255 : 0), background: [118, 118, 118, 255],
    })), "Icons load offline with mid-gray backgrounds; only Apple's artwork leaves corner masking to the OS");
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
  await call("Network.setCacheDisabled", { cacheDisabled: false });
  const previousAssets = await evaluate("[document.querySelector('script[type=module]').src, document.querySelector('link[rel=stylesheet]').href]");
  assert.equal(await evaluate("globalThis.capyTestRelease ?? null"), null);
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
  assert.equal(await evaluate("globalThis.capyTestRelease ?? null"), null);
  assert.deepEqual(await evaluate("[document.querySelector('script[type=module]').src, document.querySelector('link[rel=stylesheet]').href]"), previousAssets);
  await call("Page.navigate", { url: "about:blank" });
  // The old worker has no clients now; activation occurs without forcing reloads.
  await new Promise((resolve) => setTimeout(resolve, 500));
  await navigate(host.url);
  assert.equal(await evaluate("globalThis.capyTestRelease"), "updated", "Updated fingerprinted JS actually executes");
  assert.equal(await evaluate("getComputedStyle(document.documentElement).getPropertyValue('--capy-test-release').trim()"), "updated", "Updated fingerprinted CSS actually applies");
  const nextAssets = await evaluate("[document.querySelector('script[type=module]').src, document.querySelector('link[rel=stylesheet]').href]");
  assert.ok(nextAssets.every((url, i) => url !== previousAssets[i]), "HTML points at new JS/CSS URLs");
  const currentKeys = await evaluate("caches.keys()");
  assert.equal(currentKeys.filter((key) => key.startsWith(`capycanvas:${host.url}:`)).length, 1);
  assert.ok(currentKeys.includes(keys.find((key) => key.startsWith(`capycanvas:${host.url}nested/capy/:`))), "Root update must preserve the subpath installation");
  await offline(true);
  await call("Network.setCacheDisabled", { cacheDisabled: true });
  await reload();
  assert.equal(await evaluate("globalThis.capyTestRelease"), "updated", "The upgraded app also starts offline without HTTP cache");
  await offline(false);
  assert.ok(!filesIn(host.source).some((path) => /\.rs$|\.d\.ts$|\.map$|\.toml$/.test(path)));
  console.log("PWA fingerprinted JS/CSS upgrade, failed-update recovery, deferred activation and scope isolation: passed");
}
