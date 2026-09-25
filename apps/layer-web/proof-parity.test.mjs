import assert from 'node:assert/strict';

// Verify the worker-loaded illustration against GTK's shared Rust texture.
// An opaque gray placeholder used to pass the browser's visual check.
export async function checkProofPattern({evaluate}) {
  const result=await evaluate(`(async()=>{
    const canvas=document.querySelector('.proof-tone-pad');
    if(!canvas?.getBoundingClientRect().width)throw Error('SDR Proof must be visible');
    const source=document.createElement('canvas');source.width=source.height=512;
    source.getContext('2d',{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(layerApp.app.proof_texture(512)),512,512),0,0);
    const deadline=performance.now()+15000;
    for(;;){
      const size=canvas.getBoundingClientRect().width;
      const d=layerApp.app.color_ui({type:'proof_dial',size,recipe:layerApp.app.proof_form().rendition});
      const reference=document.createElement('canvas');reference.width=canvas.width;reference.height=canvas.height;
      const ctx=reference.getContext('2d',{willReadFrequently:true}),scale=canvas.width/size,[cx,cy]=d.center;
      ctx.scale(scale,scale);ctx.drawImage(source,cx-d.radius,cy-d.radius,d.radius*2,d.radius*2);
      const expected=ctx.getImageData(0,0,canvas.width,canvas.height).data;
      const actual=canvas.getContext('2d').getImageData(0,0,canvas.width,canvas.height).data;
      let samples=0,maxError=0;const colors=new Set();
      for(const x of [-.6,-.3,0,.3,.6])for(const y of [-.6,-.3,0,.3,.6]){
        const px=cx+x*d.radius,py=cy+y*d.radius;
        if(Math.hypot(px-d.marker[0],py-d.marker[1])<d.marker_radius+6)continue;
        const offset=(Math.floor(py*scale)*canvas.width+Math.floor(px*scale))*4;
        for(let channel=0;channel<4;channel++)maxError=Math.max(maxError,Math.abs(actual[offset+channel]-expected[offset+channel]));
        colors.add(Array.from(actual.slice(offset,offset+3)).join(','));samples++;
      }
      const result={samples,distinctColors:colors.size,maxError};
      if(samples>=16&&colors.size>=12&&maxError<=2)return result;
      if(performance.now()>deadline)throw Error('Proof pattern differs from GTK texture: '+JSON.stringify(result));
      await new Promise(resolve=>setTimeout(resolve,100));
    }
  })()`);
  assert.ok(result.samples>=16&&result.distinctColors>=12&&result.maxError<=2,JSON.stringify(result));
  console.log('SDR Proof illustration matches the shared GTK texture:',result);
  return result;
}

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
    const panels=await evaluate(`(()=>{const l=layerApp.app.layout(innerWidth,innerHeight);return [...l.groups.map(g=>g.panels),...l.collapsed.flatMap(c=>c.groups.map(g=>g.icons.map(i=>i.panel)))].find(p=>p.includes('navigator'))})()`);
    assert.ok(panels,`${name} Navigator group`);
    assert.equal(panels[panels.indexOf('navigator')+1],'proof',`${name}: Proof immediately follows Navigator after restoring`);
    assert.ok(await evaluate(`!![...document.querySelectorAll('.dock-tab[data-panel="proof"]')].find(n=>n.getBoundingClientRect().width>0)`));
  }
  console.log('Window → Workspaces → Restore Starting Layout restores adjacent Navigator/Proof tabs in Photo and Paint');
}

export async function checkProofKeys({call,evaluate,settle,invoke}) {
  const recipe=()=>evaluate('layerApp.app.proof_form().rendition');
  const focus=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);
  const key=(type,name,extra={})=>call('Input.dispatchKeyEvent',{type,key:name,code:name,windowsVirtualKeyCode:{ArrowLeft:37,ArrowRight:39,ArrowUp:38,Escape:27}[name],...extra});
  const before=await recipe();
  // The preceding pen gesture may leave balance at either endpoint. Exercise
  // an actual edit so Undo cannot consume that earlier gesture instead.
  const direction=before.balance>.9?-1:1,arrow=direction<0?'ArrowLeft':'ArrowRight';
  await focus('.proof-dial-reset');await focus('.proof-tone-pad');
  for(let i=0;i<3;i++)await key('keyDown',arrow,{autoRepeat:i>0});
  await key('keyUp',arrow);await settle();
  assert.ok(Math.abs((await recipe()).balance-(before.balance+direction*.03))<1e-6,JSON.stringify({before,after:await recipe()}));
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
