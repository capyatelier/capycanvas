import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Reproduce growth of the left region at a fixed window width. A compact
// Menu Labels component must retain its own body/grip and its visible neighbors.
export async function checkMenuLabels({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/title-bar/menu-labels';await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const edit=action=>send({type:'customize',action:{type:'header',action}});
  const model=()=>evaluate('layerApp.state().workspace.layout.header');
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Missing visible '+${JSON.stringify(selector)});return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await settle();};
  const shot=async name=>{const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(s.data,'base64'));};
  let device='mouse',point,pressed=false;
  const pointer=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.6});
    pressed=type!=='up';await settle();
  };
  const click=async selector=>{const input=device;device='mouse';await pointer('down',center(await rect(selector)));await pointer('up');device=input;};
  const item=id=>`#header-item-${id}`;
  const visible=async id=>{
    const result=await evaluate(`(()=>{const n=document.querySelector('${item(id)}'),r=n.getBoundingClientRect(),g=n.querySelector('.header-item-grip').getBoundingClientRect();return{hidden:n.hidden,grip:g.width,hit:n.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)),gripHit:n.querySelector('.header-item-grip').contains(document.elementFromPoint(g.x+g.width/2,g.y+g.height/2))}})()`);
    assert.deepEqual(result,{hidden:false,grip:20,hit:true,gripHit:true},`${device}: item ${id} retains body and grip hit targets`);
  };
  const original=await evaluate('layerApp.app.workspace_persistence()'),theme=await evaluate('document.body.dataset.theme');
  try {
    for(const e of (await model()).zones.flat())await edit({type:'remove',id:e.id});
    for(const kind of ['capy','menu_labels'])await edit({type:'add',zone:'left',before:null,item:{kind}});
    const [capy,menu]=(await model()).zones[0].map(e=>e.id);
    for(const theme of ['dark','light'])for(const size of ['small','medium','large'])for(device of ['mouse','touch','pen']) {
      await send({type:'set_theme',theme});await edit({type:'set_size',size});
      const baseline=await model();await edit({type:'edit',editing:true});await resize(1800);
      const natural=(await rect(item(capy))).width+(await rect(item(menu))).width+6;
      await resize(Math.ceil(2*(natural+18+160+4)));
      assert.equal(await evaluate(`document.querySelector('${item(menu)} .header-menu-labels').hidden`),false,'Labels initially fit');
      assert.equal((await rect(`${item(menu)} .header-menu-labels`)).height,34,'Labels share the workspace switcher pill');
      await evaluate(`window.__menuLabelsNode=document.querySelector('${item(menu)}')`);
      await visible(menu);
      const added=[];
      for(const kind of ['clock','space']) {
        const before=await model(),zone=await rect('[data-zone="left"]');
        await pointer('down',center(await rect(`#header-component-${kind}`)));
        await pointer('move',{x:zone.x+zone.width-1,y:zone.y+zone.height/2});
        await visible(menu);
        assert.deepEqual(await model(),before,'Compaction during bank drag is only a preview');
        await pointer('up');
        const id=(await model()).zones[0].find(e=>e.item.kind===kind).id;added.push(id);
        await visible(menu);for(const neighbor of added)await visible(neighbor);
        assert.equal(await evaluate(`document.querySelector('${item(menu)}')===__menuLabelsNode`),true,'Compaction retains the original editable item');
        assert.equal(await evaluate(`document.querySelector('${item(menu)} .header-menu-labels').hidden`),true,'Only the labels compact');
        assert.equal(await evaluate(`document.querySelector('${item(menu)} .header-menu-labels-compact')?.hidden`),false);
        assert.equal(await evaluate('document.querySelector("#header-overflow-0").hidden'),true,'Fitting neighbors stay on the bar');
      }
      await shot(`compact-${theme}-${size}-${device}`);
      for(const id of [...added,menu]) {
        const selector=item(id)+(id===menu&&theme==='dark'?' .header-item-grip':'');
        await pointer('down',center(await rect(selector)));await pointer('move',{x:380,y:260});
        assert.equal(await evaluate('Number(document.querySelector(".header-drag-preview")?.dataset.headerItem)'),id,'The intended body/grip owns the drag');
        await pointer('move',id===menu?center(await rect('[data-zone="center"]')):{x:8,y:24});await pointer('up');
        const header=await model();assert.equal(id===menu?header.zones[1][0].id:header.zones[0][0].id,id);
        await visible(id);assert.ok(header.zones.flat().some(e=>e.id===capy),'Capy never needs removal');
      }
      const accepted=await model();await click('#header-edit-done');
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await model(),baseline);
      await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await model(),accepted);
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await model(),baseline);
      console.log(`PASS fixed-width Menu Labels ${theme} ${size} ${device}: add, compact, retained grips/neighbors, drag and undo/redo`);
    }
  } catch(error) {
    await shot('failure');throw error;
  } finally {
    if(pressed)await pointer('up');await edit({type:'cancel'});await resize(1440);
    await send({type:'restore_workspace',workspace:original});await send({type:'set_theme',theme});
  }
}
