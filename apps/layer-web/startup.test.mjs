import assert from "node:assert/strict";

// Delay real WebGPU validation promises at the browser boundary. The renderer,
// Wasm session, UI and actual GPU rendering continue to run unchanged.
export async function checkStagedStartup({ call, evaluate, settle, canvasPixels }) {
  const waitFor = async (condition, timeout = 25000) => { for(let attempt=0;;attempt++){try{return await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){if(${condition})resolve();
    else if(performance.now()-start>${timeout})reject(Error('Staged startup timed out: '+JSON.stringify({test:window.startupTest,times:window.layerApp?.startupTimes,notice:document.querySelector('#gpu-notice')?.textContent})));
    else setTimeout(check,25)}check();})`);}catch(error){if(attempt>=3 || !/navigated|context.*destroyed|Cannot find context/i.test(String(error)))throw error;}}};
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
    const p=window.startupTest={pipelines:[],held:null,required:false,optional:false};
    const scopes=[];
    const push=GPUDevice.prototype.pushErrorScope,pop=GPUDevice.prototype.popErrorScope;
    GPUDevice.prototype.pushErrorScope=function(filter){scopes.push({filter});return push.call(this,filter)};
    GPUDevice.prototype.popErrorScope=function(){
      const scope=scopes.pop(),result=pop.call(this);
      if(!scope?.hold)return result;
      return result.then(error=>new Promise(resolve=>{
        p.held=scope.hold;p.release=()=>{p.held=null;resolve(error)};
      }));
    };
    for(const name of ['createRenderPipeline','createComputePipeline','createRenderPipelineAsync','createComputePipelineAsync']){
      const original=GPUDevice.prototype[name];
      GPUDevice.prototype[name]=function(descriptor){
        const times=window.layerApp?.startupTimes;
        p.pipelines.push({method:name,label:descriptor.label,time:performance.now(),canvas:times?.canvas,brush:times?.brush});
        let hold;
        if(descriptor.label==='layer destination brush color'&&times?.brush==null&&!p.required){p.required=true;hold='required'}
        else if(descriptor.label==='layer mask paint'&&times?.brush!=null&&!p.optional){p.optional=true;hold='optional'}
        if(hold==='required'&&name.endsWith('Async')) return original.call(this,descriptor).then(pipeline=>new Promise(resolve=>{p.held=hold;p.release=()=>{p.held=null;resolve(pipeline)}}));
        if(hold){const scope=scopes.findLast(s=>s.filter==='validation');if(scope)scope.hold=hold}
        return original.call(this,descriptor);
      };
    }
  ` });
  try {
    await activateForReload(call);
    await call("Page.reload", { ignoreCache: true });
    await waitFor("window.startupTest?.held === 'required'");
    assert.equal(await evaluate("layerApp.app.canvas_presented()"), true);
    assert.equal(await evaluate("layerApp.app.brush_ready()"), false,
      "An unresolved required pipeline promise must gate painting");
    const blank = await canvasPixels();
    assert.ok(blank.white > blank.total * 0.1, "Paper is visible before the brush compiles");
    await waitFor("layerApp.state().filter_load.pending");
    await waitFor("JSON.parse(layerApp.app.workspace_view())?.ready");
    assert.equal(await evaluate("layerApp.app.startup_progress()[2]"), false,
      "Workspace adoption must not wait for the startup filter library");
    for (const pointer of ["mouse", "touch", "pen"]) {
      const point = await evaluate(`(()=>{const b=document.querySelector('[data-command="settings"]').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
      if (pointer === "touch") {
        await call("Input.dispatchTouchEvent", {type:"touchStart",touchPoints:[{id:1,...point}]});
        await new Promise(resolve => setTimeout(resolve, 70));
        await call("Input.dispatchTouchEvent", {type:"touchEnd",touchPoints:[]});
      } else {
        for (const type of ["mousePressed", "mouseReleased"]) {
          await call("Input.dispatchMouseEvent", {type,...point,button:"left",buttons:type==="mousePressed"?1:0,clickCount:1,pointerType:pointer});
        }
      }
      await waitFor("document.querySelector('#settings').open");
      await evaluate("layerApp.dispatch({type:'close_settings'})");
      await settle();
    }
    await evaluate(`
      window.earlyContact = {type:'pointer',id:999n,kind:'pen',button:'primary',position:[650,450]};
      layerApp.app.input({...earlyContact,phase:'down'}); startupTest.release()`);
    await waitFor("window.startupTest?.held === 'optional'");
    assert.equal(await evaluate("layerApp.app.brush_ready()"), true);
    assert.equal(await evaluate("layerApp.app.startup_progress()[2]"), false);
    assert.equal(await evaluate("layerApp.app.input({...earlyContact,phase:'move'}).paint"), false,
      "A contact started before readiness must not start painting halfway through");
    await evaluate("layerApp.app.input({...earlyContact,phase:'up'}); undefined");
    const before = await canvasPixels();
    for (const [type, x, buttons] of [["mousePressed",650,1],["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", { type, x, y:450, button:"left", buttons, clickCount:1 });
    }
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50,
      "The current brush paints while an optional shader is still compiling");
    const panBefore = await evaluate("layerApp.state().camera.translation");
    await call("Input.dispatchMouseEvent", { type:"mouseWheel", x:650,y:450,deltaX:0,deltaY:40 });
    await settle();
    assert.notDeepEqual(await evaluate("layerApp.state().camera.translation"), panBefore,
      "Camera input works while optional compilation is pending");
    await evaluate("startupTest.release()");
    await waitFor("window.layerApp?.startupTimes.complete != null", 55000);
    const result = await evaluate("({times:layerApp.startupTimes,pipelines:startupTest.pipelines})");
    assert.ok(result.times.canvas < result.times.brush && result.times.brush < result.times.complete);
    const early = result.pipelines.filter(p => p.time < result.times.canvas).map(p => p.label);
    assert.ok(!early.some(label => /brush|watercolor|export/i.test(label)),
      `Only general compositing is compiled before paper: ${early}`);
    assert.equal(early.length, 4, `Only the four paper/presentation pipelines precede canvas: ${early}`);
    assert.ok(result.pipelines.filter(p => p.time > result.times.canvas).some(p => p.method === 'createComputePipelineAsync' && p.label === 'native SDR tile writeback'));
    assert.ok(result.pipelines.filter(p => /brush|pointwise effect/.test(p.label)).every(p => p.method === 'createRenderPipelineAsync'), 'Startup brushes and effects use real async pipeline creation');
    console.log("Staged startup: visible paper before brush, GPU validation gates readiness, painting and camera input during optional compilation passed", result.times);
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.startupTest?.release?.()");
  }
  await checkLoadedDocument({ call, evaluate, waitFor });
  await checkCompilationFailure({ call, evaluate, waitFor });
  await checkFilterRejection({ call, evaluate, waitFor });
}

async function checkLoadedDocument({ call, evaluate, waitFor }) {
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
    window.documentStartupTest={held:false};
    const adapter=navigator.gpu.requestAdapter.bind(navigator.gpu);
    navigator.gpu.requestAdapter=async(...args)=>{
      // Workspace ownership starts before GPU acquisition. Its read-only gate
      // must finish before this fixture can insert a document filter.
      await new Promise((resolve,reject)=>{const start=performance.now();function check(){
        if(JSON.parse(layerApp.app.workspace_view())?.ready)resolve();
        else if(performance.now()-start>10000)reject(Error('Workspace fixture timed out'));
        else setTimeout(check,25);
      }check();});
      // The document model is populated before GPU attachment, as on restore.
      layerApp.dispatch({type:'effect',action:{op:'insert',effect:'domain_warp'}});
      return adapter(...args);
    };
    const create=GPUDevice.prototype.createRenderPipelineAsync,pop=GPUDevice.prototype.popErrorScope;
    let hold=false,selected=false;
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      if(!selected&&descriptor.label==='pointwise effect chain') {selected=true;hold=true}
      return create.call(this,descriptor);
    };
    GPUDevice.prototype.popErrorScope=function(){
      const result=pop.call(this);if(!hold)return result;hold=false;
      return result.then(error=>new Promise(resolve=>{
        documentStartupTest.held=true;documentStartupTest.release=()=>resolve(error);
      }));
    };
  ` });
  try {
    await activateForReload(call);
    await call("Page.reload", { ignoreCache:true });
    await waitFor("window.documentStartupTest?.held");
    assert.equal(await evaluate("layerApp.app.canvas_presented()"), true);
    assert.deepEqual(await evaluate("Array.from(layerApp.app.startup_progress()).slice(0,2)"), [false,false],
      "Loaded document filters validate before the document and current brush become ready");
    await evaluate("documentStartupTest.release()");
    await waitFor("window.layerApp?.app.brush_ready()");
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    await waitFor("(()=>{const v=JSON.parse(layerApp.app.workspace_view());return v.ready&&!v.busy&&!v.dirty&&!v.switcher_busy;})()");
    console.log("Staged startup: loaded domain-warp filter prepared before current brush passed");
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.documentStartupTest?.release?.()");
  }
}

async function checkCompilationFailure({call,evaluate,waitFor}) {
  const {identifier}=await call('Page.addScriptToEvaluateOnNewDocument',{source:`
    window.compilationFailureTest={rejected:false};
    const create=GPUDevice.prototype.createComputePipelineAsync;
    GPUDevice.prototype.createComputePipelineAsync=function(descriptor){
      if(descriptor.label==='native SDR tile writeback'&&!compilationFailureTest.rejected){
        compilationFailureTest.rejected=true;
        return Promise.reject(new GPUPipelineError('injected startup compiler failure',{reason:'validation'}));
      }
      return create.call(this,descriptor);
    };
  `});
  try {
    await activateForReload(call);
    await call('Page.reload',{ignoreCache:true});
    await waitFor("window.compilationFailureTest?.rejected && document.body.dataset.gpu==='unavailable'");
    assert.match(await evaluate("document.querySelector('#gpu-notice').textContent"),/native SDR tile writeback.*injected startup compiler failure/s);
    assert.equal(await evaluate('layerApp.startupTimes.brush'),null);
    assert.equal(await evaluate('layerApp.startupTimes.complete'),null);
    // The failed candidate never publishes a pipeline or starts a stroke. GPU
    // replacement owns a fresh queue and may recover without a page reload.
    await evaluate('layerApp.restartGpu()');
    await waitFor('window.layerApp?.startupTimes.complete != null', 55000);
    assert.equal(await evaluate('layerApp.app.brush_ready()'),true);
    console.log('Staged startup: async compute rejection gates input, reports its label, and canvas restart recovers');
  } finally {
    await call('Page.removeScriptToEvaluateOnNewDocument',{identifier});
  }
}

async function checkFilterRejection({evaluate,waitFor}) {
  const revision=await evaluate('String(layerApp.state().filter_catalog_revision)');
  await evaluate(`(async()=>{
    const app=layerApp.app;
    const manifest=await (await fetch('./filters/manifest.json')).json();
    // A fresh program identity forces GPU validation rather than cache reuse.
    manifest.filters[0].program.label+=' async rejection fixture';
    const text=JSON.stringify(manifest),names=app.filter_package_modules(text);
    const modules=Object.fromEntries(await Promise.all(names.map(async name=>[name,await(await fetch('./filters/'+name)).text()])));
    window.filterRejectionTest={rejected:false,text,modules};
    const original=GPUDevice.prototype.createRenderPipelineAsync;
    filterRejectionTest.restore=()=>{GPUDevice.prototype.createRenderPipelineAsync=original;};
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      if(descriptor.label==='pointwise effect chain'&&!filterRejectionTest.rejected){
        filterRejectionTest.rejected=true;
        return Promise.reject(new GPUPipelineError('injected filter compiler failure',{reason:'validation'}));
      }
      return original.call(this,descriptor);
    };
    app.load_filter_package(text,modules,'replace');layerApp.wake();
  })()`);
  try {
    await waitFor('window.filterRejectionTest?.rejected && !layerApp.state().filter_load.pending');
    assert.match(await evaluate('layerApp.state().filter_load.error'),/pointwise effect chain.*injected filter compiler failure/s);
    assert.equal(await evaluate('String(layerApp.state().filter_catalog_revision)'),revision,'Rejected async pipelines never publish a replacement catalog');
    assert.equal(await evaluate('layerApp.app.brush_ready()'),true,'Rejected filters leave the working canvas available');
    await evaluate("filterRejectionTest.restore();layerApp.app.load_filter_package(filterRejectionTest.text,filterRejectionTest.modules,'replace');layerApp.wake()");
    await waitFor('!layerApp.state().filter_load.pending');
    assert.equal(await evaluate('layerApp.state().filter_load.error ?? null'),null,'A clean retry must not reuse a failed compilation');
    console.log('Staged startup: async render rejection preserves the working catalog and clean retry succeeds');
  } finally { await evaluate('filterRejectionTest.restore()'); }
}

async function activateForReload(call) {
  // Reloading an untouched fixture can race a workspace lease/switcher task.
  // Give Chrome a trusted contact so its ordinary beforeunload prompt can be
  // handled by the harness, rather than emitting a blocked-prompt diagnostic.
  for (const type of ['mousePressed','mouseReleased']) {
    await call('Input.dispatchMouseEvent',{type,x:1,y:1,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  }
}
