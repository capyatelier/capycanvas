import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkCallbackBoundaries({call,evaluate,settle,errors}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+60000;function check(){if(${condition})resolve();else if(performance.now()>end)reject(Error('Callback boundary timeout: '+${JSON.stringify(condition)}+' '+document.body.innerText.slice(-1200)));else setTimeout(check,30)}check()})`);
  const click=label=>evaluate(`(()=>{const button=[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent===${JSON.stringify(label)});if(!button)throw Error('Missing '+${JSON.stringify(label)});button.click()})()`);
  await evaluate(`(async()=>{
    const wasm=await import('./pkg/layer_web.js');
    const {createRasterWorker}=await import('./raster-worker-client.js');
    window.callbackBoundaries={wasm,transport:createRasterWorker(),diagnostics:[],requests:[],saved:null,outputs:[],picker:window.showSaveFilePicker};
    window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){for(const token of callbackBoundaries.outputs.splice(0))await callbackBoundaries.transport({operation:'output-close',metadata:token,buffers:[]});callbackBoundaries.saved=Array.from(bytes.slice(0,4))},async abort(){}}}});
  })()`);
  try {
    for(const theme of ['light','dark']) {
      await evaluate(`(()=>{
        layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});
        const test=callbackBoundaries;test.diagnostics=[];test.requests=[];test.completions=[];test.saved=null;
        const replacement=event=>{test.wasm.configure_gpu_diagnostics(replacement);test.diagnostics.push({callback:'replacement',kind:event.kind,message:event.message})};
        test.wasm.configure_gpu_diagnostics(event=>{test.wasm.configure_gpu_diagnostics(replacement);test.diagnostics.push({callback:'first',kind:event.kind,message:event.message})});
        const device=layerApp.canvas.getContext('webgpu').getConfiguration().device;
        for(const message of ['capy-test: first diagnostic','capy-test: replacement diagnostic'])device.dispatchEvent(new GPUUncapturedErrorEvent('uncapturederror',{error:new GPUValidationError(message)}));
        const worker=request=>{test.wasm.configure_raster_worker(worker);test.requests.push({operation:request.operation,cancellable:typeof request.cancelled==='function'});return test.transport(request).then(value=>{if(request.operation==='write')test.outputs.push(value.token);if(typeof request.cancelled==='function')test.completions.push({operation:request.operation,cancelled:request.cancelled()});return value})};
        test.wasm.configure_raster_worker(worker);
      })()`);
      const diagnostics=await evaluate('callbackBoundaries.diagnostics');
      assert.deepEqual(diagnostics.map(({callback,kind})=>({callback,kind})),[{callback:'first',kind:'validation'},{callback:'replacement',kind:'validation'}]);
      for(const [index,message] of ['first','replacement'].entries())assert.match(diagnostics[index].message,new RegExp(`capy-test: ${message} diagnostic`));
      await settle();
      assert.equal(errors.length,2,'Both injected validation diagnostics reach the browser console');
      for(const error of errors.splice(0))assert.match(error,/capy-test: (first|replacement) diagnostic/,'Only injected diagnostics are expected');
      await evaluate("layerApp.dispatch({type:'invoke',command:'save_document_as'})");
      await wait("callbackBoundaries.saved!==null&&!layerApp.documents.busy()&&layerApp.state().commands.find(command=>command.id==='export_document').enabled");
      assert.deepEqual(await evaluate('callbackBoundaries.saved'),[80,75,3,4],'Reconfigured worker returns a real saved package');
      await evaluate("layerApp.dispatch({type:'invoke',command:'export_document'})");
      await wait("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Preview Output'&&!button.disabled)");
      await click('Preview Output');
      await wait("!!document.querySelector('canvas[aria-label=\"Output preview\"]')");
      const requests=await evaluate('callbackBoundaries.requests');
      assert.ok(requests.some(request=>request.operation==='write'&&!request.cancellable),'Save reconfigures the ordinary worker callback');
      for(const operation of ['output-begin','output-encode'])assert.ok(requests.some(request=>request.operation===operation&&request.cancellable),`${operation} reconfigures the cancellable worker callback`);
      const completions=await evaluate('callbackBoundaries.completions');
      for(const operation of ['output-begin','output-encode'])assert.ok(completions.some(completion=>completion.operation===operation&&!completion.cancelled),`${operation} retains its cancellation callback until the transport promise completes`);
      const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/callback-boundaries';await mkdir(directory,{recursive:true});
      const screenshot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/web-${theme}.png`,Buffer.from(screenshot.data,'base64'));
      await click('Cancel');await settle();
      assert.equal(await evaluate('document.body.dataset.gpu'),'ready');
      console.log(`Callback boundaries ${theme}: diagnostics replacement, package save and cancellable export preview passed`);
    }
  } finally {await evaluate('window.showSaveFilePicker=callbackBoundaries.picker');}
}

// API exceptions escaping the actual Wasm initialization future (not a mock app).
export async function checkGpuCompatibility({ call, evaluate, settle, url, errors }) {
  const waitFor = async condition => {
    const deadline=Date.now()+20000;
    while(Date.now()<deadline) {
      try { if(await evaluate(`Boolean(${condition})`))return; }
      catch(error) { if(!/navigated|context.*destroyed|Cannot find context/i.test(String(error)))throw error; }
      await new Promise(resolve=>setTimeout(resolve,50));
    }
    throw Error('Startup stuck: '+condition);
  };
  const activate = async () => {
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:1,y:1,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  };
  for (const mode of ["pipeline-throw", "escaped-rejection"]) {
    assert.deepEqual(errors, []);
    await activate();
    await call("Page.navigate", { url: "about:blank" });
    const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
      window.layoutChecks=0;
      const requestAdapter=navigator.gpu.requestAdapter.bind(navigator.gpu);
      navigator.gpu.requestAdapter=async (...args)=>{
        const adapter=await requestAdapter(...args), requestDevice=adapter.requestDevice.bind(adapter);
        adapter.requestDevice=async (...args)=>{
          if(${JSON.stringify(mode)}==='escaped-rejection') return new Promise(()=>{
            Promise.reject(new Error('capy-test: escaped rejection'));
          });
          const device=await requestDevice(...args), createLayout=device.createPipelineLayout.bind(device);
          device.createPipelineLayout=descriptor=>{
            window.layoutChecks++;
            if(${JSON.stringify(mode)}==='pipeline-throw') throw new TypeError('capy-test: pipeline exception');
            return createLayout(descriptor);
          };
          return device;
        };
        return adapter;
      };
    ` });
    await call("Page.navigate", { url: url + "nested/capy/" });
    await waitFor("!!window.layerApp && document.body.dataset.gpu === 'unavailable'");
    await settle();
    assert.equal(await evaluate("window.layoutChecks > 0"), mode !== "escaped-rejection");
    assert.equal(await evaluate("layerApp.app.gpu_ready()"), false);
    assert.equal(await evaluate("document.querySelector('.gpu-help h1').textContent"), "Could not initialize canvas");
    assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), false);
    assert.ok((await evaluate("document.querySelector('#gpu-notice').innerText")).includes(
      mode === "pipeline-throw" ? "capy-test: pipeline exception" : "capy-test: escaped rejection"));
    await evaluate("layerApp.dispatch({type:'open_settings',page:'appearance'})");
    assert.ok(await evaluate("document.querySelector('#settings').open"), "Settings remain usable after startup failure");
    await evaluate("layerApp.dispatch({type:'close_settings'})");
    assert.ok(errors.length, "Injected exception was reported");
    for (const error of errors.splice(0)) assert.match(error, /capy-test:/, "Only injected failures are expected");
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    const previous = await evaluate("performance.timeOrigin");
    await activate();
    await call("Page.reload");
    for (;;) try { await waitFor(`performance.timeOrigin !== ${previous} && !!window.layerApp && document.body?.dataset.gpu === 'ready'`); break; }
      catch (error) { if (!/navigated|context|Cannot find/i.test(String(error))) throw error; }
    await settle();
    assert.deepEqual(errors, [], "Reload without injection recovers cleanly");
    console.log(`GPU compatibility: ${mode} passed`);
  }
  await evaluate(`(()=>{
    const app=layerApp.app;
    window.gpuStopFault={failure:app.gpu_failure,suspend:app.suspend_gpu};
    app.gpu_failure=()=>"capy-test: primary shader failure";
    app.suspend_gpu=()=>{throw Error("capy-test: secondary suspension failure")};
  })()`);
  try {
    await waitFor("document.body.dataset.gpu === 'unavailable'");
    assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), false);
    assert.match(await evaluate("document.querySelector('#gpu-notice').innerText"), /capy-test: primary shader failure/);
    assert.ok(await evaluate("[...document.querySelectorAll('#gpu-notice button')].some(button=>button.textContent==='Restart canvas'&&!button.disabled)"));
  } finally {
    await evaluate("Object.assign(layerApp.app,{gpu_failure:gpuStopFault.failure,suspend_gpu:gpuStopFault.suspend})");
    for(const error of errors.splice(0))assert.match(error,/capy-test:/);
    await activate();
    await call('Page.reload');
    await waitFor("!!window.layerApp && document.body.dataset.gpu === 'ready'");
  }
}

// Failure injection happens at the browser API boundary, not in app code.
// Every case executes the real packaged JS/Wasm and the actual native UI model.
export async function checkGpuStartup({ call, evaluate, settle, canvasPixels, url, errors }) {
  const waitFor = (condition) => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve(true);else if(performance.now()-start>20000)reject(new Error('GPU test timed out: '+JSON.stringify({gpu:document.body.dataset.gpu,dialogs:[...document.querySelectorAll('dialog[open]')].map(dialog=>dialog.textContent),busy:window.layerApp?.documents.busy(),park:window.layerApp?.app.document_park_ready(),camera:window.layerApp?.state().camera,commands:window.layerApp?.state().commands.filter(command=>['undo','brush','deselect'].includes(command.id))},(_,value)=>typeof value==='bigint'?String(value):value)));else setTimeout(check,50)}check()})`);
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const checkPanelStyle = async () => {
    assert.ok(await evaluate("(()=>{const help=getComputedStyle(document.querySelector('.gpu-help')),node=document.querySelector('#workspace').appendChild(document.createElement('section')),probe=document.body.appendChild(document.createElement('i'));node.className='dock-group';probe.style.background='var(--panel)';const panel=getComputedStyle(node),result=help.backgroundColor===getComputedStyle(probe).backgroundColor&&['color','borderRadius','boxShadow'].every(key=>help[key]===panel[key]);node.remove();probe.remove();return result})()"), "GPU help shares the opaque panel background, text, corners and shadow");
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
  await evaluate('layerApp.documents.autosave()');
  for (const mode of ["missing-api", "insecure", "no-adapter", "device-failure", "renderer-failure", "pending", ...Object.keys(browsers)]) {
    assert.deepEqual(errors, [], "The preceding recovery has no unexpected browser errors");
    const durableLayerCount = await evaluate('layerApp.state().layers.length');
    const browserList = ["unsupported-browser", "safari", "firefox", "firefox-linux", "firefox-no-adapter"].includes(mode);
    const missingApi = ["missing-api", "ios", "ios-chrome", "ipad-desktop"].includes(mode) || (browserList && mode !== "firefox-no-adapter");
    const chromeSteps = !browserList && !["insecure", "android", "ios", "ios-chrome", "ipad-desktop"].includes(mode);
    const linuxSteps = chromeSteps && (browsers[mode] ? mode === "edge-linux" : process.platform === "linux");
    const noAdapter = ["no-adapter", "non-linux", "chrome-mac", "chromeos", "edge", "edge-linux", "android", "firefox-no-adapter"].includes(mode);
    await call("Page.navigate", { url: "about:blank" });
    const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
      const originalGpu = navigator.gpu;
      const requestAdapter = originalGpu.requestAdapter.bind(originalGpu);
      const secure = isSecureContext;
      window.restoreGpu = () => { Object.defineProperty(window, 'isSecureContext', { configurable:true, value:secure }); Object.defineProperty(navigator, 'gpu', { configurable:true, value:originalGpu }); originalGpu.requestAdapter = requestAdapter; };
      window.adapterRequests = 0;
      window.deviceFault = true;
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
        if (${JSON.stringify(mode)} === 'device-failure') {
          const requestDevice = adapter.requestDevice.bind(adapter);
          adapter.requestDevice = async (...args) => {
            if(window.deviceFault) throw new Error('test: device refused'+' capy-test: long device diagnostic'.repeat(400));
            return requestDevice(...args);
          };
        }
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
    await waitFor("(v=>v?.ready&&!v.busy)(JSON.parse(layerApp.app.workspace_view()))");
    assert.ok(await evaluate("(()=>{const groups=layerApp.app.layout(innerWidth,innerHeight).groups.length,brushes=document.querySelectorAll('.brushes-control [data-brush]');return groups>0&&document.querySelectorAll('.dock-group').length===groups&&[...brushes].every(n=>n.querySelector('.brush-preview'))&&document.querySelectorAll('#header .header-item').length>0})()"), "The workspace controls render without a GPU");
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
    await action({ type: "close_settings" });
    await action({ type: "invoke", command: "add_layer" });
    assert.equal(await evaluate("window.frameCalls"), 0, "No paint loop while the GPU is unavailable");
    if (mode === "no-adapter") {
      await call("Runtime.evaluate", { expression: "document.querySelector('#fullscreen').click()", userGesture: true });
      await waitFor("!!document.fullscreenElement && document.querySelector('#fullscreen svg').dataset.asset === 'fullscreen-exit'");
      assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), false);
      await evaluate("document.exitFullscreen()");
      await waitFor("!document.fullscreenElement && document.querySelector('#fullscreen svg').dataset.asset === 'fullscreen-enter'");
      await settle();
    }
    if (mode !== "pending") {
      assert.equal(await evaluate("document.querySelectorAll('.gpu-help details').length"), 0);
      assert.equal(await evaluate("document.querySelectorAll('.gpu-help .gpu-retry').length"), 1);
      assert.ok(await evaluate("[...document.querySelectorAll('.gpu-help button')].every(n=>n.closest('.gpu-address')||n.classList.contains('gpu-retry'))"), "Copy controls and one canvas restart remain reachable");
      const visibleText = await evaluate("document.querySelector('.gpu-help').innerText");
      assert.equal(await evaluate("document.querySelector('.gpu-help h1').textContent"), "Could not initialize canvas");
      assert.match(visibleText, /Capy Canvas is a GPU-accelerated drawing app/);
      const instructions = await evaluate("[...document.querySelector('.gpu-help').children].filter(node=>!node.matches('.diagnostic-detail')).map(node=>node.innerText).join(' ')");
      assert.ok(instructions.split(/\s+/).length < 150, "Instructions stay concise independently of driver diagnostics");
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
        assert.equal(await evaluate("[...document.querySelectorAll('.gpu-help > p')].find(node=>node.textContent.startsWith('Capy Canvas is a GPU-accelerated')).textContent"), "Capy Canvas is a GPU-accelerated drawing app and needs access to your GPU. At the moment, the only supported browsers are:");
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.gpu-browsers li')].map(n=>[n.querySelector('strong').textContent,n.textContent])"), [
          ["iPadOS", "iPadOS: Safari (iPadOS 26+)"],
          ["Android", "Android: Chrome (Android 12+)"],
          ["Windows", "Windows: Chrome, Edge, Firefox 141+"],
          ["macOS", "macOS: Chrome, Edge, Safari 26+, Firefox 147+ (Apple Silicon)"],
          ["Linux (Wayland)", "Linux (Wayland): Chrome, Edge"],
        ]);
        assert.ok(await evaluate("[...document.querySelectorAll('.gpu-browsers strong')].every(n=>Number(getComputedStyle(n).fontWeight)>=600)"), "OS names are bold");
        assert.equal(await evaluate("document.querySelectorAll('.gpu-help h2').length"), 0);
        assert.equal(await evaluate("document.querySelectorAll('.gpu-help button').length"), 1);
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
        const horizontal = "(()=>{const n=document.querySelector('#gpu-notice');return n.scrollWidth-n.clientWidth})()";
        const switchTo = async id => { await action({ type: "workspace_manager", command: { type: "switch", id } }); await waitFor(`(v=>v?.id===${JSON.stringify(id)}&&v.ready&&!v.busy)(JSON.parse(layerApp.app.workspace_view()))`); await settle(); };
        const startWorkspace = await evaluate("JSON.parse(layerApp.app.workspace_view()).id");
        await call("Emulation.setDeviceMetricsOverride", { width: 1280, height: 720, deviceScaleFactor: 1, mobile: false });
        await switchTo("builtin:workspace:illustrator");
        assert.equal(await evaluate(horizontal), 0, "Paint's narrow help column never needs horizontal scrolling");
        await switchTo("builtin:workspace:painter");
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
        await switchTo(startWorkspace);
      }
      assert.equal(await evaluate("window.adapterRequests"), missingApi || mode === "insecure" ? 0 : noAdapter ? 2 : 1);
      if (mode === "device-failure") {
        const origin = await evaluate("performance.timeOrigin");
        assert.match(await evaluate("document.querySelector('#gpu-notice').innerText"), /test: device refused/);
        await call("Emulation.setDeviceMetricsOverride", {width:900,height:600,deviceScaleFactor:1,mobile:false});
        const clickRestart = async () => {
          const point = await evaluate("(()=>{const button=document.querySelector('.gpu-retry'),r=button.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return{x,y,visible:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight,hit:document.elementFromPoint(x,y)?.closest('.gpu-retry')===button};})()");
          assert.equal(point.visible,true,'Long diagnostic keeps Restart in the viewport');
          assert.equal(point.hit,true,'Restart receives native pointer input');
          for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:point.x,y:point.y,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
        };
        for(const [index,theme] of ['light','dark'].entries()) {
          await action({type:'set_theme',theme});
          assert.ok(await evaluate("(()=>{const node=document.querySelector('.diagnostic-detail');return node.scrollHeight>node.clientHeight&&node.getBoundingClientRect().height<=Math.min(innerHeight*.3,180)+1&&getComputedStyle(node).userSelect==='text';})()"),'Long cause remains selectable in a bounded scrolling region');
          await clickRestart();
          await waitFor(`document.body.dataset.gpu === 'unavailable' && adapterRequests === ${index+2}`);
          assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), false);
          assert.match(await evaluate("document.querySelector('#gpu-notice').innerText"), /test: device refused/);
        }
        await evaluate("window.deviceFault = false");
        await clickRestart();
        await waitFor("document.body.dataset.gpu === 'ready' && layerApp.app.brush_ready()");
        await evaluate('layerApp.documents.startRecovery()');
        await evaluate('layerApp.documents.autosave()');
        assert.equal(await evaluate("performance.timeOrigin"), origin);
        assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), true);
        assert.doesNotMatch(await evaluate("document.querySelector('#gpu-notice').textContent"), /test: device refused/);
        await call("Emulation.setDeviceMetricsOverride", {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
      }
      await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
      const previous = await evaluate("performance.timeOrigin");
      await call("Page.reload");
      for (;;) try { await waitFor(`performance.timeOrigin !== ${previous} && !!window.layerApp`); break; }
        catch (error) { if (!/navigated|context|Cannot find/i.test(String(error))) throw error; }
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
    for (const error of errors.splice(0)) assert.match(error, /^GPU canvas unavailable:|^Blocked attempt to show a 'beforeunload' confirmation panel/,
      "Only the injected initialization failures and the unsaved added layer's reload prompt are expected");
    await waitFor("document.body.dataset.gpu === 'ready' && layerApp.app.brush_ready()");
    await evaluate(`Promise.race([layerApp.documents.startRecovery(),new Promise((_,reject)=>{const start=performance.now();function check(){const dialogs=[...document.querySelectorAll('dialog[open]')].map(dialog=>dialog.textContent);if(dialogs.length||performance.now()-start>20000)return reject(Error('Recovery did not settle: '+JSON.stringify({dialogs,gpu:document.body.dataset.gpu,busy:layerApp.documents.busy(),park:layerApp.app.document_park_ready(),status:document.querySelector('#status').textContent})));setTimeout(check,50);}check();})])`);
    await waitFor('!layerApp.documents.busy() && layerApp.app.document_park_ready()');
    if (mode === "pending") assert.equal(await evaluate("layerApp.state().brush.diameter"), 37);
    assert.equal(await evaluate("layerApp.state().layers.length"), ["pending","device-failure"].includes(mode) ? layerCount+1 : durableLayerCount);
    assert.equal(await evaluate("document.querySelector('#gpu-notice').hidden"), true);
    await action({ type: "set_theme", theme: "light" });
    await action({ type: "invoke", command: "brush" });
    await action({ type: "invoke", command: "fit_canvas" });
    const before = await canvasPixels();
    const point = await evaluate("(()=>{const c=layerApp.state().camera,r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]};})()");
    await call("Input.dispatchMouseEvent", { type: "mousePressed", x: point.x-50, y: point.y, button: "left", buttons: 1, clickCount: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x+50, y: point.y, button: "left", buttons: 1 });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", x: point.x+50, y: point.y, button: "left", buttons: 0, clickCount: 1 });
    await settle();
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled");
    const after = await canvasPixels();
    assert.ok(after.white < before.white - 50, `GPU drawing works after attachment/reload: ${JSON.stringify({ before, after })}`);
    await action({type:'invoke',command:'undo'});
    await waitFor("layerApp.state().commands.find(command=>command.id==='redo').enabled");
    await evaluate('layerApp.documents.autosave()');
    console.log(`GPU startup: ${mode}, usable UI, no invisible painting and ${mode === "pending" ? "preserved session" : "reload recovery"} passed`);
  }
}

export async function checkGpuFailureLifecycle({call,evaluate,settle,errors}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${condition})resolve();else if(performance.now()>end)reject(Error('GPU failure lifecycle timeout: '+${JSON.stringify(condition)}+' '+document.querySelector('#gpu-notice')?.textContent));else setTimeout(check,30)}check()})`);
  const click=async selector=>{
    for(const type of ['mousePressed','mouseReleased']){
      const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
      await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    }
    await settle();
  };
  await evaluate(`(()=>{
    window.gpuFailureTest={devices:[],copied:null,originals:{},clipboardWrite:navigator.clipboard.writeText};
    for(const method of ['createCommandEncoder','createComputePipelineAsync','createRenderPipelineAsync']){
      const original=gpuFailureTest.originals[method]=GPUDevice.prototype[method];
      GPUDevice.prototype[method]=function(...args){if(method==='createCommandEncoder')gpuFailureTest.current=this;if(!gpuFailureTest.devices.includes(this))gpuFailureTest.devices.push(this);return original.apply(this,args)};
    }
    navigator.clipboard.writeText=async text=>{gpuFailureTest.copied=text};
  })()`);
  try {
    await evaluate('layerApp.restartGpu()');
    await wait('window.layerApp?.app.brush_ready()&&gpuFailureTest.current&&document.body.dataset.gpu==="ready"');
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});gpuFailureTest.old=gpuFailureTest.current;gpuFailureTest.old.dispatchEvent(new GPUUncapturedErrorEvent('uncapturederror',{error:new GPUOutOfMemoryError('capy-test: current device memory failure')}))`);
      await wait(`document.body.dataset.gpu==='unavailable'&&!document.querySelector('#gpu-notice').hidden`);
      assert.equal(await evaluate('layerApp.app.gpu_ready()'),false,'Current device memory failure retires its canvas');
      const report=await evaluate(`JSON.parse(document.querySelector('#gpu-notice details pre').textContent)`);
      assert.match(report.error,/memory/i,'Failure details preserve the original GPU error');
      assert.ok(report.events.some(event=>event.kind==='out_of_memory'),'Failure details retain the actual uncaptured GPU event');
      await click('#gpu-notice details summary');
      assert.equal(await evaluate(`document.querySelector('#gpu-notice details').open`),true,'Failure details expand through the UI');
      await click('#gpu-notice details button');
      assert.deepEqual(await evaluate('JSON.parse(gpuFailureTest.copied)'),report,'Copy Failure Details publishes the complete report');
      const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/ui/gpu-failure';await mkdir(directory,{recursive:true});
      const screenshot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${theme}.png`,Buffer.from(screenshot.data,'base64'));
      await click('#gpu-notice > button');
      await wait(`document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready()&&gpuFailureTest.current!==gpuFailureTest.old`);
      await evaluate(`gpuFailureTest.old.dispatchEvent(new GPUUncapturedErrorEvent('uncapturederror',{error:new GPUOutOfMemoryError('capy-test: retired device memory failure')}))`);
      await new Promise(resolve=>setTimeout(resolve,1600));
      assert.equal(await evaluate(`document.body.dataset.gpu`),'ready','A retired device event cannot stop its replacement');
      assert.equal(await evaluate('layerApp.app.gpu_ready()&&layerApp.app.brush_ready()'),true,'Replacement canvas remains ready after the failure poll');
      assert.equal(await evaluate(`document.querySelector('#gpu-notice').hidden`),true,'Retired events leave the recovered canvas visible');
      for(const error of errors.splice(0))assert.match(error,/memory/i,'Only injected memory failures are reported');
      console.log(`GPU failure lifecycle ${theme}: original error, expanded/copyable details, UI restart and retired-device isolation passed`);
    }
  } finally {await evaluate(`for(const [method,original] of Object.entries(gpuFailureTest.originals))GPUDevice.prototype[method]=original;navigator.clipboard.writeText=gpuFailureTest.clipboardWrite`);}
}
