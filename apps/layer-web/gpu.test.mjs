import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

// Failure injection happens at the browser API boundary, not in app code.
// Every case executes the real packaged JS/Wasm and the actual native UI model.
export async function checkGpuStartup({ call, evaluate, settle, canvasPixels, url }) {
  const waitFor = (condition) => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>20000)reject(new Error('GPU test timed out: '+document.body.dataset.gpu));else setTimeout(check,50)}check()})`);
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const checkPanelStyle = async () => {
    assert.ok(await evaluate("(()=>{const help=getComputedStyle(document.querySelector('.gpu-help')),panel=getComputedStyle(document.querySelector('.dock-group'));return ['backgroundColor','color','borderRadius','boxShadow'].every(key=>help[key]===panel[key])})()"), "GPU help shares the panel background, text, corners and shadow");
    assert.ok(await evaluate("(()=>{const n=document.querySelector('#gpu-notice').getBoundingClientRect(),h=document.querySelector('.gpu-help').getBoundingClientRect();return Math.abs((n.top+n.bottom-h.top-h.bottom)/2)<1})()"), "Help panel is vertically centered");
    assert.ok(await evaluate("(()=>{const h=document.querySelector('.gpu-help');return [...h.querySelectorAll('p')].every(p=>getComputedStyle(p).color===getComputedStyle(h).color)})()"), "Help text uses a consistent color");
  };
  const capture = async (name) => {
    const dir = "artifacts/ui/gpu-startup";
    await mkdir(dir, { recursive: true });
    const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/${name}.png`, Buffer.from(shot.data, "base64"));
  };
  for (const mode of ["missing-api", "unsupported-browser", "insecure", "no-adapter", "non-linux", "device-failure", "pending"]) {
    const missingApi = ["missing-api", "unsupported-browser"].includes(mode);
    const chromeSteps = !["unsupported-browser", "insecure"].includes(mode);
    const linuxSteps = chromeSteps && mode !== "non-linux";
    await call("Page.navigate", { url: "about:blank" });
    const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
      const originalGpu = navigator.gpu;
      const requestAdapter = originalGpu.requestAdapter.bind(originalGpu);
      const secure = isSecureContext;
      window.restoreGpu = () => { Object.defineProperty(window, 'isSecureContext', { configurable:true, value:secure }); Object.defineProperty(navigator, 'gpu', { configurable:true, value:originalGpu }); originalGpu.requestAdapter = requestAdapter; };
      window.adapterRequests = 0;
      if (${JSON.stringify(mode)} === 'unsupported-browser') Object.defineProperty(navigator,'userAgent',{ configurable:true, value:'Capy test browser' });
      if (${JSON.stringify(mode)} === 'non-linux') Object.defineProperty(navigator,'userAgent',{ configurable:true, value:'Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/140.0.0.0' });
      if (${JSON.stringify(mode)} === 'insecure') Object.defineProperty(window,'isSecureContext',{ configurable:true, value:false });
      if (${missingApi}) Object.defineProperty(navigator,'gpu',{ configurable:true, value:undefined });
      else originalGpu.requestAdapter = async (...args) => {
        window.adapterRequests++;
        if (${JSON.stringify(mode)} === 'no-adapter' || ${JSON.stringify(mode)} === 'non-linux') return null;
        if (${JSON.stringify(mode)} === 'pending') await new Promise(resolve => { window.releaseAdapter = resolve; });
        const adapter = await requestAdapter(...args);
        if (${JSON.stringify(mode)} === 'device-failure') adapter.requestDevice = async () => { throw new Error('test: device refused'); };
        return adapter;
      };
    ` });
    await call("Page.navigate", { url: url + "nested/capy/" });
    await waitFor("!!window.layerApp");
    await waitFor(mode === "pending" ? "!!window.releaseAdapter" : "document.body.dataset.gpu === 'unavailable'");
    assert.equal(await evaluate("layerApp.app.gpu_ready()"), false);
    assert.ok(await evaluate("document.querySelectorAll('.dock-group').length >= 3 && document.querySelectorAll('.brush-preview').length > 10 && !!document.querySelector('#header-end button')"));
    await action({ type: "set_theme", theme: "dark" });
    await checkPanelStyle();
    assert.equal(await evaluate("document.querySelectorAll('head > meta[name=darkreader-lock]').length"), 1);
    assert.equal(await evaluate("document.querySelector('meta[name=color-scheme]').content"), "dark");
    assert.equal(await evaluate("getComputedStyle(document.documentElement).colorScheme"), "dark");
    assert.equal(await evaluate("document.querySelectorAll('meta[name=theme-color]').length"), 1);
    assert.equal(await evaluate("document.querySelector('meta[name=theme-color]').content"), "#333333");
    assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).backgroundColor"), "rgb(51, 51, 51)");
    assert.equal(await evaluate("getComputedStyle(layerApp.canvas).visibility"), "hidden");
    assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).cursor"), "default");
    // The Rust entry point must reject invisible painting, even if called directly.
    assert.match(await evaluate("(()=>{try{layerApp.app.pen(new Float64Array(),0n);return 'accepted'}catch(e){return String(e)}})()"), /unavailable/);
    await evaluate("window.frameCalls=0;const frame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{window.frameCalls++;return frame(...args)}");
    const layerCount = await evaluate("layerApp.state().layers.length");
    await action({ type: "set_brush_size", value: 37 });
    await action({ type: "open_settings", page: "appearance" });
    assert.equal(await evaluate("document.querySelector('#settings').open"), true);
    await action({ type: "cancel_settings" });
    await action({ type: "invoke", command: "add_layer" });
    assert.equal(await evaluate("window.frameCalls"), 0, "No paint loop while the GPU is unavailable");
    if (mode !== "pending") {
      assert.equal(await evaluate("document.querySelectorAll('.gpu-help details, .gpu-help pre, .gpu-retry').length"), 0);
      assert.ok(await evaluate("[...document.querySelectorAll('.gpu-help button')].every(n=>n.closest('.gpu-address'))"), "Only address Copy buttons remain");
      const visibleText = await evaluate("document.querySelector('.gpu-help').innerText");
      assert.equal(await evaluate("document.querySelector('.gpu-help h1').textContent"), "Could not initialize canvas");
      assert.match(visibleText, /Capy Canvas is a GPU-accelerated drawing app/);
      assert.ok(visibleText.split(/\s+/).length < 150, "Instructions stay concise");
      assert.doesNotMatch(visibleText, /Instructions for other platforms|edge:\/\/|brave:\/\/|opera:\/\//);
      assert.doesNotMatch(visibleText, /Experimental flags may cause|Restore Default if needed/);
      assert.equal(await evaluate("document.querySelectorAll('.gpu-steps > li').length"), chromeSteps ? 4 : 0);
      if (chromeSteps) {
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.gpu-steps > li')].slice(0,4).map(n=>n.firstChild.textContent)"), [
          "Open your browser’s system settings:",
          "Turn on “Use graphics acceleration when available”, if available.",
          "Restart the browser.",
          "Reload this page.",
        ]);
        if (linuxSteps) {
          assert.match(visibleText, /On Linux, if it still fails, set “Override software rendering list” to Enabled/);
          assert.match(visibleText, /If that still doesn’t work, set “Unsafe WebGPU” to Enabled/);
          assert.ok(await evaluate("[...document.querySelectorAll('.gpu-help > p')].some(n=>n.textContent.startsWith('On Linux,'))"), "Linux help is an unnumbered paragraph");
          assert.equal(await evaluate("document.querySelectorAll('.gpu-steps ol').length"), 0, "No nested troubleshooting steps");
        } else assert.doesNotMatch(visibleText, /Linux|experimental|chrome:\/\/flags|Unsafe WebGPU/i);
        assert.match(visibleText, /Vulkan should be enabled in Chrome's graphics report/);
        assert.match(visibleText, /chrome:\/\/gpu/);
        const addresses = linuxSteps ? ["chrome://settings/system", "chrome://flags/#ignore-gpu-blocklist", "chrome://flags/#enable-unsafe-webgpu", "chrome://gpu"] : ["chrome://settings/system", "chrome://gpu"];
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.gpu-address code')].map(n=>n.textContent)"), addresses);
        assert.ok(await evaluate("(()=>{const rows=[...document.querySelectorAll('.gpu-address code')];return rows.every(n=>Math.abs(n.getBoundingClientRect().left-rows[0].getBoundingClientRect().left)<1)})()"), "All addresses share one left indent");
        assert.equal(await evaluate("document.querySelectorAll('.gpu-steps h3').length"), 0, "Simple steps, not separate cards");
        assert.equal(await evaluate("getComputedStyle(document.querySelector('.gpu-address code')).userSelect"), "text");
        for (let index = 0; index < addresses.length; index++) {
          await evaluate(`navigator.clipboard.writeText=async text=>{window.copiedAddress=text};document.querySelectorAll('.gpu-address button')[${index}].click()`);
          assert.equal(await evaluate("window.copiedAddress"), addresses[index]);
          assert.equal(await evaluate(`document.querySelectorAll('.gpu-address button')[${index}].textContent`), "Copied");
          await evaluate(`document.querySelectorAll('.gpu-address button')[${index}].textContent='Copy'`);
        }
        await evaluate("navigator.clipboard.writeText=async()=>{throw Error('denied')};document.querySelector('.gpu-address button').click()");
        assert.equal(await evaluate("document.querySelector('.gpu-address button').textContent"), "Copy manually");
        await evaluate("document.querySelector('.gpu-address button').textContent='Copy'");
      } else {
        assert.doesNotMatch(visibleText, /chrome:\/\/flags|experimental/);
        assert.match(visibleText, mode === "insecure" ? /secure connection/ : /WebGPU is not available/);
      }
      await capture(mode + "-dark");
      await action({ type: "set_theme", theme: "light" });
      await checkPanelStyle();
      assert.equal(await evaluate("document.querySelector('meta[name=color-scheme]').content"), "light");
      assert.equal(await evaluate("getComputedStyle(document.documentElement).colorScheme"), "light");
      assert.equal(await evaluate("document.querySelector('meta[name=theme-color]').content"), "#b8b8b8");
      assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).backgroundColor"), "rgb(184, 184, 184)");
      await capture(mode + "-light");
      if (mode === "no-adapter") {
        await action({ type: "set_theme", theme: "dark" });
        for (const [width, height] of [[1280, 720], [900, 700]]) {
          await call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
          await settle();
          await checkPanelStyle();
          assert.deepEqual(await evaluate("(()=>{const n=document.querySelector('#gpu-notice');return [n.scrollWidth-n.clientWidth,n.scrollHeight-n.clientHeight]})()"), [0, 0], `All instructions fit without scrolling at ${width}×${height}`);
          await capture(`${mode}-${width}x${height}`);
        }
        await call("Emulation.setDeviceMetricsOverride", { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
        await settle();
      }
      assert.equal(await evaluate("window.adapterRequests"), missingApi || mode === "insecure" ? 0 : ["no-adapter", "non-linux"].includes(mode) ? 2 : 1);
      await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
      await call("Page.reload");
    } else {
      await action({ type: "set_theme", theme: null });
      for (const value of ["light", "dark"]) {
        await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value }] });
        await settle();
        assert.equal(await evaluate("document.body.dataset.theme"), value);
        assert.equal(await evaluate("document.querySelector('meta[name=color-scheme]').content"), value);
        assert.equal(await evaluate("getComputedStyle(document.documentElement).colorScheme"), value);
      }
      await evaluate("window.releaseAdapter();window.restoreGpu()");
      await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    }
    await waitFor("document.body.dataset.gpu === 'ready'");
    if (mode === "pending") assert.equal(await evaluate("layerApp.state().brush.diameter"), 37);
    assert.equal(await evaluate("layerApp.state().layers.length"), layerCount + (mode === "pending" ? 1 : 0));
    assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), true);
    await action({ type: "set_theme", theme: "light" });
    await action({ type: "invoke", command: "fit_canvas" });
    const before = await canvasPixels();
    await call("Input.dispatchMouseEvent", { type: "mousePressed", x: 650, y: 450, button: "left", buttons: 1, clickCount: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 850, y: 450, button: "left", buttons: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", x: 850, y: 450, button: "left", buttons: 0, clickCount: 1 });
    await settle();
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled");
    const after = await canvasPixels();
    assert.ok(after.white < before.white - 50, `GPU drawing works after attachment/reload: ${JSON.stringify({ before, after })}`);
    console.log(`GPU startup: ${mode}, usable UI, no invisible painting and ${mode === "pending" ? "preserved session" : "reload recovery"} passed`);
  }
}
