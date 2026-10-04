import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

export async function checkLayerHolding({call, evaluate, settle}) {
  const wait=async(ms=180)=>{await settle();await evaluate(`new Promise(r=>setTimeout(r,${ms}))`);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const capture = async name => {
    if (!process.env.LAYER_TEST_ARTIFACTS) return;
    await mkdir(process.env.LAYER_TEST_ARTIFACTS,{recursive:true});
    const shot = await call("Page.captureScreenshot",{format:"png"});
    await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/${name}.png`,Buffer.from(shot.data,"base64"));
  };
  const order=()=>evaluate("layerApp.state().layers.map(l=>String(l.id))");
  const menu=()=>evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')");
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()`);
  const saved=await evaluate("layerApp.state().workspace");
  let down=false,device="touch",point;
  const input=async(type,p=point)=>{
    point=p;
    if(device==="touch")await call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd",cancel:"touchCancel"}[type],touchPoints:["up","cancel"].includes(type)?[]:[{id:1,...p}]});
    else await call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[type],...p,button:"left",buttons:type==="up"?0:1,clickCount:1,pointerType:device});
    down=!["up","cancel"].includes(type);await wait();
  };
  try {
    await send({type:"invoke",command:"reset_layout"});
    for(let i=0;i<2;i++)await send({type:"layer",action:{op:"new",group:false,clipped:false}});
    const before=await order(),source=`#layer-rows .layer-row[data-layer="${before[0]}"]`;
    await send({type:"layer",action:{op:"add_mask",id:Number(before[0]),replace:false}});
    for(const region of ["padding",".layer-name",".layer-thumbnail","mask",".layer-icon:first-child",".layer-icon:nth-child(2)",".layer-link",".layer-grip"]) {
      const selector=region==="padding"?source:region==="mask"?`${source} [aria-label="Edit layer mask"]`:`${source} ${region}`;
      for(const releaseOnly of [true,false]) {
        await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
        const r=await rect(selector),p={x:r.x+(region==="padding"?2:r.width/2),y:r.y+r.height/2};
        const visible=await evaluate(`layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(before[0])}).visible`);
        const maskBefore = await evaluate("layerApp.state().layer_tools.editing_layer.mask_selected");
        await input("down",p);await wait(600);
        assert.equal(await menu(),true,`${region}: hold opens menu`);
        assert.equal(await evaluate("layerApp.state().layer_tools.editing_layer.mask_selected"),region==="mask" || maskBefore,"hold preserves editing target unless the mask menu is requested");
        await input("move",{x:p.x+1,y:p.y});assert.equal(await menu(),true,"jitter retains menu");
        if(releaseOnly){
          await input("up");await wait(350);
          assert.equal(await menu(),true,`${region}: release retains menu`);
          assert.deepEqual(await order(),before);
          assert.equal(await evaluate(`layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(before[0])}).visible`),visible,"hold must not click a row button");
        }else{
          const target=await rect(`#layer-rows .layer-row[data-layer="${before[1]}"]`);
          await input("move",{x:target.x+target.width/2,y:target.y+target.height-3});
          assert.equal(await menu(),false,`${region}: drag dismisses menu`);
          assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),1,`${region}: same contact starts reorder`);
          await input("up");assert.notDeepEqual(await order(),before,`${region}: drop reorders`);
          await send({type:"invoke",command:"undo"});assert.deepEqual(await order(),before,"one undo restores order");
          await send({type:"invoke",command:"redo"});assert.notDeepEqual(await order(),before);
          await send({type:"invoke",command:"undo"});
        }
      }
    }
    for(const mode of ["cancel-hold","cancel-drag","blur","mouse","pen"]) {
      device=["mouse","pen"].includes(mode)?mode:"touch";
      await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
      const r=await rect(`${source} .layer-name`);
      await input("down",{x:r.x+r.width/2,y:r.y+r.height/2});await wait(600);
      assert.equal(await menu(),device!=="mouse",`${mode}: only touch/pen holds open menus`);
      if(mode!=="cancel-hold"){
        const target=await rect(`#layer-rows .layer-row[data-layer="${before[1]}"]`);
        await input("move",{x:target.x+target.width/2,y:target.y+target.height-3});
        assert.equal(await menu(),false);
      }
      if(["mouse","pen"].includes(mode)) {await input("up");assert.notDeepEqual(await order(),before);await send({type:"invoke",command:"undo"});}
      else if(mode==="blur") {await evaluate("window.dispatchEvent(new Event('blur'))");await input("up");}
      else await input("cancel");
      assert.deepEqual(await order(),before);
      assert.equal(await menu(),false);
      assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview,.layer-drop-before,.layer-drop-after,.layer-drop-into').length"),0);
    }
    const click = async (selector, modifiers = 0) => {
      const r = await rect(selector), p = {x:r.x+r.width/2,y:r.y+r.height/2};
      await call("Input.dispatchMouseEvent",{type:"mousePressed",...p,button:"left",buttons:1,clickCount:1,modifiers});
      await call("Input.dispatchMouseEvent",{type:"mouseReleased",...p,button:"left",buttons:0,clickCount:1,modifiers});await wait();
    };
    const checked = () => evaluate("layerApp.state().layers.filter(l=>l.selected).map(l=>String(l.id))");
    for (const theme of ["light","dark"]) {
      await send({type:"set_theme",theme});
      await send({type:"layer",action:{op:"select",id:Number(before[0]),mask:true}});
      await click(`#layer-rows .layer-row[data-layer="${before[1]}"] .layer-icon:nth-child(2)`);
      await click(`${source} .layer-name`);
      assert.equal(await evaluate("layerApp.state().layer_tools.editing_layer.mask_selected"),true,"active row keeps mask editing");
      assert.deepEqual(await checked(),before.slice(0,2),"active row keeps checked companions");
      await click(`${source} .layer-link`);
      assert.equal(await evaluate(`document.querySelector('${source} .layer-link svg').dataset.asset`),"unlink");
      assert.equal(await evaluate(`document.querySelector('${source} .layer-link').getAttribute('aria-pressed')`),"false");
      await capture(`mask-unlinked-${theme}`);
      await click(`${source} .layer-link`);
      assert.equal(await evaluate(`document.querySelector('${source} .layer-link svg').dataset.asset`),"link");
      await capture(`mask-linked-${theme}`);
      await click(`${source} .layer-thumbnail`);
      await click(`#layer-rows .layer-row[data-layer="${before[2]}"] .layer-name`,8);
      assert.deepEqual(await checked(),before.slice(0,3),"Shift selects the visible consecutive range");
      await click(`${source} .layer-thumbnail`);
      await click(`#layer-rows .layer-row[data-layer="${before[2]}"] .layer-icon:nth-child(2)`,8);
      assert.deepEqual(await checked(),before.slice(0,3),"Shift checkbox selects the same range");
      for (const id of before.slice(0,3)) assert.equal(await evaluate(`document.querySelector('#layer-rows .layer-row[data-layer="${id}"] .layer-icon:nth-child(2)').getAttribute('aria-pressed')`),"true");
      await click('.layer-footer button[aria-label="New group"]');
      const group = await evaluate("String(layerApp.state().layer_tools.editing_layer.id)");
      assert.ok((await order()).includes(group));
      assert.equal(await evaluate(`layerApp.state().layers.filter(l=>${JSON.stringify(before.slice(0,3))}.includes(String(l.id))).every(l=>l.depth===1)`),true,"New group contains every checked row");
      await click('.layer-footer button[aria-label="Delete selected layers"]');
      assert.deepEqual(await order(),before,"deleting an expanded folder preserves unchecked children");
      await send({type:"invoke",command:"undo"});await send({type:"invoke",command:"undo"});
      assert.deepEqual(await order(),before);
      await send({type:"layer",action:{op:"select",id:Number(before[0]),mask:false}});
      await send({type:"effect",action:{op:"insert",effect:"curves"}});
      const effect = await evaluate("String(layerApp.state().layer_tools.editing_layer.id)");
      assert.equal(await evaluate(`document.querySelector('#layer-rows .layer-row[data-layer="${effect}"] .layer-thumbnail').classList.contains('editing-target')`),false,"filter icon has no editable-pixel corners");
      await capture(`filter-selected-${theme}`);
      await send({type:"invoke",command:"undo"});
      await send({type:"layer",action:{op:"select",id:Number(before[0]),mask:false}});
      await click(`#layer-rows .layer-row[data-layer="${before[1]}"] .layer-icon:nth-child(2)`);
      device="mouse";
      const start = await rect(`${source} .layer-name`), target = await rect(`#layer-rows .layer-row[data-layer="${before[2]}"]`);
      await input("down",{x:start.x+start.width/2,y:start.y+start.height/2});
      await input("move",{x:target.x+target.width/2,y:target.y+target.height-3});await input("up");
      assert.deepEqual(await order(),[before[2],...before.slice(0,2),...before.slice(3)],"drag moves the checked block in stack order");
      await send({type:"invoke",command:"undo"});assert.deepEqual(await order(),before);
      await send({type:"layer",action:{op:"select",id:Number(before[0]),mask:false}});
    }
    device="touch";
    await send({type:"layer",action:{op:"begin_rename",id:Number(before[0])}});
    const entry=await rect(`${source} input`);
    await input("down",{x:entry.x+entry.width/2,y:entry.y+entry.height/2});await wait(650);
    assert.equal(await menu(),false,"editing a name keeps native text input");
    await input("cancel");await send({type:"layer",action:{op:"cancel_rename"}});
    device="touch";
    for(const width of [1440,900]) {
      await call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await wait();
      for(const theme of ['light','dark']) {
        await send({type:'set_theme',theme});
        const r=await rect(`${source} .layer-name`);
        await input('down',{x:r.x+r.width/2,y:r.y+r.height/2});await wait(600);
        assert.equal(await menu(),true,`${theme}/${width}: touch hold`);
        const target=await rect(`#layer-rows .layer-row[data-layer="${before[1]}"]`);
        await input('move',{x:target.x+target.width/2,y:target.y+target.height-3});
        assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),1);
        await input('cancel');assert.deepEqual(await order(),before);
      }
    }
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});await wait();
    await evaluate("for(let i=0;i<30;i++)layerApp.dispatch({type:'layer',action:{op:'new',group:false,clipped:false}});document.querySelector('#layer-rows').scrollTop=0;");await wait();
    const scrollingOrder=await order(),r=await rect("#layer-rows .layer-swipe:nth-child(4) .layer-name");
    await input("down",{x:r.x+r.width/2,y:r.y+r.height/2});
    await input("move",{x:point.x,y:point.y-85});await input("up");await wait(650);
    assert.ok(await evaluate("document.querySelector('#layer-rows').scrollTop")>20,"movement before hold scrolls normally");
    assert.equal(await menu(),false);assert.deepEqual(await order(),scrollingOrder);
    for (const deviceType of ["mouse","touch","pen"]) {
      device=deviceType;
      await evaluate("document.querySelector('#layer-rows').scrollTop=0");await wait();
      const source = await rect("#layer-rows .layer-swipe:nth-child(2) .layer-name"), viewport = await rect("#layer-rows");
      await input("down",{x:source.x+source.width/2,y:source.y+source.height/2});
      if (device!=="mouse") await wait(600);
      await input("move",{x:viewport.x+viewport.width/2,y:viewport.y+viewport.height-8});
      await wait(450);
      assert.ok(await evaluate("document.querySelector('#layer-rows').scrollTop")>40,`${device}: dragging at the edge scrolls to offscreen destinations`);
      await evaluate("window.dispatchEvent(new Event('blur'))");await input(device==="touch"?"cancel":"up");
      const stopped = await evaluate("document.querySelector('#layer-rows').scrollTop");await wait(250);
      assert.equal(await evaluate("document.querySelector('#layer-rows').scrollTop"),stopped,"cancelled drag stops edge scrolling");
      assert.deepEqual(await order(),scrollingOrder);
    }
    console.log("PASS: layer range/group/delete, checked drag, content corners, mask link, accessibility, edge scrolling, whole-row holds and undo/redo");
  } finally {
    if(down)await input(device==="touch"?"cancel":"up");
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await send({type:"restore_workspace",workspace:saved});
  }
}

export async function checkLayerSwipes({call, evaluate, settle}) {
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,200))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  let targetId=1;
  const layer=()=>evaluate(`(()=>{const l=layerApp.state().layers.find(l=>Number(l.id)===${targetId});return{alpha_locked:l.alpha_locked,pass_through:l.pass_through,blend:l.blend_label}})()`);
  const rect=()=>evaluate(`(()=>{const r=document.querySelector('#layer-rows .layer-row[data-layer="${targetId}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  const revealed=()=>evaluate(`!document.querySelector('#layer-rows .layer-row[data-layer="${targetId}"]').parentElement.querySelector('.layer-swipe-delete').hidden`);
  const swipe=async(device,dx,{cancel=false,returnToStart=false}={})=>{
    const start=await rect();
    const input=async(type,x)=>{
      if(device==='touch')await call('Input.dispatchTouchEvent',{type,touchPoints:['touchEnd','touchCancel'].includes(type)?[]:[{id:1,x,y:start.y}]});
      else await call('Input.dispatchMouseEvent',{type:{touchStart:'mousePressed',touchMove:'mouseMoved',touchEnd:'mouseReleased'}[type],x,y:start.y,button:'left',buttons:type==='touchEnd'?0:1,clickCount:1,pointerType:device});
    };
    await input('touchStart',start.x);
    await input('touchMove',start.x+dx/3);await input('touchMove',start.x+dx);
    if(returnToStart)await input('touchMove',start.x);
    if(cancel && device!=='touch')await evaluate("window.dispatchEvent(new Event('blur'))");
    await input(cancel && device==='touch'?'touchCancel':'touchEnd',start.x+(returnToStart?0:dx));await wait();
  };
  await send({type:'invoke',command:'reset_layout'});
  await send({type:'layer',action:{op:'alpha_lock',id:1,value:false}});
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const device of ['touch','pen','mouse']) {
      for(const options of [{dx:18},{dx:60,cancel:true},{dx:60,returnToStart:true}]) {
        await swipe(device,options.dx,options);assert.equal((await layer()).alpha_locked,false,`${device}: unfinished swipe`);
      }
      await swipe(device,60);assert.equal((await layer()).alpha_locked,device!=='mouse',`${theme}/${device}: swipe right`);
      if(device==='mouse')continue;
      await send({type:'invoke',command:'undo'});assert.equal((await layer()).alpha_locked,false);
      await send({type:'invoke',command:'redo'});assert.equal((await layer()).alpha_locked,true);
      await swipe(device,60);assert.equal((await layer()).alpha_locked,false,'second swipe unlocks');
      await swipe(device,-60);assert.equal(await revealed(),true,'left swipe reveals Delete');
      await swipe(device,90);assert.equal(await revealed(),false,'reverse swipe closes Delete');
      assert.equal((await layer()).alpha_locked,false,'closing Delete does not toggle alpha lock');
      await send({type:'layer',action:{op:'lock',id:1,value:true}});
      await swipe(device,60);assert.equal((await layer()).alpha_locked,false,'locked layer refuses alpha lock');
      await send({type:'layer',action:{op:'lock',id:1,value:false}});
    }
  }
  await send({type:'layer',action:{op:'new',group:true,clipped:false}});
  targetId=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  await send({type:'layer',action:{op:'blend',id:targetId,value:1}});
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const device of ['touch','pen']) {
      for(const options of [{dx:18},{dx:60,cancel:true},{dx:60,returnToStart:true}]) {
        await swipe(device,options.dx,options);assert.equal((await layer()).blend,'Multiply',`${device}: unfinished group swipe`);
      }
      await swipe(device,60);assert.equal((await layer()).pass_through,true);
      assert.equal(await evaluate(`!!document.querySelector('#layer-rows .layer-row[data-layer="${targetId}"] .layer-group-pass-through')`),true);
      await send({type:'invoke',command:'undo'});assert.equal((await layer()).blend,'Multiply');
      await send({type:'invoke',command:'redo'});assert.equal((await layer()).pass_through,true);
      await swipe(device,60);assert.equal((await layer()).blend,'Multiply','group swipe restores retained isolated blend');
      await swipe(device,-60);assert.equal(await revealed(),true);
      await swipe(device,90);assert.equal(await revealed(),false);
      assert.equal((await layer()).blend,'Multiply','closing group Delete does not change mode');
      await send({type:'layer',action:{op:'lock',id:targetId,value:true}});
      await swipe(device,60);assert.equal((await layer()).blend,'Multiply','locked group has no right action');
      await send({type:'layer',action:{op:'lock',id:targetId,value:false}});
    }
  }
  await send({type:'layer',action:{op:'select',id:targetId,mask:false}});
  await send({type:'effect',action:{op:'insert',effect:'curves'}});
  const effect=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  await send({type:'layer',action:{op:'clip',id:effect,value:true}});
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${targetId}).right_swipe??null`),null);
  await swipe('touch',60);assert.equal((await layer()).blend,'Multiply','attached FX protect group isolation');
  console.log('PASS: paint/group shared swipe actions, themes, touch/pen/mouse, retained Multiply, undo/redo, cancelled swipes, Delete closure, lock and attached-content protection');
}
