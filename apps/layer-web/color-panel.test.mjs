import assert from 'node:assert/strict';
import {mkdir,writeFile,rename} from 'node:fs/promises';
import {existsSync} from 'node:fs';

// Production DOM, Rust color policy, and real Chrome input (including pen).
export async function checkColorPanel({call,evaluate,settle}) {
  const native=process.argv.includes('--native-input'),inputDir=process.env.LAYER_NATIVE_INPUT_DIR;
  let nativeStep=0;
  const performNative=async events=>{
    const step=nativeStep++,path=`${inputDir}/step-${step}.json`;
    await writeFile(`${path}.tmp`,JSON.stringify(events));await rename(`${path}.tmp`,path);
    const deadline=Date.now()+15000;
    while(!existsSync(`${inputDir}/done-${step}`)){assert.ok(Date.now()<deadline,'Compositor color input timed out');await new Promise(r=>setTimeout(r,2));}
  };
  const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/color-panel/web';
  await mkdir(output,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const read=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().colors,(_,v)=>typeof v==="bigint"?String(v):v))');
  const root='.dock-group .color-wheel-control';
  const inputReports=[];
  const bounds=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const point=async(selector,fx=.5,fy=.5)=>{const r=await bounds(selector);return{x:r.x+r.width*fx,y:r.y+r.height*fy};};
  const gesture=async(device,from,to=from,cancel=false)=>{
    // Chrome can synthesize a touch click after its pointer-up acknowledgement.
    // Wait for that event and verify its target instead of racing a fixed delay.
    const tap=!cancel&&from.x===to.x&&from.y===to.y&&await evaluate(`(()=>{const target=document.elementFromPoint(${from.x},${from.y})?.closest('button');if(!target)return false;window.colorPanelTap=new Promise(resolve=>{const events=[],record=e=>events.push({type:e.type,x:e.clientX,y:e.clientY,button:e.button,pointer:e.pointerType,slot:e.target.closest('[data-color-slot]')?.dataset.colorSlot,label:e.target.closest('button')?.getAttribute('aria-label')});for(const type of ['pointerdown','pointerup','click'])document.addEventListener(type,record,true);const done=e=>{clearTimeout(timer);document.removeEventListener('click',done,true);for(const type of ['pointerdown','pointerup','click'])document.removeEventListener(type,record,true);resolve({hit:!!e&&target.contains(e.target),events});};const timer=setTimeout(()=>done(null),2000);document.addEventListener('click',done,true);});return true;})()`);
    if(native&&device!=='pen'&&!cancel) {
      const events=device==='touch'?[{touch:'down',point:[from.x,from.y]},{touch:'move',point:[to.x,to.y]},{touch:'up'}]:[{point:[from.x,from.y]},{down:true},{point:[to.x,to.y]},{down:false}];
      await performNative(events);
    } else if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...from,id:1}]});
      // Avoid zero-duration contacts in Android Chrome's gesture recognizer.
      await new Promise(resolve=>setTimeout(resolve,40));
      if(from.x!==to.x||from.y!==to.y)await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...to,id:1}]});
      await new Promise(resolve=>setTimeout(resolve,40));
      await call('Input.dispatchTouchEvent',{type:cancel?'touchCancel':'touchEnd',touchPoints:[]});
    } else {
      for(const [type,p,buttons] of [['mousePressed',from,1],['mouseMoved',to,1],['mouseReleased',to,0]])
        await call('Input.dispatchMouseEvent',{type,...p,buttons,button:'left',clickCount:1,pointerType:device});
    }
    if(tap){const result=await evaluate('colorPanelTap.then(result=>{delete window.colorPanelTap;return result})');inputReports.push({device,from,...result});await writeFile(`${output}/input.json`,JSON.stringify(inputReports,null,2));assert.ok(result.hit,`${device}: tap reaches its button: ${JSON.stringify(result)}`);}
    await settle();
    await evaluate('new Promise(r=>setTimeout(r,100))');
  };
  const swatch=slot=>`${root} [data-color-slot="${slot}"]`;
  const hoverAt=async selector=>{
    const p=await point(selector);
    if(native)await performNative([{point:[p.x,p.y]}]);
    else await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'mouse'});
    await settle();
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).matches(':hover')&&!document.documentElement.hasAttribute('data-touch')`),'Mouse motion establishes the swatch hover');
  };
  const overlap=()=>evaluate(`(()=>{const a=document.querySelector(${JSON.stringify(swatch('foreground'))}).getBoundingClientRect(),b=document.querySelector(${JSON.stringify(swatch('background'))}).getBoundingClientRect(),x=a.x+a.width/2,y=a.y+a.height/2,dx=b.x+b.width/2-x,dy=b.y+b.height/2-y,d=Math.hypot(dx,dy),t=(d-b.width/2+a.width/2)/2;return{x:x+dx*t/d,y:y+dy*t/d};})()`);
  const front=async expected=>{
    const p=await overlap();
    assert.equal(await evaluate(`document.elementFromPoint(${p.x},${p.y})?.closest('[data-color-slot]')?.dataset.colorSlot`),expected,'The remembered paint swatch receives the overlap');
    assert.equal(await evaluate('layerApp.app.color_panel().front_swatch'),expected);
  };
  const rim=async(selector,name)=>{
    const r=await bounds(selector),viewport=await evaluate('({width:innerWidth,height:innerHeight})');
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:false});
    await writeFile(`${output}/${name.replace(/[^a-z0-9]+/gi,'-')}-displayed.png`,Buffer.from(shot.data,'base64'));
    const sample=await evaluate(`(async()=>{
      const image=new Image();image.src=${JSON.stringify('data:image/png;base64,'+shot.data)};await image.decode();
      const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const ctx=c.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);
      const n=document.querySelector(${JSON.stringify(selector)}),s=getComputedStyle(n),r=${JSON.stringify(r)},sx=c.width/${viewport.width},sy=c.height/${viewport.height},cx=r.x+r.width/2,cy=r.y+r.height/2,radius=r.width/2-1;
      const pixels=Array.from({length:16},(_,i)=>{
        const a=i*Math.PI/8,px=cx+radius*Math.cos(a),py=cy+radius*Math.sin(a);
        if(!n.contains(document.elementFromPoint(px,py)))return null;
        const candidates=Array.from({length:9},(_,j)=>{
          const x=Math.floor(px*sx)+j%3-1,y=Math.floor(py*sy)+Math.floor(j/3)-1,lx=(x+.5)/sx,ly=(y+.5)/sy;
          const minimum=Math.hypot(Math.max(x/sx-cx,0,cx-(x+1)/sx),Math.max(y/sy-cy,0,cy-(y+1)/sy));
          const maximum=Math.max(...[[0,0],[1,0],[0,1],[1,1]].map(([dx,dy])=>Math.hypot((x+dx)/sx-cx,(y+dy)/sy-cy)));
          return{x,y,withinRim:minimum>=radius-1+.02&&maximum<=radius+1-.02,score:4*Math.abs(Math.hypot(lx-cx,ly-cy)-radius)+Math.hypot(lx-px,ly-py)};
        })
          .filter(({x,y,withinRim})=>withinRim&&[[.01,.01],[.99,.01],[.01,.99],[.99,.99]].every(([dx,dy])=>n.contains(document.elementFromPoint((x+dx)/sx,(y+dy)/sy))))
          .sort((a,b)=>a.score-b.score);
        return candidates.length?[...ctx.getImageData(candidates[0].x,candidates[0].y,1,1).data]:null;
      }).filter(Boolean);
      ctx.fillStyle=s.color;ctx.fillRect(0,0,1,1);const text=[...ctx.getImageData(0,0,1,1).data];ctx.clearRect(0,0,1,1);ctx.fillStyle=s.backgroundColor;ctx.fillRect(0,0,1,1);
      return{pixels,text,backingAlpha:ctx.getImageData(0,0,1,1).data[3],hover:n.matches(':hover'),touch:document.documentElement.hasAttribute('data-touch'),shadow:getComputedStyle(n,'::after').boxShadow};
    })()`);
    if(selector.includes('data-color-slot')){
      assert.ok(sample.pixels.length>=6,`${name}: exposed rim samples`);
      assert.ok(sample.pixels.every(pixel=>pixel.slice(0,3).every((v,i)=>Math.abs(v-sample.text[i])<=40)),`${name}: the complete 2 px rim appears above the paint: ${JSON.stringify(sample)}`);
      assert.ok(sample.pixels.every(pixel=>pixel[3]===255),`${name}: opaque rim`);
    }
    assert.match(sample.shadow,/0px 0px 0px 2px inset$/,`${name}: selected or hovered rim`);
    assert.equal(sample.backingAlpha,255,`${name}: opaque backing`);
  };
  const capturePanel=async name=>{
    const r=await bounds(root),shot=await call('Page.captureScreenshot',{format:'png',clip:{x:r.x-8,y:r.y-44,width:r.width+16,height:r.height+52,scale:1}});
    await writeFile(`${output}/${name}.png`,Buffer.from(shot.data,'base64'));
  };
  await send({type:'invoke',command:'reset_layout'});
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){const v=JSON.parse(layerApp.app.workspace_view());if(v?.ready&&!v.busy&&!v.dirty)resolve();else if(performance.now()>end)reject(Error('Workspace reset timed out'));else setTimeout(poll,30);}poll();})`);
  if(await evaluate('layerApp.state().workspace.zen_mode'))await send({type:'invoke',command:'zen_mode'});
  await send({type:'move_panel',panel:'color',target:{kind:'float',position:[480,120]}});
  const resizePanel=async(width,height=width+36)=>{await evaluate(`(()=>{const workspace=layerApp.app.workspace_persistence(),f=workspace.layout.floating.find(f=>f.root.panels?.includes('color'));f.width=${width};f.height=${height};layerApp.dispatch({type:'restore_workspace',workspace});})()`);await settle();};
  const setShape=shape=>send({type:'color',action:{op:'shape',shape}});
  const reports=[];
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:2,mobile:false});
  await settle();
  for(const theme of ['dark','light'])for(const width of [144,160,200,280,360]) {
    await send({type:'set_theme',theme});await resizePanel(width);
    await send({type:'set_color',rgba:[.2,.72,.58,1]});
    for(const shape of ['circle','square','triangle']) {
      await setShape(shape);
      assert.equal((await read()).readout,'shape');
      const frames=await evaluate(`(()=>{
        const root=document.querySelector(${JSON.stringify(root)}),rect=n=>n.getBoundingClientRect().toJSON(),wheel=root.querySelector('.color-wheel'),g=layerApp.app.color_panel().geometry,w=rect(wheel);
        return {root:rect(root),stage:rect(root.querySelector('.color-wheel-square')),wheel:w,
          controls:[...root.querySelectorAll('.color-swatch,.color-utility,.color-shape')].map(n=>({slot:n.dataset.colorSlot,label:n.getAttribute('aria-label'),...rect(n),hit:n.contains(document.elementFromPoint(rect(n).x+rect(n).width/2,rect(n).y+rect(n).height/2))})),
          ringClear:Array.from({length:72},(_,i)=>{const a=i*5*Math.PI/180,r=(g.inner+g.outer)/2*w.width;return document.elementFromPoint(w.x+w.width/2+r*Math.cos(a),w.y+w.height/2+r*Math.sin(a))===wheel;}).every(Boolean),
          readoutInk:[...root.querySelector('.color-readout canvas').getContext('2d').getImageData(0,0,root.querySelector('.color-readout canvas').width,32).data].some((v,i)=>i%4===3&&v>0),pixels:[...wheel.getContext('2d').getImageData(Math.floor(wheel.width*.5),Math.floor(wheel.height*.5),1,1).data],inputs:root.querySelectorAll('input').length,shapeButtons:[...root.querySelectorAll('.color-shape')].map(n=>n.dataset.colorShape)};
      })()`);
      assert.ok(Math.abs(frames.root.height-frames.stage.height)<1,'All controls fit the wheel and footer');
      assert.ok(Math.abs(frames.wheel.width-frames.wheel.height)<1,'Square wheel');
      assert.ok(frames.readoutInk,'Curved readout is actually rendered');
      assert.equal(frames.pixels[3],255,`${shape}: field pixels are actually rendered`);
      assert.equal(frames.inputs,0,'Readouts are compact, read-only values');
      assert.ok(frames.ringClear,`${width}px ${shape}: unobscured hue ring`);
      assert.equal(frames.shapeButtons.length,2);assert.ok(!frames.shapeButtons.includes(shape));
      for(const c of frames.controls){assert.ok(c.hit,`Unobscured ${c.label}`);assert.ok(c.width>=20&&c.height>=20);assert.ok(c.x>=frames.root.x-1&&c.right<=frames.root.right+1&&c.y>=frames.root.y-1&&c.bottom<=frames.root.bottom+1,`${c.label} fits`);}
      const fg=frames.controls.find(c=>c.slot==='foreground'),bg=frames.controls.find(c=>c.slot==='background'),transparent=frames.controls.find(c=>c.slot==='transparent');
      assert.ok(fg.width>bg.width&&fg.x<bg.x&&fg.y<bg.y&&fg.right>bg.x&&fg.bottom>bg.y,'Overlapping foreground sits above and left of background');
      assert.equal(bg.width,transparent.width);
      for(const model of ['shape','rgb']) {
        while((await read()).readout!==model)await send({type:'color',action:{op:'toggle_readout'}});
        assert.equal(await evaluate('layerApp.app.color_panel().readout_label'),model==='rgb'?'RGB':{circle:'OKLCH',square:'HSB',triangle:'HLS'}[shape]);
        const name=`${theme}-${width}-${shape}-${model}`;reports.push({name,shape,model,...frames});
        const clip={x:frames.root.x-8,y:frames.root.y-44,width:frames.root.width+16,height:frames.root.height+52};
        for(const [suffix,scale] of [['',1],['-1x',.5]]) {
          const shot=await call('Page.captureScreenshot',{format:'png',clip:{...clip,scale}});
          await writeFile(`${output}/${name}${suffix}.png`,Buffer.from(shot.data,'base64'));
        }
      }
    }
  }
  await writeFile(`${output}/geometry.json`,JSON.stringify(reports,null,2));
  // Observe actual Canvas text transforms: blank cells, digits and unit suffixes
  // must keep the same font and arc position across 1/2/3-digit values in every shape's units.
  for(const [shape,model] of [['square','HSB'],['circle','OKLCH'],['triangle','HLS']])for(const width of [144,280,360]){
    await setShape(shape);
    await resizePanel(width);
    const placements=await evaluate(`(()=>{
      const canvas=document.querySelector(${JSON.stringify(root+' .color-readout canvas')}),prototype=CanvasRenderingContext2D.prototype,original=prototype.fillText;
      let glyphs=[];const frames=[];
      prototype.fillText=function(text,...args){
        if(this.canvas===canvas){if(text===${JSON.stringify(model)})glyphs=[];else{const m=this.getTransform();glyphs.push({cell:/^[0-9 ]$/.test(text)?'#':text,font:this.font,transform:[m.a,m.b,m.c,m.d,m.e,m.f]});}}
        return original.call(this,text,...args);
      };
      try{for(const value of [9,10,100]){layerApp.dispatch({type:'color',action:{op:'definition',color:{space:'Srgb',rgba:[value/100,value/100,value/100,1]}}});frames.push(glyphs);}}
      finally{prototype.fillText=original;}return frames;
    })()`);
    assert.equal(placements[0].length,model==='OKLCH'?13:12);
    assert.deepEqual(placements[1],placements[0],`${width}px: 9 to 10 retains ${model} glyph slots`);
    assert.deepEqual(placements[2],placements[0],`${width}px: 10 to 100 retains ${model} glyph slots`);
  }
  await call('Emulation.clearDeviceMetricsOverride');await settle();
  await resizePanel(280,230);
  assert.ok(await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(root)}),panel=root.closest('.panel').getBoundingClientRect(),stage=root.querySelector('.color-wheel-square').getBoundingClientRect();return stage.bottom<=panel.bottom&&stage.width>=128&&Math.abs(stage.height-layerApp.app.color_ui({type:'layout',size:stage.width,hdr:false}).height)<1;})()`),'Short dock retains the complete wheel and footer');
  await resizePanel(100,180);
  assert.equal(Math.round((await bounds(`${root}`)).width),128,'Four-tile width is enforced');
  await setShape('circle');
  // A smooth color field can use logical-pixel sampling while its clip, ring and
  // markers stay HiDPI. Compare every interior channel with a full 2x raster at
  // minimum, medium and large sizes, including both sides of Okhsv's blue cusp.
  const sampling=await evaluate(`(()=>{
    const a=layerApp.app,results=[];
    for(const low of [108,236,312])for(const h of [0,60,120,180,240,264.05,264.1,300]){
      const angle=(h+a.color_panel().wheel_hue_start_degrees)*Math.PI/180;
      layerApp.dispatch({type:'color',action:{op:'pick_wheel',part:'hue',size:1,point:[.5+.43*Math.cos(angle),.5+.43*Math.sin(angle)]}});
      const high=low*2,reference=a.color_field_pixels(high),bytes=a.color_field_pixels(low);
      const src=document.createElement('canvas'),dst=document.createElement('canvas');src.width=src.height=low;dst.width=dst.height=high;
      src.getContext('2d',{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer,bytes.byteOffset,bytes.byteLength),low,low),0,0);
      const ctx=dst.getContext('2d',{willReadFrequently:true});ctx.drawImage(src,0,0,high,high);
      const actual=ctx.getImageData(0,0,high,high).data,radius=a.color_panel().geometry.disc_radius*high-3;
      let maximum=0,channels=0;
      for(let y=0;y<high;y++)for(let x=0;x<high;x++){
        if((x+.5-high/2)**2+(y+.5-high/2)**2>=radius*radius)continue;
        const i=(y*high+x)*4;
        for(let j=0;j<3;j++){maximum=Math.max(maximum,Math.abs(actual[i+j]-reference[i+j]));channels++;}
      }
      results.push({low,h,maximum,channels});
    }return results;
  })()`);
  await writeFile(`${output}/raster-resolution.json`,JSON.stringify(sampling,null,2));
  for(const sample of sampling)assert.ok(sample.maximum<=2,`Disc interpolation: ${JSON.stringify(sample)}`);
  if(native){await writeFile(`${inputDir}/ready`,'ready');await performNative([]);}
  for(const device of ['mouse','touch','pen']) {
    console.log('Color panel input:',device);
    await gesture(device,await point(`${root} [data-color-slot="foreground"]`));
    await front('foreground');
    await send({type:'set_color',rgba:[.2,.72,.58,1]});
    const before=await read();
    await gesture(device,await point(`${root} .color-wheel`,.935,.5),await point(`${root} .color-wheel`,.5,.935));
    assert.notDeepEqual((await read()).foreground,before.foreground,`${device}: hue drag`);
    for(const shape of ['circle','square','triangle']) {
      await setShape(shape);await send({type:'set_color',rgba:[.2,.72,.58,1]});
      const color=(await read()).foreground;
      await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.58,.42));
      assert.notDeepEqual((await read()).foreground,color,`${device}: ${shape} field drag`);
    }
    const backgroundPoint=await point(`${root} [data-color-slot="background"]`);
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(root+' [data-color-slot="background"]')}).contains(document.elementFromPoint(${backgroundPoint.x},${backgroundPoint.y}))`),'Background center receives input');
    await gesture(device,backgroundPoint);assert.equal((await read()).slot,'background');await front('background');
    await gesture(device,await point(`${root} [data-color-slot="transparent"]`));assert.equal((await read()).slot,'transparent');
    await front('background');
    await gesture(device,await overlap());assert.equal((await read()).slot,'background',`${device}: overlap resumes its front paint`);
    await gesture(device,await point(`${root} [data-color-slot="transparent"]`));assert.equal((await read()).slot,'transparent');
    await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.55,.45));assert.equal((await read()).slot,'background','Picking resumes the remembered paint');
    const pair=await read();await gesture(device,await point(`${root} [data-color-slot="transparent"]`));
    for(const [name,rgba] of [['black',[0,0,0,1]],['white',[1,1,1,1]]]) {
      await gesture(device,await point(`${root} [data-quick-color="${name}"]`));
      const colors=await read();assert.equal(colors.slot,'temporary');assert.deepEqual(colors.temporary.rgba,rgba);
      assert.deepEqual([colors.foreground,colors.background],[pair.foreground,pair.background]);
      await front('foreground');
    }
    await gesture(device,await point(`${root} [data-color-slot="background"]`));
    for(const shape of ['circle','square','triangle','circle']) {
      if((await read()).readout!=='rgb')await send({type:'color',action:{op:'toggle_readout'}});
      await gesture(device,await point(`${root} [data-color-shape="${shape}"]`));assert.equal((await read()).shape,shape);
      assert.equal((await read()).readout,'shape','Changing shape restores its units from RGB');
      assert.equal(await evaluate('layerApp.app.color_panel().readout_label'),{circle:'OKLCH',square:'HSB',triangle:'HLS'}[shape]);
    }
    const swatches=await read();await gesture(device,await point(`${root} .color-swap`));
    assert.deepEqual((await read()).foreground,swatches.background);assert.deepEqual((await read()).background,swatches.foreground);
    await front('background');
    for(let i=0;i<2;i++) {
      const before=await read(),expected={shape:'rgb',rgb:'shape'}[before.readout];
      await gesture(device,await point(`${root} .color-readout`,.12,.07));
      assert.equal((await read()).readout,expected);assert.deepEqual((await read()).background,before.background);
    }
    for(const shape of ['circle','square'])for(const saturation of [20,80]) {
      await setShape(shape);
      const target=await evaluate(`(()=>{const g=layerApp.app.color_panel().geometry,s=${saturation}/100;if(${JSON.stringify(shape)}==='circle'){const a=2*s-1,r=g.disc_radius;return[g.center[0]+r*a/Math.SQRT2,g.center[1]+r*Math.sqrt(1-a*a/2)];}return[g.square[0]+s*g.square[2],g.square[1]+g.square[2]];})()`);
      await send({type:'set_color',rgba:[.2,.72,.58,1]});
      await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,...target));
      const values=await evaluate('layerApp.app.color_panel().wheel_components');
      assert.ok(Math.abs(values[1]-saturation)<2&&values[2]<2,`${device}: ${shape} retains black position: ${values}`);
      await gesture(device,await point(`${root} .color-wheel`,.935,.5),await point(`${root} .color-wheel`,.5,.935));
      assert.ok(Math.abs((await evaluate('layerApp.app.color_panel().wheel_components[1]'))-values[1])<.001,'Hue at black preserves saturation');
    }
  }
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});await resizePanel(280);
    await evaluate(`window.retainedColorSwatches=[...document.querySelectorAll(${JSON.stringify(root+' [data-color-slot]')})]`);
    for(const slot of ['background','foreground']) {
      await gesture('mouse',await point(swatch(slot)));await front(slot);
      await rim(swatch(slot),`${theme}: selected ${slot}`);
      await capturePanel(`${theme}-${slot}-front`);
      const rear=slot==='background'?'foreground':'background';
      await hoverAt(swatch(rear));
      await front(slot);await rim(swatch(rear),`${theme}: hovered ${rear}`);
      const backing=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(swatch(rear))}),s=getComputedStyle(n),c=document.createElement('canvas').getContext('2d');c.fillStyle=s.backgroundColor;const actual=c.fillStyle;c.fillStyle=s.getPropertyValue('--panel').trim();return[actual,c.fillStyle];})()`);
      assert.equal(backing[0],backing[1],`${theme}: hover retains the panel backing`);
    }
    for(const [slot,key,code,windowsVirtualKeyCode] of [['background','Enter','Enter',13],['foreground',' ','Space',32]]) {
      await evaluate(`document.querySelector(${JSON.stringify(swatch(slot))}).focus()`);
      if(native)await performNative([{key:key===' '?32:65293,down:true},{key:key===' '?32:65293,down:false}]);
      else for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode,...(key==='Enter'&&type==='keyDown'?{text:'\r'}:{})});
      await settle();assert.equal((await read()).slot,slot);await front(slot);
      await gesture('mouse',await point(swatch('transparent')));await front(slot);
    }
    for(const name of ['black','white']) {
      const selector=`${root} [data-quick-color="${name}"]`;
      await hoverAt(selector);await rim(selector,`${theme}: hovered ${name}`);
    }
    await gesture('mouse',await point(swatch('transparent')));await rim(swatch('transparent'),`${theme}: selected transparent`);
    await capturePanel(`${theme}-transparent`);
    assert.ok(await evaluate(`retainedColorSwatches.every(n=>n===document.querySelector(${JSON.stringify(root)}).querySelector('[data-color-slot="'+n.dataset.colorSlot+'"]'))`),'Selection and hover retain the swatch controls');
    await evaluate('delete window.retainedColorSwatches');
  }
  // The readout has neither a tooltip nor hover decoration; native key activation remains.
  const readout=`${root} .color-readout`,p=await point(readout,.12,.07);
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:20,y:80,buttons:0});await settle();
  const style=()=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(readout)}),s=getComputedStyle(n);return [n.title,s.backgroundColor,s.color,s.boxShadow];})()`);
  const idleStyle=await style();await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0});await settle();assert.deepEqual(await style(),idleStyle,'Readout has no hover action');assert.equal(idleStyle[0],'');
  await evaluate(`document.querySelector(${JSON.stringify(readout)}).focus()`);
  for(const [key,code,windowsVirtualKeyCode] of [[' ','Space',32],['Enter','Enter',13]]) {
    const before=await read();
    if(native)await performNative([{key:key===' '?32:65293,down:true},{key:key===' '?32:65293,down:false}]);
    else for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode,...(key==='Enter'&&type==='keyDown'?{text:'\r'}:{})});
    await settle();assert.equal((await read()).readout,{shape:'rgb',rgb:'shape'}[before.readout]);
  }
  await call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:1});
  await gesture('touch',await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.55,.45),true);
  const cancelled=await read();await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  const hover=await point(`${root} .color-wheel`,.9,.5);
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',...hover,buttons:0,pointerType:'pen'});await settle();assert.deepEqual(await read(),cancelled,'Cancellation releases color contact');
  await send({type:'customize',action:{type:'set_panel_visible',panel:'toolbar',visible:true}});
  await send({type:'move_panel',panel:'toolbar',target:{kind:'edge',edge:'bottom',outer:true}});
  await evaluate(`(()=>{const workspace=layerApp.app.workspace_persistence(),header=workspace.layout.header;if(!header.zones.flat().some(e=>e.item.control?.kind==='color'))header.zones[0].push({id:header.next_id++,item:{kind:'tool',control:{kind:'color'}}});layerApp.dispatch({type:'restore_workspace',workspace});})()`);await settle();
  const pairView=()=>evaluate('layerApp.app.paint_pair()');
  const checkPair=async(name,expected)=>{
    const pair=await pairView();assert.equal(pair.front_swatch,expected,name);
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:false});
    await writeFile(`${output}/${name}.png`,Buffer.from(shot.data,'base64'));
    const icons=await evaluate(`(async()=>{
      const image=new Image();image.src=${JSON.stringify('data:image/png;base64,'+shot.data)};await image.decode();
      const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);
      return [...document.querySelectorAll('svg[data-paint-pair]')].filter(svg=>{const r=svg.getBoundingClientRect();return r.width&&r.height;}).map(svg=>{
        const r=svg.getBoundingClientRect(),front=svg.lastElementChild.dataset.paintSlot,pattern=svg.querySelector('pattern[data-paint-slot="'+front+'"]');
        return {header:!!svg.closest('.header-tool'),front,size:pattern.getAttribute('width'),pixel:[...ctx.getImageData(Math.floor((r.x+r.width*9.25/16)*canvas.width/innerWidth),Math.floor((r.y+r.height*9.25/16)*canvas.height/innerHeight),1,1).data]};
      });
    })()`);
    assert.ok(icons.some(i=>i.header)&&icons.some(i=>!i.header),`${name}: actual toolbar and title-bar icons: ${JSON.stringify(icons)}`);
    const paint=pair.swatches.find(s=>s.slot===expected);
    for(const icon of icons){
      assert.equal(icon.front,expected,`${name}: selected circle drawn last`);
      assert.equal(Number(icon.size),2*pair.checker_cell,`${name}: canonical checker size`);
      assert.equal(icon.pixel[3],255,`${name}: opaque overlap`);
      assert.ok(paint.checker[0].slice(0,3).every((v,i)=>Math.abs(v*255-icon.pixel[i])<=8),`${name}: opaque light checker at the canonical overlap belongs to ${expected}: ${JSON.stringify({icon,paint})}`);
    }
    assert.ok(await evaluate(`!window.retainedPairIcons||retainedPairIcons.every(({svg,groups})=>svg.isConnected&&groups.every(g=>g.parentNode===svg))`),`${name}: retained SVG and circles`);
  };
  const editCompact=async(hex,slot)=>{
    await send({type:'customize',action:{type:'open_control',control:'brush_color'}});
    const definition=(await pairView()).definition;
    await gesture('mouse',await point('.tile-popover:popover-open .property-color'));
    const draft=await evaluate(`(()=>{const model=document.querySelector('.color-dialog select');model.value='srgb_hex';model.dispatchEvent(new Event('change'));return document.querySelector('.color-dialog [data-color-field="0"]').value;})()`);
    assert.equal(draft,(await evaluate(`layerApp.app.color_ui({type:'form',request:{color:${JSON.stringify(definition)},document_space:'Srgb',model:'srgb_hex'}})`)).draft.fields[0],'Compact editor opens the active definition');
    await evaluate(`(()=>{const field=document.querySelector('.color-dialog [data-color-field="0"]');field.value=${JSON.stringify(hex)};field.dispatchEvent(new Event('input'));})()`);
    const before=await read();await gesture('mouse',await point('.color-dialog .suggested-action'));
    assert.notDeepEqual((await read())[slot],before[slot],`Compact editing changes ${slot}`);
    for(const other of ['foreground','background','temporary'].filter(s=>s!==slot))assert.deepEqual((await read())[other],before[other],`Compact editing preserves ${other}`);
    await send({type:'customize',action:{type:'close_control'}});
  };
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const [slot,rgba] of [['foreground',[.9,.08,.18,.45]],['background',[.04,.25,.9,.7]]])await send({type:'color',action:{op:'set_slot',slot,color:{space:'Srgb',rgba}}});
    await evaluate(`window.retainedPairIcons=[...document.querySelectorAll('svg[data-paint-pair]')].map(svg=>({svg,groups:[...svg.querySelectorAll(':scope > g')]}))`);
    for(const device of ['mouse','touch','pen'])for(const selected of ['background','foreground']) {
      await gesture(device,await point(swatch(selected)));await checkPair(`${theme}-${device}-${selected}-pair`,selected);
      await gesture(device,await point(swatch('transparent')));await checkPair(`${theme}-${device}-${selected}-transparent-pair`,selected);
    }
    await gesture('mouse',await point(swatch('background')));await gesture('mouse',await point(swatch('transparent')));
    await editCompact('#336699','background');await checkPair(`${theme}-compact-background-pair`,'background');
    const remembered=await read();await gesture('mouse',await point(swatch('transparent')));await gesture('mouse',await point(`${root} [data-quick-color="white"]`));
    assert.equal((await read()).slot,'temporary');
    await editCompact('#cc9933','temporary');assert.deepEqual([(await read()).foreground,(await read()).background],[remembered.foreground,remembered.background]);
    await checkPair(`${theme}-temporary-pair`,'foreground');
    const artwork=await read();await send({type:'invoke',command:'quick_mask'});
    assert.ok(await evaluate('!!layerApp.state().layer_tools.mask_editing'));
    await send({type:'color',action:{op:'set_slot',slot:'background',color:{space:'Srgb',rgba:[.15,.8,.3,.35]}}});
    await checkPair(`${theme}-mask-background-pair`,'background');
    await send({type:'invoke',command:'quick_mask'});assert.deepEqual(await read(),artwork);await checkPair(`${theme}-artwork-pair`,'foreground');
    await send({type:'customize',action:{type:'header',action:{type:'edit',editing:true}}});
    const source=await evaluate(`(()=>{const n=document.querySelector('#header svg[data-paint-pair]').closest('.header-item'),r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    if(native)await performNative([{point:[source.x,source.y]},{down:true},{point:[source.x+50,source.y+45]}]);
    else {await call('Input.dispatchMouseEvent',{type:'mousePressed',...source,button:'left',buttons:1,clickCount:1});await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:source.x+50,y:source.y+45,button:'left',buttons:1});}
    await settle();assert.ok(await evaluate(`!!document.querySelector('.header-drag-preview svg[data-paint-pair]')`),'Header drag retains a live pair icon');
    assert.ok(await evaluate(`(()=>{const ids=[...document.querySelectorAll('svg[data-paint-pair] pattern[id]')].map(n=>n.id);return new Set(ids).size===ids.length;})()`),'Drag copies keep independent SVG paint references');
    await checkPair(`${theme}-header-drag-pair`,'foreground');
    if(native)await performNative([{down:false}]);else await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:source.x+50,y:source.y+45,button:'left',buttons:0,clickCount:1});
    await send({type:'customize',action:{type:'header',action:{type:'cancel'}}});
    await evaluate('delete window.retainedPairIcons');
  }
  console.log('Paint pair: native icons, compact editors, masks and drag copies passed in both themes; opening HDR fixture');
  await send({type:'invoke',command:'new_document'});
  console.log('Paint pair: HDR creation requested');
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if(document.querySelector('[data-document-field="depth"]'))resolve();else if(performance.now()>end)reject(Error('HDR creation dialog'));else setTimeout(poll,30);}poll();})`);
  console.log('Paint pair: HDR creation dialog ready');
  await evaluate(`(()=>{for(const name of ['width','height']){const n=document.querySelector('[data-document-field="'+name+'"]');n.value='256';n.dispatchEvent(new Event('input',{bubbles:true}));}const field=document.querySelector('[data-document-field="depth"]');field.value='F16';field.dispatchEvent(new Event('change'));})()`);
  console.log('Paint pair: HDR Create bounds',await bounds('[data-document-action="create"]'),await evaluate('({width:innerWidth,height:innerHeight})'));
  await evaluate(`document.querySelector('[data-document-action="create"]').scrollIntoView({block:'center'})`);await settle();
  await gesture('mouse',await point('[data-document-action="create"]'));
  assert.equal(await evaluate(`!!document.querySelector('.document-dialog[open] [data-document-action="create"]')`),false,'Native Create closes the drawing dialog');
  console.log('Paint pair: HDR creation accepted');
  const hdrDeadline=Date.now()+60000;let hdrState;
  do {
    hdrState=await evaluate(`({depth:layerApp.app.document_color().depth,ready:layerApp.app.brush_ready(),progress:document.querySelector('.file-progress')?.textContent,dialogs:[...document.querySelectorAll('dialog[open]')].map(n=>n.textContent),errors:[...document.querySelectorAll('#gpu-notice,.message,.toast')].map(n=>n.textContent)})`);
    await writeFile(`${output}/hdr-startup.json`,JSON.stringify(hdrState,null,2));
    assert.ok(Date.now()<hdrDeadline,`HDR document ready: ${JSON.stringify(hdrState)}`);
    if(hdrState.depth!=='F16'||!hdrState.ready)await new Promise(resolve=>setTimeout(resolve,100));
  }while(hdrState.depth!=='F16'||!hdrState.ready);
  assert.equal(await evaluate('layerApp.app.color_panel().hdr'),true);
  for(const theme of ['light','dark']) {
    console.log('Paint pair: HDR rendition theme',theme);
    await send({type:'set_theme',theme});
    await send({type:'color',action:{op:'set_slot',slot:'background',color:{space:'Srgb',rgba:[.7,.2,.05,1]}}});
    await send({type:'color',action:{op:'hdr_intensity',stops:2}});
    await evaluate(`window.retainedPairIcons=[...document.querySelectorAll('svg[data-paint-pair]')].map(svg=>({svg,groups:[...svg.querySelectorAll(':scope > g')]}))`);
    await send({type:'invoke',command:'sdr_rendition'});
    await evaluate(`document.querySelector('.proof-dial-accessibility [aria-label="Brightness"]').focus()`);
    const original=await pairView();
    if(native)await performNative(Array.from({length:10},()=>[{key:65364,down:true},{key:65364,down:false}]).flat());
    else for(let i=0;i<10;i++)for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
    await settle();const changed=await pairView();assert.deepEqual(changed.definition,original.definition,'Rendition edit preserves portable paint');assert.notDeepEqual(changed.swatches,original.swatches,'Rendition changes the icon preview');
    await checkPair(`${theme}-rendition-pair`,'background');
    await send({type:'invoke',command:'undo'});await evaluate('delete window.retainedPairIcons');
  }
  if(native)await writeFile(`${inputDir}/finished`,'done');
  console.log(`Color panel: ${reports.length} layouts and both-resolution captures; ${native?'native mouse/touch + CDP pen':'CDP mouse/touch/pen'}; selected swatch overlap and rims, shape/readout buttons, swap, black position, keyboard, and cancellation passed`);
}
