import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

// Failure injection happens at the browser API boundary, not in app code.
// Every case executes the real packaged JS/Wasm and the actual native UI model.
export async function checkGpuStartup({ call, evaluate, settle, canvasPixels, url }) {
  const waitFor = (condition) => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>20000)reject(new Error('GPU test timed out: '+document.body.dataset.gpu));else setTimeout(check,50)}check()})`);
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const checkPanelStyle = async () => {
    assert.ok(await evaluate("(()=>{const help=getComputedStyle(document.querySelector('.gpu-help')),panel=getComputedStyle(document.querySelector('.dock-group'));return ['backgroundColor','color','borderRadius','boxShadow'].every(key=>help[key]===panel[key])})()"), "GPU help shares the panel background, text, corners and shadow");
    assert.ok(await evaluate("(()=>{const notice=document.querySelector('#gpu-notice'),n=notice.getBoundingClientRect(),h=document.querySelector('.gpu-help').getBoundingClientRect(),padding=parseFloat(getComputedStyle(notice).paddingTop);return h.height>n.height-2*padding?Math.abs(h.top-n.top-padding)<1:Math.abs((n.top+n.bottom-h.top-h.bottom)/2)<1})()"), "Help panel is centered when it fits and starts at the top when scrolling is needed");
    assert.ok(await evaluate("(()=>{const h=document.querySelector('.gpu-help');return [...h.querySelectorAll('p')].every(p=>getComputedStyle(p).color===getComputedStyle(h).color)})()"), "Help text uses a consistent color");
  };
  const capture = async (name) => {
    const dir = "artifacts/ui/gpu-startup";
    await mkdir(dir, { recursive: true });
    const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/${name}.png`, Buffer.from(shot.data, "base64"));
  };
  // UA overrides validate help routing, not Safari/Firefox/Android GPU drivers.
  const browsers = {
    "unsupported-browser": "Capy test browser",
    "non-linux": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/150.0.0.0",
    "chrome-mac": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/150.0.0.0 Safari/537.36",
    chromeos: "Mozilla/5.0 (X11; CrOS x86_64) Chrome/150.0.0.0",
    edge: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/150.0.0.0 Edg/150.0.0.0",
    "edge-linux": "Mozilla/5.0 (X11; Linux x86_64) Chrome/150.0.0.0 Edg/150.0.0.0",
    android: "Mozilla/5.0 (Linux; Android 12) Chrome/150.0.0.0 Mobile Safari/537.36",
    ios: "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) Version/26.0 Mobile Safari/605.1.15",
    "ios-chrome": "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) CriOS/150.0 Mobile Safari/605.1.15",
    "ipad-desktop": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Version/26.0 Safari/605.1.15",
    safari: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Version/26.0 Safari/605.1.15",
    firefox: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:150.0) Gecko/20100101 Firefox/150.0",
    "firefox-linux": "Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0",
    "firefox-no-adapter": "Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0",
  };
  for (const mode of ["missing-api", "insecure", "no-adapter", "device-failure", "renderer-failure", "pending", ...Object.keys(browsers)]) {
    const browserList = ["unsupported-browser", "safari", "firefox", "firefox-linux", "firefox-no-adapter"].includes(mode);
    const missingApi = ["missing-api", "ios", "ios-chrome", "ipad-desktop"].includes(mode) || (browserList && mode !== "firefox-no-adapter");
    const chromeSteps = !browserList && !["insecure", "android", "ios", "ios-chrome", "ipad-desktop"].includes(mode);
    const linuxSteps = chromeSteps && (!browsers[mode] || mode === "edge-linux");
    const noAdapter = ["no-adapter", "non-linux", "chrome-mac", "chromeos", "edge", "edge-linux", "android", "firefox-no-adapter"].includes(mode);
    await call("Page.navigate", { url: "about:blank" });
    const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
      const originalGpu = navigator.gpu;
      const requestAdapter = originalGpu.requestAdapter.bind(originalGpu);
      const secure = isSecureContext;
      window.restoreGpu = () => { Object.defineProperty(window, 'isSecureContext', { configurable:true, value:secure }); Object.defineProperty(navigator, 'gpu', { configurable:true, value:originalGpu }); originalGpu.requestAdapter = requestAdapter; };
      window.adapterRequests = 0;
      if (${JSON.stringify(!!browsers[mode])}) {
        Object.defineProperty(navigator,'userAgent',{ configurable:true, value:${JSON.stringify(browsers[mode] || "")} });
        Object.defineProperty(navigator,'userAgentData',{ configurable:true, value:undefined });
        Object.defineProperty(navigator,'platform',{ configurable:true, value:'' });
        Object.defineProperty(navigator,'maxTouchPoints',{ configurable:true, value:${mode === "ipad-desktop" ? 5 : 0} });
      }
      if (${JSON.stringify(mode)} === 'insecure') Object.defineProperty(window,'isSecureContext',{ configurable:true, value:false });
      if (${missingApi}) Object.defineProperty(navigator,'gpu',{ configurable:true, value:undefined });
      else originalGpu.requestAdapter = async (...args) => {
        window.adapterRequests++;
        if (${noAdapter}) return null;
        if (${JSON.stringify(mode)} === 'pending') await new Promise(resolve => { window.releaseAdapter = resolve; });
        const adapter = await requestAdapter(...args);
        if (${JSON.stringify(mode)} === 'device-failure') adapter.requestDevice = async () => { throw new Error('test: device refused'); };
        if (${JSON.stringify(mode)} === 'renderer-failure') {
          const requestDevice=adapter.requestDevice.bind(adapter);
          adapter.requestDevice=async (...args)=>{
            const device=await requestDevice(...args), createShader=device.createShaderModule.bind(device);
            device.createShaderModule=descriptor=>createShader({...descriptor,code:'invalid shader for startup test'});
            return device;
          };
        }
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
    if (mode === "no-adapter") {
      await call("Runtime.evaluate", { expression: "document.querySelector('#fullscreen').click()", userGesture: true });
      await waitFor("!!document.fullscreenElement && document.querySelector('#fullscreen').title === 'Exit fullscreen'");
      assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), false);
      await evaluate("document.exitFullscreen()");
      await waitFor("!document.fullscreenElement && document.querySelector('#fullscreen').title === 'Enter fullscreen'");
      await settle();
    }
    if (mode !== "pending") {
      assert.equal(await evaluate("document.querySelectorAll('.gpu-help details, .gpu-help pre, .gpu-retry').length"), 0);
      assert.ok(await evaluate("[...document.querySelectorAll('.gpu-help button')].every(n=>n.closest('.gpu-address'))"), "Only address Copy buttons remain");
      const visibleText = await evaluate("document.querySelector('.gpu-help').innerText");
      assert.equal(await evaluate("document.querySelector('.gpu-help h1').textContent"), "Could not initialize canvas");
      assert.match(visibleText, /Capy Canvas is a GPU-accelerated drawing app/);
      assert.ok(visibleText.split(/\s+/).length < 150, "Instructions stay concise");
      assert.doesNotMatch(visibleText, /Instructions for other platforms|Vulkan:\s*Enabled/);
      const reason = await evaluate("document.querySelector('.gpu-cause').textContent");
      assert.match(reason, mode === "insecure" ? /secure connection/
        : missingApi ? /WebGPU is not available/
        : noAdapter ? /could not find a GPU adapter/
        : mode === "device-failure" ? /found a GPU but could not start/
        : /canvas renderer/);
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
          assert.match(visibleText, /On Linux, enable “Override software rendering list”/);
          assert.match(visibleText, /If needed, enable “Unsafe WebGPU”/);
          const name = mode === "edge-linux" ? "Edge" : "Chrome";
          assert.ok(visibleText.includes(`Alternatively, force ${name} to use the Vulkan driver:`));
          assert.equal(await evaluate("document.querySelector('.gpu-launch').textContent"), `Relaunch ${name} with the --use-angle=vulkan command line option.`);
          assert.equal(await evaluate("document.querySelector('.gpu-launch code').textContent"), "--use-angle=vulkan");
          assert.ok(await evaluate("(()=>{const launch=getComputedStyle(document.querySelector('.gpu-launch code')),address=getComputedStyle(document.querySelector('.gpu-address code'));return ['backgroundColor','color','fontFamily','fontSize','borderRadius','padding','userSelect'].every(key=>launch[key]===address[key])})()"), "Launcher flag uses the same code styling as browser addresses");
          assert.ok(await evaluate("(()=>{const line=document.querySelector('.gpu-launch'),address=document.querySelector('.gpu-address code');return Math.abs(line.getBoundingClientRect().left-address.getBoundingClientRect().left)<1})()"), "Relaunch instruction shares the address-row indent");
          assert.equal(await evaluate("getComputedStyle(document.querySelector('.gpu-launch')).marginTop"), "8px", "Relaunch instruction has the same gap as other instruction rows");
          assert.equal(await evaluate("getComputedStyle(document.querySelector('.gpu-launch')).lineHeight"), await evaluate("getComputedStyle(document.querySelector('.gpu-address button')).minHeight"), "Relaunch line height matches rows with Copy buttons");
          assert.equal(await evaluate("document.querySelector('.gpu-launch').parentElement.nextElementSibling.textContent"), "Display Type should show ANGLE_VULKAN in the GPU debug page:");
          assert.doesNotMatch(visibleText, /WebGPU should show/);
          assert.ok(await evaluate("[...document.querySelectorAll('.gpu-help > p')].some(n=>n.textContent.startsWith('On Linux,'))"), "Linux help is an unnumbered paragraph");
          assert.equal(await evaluate("document.querySelectorAll('.gpu-steps ol').length"), 0, "No nested troubleshooting steps");
        } else {
          assert.doesNotMatch(visibleText, /Linux|experimental|chrome:\/\/flags|Unsafe WebGPU|ANGLE_VULKAN|--use-angle/i);
          assert.match(visibleText, /WebGPU should show “Hardware accelerated”/);
        }
        const scheme = mode.startsWith("edge") ? "edge" : "chrome";
        const addresses = (linuxSteps ? ["settings/system", "flags/#ignore-gpu-blocklist", "flags/#enable-unsafe-webgpu", "gpu"] : ["settings/system", "gpu"]).map(path => `${scheme}://${path}`);
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
        assert.doesNotMatch(visibleText, /chrome:\/\/|edge:\/\/|experimental|graphics acceleration/);
        if (["ios", "ios-chrome", "ipad-desktop"].includes(mode)) assert.match(visibleText, /iOS or iPadOS to 26.*Safari/);
        if (mode === "android") assert.match(visibleText, /Android 12.*supported GPU/);
      }
      assert.equal(await evaluate("document.querySelectorAll('.gpu-browsers').length"), browserList ? 1 : 0);
      if (browserList) {
        assert.equal(reason, missingApi ? "WebGPU is not available in this browser." : "Your browser could not find a GPU adapter.");
        assert.equal(await evaluate("document.querySelector('.gpu-cause').nextElementSibling.textContent"), "Capy Canvas is a GPU-accelerated drawing app and needs access to your GPU. At the moment, the only supported browsers are:");
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.gpu-browsers li')].map(n=>[n.querySelector('strong').textContent,n.textContent])"), [
          ["iPadOS", "iPadOS: Safari (iPadOS 26+)"],
          ["Android", "Android: Chrome (Android 12+)"],
          ["Windows", "Windows: Chrome, Edge, Firefox 141+"],
          ["macOS", "macOS: Chrome, Edge, Safari 26+, Firefox 147+ (Apple Silicon)"],
          ["Linux (Wayland)", "Linux (Wayland): Chrome, Edge"],
        ]);
        assert.ok(await evaluate("[...document.querySelectorAll('.gpu-browsers strong')].every(n=>Number(getComputedStyle(n).fontWeight)>=600)"), "OS names are bold");
        assert.equal(await evaluate("document.querySelectorAll('.gpu-help h2, .gpu-help button').length"), 0);
        assert.doesNotMatch(visibleText, /This browser can’t draw here|Try an updated browser|Update your browser, or try opening/);
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
        for (const [width, height] of [[1280, 720], [900, 760], [900, 700]]) {
          await call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
          await settle();
          await checkPanelStyle();
          const overflow = await evaluate("(()=>{const n=document.querySelector('#gpu-notice');return [n.scrollWidth-n.clientWidth,n.scrollHeight-n.clientHeight]})()");
          assert.equal(overflow[0], 0, "Help never needs horizontal scrolling");
          if (height !== 700) assert.equal(overflow[1], 0, `All instructions fit without scrolling at ${width}×${height}`);
          else {
            assert.ok(await evaluate("(()=>{const n=document.querySelector('#gpu-notice');n.scrollTop=n.scrollHeight;const r=n.getBoundingClientRect(),last=n.querySelector('.gpu-address:last-child').getBoundingClientRect();return last.top>=r.top&&last.bottom<=r.bottom})()"), "The GPU debug-page address and Copy button remain reachable in shorter windows");
            await evaluate("document.querySelector('#gpu-notice').scrollTop=0");
          }
          await capture(`${mode}-${width}x${height}`);
        }
        await call("Emulation.setDeviceMetricsOverride", { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
        await settle();
      }
      assert.equal(await evaluate("window.adapterRequests"), missingApi || mode === "insecure" ? 0 : noAdapter ? 2 : 1);
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
