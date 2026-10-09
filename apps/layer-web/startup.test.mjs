import {restartStoreUrl} from './package-fixture.test.mjs';
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
    const p=window.startupTest={pipelines:[],held:null,required:false,requiredReleased:false};
    const requiredResolvers=[];
    for(const name of ['createRenderPipeline','createComputePipeline','createRenderPipelineAsync','createComputePipelineAsync']){
      const original=GPUDevice.prototype[name];
      GPUDevice.prototype[name]=function(descriptor){
        const times=window.layerApp?.startupTimes;
        p.pipelines.push({method:name,label:descriptor.label,time:performance.now(),canvas:times?.canvas,brush:times?.brush});
        let hold;
        if(descriptor.label==='layer destination brush color'&&times?.brush==null&&!p.requiredReleased){p.required=true;hold='required'}
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
    await checkpointBeforeReload(evaluate);
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
      await waitFor("document.querySelector('#settings').open").catch(async error=>{
        const target=await evaluate(`(()=>{const button=document.querySelector('[data-command="settings"]'),point=${JSON.stringify(point)};return JSON.parse(JSON.stringify({pointer:${JSON.stringify(pointer)},rect:button.getBoundingClientRect().toJSON(),disabled:button.disabled,host_error:layerApp.state().host_error,session:layerApp.app.gpu_diagnostics().session,hit:document.elementFromPoint(point.x,point.y)?.closest("[data-command]")?.dataset.command,open:layerApp.state().settings_open,gpu:document.body.dataset.gpu,status:document.querySelector('#status').textContent},(_,value)=>typeof value==='bigint'?String(value):value))})()`);
        throw Error(error.message+'; Settings contact: '+JSON.stringify(target));
      });
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
    const strokePixels=()=>canvasPixels({x:620,y:420,width:280,height:60,scale:1});
    const expectPainted=async(before,label)=>{
      let after;
      await waitFor(async()=>{after=await strokePixels();return after.white<before.white-50}).catch(async error=>{
        const state=await evaluate(`JSON.parse(JSON.stringify({pending:layerApp.app.canvas_work_pending(),park:layerApp.app.document_park_ready(),brush:layerApp.app.brush_ready(),undo:layerApp.state().commands.find(c=>c.id==='undo').enabled,session:layerApp.app.gpu_diagnostics().session},(_,value)=>typeof value==='bigint'?String(value):value))`);
        throw Error(label+': '+JSON.stringify({before,after,state})+'; '+error.message);
      });
    };
    const earlyBlank = await strokePixels();
    for (const [type,x] of [["mousePressed",650],["mouseMoved",750]]) {
      await call("Input.dispatchMouseEvent", {type,x,y:450,button:"left",buttons:1,clickCount:1,pointerType:"pen",force:0.7});
    }
    await settle();
    assert.equal(await evaluate("layerApp.app.brush_ready()"), false);
    assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='undo').enabled"), false);
    assert.ok((await strokePixels()).white >= earlyBlank.white - 50,
      "A contact held before brush readiness deposits no ink");
    await evaluate("startupTest.release()");
    await waitFor("layerApp.app.brush_ready()");
    assert.equal(await evaluate("layerApp.app.brush_ready()"), true);
    for (const [type,x,buttons] of [["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", {type,x,y:450,button:"left",buttons,clickCount:1,pointerType:"pen",force:buttons?0.7:0});
    }
    await settle();
    await expectPainted(earlyBlank,"Brush readiness replays the held contact as a complete stroke");
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled", 5000);
    await evaluate("layerApp.dispatch({type:'invoke',command:'undo'})");
    await settle();
    assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='undo').enabled"), false);
    let restored;
    await waitFor(async () => {
      restored = await strokePixels();
      return restored.white >= earlyBlank.white - 50;
    }).catch(error => { throw Error(`${error.message}; One Undo must restore presented pixels: ${JSON.stringify({earlyBlank,restored})}`); });
    assert.ok(restored.white >= earlyBlank.white - 50,
      `One Undo removes the whole contact, including samples received before readiness: ${JSON.stringify({earlyBlank,restored})}`);
    const before = await strokePixels();
    for (const [type, x, buttons] of [["mousePressed",650,1],["mouseMoved",850,1],["mouseReleased",850,0]]) {
      await call("Input.dispatchMouseEvent", { type, x, y:450, button:"left", buttons, clickCount:1 });
    }
    await settle();
    await expectPainted(before,"The current brush paints without warming unused shaders");
    await waitFor("layerApp.state().commands.find(c=>c.id==='undo').enabled", 5000);
    const panBefore = await evaluate("layerApp.state().camera.translation");
    await call("Input.dispatchMouseEvent", { type:"mouseWheel", x:650,y:450,deltaX:0,deltaY:40 });
    await settle();
    assert.notDeepEqual(await evaluate("layerApp.state().camera.translation"), panBefore,
      "Camera input works after required compilation");
    await waitFor("window.layerApp?.startupTimes.complete != null", 120000);
    const result = await evaluate("({times:layerApp.startupTimes,pipelines:startupTest.pipelines})");
    assert.ok(result.times.canvas < result.times.brush && result.times.brush <= result.times.complete);
    const early = result.pipelines.filter(p => p.time < result.times.canvas).map(p => p.label);
    assert.ok(!early.some(label => /brush|watercolor|export/i.test(label)),
      `Only general compositing is compiled before paper: ${early}`);
    const canvasRecipes = new Set(["backdrop blur down", "backdrop blur regions", "backdrop blur up", "backdrop glass interiors", "compose display layers", "display-only cursor", "reduce adjacent display level", "reduce paint to display", "reduce offset paint to display", "display area resample", "viewport presentation"]);
    assert.ok(early.every(label => canvasRecipes.has(label)),
      `Only paper, presentation and default panel glass pipelines precede canvas: ${early}`);
    assert.ok(result.pipelines.filter(p => p.time > result.times.canvas).some(p => p.method === 'createComputePipelineAsync' && p.label === 'native SDR tile writeback'));
    assert.ok(result.pipelines.filter(p => /brush|dry material |pointwise effect/.test(p.label)).every(p => p.method.endsWith('PipelineAsync')), 'Startup brushes and effects use real async pipeline creation');
    assert.ok(!result.pipelines.some(p => /layer mask paint/.test(p.label)),
      'Unused mask brushes retain their recipes');
    assert.ok(!result.pipelines.some(p => /watercolor capillary relaxation|transform pixels|transform mesh/.test(p.label)),
      'Unselected specialty tools and transforms stay cold after startup completes');
    const tools = [2,5,4,7,9,10,11,20,21,35,12,40,41,42,1];
    const toolPipelineCount=()=>evaluate("startupTest.pipelines.filter(p=>p.label!=='pointwise effect chain').length");
    const coldCount = result.pipelines.filter(p=>p.label!=='pointwise effect chain').length;
    const selectTools = async () => {
      for (const id of tools) {
        await evaluate(`layerApp.dispatch({type:'select_brush',id:${id}})`);
        await waitFor("layerApp.app.brush_ready() && !layerApp.app.shader_work_pending(false)");
      }
    };
    await selectTools();
    const demandedCount = await toolPipelineCount();
    assert.ok(demandedCount > coldCount, 'First selection compiles demanded recipes');
    await selectTools();
    assert.equal(await toolPipelineCount(), demandedCount,
      'Repeated demanded tools reuse ready pipelines');
    console.log("Staged startup: required input gates, cold unused tools and demanded reuse", result.times);
  } finally {
    await call("Page.removeScriptToEvaluateOnNewDocument", { identifier });
    await evaluate("window.startupTest?.release?.()");
  }
  await checkFirstUse({ call, evaluate, waitFor });
  await checkLoadedDocument({ call, evaluate, waitFor });
  await checkCompilationFailure({ call, evaluate, waitFor, settle, canvasPixels });
  await checkCompilationFailure({ call, evaluate, waitFor, settle, canvasPixels }, "dry");
  await checkRecoveryGpuLoss({ call, evaluate, waitFor, settle, canvasPixels });
  await checkRecoveryGpuLoss({ call, evaluate, waitFor, settle, canvasPixels }, true);
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
  await checkpointBeforeReload(evaluate);
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

async function checkCompilationFailure({call,evaluate,waitFor,settle,canvasPixels}, target="writeback") {
  await evaluate(`(()=>{const layer=layerApp.state().layers.find(layer=>layer.adjustment_effect);if(layer)layerApp.dispatch({type:'effect',action:{op:'set',layer:BigInt(layer.id),key:'animate',value:{kind:'toggle',value:true}}});})()`);
  const {identifier}=await call('Page.addScriptToEvaluateOnNewDocument',{source:`
    window.compilationFailureTest={rejected:false};
    const create=GPUDevice.prototype.createComputePipelineAsync;
    GPUDevice.prototype.createComputePipelineAsync=function(descriptor){
      if((${JSON.stringify(target)}==='dry'?descriptor.label?.startsWith('dry material '):descriptor.label==='native SDR tile writeback')&&!compilationFailureTest.rejected){
        compilationFailureTest.label=descriptor.label;
        compilationFailureTest.rejected=true;
        return Promise.reject(new GPUPipelineError('injected startup compiler failure',{reason:'validation'}));
      }
      return create.call(this,descriptor);
    };
  `});
  try {
    await checkpointBeforeReload(evaluate);
    await activateForReload(call);
    await call('Page.reload',{ignoreCache:true});
    await waitFor("window.compilationFailureTest?.rejected && document.body.dataset.gpu==='unavailable'");
    const notice = await evaluate("document.querySelector('#gpu-notice').textContent");
    const label = await evaluate("compilationFailureTest.label");
    assert.ok(notice.includes(label), `Exact failed variant remains visible: ${notice}`);
    assert.match(notice, /injected startup compiler failure/);
    if(target === "dry") assert.match(label, /dry material .* operation=\d+ contact=0x[0-9a-f]+/i);
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
      await evaluate(`layerApp.dispatch({type:'effect',action:{op:'set',layer:BigInt(${JSON.stringify(animation.layer)}),key:'animate',value:{kind:'toggle',value:false}}})`);
    }
    await waitFor('layerApp.app.brush_ready()');
    await waitFor('!layerApp.app.canvas_work_pending()');
    await settle();
    const before = await canvasPixels();
    const point=await evaluate(`(()=>{const c=layerApp.state().camera,r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*${target==='dry' ? .65 : .4})*r.height/c.viewport[1]};})()`);
    for (const [type,dx,buttons] of [["mousePressed",-75,1],["mouseMoved",75,1],["mouseReleased",75,0]]) {
      await call("Input.dispatchMouseEvent", {type,x:point.x+dx,y:point.y,button:"left",buttons,clickCount:1,pointerType:"pen",force:buttons?0.7:0});
    }
    await settle();
    const after = await canvasPixels();
    assert.ok(after.white < before.white - 50,
      `The recovered brush paints after demanded readiness: ${JSON.stringify({before,after})}`);
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

async function checkRecoveryGpuLoss({call,evaluate,waitFor,settle}, suspensionThrows=false) {
  const presentedPixels = async () => {
    const clip=await evaluate("(()=>{const c=layerApp.state().camera,r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0]-8,y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]-8,width:16,height:16,scale:1};})()");
    const shot=await call('Page.captureScreenshot',{format:'png',clip});
    return evaluate(`(async()=>{const image=new Image();image.src=${JSON.stringify('data:image/png;base64,'+shot.data)};await image.decode();const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return Array.from(context.getImageData(0,0,canvas.width,canvas.height).data);})()`);
  };
  const click = async expression => {
    const point = await evaluate(`(()=>{const button=${expression};if(!button||button.disabled)throw Error('Recovery control unavailable');const r=button.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;return{x,y,visible:r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight,hit:button.contains(document.elementFromPoint(x,y))};})()`);
    assert.equal(point.visible,true,'Recovery control remains within the viewport');
    assert.equal(point.hit,true,'Recovery control receives native pointer input');
    for(const type of ['mousePressed','mouseReleased']) {
      await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    }
  };
  await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready() && !layerApp.documents.busy()');
  await evaluate('layerApp.documents.startRecovery()');
  const count = await evaluate('layerApp.app.document_tabs(0).tabs.length');
  await evaluate("layerApp.dispatch({type:'invoke',command:'new_document'})");
  await waitFor("!![...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Create')");
  await evaluate("for(const input of document.querySelectorAll('dialog[open] input[type=number]'))input.value=96");
  await click("[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent==='Create')");
  await waitFor(`layerApp.app.document_tabs(0).tabs.length===${count+1} && layerApp.app.brush_ready() && layerApp.app.document_park_ready()`);
  for(const command of ['add_layer','select_all'])await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  await evaluate("layerApp.dispatch({type:'set_color',rgba:[.1,.3,.9,1]});layerApp.dispatch({type:'invoke',command:'fill_selection'});layerApp.dispatch({type:'invoke',command:'deselect'})");
  await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready() && !layerApp.app.canvas_work_pending()');
  await settle();
  const expectedPixels = await presentedPixels();
  assert.ok(expectedPixels.every((value,index)=>index%4!==0||expectedPixels[index+2]>expectedPixels[index+1]&&expectedPixels[index+1]>value&&expectedPixels[index+2]-value>100),'The drawing interior is blue before checkpointing');
  const expectedLayers = await evaluate("layerApp.state().layers.map(({label})=>label)");
  await checkpointBeforeReload(evaluate);
  const storeUrl = await restartStoreUrl(evaluate);
  const seed = await evaluate(`(async()=>{
    const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();
    const prior=[];
    for(const origin of await store.windows()) {
      const manifest=await store.manifest(origin),failed=new Set([...manifest.blocked,...manifest.restoring.map(ticket=>ticket.id)].map(String));
      for(const drawing of manifest.drawings.filter(drawing=>failed.has(String(drawing.id)))) {
        const record=await store.read(drawing.key),ids=[...new Set([...(record?.current?.resources??[]),...(record?.previous?.resources??[])])];
        prior.push({key:drawing.key,record:record??null,ids,bytes:(await store.resources(drawing.key,ids)).map(value=>Array.from(value))});
      }
    }
    const key='capy-test-gpu-loss-'+crypto.randomUUID(),origin='zz-capy-test-gpu-loss-'+crypto.randomUUID();
    const tabs=layerApp.app.document_tabs(0),identity=Math.max(...tabs.tabs.map(tab=>Number(tab.id)))+1024;
    const capture=layerApp.app.capture_tab_session(tabs.selected),generation=${suspensionThrows ? 2000003 : 1000003};
    try{await capture.write(key,BigInt(generation),0n,[],[]);}finally{capture.free();}
    await store.publishManifest(origin,{generation:1,drawings:[{id:identity,key}],active:identity,clean_exit:false,restoring:[],blocked:[]});
    const record=await store.read(key),ids=[...new Set([...(record.current?.resources??[]),...(record.previous?.resources??[])])];
    const bytes=(await store.resources(key,ids)).map(value=>Array.from(value));
    return{key,origin,identity,generation,record,ids,bytes,prior};
  })()`);
  const {identifier} = await call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{
    let current;
    const requestAdapter=navigator.gpu.requestAdapter.bind(navigator.gpu);let requests=0;
    navigator.gpu.requestAdapter=(...args)=>{requests++;return requestAdapter(...args)};
    Object.defineProperty(window,'layerApp',{configurable:true,get:()=>current,set:value=>{
      current=value;
      const app=value.app,prepare=app.prepare_session_restart.bind(app),failure=app.gpu_failure.bind(app),suspend=app.suspend_gpu.bind(app);
      const state=window.recoveryGpuLoss={attempts:0,adapterRequests:()=>requests,restore(){app.prepare_session_restart=prepare;app.gpu_failure=failure;app.suspend_gpu=suspend;navigator.gpu.requestAdapter=requestAdapter;},resumeFailure(){app.gpu_failure=failure;app.suspend_gpu=suspend;}};
      app.prepare_session_restart=(...args)=>{
        if(Number(args[0]?.generation)===${seed.generation}){
          state.attempts++;
          if(state.attempts===1)return Promise.reject(Error('capy-test: transient recovery open failure'));
        }
        return prepare(...args);
      };
    }});
  })()`});
  try {
    await activateForReload(call);
    const previous = await evaluate('performance.timeOrigin');
    await call('Page.reload',{ignoreCache:true});
    const recoveryDeadline=Date.now()+90000;
    let skipped=0;
    for(;;) {
      await waitFor(`performance.timeOrigin!==${previous} && window.recoveryGpuLoss && (recoveryGpuLoss.attempts===1 || !!document.querySelector('dialog[open]'))`,Math.max(1,recoveryDeadline-Date.now()));
      if(await evaluate('recoveryGpuLoss.attempts===1'))break;
      assert.ok(skipped++<seed.prior.length,'Only known interrupted fixture entries may be left for later');
      assert.match(await evaluate("document.querySelector('dialog[open]').textContent"),/Recovery storage needs attention/);
      await click("[...document.querySelectorAll('dialog[open] footer button')].find(button=>button.textContent==='Later')");
      await waitFor("!document.querySelector('dialog[open]')",Math.max(1,recoveryDeadline-Date.now()));
    }
    await waitFor("!!document.querySelector('dialog[open]')");
    assert.match(await evaluate("document.querySelector('dialog[open]').textContent"),/capy-test: transient recovery open failure/);
    assert.ok(await evaluate("[...document.querySelectorAll('dialog[open] footer button')].length===3 && [...document.querySelectorAll('dialog[open] footer button')].every(button=>!button.disabled)"));
    await evaluate("layerApp.app.gpu_failure=()=> 'capy-test: GPU stopped during recovery'");
    if(suspensionThrows)await evaluate("layerApp.app.suspend_gpu=()=>{throw Error('capy-test: secondary recovery suspension failure')}");
    await waitFor("document.body.dataset.gpu==='unavailable'");
    assert.match(await evaluate("document.querySelector('#gpu-notice').textContent"),/capy-test: GPU stopped during recovery/);
    assert.equal(await evaluate('layerApp.app.gpu_ready()'),suspensionThrows,
      'Failed suspension leaves the backend ready while the host has stopped');
    if(suspensionThrows)assert.match(await evaluate("document.querySelector('#gpu-notice').textContent"),/capy-test: GPU stopped during recovery.*capy-test: secondary recovery suspension failure/s);
    await click("document.querySelector('dialog[open] footer .suggested-action')");
    await waitFor("!document.querySelector('dialog[open]')");
    for(let poll=0;poll<10;poll++) {
      await new Promise(resolve=>setTimeout(resolve,25));
      assert.equal(await evaluate('recoveryGpuLoss.attempts'),1,'Retry waits for GPU readiness before reopening');
      assert.equal(await evaluate("!!document.querySelector('dialog[open]')"),false,'A second recovery modal must not obscure Restart');
    }
    const saved = await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();return{record:await store.read(${JSON.stringify(seed.key)}),bytes:(await store.resources(${JSON.stringify(seed.key)},${JSON.stringify(seed.ids)})).map(value=>Array.from(value))};})()`);
    assert.deepEqual(saved,{record:seed.record,bytes:seed.bytes},'GPU loss and Retry preserve the original failed copy exactly');
    if(suspensionThrows) {
      const requests=await evaluate('recoveryGpuLoss.adapterRequests()');
      await click("[...document.querySelectorAll('#gpu-notice button')].find(button=>button.textContent==='Restart canvas')");
      assert.equal(await evaluate('recoveryGpuLoss.adapterRequests()'),requests,'Failed cleanup must not request another GPU');
      assert.equal(await evaluate("document.body.dataset.gpu"),'unavailable');
      assert.match(await evaluate("document.querySelector('#gpu-notice').textContent"),/capy-test: secondary recovery suspension failure/);
      assert.doesNotMatch(await evaluate("document.querySelector('#gpu-notice').textContent"),/GPU is already attached/);
    }
    await evaluate('recoveryGpuLoss.resumeFailure()');
    await click("[...document.querySelectorAll('#gpu-notice button')].find(button=>button.textContent==='Restart canvas')");
    await waitFor("document.body.dataset.gpu==='ready' && recoveryGpuLoss.attempts===2",90000);
    await evaluate('layerApp.documents.startRecovery()');
    await waitFor(`layerApp.app.document_tabs(0).tabs.some(tab=>String(tab.id)===${JSON.stringify(String(seed.identity))}) && layerApp.app.document_park_ready()`,90000);
    await evaluate(`layerApp.documents.select(BigInt(${JSON.stringify(String(seed.identity))}))`);
    await waitFor('layerApp.app.brush_ready() && layerApp.app.document_park_ready() && !layerApp.app.canvas_work_pending()',90000);
    await settle();
    assert.deepEqual(await evaluate("layerApp.state().layers.map(({label})=>label)"),expectedLayers,'Successful Retry adopts the actual saved layer stack');
    assert.deepEqual(await presentedPixels(),expectedPixels,'Successful Retry presents the original filled drawing pixels exactly');
    const transferred = await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();return{manifest:await store.manifest(${JSON.stringify(seed.origin)})??null,bytes:(await store.resources(${JSON.stringify(seed.key)},${JSON.stringify(seed.ids)})).map(value=>Array.from(value))};})()`);
    assert.equal(transferred.manifest,null,'Successful adoption transfers sole manifest ownership');
    assert.deepEqual(transferred.bytes,seed.bytes,'Adoption retains the immutable authored resources');
    for(const prior of seed.prior) {
      const retained=await evaluate(`(async()=>{const store=(await import(${JSON.stringify(storeUrl)})).createRestartStore();return{record:await store.read(${JSON.stringify(prior.key)})??null,bytes:(await store.resources(${JSON.stringify(prior.key)},${JSON.stringify(prior.ids)})).map(value=>Array.from(value))};})()`);
      assert.deepEqual(retained,{record:prior.record,bytes:prior.bytes},'Later keeps known interrupted fixture copies unchanged');
    }
    console.log('Staged startup: recovery Retry exits modal during GPU loss, preserves bytes and restores after Restart', {suspensionThrows});
  } finally {
    await call('Page.removeScriptToEvaluateOnNewDocument',{identifier});
    await evaluate('window.recoveryGpuLoss?.restore()');
  }
}

async function checkpointBeforeReload(evaluate) {
  await evaluate('layerApp.documents.startRecovery().then(()=>layerApp.documents.autosave())');
}

async function activateForReload(call) {
  // Reloading an untouched fixture can race a workspace lease/switcher task.
  // Give Chrome a trusted contact so its ordinary beforeunload prompt can be
  // handled by the harness, rather than emitting a blocked-prompt diagnostic.
  for (const type of ['mousePressed','mouseReleased']) {
    await call('Input.dispatchMouseEvent',{type,x:1,y:1,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  }
}
