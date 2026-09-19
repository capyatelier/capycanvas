import assert from 'node:assert/strict';

// GTK's restored arrangement and contact/history rules, through the host UI.
export async function checkProofStartingLayout({call,evaluate,settle}) {
  const touch=await evaluate("navigator.maxTouchPoints>0");
  const idle=()=>evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+15000;function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v.ready&&!v.busy&&!v.dirty)resolve(v);else if(performance.now()>deadline)reject(Error(JSON.stringify(v)));else setTimeout(check,50);}check();})`);
  const click=async selector=>{
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if(touch){await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...point,id:72,radiusX:3,radiusY:3,force:.5}]});await new Promise(r=>setTimeout(r,60));await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});}
    else {await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});}
    await settle();await new Promise(r=>setTimeout(r,200));
  };
  const label=async(text,root)=>{
    const selector=await evaluate(`(()=>{const n=[...document.querySelectorAll(${JSON.stringify(root+' button')})].find(n=>n.textContent===${JSON.stringify(text)});if(!n)throw Error('Missing '+${JSON.stringify(text)}+': '+document.querySelector(${JSON.stringify(root)})?.innerText);n.dataset.proofParity='target';return '[data-proof-parity="target"]'})()`);
    await click(selector);await evaluate(`document.querySelector('[data-proof-parity="target"]')?.removeAttribute('data-proof-parity')`);
  };
  for(const name of ['Photo','Paint']) {
    await label(name,'.workspace-switcher');
    assert.equal((await idle()).name,name,'The real workspace switch completed');
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'proof',visible:false}})`);await idle();await settle();
    const windowVisible=await evaluate(`document.querySelector('[data-menu="window"] > summary')?.getBoundingClientRect().width>0`);
    if(windowVisible)await click('[data-menu="window"] > summary');
    else {
      // Narrow tablet headers use the same application menu behind a button.
      const selector=await evaluate(`(()=>{const n=[...document.querySelectorAll('summary[aria-label="Application menus"],summary[aria-label="Main Menu"]')].find(n=>n.getBoundingClientRect().width>0);if(!n)throw Error('Missing visible application menu');n.dataset.proofParityMenu='target';return '[data-proof-parity-menu="target"]'})()`);
      await click(selector);await label('Window','.header-menu[open] .popover');
      await evaluate(`document.querySelector('[data-proof-parity-menu="target"]')?.removeAttribute('data-proof-parity-menu')`);
    }
    await label('Workspaces','.header-menu[open] .popover');
    await label('Restore Starting Layout…','.header-menu[open] .popover');
    await click('.workspace-form .suggested-action');
    await idle();await settle();
    const groups=await evaluate('layerApp.app.layout(innerWidth,innerHeight).groups');
    const group=groups.find(g=>g.panels.includes('color'));
    assert.ok(group,`${name} Color group`);
    assert.equal(group.panels[group.panels.indexOf('color')+1],'proof',`${name}: Proof immediately follows Color after restoring`);
    assert.ok(await evaluate(`!![...document.querySelectorAll('.dock-tab[data-panel="proof"]')].find(n=>n.getBoundingClientRect().width>0)`));
  }
  console.log('Window → Workspaces → Restore Starting Layout restores adjacent Color/Proof tabs in Photo and Paint');
}

export async function checkProofKeys({call,evaluate,settle,invoke}) {
  const recipe=()=>evaluate('layerApp.app.proof_form().rendition');
  const focus=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);
  const key=(type,name,extra={})=>call('Input.dispatchKeyEvent',{type,key:name,code:name,windowsVirtualKeyCode:{ArrowRight:39,ArrowUp:38,Escape:27}[name],...extra});
  const before=await recipe();
  await focus('.proof-dial-reset');await focus('.proof-tone-pad');
  for(let i=0;i<3;i++)await key('keyDown','ArrowRight',{autoRepeat:i>0});
  await key('keyUp','ArrowRight');await settle();
  assert.ok(Math.abs((await recipe()).balance-Math.min(1,before.balance+.03))<1e-6,JSON.stringify({before,after:await recipe()}));
  await invoke('undo');assert.deepEqual(await recipe(),before,'Held arrow is one undo');
  await key('keyDown','ArrowUp',{modifiers:8});await key('keyDown','Escape');await key('keyUp','Escape');await key('keyUp','ArrowUp');
  assert.deepEqual(await recipe(),before,'Escape restores the complete gesture');
  await focus('.proof-tone-pad');await key('keyDown','ArrowRight');
  await focus('.proof-dial-reset');await key('keyUp','ArrowRight');
  assert.deepEqual(await recipe(),before,'Losing focus cancels an uncommitted gesture');
  await focus('.proof-dial-accessibility [aria-label="Brightness"]');
  await key('keyDown','ArrowUp');await key('keyUp','ArrowUp');
  assert.ok(Math.abs((await recipe()).exposure-Math.min(2,before.exposure+.04))<1e-6);
  await invoke('undo');assert.deepEqual(await recipe(),before,'Brightness uses the GTK numeric step');
  await focus('.proof-dial-accessibility [aria-label="Color intensity"]');
  await key('keyDown','ArrowUp',{modifiers:8});await key('keyUp','ArrowUp');
  assert.ok(Math.abs((await recipe()).highlight_color-Math.min(1,before.highlight_color+.1))<1e-6);
  await invoke('undo');assert.deepEqual(await recipe(),before);
  console.log('GTK keyboard increments, held-key one-step undo, Escape and focus-loss cancellation pass');
}
