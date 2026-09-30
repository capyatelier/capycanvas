import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const COLORS={mouse:[.85,.08,.05,1],touch:[.05,.6,.1,1],pen:[.1,.2,.85,1]};
const CORNERS={mouse:'bottom_right',touch:'top_left',pen:'bottom_left'};
const dialog='#canvas-size-dialog';

export async function checkCanvasSize({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/canvas-size/web-tablet':'artifacts/canvas-size/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`);
    await send({type:'invoke',command});
  };
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const size=async()=>{const tab=(await state()).tabs[0];return[tab.width,tab.height];};
  const view=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_tools.canvas_size))');
  const status=()=>evaluate(`document.querySelector('#status').textContent`);
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=300;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(points,kind)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',points[0],kind);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,kind);await settle();}
    await pointer('mouseReleased',points.at(-1),kind);await settle();
  };
  const line=(a,b,steps=8)=>Array.from({length:steps+1},(_,i)=>({x:a.x+(b.x-a.x)*i/steps,y:a.y+(b.y-a.y)*i/steps}));
  const menuRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const choose=async(menu,path,kind)=>{
    await tap(await middle(`.header-menu[data-menu="${menu}"] > summary`),kind);
    await wait(`document.querySelector('.header-menu[data-menu="${menu}"]').open`);
    for(const label of path) {
      await wait(`!!${menuRow(label)}&&!${menuRow(label)}.disabled`);
      await tap(await evaluate(`(()=>{const r=${menuRow(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
    }
    await wait(`!document.querySelector('.header-menu[data-menu="${menu}"]').open`);
  };
  const bar='.canvas-action-bar';
  const openMenu='.panel-context-menu:popover-open';
  const pressBar=async(command,kind)=>{
    const selector=`${bar} [data-command="${command}"]`;
    await wait(`!!document.querySelector('${selector}')&&(b=>!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`);
    if(!await evaluate(`document.querySelector('${selector}').closest('.canvas-action-bar-item').hidden`))return tap(await middle(selector),kind);
    await tap(await middle(`${bar} .canvas-action-bar-more`),kind);
    const label=await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').label`);
    const row=`[...document.querySelectorAll('${openMenu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
    await wait(`!!${row}`);
    await tap(await evaluate(`(()=>{const r=${row}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
    return 'more';
  };
  const tool=async command=>{
    if(await evaluate("layerApp.state().layer_tools.tool==='pick_visible'"))await invoke('eyedropper');
    await invoke(command);
  };
  const sample=async(p,test,label)=>{
    if(await evaluate("layerApp.state().layer_tools.tool!=='pick_visible'"))await invoke('eyedropper');
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
  const typeInto=async(axis,text,kind)=>{
    const entry=`${dialog} [data-canvas-size="${axis}"] .number-entry`;
    await tap(await middle(entry),kind);
    try{await wait(`document.activeElement===document.querySelector('${entry}')`,3000);}
    catch{assert.fail(`${kind}: ${axis} takes focus for editing; focus is on ${await evaluate('document.activeElement?.outerHTML.slice(0,120)')}`);}
    await evaluate(`document.querySelector('${entry}').select()`);
    await call('Input.insertText',{text});await settle();
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    const [width,height]=await size();

    const strokeAt=async(points,rgba)=>{
      await tool('pen');await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:40});await send({type:'set_color',rgba});
      await drag(points,'pen');
    };
    let refused=0;
    for(let attempt=0;attempt<4;attempt++) {
      await strokeAt(line(await screen(width*.6,height*.6),await screen(width*.8,height*.62)),COLORS.pen);
      const outcome=await evaluate(`(()=>{const send=action=>layerApp.dispatch({type:'canvas_size',action});
        layerApp.dispatch({type:'invoke',command:'canvas_size'});
        send({op:'anchor',anchor:'bottom_right'});send({op:'width',value:${width+300}});send({op:'height',value:${height+300}});
        const ready=layerApp.state().layer_tools.canvas_size.can_apply,message=layerApp.state().layer_tools.canvas_size.message;
        send({op:'apply'});
        const tab=layerApp.state().tabs[0];return{ready,message,size:[tab.width,tab.height],status:document.querySelector('#status').textContent,open:!!layerApp.state().layer_tools.canvas_size};})()`);
      await settle();
      if(outcome.size[0]!==width+300){
        refused++;
        console.log(`Growing left and up right after a stroke was refused: ${JSON.stringify(outcome)}`);
        if(outcome.open)await send({type:'canvas_size',action:{op:'cancel'}});
      } else {
        assert.ok(!outcome.status.includes('busy'),JSON.stringify(outcome));
        await invoke('undo');
        assert.deepEqual(await size(),[width,height],'Undo restores the size after growing left and up');
      }
      await invoke('undo');
    }
    assert.equal(refused,0,'Growing the canvas left and up right after a stroke is never refused');

    for(const kind of ['mouse','touch','pen']) {
      const corner=CORNERS[kind],rgba=COLORS[kind];
      const [right,bottom]=[corner.endsWith('right'),corner.startsWith('bottom')];
      const cut=[right?Math.round(width*.45):-40,bottom?Math.round(height*.45):-40];
      const far=[right?width+40:Math.round(width*.55),bottom?height+40:Math.round(height*.55)];
      const inside=[(Math.max(cut[0],0)+Math.min(far[0],width))/2,(Math.max(cut[1],0)+Math.min(far[1],height))/2];
      const hidden=[right?width*.2:width*.8,bottom?height*.2:height*.8];
      await strokeAt(line(await screen(hidden[0]-80,hidden[1]),await screen(hidden[0]+80,hidden[1])),rgba);
      await strokeAt(line(await screen(inside[0]-60,inside[1]),await screen(inside[0]+60,inside[1])),rgba);
      const hiddenPoint=await screen(...hidden),insidePoint=await screen(...inside);
      await sample(hiddenPoint,p=>near(p,rgba),`${kind}: the stroke to be hidden is painted`);

      await tool('rectangle_select');
      await drag(line(await screen(...cut),await screen(...far),6),kind==='touch'?'pen':kind);
      await wait(`layerApp.state().layer_tools.has_selection&&layerApp.state().canvas_bar?.context.kind==='selection'`);
      const crop=await evaluate(`document.querySelector('${bar} [data-command="crop_canvas_to_selection"]').textContent.trim()`);
      assert.equal(crop,'Crop',`${kind}: the selection bar offers Crop`);
      if(await pressBar('crop_canvas_to_selection',kind)==='more')console.log(`${kind}: Crop is in More at this window width`);
      await wait(`layerApp.state().tabs[0].width<${width}&&layerApp.state().tabs[0].height<${height}`);
      const cropped=await size();
      await sample(insidePoint,p=>near(p,rgba),`${kind}: the image stays in place after the crop`);
      await sample(hiddenPoint,p=>!near(p,rgba),`${kind}: the cropped stroke is hidden`);
      await invoke('undo');
      assert.deepEqual(await size(),[width,height],`${kind}: one undo step restores the canvas`);
      await sample(hiddenPoint,p=>near(p,rgba),`${kind}: undo shows the cropped stroke again`);
      await invoke('redo');
      assert.deepEqual(await size(),cropped,`${kind}: redo crops again`);
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');

      await choose('edit',['Image',await evaluate(`layerApp.state().commands.find(c=>c.id==='canvas_size').label`)],kind);
      await wait(`!!document.querySelector('${dialog}')?.open`);
      assert.equal(await evaluate(`document.querySelector('${dialog}').contains(document.activeElement)&&!document.activeElement.matches('input,select')`),true,`${kind}: the dialog opens without focusing a field`);
      assert.equal(await evaluate(`document.querySelector('${dialog} footer .suggested-action').disabled`),true,`${kind}: Apply is disabled at the current size`);
      let values=[width,height];
      if(kind==='touch') {
        await tap(await middle(`${dialog} .size-dialog-check input`),kind);
        await wait(`layerApp.state().layer_tools.canvas_size.relative`);
        values=[width-cropped[0],height-cropped[1]];
      } else if(kind==='pen') {
        await evaluate(`(s=>{s.value='percent';s.dispatchEvent(new Event('change',{bubbles:true}));})(document.querySelector('${dialog} .size-dialog-select'))`);await settle();
        await wait(`layerApp.state().layer_tools.canvas_size.unit==='percent'`);
        values=[width/cropped[0]*100,height/cropped[1]*100].map(v=>Math.round(v*100)/100);
      }
      await typeInto('width',String(values[0]),kind);
      await typeInto('height',String(values[1]),kind);
      const cell=`${dialog} [data-anchor="${corner}"]`;
      await tap(await middle(cell),kind);
      await wait(`layerApp.state().layer_tools.canvas_size.anchor==='${corner}'`);
      assert.equal(await evaluate(`document.activeElement===document.querySelector('${cell}')`),false,`${kind}: the anchor picker never takes focus`);
      const drafted=await view();
      assert.deepEqual(drafted.values,values,`${kind}: typed values are committed before the anchor changes`);
      assert.equal(drafted.message,`New size: ${width} × ${height} px`);
      assert.equal(drafted.can_apply,true);
      assert.equal(await evaluate(`document.querySelector('${cell}').getAttribute('aria-pressed')`),'true');
      if(kind==='mouse')for(const name of ['light','dark']) {
        await send({type:'set_theme',theme:name});await pause(150);
        const r=await rect(dialog);
        await writeFile(`${directory}/canvas-size-${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{x:r.x-8,y:r.y-8,width:r.width+16,height:r.height+16,scale:1}})).data,'base64'));
      }
      await tap(await middle(`${dialog} footer .suggested-action`),kind);
      await wait(`!document.querySelector('${dialog}')&&!layerApp.state().layer_tools.canvas_size`);
      assert.deepEqual(await size(),[width,height],`${kind}: Canvas Size restores the original size (${await status()})`);
      await sample(hiddenPoint,p=>near(p,rgba),`${kind}: the hidden stroke reappears in its original place`);
      await sample(insidePoint,p=>near(p,rgba),`${kind}: the kept stroke stays in place`);
      await invoke('undo');
      assert.deepEqual(await size(),cropped,`${kind}: one undo step returns to the cropped canvas`);
      await sample(hiddenPoint,p=>!near(p,rgba),`${kind}: undo hides the stroke again`);
      await invoke('redo');
      assert.deepEqual(await size(),[width,height],`${kind}: redo restores the original size`);
      await sample(hiddenPoint,p=>near(p,rgba),`${kind}: redo shows the stroke again`);
    }
    console.log(`PASS canvas size (${device?'tablet':'desktop'}): Crop on the selection bar hides pixels in one undo step, and Edit › Image › Canvas Size… with the matching anchor restores the size and position so the hidden strokes reappear, with mouse, touch and pen; growing left and up right after a stroke is never refused; screenshots in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Canvas size state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,status:document.querySelector('#status').textContent,size:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height],
      tool:layerApp.state().layer_tools.tool,view:layerApp.state().layer_tools.canvas_size},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    if(await evaluate('!!layerApp.state().layer_tools.canvas_size'))await send({type:'canvas_size',action:{op:'cancel'}});
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme});
  }
}
