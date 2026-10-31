import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";
import {createDrawing} from "./new-drawing-fixture.mjs";
import {readyCanvas,readyCanvasPixels,waitForPixels} from "./canvas-pixels-fixture.mjs";
import {checkTextEntry,checkSearchField} from './control-style-fixture.mjs';
export async function checkEditor({call,evaluate,settle,canvasPixels:capturePixels}) {
  const masks=await evaluate(`(()=>{const input=document.createElement('input');input.type='checkbox';input.checked=true;input.className='panel-check';document.body.append(input);const visible=getComputedStyle(input,'::after').maskImage;editorMeasurementRoot.append(input);const measured=getComputedStyle(input,'::after').maskImage;input.remove();return {visible,measured};})()`);
  assert.notEqual(masks.visible,'none');
  assert.equal(masks.measured,masks.visible,'Measurement controls keep their external stylesheet asset base');
  const wait=(condition,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){const error=window.layerApp?.state().host_error;if(error)reject(Error('Host error: '+error));else if(${condition})resolve(true);else if(performance.now()-start>${timeout})reject(Error("Timed out: "+${JSON.stringify(condition)}+"; "+document.querySelector("#status").textContent));else setTimeout(check,40);}check();})`);
  const canvasPixels=readyCanvasPixels({evaluate,settle,capturePixels});
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();};
  const invoke=async id=>{await evaluate(`layerApp.dispatch({type:"invoke",command:${JSON.stringify(id)}})`);await settle();};
  const show=async panel=>{
    await evaluate(`(()=>{const group=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes("${panel}"));if(group.active!=="${panel}")layerApp.dispatch({type:"select_panel_tab",group:group.id,panel:"${panel}"});})()`);await settle();
  };
  const pointer=async(selector,fx,fy,move,pointerType="mouse")=>{
    const p=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width*${fx},y:r.y+r.height*${fy}}})()`);
    const appearance=()=>evaluate(`([...document.querySelectorAll('.tool-tile > button[data-command]')].filter(n=>!n.closest('[inert]')).map(n=>({command:n.dataset.command,disabled:n.disabled,opacity:getComputedStyle(n).opacity})))`);
    const before=selector==="#canvas"?await appearance():null;
    if(before)assert.ok(before.length>0,"Check actual toolbar command buttons");
    for(const [type,x,y,buttons]of [["mousePressed",p.x,p.y,1],...(move?[["mouseMoved",p.x+move[0],p.y+move[1],1]]:[]),["mouseReleased",p.x+(move?.[0]||0),p.y+(move?.[1]||0),0]]){
      await call("Input.dispatchMouseEvent",{type,x,y,button:"left",buttons,clickCount:1,pointerType,force:buttons ? 0.6 : 0});
      if(before&&buttons){
        await settle();
        assert.deepEqual(await appearance(),before,`${pointerType} ${type}: canvas contact must not dim toolbar icons`);
      }
    }
    await settle();
  };
  // This journey exercises Paint's docked panels; the previous test may have
  // selected the minimal Sketch workspace.
  await evaluate('layerApp.dispatch({type:"workspace_manager",command:{type:"switch",id:"builtin:workspace:illustrator"}})');
  await wait('(()=>{const v=JSON.parse(layerApp.app.workspace_view());return v.id==="builtin:workspace:illustrator"&&v.ready&&!v.busy&&!v.dirty;})()');
  if(await evaluate("layerApp.state().commands.find(c=>c.id==='reset_layout').enabled")) {
    await invoke("reset_layout");
    await wait(`!![...document.querySelectorAll('.workspace-form[open] button')].find(n=>n.textContent==='Restore')`);
    await evaluate(`[...document.querySelectorAll('.workspace-form[open] button')].find(n=>n.textContent==='Restore').click()`);
    await wait('!document.querySelector(".workspace-form[open]") && !JSON.parse(layerApp.app.workspace_view()).busy');
  }
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='reset_layout').enabled"),false,"Paint starts from its starting layout");
  if(await evaluate("layerApp.state().workspace.zen_mode"))await invoke("zen_mode");
  console.log("editor startup",await evaluate('layerApp.startupTimes'));
  await wait('layerApp.startupTimes.complete!==null');
  await invoke("new_document");
  await evaluate(createDrawing());
  await readyCanvas({evaluate,settle});
  await settle();
  assert.ok(await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).application_menus.every(menu=>document.querySelector(`[data-menu="${menu.id}"]`))'));
  assert.ok(await evaluate('!!document.querySelector(".commands-panel .tile-button")'));
  await waitForPixels(canvasPixels,settle,pixels=>pixels.white>10000,"Paper must be visible in the editor");
  await show("adjustments");
  for(const theme of ["light","dark"]) {
    await evaluate(`layerApp.dispatch({type:"set_theme",theme:${JSON.stringify(theme)}})`);
    await click(".filter-picker-header button:not(.dropdown)");
    await call("Input.insertText",{text:"glass"});await settle();
    await checkSearchField({call,evaluate,settle},'.filter-picker-header input[type="search"]','filters-search');
    const search=await evaluate(`(()=>{
      const input=document.querySelector('.filter-picker-header input[type=search]'),header=input.parentElement,select=header.querySelector('.dropdown');
      const style=getComputedStyle(input),control=getComputedStyle(select),rect=input.getBoundingClientRect();
      const fields=['height','borderTopWidth','borderTopLeftRadius','cornerShape','backgroundColor','color','fontFamily','fontSize','paddingLeft','outlineStyle','boxShadow'];
      return {value:input.value,focused:document.activeElement===input,left:rect.left,headerLeft:header.getBoundingClientRect().left,
        actual:fields.map(k=>style[k]),expected:fields.map(k=>control[k]),
        clear:{x:rect.right-parseFloat(style.paddingRight)-6,y:rect.top+rect.height/2}};
    })()`);
    assert.equal(search.value,"glass");assert.equal(search.focused,true);
    assert.equal(search.left,search.headerLeft,"Filters search aligns with its header");
    assert.deepEqual(search.actual,search.expected,`${theme} Filters search matches the panel dropdown`);
    assert.deepEqual(await evaluate("layerApp.state().adjustments.map(c=>c.id)"),["glass","rainy_glass"]);
    await mkdir("artifacts/ui/filter-search",{recursive:true});
    const shot=await call("Page.captureScreenshot",{format:"png"});
    await writeFile(`artifacts/ui/filter-search/${theme}.png`,Buffer.from(shot.data,"base64"));
    for(const type of ["mousePressed","mouseReleased"])await call("Input.dispatchMouseEvent",{type,...search.clear,button:"left",buttons:type==="mousePressed"?1:0,clickCount:1});
    await settle();
    assert.equal(await evaluate("layerApp.state().filter_picker.search"),"","Native search clear updates the filter list");
    await click(".filter-picker-header button:not(.dropdown)");
  }
  await show("layers");
  await evaluate("layerApp.dispatch({type:'layer',action:{op:'begin_rename',id:1}})");
  await checkTextEntry(evaluate,'.layer-name-entry',{height:await evaluate('layerApp.app.catalog().panel_metrics.control_height'),padding:2});
  await evaluate("layerApp.dispatch({type:'layer',action:{op:'cancel_rename'}})");
  for(let i=0;i<4;i++)await invoke("zoom_in");
  const middle=await evaluate("Math.floor(innerWidth/2)");
  const header=await call("Page.captureScreenshot",{format:"png",fromSurface:true,clip:{x:middle,y:2,width:1,height:1,scale:1}});
  const headerPixel=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,'+${JSON.stringify(header.data)};await image.decode();const c=document.createElement('canvas');c.width=c.height=1;const ctx=c.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);return [...ctx.getImageData(0,0,1,1).data];})()`);
  assert.deepEqual(headerPixel,[255,255,255,255],"Zoomed paper must composite through empty header space");
  await invoke("fit_canvas");
  await invoke("fill");
  assert.ok(await evaluate('layerApp.state().tool_settings.some(f=>f.id.includes("gap"))'),"Fill exposes gap controls");
  assert.ok(await evaluate('!!document.querySelector(".tool-settings-control [data-tool-setting]")'));
  await invoke("auto_select");
  assert.ok(await evaluate('layerApp.state().tool_settings.some(f=>f.id.includes("gap"))'),"Auto Select exposes gap controls");
  await invoke("brush");
  await show("color");
  await evaluate('layerApp.dispatch({type:"color",action:{op:"shape",shape:"circle"}})');
  await settle();
  const before=await evaluate('layerApp.state().brush.color');
  await pointer(".dock-group .color-wheel",.95,.5);
  await pointer(".dock-group .color-wheel",.6,.4);
  assert.notDeepEqual(await evaluate('layerApp.state().brush.color'),before,"Color wheel changes paint");
  console.log("color/tool controls passed");
  await show("navigator");
  assert.ok(await evaluate(`([...document.querySelectorAll('.dock-group [data-navigator-command]')].every(button=>{
    const r=button.getBoundingClientRect(),hit=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);
    return r.width>0&&r.height>0&&r.top>=0&&r.bottom<=innerHeight&&button.contains(hit);
  }))`),"Every Navigator command must have an unobscured hit target in the default panel");
  const zoom=await evaluate('layerApp.state().camera.zoom');
  await pointer('.dock-group [data-navigator-command="zoom_in"]',.5,.5);
  assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom);
  await pointer('.dock-group [data-navigator-command="flip_horizontal"]',.5,.5);
  assert.equal(await evaluate('document.querySelector(\'.dock-group [data-navigator-command="flip_horizontal"]\').getAttribute("aria-pressed")'),"true");
  await pointer('.dock-group [data-navigator-command="flip_horizontal"]',.5,.5);
  const pan=await evaluate('layerApp.state().camera.translation');
  await pointer(".dock-group .navigator-overview",.5,.5,[20,10]);
  assert.notDeepEqual(await evaluate('layerApp.state().camera.translation'),pan,"Navigator moves the camera");
  assert.ok(await evaluate('!!document.querySelector(".navigator-surface")'),"Navigator uses a native GPU surface");
  await evaluate('window.editorWorkspace=layerApp.app.workspace_persistence(); window.editorSettings=layerApp.state().settings; layerApp.dispatch({type:"restore_settings",settings:editorSettings})');
  await invoke("zen_mode");
  await call("Input.dispatchMouseEvent",{type:"mouseMoved",...await evaluate('({x:innerWidth/2,y:innerHeight/2})'),buttons:0});
  await settle();
  assert.ok(await evaluate('document.querySelector("#workspace").classList.contains("zen-hidden")'));
  await invoke("zen_mode");
  await evaluate(`(()=>{const group=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes("navigator"));window.editorColumnGroup=group.id;window.editorColumnWidth=group.bounds.width;layerApp.dispatch({type:"customize",action:{type:"set_column_collapsed",group:group.id,collapsed:true}});const column=layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.groups.some(g=>g.group===group.id)).id;layerApp.dispatch({type:"customize",action:{type:"set_column_drawers",column,drawers:true}});})()`);
  await settle();
  await click('.collapsed-column [data-panel="navigator"]');
  await wait('!!document.querySelector(".content-drawer .navigator-overview")');
  await settle();
  await wait('(document.querySelector(".content-drawer")?.getBoundingClientRect().width ?? 0)+1/devicePixelRatio>=editorColumnWidth');

  await click('.content-drawer [data-navigator-command="zoom_out"]');
  await pointer("#canvas",.5,.5);
  assert.ok(await evaluate('!!document.querySelector(".content-drawer")'),"Column drawers remain open while drawing");
  await click('.collapsed-column [data-panel="navigator"]');
  await wait('!document.querySelector(".content-drawer")');
  await evaluate('layerApp.dispatch({type:"customize",action:{type:"set_column_collapsed",group:window.editorColumnGroup,collapsed:false}})');
  await settle();
  assert.equal(await evaluate('layerApp.state().workspace.layout.collapsed.length'),0);
  await evaluate('(()=>{const send=action=>layerApp.dispatch({type:"customize",action}),toolbar=()=>layerApp.state().workspace.layout.panels.find(p=>p.id==="toolbar");send({type:"insert_tools",panel:"toolbar",before:toolbar().content.tiles[0].id});send({type:"picker_select",control:{kind:"divider"},selected:true});send({type:"picker_select",control:{kind:"panel",panel:"color"},selected:true});send({type:"confirm_tools"});window.editorDrawerTile=toolbar().content.tiles.find(t=>t.control.kind==="panel"&&t.control.panel==="color").id;const group=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes("navigator"));layerApp.dispatch({type:"move_panel",panel:"toolbar",target:{kind:"tab",group:group.id,index:null}});send({type:"set_column_collapsed",group:group.id,collapsed:true});const column=layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.groups.some(g=>g.group===group.id)).id;send({type:"set_column_drawers",column,drawers:true});})()');
  await settle();
  await click('.collapsed-column [data-panel="toolbar"]');
  await wait('[...document.querySelectorAll(".content-drawer [data-tile] button")].some(n=>Number(n.parentElement.dataset.tile)===editorDrawerTile)');
  await evaluate('new Promise(r=>setTimeout(r,200))');
  assert.ok(await evaluate('!!document.querySelector(".content-drawer .tile-divider")'),"Toolbar divider is a separator");
  await evaluate('[...document.querySelectorAll(".content-drawer [data-tile] button")].find(n=>Number(n.parentElement.dataset.tile)===editorDrawerTile).click()');
  await settle();
  await wait('!!document.querySelector(".content-drawer[data-drawer=tool] .color-wheel")');
  await evaluate('new Promise(r=>setTimeout(r,200))');
  assert.ok(await evaluate('(document.querySelector(".content-drawer[data-drawer=tool]")?.getBoundingClientRect().width ?? 0)+1/devicePixelRatio>=editorColumnWidth'),"Nested content drawer uses measured toolbar anchor");
  await pointer("#canvas",.5,.5);
  await wait('!document.querySelector(".content-drawer[data-drawer=tool]")');
  await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:editorWorkspace})');
  await settle();
  console.log("Navigator, partial Zen, collapsed columns, nested drawers, dividers passed");
  // Real Wasm save/open/export with an in-memory implementation only of the
  // browser's file transport. Check bytes and shared checkpoints, not mock models.
  await evaluate(`window.editorFiles=new Map();window.editorSavedPickers=[window.showSaveFilePicker,window.showOpenFilePicker];
    window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){editorFiles.set(options.suggestedName,bytes)},async abort(){}}}});
    window.showOpenFilePicker=async()=>[{name:"roundtrip.capy",async getFile(){return new File([editorFiles.get([...editorFiles.keys()].find(k=>k.endsWith(".capy")))],"roundtrip.capy")}}];`);
  await invoke("fit_canvas");
  await pointer("#canvas",.50,.50,[65,20],"pen");
  assert.ok(await evaluate('layerApp.state().document_file.modified'));
  await evaluate('(()=>{const effect=layerApp.state().adjustments.find(a=>a.id.includes("domain_warp"));if(!effect)throw Error("Domain Warp missing");layerApp.dispatch(effect.action);})()');
  await settle();
  await invoke("save_document_as");
  await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified && !layerApp.documents.busy()');
  assert.equal(await evaluate('layerApp.state().document_file.modified'),false);
  assert.ok(await evaluate('[...editorFiles.entries()].some(([name,bytes])=>name.endsWith(".capy")&&bytes.length>100)'));
  await invoke("export_document");
  await wait(`!!document.querySelector('dialog[open] .dropdown[aria-label="Format"]')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Choose File…').click()`);
  await wait('[...editorFiles.keys()].some(name=>name.endsWith(".png")) && !layerApp.state().document_file.busy && !layerApp.documents.busy()',120000);
  assert.deepEqual(await evaluate('[...editorFiles.entries()].find(([name])=>name.endsWith(".png"))?.[1].slice(0,8)&&Array.from([...editorFiles.entries()].find(([name])=>name.endsWith(".png"))[1].slice(0,8))'),[137,80,78,71,13,10,26,10]);
  await invoke("new_document");
  await wait('!!document.querySelector("#new-drawing-dialog[open] #new-drawing-width input")');
  assert.equal(await evaluate('document.querySelector("#new-drawing-width input").type'),"text");
  await evaluate(`(()=>{const units=layerApp.state().new_drawing.units,unit=document.querySelector('#new-drawing-unit');unit.value=String(units.findIndex(u=>u.value==='px'));unit.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  const editDimension=(id,text)=>evaluate(`(()=>{const input=document.querySelector('#new-drawing-${id} input');input.focus();input.value=${JSON.stringify(text)};input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
  const tabs=()=>evaluate('[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height,layerApp.app.document_tabs(0).tabs.length]');
  await editDimension("width","３２０＋");
  await evaluate('document.querySelector("#new-drawing-create").click()');
  assert.ok(await evaluate('!!document.querySelector("#new-drawing-dialog[open] #new-drawing-width input[aria-invalid=true]")'),"Malformed shared numeric input keeps Create from closing the dialog");
  const beforeRejectedDimensions=await tabs();
  await editDimension("width","１６０＊２");
  await evaluate('document.querySelector("#new-drawing-create").click()');
  assert.ok(await evaluate('!!document.querySelector("#new-drawing-dialog[open] #new-drawing-width input[aria-invalid=true]")'),"Fullwidth arithmetic is rejected without creating a document");
  assert.equal(await evaluate('document.querySelector("#new-drawing-width input").value'),"１６０＊２","Rejected numeric text remains editable");
  assert.deepEqual(await tabs(),beforeRejectedDimensions,"Rejected dimensions preserve the drawings");
  await editDimension("width","160*2");
  await evaluate('document.querySelector("#new-drawing-width input").dispatchEvent(new CompositionEvent("compositionstart",{bubbles:true}));document.querySelector("#new-drawing-create").click()');
  assert.ok(await evaluate('!!document.querySelector("#new-drawing-dialog[open] #new-drawing-width input")'),"Create waits for numeric composition to finish");
  await evaluate('document.querySelector("#new-drawing-width input").dispatchEvent(new CompositionEvent("compositionend",{bubbles:true}))');
  await editDimension("height","120+120");
  await evaluate('(()=>{const name=document.querySelector("#new-drawing-name");name.value="Arithmetic";name.dispatchEvent(new Event("input",{bubbles:true}));document.querySelector("#new-drawing-create").click();})()');
  await wait('layerApp.state().tabs[0].width===320 && !layerApp.state().document_file.busy && !layerApp.documents.busy()');
  assert.deepEqual(await tabs(),[320,240,beforeRejectedDimensions[2]+1],"Create adds a drawing at the evaluated size");
  assert.equal(await evaluate('layerApp.app.document_tabs(0).tabs.find(t=>String(t.id)===String(layerApp.app.document_tabs(0).selected)).title'),"Arithmetic","The drawing takes its name");
  await invoke("new_document");
  await wait('!!document.querySelector("#new-drawing-dialog[open]")');
  assert.deepEqual(await evaluate('layerApp.state().new_drawing.values'),[320,240],"New drawing opens with the last drawing's setup");
  await evaluate('document.querySelector("#new-drawing-cancel").click()');
  await wait('!layerApp.state().document_file.busy && !document.querySelector("#new-drawing-dialog")');
  await invoke("pen");
  await invoke("open_document");
  await wait('layerApp.state().document_file.location?.name==="roundtrip.capy" && !layerApp.state().document_file.busy');
  await wait('layerApp.app.brush_ready()');
  assert.equal(await evaluate('layerApp.state().brush.tool'),"pen","The current brush selection is retained when opening a drawing");
  assert.ok(await evaluate('layerApp.state().tabs[0].width>320'));
  await evaluate('layerApp.dispatch({type:"select_layer",id:layerApp.state().layers.find(l=>l.label==="Current ink").id})');
  await settle();
  // Selecting the paint layer can prepare a different brush target. Wait for
  // that target, not just the brush that was ready before selecting the layer.
  await wait('layerApp.app.brush_ready()');
  // Use untouched paper: painting the same opaque stroke again can be a no-op.
  await pointer("#canvas",.4,.65,[30,0]);
  await wait('layerApp.state().document_file.modified');
  await invoke("close_document");
  await wait('!![...document.querySelectorAll(".document-dialog h2")].find(n=>n.textContent.includes("Save changes"))');
  await click(".document-dialog footer button");
  assert.equal(await evaluate('layerApp.state().document_file.modified'),true,"Cancel preserves unsaved drawing");
  await evaluate('window.editorSavePicker=showSaveFilePicker;window.showSaveFilePicker=async()=>{throw new DOMException("Cancelled","AbortError")}');
  await invoke("save_document_as");
  await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('layerApp.state().document_file.modified'),true,"Cancelled file picker does not mark edits saved");
  await evaluate('window.showSaveFilePicker=editorSavePicker');
  await invoke("close_document");
  await wait('!!document.querySelector(".document-dialog")');
  await evaluate('[...document.querySelectorAll(".document-dialog button")].find(n=>n.textContent==="Discard Changes").click()');
  await wait('!layerApp.state().document_file.close_ready && layerApp.state().document_file.location==null && !layerApp.state().document_file.modified');
  await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:editorWorkspace}); layerApp.dispatch({type:"restore_settings",settings:editorSettings}); [window.showSaveFilePicker,window.showOpenFilePicker]=editorSavedPickers;');
  await settle();
  // Workspace persistence now uses the shared database controller; the old
  // localStorage key is only a migration input. Verify the Commands panel
  // remains available after a completed save/reload. The profile is isolated.
  await wait('JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
  const reloadedFrom = await evaluate("performance.timeOrigin");
  await call("Page.reload", {ignoreCache: true});
  for (;;) try { await wait(`performance.timeOrigin !== ${reloadedFrom} && window.layerApp?.startupTimes.complete != null`,60000); break; }
    catch (error) { if (!/navigated|context|Cannot find/i.test(String(error))) throw error; }
  await wait('JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  assert.ok(await evaluate('!!layerApp.app.workspace_persistence().layout.panels.find(p=>p.id==="commands")'));
  assert.ok(await evaluate('!!document.querySelector(".commands-panel .tile-button")'));
  const directory=process.env.LAYER_TEST_ARTIFACTS||"artifacts/web";
  await mkdir(directory,{recursive:true});
  const image=await call("Page.captureScreenshot",{format:"png"});
  await writeFile(`${directory}/editor-parity.png`,Buffer.from(image.data,"base64"));
  console.log("Project save/open/new, PNG export, unsaved cancellation, workspace restoration passed");
}
