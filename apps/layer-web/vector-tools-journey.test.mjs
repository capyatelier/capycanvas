import assert from 'node:assert/strict';
import {vectorHarness} from './vector-journey-fixture.mjs';
import {createDrawing} from './new-drawing-fixture.mjs';
import {placementSave} from './image-placement-motion.test.mjs';
import {authoredIdentity,rasterIdentity,packageOccurrences,packageObject} from './package-fixture.test.mjs';

export async function checkVectorTools({call,evaluate,settle}) {
  const {send,invoke,tool,stroke,line,key,color:sampleColor,capture,until}=await vectorHarness({call,evaluate,settle},{directory:'artifacts/vector-tools/web',keyDown:'keyDown'});
  const color=at=>sampleColor(at,true);
  const orange=([r,,b])=>r-b>90;
  const green=([r,g,b])=>g-Math.max(r,b)>25;
  const expect=async(name,at,inked,ink=orange)=>{
    const end=Date.now()+20000;let seen;
    do{seen=await color(at);if(ink(seen)===inked)return;await new Promise(r=>setTimeout(r,100));}while(Date.now()<end);
    assert.equal(ink(seen),inked,`${name}: ${seen}`);
  };
  const selectedLayer=()=>evaluate('Number(layerApp.state().layers.find(l=>l.selected).id)');
  await invoke('new_document');
  await evaluate(createDrawing({width:2000,height:1400}));
  await until('the vector drawing is ready','!layerApp.documents.busy()&&!s.document_file.busy&&layerApp.app.brush_ready()',60000);
  await settle();
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    await send({type:'layer',action:{op:'new_vector'}});
    await invoke('fit_canvas');
    await send({type:'set_color',rgba:[.85,.35,.1,1]});
    await send({type:'set_brush_size',value:24});

    await tool({figure:{shape:'rectangle',paint:'fill'}});
    await stroke(line([400,400],[700,620]));
    await expect(`${theme} rectangle`,[550,510],true);
    await expect(`${theme} beside the rectangle`,[780,510],false);
    await capture(`vector-${theme}-figure`);

    await tool({figure:{shape:'path',paint:'outline'}});
    for(const at of [[1000,400],[1300,400]])await stroke([at,at]);
    await stroke(line([1300,700],[1400,780],4));
    await capture(`vector-${theme}-path-drafting`);
    await key('Enter','Enter',13);
    await expect(`${theme} path`,[1150,400],true);
    await capture(`vector-${theme}-path`);

    await invoke('edit_points');
    await expect(`${theme} before editing`,[1150,325],false);
    await stroke(line([1000,400],[1000,250]));
    await expect(`${theme} edited anchor`,[1150,325],true);
    await capture(`vector-${theme}-edit-points`);

    await expect(`${theme} before restyling`,[1300,550],false);
    await send({type:'effect',action:{op:'set',layer:await selectedLayer(),key:'path_width',value:{kind:'number',value:160}}});
    await expect(`${theme} restyled`,[1300,550],true);
    await capture(`vector-${theme}-properties`);

    await invoke('move');
    await stroke(line([350,350],[750,670]));
    await capture(`vector-${theme}-frame`);
    await expect(`${theme} before scaling`,[780,700],false);
    await stroke(line([700,620],[850,760]));
    await expect(`${theme} scaled`,[780,700],true);
    await capture(`vector-${theme}-scaled`);

    await send({type:'select_brush',id:1});
    await send({type:'set_color',rgba:[.1,.7,.2,1]});
    await stroke(line([300,1100],[1700,1100],40));
    await expect(`${theme} inked`,[1000,1100],true,green);
    await invoke('eraser');
    await send({type:'set_brush_size',value:40});
    await stroke(line([1000,1000],[1000,1200]));
    await expect(`${theme} erased`,[1000,1100],false,green);
    await expect(`${theme} kept`,[700,1100],true,green);
    await expect(`${theme} kept beyond the eraser`,[1300,1100],true,green);
    await capture(`vector-${theme}-erased`);
    await send({type:'layer',action:{op:'delete_selected'}});
    await invoke('pen');
  }
}

export async function checkVectorFill({call,evaluate,settle}) {
  const {send,invoke,tool,state,until,idle,line,draw,color,park,capture,brush,activeVector,screen,mouseEvent}=await vectorHarness({call,evaluate,settle},{directory:'artifacts/vector-fill/web'});
  const save=placementSave({evaluate,invoke,idle});
  const gesture=()=>evaluate('JSON.parse(JSON.stringify(layerApp.app.color_preview().fill_gesture,(_,value)=>typeof value==="bigint"?String(value):value))');
  const pointer=(type,point)=>mouseEvent(type,point,'pen',{buttons:type==='mouseReleased'?0:1});
  const presented=async()=>{await evaluate('layerApp.app.wait_for_canvas()');await settle();};
  const shownSeal=async()=>{
    const view=await gesture();assert.ok(view.seals.length>0,'Committed vector fill shows its actual seals');
    const [a,b]=view.seals[0],origin=await evaluate('(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x,y:r.y}})()');
    return {x:origin.x+(a[0]+b[0])/2,y:origin.y+(a[1]+b[1])/2};
  };

  await evaluate(`window.vectorFillSave=window.showSaveFilePicker;window.placementTest={};window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){return{async write(value){placementTest.saved=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){},async abort(){}}}})`);
  try {
    for(const theme of ['light','dark']){
      await invoke('new_document');await evaluate(createDrawing({width:640,height:480}));await idle();await send({type:'set_theme',theme});
      await send({type:'layer',action:{op:'new_vector'}});await until('vector target',activeVector);await invoke('fit_canvas');
      await send({type:'select_brush',id:await brush('G-Pen')});await send({type:'set_brush_size',value:8});await send({type:'set_color',rgba:[0,0,0,1]});await idle();
      for(const [from,to]of [[[160,120],[440,120]],[[440,120],[440,360]],[[440,360],[160,360]],[[160,360],[160,120]]]){await draw('pen',line(from,to));await idle();}
      const target=await state('Number(s.layers.find(layer=>layer.editing).id)'),label=await state('s.layers.find(layer=>layer.editing).label'),layers=await state('s.layers.map(layer=>Number(layer.id))'),before=await save(),outside=await color([500,240],true);
      const vectorId=packageOccurrences(before).find(occurrence=>occurrence.data.name===label).data.content.object.ref,vector=manifest=>packageObject(manifest,vectorId),next=BigInt(vector(before).data.next_item);assert.equal(vector(before).type,'capy.vector-object/1');
      await tool({region:{fill:true,source:'editing'}});await send({type:'set_color',rgba:[.1,.75,.2,1]});await draw('pen',[[300,240]]);await idle();await park();await settle();
      const filled=await save();assert.equal(BigInt(vector(filled).data.next_item),next+1n,'A face tap creates one vector path');
      assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),layers,'Vector lines create no hidden paint layer');assert.equal(await state('Number(s.layers.find(layer=>layer.editing).id)'),target);assert.deepEqual(rasterIdentity(filled),rasterIdentity(before),'Vector Fill does not author raster tiles');
      assert.deepEqual(await color([500,240],true),outside,'Outside the vector face stays unchanged');const green=await color([300,240],true);assert.ok(green[1]>green[0]+60&&green[1]>green[2]+60,`${theme}: face is green ${green}`);await capture(`vector-fill-${theme}-created`);
      await send({type:'set_color',rgba:[.8,.1,.2,1]});await draw('pen',[[300,240]]);await idle();await park();await settle();
      const recoloured=await save();assert.equal(BigInt(vector(recoloured).data.next_item),next+1n,'Recolour reuses the filled path');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),layers);
      const red=await color([300,240],true);assert.ok(red[0]>red[1]+60&&red[0]>red[2]+60,`${theme}: face is recoloured ${red}`);await capture(`vector-fill-${theme}-recoloured`);
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(filled),'One Undo restores the original face colour');
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(before),'Second Undo removes only the new face');
      await invoke('redo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(filled),'Redo restores the vector face');await capture(`vector-fill-${theme}-undo-redo`);
      await invoke('undo');await idle();await tool({enclose_fill:{source:'editing'}});await send({type:'set_color',rgba:[.1,.2,.85,1]});
      await draw('pen',[[140,100],[460,100],[460,380],[140,380],[140,100]]);await until('vector enclosure published','layerApp.app.document_park_ready()');
      const enclosed=await save();assert.equal(BigInt(vector(enclosed).data.next_item),next+1n,'Vector enclosure creates one face path');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),layers);assert.deepEqual(rasterIdentity(enclosed),rasterIdentity(before));
      await park();await settle();const blue=await color([300,240],true);assert.ok(blue[2]>blue[0]+60&&blue[2]>blue[1]+60,`${theme}: enclosed vector face is blue ${blue}`);await capture(`vector-fill-${theme}-enclosed`);
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(before),'One Undo removes the vector enclosure');
      await invoke('new_document');await evaluate(createDrawing({width:640,height:480}));await idle();await send({type:'layer',action:{op:'new',group:false,clipped:false}});await invoke('fit_canvas');
      await send({type:'select_brush',id:await brush('G-Pen')});await send({type:'set_brush_size',value:8});await send({type:'set_color',rgba:[0,0,0,1]});await tool({figure:{shape:'rectangle',paint:'outline'}});await draw('pen',line([160,120],[440,360]));await idle();await until('raster reference published','layerApp.app.document_park_ready()');
      await send({type:'layer',action:{op:'reference_selection'}});await send({type:'layer',action:{op:'new_vector'}});await until('vector colour layer',activeVector);await invoke('lower_layer');
      const traceTarget=await state('Number(s.layers.find(layer=>layer.editing).id)'),traceLabel=await state('s.layers.find(layer=>layer.editing).label'),traceLayers=await state('s.layers.map(layer=>Number(layer.id))'),traceBefore=await save();
      const traceId=packageOccurrences(traceBefore).find(occurrence=>occurrence.data.name===traceLabel).data.content.object.ref,traceVector=manifest=>packageObject(manifest,traceId);assert.equal((traceVector(traceBefore).data.items??[]).length,0,'Trace starts on an empty vector layer');
      await tool({region:{fill:true,source:'reference'}});await send({type:'set_color',rgba:[.1,.75,.2,1]});await draw('pen',[[300,240]]);await until('traced fill published','layerApp.app.document_park_ready()');
      const traced=await save();assert.equal(BigInt(traceVector(traced).data.next_item),2n,'Raster region becomes one vector path');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),traceLayers,'Raster tracing creates no layer');assert.equal(await state('Number(s.layers.find(layer=>layer.editing).id)'),traceTarget);assert.deepEqual(rasterIdentity(traced),rasterIdentity(traceBefore),'Tracing preserves raster sources');
      await park();await settle();const tracedGreen=await color([300,240],true);assert.ok(tracedGreen[1]>tracedGreen[0]+60&&tracedGreen[1]>tracedGreen[2]+60,`${theme}: traced vector face is green ${tracedGreen}`);await capture(`vector-fill-${theme}-raster-reference`);
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(traceBefore),'One Undo removes the traced path');
      await invoke('new_document');await evaluate(createDrawing({width:640,height:480}));await idle();await send({type:'layer',action:{op:'new_vector'}});await until('gappy vector target',activeVector);await invoke('fit_canvas');
      await send({type:'select_brush',id:await brush('G-Pen')});await send({type:'set_brush_size',value:8});await send({type:'set_color',rgba:[0,0,0,1]});await idle();
      for(const [from,to]of [[[100,100],[160,100]],[[200,100],[260,100]],[[260,100],[260,260]],[[260,260],[100,260]],[[100,260],[100,100]]]){await draw('pen',line(from,to));await idle();}
      const gapLabel=await state('s.layers.find(layer=>layer.editing).label'),gapLayers=await state('s.layers.map(layer=>Number(layer.id))'),gapBefore=await save();
      const gapId=packageOccurrences(gapBefore).find(occurrence=>occurrence.data.name===gapLabel).data.content.object.ref,gapVector=manifest=>packageObject(manifest,gapId),gapNext=BigInt(gapVector(gapBefore).data.next_item);
      await tool({region:{fill:true,source:'editing'}});await invoke('fill_connected');await invoke('close_gaps_auto');await send({type:'set_color',rgba:[.1,.75,.2,1]});
      const press=await screen([180,180]),unfilled=await color([190,190],true),origin=await evaluate('layerApp.canvas.getBoundingClientRect().x'),gapCaption=await state('s.tool_extra.find(o=>o.Choice?.id==="close-gaps").Choice.label');
      await pointer('mousePressed',press);await presented();
      assert.ok(!(await gesture()).pill,'The press preview precedes the held control');const preview=await color([190,190],true);assert.ok(preview[1]>preview[0]+20&&preview[1]>preview[2]+20&&preview[0]>100,`${theme}: half-alpha vector face is visible before lift: ${preview}`);await capture(`vector-fill-${theme}-press-preview`);
      await until('vector Close gaps hold','!!layerApp.app.color_preview().fill_gesture?.pill');
      const held=await gesture();assert.equal(held.pill.label,gapCaption);assert.equal(parseFloat(held.pill.value),48);assert.ok(await evaluate('document.querySelector(".fill-gesture-pill").checkVisibility()'));
      let release;
      for(const [slot,reach] of [[0,0],[3,48]]) {
        release={x:origin+held.pill.track[0]+(held.pill.track[1]-held.pill.track[0])*slot/4,y:press.y};await pointer('mouseMoved',release);await settle();
        await until(`vector gap ${reach}`,reach===0?'layerApp.app.color_preview().fill_gesture?.pill?.value===s.commands.find(c=>c.id==="close_gaps_off").label':'parseFloat(layerApp.app.color_preview().fill_gesture?.pill?.value)===48');await presented();
        const pixel=await color([190,190],true);if(reach===0)assert.deepEqual(pixel,unfilled,'Off removes the held face preview');else assert.ok(pixel[1]>pixel[0]+20&&pixel[1]>pixel[2]+20,'48 restores the held face preview');
        await capture(`vector-fill-${theme}-hold-${reach}`);
      }
      await pointer('mouseReleased',release);await settle();await until('Auto vector fill published','layerApp.app.document_park_ready()');
      const gapFilled=await save();assert.equal(BigInt(gapVector(gapFilled).data.next_item),gapNext+1n,'Auto closes the 40px vector gap with one fill path');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),gapLayers);assert.deepEqual(rasterIdentity(gapFilled),rasterIdentity(gapBefore));
      await park();await settle();const gapGreen=await color([180,180],true);assert.ok(gapGreen[1]>gapGreen[0]+60&&gapGreen[1]>gapGreen[2]+60,`${theme}: Auto fills the gappy face ${gapGreen}`);await capture(`vector-fill-${theme}-auto-gap`);
      const noFaceSeal=await shownSeal();await pointer('mousePressed',noFaceSeal);await pointer('mouseReleased',noFaceSeal);await settle();await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(gapFilled),'Removing the only seal without a fallback face leaves the fill unchanged');
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(gapBefore),'One Undo removes the Auto fill');await invoke('redo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(gapFilled));
      await tool({figure:{shape:'rectangle',paint:'fill'}});await send({type:'set_color',rgba:[.1,.75,.2,1]});
      for(const [from,to]of [[[330,100],[430,210]],[[100,330],[230,430]]]){await draw('pen',line(from,to));await idle();}
      await send({type:'set_color',rgba:[.1,.2,.85,1]});await draw('pen',line([330,300],[460,420]));await idle();
      const everywhereBefore=await save(),different=await color([395,360],true),everywhereNext=gapVector(everywhereBefore).data.next_item,samePaintPoints=[[180,180],[380,155],[165,380]];for(const point of samePaintPoints){const value=await color(point,true);assert.ok(value[1]>value[0]+60&&value[1]>value[2]+60,'Three regions start with the same green paint');}await capture(`vector-fill-${theme}-everywhere-before`);
      await tool({region:{fill:true,source:'editing'}});await invoke('fill_everywhere');await send({type:'set_color',rgba:[.8,.1,.2,1]});await draw('pen',[[180,180]]);await until('Everywhere vector recolour published','layerApp.app.document_park_ready()');
      const everywhereAfter=await save();assert.equal(gapVector(everywhereAfter).data.next_item,everywhereNext,'Everywhere recolours existing paths');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),gapLayers);assert.deepEqual(rasterIdentity(everywhereAfter),rasterIdentity(everywhereBefore));await park();await settle();
      for(const point of samePaintPoints){const value=await color(point,true);assert.ok(value[0]>value[1]+60&&value[0]>value[2]+60,`${theme}: Everywhere recolours same paint at ${point}: ${value}`);}assert.deepEqual(await color([395,360],true),different,'Everywhere preserves different paint');await capture(`vector-fill-${theme}-everywhere-after`);
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(everywhereBefore),'One Undo restores all three same-paint regions');await capture(`vector-fill-${theme}-everywhere-undo`);await invoke('fill_connected');
      for(let i=0;i<4;i++){await invoke('undo');await idle();}assert.deepEqual(authoredIdentity(await save()),authoredIdentity(gapBefore));
      await send({type:'select_brush',id:await brush('G-Pen')});await send({type:'set_color',rgba:[0,0,0,1]});
      for(const [from,to]of [[[70,70],[290,70]],[[290,70],[290,290]],[[290,290],[70,290]],[[70,290],[70,70]]]){await draw('pen',line(from,to));await idle();}
      const framed=await save(),fallbackBefore=await color([85,180],true);await tool({region:{fill:true,source:'editing'}});assert.equal(parseFloat(await state('s.tool_extra.find(o=>o.Choice?.id==="close-gaps").Choice.items[0].label')),48,'The framed fill retains the settled 48px reach');await send({type:'set_color',rgba:[.1,.75,.2,1]});await draw('pen',[[180,180]]);await until('framed vector fill published','layerApp.app.document_park_ready()');
      const sealed=await save();assert.deepEqual(await color([85,180],true),fallbackBefore,'The initial face stays inside the gappy contour');
      const seal=await shownSeal();await pointer('mousePressed',seal);await pointer('mouseReleased',seal);await settle();await until('vector seal amendment published','layerApp.app.document_park_ready()');
      const amended=await save();assert.equal(gapVector(amended).data.next_item,gapVector(sealed).data.next_item,'Seal removal reuses the filled path');assert.deepEqual(await state('s.layers.map(layer=>Number(layer.id))'),gapLayers);assert.deepEqual(rasterIdentity(amended),rasterIdentity(framed));
      await park();await settle();const fallback=await color([85,180],true);assert.ok(fallback[1]>fallback[0]+60&&fallback[1]>fallback[2]+60,'Removing the shown seal refills the outer closed face');await capture(`vector-fill-${theme}-seal-amended`);
      await invoke('undo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(framed),'One Undo removes the original fill and its seal amendment');await invoke('redo');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(amended),'Redo restores the amended face');
      console.log(`PASS ${theme}: vector baseline, Enclose/trace, 40px preview/hold/lift, shown-seal amendment, Everywhere and exact Undo without raster/layer edits`);
    }
  } finally {await evaluate('window.showSaveFilePicker=window.vectorFillSave;delete window.vectorFillSave;delete window.placementTest');}
}
