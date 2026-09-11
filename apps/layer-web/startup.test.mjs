import assert from "node:assert/strict";

// Delay real WebGPU validation promises at the browser boundary. The renderer,
// Wasm session, UI and actual GPU rendering continue to run unchanged.
export async function checkStagedStartup({ call, evaluate, settle, canvasPixels }) {
  const waitFor = async condition => { for(let attempt=0;;attempt++){try{return await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){if(${condition})resolve();
    else if(performance.now()-start>25000)reject(Error('Staged startup timed out: '+JSON.stringify(window.startupTest)));
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
    for(const name of ['createRenderPipeline','createComputePipeline']){
      const original=GPUDevice.prototype[name];
      GPUDevice.prototype[name]=function(descriptor){
        const times=window.layerApp?.startupTimes;
        p.pipelines.push({label:descriptor.label,time:performance.now(),canvas:times?.canvas,brush:times?.brush});
        let hold;
        if(descriptor.label==='layer analytic paint'&&!p.required){p.required=true;hold='required'}
        else if(descriptor.label==='layer mask paint'&&times?.brush!=null&&!p.optional){p.optional=true;hold='optional'}
        if(hold){const scope=scopes.findLast(s=>s.filter==='validation');if(scope)scope.hold=hold}
        return original.call(this,descriptor);
      };
    }
  ` });
  try {
    await call("Page.reload", { ignoreCache: true });
    await waitFor("window.startupTest?.held === 'required'");
    assert.equal(await evaluate("layerApp.app.canvas_presented()"), true);
    assert.equal(await evaluate("layerApp.app.brush_ready()"), false,
      "An unresolved required GPU validation must gate painting");
    const blank = await canvasPixels();
    assert.ok(blank.white > blank.total * 0.1, "Paper is visible before the brush compiles");
    await evaluate("layerApp.dispatch({type:'open_settings',page:'appearance'})");
    assert.ok(await evaluate("document.querySelector('#settings').open"), "Settings work during compilation");
    await evaluate(`layerApp.dispatch({type:'close_settings'});
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
    await waitFor("window.layerApp?.startupTimes.complete !== null");
    const result = await evaluate("({times:layerApp.startupTimes,pipelines:startupTest.pipelines})");
    assert.ok(result.times.canvas < result.times.brush && result.times.brush < result.times.complete);
    const early = result.pipelines.filter(p => p.time < result.times.canvas).map(p => p.label);
    assert.ok(!early.some(label => /brush|watercolor|export/i.test(label)),
      `Only general compositing is compiled before paper: ${early}`);
    console.log("Staged startup: visible paper before brush, GPU validation gates readiness, painting and camera input during optional compilation passed", result.times);
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.startupTest?.release?.()");
  }
  await checkLoadedDocument({ call, evaluate, waitFor });
}

async function checkLoadedDocument({ call, evaluate, waitFor }) {
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
    window.documentStartupTest={held:false};
    const adapter=navigator.gpu.requestAdapter.bind(navigator.gpu);
    navigator.gpu.requestAdapter=(...args)=>{
      // The document model is populated before GPU attachment, as on restore.
      layerApp.dispatch({type:'effect',action:{op:'insert',effect:'domain_warp'}});
      return adapter(...args);
    };
    const create=GPUDevice.prototype.createRenderPipeline,pop=GPUDevice.prototype.popErrorScope;
    let hold=false,selected=false;
    GPUDevice.prototype.createRenderPipeline=function(descriptor){
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
    await call("Page.reload", { ignoreCache:true });
    await waitFor("window.documentStartupTest?.held");
    assert.equal(await evaluate("layerApp.app.canvas_presented()"), true);
    assert.deepEqual(await evaluate("Array.from(layerApp.app.startup_progress()).slice(0,2)"), [false,false],
      "Loaded document filters validate before the document and current brush become ready");
    await evaluate("documentStartupTest.release()");
    await waitFor("window.layerApp?.app.brush_ready()");
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    console.log("Staged startup: loaded domain-warp filter prepared before current brush passed");
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.documentStartupTest?.release?.()");
  }
}
