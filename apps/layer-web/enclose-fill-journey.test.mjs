import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {png} from './clone-journey.test.mjs';
import {placementSave} from './image-placement-motion.test.mjs';
import {rasterIdentity,sourceIdentity,packageOccurrences} from './package-fixture.test.mjs';
import {chooseToolActionGroup} from './panel-controls-journey.test.mjs';
import {clickDropdownChoice} from './dropdown-fixture.mjs';
import {createDrawing} from './new-drawing-fixture.mjs';

function scribbleDistance(caseData,region,point) {
  const distance=(p,a,b)=>{const dx=b[0]-a[0],dy=b[1]-a[1],t=Math.max(0,Math.min(1,((p[0]-a[0])*dx+(p[1]-a[1])*dy)/Math.max(Number.EPSILON,dx*dx+dy*dy)));return Math.hypot(p[0]-a[0]-dx*t,p[1]-a[1]-dy*t);};
  return Math.min(...region.polygon.map((a,i)=>distance(point,a,region.polygon[(i+1)%region.polygon.length])),...caseData.paths.flatMap(path=>path.slice(1).map((b,i)=>distance(point,path[i],b))));
}

function scoreScribble(caseData,region,result,reference,outsideCell=0) {
  const band=caseData.size/2+1.5;
  const inside=p=>{let hit=false;for(let i=0;i<region.polygon.length;i++){const a=region.polygon[i],b=region.polygon[(i+1)%region.polygon.length];if((a[1]>p[1])!==(b[1]>p[1])&&p[0]<(b[0]-a[0])*(p[1]-a[1])/(b[1]-a[1])+a[0])hit=!hit;}return hit;};
  const alpha=(x,y)=>result.pixels[(y*result.extent[0]+x)*4+3];
  let missed=0,leaked=outsideCell;
  const [left,top,width,height]=caseData.bounds;
  for(let y=top;y<top+height;y++)for(let x=left;x<left+width;x++) {
    const point=[x+.5,y+.5],line=scribbleDistance(caseData,region,point),junctionInk=reference.pixels[(y*reference.extent[0]+x)*4+3]>0&&caseData.junctions.some(junction=>Math.hypot(point[0]-junction[0],point[1]-junction[1])<=24);
    if(inside(point)){if(line>band&&!junctionInk&&alpha(x,y)<250)missed++;}
    else if(line>band&&!junctionInk&&alpha(x,y))leaked++;
  }
  for(const [x,y] of region.wash_samples)assert.ok(alpha(x,y)>=250,`${caseData.name}: missed bottom wash at ${x},${y}`);
  const leakCeiling=region.known_leaks?.web??0;
  assert.ok(leaked<=leakCeiling,`${caseData.name} ${region.side}: ${leaked} unexpected leaked pixels (ceiling ${leakCeiling})`);
  if(leakCeiling)console.log(`${caseData.name} ${region.side} known leak: ${leaked}/${leakCeiling}; shrink or remove when improved`);
  const ceiling=region.known_misses?.web??4;
  assert.ok(missed<=ceiling,`${caseData.name}: ${missed} missed interior/tip/corner pixels (ceiling ${ceiling})`);
  if(ceiling>4)console.log(`${caseData.name} ${region.side} known misses: ${missed}/${ceiling}; shrink or remove the ceiling when improved`);
  let knownMissing=0;
  for(const centre of region.centres){
    const covered=[-1,0,1].some(dy=>[-1,0,1].some(dx=>{const x=Math.round(centre[0])+dx,y=Math.round(centre[1])+dy;return Math.hypot(x+.5-centre[0],y+.5-centre[1])<=1.5&&inside([x+.5,y+.5])&&alpha(x,y)>0;}));
    if(region.known_empty_centres.some(known=>known.every((value,i)=>value===centre[i])))knownMissing+=Number(!covered);
    else assert.ok(covered,`${caseData.name}: no fill under centre line ${centre}`);
  }
  if(region.known_empty_centres.length)console.log(`${caseData.name} ${region.side} known centre misses: ${knownMissing}/${region.known_empty_centres.length}; remove only after all hosts cover them`);
  const knownProbes=region.known_filled_probes?.web??[];
  assert.ok(knownProbes.every(index=>Number.isInteger(index)&&index>=0&&index<region.empty_probes.length));
  for(const [index,point] of region.empty_probes.entries())assert.ok(alpha(Math.floor(point[0]),Math.floor(point[1]))===0||leaked>0&&leakCeiling>0&&knownProbes.includes(index),`${caseData.name} ${region.side}: far side stays empty at ${point}`);
}

export async function checkEncloseFill({call,evaluate,settle}) {
  const scribbles=JSON.parse(await readFile(new URL('../../tools/fixtures/fill-scribble.json',import.meta.url),'utf8'));
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/enclose-fill/web';
  await mkdir(directory,{recursive:true});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(poll,30)}poll()})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const idle=()=>wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&layerApp.app.document_park_ready()');
  const save=placementSave({evaluate,invoke,idle});
  const fixture=png(384,256,(x,y)=>[40,150,270].some(left=>
    x>=left&&x<left+90&&y>=50&&y<170&&(x<left+6||x>=left+84||y<56||y>=164))?[0,0,0,255]:[0,0,0,0],4);
  const screen=([x,y])=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  const trackPoint=(x,y)=>evaluate(`({x:layerApp.canvas.getBoundingClientRect().x+${x},y:${y}})`);
  const pointer=(type,p,kind='mouse')=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.5});
  const contact=async selector=>{
    const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await pointer('mousePressed',p);await pointer('mouseReleased',p);await settle();
  };
  const selected=command=>evaluate(`!!layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.selected`);
  const path=async(cancel=false)=>{
    const points=[[20,30],[315,30],[315,190],[20,190],[20,30]];
    await pointer('mousePressed',await screen(points[0]));
    for(const p of points.slice(1)){await pointer('mouseMoved',await screen(p));await settle();}
    if(cancel)for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await pointer('mouseReleased',await screen(points.at(-1)));await settle();await idle();
  };
  const click=label=>evaluate(`(()=>{const button=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!button||button.disabled)throw Error('Missing enabled '+${JSON.stringify(label)});button.click()})()`);
  const output=async()=>{
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] .dropdown[aria-label="Format"]')`);
    for(const [label,value] of [['Format','Png'],['Bit depth','U8'],['Pixel size','Original'],['Dither','None']])await clickDropdownChoice({call,evaluate,settle},`dialog[open] .dropdown[aria-label="${label}"]`,value);
    await evaluate('placementTest.saved=null');await click('Choose File…');
    await wait('placementTest.saved?.length>0&&!document.querySelector("dialog[open]")');await idle();
    return evaluate(`(async()=>{const bitmap=await createImageBitmap(new Blob([placementTest.saved],{type:'image/png'})),canvas=new OffscreenCanvas(bitmap.width,bitmap.height),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(bitmap,0,0);bitmap.close();return{extent:[canvas.width,canvas.height],pixels:Array.from(context.getImageData(0,0,canvas.width,canvas.height).data)}})()`);
  };
  const preview=async()=>JSON.parse(await evaluate('JSON.stringify(layerApp.app.color_preview().fill_gesture,(_,value)=>typeof value==="bigint"?String(value):value)'));
  const pressPreview=async(point,theme)=>{
    const end=Date.now()+30000;
    for(;;){
      const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:Math.floor(point.x),y:Math.floor(point.y),width:1,height:1,scale:1}});
      const rgba=await evaluate(`(async()=>{const image=await createImageBitmap(new Blob([Uint8Array.from(atob('${shot.data}'),c=>c.charCodeAt(0))],{type:'image/png'})),c=new OffscreenCanvas(1,1),x=c.getContext('2d');x.drawImage(image,0,0);image.close();return Array.from(x.getImageData(0,0,1,1).data)})()`);
      if(rgba[0]>180&&rgba[1]<80&&rgba[3]===255){assert.ok(!(await preview()).pill,'Press preview appears before hold engages');break;}
      assert.ok(Date.now()<end,`Press preview becomes visible before release: ${rgba}`);await settle();
    }
    await writeFile(`${directory}/press-preview-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  };
  const heldPill=async label=>{
    await wait('!!layerApp.app.color_preview().fill_gesture?.pill');
    const view=await preview();assert.equal(view.pill.label,label);
    const rendered=await evaluate(`(()=>{const root=document.querySelector('.fill-gesture-pill'),r=root.getBoundingClientRect(),c=layerApp.canvas.getBoundingClientRect(),f=root.querySelector('.fill-gesture-fill').getBoundingClientRect();return{visible:root.checkVisibility(),x:r.x-c.x,y:r.y-c.y,width:r.width,height:r.height,end:f.right-c.x,label:root.querySelector('.number-title').textContent,labelFits:root.querySelector('.number-title').scrollWidth<=root.querySelector('.number-title').clientWidth,value:root.querySelector('.number-value').textContent,cursor:getComputedStyle(layerApp.canvas).cursor}})()`);
    assert.ok(rendered.visible);assert.ok(rendered.labelFits,'The shared pill label fits');for(const key of ['x','y','width','height'])assert.ok(Math.abs(rendered[key]-view.pill.bounds[key])<.5,`Shared pill ${key}`);
    assert.ok(Math.abs(rendered.end-view.pill.fill_end)<.5);assert.equal(rendered.label,view.pill.label);assert.equal(rendered.value,view.pill.value);assert.equal(rendered.cursor,'ew-resize');return view;
  };
  const scribblePath=async points=>{
    await idle();
    for(let i=0;i<points.length;i++){await call('Input.dispatchMouseEvent',{type:i?'mouseMoved':'mousePressed',...await screen(points[i]),button:'left',buttons:1,clickCount:1,pointerType:'pen',force:.5});await settle();}
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...await screen(points.at(-1)),button:'left',buttons:0,clickCount:1,pointerType:'pen',force:0});await settle();await idle();
  };
  const scribblePaper=async(theme,paper)=>{
    await invoke('new_document');await wait('!!layerApp.state().new_drawing');
    await contact('#new-drawing-texture');
    const paperIndex=await evaluate(`layerApp.state().new_drawing.drawer.tiles.findIndex(tile=>tile.id===${JSON.stringify(paper)})`);assert.ok(paperIndex>=0);
    await contact(`#new-drawing-papers [data-new-drawing-paper="${paperIndex}"]`);
    await contact('#new-drawing-paper-relief-field input');
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await call('Input.insertText',{text:'100'});
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await contact('#new-drawing-paper-use');

    const setup=await evaluate(createDrawing({width:scribbles.size[0],height:scribbles.size[1],space:'Srgb',depth:'U8',relief:null}));assert.equal(setup.paper.texture,paper);assert.equal(setup.paper.relief,100);assert.equal(await evaluate('document.body.dataset.theme'),theme);await idle();await invoke('fit_canvas');
    await send({type:'set_color',rgba:[0,0,0,1]});
    const cases=scribbles.cases.filter(c=>c.paper===paper);
    for(const caseData of cases) {
      if(caseData.vector){await send({type:'layer',action:{op:'reference_selection'}});await send({type:'layer',action:{op:'new_vector'}});assert.equal(await evaluate('layerApp.state().layers.find(l=>l.editing).content_icon'),'layer-path-symbolic');}
      await send({type:'select_brush',id:caseData.preset});await send({type:'set_brush_size',value:caseData.size});await send({type:'set_tool_setting',id:'smoothing_strength',value:0});
      for(const path of caseData.paths)await scribblePath(path);
      if(caseData.eraser.length){const revision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');await send({type:'select_brush',id:3});await send({type:'set_brush_size',value:caseData.gap});assert.ok(await selected('eraser'));await scribblePath(caseData.eraser);assert.notEqual(await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)'),revision,'Real eraser contact cuts the reference ink');}
    }
    const paperLayer=await evaluate('Number(layerApp.state().layers.find(layer=>layer.content_icon==="layer-paper-symbolic").id)');
    assert.equal(await evaluate(`layerApp.state().layers.find(layer=>Number(layer.id)===${paperLayer}).visible`),true);
    await send({type:'layer',action:{op:'visibility',id:paperLayer,value:false}});
    const reference=await output();assert.deepEqual(reference.extent,scribbles.size);
    await writeFile(`${directory}/scribble-reference-${paper}-${theme}.png`,Buffer.from(await evaluate("Array.from(placementTest.saved)")));
    await invoke('undo');await idle();
    assert.equal(await evaluate(`layerApp.state().layers.find(layer=>Number(layer.id)===${paperLayer}).visible`),true,'Reference export Undo restores Paper');
    await send({type:'layer',action:{op:'reference_selection'}});await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    await invoke('fill');await chooseToolActionGroup({call,evaluate,settle},'selection-source','selection_reference');
    await chooseToolActionGroup({call,evaluate,settle},'close-gaps','close_gaps_auto');
    await chooseToolActionGroup({call,evaluate,settle},'region-edges','edges_line_middle');
    for(const [id,value] of [['grain_depth',0],['tolerance',.05],['opacity',1]])await send({type:'set_tool_setting',id,value});
    assert.equal(await evaluate('layerApp.state().tool_settings.find(c=>c.id==="grain_depth").value'),0);
    await send({type:'set_color',rgba:[.85,.1,.05,1]});
    const baseline=rasterIdentity(await save());
    const visibility=await evaluate('layerApp.state().layers.map(l=>({id:Number(l.id),visible:l.visible}))');
    const references=await evaluate('layerApp.state().layers.filter(l=>!l.editing&&l.visible).map(l=>Number(l.id))');
    for(const caseData of cases)for(const region of caseData.regions){
      await scribblePath([region.seed,region.seed]);
      await writeFile(`${directory}/scribble-${caseData.preset}-${region.side}-${paper}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      const hidden=[];
      try {
        for(const id of references){await send({type:'layer',action:{op:'visibility',id,value:false}});assert.equal(await evaluate(`layerApp.state().layers.find(layer=>Number(layer.id)===${id}).visible`),false,'Export isolation changes actual visibility');hidden.push(id);}
        const result=await output();assert.deepEqual(result.extent,scribbles.size,'Geometry scoring reads original exported pixels');
        await writeFile(`${directory}/scribble-target-${caseData.preset}-${region.side}-${paper}-${theme}.png`,Buffer.from(await evaluate("Array.from(placementTest.saved)")));
        let outsideCell=0;for(let y=0;y<result.extent[1];y++)for(let x=0;x<result.extent[0];x++)if(result.pixels[(y*result.extent[0]+x)*4+3]&&!((x>=caseData.bounds[0]&&y>=caseData.bounds[1]&&x<caseData.bounds[0]+caseData.bounds[2]&&y<caseData.bounds[1]+caseData.bounds[3])||scribbleDistance(caseData,region,[x+.5,y+.5])<=caseData.size/2+1.5))outsideCell++;
        scoreScribble(caseData,region,result,reference,outsideCell);
      } finally {for(let i=hidden.length;i>0;i--){await invoke('undo');await idle();}}
      assert.deepEqual(await evaluate('layerApp.state().layers.map(l=>({id:Number(l.id),visible:l.visible}))'),visibility,`${caseData.name} ${region.side}: isolation Undo restores original visibility`);
      await invoke('undo');await idle();
      assert.deepEqual(rasterIdentity(await save()),baseline,`${caseData.name} ${region.side}: Undo restores exact empty target`);
      console.log(`PASS ${theme}/${paper}/${caseData.name}/${region.side}: checked geometry bounds and exact Undo`);
    }
  };
  const settings=await evaluate('layerApp.state().settings');
  const workspace=await evaluate('layerApp.state().workspace');
  await evaluate(`window.enclosePickers={open:window.showOpenFilePicker,save:window.showSaveFilePicker};window.placementTest={};
    window.showOpenFilePicker=async()=>[{name:'Reference ink.png',async getFile(){return new File([Uint8Array.from(atob('${fixture.toString('base64')}'),c=>c.charCodeAt(0))],'Reference ink.png',{type:'image/png'})}}];
    window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){return{async write(value){placementTest.saved=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){},async abort(){}}}});`);
  try {
    await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
    await send({type:'customize',action:{type:'set_panel_visible',panel:'tool_settings',visible:true}});
    for(const theme of ['light','dark']) {
      await invoke('open_document');
      await wait('!layerApp.documents.busy()&&layerApp.state().tabs.some(t=>t.active&&t.width===384&&t.height===256)&&layerApp.app.brush_ready()');
      await send({type:'set_theme',theme});
      const original=await output();assert.deepEqual(original.extent,[384,256]);
      const referencePackage=await save(),reference=rasterIdentity(referencePackage),referenceOriginals=sourceIdentity(referencePackage);
      await send({type:'layer',action:{op:'reference_selection'}});
      await send({type:'layer',action:{op:'new',group:false,clipped:false}});
      await invoke('fill');
      const toolset='.dock-group .brushes-control';
      assert.deepEqual(await evaluate('layerApp.state().tool_set.groups.map(item=>item.label)'),['Fill','Lasso fill','Enclose and Fill']);
      for(const label of ['Lasso fill','Enclose and Fill','Fill','Enclose and Fill'])
        await contact(`${toolset} .tool-groups [data-tool-choice="${label}"]`);
      assert.ok(await selected('enclose_fill'));
      assert.deepEqual(await evaluate('["selection_reference","selection_visible","selection_editing"].map(id=>layerApp.state().commands.find(c=>c.id===id).label)'),['Reference layers','All visible layers','This layer']);
      for(const source of ['selection_visible','selection_editing','selection_reference']){
        await chooseToolActionGroup({call,evaluate,settle},'selection-source',source);
        assert.ok(await selected(source));assert.ok(await selected('enclose_fill'),'source preserves Enclose variant');
      }
      const checklist='.dock-group [data-tool-checklist="reference-layers"]';
      const marks=()=>evaluate('layerApp.state().tool_extra.find(o=>o.Checklist?.id==="reference-layers").Checklist.items.map(i=>({label:i.label,selected:i.selected}))');
      const initialMarks=await marks();
      assert.deepEqual(initialMarks.map(i=>i.label),['Layer 3','Reference ink'],'Checklist presents the eligible paint layers');
      assert.deepEqual(initialMarks.map(i=>i.selected),[false,true]);
      await contact(`${checklist} input`);
      assert.deepEqual((await marks()).map(i=>i.selected),[true,true],'Checking one layer preserves the other mark');
      assert.equal(packageOccurrences(await save()).filter(o=>o.data.reference).length,2,'Both reference marks are saved');
      await invoke('undo');await idle();assert.deepEqual(await marks(),initialMarks,'One undo restores the reference mark');
      await invoke('redo');await idle();assert.deepEqual((await marks()).map(i=>i.selected),[true,true]);
      await invoke('undo');await idle();
      await evaluate(`document.querySelector('.dock-group [data-tool-choice-bar="selection-source"]').scrollIntoView({block:'nearest'})`);await settle();
      await writeFile(`${directory}/controls-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      const controls=await evaluate('layerApp.state().tool_settings.map(c=>c.id)');
      for(const id of ['tolerance']){
        assert.ok(controls.includes(id),`${id} is shared with Fill`);
        assert.ok(await evaluate(`!!document.querySelector('[data-tool-setting="${id}"]')`),`${id} has a native Web control`);
      }
      await chooseToolActionGroup({call,evaluate,settle},'close-gaps','close_gaps_off');
      await chooseToolActionGroup({call,evaluate,settle},'region-edges','edges_grow_shrink');
      assert.ok(await evaluate('layerApp.state().tool_settings.some(c=>c.id==="edge_offset")'));
      const placement=await evaluate(`(()=>{const a=document.querySelector('[data-tool-choice-bar="region-edges"]').getBoundingClientRect(),b=document.querySelector('[data-tool-setting="edge_offset"]').getBoundingClientRect();return{bar:{left:a.left,right:a.right,bottom:a.bottom},value:{left:b.left,right:b.right,top:b.top}}})()`);
      assert.ok(placement.value.top>=placement.bar.bottom&&placement.value.left>=placement.bar.left-.5&&placement.value.right<=placement.bar.right+.5,'Grow or shrink value sits below Edges within its width');
      await chooseToolActionGroup({call,evaluate,settle},'region-edges','edges_exact');
      assert.equal(await evaluate('layerApp.state().tool_settings.some(c=>["gap_closing","expansion","edge_offset"].includes(c.id))'),false);
      for(const [id,value] of [['tolerance',.15],['grain_depth',0]])await send({type:'set_tool_setting',id,value});
      if(await selected('smooth_region_edges'))await contact('[data-tool-action="smooth_region_edges"]');
      assert.equal(await selected('smooth_region_edges'),false);
      await send({type:'set_color',rgba:[.85,.1,.05,1]});await invoke('fit_canvas');await idle();
      const empty=rasterIdentity(await save());
      const revision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');
      await path();await wait(`String(layerApp.state().layers.find(l=>l.editing).paint_revision)!==${JSON.stringify(revision)}`);
      const filledPackage=await save(),filled=rasterIdentity(filledPackage);
      assert.deepEqual(sourceIdentity(filledPackage),referenceOriginals,'imported reference samples stay exact');
      for(const source of reference)assert.deepEqual(filled.find(s=>s.id===source.id),source,'reference samples stay exact');
      const pixels=await output();assert.deepEqual(pixels.extent,[384,256]);
      const sample=(x,y)=>pixels.pixels.slice((y*384+x)*4,(y*384+x)*4+4);
      for(const [x,y] of [[85,110],[195,110]]){const rgba=sample(x,y);assert.ok(rgba[0]>180&&rgba[1]<80&&rgba[3]===255,`enclosed hole ${x},${y}: ${rgba}`);}
      for(const [x,y] of [[25,110],[140,110],[290,110],[85,195]])assert.equal(sample(x,y)[3],0,`exterior stays transparent at ${x},${y}`);
      assert.deepEqual(sample(42,110),[0,0,0,255],'reference ink remains visible');
      await writeFile(`${directory}/${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),empty,'one undo removes both holes');
      await invoke('redo');await idle();assert.deepEqual(rasterIdentity(await save()),filled,'redo restores exact fill samples');
      await invoke('undo');await idle();await path(true);
      assert.deepEqual(rasterIdentity(await save()),empty,'Escape cancels the enclosure');
      await invoke('redo');await idle();assert.deepEqual(rasterIdentity(await save()),filled,'cancel preserves redo');
      await invoke('undo');await idle();
      await contact(`${toolset} .tool-groups [data-tool-choice="Fill"]`);
      await chooseToolActionGroup({call,evaluate,settle},'selection-source','selection_reference');
      for(const [mode,other] of [['fill_connected',false],['fill_everywhere',true]]) {
        await chooseToolActionGroup({call,evaluate,settle},'fill-scope',mode);
        await pointer('mousePressed',await screen([85,110]));await pointer('mouseReleased',await screen([85,110]));await settle();await idle();
        const result=await output(),rgba=([x,y])=>result.pixels.slice((y*384+x)*4,(y*384+x)*4+4);
        for(const point of [[85,110],...(other?[[195,110],[290,110],[25,110]]:[])])assert.ok(rgba(point)[0]>180&&rgba(point)[1]<80,`${mode}: ${point} fills`);
        if(!other)assert.deepEqual(rgba([195,110]),[0,0,0,0],'Connected leaves the other component empty');
        const after=rasterIdentity(await save());
        await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),empty,`${mode}: one undo restores the target`);
        await invoke('redo');await idle();assert.deepEqual(rasterIdentity(await save()),after,`${mode}: exact redo`);
        await invoke('undo');await idle();
      }
      const targetBeforeEdit=await evaluate('Number(layerApp.state().layers.find(l=>l.editing).id)');
      const referenceId=await evaluate('Number(layerApp.state().layers.find(l=>l.label==="Reference ink").id)');
      const targetRaster=empty.find(source=>!reference.some(original=>original.id===source.id));assert.ok(targetRaster);
      await chooseToolActionGroup({call,evaluate,settle},'fill-scope','fill_connected');
      for(const [from,to,seed,other] of [[[85,50],[85,170],[65,110],[105,110]],[[40,110],[130,110],[85,85],[85,135]]]){
        await send({type:'layer',action:{op:'select',id:referenceId,mask:false}});
        await send({type:'select_brush',id:2});await send({type:'set_brush_size',value:8});await send({type:'set_color',rgba:[0,0,0,1]});
        const revision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');
        await scribblePath([from,to]);
        const editedRevision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');
        assert.notEqual(editedRevision,revision,'Native reference edit commits');
        const point=await screen(seed);
        await evaluate(`layerApp.dispatch({type:'set_color',rgba:[.85,.1,.05,1]});layerApp.dispatch({type:'layer',action:{op:'select',id:${targetBeforeEdit},mask:false}});layerApp.dispatch({type:'invoke',command:'fill'})`);
        await pointer('mousePressed',point);await pointer('mouseReleased',point);await idle();
        const result=await output(),alpha=([x,y])=>result.pixels[(y*384+x)*4+3];
        assert.equal(alpha(seed),255);assert.equal(alpha(other),0,'Fill uses the edited reference divider');
        assert.equal(await evaluate(`String(layerApp.state().layers.find(l=>Number(l.id)===${referenceId}).paint_revision)`),editedRevision,'Fill preserves the edited reference');
        await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()).find(source=>source.id===targetRaster.id),targetRaster,'One Undo removes the promoted Fill');
        if(from[0]===to[0]){
          const start=await screen([65,110]),end=await screen([105,110]);
          await pointer('mousePressed',start);await pointer('mouseMoved',end);await pointer('mouseReleased',end);await idle();
          const dragged=await output();for(const x of [65,105])assert.equal(dragged.pixels[(110*384+x)*4+3],255,`Drag-through fills the adjacent face at ${x},110`);
          assert.equal(dragged.pixels[(110*384+195)*4+3],0,'Drag-through leaves an uncrossed face empty');
          await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()).find(source=>source.id===targetRaster.id),targetRaster,'One Undo removes the complete drag-through');
        }
        await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),empty,'Second Undo restores the reference edit');
        await send({type:'layer',action:{op:'select',id:targetBeforeEdit,mask:false}});
      }
      console.log(`PASS ${theme}: edited cached reference, immediate Fill, exact fill/edit Undo`);
      const press=await screen([85,110]);await pointer('mousePressed',press,'pen');await pressPreview(await screen([95,120]),theme);await pointer('mouseReleased',press,'pen');await idle();
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),empty,'One Undo removes the press fill');
      await send({type:'layer',action:{op:'select',id:referenceId,mask:false}});await send({type:'select_brush',id:3});await send({type:'set_brush_size',value:48});
      const cutRevision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');await scribblePath([[85,40],[85,66]]);
      assert.notEqual(await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)'),cutRevision,'Native eraser creates the gap');
      await send({type:'layer',action:{op:'select',id:targetBeforeEdit,mask:false}});await invoke('fill');await send({type:'set_color',rgba:[.85,.1,.05,1]});
      await chooseToolActionGroup({call,evaluate,settle},'close-gaps','close_gaps_auto');
      const gapBase=rasterIdentity(await save()),gapLabel=await evaluate('layerApp.state().tool_extra.find(o=>o.Choice?.id==="close-gaps").Choice.label');
      await pointer('mousePressed',press);let held=await heldPill(gapLabel);const transaction=String(held.transaction),track=held.pill.track;
      for(const [index,reach] of [0,16,32,48,64].entries()){
        await pointer('mouseMoved',await trackPoint(track[0]+(track[1]-track[0])*index/4,press.y));
        await wait(`(()=>{const p=layerApp.app.color_preview().fill_gesture?.pill;return p&&${reach===0?'p.value===layerApp.state().commands.find(c=>c.id==="close_gaps_off").label':`parseFloat(p.value)===${reach}`}})()`);
        held=await heldPill(gapLabel);assert.equal(String(held.transaction),transaction);
      }
      assert.ok(held.seals.length>0,'The gap hold presents actual seals');
      await writeFile(`${directory}/hold-close-gaps-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      await pointer('mouseReleased',await trackPoint(track[1],press.y));await idle();
      assert.equal(parseFloat(await evaluate('layerApp.state().tool_extra.find(o=>o.Choice?.id==="close-gaps").Choice.items[0].label')),64,'Close gaps hold remembers the selected reach');
      const firstFill=rasterIdentity(await save()),seals=(await preview()).seals,neighbour=await screen([195,110]),origin=await evaluate('(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x,y:r.y}})()');
      assert.ok(seals.length>0);
      for(const [a,b]of seals){const x=neighbour.x-origin.x,y=neighbour.y-origin.y,dx=b[0]-a[0],dy=b[1]-a[1],t=Math.max(0,Math.min(1,((x-a[0])*dx+(y-a[1])*dy)/Math.max(Number.EPSILON,dx*dx+dy*dy)));assert.ok(Math.hypot(x-a[0]-t*dx,y-a[1]-t*dy)>6,'Neighbour press is outside every seal hit radius');}
      await pointer('mousePressed',neighbour);await pointer('mouseReleased',neighbour);await idle();
      const neighbouring=await output();for(const x of [85,195])assert.equal(neighbouring.pixels[(110*384+x)*4+3],255,'Normal neighbouring fill preserves the previous region');
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),firstFill,'Neighbour press creates a normal fill instead of amending the previous fill');
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),gapBase,'The first gap fill has its own Undo');
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),empty,'Restore the authored eraser cut');
      await send({type:'layer',action:{op:'select',id:targetBeforeEdit,mask:false}});await chooseToolActionGroup({call,evaluate,settle},'close-gaps','close_gaps_off');
      console.log(`PASS ${theme}: press preview, Close gaps hold, drag-through, neighbouring seal guard and single Undo`);

      const paperLayer=await evaluate('Number(layerApp.state().layers.find(l=>l.content_icon==="layer-paper-symbolic")?.id)');assert.ok(Number.isFinite(paperLayer),'Imported drawing supplies its default Paper');
      await send({type:'layer',action:{op:'delete',id:paperLayer}});
      assert.ok(await evaluate('!!layerApp.state().layer_tools.add_paper'),'No Paper fixture removes its Paper');
      const target=await evaluate('Number(layerApp.state().layers.find(l=>l.editing).id)');
      for(const paper of [false,true]) {
        if(paper) {
          const action=await evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_tools.add_paper.action,(_,v)=>typeof v==="bigint"?String(v):v))');
          await send({type:'effect',action});await idle();
          await send({type:'layer',action:{op:'select',id:target,mask:false}});
        }
        for(const command of ['fill','lasso_fill','enclose_fill','gradient']) {
          await invoke(command);await idle();
          const control=await evaluate('layerApp.state().tool_settings.find(c=>c.id==="grain_depth")');
          assert.ok(control,`${command}: Texture strength exists`);assert.equal(control.numeric.max,2);
          const results=[];
          for(const strength of [0,1,2]) {
            await send({type:'set_tool_setting',id:'grain_depth',value:strength});
            assert.equal(await evaluate('layerApp.state().tool_settings.find(c=>c.id==="grain_depth").value'),strength);
            const before=rasterIdentity(await save());
            if(command==='fill') {
              await pointer('mousePressed',await screen([85,110]));await pointer('mouseReleased',await screen([85,110]));await settle();
            } else if(command==='gradient') {
              await pointer('mousePressed',await screen([60,80]));await pointer('mouseMoved',await screen([120,140]));await pointer('mouseReleased',await screen([120,140]));await settle();
            } else await path();
            await idle();const after=rasterIdentity(await save());results.push(await output());
            if(paper&&strength===1)await writeFile(`${directory}/texture-${command}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
            assert.notDeepEqual(after,before,`${command}/${paper}/${strength}: deposits paint`);
            await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),before,`${command}/${paper}/${strength}: one undo`);
            await invoke('redo');await idle();assert.deepEqual(rasterIdentity(await save()),after,`${command}/${paper}/${strength}: exact redo`);
            await invoke('undo');await idle();
          }
          if(!paper)for(const result of results.slice(1))assert.deepEqual(result,results[0],`${command}: No paper is bit-exact flat at every strength`);
          else for(const [index,result] of results.entries())if(index)assert.notDeepEqual(result,results[0],`${command}: paper changes coverage at ${index*100}%`);
        }
      }
      const colours=png(384,256,(x,y)=>x<128?[64,64,64,255]:x<256?[80,80,80,255]:[160,160,160,255],4);
      await evaluate(`window.enclosePickers.fixture=window.showOpenFilePicker;window.showOpenFilePicker=async()=>[{name:'Fill colours.png',async getFile(){return new File([Uint8Array.from(atob('${colours.toString('base64')}'),c=>c.charCodeAt(0))],'Fill colours.png',{type:'image/png'})}}]`);
      await invoke('open_document');await wait('!layerApp.documents.busy()&&layerApp.state().layers.some(l=>l.editing&&l.label==="Fill colours")&&layerApp.app.brush_ready()');await idle();await evaluate('window.showOpenFilePicker=enclosePickers.fixture;delete enclosePickers.fixture');await invoke('fit_canvas');await invoke('fill');
      await chooseToolActionGroup({call,evaluate,settle},'selection-source','selection_editing');
      await send({type:'set_color',rgba:[.85,.1,.05,1]});await send({type:'set_tool_setting',id:'grain_depth',value:0});await send({type:'set_tool_setting',id:'tolerance',value:.1});
      const colourBase=rasterIdentity(await save()),toleranceLabel=await evaluate('layerApp.state().tool_settings.find(f=>f.id==="tolerance").label'),colourPoint=await screen([85,110]);
      await pointer('mousePressed',colourPoint,'pen');const tolerance=await heldPill(toleranceLabel),range=tolerance.pill.track;
      for(const fraction of [.8,.2])await pointer('mouseMoved',await trackPoint(range[0]+(range[1]-range[0])*fraction,colourPoint.y),'pen');
      await wait('parseFloat(layerApp.app.color_preview().fill_gesture?.pill?.value)===20');await heldPill(toleranceLabel);
      await writeFile(`${directory}/hold-tolerance-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      await pointer('mouseReleased',await trackPoint(range[0]+(range[1]-range[0])*.2,colourPoint.y),'pen');await idle();
      assert.ok(Math.abs(await evaluate('layerApp.state().tool_settings.find(f=>f.id==="tolerance").value')-.2)<.00001,'Tolerance hold remembers the newest value');
      const recoloured=await output();for(const x of [85,250]){const i=(110*384+x)*4;assert.ok(recoloured.pixels[i]>180&&recoloured.pixels[i+1]<80,'Newest tolerance result is committed');}
      assert.deepEqual(recoloured.pixels.slice((110*384+320)*4,(110*384+320)*4+4),[160,160,160,255],'A stale higher-tolerance result cannot recolour the far band');
      await invoke('undo');await idle();assert.deepEqual(rasterIdentity(await save()),colourBase,'One Undo removes the tolerance fill');
      await pointer('mousePressed',colourPoint,'pen');const cancelling=await heldPill(toleranceLabel),cancelRange=cancelling.pill.track;
      await pointer('mouseMoved',await trackPoint(cancelRange[0]+(cancelRange[1]-cancelRange[0])*.6,colourPoint.y),'pen');await wait('parseFloat(layerApp.app.color_preview().fill_gesture?.pill?.value)===60');
      for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await pointer('mouseReleased',colourPoint,'pen');await idle();
      assert.ok(Math.abs(await evaluate('layerApp.state().tool_settings.find(f=>f.id==="tolerance").value')-.2)<.00001,'Escape restores the held option');
      assert.deepEqual(rasterIdentity(await save()),colourBase,'Escape adds no fill history');assert.equal(await evaluate('layerApp.canvas.classList.contains("fill-gesture-resize")'),false);
      console.log(`PASS ${theme}: Tolerance hold newest-value publication and exact single Undo`);
      await invoke('new_document');await wait('!!layerApp.state().new_drawing');
      await contact('#new-drawing-presets-button');
      const digital=await evaluate('layerApp.state().new_drawing.drawer.tabs.findIndex(t=>t.label==="Digital")');assert.ok(digital>=0);
      await contact(`#new-drawing-tabs [data-new-drawing-tab="${digital}"]`);
      await contact('#new-drawing-search');await call('Input.insertText',{text:'Full HD'});await settle();
      assert.equal(await evaluate('layerApp.state().new_drawing.drawer.query'),'Full HD');
      const preset=await evaluate('layerApp.state().new_drawing.drawer.tiles.findIndex(t=>t.name==="Full HD")');assert.ok(preset>=0);
      await contact(`#new-drawing-presets [data-new-drawing-preset="${preset}"]`);await contact('#new-drawing-select');
      assert.equal(await evaluate('layerApp.state().new_drawing.setup.paper.texture??null'),null,'Digital Full HD uses No paper');
      await wait(`(()=>{const n=document.querySelector('#new-drawing-create'),r=n.getBoundingClientRect();return !layerApp.state().new_drawing.drawer&&n.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))})()`);
      await contact('#new-drawing-create');await wait('!layerApp.state().new_drawing');await idle();
      assert.ok(await evaluate('!!layerApp.state().layer_tools.add_paper'),'Digital drawing has no Paper layer');
      for(const paper of ['cold_press','rough'])await scribblePaper(theme,paper);
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      console.log(`PASS ${theme}: Enclose and Fill reference holes, untouched ink/exterior/reference, exact single undo/redo and Escape`);
    }
  } finally {
    await send({type:'restore_settings',settings});
    await send({type:'restore_workspace',workspace});
    await evaluate('window.showOpenFilePicker=enclosePickers.open;window.showSaveFilePicker=enclosePickers.save;delete window.enclosePickers;delete window.placementTest');
  }
}
