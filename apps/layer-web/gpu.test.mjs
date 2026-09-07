import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

// Failure injection happens at the browser API boundary, not in app code.
// Every case executes the real packaged JS/Wasm and the actual native UI model.
export async function checkGpuStartup({ call, evaluate, settle, canvasPixels, url }) {
  const waitFor = (condition) => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>20000)reject(new Error('GPU test timed out: '+document.body.dataset.gpu));else setTimeout(check,50)}check()})`);
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const capture = async (name) => {
    const dir = "artifacts/ui/gpu-startup";
    await mkdir(dir, { recursive: true });
    const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/${name}.png`, Buffer.from(shot.data, "base64"));
  };
  for (const mode of ["missing-api", "no-adapter", "device-failure", "pending"]) {
    await call("Page.navigate", { url: "about:blank" });
    const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
      const originalGpu = navigator.gpu;
      const requestAdapter = originalGpu.requestAdapter.bind(originalGpu);
      window.restoreGpu = () => { Object.defineProperty(navigator, 'gpu', { configurable:true, value:originalGpu }); originalGpu.requestAdapter = requestAdapter; };
      window.adapterRequests = 0;
      if (${JSON.stringify(mode)} === 'missing-api') Object.defineProperty(navigator,'gpu',{ configurable:true, value:undefined });
      else originalGpu.requestAdapter = async (...args) => {
        window.adapterRequests++;
        if (${JSON.stringify(mode)} === 'no-adapter') return null;
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
      assert.equal(await evaluate("document.querySelectorAll('.gpu-help details[open]').length"), 0);
      const visibleText = await evaluate("document.querySelector('.gpu-help').innerText");
      assert.ok(visibleText.split(/\s+/).length < 70, "Default help stays brief");
      assert.doesNotMatch(visibleText, /WebGPU|adapter|API|driver|renderer|chrome:\/\//);
      assert.equal(await evaluate("document.querySelector('.gpu-retry').textContent"), "Try again");
      await capture(mode + "-dark");
      assert.equal(await evaluate("getComputedStyle(document.querySelector('.gpu-address code')).userSelect"), "text");
      await action({ type: "set_theme", theme: "light" });
      assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).backgroundColor"), "rgb(184, 184, 184)");
      await capture(mode + "-light");
      assert.equal(await evaluate("window.adapterRequests"), mode === "missing-api" ? 0 : mode === "no-adapter" ? 2 : 1);
      await evaluate("window.restoreGpu();document.querySelector('.gpu-retry').click()");
    } else {
      await evaluate("window.releaseAdapter();window.restoreGpu()");
    }
    await waitFor("document.body.dataset.gpu === 'ready'");
    assert.equal(await evaluate("layerApp.state().brush.diameter"), 37);
    assert.equal(await evaluate("layerApp.state().layers.length"), layerCount + 1);
    assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), true);
    await action({ type: "set_theme", theme: "light" });
    await action({ type: "invoke", command: "fit_canvas" });
    const before = await canvasPixels();
    await call("Input.dispatchMouseEvent", { type: "mousePressed", x: 650, y: 450, button: "left", buttons: 1, clickCount: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: 850, y: 450, button: "left", buttons: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", x: 850, y: 450, button: "left", buttons: 0, clickCount: 1 });
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50, "GPU drawing works after attachment/retry");
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    console.log(`GPU startup: ${mode}, usable UI, no invisible painting, preserved session and recovery passed`);
  }
}
