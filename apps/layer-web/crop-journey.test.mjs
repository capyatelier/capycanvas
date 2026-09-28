import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const BLUE=[.1,.3,.8,1];
const bar='.canvas-action-bar';
const visible=`(b=>!!b&&!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`;

export async function checkCrop({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/crop/web-tablet':'artifacts/crop/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`);
    await send({type:'invoke',command});
  };
  const size=async()=>evaluate('(t=>[t.width,t.height])(layerApp.state().tabs[0])');
  const tool=()=>evaluate('layerApp.state().layer_tools.tool');
  const setting=id=>evaluate(`layerApp.state().tool_settings.find(s=>s.id===${JSON.stringify(id)})?.value??null`);
  const selected=command=>evaluate(`!!layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.selected`);
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:[...c.translation],v:[...c.viewport],r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const key=async(key,code,vk)=>{
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk,...(type==='keyDown'&&key.length===1?{text:key,unmodifiedText:key}:{})});
    await settle();
  };
  let touchId=700;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(from,to,kind,steps=10)=>{
    touchId++;await pointer('mousePressed',from,kind);await pause(30);
    for(let i=1;i<=steps;i++){await pointer('mouseMoved',{x:from.x+(to.x-from.x)*i/steps,y:from.y+(to.y-from.y)*i/steps},kind);await settle();}
    await pointer('mouseReleased',to,kind);await settle();
  };
  const menuRow=(root,label)=>`[...document.querySelectorAll('${root} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const tapRow=async(root,label,kind)=>{
    await wait(`!!${menuRow(root,label)}&&!${menuRow(root,label)}.disabled`);
    await tap(await evaluate(`(r=>({x:r.x+r.width/2,y:r.y+r.height/2}))(${menuRow(root,label)}.getBoundingClientRect())`),kind);
  };
  const calm=()=>evaluate('new Promise(resolve=>{let last=performance.now(),steady=0;const frame=t=>{steady=t-last<40?steady+1:0;last=t;if(steady>=3)resolve();else requestAnimationFrame(frame);};requestAnimationFrame(frame);})');
  const choose=async(menu,path,kind)=>{
    await calm();
    await tap(await middle(`.header-menu[data-menu="${menu}"] > summary`),kind);
    await wait(`document.querySelector('.header-menu[data-menu="${menu}"]').open`);
    for(const label of path)await tapRow('.header-menu[open] .popover',label,kind);
    await wait(`!document.querySelector('.header-menu[data-menu="${menu}"]').open`);
  };
  const onBar=selector=>`(n=>!!n&&!n.closest('.canvas-action-bar-item,.canvas-action-bar-completion').hidden)(document.querySelector('${bar} ${selector}'))`;
  const viaMore=async(path,kind)=>{
    await tap(await middle(`${bar} .canvas-action-bar-more`),kind);
    for(const label of path)await tapRow('.panel-context-menu:popover-open',label,kind);
  };
  const pressBar=async(command,kind)=>{
    await wait(`!!document.querySelector('${bar} [data-command="${command}"]')&&${visible}`);
    if(await evaluate(onBar(`[data-command="${command}"]`)))return tap(await middle(`${bar} [data-command="${command}"]`),kind);
    await viaMore([await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').label`)],kind);
  };
  const pick=async(group,label,kind)=>{
    const choice=`[data-toolbar-choice="${group}"]`;
    await wait(`!!document.querySelector('${bar} ${choice}')&&${visible}`);
    if(!await evaluate(onBar(choice)))return viaMore([await evaluate(`document.querySelector('${bar} ${choice}').getAttribute('aria-label')`),label],kind);
    await tap(await middle(`${bar} ${choice} > button`),kind);
    await wait(`!!document.querySelector('.toolbar-choice-menu')`);
    const entries=await evaluate(`[...document.querySelectorAll('.toolbar-choice-menu > button')].map(b=>[b.textContent,b.getAttribute('role')])`);
    const index=entries.findIndex(([text])=>text===label);
    assert.ok(index>=0&&entries.every(([,role])=>role==='menuitemradio'),`${kind}: the ${group} dropdown lists ${label} as a choice: ${JSON.stringify(entries)}`);
    await tap(await middle(`.toolbar-choice-menu > button:nth-child(${index+1})`),kind);
    await wait(`!document.querySelector('.toolbar-choice-menu')`);
    assert.equal(await evaluate(`document.querySelector('${bar} ${choice} .toolbar-choice-label').textContent`),label,`${kind}: the ${group} dropdown shows ${label}`);
    return entries.map(([text])=>text);
  };
  const cropping=`layerApp.state().layer_tools.tool==='crop'&&layerApp.state().canvas_bar?.context.kind==='crop'&&${visible}`;
  const openCrop=async kind=>{
    if(await tool()==='pick_visible')await invoke('eyedropper');
    if(kind==='mouse')await key('c','KeyC',67);
    else await choose('edit',['Image','Crop'],kind);
    await wait(cropping);
  };
  const apply=async kind=>{
    await pressBar('apply_transform',kind);
    await wait(`layerApp.state().layer_tools.tool!=='crop'&&layerApp.state().canvas_bar?.context.kind!=='crop'`);
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
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
  const blue=rgba=>!!rgba&&rgba.slice(0,3).every((v,i)=>Math.abs(v-BLUE[i])<.04);
  const white=rgba=>!!rgba&&rgba.slice(0,3).every(v=>v>.9);
  const shot=async()=>{
    const data=(await call('Page.captureScreenshot',{format:'png'})).data;
    await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();const c=document.createElement('canvas');c.width=image.width;c.height=image.height;
      const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(image,0,0);window.cropShot={g,scale:image.width/innerWidth};})()`);
  };
  const shown=p=>evaluate(`Array.from(cropShot.g.getImageData(Math.round(${p.x}*cropShot.scale),Math.round(${p.y}*cropShot.scale),1,1).data)`);
  const capture=async name=>{
    const c=await camera(),b=await rect(bar);
    const x=Math.max(0,Math.min(c.r.x,b.x)-8),y=Math.max(0,c.r.y-8);
    const clip={x,y,width:Math.max(c.r.x+c.r.width,b.x+b.width)+8-x,height:Math.max(c.r.y+c.r.height,b.y+b.height)+8-y};
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{...clip,scale:1}})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  let overlay='is unchecked because screenshots here omit WebGPU pixels';
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');await invoke('zoom_out');
    const [width,height]=await size();
    await invoke('rectangle_select');
    await drag(await screen(width*.2,height*.2),await screen(width*.8,height*.8),'mouse',6);
    await wait('layerApp.state().layer_tools.has_selection');
    await send({type:'set_color',rgba:BLUE});await invoke('fill_selection');await invoke('deselect');
    await sample(await screen(width*.5,height*.5),blue,'The fill paints the middle of the canvas');
    const center=await screen(width*.5,height*.5);

    for(const kind of ['mouse','touch','pen']) {
      await openCrop(kind);
      if(await selected('crop_delete_cropped_pixels')) {
        await pressBar('crop_delete_cropped_pixels',kind);
        await wait(`!layerApp.state().commands.find(c=>c.id==='crop_delete_cropped_pixels').selected`);
      }
      assert.deepEqual([await setting('crop_width'),await setting('crop_height'),await setting('crop_angle')],[width,height,0],`${kind}: the frame starts as the whole canvas`);
      assert.equal(await evaluate(`document.querySelector('${bar} [data-command="crop_straighten"]').getAttribute('aria-pressed')`),'false',`${kind}: Straighten is a toggle`);
      const ratios=await pick('crop-ratio','1:1',kind);
      if(ratios)assert.deepEqual(ratios,['Free','Original','1:1','4:5','2:3','5:7','16:9'],`${kind}: the Ratio dropdown`);
      await wait(`layerApp.state().commands.find(c=>c.id==='crop_ratio_square').selected`);
      const side=Math.min(width,height);
      assert.deepEqual([await setting('crop_width'),await setting('crop_height')],[side,side],`${kind}: 1:1 fits the largest square`);
      const corner=await screen((width-side)/2,(height-side)/2);
      await drag(corner,{x:corner.x+120,y:corner.y+60},kind);
      await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value<${side-20}`);
      const cropped=await setting('crop_width');
      assert.ok(Math.abs(cropped-await setting('crop_height'))<.01,`${kind}: the ratio holds while the handle drags`);
      assert.equal(await evaluate(cropping),true,`${kind}: the crop bar returns after the drag`);
      if(kind==='mouse') {
        await key('o','KeyO',79);
        await wait(`layerApp.state().commands.find(c=>c.id==='crop_overlay_grid').selected`);
        assert.equal(await evaluate(`document.querySelector('${bar} [data-toolbar-choice="crop-overlay"] .toolbar-choice-label').textContent`),'Grid','O cycles the overlay');
        const overlays=await pick('crop-overlay','Diagonal',kind);
        if(overlays)assert.deepEqual(overlays,['Thirds','Grid','Diagonal','Golden Ratio'],'the Overlay dropdown');
        assert.equal(await selected('crop_overlay_diagonal'),true);
        await pick('crop-overlay','Thirds',kind);
        await pause(300);
        await shot();
        const paper=await shown(await screen((width+side)/2-80,height-80));
        const shield=await shown(await screen(width*.02,height*.1));
        const surround=await shown(await screen(-width*.05,height*.5));
        if(surround.slice(0,3).some((v,i)=>Math.abs(v-paper[i])>8)) {
          assert.ok(paper.slice(0,3).every(v=>v>200),`paper inside the crop stays bright: ${paper}`);
          assert.ok(shield.slice(0,3).every(v=>v>95&&v<155),`the shield keeps a fifth of the light outside the crop: ${shield}`);
          overlay='dims the canvas outside the frame';
        }
        for(const name of ['light','dark']){await send({type:'set_theme',theme:name});await pause(250);await capture(`crop-${name}`);}
        await send({type:'set_theme',theme});
        await evaluate(`document.querySelector('[data-tool-setting="crop_width"] .number-value')?.scrollIntoView({block:'nearest'})`);
        const field=`[data-tool-setting="crop_width"]`;
        if(await evaluate(`!!document.querySelector('${field} .number-value')&&document.querySelector('${field}').getBoundingClientRect().width>0`)) {
          await tap(await middle(`${field} .number-value`),kind);
          await wait(`document.activeElement===document.querySelector('${field} .number-entry')`);
          await evaluate(`document.querySelector('${field} .number-entry').select()`);
          const typed=Math.round(cropped)-100;
          await call('Input.insertText',{text:String(typed)});await key('Enter','Enter',13);
          await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value===${typed}`);
          assert.equal(await setting('crop_height'),typed,'Tool Options width keeps the 1:1 ratio');
        } else console.log('Tool Options is not shown in this workspace; its width is set through the session');
        await send({type:'set_tool_setting',id:'crop_width',value:cropped});
      }
      await apply(kind);
      const square=await size();
      assert.ok(square[0]===square[1]&&Math.abs(square[0]-cropped)<=1,`${kind}: Apply crops to the square frame: ${square} for ${cropped}`);
      await sample(center,blue,`${kind}: the fill stays in place after the crop`);
      await invoke('undo');
      assert.deepEqual(await size(),[width,height],`${kind}: one undo step restores the canvas`);

      await openCrop(kind);
      assert.equal(await selected('crop_ratio_square'),true,`${kind}: the crop keeps its ratio`);
      await pick('crop-ratio','Free',kind);
      await wait(`layerApp.state().commands.find(c=>c.id==='crop_ratio_free').selected`);
      await pressBar('reset_transform',kind);
      await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value===${width}`);
      await pressBar('crop_straighten',kind);
      await wait(`layerApp.state().commands.find(c=>c.id==='crop_straighten').selected`);
      if(await evaluate(onBar('[data-command="crop_straighten"]')))
        assert.equal(await evaluate(`document.querySelector('${bar} [data-command="crop_straighten"]').getAttribute('aria-pressed')`),'true',`${kind}: Straighten shows as pressed`);
      const angle=.1,from=[width*.3,height*.5],to=[from[0]+width*.4*Math.cos(angle),from[1]+width*.4*Math.sin(angle)];
      await drag(await screen(...from),await screen(...to),kind);
      await wait(`!layerApp.state().commands.find(c=>c.id==='crop_straighten').selected`);
      const turned=await setting('crop_angle');
      assert.ok(Math.abs(turned-angle)<.01,`${kind}: the frame turns to the line: ${turned}`);
      const [w,h]=[await setting('crop_width'),await setting('crop_height')];
      await apply(kind);

      const straight=await size();
      assert.ok(Math.abs(straight[0]-w)<=1&&Math.abs(straight[1]-h)<=1,`${kind}: Apply cuts the turned frame: ${straight} for ${[w,h]}`);
      assert.ok(straight[0]<width&&straight[1]<height,`${kind}: the level crop fits inside the old canvas`);
      const around=(x,y)=>screen(straight[0]/2+x,straight[1]/2+y),edge=-height*.3+20;
      await sample(await around(0,0),blue,`${kind}: the fill stays in the middle`);
      await sample(await around(-width*.22,edge),white,`${kind}: the image turns, lowering the fill's top-left corner`);
      await sample(await around(width*.22,edge),blue,`${kind}: the image turns, raising the fill's top-right corner`);
      await invoke('undo');
      assert.deepEqual(await size(),[width,height],`${kind}: one undo step restores the straightened drawing`);

      await openCrop(kind);
      await pressBar('crop_delete_cropped_pixels',kind);
      await wait(`layerApp.state().commands.find(c=>c.id==='crop_delete_cropped_pixels').selected`);
      await drag(await screen(0,0),await screen(width*.4,height*.4),kind);
      await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value<${width*.7}`);
      await apply(kind);
      const kept=await size(),origin=[width-kept[0],height-kept[1]];
      assert.ok(origin[0]>width*.3&&origin[1]>height*.3,`${kind}: the top-left handle cuts the canvas: ${kept}`);
      await invoke('canvas_size');
      for(const action of [{op:'anchor',anchor:'bottom_right'},{op:'width',value:width},{op:'height',value:height},{op:'apply'}])await send({type:'canvas_size',action});
      await wait(`layerApp.state().tabs[0].width===${width}&&!layerApp.state().layer_tools.canvas_size`);
      await sample(await screen(origin[0]*.75,height*.5),white,`${kind}: Canvas Size shows no deleted pixels`);
      await sample(await screen(width*.6,height*.6),blue,`${kind}: the pixels inside the crop stay`);
      await invoke('undo');await invoke('undo');
      assert.deepEqual(await size(),[width,height],`${kind}: two undo steps return to the uncropped drawing`);
      await sample(await screen(origin[0]*.75,height*.5),blue,`${kind}: undo brings the deleted pixels back`);
    }

    await openCrop('touch');
    const history=()=>evaluate(`(c=>[c.enabled,c.tooltip])(layerApp.state().commands.find(c=>c.id==='undo'))`);
    const [before,undo]=[await camera(),await history()];
    const inside=await screen(width*.5,height*.5);
    await drag(inside,{x:inside.x+90,y:inside.y+50},'touch');
    assert.deepEqual([await setting('crop_width'),await setting('crop_height'),await setting('crop_angle')],[width,height,0],'A finger inside the frame leaves the frame');
    assert.deepEqual((await camera()).t,before.t,'One finger inside the frame does not move the view');
    touchId++;
    const fingers=(type,dx)=>call('Input.dispatchTouchEvent',{type,touchPoints:type==='touchEnd'?[]:[{id:touchId*2,x:inside.x+dx,y:inside.y-60},{id:touchId*2+1,x:inside.x+dx,y:inside.y+60}]});
    await fingers('touchStart',0);
    for(let i=1;i<=8;i++){await fingers('touchMove',i*12);await settle();}
    await fingers('touchEnd',96);await settle();
    await wait(`(c=>Math.hypot(c.translation[0]-${before.t[0]},c.translation[1]-${before.t[1]})>20)(layerApp.app.camera())`);
    assert.deepEqual([await setting('crop_width'),await setting('crop_height')],[width,height],'Two fingers inside the frame pan the view, not the frame');
    const panned=await camera();
    await pressBar('crop_delete_cropped_pixels','touch');
    await wait(`!layerApp.state().commands.find(c=>c.id==='crop_delete_cropped_pixels').selected`);
    await pressBar('apply_transform','touch');
    await wait(`layerApp.state().layer_tools.tool!=='crop'`);
    assert.deepEqual([await size(),(await camera()).t,await history()],[[width,height],panned.t,undo],'The frame never moved, so Apply leaves the drawing and the history as they were');
    await openCrop('mouse');
    await key('Escape','Escape',27);
    await wait(`layerApp.state().layer_tools.tool!=='crop'`);
    assert.deepEqual(await size(),[width,height],'Escape cancels the crop');
    console.log(`PASS crop (${device?'tablet':'desktop'}): with mouse, touch and pen, Ratio ▾ 1:1, a handle drag and Apply crop in one undo step with the image in place; Straighten by a drawn line turns the image; Delete Cropped Pixels leaves nothing for Canvas Size to reveal; a finger on a handle drags it, one finger inside the frame leaves it and two pan the view; C, O, the Overlay dropdown and the Tool Options width work; the shield ${overlay}; captures in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Crop state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,status:document.querySelector('#status').textContent,size:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height],
      tool:layerApp.state().layer_tools.tool,bar:layerApp.state().canvas_bar?.context,settings:layerApp.state().tool_settings.map(s=>[s.id,s.value])},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    if(await tool()==='crop')await send({type:'invoke',command:'cancel_transform'});
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme});
  }
}
