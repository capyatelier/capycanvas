import assert from "node:assert/strict";
import {mkdir, writeFile} from "node:fs/promises";

export async function checkWorkspaceSwitcher({call, evaluate, settle, reload}) {
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const view = () => evaluate("JSON.parse(layerApp.app.workspace_view())");
  const wait = async predicate => {
    for (let i=0;i<180;i++) { if (await evaluate(predicate)) return; await pause(100); }
    throw Error(`Workspace switcher timeout: ${predicate}\n${JSON.stringify(await view())}`);
  };
  const ready = "window.layerApp?.startupTimes.complete != null && JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).switcher_busy && !JSON.parse(layerApp.app.workspace_view()).dirty";
  const idle = async () => { await pause(150); await wait(ready); await settle(); };
  const send = async input => { await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(input))});null`); await idle(); };
  const row = id => `.workspace-list > .workspace-row[data-id=${JSON.stringify(id)}]`;
  const rect = selector => evaluate(`(()=>{const node=document.querySelector(${JSON.stringify(selector)});if(!node)throw Error('Missing '+${JSON.stringify(selector)});const r=node.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()`);
  const point = async (selector, y=.5) => { const r=await rect(selector); return {x:r.x+r.width/2,y:r.y+r.height*y}; };
  let pressed = false, device = "mouse", lastPoint;
  const pointer = async (type, p=lastPoint, button="left") => {
    lastPoint=p;
    if (device === "touch") await call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd",cancel:"touchCancel"}[type],touchPoints:["up","cancel"].includes(type)?[]:[{id:1,...p}]});
    else await call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[type],...p,pointerType:device,button,buttons:type==="up"?0:button==="right"?2:1,clickCount:1});
    pressed=!["up","cancel"].includes(type); await pause(35);
  };
  const click = async selector => {
    device="mouse";
    await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(e?.closest('.workspace-list'))e.scrollIntoView({block:'nearest',behavior:'instant'});})()`);
    const p=await point(selector); await pointer("move",p); await pointer("down",p); await pointer("up",p); await idle();
  };
  const key = async key => {
    const code={Escape:27,Enter:13,ArrowDown:40,ArrowUp:38,Home:36,End:35}[key];
    await call("Input.dispatchKeyEvent",{type:"keyDown",key,code:key,windowsVirtualKeyCode:code,...(key==="Enter"?{text:"\r",unmodifiedText:"\r"}:{})});
    await call("Input.dispatchKeyEvent",{type:"keyUp",key,code:key,windowsVirtualKeyCode:code}); await idle();
  };
  const menuOpen = () => evaluate("document.querySelector('.workspace-row-menu').matches(':popover-open')");
  const order = async () => (await view()).order;
  const pins = async () => (await view()).switcher.map(row=>row.id);
  const durable = () => evaluate("JSON.parse(layerApp.app.workspace_capture())");
  const layout = () => evaluate("layerApp.state().workspace.layout");
  const shown = () => evaluate("[...document.querySelectorAll('.workspace-switcher button')].map(node=>node.dataset.workspaceId)");
  const options = async (id, action) => { await click(`${row(id)} .workspace-options`); await click(`.workspace-row-menu [data-action="${action}"]`); };
  const drag = async (id, target, after, sourceDevice, handle=false, hold=false) => {
    device=sourceDevice;
    const start=await point(`${row(id)} ${handle?'.workspace-grip':'.workspace-choice'}`), end=await point(row(target), after?.85:.15);
    await pointer("down",start); if(hold) {await pause(600);assert.equal(await menuOpen(),device!=="mouse",`${device}: only touch/pen holds open menus before drag`);}
    for(const t of [.3,.7,1]) await pointer("move",{x:start.x+(end.x-start.x)*t,y:start.y+(end.y-start.y)*t});
    if(hold)assert.equal(await menuOpen(),false,`${device} same-contact drag closes menu`);
    await pointer("up"); await idle();
  };
  const artifacts=process.env.LAYER_TEST_ARTIFACTS||"/tmp/capy-workspace-evidence/web-switcher"; await mkdir(artifacts,{recursive:true});
  const shot=async name=>{const image=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${artifacts}/${name}.png`,Buffer.from(image.data,"base64"));};
  await idle();
  const initial=await view(), [p,i,f]=initial.defaults.map(row=>row.id), original=await durable(), originalLayout=await layout();
  await evaluate("window.workspacePointerTypes=[];document.addEventListener('pointerdown',e=>workspacePointerTypes.push(e.pointerType));window.workspaceEvents=[];for(const type of ['pointerdown','pointermove','pointerup','pointercancel','gotpointercapture','lostpointercapture'])document.addEventListener(type,e=>{workspaceEvents.push({type,device:e.pointerType,target:e.target.className?.baseVal??e.target.className,x:e.clientX,y:e.clientY});if(workspaceEvents.length>80)workspaceEvents.shift();},true);");
  try {
    await send({type:"open",page:"workspaces"}); await click(`${row(f)} .workspace-choice`);
    const preview=await layout(); assert.equal((await view()).id,initial.id);
    for(const id of [p,i,f])assert.ok((await rect(`${row(id)} .workspace-grip`)).width<=16,"narrow grip on every row");
    await shot("workspaces");
    device="mouse"; await pointer("down",await point(`${row(p)} .workspace-choice`),"right"); await pointer("up",undefined,"right");
    assert.equal(await menuOpen(),true,"right-click opens menu"); await shot("row-menu"); await key("Escape");
    for(const kind of ["mouse","touch","pen"]) {
      device=kind;
      await pointer("down",await point(`${row(p)} .workspace-choice`)); await pause(600);
      assert.equal(await menuOpen(),kind!=="mouse",`${kind}: only touch/pen holds open menus`); await pointer("up"); await idle();
      assert.equal(await menuOpen(),kind!=="mouse",`${kind}: menu lifetime after release`);
      if(kind==="mouse") {
        assert.equal((await view()).selected,p,"mouse hold release keeps ordinary row selection");
        await click(`${row(f)} .workspace-choice`);
      } else await key("Escape");
      assert.deepEqual(await layout(),preview);
      await drag(p,f,true,kind,true);
      assert.deepEqual(await pins(),[i,f,p],`${kind} handle starts without hold`);
      await drag(p,i,false,kind,true); assert.deepEqual(await pins(),[p,i,f]);
      if(kind!=="mouse") {
        await drag(p,f,true,kind); assert.deepEqual(await pins(),[p,i,f],`${kind} row cannot reorder before hold`);
      }
      await drag(p,f,true,kind,false,true); assert.deepEqual(await pins(),[i,f,p],`${kind} held row reorder`);
      await drag(p,i,false,kind,true);
      if(kind==="mouse") {
        await drag(p,f,true,kind); assert.deepEqual(await pins(),[i,f,p],"mouse row drags immediately");
        await drag(p,i,false,kind,true);
      }
      assert.deepEqual(await durable(),original,"preferences never enter workspace history");
      assert.deepEqual(await layout(),preview); assert.equal((await view()).selected,f);
    }
    assert.deepEqual([...new Set(await evaluate("workspacePointerTypes"))].sort(),["mouse","pen","touch"]);
    // Cancel both an established drag and a held contact. Neither closes the dialog.
    for(const hold of [false,true]) {
      device="touch"; const start=await point(`${row(p)} ${hold?'.workspace-choice':'.workspace-grip'}`);
      await pointer("down",start);
      if(hold) await pause(600); else await pointer("move",await point(row(f),.8));
      await key("Escape"); await pointer("up"); await idle();
      assert.equal(await menuOpen(),false); assert.deepEqual(await pins(),[p,i,f]);
      assert.equal(await evaluate("document.querySelector('.workspace-manager').open"),true);
    }
    await options(p,"pin"); assert.deepEqual(await pins(),[i,f]);
    assert.ok(await evaluate(`!!document.querySelector(${JSON.stringify(`${row(p)} .workspace-grip`)})`));
    await drag(p,f,true,"touch",true); assert.deepEqual((await order()).slice(0,3),[i,f,p]); assert.deepEqual(await pins(),[i,f]);
    await click(`${row(p)} .workspace-options`); await evaluate("document.querySelector('.workspace-row-menu [data-action=up]').focus()"); assert.equal(await menuOpen(),true); assert.equal(await evaluate("document.activeElement.dataset.action"),"up"); await key("Enter");
    assert.deepEqual((await order()).slice(0,3),[i,p,f]);
    assert.deepEqual(await layout(),preview); assert.deepEqual(await durable(),original);
    await click(".workspace-manager footer button"); assert.deepEqual(await layout(),originalLayout);
    // Create enough real workspace records to exercise the scrolling list and pill.
    const custom=[];
    for(let n=0;n<9;n++) {
      await send({type:"form",kind:"new"}); await send({type:"submit",name:n?`Study ${n}`:"Sketching",source:null}); custom.push((await view()).id);
      assert.ok((await pins()).includes(custom[n]),"new workspace is pinned by default");
      assert.deepEqual(await shown(),await pins());
    }
    await send({type:"switch",id:initial.id}); await send({type:"open",page:"workspaces"}); await click(`${row(f)} .workspace-choice`);
    assert.deepEqual(await pins(),[i,f,...custom],"new pins survive switching away");
    for(const id of custom.slice(1))await options(id,"pin");
    assert.deepEqual(await pins(),[i,f,custom[0]]);
    for(const id of [i,f,custom[0]])await options(id,"pin");
    assert.equal(await evaluate("document.querySelector('.workspace-switcher').hidden"),false);
    assert.deepEqual(await shown(),[initial.id],"current workspace stays visible, not the dialog preview");
    await evaluate("document.querySelector('.workspace-list').scrollTop=0"); await settle();
    await drag(custom[0],i,false,"mouse",true); assert.equal((await order())[0],custom[0]); assert.deepEqual(await pins(),[]);
    await options(custom[0],"pin"); await options(p,"pin");
    assert.deepEqual(await shown(),[initial.id,...await pins()]);
    // Native touch and pen body motion scrolls rather than publishing a reorder.
    const savedOrder=await order();
    for(const kind of ["touch","pen"]) {
      await evaluate("document.querySelector('.workspace-list').scrollTop=0"); await pause(150);
      const start=await point(`${row(f)} .workspace-choice`); device=kind; await pointer("down",start);
      for(const dy of [25,55,90])await pointer("move",{x:start.x,y:start.y-dy}); await pointer("up"); await pause(550);
      assert.deepEqual(await order(),savedOrder,`${kind} swipe preserves order`);
      if(kind==="touch")assert.ok(await evaluate("document.querySelector('.workspace-list').scrollTop")>20,"touch swipes scroll");
      assert.equal(await menuOpen(),false);
    }
    await evaluate("document.querySelector('.workspace-list').scrollTop=0"); await pause(300);
    assert.deepEqual(await layout(),preview);
    // A lost capture cancels a mouse drag; an external catalog refresh cannot
    // replace its source nodes mid-contact.
    device="mouse"; await pointer("down",await point(`${row(custom[0])} .workspace-grip`)); await pointer("move",await point(row(f),.8));
    assert.equal(await evaluate("document.querySelectorAll('.workspace-drag-preview').length"),1);
    await send({type:"refresh_switcher"});
    assert.equal(await evaluate("document.querySelectorAll('.workspace-drag-preview').length"),1);
    await evaluate("window.dispatchEvent(new Event('blur'))"); await pointer("up"); await idle();
    assert.deepEqual(await order(),savedOrder); assert.equal(await evaluate("document.querySelectorAll('.workspace-drag-preview').length"),0);
    await shot("custom-workspaces");
    // Broadcast updates arrive in a second real tab and preserve this preview.
    const target=await call("Target.createTarget",{url:"about:blank"},null);
    const {sessionId}=await call("Target.attachToTarget",{targetId:target.targetId,flatten:true},null);
    const other=async expression=>{const r=await call("Runtime.evaluate",{expression,returnByValue:true,awaitPromise:true},sessionId);if(r.exceptionDetails)throw Error(r.exceptionDetails.text);return r.result.value;};
    try {
      await call("Runtime.enable",{},sessionId);await call("Page.enable",{},sessionId);await call("Page.navigate",{url:await evaluate("location.href")},sessionId);
      for(let n=0;n<180;n++){if(await other(ready))break;await pause(100);if(n===179)throw Error("Second tab startup");}
      const otherPins=await other("JSON.parse(layerApp.app.workspace_view()).switcher.map(r=>r.id)");
      await wait(`JSON.stringify(JSON.parse(layerApp.app.workspace_view()).switcher.map(r=>r.id))===${JSON.stringify(JSON.stringify(otherPins))}`);
      assert.deepEqual(await other("JSON.parse(layerApp.app.workspace_view()).switcher.map(r=>r.id)"),await pins());
      await other(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify({type:"edit_switcher",edit:{type:"show",id:f,visible:true}}))});null`);
      await wait(`JSON.parse(layerApp.app.workspace_view()).switcher.some(row=>row.id===${JSON.stringify(f)})`);
      assert.equal((await view()).selected,f); assert.deepEqual(await layout(),preview);
    } finally {await call("Target.closeTarget",{targetId:target.targetId},null);}
    // Long names and many shown entries stay within the header; keyboard focus
    // and horizontal scrolling can reach entries beyond its visible width.
    for(const id of custom)await send({type:"edit_switcher",edit:{type:"show",id,visible:true}});
    assert.ok(await evaluate("document.querySelector('.workspace-switcher').scrollWidth > document.querySelector('.workspace-switcher').clientWidth"));
    assert.ok(await evaluate("document.querySelector('.workspace-switcher').getBoundingClientRect().width <= 420"));
    const beforeRestart={pins:await pins(),order:await order()};
    await click(".workspace-manager footer button"); await reload(); await wait(ready); await idle();
    assert.deepEqual(await pins(),beforeRestart.pins); assert.deepEqual(await order(),beforeRestart.order); assert.deepEqual(await shown(),[initial.id,...beforeRestart.pins]);
    assert.deepEqual(await durable(),original,"restart preserves original workspace and history");
    await click(`.workspace-switcher button[data-workspace-id=${JSON.stringify(custom[0])}]`);
    assert.equal((await view()).id,custom[0]);
    assert.equal(await evaluate(`document.querySelector('.workspace-switcher button[data-workspace-id=${JSON.stringify(custom[0])}]').getAttribute('aria-pressed')`),"true");
    assert.deepEqual(await shown(),await pins(),"temporary entry disappears when switching to a pinned workspace");
    await send({type:"edit_switcher",edit:{type:"show",id:custom[0],visible:false}});
    assert.deepEqual(await shown(),[custom[0],...await pins()]);
    await click(`.workspace-switcher button[data-workspace-id=${JSON.stringify(p)}]`);
    assert.deepEqual(await shown(),await pins());
    console.log("PASS: configurable Web pill, narrow grips on every row, mouse/touch/pen pickup and menus, same-contact drag, hidden-row order, keyboard, scrolling, cancel/blur, preview preservation, cross-tab refresh and restart");
  } catch(error) {await shot("failure");console.error("Workspace switcher failure",await view());await writeFile(`${artifacts}/events.json`,JSON.stringify(await evaluate("workspaceEvents"),null,2));throw error;}
  finally {if(pressed)await pointer(device==="touch"?"cancel":"up");}
}
