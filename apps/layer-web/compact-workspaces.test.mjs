import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkCompactWorkspaces({call,evaluate,settle}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/title-bar/compact-workspaces';
  await mkdir(dir,{recursive:true});
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const view=()=>evaluate('JSON.parse(layerApp.app.workspace_view())');
  const idle=async()=>{for(let i=0;i<200;i++){const v=await view();if(v?.ready&&!v.busy&&!v.switcher_busy&&!v.dirty){await settle();return;}await pause(50);}throw Error('Workspace did not settle');};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await idle();};
  const input=async value=>{await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`);await idle();};
  const click=async(selector,device='mouse')=>{
    const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...p}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    } else for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,pointerType:device});
    await pause(150);await idle();
  };
  const shot=async name=>{const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${name}.png`,Buffer.from(s.data,'base64'));};
  await idle();
  const saved=await evaluate('layerApp.app.workspace_persistence()');
  const original=(await view()).id;
  const ids=(await view()).switcher_display.map(row=>row.id);
  await input({type:'edit_switcher',edit:{type:'move',id:ids[2],before:ids[0]}});
  const fixture=structuredClone(saved);
  let next=1;
  const entry=kind=>({id:next++,item:{kind}});
  fixture.layout.header={size:'small',zones:[['capy','menu','settings',...Array(8).fill('space')].map(entry),[entry('workspaces')],Array.from({length:8},()=>entry('space'))],next_id:next};
  const workspaceId=fixture.layout.header.zones[1][0].id;
  for(const theme of ['dark','light'])for(const [index,size] of ['small','medium','large'].entries()) {
    const device=['mouse','touch','pen'][index];
    await call('Emulation.setDeviceMetricsOverride',{width:480,height:870,deviceScaleFactor:theme==='dark'?1:2,mobile:false});
    fixture.layout.header.size=size;
    await send({type:'restore_workspace',workspace:fixture});
    await send({type:'set_theme',theme});
    const dimensions=await evaluate(`Array.from(document.querySelectorAll('.header-overflow:not([hidden]) > summary > svg'),n=>{const r=n.getBoundingClientRect(),p=n.parentElement.getBoundingClientRect();return{width:r.width,height:r.height,dx:r.x+r.width/2-p.x-p.width/2,dy:r.y+r.height/2-p.y-p.height/2}})`);
    assert.ok(dimensions.length);
    for(const r of dimensions){assert.equal(r.width,[20,28,36][index]);assert.equal(r.height,r.width);assert.ok(Math.abs(r.dx)<1&&Math.abs(r.dy)<1);}
    await click('#header-workspace-selector > summary',device);
    const current=await view(),choices=current.switcher_display;
    const menu='#header-workspace-selector .popover';
    const rows=await evaluate(`Array.from(document.querySelectorAll('${menu} button'),b=>({label:b.querySelector('.menu-label').textContent,checked:b.getAttribute('aria-checked')}))`);
    assert.deepEqual(rows,choices.map(row=>({label:row.title,checked:String(row.id===current.id)})));
    await shot(`${theme}-${size}`);
    const active=(await view()).id,target=choices.find(row=>row.id!==active);
    await click(`${menu} button:nth-child(${choices.indexOf(target)+1})`,device);
    assert.equal((await view()).id,target.id);
    assert.equal(await evaluate("document.querySelector('#header-workspace-selector')?.open || false"),false);
  }
  // The same selector remains usable when its whole item is in overflow.
  fixture.layout.header.zones[0].push(fixture.layout.header.zones[1].pop());
  await send({type:'restore_workspace',workspace:fixture});
  await click('#header-overflow-0 > summary');
  await click(`[data-header-overflow-item="${workspaceId}"]`);
  const current=await view(),target=current.switcher_display.find(row=>row.id!==current.id);
  const index=current.switcher_display.indexOf(target)+1;
  const selector=`#header-overflow-0 .popover button:nth-child(${index})`;
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);
  for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13,...(type==='keyDown'?{text:'\r',unmodifiedText:'\r'}:{})});
  await pause(150);await idle();assert.equal((await view()).id,target.id);
  await input({type:'switch',id:original});
  await send({type:'restore_workspace',workspace:saved});
  console.log('PASS: compact and overflow workspace choices, configured order/current selection, mouse/touch/pen/keyboard, all icon sizes, both themes and 1x/2x');
}
