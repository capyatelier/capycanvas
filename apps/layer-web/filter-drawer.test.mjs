import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkFilterDrawer({call,evaluate,settle}) {
  const pause=()=>new Promise(r=>setTimeout(r,220));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await pause();};
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,50);}check();})`);
  const point=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  const event=async(type,p,device)=>{
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:type==='move'?'none':'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.7});
  };
  const contact=async(selector,device='mouse')=>{
    // Recovery discovery can finish after startup; retain earlier test drawings.
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click()`);
    const p=await point(selector);await event('down',p,device);await event('up',p,device);await settle();await pause();
  };
  const swipe=async(selector,dx,dy,device)=>{const p=await point(selector);await event('down',p,device);for(let i=1;i<=6;i++)await event('move',{x:p.x+dx*i/6,y:p.y+dy*i/6},device);await event('up',{x:p.x+dx,y:p.y+dy},device);await settle();await pause();};
  const capture=async name=>{await mkdir('artifacts/filters-web',{recursive:true});const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`artifacts/filters-web/${name}.png`,Buffer.from(shot.data,'base64'));};
  console.log("Filter drawer: switching to Sketch");
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:painter'}));null`);
  await wait('JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:painter"&&!JSON.parse(layerApp.app.workspace_view()).busy');
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click()`);
  // Runtime filter validation owns a document-operation gate during startup.
  await wait('!layerApp.documents.busy() && layerApp.app.brush_ready() && layerApp.state().commands.find(c=>c.id==="new_document").enabled');
  await send({type:'invoke',command:'new_document'});
  await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
  await evaluate(`(()=>{const create=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create');const d=create.closest('dialog');for(const n of d.querySelectorAll('input[type=number]'))n.value=256;create.click();})()`);
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !layerApp.state().document_file.busy && !layerApp.documents.busy() && layerApp.app.brush_ready() && !document.querySelector('dialog[open]');})()`);
  console.log("Filter drawer: opening filters");
  const entries=await evaluate('layerApp.state().workspace.layout.header.zones.flat()');
  const header=(key,value)=>`[data-header-item="${entries.find(e=>e.item.control?.[key]===value).id}"] .header-tool`;
  const filters=header('panel','adjustments'), layers=header('panel','layers');
  if(await evaluate("layerApp.state().customization.drawer"))await send({type:"customize",action:{type:"close_expanded"}});
  await contact(filters);
  assert.deepEqual(await evaluate('layerApp.state().customization.drawer.columns'),[['filter_types'],['adjustments'],['properties']]);
  const choices=await evaluate('layerApp.state().adjustments.slice(0,2).map(c=>c.id)');
  const count=await evaluate('layerApp.state().layers.length');let id;
  for(const [i,device] of ['mouse','touch','pen'].entries()) {
    const choice=choices[i%2];await contact(`.content-drawer [data-effect="${choice}"]`,device);
    await wait(`layerApp.state().filter_picker.selected===${JSON.stringify(choice)}`);
    const selected=await evaluate('Number(layerApp.state().layer_properties.layer)');
    if(id!=null)assert.equal(selected,id);id=selected;
    assert.equal(await evaluate('layerApp.state().layers.length'),count+1);
    assert.equal(await evaluate('layerApp.state().filter_picker.selected'),choice);
    assert.equal(await evaluate('layerApp.state().layers.find(l=>Number(l.id)===1).drawing'),true);
  }
  await contact(filters);assert.equal(await evaluate('layerApp.state().customization.drawer'),undefined);
  await contact(filters);assert.equal(await evaluate('Number(layerApp.state().layer_properties.layer)'),id);
  for(const theme of ['light','dark']){await send({type:'set_theme',theme});await capture(`filters-${theme}`);}
  await contact('.content-drawer .cancel-filter','pen');
  assert.equal(await evaluate('layerApp.state().layers.length'),count);
  assert.equal(await evaluate('layerApp.state().customization.drawer'),undefined);
  await contact(filters);await send({type:'layer',action:{op:'select',id:2,mask:false}});await wait('Number(layerApp.state().layer_properties.layer)===2');
  await send({type:'set_color',rgba:[1,0,0,1]});await contact('.content-drawer [data-action="color-bucket"]','touch');
  assert.equal(await evaluate('layerApp.state().layers.find(l=>Number(l.id)===2).content_icon'),'layer-fill-symbolic');
  assert.equal(await evaluate('layerApp.state().layers.find(l=>Number(l.id)===2).has_mask'),false);
  assert.equal(await evaluate('layerApp.state().layer_tools.controls.opacity'),true);
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const [name,expected] of [['Paper','Paper (Solid Color)'],['Solid Color','Solid Color'],['Paper','Paper (Solid Color)']]) {
      await send({type:'layer',action:{op:'rename',id:2,name}});
      assert.equal(await evaluate('document.querySelector(".content-drawer .effect-properties h3").textContent'),expected);
    }
    await capture(`paper-properties-${theme}`);
  }
  console.log('PASS: filter replacement, cancellation, reopening and paper properties');
  await contact(layers);
  const row=id=>`.content-drawer .layer-row[data-layer="${id}"]`;
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await wait(`(()=>{const c=document.querySelector('${row(2)} .layer-thumbnail canvas');if(!c)return false;const p=c.getContext('2d').getImageData(16,16,1,1).data;return p[0]>240&&p[1]<10&&p[2]<10&&p[3]===255;})()`);
    assert.equal(await evaluate(`document.querySelector('${row(2)} .layer-type-symbol')?.getAttribute('aria-hidden')`),'true');
    assert.equal(await evaluate(`document.querySelector('${row(1)} .layer-type-symbol')`),null);
    await capture(`fill-thumbnail-${theme}`);
  }
  await contact(filters);
  await send({type:'effect',action:{op:'insert',effect:'gradient_fill'}});
  await send({type:'effect',action:{op:'set',layer:2,key:'angle',value:{kind:'number',value:0}}});
  assert.equal(await evaluate('layerApp.state().layers.find(l=>Number(l.id)===2).content_icon'),'layer-gradient-symbolic');
  await contact(layers);
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await wait(`(()=>{const c=document.querySelector('${row(2)} .layer-thumbnail canvas');if(!c)return false;const d=c.getContext('2d');return d.getImageData(4,16,1,1).data[0]<60&&d.getImageData(27,16,1,1).data[0]>200;})()`);
    assert.equal(await evaluate(`document.querySelector('${row(2)} .layer-type-symbol')?.getBoundingClientRect().width`),14);
    await capture(`gradient-thumbnail-${theme}`);
  }
  await evaluate(`layerApp.dispatch({type:'effect',action:{op:'gradient',target:layerApp.state().layer_properties.controls.find(c=>c.key==='gradient').gradient.destination,edit:{kind:'reverse'}}})`);await settle();
  await wait(`(()=>{const d=document.querySelector('${row(2)} .layer-thumbnail canvas')?.getContext('2d');return d&&d.getImageData(4,16,1,1).data[0]>200&&d.getImageData(27,16,1,1).data[0]<60;})()`);
  await send({type:'invoke',command:'undo'});
  await wait(`(()=>{const d=document.querySelector('${row(2)} .layer-thumbnail canvas')?.getContext('2d');return d&&d.getImageData(4,16,1,1).data[0]<60&&d.getImageData(27,16,1,1).data[0]>200;})()`);
  console.log('PASS: gradient thumbnails, parameter changes and undo in both themes');
  await swipe(row(1),-90,0,'mouse');
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(row(1))}).parentElement.style.getPropertyValue('--swipe')`),'0px');
  for(const [id,device] of [[1,'pen'],[2,'touch']]) {
    await swipe(row(id),-90,0,device);
    const offset=()=>evaluate(`parseFloat(document.querySelector(${JSON.stringify(row(id))}).parentElement.style.getPropertyValue('--swipe'))`);
    assert.equal(await offset(),72);
    await swipe(row(id),90,0,device);assert.equal(await offset(),0);
    await swipe(row(id),-90,0,device);await capture(`delete-${id}`);
    // The action precedes its translated content in the retained row wrapper.
    await contact(`.content-drawer .layer-swipe:has([data-layer="${id}"]) .layer-swipe-delete`,device);
    await wait(`!layerApp.state().layers.some(l=>Number(l.id)===${id})`);
  }
  assert.equal(await evaluate('layerApp.state().layers.length'),0);await capture('empty-canvas');
  await send({type:'invoke',command:'undo'});await send({type:'invoke',command:'undo'});
  assert.equal(await evaluate('layerApp.state().layers.length'),2);
  console.log('PASS: swipe deletion, empty stack and undo');
  // Pen panning must yield to a completed layer-row hold, even in an overflowing list.
  await evaluate(`document.querySelector('.content-drawer .layer-rows').style.maxHeight='60px'`);
  const heldRow=await point(`${row(1)} .layer-name`);
  await event('down',heldRow,'pen');await new Promise(r=>setTimeout(r,650));
  assert.equal(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"),true,'Pen hold opens the layer menu');
  await event('move',{x:heldRow.x,y:heldRow.y+16},'pen');await settle();
  assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),1,'Pen hold retains layer reordering');
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await event('up',{x:heldRow.x,y:heldRow.y+16},'pen');
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),0);
  await evaluate(`document.querySelector('.content-drawer .layer-rows').style.maxHeight=''`);
  const brush=header('command','drawing_brush');await contact(brush);
  if(!await evaluate('layerApp.state().customization.drawer'))await contact(brush);
  const sets=await evaluate('layerApp.state().tool_panels.brush_sets.groups');let longest=sets[0],size=0;
  for(const set of sets){await send(set.action);const count=await evaluate('layerApp.state().tool_set.subtools.length');if(count>size){size=count;longest=set;}}
  await send(longest.action);
  // Keep the native drawer scroller small enough for the six-tool catalog to overflow.
  await evaluate(`(()=>{const p=document.querySelector('.content-drawer .tools-control').closest('.drawer-column');p.style.maxHeight='180px';if(p.scrollHeight<=p.clientHeight+10)throw Error('Tools must overflow');window.filterScroll=p;})()`);
  for(const device of ['mouse','touch','pen']) {
    await evaluate('filterScroll.scrollTop=0');await settle();
    const p=await evaluate('(()=>{const r=filterScroll.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+Math.min(r.height-30,200)}})()');
    await event('down',p,device);for(let i=1;i<=6;i++)await event('move',{x:p.x,y:p.y-20*i},device);await event('up',{x:p.x,y:p.y-120},device);await pause();
    const scroll=await evaluate('filterScroll.scrollTop');
    if(device==='mouse')assert.equal(scroll,0);else assert.ok(scroll>25,`${device} scrolls tools: ${scroll}`);
  }
  await capture('pen-tools-scroll');await evaluate('filterScroll.style.maxHeight="";delete window.filterScroll');
  console.log('PASS: filter drawer replacement/reopen/cancel; paper properties; touch/pen swipe/delete; empty canvas and undo; mouse/touch/pen tool scrolling');
  await send({type:'invoke',command:'new_document'});
  await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create').click()`);
  await wait(`!layerApp.state().document_file.busy && !layerApp.documents.busy() && layerApp.app.brush_ready() && !document.querySelector('dialog[open]')`);
  await send({type:'customize',action:{type:'set_panel_visible',panel:'layers',visible:true}});
  const showLayers = async () => send({type:'select_panel_tab',group:(await evaluate('layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes("layers")).id')),panel:'layers'});
  await showLayers();
  const owner = await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  for (const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for (const label of ['Grayscale','Two-tone (black & white)','Full color']) {
      await contact('#layer-color-mode');
      await wait(`!![...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes(${JSON.stringify(label)}))`);
      await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes(${JSON.stringify(label)})).dataset.layerChoice="true"`);
      await contact('[data-layer-choice="true"]');
      await wait(`layerApp.state().layer_tools.color_mode?.value===${JSON.stringify(label)}`);
    }
    const before = await evaluate('layerApp.state().layers.length');
    for (const [index,filter] of ['Exposure','Curves'].entries()) {
      await showLayers();
      await contact('#layer-add-filter');
      await wait(`!![...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes('Tone'))`);
      await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes('Tone')).click()`);
      await wait(`!![...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes(${JSON.stringify(filter)}))`);
      await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.textContent.includes(${JSON.stringify(filter)})).click()`);
      await wait(`layerApp.state().layer_tools.editing_layer.adjustment_effect && layerApp.state().layers.length===${before+index+1}`);
    }
    assert.equal(await evaluate('layerApp.state().layers.length'),before+2);
    await send({type:'invoke',command:'undo'});await send({type:'invoke',command:'undo'});
    assert.equal(await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)'),owner);
    await showLayers();
    await capture(`layer-modes-and-filters-${theme}`);
  }
  console.log('PASS: layer color modes and local filter insertion in both themes');

}
