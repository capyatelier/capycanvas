import assert from "node:assert/strict";
import {mkdir, writeFile} from "node:fs/promises";

export async function checkWorkspaceMenuRefresh({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const view=()=>evaluate('JSON.parse(layerApp.app.workspace_view())');
  const wait=async condition=>{for(let i=0;i<200;i++){if(await evaluate(condition)){await settle();return;}await pause(25);}throw Error(`Menu refresh timeout: ${condition}`);};
  const idle=()=>wait('JSON.parse(layerApp.app.workspace_view())?.ready&&!JSON.parse(layerApp.app.workspace_view()).busy&&!JSON.parse(layerApp.app.workspace_view()).switcher_busy&&!JSON.parse(layerApp.app.workspace_view()).dirty');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await idle();};
  const click=async selector=>{
    const p=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)})?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,pointerType:'mouse'});
    await settle();
  };
  const begin=()=>evaluate(`(()=>{const app=layerApp.app,tick=app.workspace_tick;window.pendingMenuRefresh={tick};app.workspace_input(JSON.stringify({type:'refresh_switcher'}));tick.call(app);app.workspace_tick=()=>({regions:0,canvas_wake:false});return JSON.parse(app.workspace_view()).switcher_busy})()`);
  const release=async()=>{await evaluate('layerApp.app.workspace_tick=pendingMenuRefresh.tick');await idle();};
  const pinAction=id=>evaluate(`(()=>{const v=JSON.parse(layerApp.app.workspace_view()),row=v.switcher_options.sections[0].find(r=>r.action.command.id===${JSON.stringify(id)});layerApp.dispatch(row.action)})()`);
  await idle();
  const saved=await evaluate('layerApp.app.workspace_persistence()'),original=await view();
  const fixture=structuredClone(saved);fixture.layout.header={size:'small',zones:[[],[{id:1,item:{kind:'workspaces'}}],[]],next_id:2};
  try {
    await send({type:'restore_workspace',workspace:fixture});
    assert.equal(await begin(),true);await pause(150);
    await click('.workspace-switcher-options');
    assert.ok(await evaluate("Array.from(document.querySelectorAll('.panel-context-menu button[aria-checked]')).every(b=>b.disabled)"));
    await evaluate("window.retainedMenuRows=[...document.querySelectorAll('.panel-context-menu button')];retainedMenuRows.at(-1).focus()");
    await release();
    assert.equal(await evaluate("retainedMenuRows.every((n,i)=>document.querySelectorAll('.panel-context-menu button')[i]===n)&&document.activeElement===retainedMenuRows.at(-1)"),true,'availability refresh retains options rows and focus');
    assert.ok(await evaluate("Array.from(document.querySelectorAll('.panel-context-menu button[aria-checked]')).every(b=>!b.disabled)"));
    const target=original.switcher_options.sections[0].find(row=>row.action.command.id!==original.id),id=target.action.command.id;
    await pinAction(id);await idle();
    const index=(await view()).switcher_options.sections[0].findIndex(row=>row.action.command.id===id);
    assert.equal(await evaluate(`document.querySelectorAll('.panel-context-menu button[aria-checked]')[${index}].getAttribute('aria-checked')`),String(!target.selected));
    await click(`.panel-context-menu button:nth-of-type(${index+1})`);await idle();
    assert.equal((await view()).switcher_options.sections[0][index].selected,target.selected,'refreshed row invokes its current toggle action');
    for(const overflow of [false,true]) {
      let next=1;const item=kind=>({id:next++,item:{kind}});
      fixture.layout.header={size:'small',zones:[['capy','menu','settings',...Array(8).fill('space')].map(item),[item('workspaces')],Array.from({length:8},()=>item('space'))],next_id:next};
      const workspaceId=fixture.layout.header.zones[1][0].id;
      if(overflow)fixture.layout.header.zones[0].push(fixture.layout.header.zones[1].pop());
      await call('Emulation.setDeviceMetricsOverride',{width:480,height:870,deviceScaleFactor:1,mobile:false});
      await send({type:'restore_workspace',workspace:fixture});
      const menu=overflow?'#header-overflow-0 .popover':'#header-workspace-selector .popover';
      await click(overflow?'#header-overflow-0 > summary':'#header-workspace-selector > summary');
      if(overflow)await click(`[data-header-overflow-item="${workspaceId}"]`);
      const submenu=(await view()).switcher_display.length+1;
      await click(`${menu} button:nth-of-type(${submenu})`);
      await evaluate(`window.retainedMenuRows=[...document.querySelectorAll('${menu} button')];retainedMenuRows[0].focus()`);
      assert.equal(await begin(),true);
      await wait(`Array.from(document.querySelectorAll('${menu} button[aria-checked]')).every(b=>b.disabled)`);
      await release();
      assert.equal(await evaluate(`retainedMenuRows.every((n,i)=>document.querySelectorAll('${menu} button')[i]===n)&&document.activeElement===retainedMenuRows[0]`),true,'compact/overflow refresh retains submenu rows and focused Back');
      assert.ok(await evaluate(`Array.from(document.querySelectorAll('${menu} button[aria-checked]')).every(b=>!b.disabled)`));
      await click(`${menu} .submenu-back`);
      assert.ok(await evaluate(`document.querySelectorAll('${menu} button').length===JSON.parse(layerApp.app.workspace_view()).switcher_menu.sections.flat().length`));
      await evaluate(`document.querySelector('${overflow?'#header-overflow-0':'#header-workspace-selector'}').open=false`);
    }
    console.log('PASS: Web workspace menus reenable after same-binding preference refresh, retain rows/focus/submenu, and dispatch current checkbox actions in options, compact and overflow');
  } finally {
    await evaluate('if(window.pendingMenuRefresh)layerApp.app.workspace_tick=pendingMenuRefresh.tick');
    await call('Emulation.clearDeviceMetricsOverride');await send({type:'restore_workspace',workspace:saved});
  }
}

export async function checkWorkspaceOptions({call, evaluate, settle}) {
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const view = () => evaluate("JSON.parse(layerApp.app.workspace_view())");
  const idle = async () => {
    for (let i=0;i<200;i++) {
      const v = await view();
      if (v?.ready && !v.busy && !v.switcher_busy && !v.dirty) { await settle(); return; }
      await pause(50);
    }
    throw Error("Workspace options did not settle");
  };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await idle(); };
  const input = async value => { await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`); await idle(); };
  const center = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  let device = "mouse", point;
  const pointer = async (type, p=point, button="left") => {
    point = p;
    if (device === "touch") await call("Input.dispatchTouchEvent", {type:{down:"touchStart",move:"touchMove",up:"touchEnd",cancel:"touchCancel"}[type], touchPoints:["up","cancel"].includes(type)?[]:[{id:1,...p}]});
    else await call("Input.dispatchMouseEvent", {type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[type],...p,pointerType:device,button,buttons:type==="up"?0:button==="right"?2:1,clickCount:1});
    await pause(30);
  };
  const click = async selector => { await pointer("down",await center(selector)); await pointer("up"); await idle(); };
  const key = async (name, modifiers=0) => {
    for (const type of ["keyDown","keyUp"]) await call("Input.dispatchKeyEvent", {type,key:name,code:name,modifiers,windowsVirtualKeyCode:{Escape:27,Enter:13,F10:121}[name]});
    await idle();
  };
  const menu = ".panel-context-menu";
  const opened = () => evaluate(`document.querySelector('${menu}').matches(':popover-open')`);
  const check = async () => {
    assert.equal(await opened(),true);
    const current=await view();
    const rows=await evaluate(`Array.from(document.querySelectorAll('${menu} button'),b=>({label:b.querySelector('.menu-label').textContent,selected:b.hasAttribute('aria-checked')?b.getAttribute('aria-checked')==='true':null,enabled:!b.disabled}))`);
    assert.deepEqual(rows,current.switcher_options.sections.flat().map(row=>({label:row.label,selected:row.selected,enabled:row.enabled})));
    assert.equal(await evaluate(`document.querySelector('${menu}').firstElementChild.matches('button[aria-checked]')`),true,"options start with the workspace checklist");
    assert.equal(current.page,null);
  };
  const dir=process.env.LAYER_TEST_ARTIFACTS||"artifacts/workspace-options/web";
  await mkdir(dir,{recursive:true});
  const shot=async name=>{const image=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${dir}/${name}.png`,Buffer.from(image.data,"base64"));};
  await idle();
  const created=[];
  const original=await view(), saved=await evaluate("layerApp.app.workspace_persistence()"), theme=await evaluate("layerApp.state().theme");
  const fixture=structuredClone(saved);
  fixture.layout.header={size:"small",zones:[[],[{id:1,item:{kind:"workspaces"}}],[]],next_id:2};
  const dots=".workspace-switcher-options";
  const choice=id=>`.workspace-switcher [data-workspace-id=${JSON.stringify(id)}]`;
  try {
    for (const theme of ["dark","light"]) {await send({type:"set_theme",theme});await shot(`${theme}-default-header`);}
    for (const theme of ["dark","light"]) {
      await send({type:"restore_workspace",workspace:fixture});
      await send({type:"set_theme",theme});
      await shot(`${theme}-closed-header`);
      const before=await evaluate("layerApp.app.workspace_capture()"), active=(await view()).id, inactive=(await view()).switcher_display.find(row=>row.id!==active).id;
      assert.equal(await evaluate(`document.querySelector('${dots}').getAttribute('aria-label')`),(await view()).switcher_options_label);
      assert.equal(await evaluate(`document.querySelector('${dots} svg').dataset.asset`),"more-small");
      assert.deepEqual(await evaluate(`(()=>{const p=document.querySelector('.workspace-switcher').getBoundingClientRect(),b=document.querySelector('${dots}').getBoundingClientRect();return{width:b.width,height:b.height,gap:b.left-document.querySelector('.workspace-switcher-choices').getBoundingClientRect().right,top:b.top-p.top,bottom:p.bottom-b.bottom}})()`),{width:20,height:26,gap:2,top:5,bottom:5},"options match workspace pill height within the well");
      for (const kind of ["mouse","touch","pen"]) {
        device=kind;await click(dots);await check();await shot(`${theme}-${kind}`);await key("Escape");
        assert.equal(await evaluate(`document.activeElement.matches('${dots}')`),true,"Escape returns focus to dots");
      }
      device="mouse";
      const well=await evaluate("(()=>{const r=document.querySelector('.workspace-switcher').getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+1}})()");
      for (const p of [await center(choice(active)),await center(choice(inactive)),await center(dots),well]) {
        await pointer("down",p,"right");await pointer("up",p,"right");await check();assert.equal((await view()).id,active);await key("Escape");
      }
      await evaluate(`document.querySelector('${dots}').focus()`);await key("F10",8);await check();await key("Escape");
      for (const kind of ["touch","pen"]) {
        device=kind;await pointer("down",await center(choice(inactive)));await pause(600);await check();await pointer("up");await idle();
        assert.equal((await view()).id,active,"hold release does not switch");await key("Escape");
        await pointer("down",await center(choice(inactive)));await pointer("move",{x:point.x,y:point.y+60});await pointer("up");await pause(550);assert.equal(await opened(),false,"motion cancels pending hold");
      }
      device="mouse";
      const toggle=async id=>{
        await click(dots);await check();const index=(await view()).switcher_options.sections[0].findIndex(row=>row.action.command.id===id);
        await click(`${menu} button:nth-of-type(${index+1})`);assert.equal(await opened(),false,"visibility activation closes menu");
      };
      for (const row of (await view()).switcher_options.sections[0].filter(row=>row.selected))await toggle(row.action.command.id);
      assert.deepEqual((await view()).switcher,[]);assert.equal((await view()).id,active);
      assert.deepEqual((await view()).switcher_display.map(row=>row.id),[active]);
      await click(dots);await check();assert.ok((await view()).switcher_options.sections[0].every(row=>!row.selected));await key("Escape");
      for (const row of original.switcher)await toggle(row.id);
      assert.equal(await evaluate("layerApp.app.workspace_capture()"),before,"pin changes preserve workspace and history");
      await click(dots);await click(`${menu} button:last-of-type`);assert.equal((await view()).page,"workspaces");
      assert.equal(await evaluate("document.querySelector('.workspace-options svg').dataset.asset"),"more","full-height editor options retain the full icon");
      assert.ok(await evaluate("Array.from(document.querySelectorAll('.workspace-grip svg'),n=>n.getBoundingClientRect()).every(r=>r.width===16&&r.height===16)"),"editor grips retain the shared icon canvas");
      await shot(`${theme}-manage-workspaces`);
      await click(".workspace-manager footer button");assert.equal((await view()).page,null);
      await send({type:"customize",action:{type:"header",action:{type:"edit",editing:true}}});
      const header=await evaluate("layerApp.state().workspace.layout.header");
      const grips=await evaluate("Array.from(document.querySelectorAll('.header-item-grip svg,.header-component-grip svg'),n=>({width:n.getBoundingClientRect().width,height:n.getBoundingClientRect().height})).filter(r=>r.width&&r.height)");
      assert.ok(grips.length&&grips.every(r=>r.width===16&&r.height===16),"title-bar grips retain the shared icon canvas");
      await click(dots);await check();await shot(`${theme}-title-bar-edit`);await key("Escape");
      for (const selector of [choice(active),choice(inactive),dots]) {
        const p=await center(selector);await pointer("down",p,"right");await pointer("up",p,"right");await check();await key("Escape");
      }
      await evaluate(`document.querySelector('[data-kind="workspaces"]').focus()`);await key("F10",8);await check();await key("Escape");
      assert.deepEqual(await evaluate("layerApp.state().workspace.layout.header"),header);
      await click("#header-edit-cancel");
      await click(choice(inactive));assert.equal((await view()).id,inactive,"left click switches");await input({type:"switch",id:active});
    }
    for (let n=0;n<28;n++) {
      await input({type:"form",action:{type:"new"}});await input({type:"submit",name:`Wide 水彩 painting workspace ${n}`});created.push((await view()).id);
    }
    await input({type:"switch",id:original.id});
    for (const id of created)await input({type:"edit_switcher",edit:{type:"show",id,visible:false}});
    await send({type:"restore_workspace",workspace:fixture});
    for (const theme of ["dark","light"]) {
      await send({type:"set_theme",theme});
      const geometry=await evaluate(`(()=>{const p=document.querySelector('.workspace-switcher'),c=p.querySelector('.workspace-switcher-choices'),b=p.querySelector('${dots}');p.style.width='130px';const before=b.getBoundingClientRect().toJSON();c.scrollLeft=c.scrollWidth;return{before,after:b.getBoundingClientRect().toJSON(),scrolled:c.scrollLeft>0}})()`);
      assert.equal(geometry.scrolled,true);assert.deepEqual(geometry.before,geometry.after,"dots stay fixed while choices scroll");
      await evaluate("document.querySelector('.workspace-switcher').style.removeProperty('width')");await settle();
      device="mouse";await click(dots);await check();
      const scroll=await evaluate(`(()=>{const n=document.querySelector('${menu}'),r=n.getBoundingClientRect();n.scrollTop=n.scrollHeight;return{overflow:n.scrollHeight>n.clientHeight,scrolled:n.scrollTop>0,top:r.top,bottom:r.bottom,height:innerHeight}})()`);
      assert.ok(scroll.overflow&&scroll.scrolled);assert.ok(scroll.top>=0&&scroll.bottom<=scroll.height);
      await shot(`${theme}-long-list`);await key("Escape");
    }
    console.log("PASS: Web workspace options, all secondary-click targets, mouse/touch/pen dots, touch/pen hold and cancellation, keyboard/Escape focus, no pins, visibility history isolation, Manage, title-bar editor, fixed dots during choice scrolling and long hidden/localized lists in both themes");
  } catch (error) {await shot("failure");throw error;}
  finally {await input({type:"switch",id:original.id});for(const id of created){await input({type:"form",action:{type:"delete",value:id}});await input({type:"submit",name:""});}await send({type:"restore_workspace",workspace:saved});await send({type:"set_theme",theme});}
}

export async function checkWorkspaceFocus({evaluate, settle}) {
  await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+5000;function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v.ready&&!v.busy&&!v.switcher_busy)resolve();else if(performance.now()>deadline)reject(Error('Workspace startup did not finish'));else setTimeout(check,20);}check();})`);
  const originalTheme = await evaluate("layerApp.state().theme");
  for (const theme of ["dark", "light"]) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
    await settle();
    const result = await evaluate(`(async () => {
      const root=document.querySelector('.workspace-switcher');
      const snapshot=()=>[...root.querySelectorAll("[data-workspace-id]")].map(node=>({id:node.dataset.workspaceId,disabled:node.disabled,opacity:getComputedStyle(node).opacity,color:getComputedStyle(node).color,active:node.getAttribute('aria-pressed')}));
      const before=snapshot(), nodes=[...root.querySelectorAll("[data-workspace-id]")], samples=[];
      for(let attempt=0;attempt<3;attempt++) {
        window.dispatchEvent(new Event('blur'));
        window.dispatchEvent(new Event('focus'));
        const deadline=performance.now()+5000;
        do {
          samples.push(snapshot());
          if(performance.now()>deadline)throw Error('Focus refresh did not finish');
          await new Promise(resolve=>requestAnimationFrame(resolve));
        } while(JSON.parse(layerApp.app.workspace_view()).switcher_busy);
        samples.push(snapshot());
      }
      return {before,samples,sameNodes:nodes.every((node,index)=>root.querySelectorAll("[data-workspace-id]")[index]===node)};
    })()`);
    assert.ok(result.before.length && result.before.every(node=>!node.disabled));
    assert.ok(result.sameNodes,"focusing retains the header buttons");
    for(const sample of result.samples)assert.deepEqual(sample,result.before,`${theme}: focus refresh must not dim or disable workspace choices`);
  }
  await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(originalTheme)}})`); await settle();
  const switching = await evaluate(`(async () => {
    const view=()=>JSON.parse(layerApp.app.workspace_view()), original=view().id;
    const next=[...document.querySelectorAll('.workspace-switcher button[data-workspace-id]')].find(node=>node.dataset.workspaceId!==original);
    const wait=async id=>{const deadline=performance.now()+5000;while(view().id!==id||view().busy||view().switcher_busy){if(performance.now()>deadline)throw Error('Switch did not finish');await new Promise(resolve=>setTimeout(resolve,20));}};
    window.dispatchEvent(new Event('focus'));
    const refreshing=view().switcher_busy;
    next.click();
    const busy=view().busy, disabled=[...document.querySelectorAll('.workspace-switcher button[data-workspace-id]')].every(node=>node.disabled);
    await wait(next.dataset.workspaceId);
    [...document.querySelectorAll('.workspace-switcher button[data-workspace-id]')].find(node=>node.dataset.workspaceId===original).click();
    await wait(original);
    return {refreshing,busy,disabled};
  })()`);
  assert.deepEqual(switching,{refreshing:true,busy:true,disabled:true},"switches work during a preference refresh and disable choices during the actual transition");
  console.log("PASS: Web workspace choices retain brightness, enabled state and selection through repeated focus refreshes in both themes");
}

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
  const shown = () => evaluate("[...document.querySelectorAll('.workspace-switcher button[data-workspace-id]')].map(node=>node.dataset.workspaceId)");
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
  await checkWorkspaceFocus({evaluate,settle});
  const initial=await view(), [p,i,f]=['builtin:workspace:painter','builtin:workspace:illustrator','builtin:workspace:photographer'], original=await durable(), originalLayout=await layout();
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
      await send({type:"form",action:{type:"new"}}); await send({type:"submit",name:n?`Study ${n}`:"Sketching"}); custom.push((await view()).id);
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
    // The shared title bar uses its compact Window menu when the retained
    // switcher cannot fit; every workspace stays reachable through its manager.
    for(const id of custom)await send({type:"edit_switcher",edit:{type:"show",id,visible:true}});
    assert.ok(await evaluate("document.querySelector('.workspace-switcher').hidden"));
    assert.ok(await evaluate("!document.querySelector('#header-workspace-selector').hidden"));
    const compactSwitch=async id=>{
      await click('#header-workspace-selector > summary');
      const title=(await view()).switcher_display.find(row=>row.id===id).title;
      await evaluate(`(()=>{const b=[...document.querySelectorAll('#header-workspace-selector .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(title)});if(!b)throw Error('Missing '+${JSON.stringify(title)});b.dataset.compactSwitch='true';})()`);
      await click('[data-compact-switch]');
    };
    const beforeRestart={pins:await pins(),order:await order()};
    await click(".workspace-manager footer button"); await reload(); await wait(ready); await idle();
    assert.deepEqual(await pins(),beforeRestart.pins); assert.deepEqual(await order(),beforeRestart.order); assert.deepEqual(await shown(),[initial.id,...beforeRestart.pins]);
    assert.deepEqual(await durable(),original,"restart preserves original workspace and history");
    await compactSwitch(custom[0]);
    assert.equal((await view()).id,custom[0]);
    assert.equal(await evaluate(`document.querySelector('.workspace-switcher button[data-workspace-id=${JSON.stringify(custom[0])}]').getAttribute('aria-pressed')`),"true");
    assert.deepEqual(await shown(),await pins(),"temporary entry disappears when switching to a pinned workspace");
    await send({type:"edit_switcher",edit:{type:"show",id:custom[0],visible:false}});
    assert.deepEqual(await shown(),[custom[0],...await pins()]);
    await compactSwitch(p);
    assert.deepEqual(await shown(),await pins());
    console.log("PASS: configurable Web pill, narrow grips on every row, mouse/touch/pen pickup and menus, same-contact drag, hidden-row order, keyboard, scrolling, cancel/blur, preview preservation, cross-tab refresh and restart");
  } catch(error) {await shot("failure");console.error("Workspace switcher failure",await view());await writeFile(`${artifacts}/events.json`,JSON.stringify(await evaluate("window.workspaceEvents ?? []"),null,2));throw error;}
  finally {if(pressed)await pointer(device==="touch"?"cancel":"up");}
}
