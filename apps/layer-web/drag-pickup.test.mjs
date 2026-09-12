import assert from "node:assert/strict";

// Browser-delivered mouse, touchscreen and pen contacts; never dispatch DOM
// pointer events to fake pickup. Native menu events are tested separately.
export async function checkDragPickup({call,evaluate,settle}) {
  const saved=await evaluate("layerApp.state().workspace"),fixture=structuredClone(saved);
  const tabs=(id,panels)=>({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
  Object.assign(fixture.layout,{bands:[
    {id:40,edge:"left",extent:252,root:tabs(41,["brushes","sizes"])},
    {id:42,edge:"right",extent:252,root:tabs(43,["layers","properties","adjustments"])},
    {id:44,edge:"top",extent:36,root:tabs(45,["toolbar"])},
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(220);};
  const snapshot=()=>evaluate("layerApp.state().workspace");
  const menu=()=>evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')");
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  let device="mouse",point,down=false;
  const input=async(type,p=point)=>{
    point=p;
    if(device==="touch")await call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd"}[type],touchPoints:type==="up"?[]:[{id:1,...p}]});
    else await call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[type],...p,button:"left",buttons:type==="up"?0:1,clickCount:1,pointerType:device});
    down=type!=="up";
  };
  const clean=async()=>{
    assert.equal(await evaluate("document.querySelectorAll('.drag-source,.tab-slide-overlay,.layer-drag-preview').length"),0);
    assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor??null"),null);
  };
  try {
    await send({type:"restore_workspace",workspace:fixture});
    for(let i=0;i<2;i++)await send({type:"layer",action:{op:"new",group:false,clipped:false}});
    for(device of ["mouse","touch","pen"])for(const grip of [false,true]) {
      const ids=await evaluate("[...document.querySelectorAll('#layer-rows .layer-row')].map(n=>n.dataset.layer)");
      const order=()=>evaluate("layerApp.state().layers.map(l=>String(l.id))");
      const before=await order(),source=`#layer-rows .layer-row[data-layer="${ids[0]}"]`;
      const target=await rect(`#layer-rows .layer-row[data-layer="${ids[1]}"]`);
      await input("down",center(await rect(`${source} ${grip?'.layer-grip':'.layer-name'}`)));
      await input("move",{x:target.x+target.width/2,y:target.y+target.height-3});await settle();
      const direct=grip||device==="mouse";
      assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),Number(direct),`${device} ${grip?'grip':'row'}: pickup`);
      if(direct)assert.equal(await evaluate("document.querySelectorAll('.layer-drop-after').length"),1,`${device} ${grip?'grip':'row'}: drop hint`);
      await input("up");await wait(350);
      if(direct){assert.notDeepEqual(await order(),before,`${device} ${grip?'grip':'row'}: drop`);await send({type:"invoke",command:"undo"});}
      assert.deepEqual(await order(),before);await clean();
    }
    for(device of ["mouse","touch","pen"]) {
      for(const source of ["tile","drawer-tile","column"]) {
        for(const mode of ["quick","hold","release","escape","blur","removed"]) {
          await send({type:"restore_workspace",workspace:fixture});
          if(source==="drawer-tile")await send({type:"move_panel",panel:"toolbar",target:{kind:"tab",group:43,index:null},viewport:await evaluate("[innerWidth,innerHeight]")});
          if(source!=="tile")await send({type:"customize",action:{type:"set_column_collapsed",group:43,collapsed:true}});
          if(source==="drawer-tile")await send({type:"customize",action:{type:"toggle_column_drawer",group:43,panel:"toolbar"}});
          const selector=source==="drawer-tile"?'.content-drawer .toolbar-controls [data-drag-pickup=hold]':source==="tile"?'.toolbar-controls [data-drag-pickup=hold]':'.column-tab[data-panel=properties]';
          const start=center(await rect(selector));
          const target=source.endsWith("tile")?await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});const r=n.parentElement.children[2].getBoundingClientRect();return{x:r.x+r.width*.8,y:r.y+r.height*.8}})()`):await evaluate("({x:innerWidth*.5,y:innerHeight*.55})");
          const before=await snapshot(),label=`${device} ${source} ${mode}`;
          await input("down",start);
          if(mode==="quick") {
            await input("move",target);await wait(650);
            assert.equal(await menu(),false,`${label}: moving before hold cancels menu`);
            assert.deepEqual(await snapshot(),before,`${label}: no early movement`);
            assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),true);
          } else if(mode==="removed") {
            await evaluate(`document.querySelector(${JSON.stringify(selector)}).remove()`);await wait(650);
            assert.equal(await menu(),false,`${label}: removed source cannot arm`);
          } else {
            await wait(650);
            assert.equal(await menu(),device!=="mouse",`${label}: only touch/pen holds open menus`);
            assert.deepEqual(await snapshot(),before,`${label}: hold does not activate`);
            if(mode!=="release") {
              await input("move",target);await settle();
              assert.equal(await menu(),false,`${label}: same contact closes menu`);
              assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor"),"grabbing");
            }
            if(mode==="escape") {
              await call("Input.dispatchKeyEvent",{type:"keyDown",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
              await call("Input.dispatchKeyEvent",{type:"keyUp",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
            } else if(mode==="blur")await evaluate("window.dispatchEvent(new Event('blur'))");
          }
          await input("up");await wait(400);await clean();
          if(mode==="hold") {
            const after=await snapshot();assert.notDeepEqual(after,before,`${label}: drop moves`);
            await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),before,`${label}: one undo`);
            await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snapshot(),after,`${label}: one redo`);
          } else {
            assert.deepEqual(await snapshot(),before,`${label}: no move/activation`);
            assert.equal(await menu(),mode==="release"&&device!=="mouse",`${label}: menu lifetime`);
          }
          await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
        }
      }
      for(const source of ["tab","toolbar-grip","column-grip"]) {
        await send({type:"restore_workspace",workspace:fixture});
        if(source==="column-grip")await send({type:"customize",action:{type:"set_column_collapsed",group:43,collapsed:true}});
        const selector={tab:'.dock-tab[data-panel=properties]',"toolbar-grip":'.toolbar-controls > .panel-grip',"column-grip":'.collapsed-column .panel-grip'}[source];
        const before=await snapshot();await input("down",center(await rect(selector)));
        await input("move",{x:source==="column-grip"?10:700,y:460});await settle();
        assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor"),"grabbing",`${device} ${source}: immediate`);
        await input("up");await wait(350);assert.notDeepEqual(await snapshot(),before);
        await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),before);await clean();
      }
    }
    device="mouse";
    for(const selector of ['.toolbar-controls [data-drag-pickup=hold]','.dock-tab[data-panel=properties]','.toolbar-controls > .panel-grip','#layer-rows .layer-name']) {
      await send({type:"restore_workspace",workspace:fixture});
      const p=center(await rect(selector));
      await input("down",p);await wait(650);
      assert.equal(await menu(),false,`${selector}: mouse hold never opens menu`);
      await input("up");
      for(const type of ["mousePressed","mouseReleased"])await call("Input.dispatchMouseEvent",{type,...p,button:"right",buttons:type==="mousePressed"?2:0,clickCount:1});
      assert.equal(await menu(),true,`${selector}: right-click still opens menu`);
      await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    }
    console.log("PASS: mouse/touch/pen tile and collapsed-icon hold gates, menu release, Escape/blur/removal, immediate tabs/grips/rows, undo/redo");
  } finally {
    if(down)await input("up");
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await send({type:"restore_workspace",workspace:saved});
  }
}
