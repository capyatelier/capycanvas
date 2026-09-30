import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const panel='#frequency-separation-panel';

export async function checkRetouchLayers({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/retouch-layers/web-tablet':'artifacts/retouch-layers/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const selected=command=>`!!layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.selected`;
  const layers=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layers,(_,v)=>typeof v==="bigint"?Number(v):v))');
  const labels=async()=>(await layers()).map(l=>l.label);
  const editing=async()=>(await layers()).find(l=>l.editing);
  const rect=expression=>evaluate(`(()=>{const r=(${expression}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async expression=>{const r=await rect(expression);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=2100;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(points,kind)=>{
    touchId++;await pointer('mousePressed',points[0],kind);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,kind);await settle();}
    await pointer('mouseReleased',points.at(-1),kind);await settle();
  };
  const stroke=async points=>{
    await wait('layerApp.app.brush_ready()');
    const revision=`layerApp.state().layers.find(l=>l.editing).paint_revision`;
    const before=await evaluate(`String(${revision})`);
    await drag(points,'pen');
    await wait(`String(${revision})!==${JSON.stringify(before)}`);
  };
  const menuRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const calm=()=>evaluate('new Promise(resolve=>{let last=performance.now(),steady=0;const frame=t=>{steady=t-last<40?steady+1:0;last=t;if(steady>=3)resolve();else requestAnimationFrame(frame);};requestAnimationFrame(frame);})');
  const choose=async(menu,path,kind)=>{
    await calm();
    const labelled=`document.querySelector('.header-menu[data-menu="${menu}"]')`;
    const folded=await evaluate(`!${labelled}?.getClientRects().length`);
    const header=folded?`document.querySelector('.header-menu-labels-compact')`:labelled;
    await tap(await middle(`${header}.querySelector('summary')`),kind);
    await wait(`${header}.open`);
    for(const row of folded?[menu[0].toUpperCase()+menu.slice(1),...path]:path) {
      await wait(`!!${menuRow(row)}&&!${menuRow(row)}.disabled`);
      await tap(await middle(menuRow(row)),kind);
    }
    await wait(`!${header}.open`);
  };
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:[...c.translation],v:[...c.viewport],r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async([x,y])=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  // Headless desktop screenshots omit WebGPU pixels, so the eyedropper reads
  // the canvas there; on a tablet the eyedropper follows no hover, so the
  // presented pixels are read instead.
  const read=async p=>{
    const at=await screen(p);
    if(device) {
      const {data}=await call('Page.captureScreenshot',{format:'png',clip:{x:at.x-2,y:at.y-2,width:5,height:5,scale:1}});
      return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();const c=new OffscreenCanvas(image.width,image.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);const rgba=x.getImageData(0,0,c.width,c.height).data,sum=[0,0,0];for(let i=0;i<rgba.length;i+=4)for(let j=0;j<3;j++)sum[j]+=rgba[i+j]/255;return sum.map(v=>v*4/rgba.length);})()`);
    }
    if(await evaluate("layerApp.state().layer_tools.tool==='pick_visible'"))await invoke('hand');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:at.x+1,y:at.y,buttons:0,pointerType:'mouse'});
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...at,buttons:0,pointerType:'mouse'});await settle();
    await invoke('eyedropper');
    for(const end=Date.now()+10000;Date.now()<end;await pause(100)){
      const rgba=await evaluate('(p=>p?Array.from(p.rgba).slice(0,3):null)(layerApp.state().color_picker.preview)');
      if(rgba)return rgba;
    }
    assert.fail('the eyedropper samples the canvas');
  };
  const reads=async points=>{const values=[];for(const p of points)values.push(await read(p));return values;};
  const until=async(points,test,label)=>{
    let last;
    for(const end=Date.now()+10000;Date.now()<end;await pause(150)){last=await reads(points);if(test(last))return last;}
    assert.fail(`${label}: ${JSON.stringify(last)}`);
  };
  const luma=rgba=>rgba[0]+rgba[1]+rgba[2];
  const tolerance=device?.02:.005;
  const same=(a,b,within=tolerance)=>a.every((values,i)=>values.every((v,c)=>Math.abs(v-b[i][c])<=within));
  const capture=async(file,expression)=>{
    const r=await rect(expression);
    const clip={x:Math.max(0,r.x-8),y:Math.max(0,r.y-8),width:r.width+16,height:r.height+16,scale:1};
    await writeFile(`${directory}/${file}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  try {
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    if(!await evaluate(selected('blend_perceptual')))await invoke('blend_perceptual');
    await invoke('fit_canvas');
    const [width,height]=await evaluate('(t=>[t.width,t.height])(layerApp.state().tabs.find(t=>t.active)??layerApp.state().tabs[0])');
    const at=(x,y)=>[width*x,height*y];
    const line=async(a,b)=>{const points=[];for(let i=0;i<=8;i++)points.push(await screen([a[0]+(b[0]-a[0])*i/8,a[1]+(b[1]-a[1])*i/8]));return points;};
    await invoke('pen');await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:height*.3});
    await send({type:'set_color',rgba:[.2,.35,.6,1]});
    await stroke(await line(at(.15,.5),at(.85,.5)));
    await send({type:'set_brush_size',value:height*.02});
    await send({type:'set_color',rgba:[.95,.8,.2,1]});
    await stroke(await line(at(.6,.45),at(.6,.55)));
    const photo=(await editing()).id;
    const count=(await layers()).length;
    const points=[at(.3,.5),at(.6,.5),at(.75,.52)];

    for(const kind of ['mouse','touch','pen']) {
      const original=await reads(points);
      await choose('layer',['New','New Dodge & Burn Layer'],kind);
      await wait(`layerApp.state().layers.length===${count+1}&&layerApp.state().layers.find(l=>l.editing)?.label==='Dodge & Burn'`);
      await until(points,values=>same(values,original),`${kind}: the gray layer leaves the canvas as it was`);
      await invoke('airbrush');await send({type:'set_brush_size',value:height*.06});await send({type:'set_brush_opacity',value:.3});
      await send({type:'set_color',rgba:[1,1,1,1]});
      await stroke(await line(at(.2,.45),at(.4,.45)));
      await send({type:'set_color',rgba:[0,0,0,1]});
      await stroke(await line(at(.2,.55),at(.4,.55)));
      await until([at(.3,.45),at(.3,.55)],([dodged,burned])=>luma(dodged)>luma(original[0])+.015&&luma(burned)<luma(original[0])-.015,`${kind}: white dodges and black burns`);
      for(let step=0;step<3;step++)await invoke('undo');
      await wait(`layerApp.state().layers.length===${count}`);

      await send({type:'layer',action:{op:'select',id:photo,mask:false}});
      await choose('filter',['Frequency Separation…'],kind);
      await wait(`!!document.querySelector('${panel} #frequency-separation-value')&&layerApp.state().layer_tools.frequency_separation?.radius===4`);
      assert.equal(await evaluate(`document.querySelector('${panel} h2').textContent`),'Frequency Separation');
      const slider=`document.querySelector('${panel} .number-slider')`;
      const track=await rect(slider);
      const along=f=>({x:track.x+8+(track.width-16)*f,y:track.y+track.height/2});
      await drag([along(.19),along(.3),along(.45)],kind);
      await wait('layerApp.state().layer_tools.frequency_separation?.radius>6');
      assert.equal((await layers()).length,count,`${kind}: the preview adds no layer`);
      await until([points[1]],([blurred])=>Math.abs(luma(blurred)-luma(original[1]))>.05,`${kind}: the canvas previews the blur`);
      if(kind==='mouse') {
        for(const name of ['light','dark']) {
          await send({type:'set_theme',theme:name});await pause(150);
          await capture(`frequency-separation-${name}`,`document.querySelector('${panel}')`);
        }
        await send({type:'set_theme',theme});
        await tap(await middle(`[...document.querySelectorAll('${panel} button')].find(b=>b.textContent==='Cancel')`),kind);
        await wait(`!document.querySelector('${panel}')&&!layerApp.state().layer_tools.frequency_separation`);
        await until(points,values=>same(values,original),'Cancel leaves the canvas as it was');
        assert.equal((await layers()).length,count,'Cancel leaves nothing');
        await send({type:'layer',action:{op:'select',id:photo,mask:false}});
        await choose('filter',['Frequency Separation…'],kind);
        await wait(`!!document.querySelector('${panel}')`);
      }
      await tap(await middle(`document.querySelector('${panel} .suggested-action')`),kind);
      await wait(`!document.querySelector('${panel}')&&layerApp.state().layers.find(l=>l.editing)?.label==='High'`);
      const split=await labels();
      assert.deepEqual(split.slice(0,4),['Frequency Separation','High','Low',split[3]],`${kind}: ${split}`);
      assert.equal((await layers())[3].visible,false,`${kind}: the photo stays below, hidden`);
      await until(points,values=>same(values,original,tolerance+.008),`${kind}: Low and High recombine`);
      await invoke('pen');await send({type:'set_brush_size',value:height*.02});await send({type:'set_brush_opacity',value:1});
      await send({type:'set_color',rgba:[.9,.1,.1,1]});
      await stroke(await line(at(.72,.52),at(.78,.52)));
      await until([points[2]],([marked])=>marked[0]>original[2][0]+.05,`${kind}: a small brush paints on High`);
      await invoke('undo');
      await until([points[2]],values=>same(values,[original[2]],tolerance+.008),`${kind}: one undo removes the stroke`);
      await invoke('undo');
      await wait(`layerApp.state().layers.length===${count}&&layerApp.state().layers.every(l=>l.visible)`);
      await send({type:'layer',action:{op:'select',id:photo,mask:false}});
    }
    console.log(`PASS retouch layers (${device?'tablet':'desktop'}): New Dodge & Burn Layer from Layer › New keeps the canvas, dodges and burns; Frequency Separation previews its blur, Cancel leaves nothing, Apply recombines and takes a brush on High, each one undo step, with mouse, touch and pen; dialogs in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Retouch layer state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,layers:layerApp.state().layers.map(l=>[l.id,l.label,l.editing,l.visible])},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await send({type:'set_theme',theme});
  }
}
