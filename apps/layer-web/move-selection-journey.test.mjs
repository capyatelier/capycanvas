import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const COLORS={pen:[.1,.2,.85,1],touch:[.05,.6,.1,1],mouse:[.85,.08,.05,1]};
const bar='.canvas-action-bar';

// Journey 26: select, then drag the selected pixels with Move, with and
// without Leave Copy, and with Alt held at the press.
export async function checkMoveSelection({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/move-selection/web-tablet':'artifacts/move-selection/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`);
    await send({type:'invoke',command});
  };
  const size=()=>evaluate('[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height]');
  const anchor=()=>evaluate('layerApp.state().canvas_bar?.anchor??null');
  const tool=()=>evaluate('layerApp.state().layer_tools.tool');
  const leaveCopy=()=>evaluate(`layerApp.state().commands.find(c=>c.id==='move_leave_copy').selected`);
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=500;
  const pointer=(type,p,kind,modifiers=0)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,modifiers,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(points,kind,during,modifiers=0)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',points[0],kind,modifiers);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,kind,modifiers);await settle();}
    const result=await during?.();
    await pointer('mouseReleased',points.at(-1),kind,modifiers);await settle();
    return result;
  };
  const line=(a,b,steps=8)=>Array.from({length:steps+1},(_,i)=>({x:a.x+(b.x-a.x)*i/steps,y:a.y+(b.y-a.y)*i/steps}));
  const alt=(pressed)=>call('Input.dispatchKeyEvent',{type:pressed?'rawKeyDown':'keyUp',key:'Alt',code:'AltLeft',windowsVirtualKeyCode:18,modifiers:pressed?1:0});
  const pressBar=async(command,kind)=>{
    const selector=`${bar} [data-command="${command}"]`;
    await wait(`!!document.querySelector('${selector}')&&(b=>!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`);
    assert.equal(await evaluate(`document.querySelector('${selector}').closest('.canvas-action-bar-item').hidden`),false,`${kind}: Leave Copy fits on the bar`);
    await tap(await middle(selector),kind);
  };
  const choose=async command=>{
    if(await tool()==='pick_visible')await invoke('eyedropper');
    await invoke(command);
  };
  const sample=async(p,test,label)=>{
    if(await tool()!=='pick_visible')await invoke('eyedropper');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+1,y:p.y,buttons:0,pointerType:'mouse'});
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'mouse'});await settle();
    let last;
    for(const end=Date.now()+10000;Date.now()<end;await pause(100)) {
      last=await evaluate('(p=>p?Array.from(p.rgba):null)(layerApp.state().color_picker.preview)');
      if(test(last))return last;
    }
    assert.fail(`${label}: ${JSON.stringify(last)}`);
  };
  const near=(rgba,expected)=>!!rgba&&rgba.slice(0,3).every((v,i)=>Math.abs(v-expected[i])<.03);
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:c.translation,v:c.viewport,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    await invoke('fit_canvas');
    const [width,height]=await size();
    const runs=['pen','touch','mouse'].flatMap(kind=>[[kind,false,false],[kind,true,false]]).concat([['mouse',false,true]]);
    for(const [kind,leave,held] of runs) {
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
      await choose('rectangle_select');
      await drag(line(await screen(width*.3,height*.3),await screen(width*.5,height*.5),6),kind==='touch'?'pen':kind);
      await wait(`layerApp.state().layer_tools.has_selection&&layerApp.state().canvas_bar?.context.kind==='selection'`);
      await send({type:'set_color',rgba:COLORS[kind]});
      await invoke('fill_selection');
      await choose('move');
      await wait(`layerApp.state().canvas_bar?.context.kind==='selection'`);
      if(await leaveCopy()!==leave) {
        await pressBar('move_leave_copy',kind);
        await wait(`layerApp.state().commands.find(c=>c.id==='move_leave_copy').selected===${leave}`);
      }
      const before=await anchor();
      const [dx,dy]=[Math.round(width*.3),Math.round(height*.25)];
      if(held)await alt(true);
      const during=await drag(line(await screen(width*.4,height*.4),await screen(width*.4+dx,height*.4+dy)),kind,
        ()=>evaluate(`({kind:layerApp.state().canvas_bar?.context.kind,tool:layerApp.state().layer_tools.tool})`),held?1:0);
      if(held)await alt(false);
      assert.deepEqual(during,{kind:'selection',tool:'move'},`${kind}: the bar keeps the selection context and Move stays the tool during the drag`);
      await wait(`(a=>!!a&&Math.abs(a[0]-${before[0]+dx})<2&&Math.abs(a[1]-${before[1]+dy})<2)(layerApp.state().canvas_bar?.anchor)`);
      const moved=await anchor();
      assert.ok([0,1].every(i=>{const d=moved[i]-before[i];return Math.abs(d-Math.round(d))<1e-3;}),`${kind}: whole pixels ${JSON.stringify([before,moved])}`);
      if(kind==='mouse'&&leave)for(const name of ['light','dark']) {
        await send({type:'set_theme',theme:name});await pause(250);
        const r=await rect(bar);
        await writeFile(`${directory}/leave-copy-${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{x:Math.max(0,r.x-40),y:Math.max(0,r.y-160),width:r.width+80,height:r.height+200,scale:1}})).data,'base64'));
        assert.equal(await evaluate(`document.querySelector('${bar} [data-command="move_leave_copy"]').getAttribute('aria-pressed')`),'true',`${name}: Leave Copy shows as on`);
      }
      const keeps=leave!==held;
      await sample(await screen(width*.4+dx,height*.4+dy),p=>near(p,COLORS[kind]),`${kind}: the pixels arrive`);
      await sample(await screen(width*.32,height*.32),p=>near(p,COLORS[kind])===keeps,
        `${kind} Leave Copy ${leave} Alt ${held}: the original ${keeps?'stays':'is cut'}`);
      await invoke('undo');
      await wait(`(a=>!!a&&Math.abs(a[0]-${before[0]})<.01&&Math.abs(a[1]-${before[1]})<.01)(layerApp.state().canvas_bar?.anchor)`);
      await sample(await screen(width*.32,height*.32),p=>near(p,COLORS[kind]),`${kind}: one undo puts the pixels back`);
    }
    if(await leaveCopy())await invoke('move_leave_copy');
    console.log(`PASS move selection (${device?'tablet':'desktop'}): Move drags the selected pixels by whole pixels with pen, touch and mouse, the selection follows, Leave Copy and Alt keep the original, and one undo restores it; screenshots in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Move selection state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector('#status').textContent,
      tool:layerApp.state().layer_tools.tool,bar:layerApp.state().canvas_bar?.context},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await send({type:'set_color_sample_size',width:sampleWidth});
    if(theme)await send({type:'set_theme',theme});
  }
}
