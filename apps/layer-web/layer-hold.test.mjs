import assert from "node:assert/strict";

export async function checkLayerHolding({call, evaluate, settle}) {
  const wait=async(ms=180)=>{await settle();await evaluate(`new Promise(r=>setTimeout(r,${ms}))`);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
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
        await input("down",p);await wait(600);
        assert.equal(await menu(),true,`${region}: hold opens menu`);
        assert.equal(await evaluate("layerApp.state().layer_tools.editing_layer.mask_selected"),region==="mask","hold targets the correct content/mask menu");
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
    device="touch";
    await send({type:"layer",action:{op:"begin_rename",id:Number(before[0])}});
    const entry=await rect(`${source} input`);
    await input("down",{x:entry.x+entry.width/2,y:entry.y+entry.height/2});await wait(650);
    assert.equal(await menu(),false,"editing a name keeps native text input");
    await input("cancel");await send({type:"layer",action:{op:"cancel_rename"}});
    await evaluate("for(let i=0;i<30;i++)layerApp.dispatch({type:'layer',action:{op:'new',group:false,clipped:false}});document.querySelector('#layer-rows').scrollTop=0;");await wait();
    const scrollingOrder=await order(),r=await rect("#layer-rows .layer-swipe:nth-child(4) .layer-name");
    await input("down",{x:r.x+r.width/2,y:r.y+r.height/2});
    await input("move",{x:point.x,y:point.y-85});await input("up");await wait(650);
    assert.ok(await evaluate("document.querySelector('#layer-rows').scrollTop")>20,"movement before hold scrolls normally");
    assert.equal(await menu(),false);assert.deepEqual(await order(),scrollingOrder);
    console.log("PASS: whole-row holds, same-contact reorder, mask/row controls, release, cancellation, mouse, native touch scrolling, undo/redo");
  } finally {
    if(down)await input(device==="touch"?"cancel":"up");
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await send({type:"restore_workspace",workspace:saved});
  }
}

export async function checkLayerSwipes({call, evaluate, settle}) {
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,200))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const layer=()=>evaluate("({alpha_locked:layerApp.state().layers.find(l=>String(l.id)==='1').alpha_locked})");
  const rect=()=>evaluate(`(()=>{const r=document.querySelector('#layer-rows .layer-row[data-layer="1"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  const revealed=()=>evaluate(`!document.querySelector('#layer-rows .layer-row[data-layer="1"]').parentElement.querySelector('.layer-swipe-delete').hidden`);
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
  console.log('PASS: layer swipe alpha lock, both themes, touch/pen/mouse, undo/redo, short/reversed/cancelled swipes, Delete dismissal, locked layer');
}
