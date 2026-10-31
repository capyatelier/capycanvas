import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {png} from './clone-journey.test.mjs';
import {placementSave} from './image-placement-motion.test.mjs';
import {rasterIdentity,sourceIdentity,packageOccurrences} from './package-fixture.test.mjs';
import {chooseToolActionGroup} from './panel-controls-journey.test.mjs';

export async function checkEncloseFill({call,evaluate,settle}) {
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
  const pointer=(type,p)=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
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
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Dynamic range"]')`);
    await click('Preview Output');await wait(`!!document.querySelector('canvas[aria-label="Output preview"]')&&!Array.from(document.querySelectorAll('dialog[open] button')).find(b=>b.textContent==='Preview Output').disabled`);
    const pixels=await evaluate(`(()=>{const c=document.querySelector('canvas[aria-label="Output preview"]');return{extent:[c.width,c.height],pixels:Array.from(c.getContext('2d').getImageData(0,0,c.width,c.height).data)}})()`);
    await click('Cancel');await idle();return pixels;
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
      await evaluate(`document.querySelector('.dock-group [data-tool-action-group="selection-source"]').scrollIntoView({block:'nearest'})`);await settle();
      await writeFile(`${directory}/controls-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      const controls=await evaluate('layerApp.state().tool_settings.map(c=>c.id)');
      for(const id of ['tolerance','gap_closing','expansion']){
        assert.ok(controls.includes(id),`${id} is shared with Fill`);
        assert.ok(await evaluate(`!!document.querySelector('[data-tool-setting="${id}"]')`),`${id} has a native Web control`);
      }
      for(const [id,value] of [['tolerance',.15],['gap_closing',0],['expansion',0],['grain_depth',0]])await send({type:'set_tool_setting',id,value});
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
      await invoke('new_document');await wait('!!layerApp.state().new_drawing');
      await contact('#new-drawing-presets-button');
      const digital=await evaluate('layerApp.state().new_drawing.drawer.tabs.findIndex(t=>t.label==="Digital")');assert.ok(digital>=0);
      await contact(`#new-drawing-tabs [data-new-drawing-tab="${digital}"]`);
      await send({type:'new_drawing',action:{op:'search',query:'Full HD'}});
      const preset=await evaluate('layerApp.state().new_drawing.drawer.tiles.findIndex(t=>t.name==="Full HD")');assert.ok(preset>=0);
      await contact(`#new-drawing-presets [data-new-drawing-preset="${preset}"]`);await contact('#new-drawing-select');
      assert.equal(await evaluate('layerApp.state().new_drawing.setup.paper.texture??null'),null,'Digital Full HD uses No paper');
      await wait(`(()=>{const n=document.querySelector('#new-drawing-create'),r=n.getBoundingClientRect();return !layerApp.state().new_drawing.drawer&&n.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))})()`);
      await contact('#new-drawing-create');await wait('!layerApp.state().new_drawing');await idle();
      assert.ok(await evaluate('!!layerApp.state().layer_tools.add_paper'),'Digital drawing has no Paper layer');
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      console.log(`PASS ${theme}: Enclose and Fill reference holes, untouched ink/exterior/reference, exact single undo/redo and Escape`);
    }
  } finally {
    await send({type:'restore_settings',settings});
    await send({type:'restore_workspace',workspace});
    await evaluate('window.showOpenFilePicker=enclosePickers.open;window.showSaveFilePicker=enclosePickers.save;delete window.enclosePickers;delete window.placementTest');
  }
}
