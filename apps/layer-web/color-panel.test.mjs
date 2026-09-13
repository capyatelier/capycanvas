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
  const tap=async selector=>gesture('mouse',await point(`${root} ${selector}`));
  await send({type:'invoke',command:'reset_layout'});
  if(await evaluate('layerApp.state().workspace.zen_mode'))await send({type:'invoke',command:'zen_mode'});
  await send({type:'move_panel',panel:'color',target:{kind:'float',position:[480,120]}});
  const reports=[];
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:2,mobile:false});
  await settle();
  for(const theme of ['dark','light'])for(const width of [200,280,360]) {
    await send({type:'set_theme',theme});
    await evaluate(`(()=>{const workspace=layerApp.app.workspace_persistence();const floating=workspace.layout.floating.find(f=>f.root.panels?.includes('color'));floating.width=${width};floating.height=${width+70};layerApp.dispatch({type:'restore_workspace',workspace});})()`);
    await settle();
    await send({type:'set_color',rgba:[.2,.72,.58,1]});
    for(const space of ['hsv','hls']) {
      await send({type:'color',action:{op:'space',space}});
      const frames=await evaluate(`(()=>{
        const root=document.querySelector(${JSON.stringify(root)}),rect=n=>n.getBoundingClientRect().toJSON();
        return {root:rect(root),wheel:rect(root.querySelector('.color-wheel')),controls:[...root.querySelectorAll('.color-swatch,.color-utility')].map(n=>({label:n.getAttribute('aria-label'),...rect(n),hit:n.contains(document.elementFromPoint(rect(n).x+rect(n).width/2,rect(n).y+rect(n).height/2))})),entries:[...root.querySelectorAll('.number-value:not([hidden]),.number-entry:not([hidden])')].map(rect)};
      })()`);
      assert.ok(frames.root.height<=frames.wheel.width+36,'Only one compact row below the square');
      assert.ok(Math.abs(frames.wheel.width-frames.wheel.height)<1,'Square wheel');
      for(const c of frames.controls){assert.ok(c.hit,`Unobscured ${c.label}`);assert.ok(c.width>=24&&c.height>=24);assert.ok(c.x>=frames.root.x-1&&c.x+c.width<=frames.root.right+1);}
      assert.ok(frames.entries.every(e=>Math.abs(e.y-frames.entries[0].y)<1),'Values share a row');
      const name=`${theme}-${width}-${space}`;reports.push({name,...frames});
      const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:frames.root.x-8,y:frames.root.y-8,width:frames.root.width+16,height:frames.root.height+16,scale:1}});
      await writeFile(`${output}/${name}.png`,Buffer.from(shot.data,'base64'));
      const physical=await call('Page.captureScreenshot',{format:'png',clip:{x:frames.root.x-8,y:frames.root.y-8,width:frames.root.width+16,height:frames.root.height+16,scale:.5}});
      await writeFile(`${output}/${name}-1x.png`,Buffer.from(physical.data,'base64'));
    }
  }
  await call('Emulation.clearDeviceMetricsOverride');
  await settle();
  await evaluate(`(()=>{const workspace=layerApp.app.workspace_persistence(),f=workspace.layout.floating.find(f=>f.root.panels?.includes('color'));f.width=280;f.height=230;layerApp.dispatch({type:'restore_workspace',workspace});})()`);
  await settle();
  assert.ok(await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(root)}),panel=root.closest('.panel').getBoundingClientRect(),footer=root.querySelector('.color-footer').getBoundingClientRect(),wheel=root.querySelector('.color-wheel').getBoundingClientRect();return footer.bottom<=panel.bottom&&wheel.height>=128&&Math.abs(wheel.width-wheel.height)<1;})()`),'Short dock retains the values and a square wheel');
  if(native)await writeFile(`${inputDir}/ready`,'ready');
  // Native compositor delivery verifies touch taps. CDP touch sequences still
  // exercise cancellation below; some Chrome builds omit compatibility clicks.
  for(const device of native?['mouse','touch','pen']:['mouse','pen']) {
    console.log('Color panel input:',device);

    await tap('[data-color-slot="foreground"]');
    await send({type:'set_color',rgba:[.2,.72,.58,1]});
    const before=await read();
    await gesture(device,await point(`${root} .color-wheel`,.94,.5),await point(`${root} .color-wheel`,.5,.94));
    assert.notDeepEqual((await read()).foreground,before.foreground,`${device}: hue drag`);
    for(const space of ['hsv','hls']) {
      await send({type:'color',action:{op:'space',space}});
      const color=(await read()).foreground;
      await gesture(device,await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.58,.42));
      assert.notDeepEqual((await read()).foreground,color,`${device}: ${space} field drag`);
    }
    await gesture(device,await point(`${root} [data-color-slot="background"]`));assert.equal((await read()).slot,'background');
    await gesture(device,await point(`${root} [data-color-slot="transparent"]`));assert.equal((await read()).slot,'transparent');
    await gesture(device,await point(`${root} .color-space`));assert.equal((await read()).space,'hsv');
    const swatches=await read();await gesture(device,await point(`${root} .color-swap`));
    assert.deepEqual((await read()).foreground,swatches.background);
    assert.deepEqual((await read()).background,swatches.foreground);
  }
  const entry=`${root} [data-color-component="0"] .number-entry`;
  const editPoint=await point(`${root} [data-color-component="0"] .number-value`);
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...editPoint,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
  assert.equal(await evaluate(`document.activeElement===document.querySelector(${JSON.stringify(entry)})`),true,'Numeric entry receives focus');
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2,commands:['selectAll']});
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',modifiers:2});
  await call('Input.insertText',{text:'180/2'});
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter'});
  await settle();
  assert.equal(await evaluate('Math.round(layerApp.app.color_panel().components[0].value)'),90,'Exact expression entry');
  assert.equal((await read()).slot,'background','Numeric edit restores remembered paint slot');
  await call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:1});
  await gesture('touch',await point(`${root} .color-wheel`,.5,.5),await point(`${root} .color-wheel`,.55,.45),true);
  const cancelled=await read();
  await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  const p=await point(`${root} .color-wheel`,.9,.5);
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'pen'});await settle();
  assert.deepEqual(await read(),cancelled,'Cancellation releases color contact');
  await writeFile(`${output}/geometry.json`,JSON.stringify(reports,null,2));
  if(native)await writeFile(`${inputDir}/finished`,'done');
  console.log(`Color panel: 12 captures; compact layout; ${native?'native mouse/touch + CDP pen':'CDP mouse/pen'}; slots, mode, swap, expression entry and touch cancellation passed`);
}
