import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Merge Down from Ctrl+E and the Layer menu, and Stamp Visible, with mouse,
// touch and pen. Each keeps the look of the canvas and undoes in one step.
export async function checkMerges({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/merge/web-tablet':'artifacts/merge/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const command=async id=>(await state()).commands.find(c=>c.id===id);
  const layers=async()=>(await state()).layers;
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=700;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async points=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',points[0],'pen');
    for(const p of points.slice(1)){await pointer('mouseMoved',p,'pen');await settle();}
    await pointer('mouseReleased',points.at(-1),'pen');await settle();
  };
  const stroke=async points=>{
    const revision=`layerApp.state().layers.find(l=>l.editing).paint_revision`;
    const before=await evaluate(`String(${revision})`);
    await drag(points);
    await wait(`String(${revision})!==${JSON.stringify(before)}`);
  };
  const menuRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const openMenu=async(menu,kind)=>{
    await tap(await middle(`.header-menu[data-menu="${menu}"] > summary`),kind);
    await wait(`document.querySelector('.header-menu[data-menu="${menu}"]').open`);
  };
  const choose=async(menu,label,kind)=>{
    await openMenu(menu,kind);
    await wait(`!!${menuRow(label)}&&!${menuRow(label)}.disabled`);
    await tap(await evaluate(`(()=>{const r=${menuRow(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
    await wait(`!document.querySelector('.header-menu[data-menu="${menu}"]').open`);
  };
  const sample=async p=>{
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+1,y:p.y,buttons:0,pointerType:'mouse'});
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'mouse'});await settle();
    if(await evaluate("layerApp.state().layer_tools.tool!=='pick_visible'"))await invoke('eyedropper');
    await pause(150);
    let last;
    for(const end=Date.now()+10000;Date.now()<end;await pause(100)){
      last=await evaluate('(p=>p?Array.from(p.rgba):null)(layerApp.state().color_picker.preview)');
      if(last)return last;
    }
    assert.fail('the eyedropper samples the canvas');
  };
  const same=(a,b,label)=>assert.ok(a.every((v,i)=>Math.abs(v-b[i])<.01),`${label}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  try {
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    const c=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{v:c.viewport,a:c.work_area,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
    const center={x:c.r.x+(c.a[0]+c.a[2]/2)*c.r.width/c.v[0],y:c.r.y+(c.a[1]+c.a[3]/2)*c.r.height/c.v[1]};
    const at=(x,y)=>({x:center.x+x,y:center.y+y});
    const line=(a,b)=>Array.from({length:9},(_,i)=>({x:a.x+(b.x-a.x)*i/8,y:a.y+(b.y-a.y)*i/8}));
    await invoke('pen');await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:40});
    await send({type:'set_color',rgba:[.1,.3,.85,1]});
    await stroke(line(at(-150,0),at(150,0)));
    await invoke('add_layer');
    await send({type:'set_color',rgba:[.9,.6,.05,.7]});
    await stroke(line(at(0,-120),at(0,120)));
    const crossing=await sample(at(0,0));
    const count=(await layers()).length;

    await invoke('pen');
    await call('Input.dispatchKeyEvent',{type:'rawKeyDown',key:'e',code:'KeyE',windowsVirtualKeyCode:69,modifiers:2});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'e',code:'KeyE',windowsVirtualKeyCode:69,modifiers:2});
    await settle();
    await wait(`layerApp.state().layers.length===${count-1}`);
    same(await sample(at(0,0)),crossing,'Ctrl+E merges down without changing the canvas');
    await invoke('undo');
    await wait(`layerApp.state().layers.length===${count}`);

    for(const kind of ['mouse','touch','pen']) {
      assert.equal((await command('merge_down')).label,'Merge Down');
      await choose('layer','Merge Down',kind);
      await wait(`layerApp.state().layers.length===${count-1}`);
      same(await sample(at(0,0)),crossing,`${kind}: Layer › Merge Down keeps the canvas`);
      await invoke('undo');
      await wait(`layerApp.state().layers.length===${count}`);

      await choose('layer','Stamp Visible',kind);
      await wait(`layerApp.state().layers.length===${count+1}`);
      const stamped=await layers();
      assert.equal(stamped[0].label,'Visible',`${kind}: the stamp goes on top`);
      assert.ok(stamped[0].editing,`${kind}: the stamp is the active layer`);
      for(const layer of stamped.slice(1))
        await send({type:'layer',action:{op:'visibility',id:layer.id,value:false}});
      same(await sample(at(0,0)),crossing,`${kind}: the stamp alone shows the canvas`);
      for(let step=0;step<stamped.length;step++)await invoke('undo');
      await wait(`layerApp.state().layers.length===${count}&&layerApp.state().layers.every(l=>l.visible)`);
      await invoke('pen');
    }

    await invoke('pen');
    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});await pause(150);
      await openMenu('layer','mouse');
      await wait(`!!${menuRow('Merge Visible')}`);
      await writeFile(`${directory}/layer-menu-${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      await call('Input.dispatchKeyEvent',{type:'rawKeyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await wait(`!document.querySelector('.header-menu[data-menu="layer"]').open`);
    }
    console.log(`PASS merges (${device?'tablet':'desktop'}): Ctrl+E and Layer › Merge Down keep the canvas and undo in one step, and Stamp Visible adds the visible image on top, with mouse, touch and pen; Layer menus in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Merge state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,layers:layerApp.state().layers.map(l=>[l.id,l.label,l.editing])},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await send({type:'set_theme',theme});
  }
}
