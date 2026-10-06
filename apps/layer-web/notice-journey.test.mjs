import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const notice='.canvas-notice',action=`${notice} .canvas-notice-action`,bar='.canvas-action-bar';
const shown=`(()=>{const n=document.querySelector('${notice}');return !!n&&!n.hidden})()`;
const barShown=`(()=>{const b=document.querySelector('${bar}');return !!b&&!b.hidden&&!b.classList.contains('suppressed')})()`;
const WAND='This tool samples reference layers, and none is marked',LOCKED='The active layer is locked';

export async function checkNotices({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/notice/web-tablet':'artifacts/notice/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const current=()=>evaluate('(n=>n==null?null:JSON.parse(JSON.stringify(n,(_,v)=>typeof v==="bigint"?Number(v):v)))(layerApp.state().notice)');
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=70;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const canvasTap=async kind=>{await wait('layerApp.app.brush_ready()');await tap(center,kind);};
  const healthy=async label=>{
    assert.equal(await evaluate('layerApp.state().host_error??null'),null,`${label}: refusals are not host errors`);
    assert.equal(await evaluate("document.querySelector('#status').textContent"),'',`${label}: the error line stays empty`);
    assert.equal(await evaluate('document.body.dataset.gpu'),'ready',`${label}: the canvas keeps its GPU`);
    assert.ok(await evaluate("document.querySelector('#gpu-notice').hidden"),`${label}: no restart prompt`);
    const frames=await evaluate('noticeProbe.frames');
    await evaluate('layerApp.wake()');
    await wait(`noticeProbe.frames>${frames}`);
    assert.equal(await evaluate('document.activeElement===layerApp.canvas'),true,`${label}: the canvas keeps focus`);
  };
  const noPopup=async label=>assert.equal(await evaluate("!!document.querySelector('dialog[open], details[open], :popover-open:not(.hover-tooltip)')"),false,`${label}: the notice is not a popup`);
  const screenshot=async name=>{
    const r=await rect(notice),clip={x:Math.max(0,r.x-40),y:Math.max(0,r.y-40),width:r.width+80,height:r.height+80,scale:1};
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  let center;
  try {
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    await evaluate(`window.noticeProbe={frame:layerApp.app.frame,frames:0};layerApp.app.frame=(...args)=>{noticeProbe.frames++;return noticeProbe.frame.apply(layerApp.app,args);};`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    const c=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{v:c.viewport,a:c.work_area,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
    center={x:c.r.x+(c.a[0]+c.a[2]/2)*c.r.width/c.v[0],y:c.r.y+(c.a[1]+c.a[3]/2)*c.r.height/c.v[1]};

    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    const layers=(await state()).layers,top=layers.findIndex(l=>l.editing),below=layers[top+1];
    assert.ok(below&&!below.reference,'A paint layer lies below the new layer');
    const offer=`Use ${below.label} as Reference`;
    await invoke('auto_select');await invoke('selection_reference');
    for(const kind of ['pen','touch','mouse']) {
      const pointing=kind==='touch'?'pen':kind;
      await canvasTap(pointing);
      await wait(`${shown}&&document.querySelector('${notice} .canvas-notice-text').textContent===${JSON.stringify(WAND)}`);
      assert.equal(await evaluate(`document.querySelector('${action}').textContent`),offer,`${kind}: the action names the layer below`);
      assert.deepEqual((await current()).actions.map(a=>a.label),[offer]);
      await healthy(`${kind} Wand without a reference`);
      await noPopup(kind);
      await tap(await middle(action),kind);
      await wait(`!${shown}&&layerApp.state().notice==null&&layerApp.state().layers.find(l=>l.id==${below.id}).reference`);
      assert.equal(await evaluate('document.activeElement===layerApp.canvas'),true,`${kind}: the notice action does not take focus`);
      await invoke('undo');
      assert.equal((await state()).layers.find(l=>l.id===below.id).reference,false,`${kind}: marking the reference is one undo step`);
      await invoke('redo');
      assert.equal((await state()).layers.find(l=>l.id===below.id).reference,true);
      await canvasTap(pointing);await pause(200);
      assert.equal(await current(),null,`${kind}: the Wand now samples the reference`);
      assert.equal(await evaluate(shown),false);
      await healthy(`${kind} Wand with a reference`);
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
      await send({type:'layer',action:{op:'reference',id:below.id}});
      assert.equal((await state()).layers.find(l=>l.id===below.id).reference,false);
    }

    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});await pause(150);
      await canvasTap('pen');await wait(shown);await screenshot(`wand-${name}`);
    }
    await canvasTap('mouse');
    await wait(shown);
    const first=(await current()).id;
    await canvasTap('pen');
    await wait(`${shown}&&layerApp.state().notice?.id>${first}`);
    assert.equal(await evaluate(`(()=>{try{layerApp.app.dispatch({type:'notice',id:BigInt(${first}),accept:true});return 'accepted'}catch(e){return String(e)}})()`),'This notice was already dismissed','A replaced notice id is rejected');
    assert.equal((await state()).layers.find(l=>l.id===below.id).reference,false,'A stale accept runs nothing');
    assert.equal(await evaluate(shown),true,'The current notice stays');
    await canvasTap('touch');
    await wait(`!${shown}&&layerApp.state().notice==null`);
    await healthy('A navigating touch');

    await canvasTap('pen');
    await wait(shown);
    const raised=Date.now();
    await wait(`!${shown}`,8000);
    const elapsed=Date.now()-raised;
    assert.ok(elapsed>3000&&elapsed<6500,`The notice times out after about 4 s: ${elapsed} ms`);
    await wait('layerApp.state().notice==null');
    assert.equal((await state()).layers.find(l=>l.id===below.id).reference,false,'The timeout declines the action');
    await healthy('After the timeout');

    await invoke('select_all');await invoke('move');
    const active=(await state()).layers.find(l=>l.editing);
    await send({type:'layer',action:{op:'lock',id:active.id,value:true}});
    await wait(`${barShown}&&layerApp.state().commands.find(c=>c.id==='fill_selection').disabled_reason===${JSON.stringify(LOCKED)}`);
    const command=await evaluate(`[...document.querySelectorAll('${bar} .canvas-action-bar-item:not([hidden]) button[aria-disabled="true"][data-command]')][0]?.dataset.command`);
    assert.ok(command,'The locked layer disables a bar item that is shown');
    const disabled=`${bar} [data-command="${command}"]`,reason=await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').disabled_reason`);
    assert.equal(await evaluate(`document.querySelector('${disabled}').title`),reason,'A disabled bar item shows its reason on hover');
    for(const kind of ['pen','touch','mouse']) {
      await tap(await middle(disabled),kind);
      await wait(`document.querySelector('#hover-tooltip').matches(':popover-open')&&document.querySelector('#hover-tooltip').textContent===${JSON.stringify(reason)}`);
      assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true,`${kind}: tapping a disabled item runs nothing`);
      await evaluate("(t=>t.matches(':popover-open')&&t.hidePopover())(document.querySelector('#hover-tooltip'))");
    }
    let previous=0;
    for(const kind of ['pen','mouse','pen']) {
      await canvasTap(kind);
      await wait(`${shown}&&layerApp.state().notice?.id>${previous}&&document.querySelector('${notice} .canvas-notice-text').textContent===${JSON.stringify(LOCKED)}`);
      previous=(await current()).id;
      assert.equal(await evaluate(`document.querySelector('${notice} .canvas-notice-actions').hidden&&!document.querySelector('${action}')`),true,`${kind}: the refusal has no action`);
      await healthy(`${kind} Move on a locked layer`);
      await noPopup(kind);
    }
    await wait(barShown);await settle();
    const edge=await rect(bar),box=await rect(notice);
    if((await state()).canvas_bar.placement==='bottom_edge')
      assert.ok(box.bottom<=edge.y-4,`The notice sits above a bottom-edge bar ${JSON.stringify({box,edge})}`);
    else assert.ok(box.bottom<=edge.y||box.y>=edge.bottom||box.right<=edge.x||box.x>=edge.right,`The notice does not cover the bar ${JSON.stringify({box,edge})}`);
    await screenshot('move-locked');
    await send({type:'layer',action:{op:'lock',id:active.id,value:false}});
    await canvasTap('mouse');
    await wait(`!${shown}&&layerApp.state().notice==null`);
    console.log(`PASS canvas notice (${device?'tablet':'desktop'}): Wand without a reference offers "${offer}" and marks it in one step with pen, touch and mouse; stale ids, contact dismissal, the 4 s timeout, Move on a locked layer and disabled bar reasons; screenshots in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Notice state',await evaluate(`JSON.stringify({notice:layerApp.state().notice,error:layerApp.state().host_error,status:document.querySelector('#status').textContent,
      tool:layerApp.state().layer_tools.tool,layers:layerApp.state().layers.map(l=>[l.id,l.label,l.editing,l.reference])},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await evaluate('if(window.noticeProbe){layerApp.app.frame=noticeProbe.frame;delete window.noticeProbe;}');
    await send({type:'set_theme',theme});
  }
}
