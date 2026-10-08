import assert from "node:assert/strict";

// Delay real WebGPU validation promises at the browser boundary. The renderer,
// Wasm session, UI and actual GPU rendering continue to run unchanged.
export async function checkStagedStartup({ call, evaluate, settle, canvasPixels }) {
  const waitFor = async (condition, timeout = 25000) => {
    const deadline = Date.now() + timeout;
    while (Date.now() < deadline) {
      try { if (await (typeof condition === 'function' ? condition() : evaluate(`Boolean(${condition})`))) return; }
      catch (error) { if (!/navigated|context.*destroyed|Cannot find context/i.test(String(error))) throw error; }
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    throw Error('Staged startup timed out: ' + await evaluate("JSON.stringify({test:window.startupTest,documentTest:window.documentStartupTest,times:window.layerApp?.startupTimes,dialogs:[...document.querySelectorAll('dialog[open],#header details[open]')].map(node=>node.textContent.slice(0,350)),notice:document.querySelector('#gpu-notice')?.textContent})"));
  };
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
    const p=window.startupTest={pipelines:[],held:null,required:false,requiredReleased:false,optional:false};
    const requiredResolvers=[];
    for(const name of ['createRenderPipeline','createComputePipeline','createRenderPipelineAsync','createComputePipelineAsync']){
      const original=GPUDevice.prototype[name];
      GPUDevice.prototype[name]=function(descriptor){
        const times=window.layerApp?.startupTimes;
        p.pipelines.push({method:name,label:descriptor.label,time:performance.now(),canvas:times?.canvas,brush:times?.brush});
        let hold;
        if(descriptor.label==='layer destination brush color'&&times?.brush==null&&!p.requiredReleased){p.required=true;hold='required'}
        if(times?.brush!=null&&!p.optional&&name.endsWith('Async')){p.optional=true;hold='optional'}
        if(hold&&name.endsWith('Async')) return original.call(this,descriptor).then(pipeline=>{
          if(hold==='required'&&p.requiredReleased)return pipeline;
          return new Promise(resolve=>{
            p.held=hold;
            if(hold==='required'){
              requiredResolvers.push(()=>resolve(pipeline));
              p.release=()=>{p.requiredReleased=true;p.held=null;for(const release of requiredResolvers.splice(0))release()};
            }else p.release=()=>{p.held=null;resolve(pipeline)};
          });
        });
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
    assert.equal(await evaluate("layerApp.state().filter_load.pending"), false, "Bundled filters need no startup transaction");
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
    const heldPan = await evaluate("layerApp.state().camera.translation");
    const panScale = await evaluate("layerApp.canvas.width/layerApp.canvas.getBoundingClientRect().width");
    await call("Input.dispatchKeyEvent", { type:"keyDown", key:" ", code:"Space", windowsVirtualKeyCode:32 });
    for (const [type, x, buttons] of [["mousePressed",600,1],["mouseMoved",660,1],["mouseReleased",660,0]]) {
      await call("Input.dispatchMouseEvent", { type, x, y:450, button:"left", buttons, clickCount:1 });
    }
    await call("Input.dispatchKeyEvent", { type:"keyUp", key:" ", code:"Space", windowsVirtualKeyCode:32 });
    await settle();
    assert.equal(await evaluate("layerApp.app.brush_ready()"), false);
    assert.deepEqual((await evaluate("layerApp.state().camera.translation")).map((v, i) => Math.round(v - heldPan[i])), [Math.round(60 * panScale), 0],
      "Space-drag pans the camera while the brush pipeline is compiling");
    const earlyBlank = await canvasPixels();
    for (const [type,x] of [["mousePressed",650],["mouseMoved",750]]) {
      await call("Input.dispatchMouseEvent", {type,x,y:450,button:"left",buttons:1,clickCount:1,pointerType:"pen",force:0.7});
    }
    await settle();
    assert.equal(await evaluate("layerApp.app.brush_ready()"), false);
    assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='undo').enabled"), false);
    assert.ok((await canvasPixels()).white >= earlyBlank.white - 50,
      "A contact held before brush readiness deposits no ink");
    await evaluate("startupTest.release()");
    await waitFor("layerApp.app.brush_ready()");
    assert.equal(await evaluate("layerApp.app.brush_ready()"), true);
    for (const [type,x,buttons] of [["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", {type,x,y:450,button:"left",buttons,clickCount:1,pointerType:"pen",force:buttons?0.7:0});
    }
    await settle();
    assert.ok((await canvasPixels()).white < earlyBlank.white - 50,
      "Brush readiness replays the held contact as a complete stroke");
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled", 5000);
    await evaluate("layerApp.dispatch({type:'invoke',command:'undo'})");
    await settle();
    assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='undo').enabled"), false);
    let restored;
    await waitFor(async () => {
      restored = await canvasPixels();
      return restored.white >= earlyBlank.white - 50;
    }).catch(error => { throw Error(`${error.message}; One Undo must restore presented pixels: ${JSON.stringify({earlyBlank,restored})}`); });
    assert.ok(restored.white >= earlyBlank.white - 50,
      `One Undo removes the whole contact, including samples received before readiness: ${JSON.stringify({earlyBlank,restored})}`);
    const before = await canvasPixels();
    for (const [type, x, buttons] of [["mousePressed",650,1],["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", { type, x, y:450, button:"left", buttons, clickCount:1 });
    }
    await settle();
    assert.ok((await canvasPixels()).white < before.white - 50,
      "The current brush paints without warming unused shaders");
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled", 5000);
    const panBefore = await evaluate("layerApp.state().camera.translation");
    await call("Input.dispatchMouseEvent", { type:"mouseWheel", x:650,y:450,deltaX:0,deltaY:40 });
    await settle();
    assert.notDeepEqual(await evaluate("layerApp.state().camera.translation"), panBefore,
      "Camera input works after required compilation");
    await waitFor("startupTest.held === 'optional'");
    await call("Input.dispatchMouseEvent", {type:"mousePressed",x:650,y:450,button:"left",buttons:1,clickCount:1});
    const pausedCount = await evaluate("startupTest.pipelines.length");
    await evaluate("startupTest.release()");
    await new Promise(resolve => setTimeout(resolve, 350));
    assert.equal(await evaluate("startupTest.pipelines.length"), pausedCount,
      "Finishing an optional driver call during a held contact must not start another");
    assert.equal(await evaluate("layerApp.app.startup_progress()[2]"), false);
    for (const [type,buttons] of [["mouseMoved",1],["mouseReleased",0]]) {
      await call("Input.dispatchMouseEvent", {type,x:800,y:450,button:"left",buttons,clickCount:1});
    }
    await waitFor("window.layerApp?.startupTimes.complete != null", 120000);
    const result = await evaluate("({times:layerApp.startupTimes,pipelines:startupTest.pipelines})");
    assert.ok(result.times.canvas < result.times.brush && result.times.brush <= result.times.complete);
    const early = result.pipelines.filter(p => p.time < result.times.canvas).map(p => p.label);
    assert.ok(!early.some(label => /brush|watercolor|export/i.test(label)),
      `Only general compositing is compiled before paper: ${early}`);
    const canvasRecipes = new Set(["backdrop blur down", "backdrop blur regions", "backdrop blur up", "backdrop glass interiors", "compose display layers", "display-only cursor", "reduce adjacent display level", "viewport presentation"]);
    assert.ok(early.every(label => canvasRecipes.has(label)),
      `Only paper, presentation and default panel glass pipelines precede canvas: ${early}`);
    assert.ok(result.pipelines.filter(p => p.time > result.times.canvas).some(p => p.method === 'createComputePipelineAsync' && p.label === 'native SDR tile writeback'));
    assert.ok(result.pipelines.filter(p => /brush|pointwise effect/.test(p.label)).every(p => p.method.endsWith('PipelineAsync')), 'Startup brushes and effects use real async pipeline creation');
    assert.ok(!result.pipelines.some(p => /layer mask paint/.test(p.label)),
      'Unused mask brushes retain their recipes');
    assert.ok(result.pipelines.some(p => /watercolor capillary relaxation/.test(p.label) && p.time > result.times.brush),
      'Unused brush families warm only after the selected brush is ready');
    const transform = result.pipelines.find(p => p.label === 'transform pixels');
    const watercolor = result.pipelines.find(p => /watercolor capillary relaxation/.test(p.label));
    assert.ok(transform && watercolor && transform.time < watercolor.time,
      'Transform preparation starts before specialty watercolor preparation');
    const warmedCount = result.pipelines.length;
    for (const id of [2,5,4,7,9,10,11,20,21,35,12,40,41,42,1]) {
      await evaluate(`layerApp.dispatch({type:'select_brush',id:${id}})`);
      await waitFor("layerApp.app.brush_ready()");
    }
    assert.equal(await evaluate("startupTest.pipelines.length"), warmedCount,
      "Tool selection reuses warmed shaders, including retouching after deselection");
    console.log("Staged startup: selected brush before idle warmup, input pauses compilation, and warmed tools reuse pipelines", result.times);
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.startupTest?.release?.()");
  }
  await checkFirstUse({ call, evaluate, waitFor });
  await checkLoadedDocument({ call, evaluate, waitFor });
  await checkCompilationFailure({ call, evaluate, waitFor, settle, canvasPixels });
}

// Cold first use must take the same asynchronous path even after startup has
// completed. A repeat insertion must reuse compilation rather than warm again.
async function checkFirstUse({call,evaluate,waitFor}) {
  await evaluate(`(()=>{
    const original=GPUDevice.prototype.createRenderPipelineAsync;
    window.firstUse={calls:0,held:false,restore(){GPUDevice.prototype.createRenderPipelineAsync=original;}};
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      const promise=original.call(this,descriptor);
      if(descriptor.label!=='pointwise effect chain')return promise;
      firstUse.calls++;
      if(firstUse.calls!==1)return promise;
      return promise.then(pipeline=>new Promise(resolve=>{firstUse.held=true;firstUse.release=()=>resolve(pipeline);}));
    };
    layerApp.dispatch({type:'effect',action:{op:'insert',effect:'domain_warp'}});
  })()`);
  try {
    await waitFor('firstUse.held');
    assert.equal(await evaluate('layerApp.app.brush_ready()'),false);
    assert.equal(await evaluate('layerApp.app.startup_progress()[0]'),false);
    await evaluate("document.querySelector('#header details summary').click()");
    assert.equal(await evaluate("!!document.querySelector('#header details[open] button')"),true,'Menus still open during first-use compilation');
    await evaluate("document.querySelector('#header details[open]').open=false;firstUse.release()");
    await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready()');
    const count=await evaluate('firstUse.calls');
    await evaluate("layerApp.dispatch({type:'invoke',command:'undo'})");
    await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready()');
    await evaluate("layerApp.dispatch({type:'effect',action:{op:'insert',effect:'domain_warp'}})");
    await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready()');
    assert.equal(await evaluate('firstUse.calls'),count,'Repeated effects reuse their pipelines');
    const nextContact=await evaluate(`(()=>{
      layerApp.dispatch({type:'invoke',command:'undo'});
      const event={type:'pointer',id:998n,kind:'pen',button:'primary',position:[650,450]};
      const paint=layerApp.app.input({...event,phase:'down'}).paint;
      layerApp.app.input({...event,phase:'up'});return paint;
    })()`);
    assert.equal(nextContact,true,'A cached tool accepts a contact before the next post-undo frame');
    await waitFor('layerApp.app.document_park_ready()');
    console.log('Demand effects: cold first use stays asynchronous, gates rendering, preserves menus and reuses compiled variants');
  } finally {await evaluate('firstUse.release?.();firstUse.restore()');}
}

async function checkLoadedDocument({ call, evaluate, waitFor }) {
  await evaluate("layerApp.dispatch({type:'effect',action:{op:'insert',effect:'domain_warp'}})");
  await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready()');
  await evaluate('layerApp.documents.autosave()');
  await waitFor("(()=>{const v=JSON.parse(layerApp.app.workspace_view());return v.ready&&!v.busy&&!v.dirty&&!v.switcher_busy;})()");
  const savedLayers = await evaluate('layerApp.state().layers.map(layer=>layer.label)');
  const { identifier } = await call("Page.addScriptToEvaluateOnNewDocument", { source: `
    window.documentStartupTest={held:false,released:false};
    const releases=[];
    documentStartupTest.release=()=>{documentStartupTest.released=true;for(const release of releases.splice(0))release()};
    const create=GPUDevice.prototype.createRenderPipelineAsync,pop=GPUDevice.prototype.popErrorScope;
    let hold=false;
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      if(!documentStartupTest.released&&descriptor.label==='pointwise effect chain')hold=true;
      return create.call(this,descriptor);
    };
    GPUDevice.prototype.popErrorScope=function(){
      const result=pop.call(this);if(!hold)return result;hold=false;
      return result.then(error=>{
        if(documentStartupTest.released)return error;
        return new Promise(resolve=>{documentStartupTest.held=true;releases.push(()=>resolve(error));});
      });
    };
  ` });
  try {
    await activateForReload(call);
    await call("Page.reload", { ignoreCache:true });
    await waitFor("window.documentStartupTest?.held");
    assert.equal(await evaluate("layerApp.app.canvas_presented()"), true);
    assert.notDeepEqual(await evaluate('layerApp.state().layers.map(layer=>layer.label)'), savedLayers,
      "Restored document layers remain private until their filters validate");
    await evaluate("documentStartupTest.release()");
    await waitFor("window.layerApp?.app.brush_ready() && layerApp.state().layers.length===3");
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    assert.deepEqual(await evaluate('layerApp.state().layers.map(layer=>layer.label)'), savedLayers);
    await waitFor("(()=>{const v=JSON.parse(layerApp.app.workspace_view());return v.ready&&!v.busy&&!v.dirty&&!v.switcher_busy;})()");
    console.log("Staged startup: loaded domain-warp filter prepared before current brush passed");
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.documentStartupTest?.release?.()");
  }
}

async function checkCompilationFailure({call,evaluate,waitFor,settle,canvasPixels}) {
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
    await waitFor('window.layerApp?.app.brush_ready()', 55000);
    assert.equal(await evaluate('layerApp.app.brush_ready()'),true);
    const dismissRecovery = async () => {
      const recovery = await evaluate("(()=>{const dialog=document.querySelector('dialog[open]');if(!dialog)return null;if(!dialog.textContent.includes('Recovery storage needs attention'))throw Error(dialog.textContent);const button=[...dialog.querySelectorAll('button')].find(button=>button.textContent==='Later');const bounds=button.getBoundingClientRect();return{x:bounds.x+bounds.width/2,y:bounds.y+bounds.height/2};})()");
      if (recovery) {
        for (const type of ['mousePressed','mouseReleased']) {
          await call('Input.dispatchMouseEvent',{type,...recovery,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
        }
        await waitFor("!document.querySelector('dialog[open]')");
      }
    };
    await evaluate("window.startupRecovery={settled:false,error:null};layerApp.documents.startRecovery().then(()=>startupRecovery.settled=true,error=>{startupRecovery.error=String(error);startupRecovery.settled=true});undefined");
    const recoveryDeadline = Date.now() + 55000;
    while (!await evaluate('startupRecovery.settled')) {
      await waitFor("startupRecovery.settled || !!document.querySelector('dialog[open]')", Math.max(1,recoveryDeadline-Date.now()));
      await dismissRecovery();
    }
    assert.equal(await evaluate('startupRecovery.error'), null);
    await dismissRecovery();
    const animation = await evaluate("(()=>{const layer=layerApp.state().layers.find(layer=>layer.adjustment_effect);return layer?{layer:String(layer.id),moving:layerApp.app.canvas_work_pending(),complete:layerApp.app.startup_progress()[2]}:null;})()");
    if (animation) {
      assert.equal(animation.moving, true, 'The restored animated drawing continues producing canvas frames');
      assert.equal(animation.complete, false, 'Animation holds speculative catalogue preparation');
      await evaluate(`layerApp.dispatch({type:'effect',action:{op:'set',layer:BigInt(${JSON.stringify(animation.layer)}),key:'animate',value:{kind:'toggle',value:false}}})`);
    }
    await waitFor('layerApp.app.brush_ready()');
    await waitFor('!layerApp.app.canvas_work_pending()');
    await settle();
    const before = await canvasPixels();
    for (const [type,x,buttons] of [["mousePressed",650,1],["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", {type,x,y:550,button:"left",buttons,clickCount:1,pointerType:"pen",force:buttons?0.7:0});
    }
    await settle();
    const after = await canvasPixels();
    assert.ok(after.white < before.white - 50,
      `The recovered brush paints before optional catalog completion: ${JSON.stringify({before,after})}`);
    const deadline = Date.now() + 55000;
    while (!await evaluate('layerApp.startupTimes.complete != null')) {
      await waitFor("layerApp.startupTimes.complete != null || !!document.querySelector('dialog[open]')", Math.max(1,deadline-Date.now()));
      await dismissRecovery();
    }
    console.log('Staged startup: async compute rejection gates input, reports its label, and canvas restart recovers');
  } finally {
    await call('Page.removeScriptToEvaluateOnNewDocument',{identifier});
  }
}

async function activateForReload(call) {
  // Reloading an untouched fixture can race a workspace lease/switcher task.
  // Give Chrome a trusted contact so its ordinary beforeunload prompt can be
  // handled by the harness, rather than emitting a blocked-prompt diagnostic.
  for (const type of ['mousePressed','mouseReleased']) {
    await call('Input.dispatchMouseEvent',{type,x:1,y:1,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  }
}
