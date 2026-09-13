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
  const read=()=>evaluate('layerApp.state().colors');
  const root='.dock-group .color-wheel-control';
  const bounds=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const point=async(selector,fx=.5,fy=.5)=>{const r=await bounds(selector);return{x:r.x+r.width*fx,y:r.y+r.height*fy};};
  const gesture=async(device,from,to=from,cancel=false)=>{
    if(native&&device!=='pen'&&!cancel) {
      const events=device==='touch'?[{touch:'down',point:[from.x,from.y]},{touch:'move',point:[to.x,to.y]},{touch:'up'}]:[{point:[from.x,from.y],down:true},{point:[to.x,to.y]},{down:false}];
      await performNative(events);
    } else if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...from,id:1}]});
      if(from.x!==to.x||from.y!==to.y)await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...to,id:1}]});
      await call('Input.dispatchTouchEvent',{type:cancel?'touchCancel':'touchEnd',touchPoints:[]});
    } else {
      for(const [type,p,buttons] of [['mousePressed',from,1],['mouseMoved',to,1],['mouseReleased',to,0]])
        await call('Input.dispatchMouseEvent',{type,...p,buttons,button:'left',clickCount:1,pointerType:device});
    }
    await settle();
    if(native)await evaluate('new Promise(r=>setTimeout(r,100))');
  };
  await send({type:'invoke',command:'reset_layout'});
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
      const frames=await evaluate(`(()=>{
        const root=document.querySelector(${JSON.stringify(root)}),rect=n=>n.getBoundingClientRect().toJSON(),wheel=root.querySelector('.color-wheel'),g=layerApp.app.color_panel().geometry,w=rect(wheel);
        return {root:rect(root),stage:rect(root.querySelector('.color-wheel-square')),wheel:w,
          controls:[...root.querySelectorAll('.color-swatch,.color-utility,.color-shape')].map(n=>({slot:n.dataset.colorSlot,label:n.getAttribute('aria-label'),...rect(n),hit:n.contains(document.elementFromPoint(rect(n).x+rect(n).width/2,rect(n).y+rect(n).height/2))})),
          ringClear:Array.from({length:72},(_,i)=>{const a=i*5*Math.PI/180,r=(g.inner+g.outer)/2*w.width;return document.elementFromPoint(w.x+w.width/2+r*Math.cos(a),w.y+w.height/2+r*Math.sin(a))===wheel;}).every(Boolean),
          readoutInk:[...root.querySelector('.color-readout canvas').getContext('2d').getImageData(0,0,root.querySelector('.color-readout canvas').width,32).data].some((v,i)=>i%4===3&&v>0),pixels:[...wheel.getContext('2d').getImageData(Math.floor(wheel.width*.5),Math.floor(wheel.height*.5),1,1).data],inputs:root.querySelectorAll('input').length,shapeButtons:[...root.querySelectorAll('.color-shape')].map(n=>n.dataset.colorShape)};
      })()`);
      assert.ok(Math.abs(frames.root.height-frames.stage.width)<1,'All controls fit one square');
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
      for(const model of ['hsb','lab','rgb']) {
        while((await read()).readout!==model)await send({type:'color',action:{op:'toggle_readout'}});
        assert.equal(await evaluate('layerApp.app.color_panel().readout_label'),model==='hsb'?(shape==='triangle'?'HLS':'HSB'):model==='lab'?'Lab':'RGB');
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
  await call('Emulation.clearDeviceMetricsOverride');await settle();
  await resizePanel(280,230);
  assert.ok(await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(root)}),panel=root.closest('.panel').getBoundingClientRect(),stage=root.querySelector('.color-wheel-square').getBoundingClientRect();return stage.bottom<=panel.bottom&&stage.width>=128&&Math.abs(stage.width-stage.height)<1;})()`),'Short dock retains the entire square');
  await resizePanel(100,180);
  assert.equal(Math.round((await bounds(`${root}`)).width),128,'Four-tile width is enforced');
  if(native)await writeFile(`${inputDir}/ready`,'ready');
  for(const device of native?['mouse','touch','pen']:['mouse','pen']) {
    console.log('Color panel input:',device);
    await gesture(device,await point(`${root} [data-color-slot="foreground"]`));
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
    await gesture(device,await point(`${root} [data-color-slot="background"]`));assert.equal((await read()).slot,'background');
    await gesture(device,await point(`${root} [data-color-slot="transparent"]`));assert.equal((await read()).slot,'transparent');
    await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.55,.45));assert.equal((await read()).slot,'background','Picking resumes the remembered paint');
    for(const shape of ['circle','square','triangle','circle']) {
      await gesture(device,await point(`${root} [data-color-shape="${shape}"]`));assert.equal((await read()).shape,shape);
    }
    const swatches=await read();await gesture(device,await point(`${root} .color-swap`));
    assert.deepEqual((await read()).foreground,swatches.background);assert.deepEqual((await read()).background,swatches.foreground);
    for(let i=0;i<3;i++) {
      const before=await read(),expected={hsb:'lab',lab:'rgb',rgb:'hsb'}[before.readout];
      await gesture(device,await point(`${root} .color-readout`,.12,.07));
      assert.equal((await read()).readout,expected);assert.deepEqual((await read()).background,before.background);
    }
    for(const shape of ['circle','square'])for(const saturation of [20,80]) {
      await setShape(shape);
      for(const [index,value] of [[1,saturation],[2,0]])await send({type:'color',action:{op:'component',index,value}});
      const target=await evaluate('layerApp.app.color_panel().wheel_marker');
      await send({type:'set_color',rgba:[.2,.72,.58,1]});
      await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,...target));
      const values=await evaluate('layerApp.app.color_panel().components.map(c=>c.value)');
      assert.ok(Math.abs(values[1]-saturation)<2&&values[2]<2,`${device}: ${shape} retains black position: ${values}`);
      await gesture(device,await point(`${root} .color-wheel`,.935,.5),await point(`${root} .color-wheel`,.5,.935));
      assert.ok(Math.abs((await evaluate('layerApp.app.color_panel().components[1].value'))-values[1])<.001,'Hue at black preserves saturation');
    }
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
    await settle();assert.equal((await read()).readout,{hsb:'lab',lab:'rgb',rgb:'hsb'}[before.readout]);
  }
  await call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:1});
  await gesture('touch',await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.55,.45),true);
  const cancelled=await read();await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  const hover=await point(`${root} .color-wheel`,.9,.5);
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',...hover,buttons:0,pointerType:'pen'});await settle();assert.deepEqual(await read(),cancelled,'Cancellation releases color contact');
  if(native)await writeFile(`${inputDir}/finished`,'done');
  console.log(`Color panel: ${reports.length} layouts and both-resolution captures; ${native?'native mouse/touch + CDP pen':'CDP mouse/pen'}; shape/readout buttons, swap, black position, keyboard, and cancellation passed`);
}
