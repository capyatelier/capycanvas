import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const BLUE=[.1,.3,.8,1];
const FILL=[.2,.2,.6,.8];
const bar='.canvas-action-bar';
const dialog='#image-size-dialog';
const visible=`(b=>!!b&&!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`;

export async function checkImageCommands({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/image-commands/web-tablet':'artifacts/image-commands/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const enabled=command=>`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`;
  const invoke=async command=>{await wait(enabled(command));await send({type:'invoke',command});};
  const size=()=>evaluate('(t=>[t.width,t.height])(layerApp.state().tabs[0])');
  const sized=([width,height])=>`(t=>t.width===${width}&&t.height===${height})(layerApp.state().tabs[0])`;
  const near=(value,expected,tolerance,label)=>assert.ok(Math.abs(value-expected)<=tolerance,`${label}: ${value} vs ${expected}`);
  const view=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_tools.image_size))');
  const setting=id=>evaluate(`layerApp.state().tool_settings.find(s=>s.id===${JSON.stringify(id)})?.value??null`);
  const selected=command=>evaluate(`!!layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.selected`);
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:[...c.translation],v:[...c.viewport],r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const key=async(key,code,vk,modifiers=0)=>{
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk,modifiers,...(type==='keyDown'&&key.length===1&&!modifiers?{text:key,unmodifiedText:key}:{})});
    await settle();
  };
  const undo=async()=>{await wait(enabled('undo'));await key('z','KeyZ',90,2);};
  let touchId=900;
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
  const choose=async(path,kind)=>{
    await calm();
    await tap(await middle('.header-menu[data-menu="edit"] > summary'),kind);
    await wait(`document.querySelector('.header-menu[data-menu="edit"]').open`);
    for(const label of ['Image',...path])await tapRow('.header-menu[open] .popover',label,kind);
    await wait(`!document.querySelector('.header-menu[data-menu="edit"]').open`);
  };
  const onBar=selector=>`(n=>!!n&&!n.closest('.canvas-action-bar-item,.canvas-action-bar-completion').hidden)(document.querySelector('${bar} ${selector}'))`;
  const pressBar=async(command,kind)=>{
    await wait(`!!document.querySelector('${bar} [data-command="${command}"]')&&${visible}`);
    if(await evaluate(onBar(`[data-command="${command}"]`)))return tap(await middle(`${bar} [data-command="${command}"]`),kind);
    await tap(await middle(`${bar} .canvas-action-bar-more`),kind);
    await tapRow('.panel-context-menu:popover-open',await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').label`),kind);
    return 'more';
  };
  const cropping=`layerApp.state().layer_tools.tool==='crop'&&layerApp.state().canvas_bar?.context.kind==='crop'&&${visible}`;
  const openCrop=async kind=>{
    if(await evaluate("layerApp.state().layer_tools.tool==='pick_visible'"))await invoke('eyedropper');
    if(kind==='mouse')await key('c','KeyC',67);
    else await choose(['Crop'],kind);
    await wait(cropping);
    if(!await selected('crop_ratio_free'))await invoke('crop_ratio_free');
    if(await selected('crop_delete_cropped_pixels'))await invoke('crop_delete_cropped_pixels');
  };
  const applyCrop=async kind=>{
    await pressBar('apply_transform',kind);
    await wait(`layerApp.state().layer_tools.tool!=='crop'`);
  };
  const sample=async(x,y,test,label)=>{
    if(await evaluate("layerApp.state().layer_tools.tool!=='pick_visible'"))await invoke('eyedropper');
    const p=await screen(x,y);
    let last;
    for(const end=Date.now()+15000;Date.now()<end;) {
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+1,y:p.y,buttons:0,pointerType:'mouse'});
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'mouse'});await settle();
      for(const next=Date.now()+1000;Date.now()<next;await pause(100)) {
        last=await evaluate('(p=>p?Array.from(p.rgba):null)(layerApp.state().color_picker.preview)');
        if(test(last))return;
      }
    }
    assert.fail(`${label}: ${JSON.stringify(last)} at ${x}, ${y}`);
  };
  const blue=rgba=>!!rgba&&rgba.slice(0,3).every((v,i)=>Math.abs(v-BLUE[i])<.04);
  const white=rgba=>!!rgba&&rgba.slice(0,3).every(v=>v>.9);
  const paper=async shown=>{
    const id=await evaluate(`Number(layerApp.state().layers.find(l=>Number(l.id)===2).id)`);
    await send({type:'layer',action:{op:'visibility',id,value:shown}});
    await wait(`layerApp.state().layers.find(l=>Number(l.id)===2).visible===${shown}`);
  };
  const capture=async(name,selector)=>{
    const r=await rect(selector);
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{x:Math.max(0,r.x-8),y:Math.max(0,r.y-8),width:r.width+16,height:r.height+16,scale:1}})).data,'base64'));
  };
  const themes=async(name,selector)=>{
    for(const theme of ['light','dark']){await send({type:'set_theme',theme});await pause(200);await capture(`${name}-${theme}`,selector);}
    await send({type:'set_theme',theme:initialTheme});
  };
  const typeInto=async(field,text,kind)=>{
    const entry=`${dialog} [data-image-size="${field}"] .number-entry`;
    await tap(await middle(entry),kind);
    try{await wait(`document.activeElement===document.querySelector('${entry}')`,3000);}
    catch{assert.fail(`${kind}: ${field} takes focus for editing; focus is on ${await evaluate('document.activeElement?.outerHTML.slice(0,120)')}`);}
    await evaluate(`document.querySelector('${entry}').select()`);
    await call('Input.insertText',{text});await settle();
  };
  const pick=async(label,value)=>{
    await evaluate(`(s=>{s.value=${JSON.stringify(value)};s.dispatchEvent(new Event('change',{bubbles:true}));})(document.querySelector('${dialog} .size-dialog-select[aria-label="${label}"]'))`);
    await settle();
  };
  const initialTheme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  const timings=[];
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');await invoke('zoom_out');
    const original=await size(),[width,height]=original;
    assert.notEqual(width,height,'a non-square canvas');
    await invoke('rectangle_select');
    await drag(await screen(width*FILL[0],height*FILL[1]),await screen(width*FILL[2],height*FILL[3]),'mouse',6);
    await wait('layerApp.state().layer_tools.has_selection');
    await send({type:'set_color',rgba:BLUE});await invoke('fill_selection');await invoke('deselect');
    await sample(width*.5,height*.5,blue,'The fill paints left of the middle');
    const fill=[(FILL[2]-FILL[0])*width,(FILL[3]-FILL[1])*height];

    for(const kind of ['mouse','touch','pen']) {
      await choose(['Image Size…'],kind);
      await wait(`!!document.querySelector('${dialog}')?.open`);
      assert.equal(await evaluate(`document.querySelector('${dialog}').contains(document.activeElement)&&!document.activeElement.matches('input,select')`),true,`${kind}: the dialog opens without focusing a field`);
      let v=await view();
      assert.deepEqual([v.values,v.constrain,v.unit],[[width,height],true,'pixels'],`${kind}: the dialog starts at the current size with Constrain proportions`);
      assert.equal(await evaluate(`document.querySelector('${dialog} footer .suggested-action').disabled`),true,`${kind}: Apply is disabled at the current size`);
      const half=[width/2,height/2];
      if(kind==='touch') {
        await tap(await middle(`${dialog} .size-dialog-check input`),kind);
        await wait('!layerApp.state().layer_tools.image_size.constrain');
        await typeInto('width',String(half[0]),kind);
        await wait(`layerApp.state().layer_tools.image_size.values[0]===${half[0]}`);
        assert.equal((await view()).values[1],height,`${kind}: without Constrain the height stays`);
        await tap(await middle(`${dialog} .size-dialog-check input`),kind);
        await wait('layerApp.state().layer_tools.image_size.constrain');
        assert.equal(await evaluate(`document.activeElement===document.querySelector('${dialog} [data-image-size="width"] .number-entry')`),false,`${kind}: the checkbox commits and leaves the field`);
      } else {
        await pick('Unit','percent');
        await wait(`layerApp.state().layer_tools.image_size.unit==='percent'`);
        await typeInto(kind==='pen'?'height':'width','50',kind);
        if(kind==='mouse') {
          await key('Enter','Enter',13);
          await wait(`!document.activeElement.matches('${dialog} input')`);
        }
        if(kind==='pen') {
          await pick('Resample','bicubic');
          await wait(`layerApp.state().layer_tools.image_size.resample==='bicubic'`);
        }
      }
      v=await view();
      assert.deepEqual(v.values,kind==='touch'?half:[50,50],`${kind}: Constrain proportions makes the other side follow`);
      assert.equal(v.message,`New size: ${half[0]} × ${half[1]} px`);
      assert.equal(v.can_apply,true);
      if(kind==='mouse')await themes('image-size',dialog);
      await tap(await middle(`${dialog} footer .suggested-action`),kind);
      await wait(`!document.querySelector('${dialog}')&&!layerApp.state().layer_tools.image_size`);
      await wait(sized(half));
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      await invoke('fit_canvas');
      await sample(half[0]/2,half[1]/2,blue,`${kind}: the fill scales with the image`);
      await sample(width*.4,half[1]/2,white,`${kind}: the paper right of the scaled fill`);
      await undo();
      await wait(sized(original));
      await sample(width*.5,height*.5,blue,`${kind}: Ctrl+Z undoes Image Size in one step`);
      console.log(`PASS ${kind}: Image Size to 50% with Constrain proportions, then Undo`);

      await choose(['Rotate Image 90° Right'],kind);
      await wait(sized([height,width]));
      await invoke('fit_canvas');
      await sample(height*.5,width*.3,blue,`${kind}: the fill left of the middle turns to above it`);
      await sample(height*.5,width*.75,white,`${kind}: below the middle stays paper`);
      await undo();
      await wait(sized(original));
      console.log(`PASS ${kind}: Rotate Image 90° Right turns the non-square canvas`);

      await invoke('fit_canvas');await invoke('zoom_out');
      await openCrop(kind);
      await drag(await screen(0,0),await screen(width*.4,height*.4),kind);
      await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value<${width*.7}`);
      await applyCrop(kind);
      const cropped=await size();
      assert.ok(cropped[0]<width*.7&&cropped[1]<height*.7,`${kind}: the crop hides the fill's left part: ${cropped}`);
      await choose(['Reveal All'],kind);
      await wait(`layerApp.state().tabs[0].width>${cropped[0]}`);
      const revealed=await size();
      near(revealed[0],width*(1-FILL[0]),2,`${kind}: Reveal All reaches the fill's hidden left edge`);
      near(revealed[1],height*(1-FILL[1]),2,`${kind}: Reveal All reaches the fill's hidden top edge`);
      await invoke('fit_canvas');
      await sample(4,4,blue,`${kind}: the hidden fill shows again at the new top left`);
      await undo();
      await wait(sized(cropped));
      await undo();
      await wait(sized(original));
      console.log(`PASS ${kind}: a crop, then Reveal All brings the hidden pixels back`);

      await choose(['Trim'],kind);
      await wait(`layerApp.state().notice?.text==='The visible pixels already reach every edge of the canvas'`);
      assert.deepEqual(await size(),original,`${kind}: with the paper showing, Trim changes nothing`);
      await paper(false);
      await choose(['Trim'],kind);
      await wait(`layerApp.state().tabs[0].width<${width}`);
      const trimmed=await size();
      near(trimmed[0],fill[0],2,`${kind}: Trim fits the width to the fill`);
      near(trimmed[1],fill[1],2,`${kind}: Trim fits the height to the fill`);
      await invoke('fit_canvas');
      for(const [x,y] of [[2,2],[trimmed[0]-3,trimmed[1]-3]])await sample(x,y,blue,`${kind}: the fill reaches the trimmed edge at ${x}, ${y}`);
      await undo();
      await wait(sized(original));
      console.log(`PASS ${kind}: Trim shrinks the canvas to the visible pixels`);

      await invoke('fit_canvas');await invoke('zoom_out');
      await openCrop(kind);
      assert.equal(await evaluate(`document.querySelector('${bar} [data-command="crop_fit_content"]')?.textContent.trim()`),'Fit Content',`${kind}: the crop bar offers Fit Content`);
      if(await pressBar('crop_fit_content',kind)==='more')console.log(`${kind}: Fit Content is in More at this window width`);
      await wait(`layerApp.state().tool_settings.find(s=>s.id==='crop_width').value<${width*.7}`);
      near(await setting('crop_width'),fill[0],3,`${kind}: Fit Content frames the fill's width`);
      near(await setting('crop_height'),fill[1],3,`${kind}: Fit Content frames the fill's height`);
      if(kind==='mouse')await themes('crop-fit-content',bar);
      await applyCrop(kind);
      const fitted=await size();
      near(fitted[0],fill[0],3,`${kind}: Apply crops to the fill's width`);
      near(fitted[1],fill[1],3,`${kind}: Apply crops to the fill's height`);
      await undo();
      await wait(sized(original));
      await paper(true);
      console.log(`PASS ${kind}: Fit Content on the crop bar frames the visible pixels`);
    }
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    const layer=await evaluate('Number(layerApp.state().layers.find(l=>l.editing).id)');
    await invoke('select_all');await invoke('fill_selection');await invoke('deselect');
    const scan=await evaluate(`new Promise(resolve=>{const frame=layerApp.app.frame;let frames=0;layerApp.app.frame=(...args)=>{frames++;return frame.apply(layerApp.app,args);};
      const before=layerApp.state().notice?.id,done=()=>(n=>!!n&&n.id!==before&&n.text==='The visible pixels already reach every edge of the canvas')(layerApp.state().notice);
      layerApp.dispatch({type:'invoke',command:'trim'});const synchronous=done();
      (function check(){if(done()||frames>2000){layerApp.app.frame=frame;resolve({frames,synchronous});}else requestAnimationFrame(check);})();})`);
    assert.equal(scan.synchronous,false,'A full layer has more edge tiles than the first frame decodes');
    assert.ok(scan.frames>=2&&scan.frames<2000,`Frames keep coming until the bounds scan finishes: ${scan.frames}`);
    await send({type:'layer',action:{op:'delete',id:layer}});
    await wait(`!layerApp.state().layers.some(l=>Number(l.id)===${layer})`);
    console.log(`PASS image commands (${device?'tablet':'desktop'}): with mouse, touch and pen, Image Size to 50% with Constrain then Ctrl+Z, Rotate Image 90° Right on a non-square canvas, a crop then Reveal All, Trim and Fit Content on the crop bar each change the canvas in one undo step; a bounds scan of a full layer finished over ${scan.frames} frames; captures in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Image commands state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector('#status').textContent,size:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height],
      tool:layerApp.state().layer_tools.tool,view:layerApp.state().layer_tools.image_size,bar:layerApp.state().canvas_bar?.context},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    if(await evaluate('!!layerApp.state().layer_tools.image_size'))await send({type:'image_size',action:{op:'cancel'}});
    if(await evaluate("layerApp.state().layer_tools.tool==='crop'"))await send({type:'invoke',command:'cancel_transform'});
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme:initialTheme});
  }
}
