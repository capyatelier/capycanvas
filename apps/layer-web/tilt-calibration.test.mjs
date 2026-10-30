import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkTiltCalibration({call,evaluate,settle}) {
  const panel='#pen-tilt-dialog';
  let primary=101,secondary=202;
  const read=()=>evaluate("JSON.parse(JSON.stringify(layerApp.state().tilt_calibration??null,(_,v)=>typeof v==='bigint'?Number(v):v))");
  const saved=()=>evaluate('layerApp.state().settings.tilt_calibrations');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();};
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,25)}poll()})`);
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  const pointer=async(type,point,device='mouse',buttons=type==='mouseReleased'?0:1)=>{await call('Input.dispatchMouseEvent',{type,...point,pointerType:device,button:buttons||type==='mouseReleased'?'left':'none',buttons,clickCount:1,force:buttons?.65:0,tiltX:30,tiltY:15});await settle();};
  const gesture=async(start,end,device='mouse')=>{
    if(device==='touch'){
      for(const [type,p] of [['touchStart',start],['touchMove',end],['touchEnd',null]]){await call('Input.dispatchTouchEvent',{type,touchPoints:p?[{...p,id:87,radiusX:1,radiusY:1,force:.65}]:[]});await settle();}
    }else{
      await pointer('mousePressed',start,device);await pointer('mouseMoved',end,device);await pointer('mouseReleased',end,device);
    }
  };
  const contact=async(selector,device)=>{const p=center(await rect(selector));await gesture(p,p,device);};
  const points=async()=>(await read()).editor.points;
  const graphPoint=async(x,y)=>evaluate(`(()=>{const r=document.querySelector('${panel} .curve-editor').getBoundingClientRect(),i=layerApp.state().tilt_calibration.editor.controls.inset;return{x:r.left+i+${x}*(r.width-2*i),y:r.top+i+(1-${y})*(r.height-2*i)}})()`);
  const response=async()=>({response:await points(),axes:(await read()).axes});
  const artwork=()=>evaluate("JSON.parse(JSON.stringify((()=>{const s=layerApp.state();return{camera:s.camera,layers:s.layers,history:s.commands.filter(c=>['undo','redo','deselect'].includes(c.id)).map(c=>({id:c.id,enabled:c.enabled,label:c.label})),workspace:s.workspace}})(),(_,v)=>typeof v==='bigint'?Number(v):v))");
  const sensor=async(selector,{id=8001,identity=primary,phase='hover',tiltX=30,tiltY=15}={})=>{
    await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n.getBoundingClientRect();for(const type of ${JSON.stringify(phase==='press'?['pointerdown','pointerup']:['pointermove'])}){const down=type==='pointerdown',e=new PointerEvent(type,{bubbles:true,cancelable:true,pointerType:'pen',pointerId:${id},clientX:r.x+r.width/2,clientY:r.y+r.height/2,button:down?0:-1,buttons:down?1:0,pressure:down?.65:0,tiltX:${tiltX},tiltY:${tiltY}});Object.defineProperty(e,'persistentDeviceId',{value:${identity}});n.dispatchEvent(e);}})()`);await settle();
  };
  const open=async()=>{
    await evaluate('layerApp.app.wait_for_canvas()');await wait('!layerApp.documents.busy()');
    await click('#header [data-command="settings"]');await wait("document.querySelector('#settings').open");
    await click('[data-settings-page="input"]');
    const rows=await evaluate('layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).map(r=>r.id)');
    assert.ok(rows.indexOf('pen_tilt')===rows.indexOf('pen_pressure')+1,'Pen tilt follows Pen pressure');
    await click('#setting-pen-tilt');
    await wait("!!layerApp.state().tilt_calibration&&!!document.querySelector('#pen-tilt-dialog')&&!document.querySelector('#settings').open");
    await settle();
  };
  const reload=async()=>{
    const origin=await evaluate('performance.timeOrigin');await call('Runtime.evaluate',{expression:'void 0',userGesture:true});await call('Page.reload');
    for(const end=Date.now()+45000;;await new Promise(resolve=>setTimeout(resolve,50))){
      assert.ok(Date.now()<end,'Tilt response reload timed out');
      if(await evaluate(`performance.timeOrigin!==${origin}&&!!window.layerApp&&document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready()`).catch(()=>false))break;
    }
    await evaluate('layerApp.documents.startRecovery()');await settle();
  };
  const capture=async name=>{
    const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/tilt-calibration/web';await mkdir(directory,{recursive:true});
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));
  };
  const pixel=async(x,y)=>{
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:Math.floor(x),y:Math.floor(y),width:1,height:1,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const canvas=new OffscreenCanvas(1,1),ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);return [...ctx.getImageData(0,0,1,1).data]})()`);
  };
  const assertCompactRow=async()=>{
    const geometry=await evaluate(`(()=>{const p=document.querySelector('${panel}'),r=n=>n.getBoundingClientRect().toJSON(),caption=selector=>{const node=p.querySelector(selector),range=document.createRange();range.selectNodeContents(node);return{box:r(node),text:range.getBoundingClientRect().toJSON()}};return{panel:r(p),labels:[caption('.utility-sensitivity-title'),caption('.utility-direction-title')],buttons:[...p.querySelectorAll('.utility-sensitivity button,.utility-direction-controls button')].map(r)}})()`);
    const [sensitivity,direction]=geometry.labels,[less,more,...axes]=geometry.buttons;
    for(const label of geometry.labels)assert.ok(Math.abs(label.box.width-label.text.width)<.75,'Compact labels keep intrinsic text widths');
    for(const button of geometry.buttons)assert.ok(Math.abs(button.width-36)<.5&&Math.abs(button.height-36)<.5,'Compact buttons keep square 36px targets');
    assert.ok(Math.abs(less.x-sensitivity.text.right-4)<.75,'Sensitivity text stays four pixels from its bar');
    assert.ok(Math.abs(axes[0].x-direction.text.right-4)<.75,'Direction text stays four pixels from its bar');
    assert.ok(direction.text.x-more.right>=7.25,'Compact groups stay at least eight pixels apart');
    assert.ok(Math.abs(sensitivity.text.x-geometry.panel.x-16)<.75,'Sensitivity starts at the left content gutter');
    assert.ok(Math.abs(geometry.panel.right-axes.at(-1).right-16)<.75,'Direction ends at the right content gutter');
    assert.ok(geometry.labels.every(label=>label.box.height<=36),'Compact captions fit one control row');
  };
  const shortViewport=async theme=>{
    await call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});await settle();
    let tiltPlot;
    try{
      for(const mode of ['tilt','pressure']){
        const selector=`#pen-${mode}-dialog`;await send({type:'invoke',command:`pen_${mode}`});
        await wait(`!!document.querySelector('${selector}')&&Math.abs(document.querySelector('${selector}').getBoundingClientRect().height-layerApp.state().${mode}_calibration.bounds.height)<1`);
        const geometry=await evaluate(`(()=>{const n=document.querySelector('${selector}'),r=n=>n?.getBoundingClientRect().toJSON();return{panel:r(n),graph:r(n.querySelector('.curve-editor')),direction:r(n.querySelector('.utility-direction')),body:r(n.querySelector('.utility-body')),footer:r(n.querySelector('.utility-actions'))}})()`);
        for(const r of [geometry.panel,geometry.footer])assert.ok(r.x>=-.5&&r.y>=-.5&&r.x+r.width<=640.5&&r.y+r.height<=480.5,`${theme}/${mode}: short viewport contains utility and footer`);
        const midpoint=geometry.panel.x+geometry.panel.width/2;
        assert.ok(Math.abs(geometry.graph.width-geometry.graph.height)<1,`${theme}/${mode}: short plot stays square`);
        assert.ok(Math.abs(geometry.graph.x+geometry.graph.width/2-midpoint)<1,`${theme}/${mode}: scrollbar preserves the outer centerline`);
        if(mode==='tilt'){
          await assertCompactRow();
          tiltPlot=geometry.graph;
          assert.ok(geometry.direction.x>=geometry.graph.x+geometry.graph.width/2&&geometry.direction.y>=geometry.graph.y+geometry.graph.height/2,'Short direction preview sits in the lower-right plot quadrant');
          assert.ok(geometry.direction.x+geometry.direction.width<=geometry.graph.x+geometry.graph.width+1&&geometry.direction.y+geometry.direction.height<=geometry.graph.y+geometry.graph.height+1,'Short direction preview stays inside the plot');
          for(const [label,key] of [['Reverse horizontal','reverse_horizontal'],['Reverse vertical','reverse_vertical'],['Swap axes','swap']]){
            const control=await rect(`${selector} [aria-label="${label}"]`);assert.ok(control.y>=geometry.graph.y+geometry.graph.height&&control.y+control.height<=geometry.footer.y,`${label} stays visible above the footer`);
            const before=(await read()).axes[key];await contact(`${selector} [aria-label="${label}"]`,'pen');assert.notEqual((await read()).axes[key],before,`Short viewport pen contact activates ${label}`);
          }
        }else assert.ok(Math.abs(geometry.graph.width-tiltPlot.width)<1&&Math.abs(geometry.graph.height-tiltPlot.height)<1,'Short pressure and tilt plots keep matching dimensions');
        await capture(`${theme}-short-${mode}`);await contact(`${selector} .utility-actions button:nth-last-child(2)`,'mouse');
      }
    }finally{await call('Emulation.clearDeviceMetricsOverride');await settle();}
  };
  const motion=[];
  const measureMotion=async theme=>{
    const bounds=await rect(panel),title={x:bounds.x+80,y:bounds.y+18};await gesture(title,{x:580,y:118});
    for(const kind of ['frame','curve']){
      await click(`${panel} .utility-actions button:first-child`);
      if(kind==='curve'){const point=await graphPoint(.4,.4);await gesture(point,point);}
      const start=async()=>kind==='curve'?graphPoint(...(await points())[1]):rect(panel).then(r=>({x:r.x+80,y:r.y+18}));
      const prime=await start();await gesture(prime,{x:prime.x+8,y:prime.y+8});
      for(let run=0;run<3;run++){
        const origin=await start();await pointer('mousePressed',origin);
        await evaluate(`(()=>{const kind=${JSON.stringify(kind)},node=document.querySelector('${panel}'),value=()=>kind==='frame'?node.style.transform:JSON.stringify(layerApp.state().tilt_calibration.editor.points),probe=window.tiltMotion={running:true,frames:[],values:[],last:value()};function frame(t){if(!probe.running)return;const next=value();if(next!==probe.last){probe.frames.push(t);probe.values.push(next);probe.last=next;}requestAnimationFrame(frame);}requestAnimationFrame(frame)})()`);
        const pending=[],begin=performance.now();let point=origin,inputs=0;
        while(performance.now()-begin<5200){
          const t=(performance.now()-begin)/1000;
          point={x:origin.x+(kind==='frame'?100:32)*Math.sin(t*5),y:origin.y+(kind==='frame'?50:24)*Math.sin(t*7)};
          pending.push(call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,pointerType:'mouse',button:'left',buttons:1,force:.65}));inputs++;
          await new Promise(resolve=>setTimeout(resolve,4));
        }
        await Promise.all(pending);
        const probe=await evaluate('(()=>{tiltMotion.running=false;return{frames:tiltMotion.frames,values:tiltMotion.values}})()');
        await pointer('mouseReleased',point);
        const frames=probe.frames,durationMs=frames.at(-1)-frames[0],gaps=frames.slice(1).map((t,i)=>t-frames[i]).sort((a,b)=>a-b);
        assert.ok(frames.length>100&&durationMs>=5000,`${theme}/${kind}/${run}: sustained observed motion`);
        motion.push({theme,kind,run:run+1,input:'CDP mouse',timing:'changed-value requestAnimationFrame callbacks',qualification:false,inputs,frames:frames.length,durationMs,callbackHz:(frames.length-1)*1000/durationMs,gapP99Ms:gaps[Math.floor(gaps.length*.99)],samples:probe});
      }
    }
    await click(`${panel} .utility-actions button:first-child`);
  };
  const axes={swap:false,reverse_horizontal:false,reverse_vertical:false};
  const savedWorkspace=await evaluate("JSON.parse(JSON.stringify(layerApp.state().workspace,(_,v)=>typeof v==='bigint'?Number(v):v))");
  const columnWorkspace=structuredClone(savedWorkspace);
  Object.assign(columnWorkspace.layout,{bands:[{id:40,edge:'right',extent:42,root:{kind:'tabs',id:41,panels:['layers','properties'],active:'layers',tab_style:'icon'}}],floating:[],collapsed:[{root:41,expanded_width:246}],column_stacks:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],next_id:Math.max(42,columnWorkspace.layout.next_id)});
  columnWorkspace.zen_mode=false;
  await evaluate('layerApp.documents.startRecovery()');
  for(const theme of ['light','dark']){
    primary=theme==='light'?101:303;secondary=primary+101;
    for(const [selector,phase] of [[`${panel} .utility-direction`,'hover'],[`${panel} .utility-direction`,'press'],[`${panel} .utility-device`,'hover'],[`${panel} .utility-device`,'press']]){
      await reload();await send({type:'set_theme',theme});await send({type:'invoke',command:'lasso'});await open();
      assert.equal((await read()).ready,false,'Cold editor waits for a pen');
      assert.equal(await evaluate(`document.querySelector('${panel} .utility-actions button:last-child').disabled`),true);
      await click(`${panel} .utility-sensitivity button:last-child`);
      await click(`${panel} [aria-label="Reverse horizontal"]`);
      const initial=await rect(panel),header={x:initial.x+80,y:initial.y+18};await gesture(header,{x:header.x-24,y:header.y+12});
      const draft=await response(),before=await artwork(),bounds=await rect(panel);
      const sensing=center(await rect(selector));
      if(phase==='press'){await pointer('mousePressed',sensing,'pen');await pointer('mouseReleased',sensing,'pen');}
      else await pointer('mouseMoved',sensing,'pen',0);
      assert.equal((await read()).ready,true,`${theme}: first pen ${phase} over ${selector} identifies a pen with Lasso`);
      assert.deepEqual(await response(),draft,'First identification retains response and direction edits');
      const identified=await rect(panel);
      for(const key of ['x','y','width','height'])assert.ok(Math.abs(bounds[key]-identified[key])<1,`First identification retains ${key}`);
      assert.deepEqual(await artwork(),before,'Utility sensing leaves artwork, selection, camera and workspace unchanged');
      assert.ok((await read()).input_direction,'Hover and press update live direction');
      assert.equal(await evaluate(`document.querySelector('${panel} .utility-actions button:last-child').disabled`),false);
      await click(`${panel} .utility-actions button:nth-last-child(2)`);
    }
    await send({type:'restore_workspace',workspace:columnWorkspace});await click('.collapsed-column .column-tab[data-panel="layers"]');
    await wait('layerApp.app.layout(innerWidth,innerHeight).collapsed.some(c=>c.open)');
    await open();await click(`${panel} .utility-actions button:first-child`);
    assert.deepEqual(await response(),{response:[[0,0],[1,1]],axes},'Reset restores neutral response and direction');
    const geometry=await evaluate(`(()=>{const p=document.querySelector('${panel}'),r=n=>n.getBoundingClientRect().toJSON(),g=p.querySelector('.curve-editor'),h=p.querySelector('.utility-header'),title=p.querySelector('.utility-title'),close=p.querySelector('.utility-close');return{panel:r(p),graph:r(g),header:r(h),title:r(title),close:r(close),buttons:[...p.querySelectorAll('.utility-sensitivity button,.utility-actions button')].map(r),toggles:[...p.querySelectorAll('[aria-pressed]')].map(n=>({rect:r(n),label:n.getAttribute('aria-label'),title:n.title,icon:r(n.querySelector('svg'))})),direction:r(p.querySelector('.utility-direction')),axesY:[...p.querySelectorAll('.curve-axis-y span')].map(n=>n.textContent),axesX:[...p.querySelectorAll('.curve-axis-x span')].map(n=>n.textContent)}})()`);
    assert.ok(geometry.panel.width>=384,'Utility retains the 384px minimum');
    assert.ok(Math.abs(geometry.graph.width-geometry.graph.height)<1,'Tilt graph is square');
    assert.ok(Math.abs(geometry.title.y+geometry.title.height/2-geometry.close.y-geometry.close.height/2)<1,'Title and Close share a centerline');
    assert.ok(geometry.direction.x>=geometry.graph.x+geometry.graph.width/2&&geometry.direction.y>=geometry.graph.y+geometry.graph.height/2,'Direction preview overlays the lower-right plot quadrant');
    assert.ok(geometry.direction.x+geometry.direction.width<=geometry.graph.x+geometry.graph.width+1&&geometry.direction.y+geometry.direction.height<=geometry.graph.y+geometry.graph.height+1,'Direction preview stays inside the graph');
    for(const r of geometry.buttons)assert.ok(Math.abs(r.height-36)<1,'Actions use 36px toolbar tiles');
    for(const r of [...geometry.buttons,...geometry.toggles.map(t=>t.rect)])assert.ok(r.x>=geometry.panel.x&&r.y>=geometry.panel.y&&r.x+r.width<=geometry.panel.x+geometry.panel.width+.5&&r.y+r.height<=geometry.panel.y+geometry.panel.height+.5,'Every compact utility target stays inside the panel');
    assert.equal(geometry.toggles.length,3);
    for(const t of geometry.toggles){assert.ok(Math.abs(t.rect.width-36)<1&&Math.abs(t.rect.height-36)<1,'Direction toggles use square 36px targets');assert.equal(t.title,t.label);assert.ok(Math.abs(t.icon.width-16)<1&&Math.abs(t.icon.height-16)<1,'Direction icons use 16px boxes');}
    for(let i=1;i<geometry.toggles.length;i++)assert.ok(Math.abs(geometry.toggles[i].rect.x-geometry.toggles[i-1].rect.x-36)<1,'Direction segments touch');
    const sensitivity=geometry.buttons.slice(0,2);for(const r of sensitivity)assert.ok(Math.abs(r.width-36)<1&&Math.abs(r.y-geometry.toggles[0].rect.y)<1,'Sensitivity uses square targets on the direction row');
    assert.ok(Math.abs(sensitivity[1].x-sensitivity[0].x-36)<1,'Sensitivity segments touch');
    await assertCompactRow();
    assert.deepEqual(geometry.axesY,['90°','Output','0°']);assert.deepEqual(geometry.axesX,['0°','Input','90°']);
    assert.equal(await evaluate(`document.querySelector('${panel}').getAttribute('aria-modal')`),'false');
    const layout=await evaluate('layerApp.app.layout(innerWidth,innerHeight)'),opened=layout.collapsed.find(c=>c.open).open.bounds;
    const placed=(await read()).bounds;
    assert.ok(placed.x>=layout.work_area.x+15&&placed.y>=layout.work_area.y+15,'Default position has the clear-canvas inset');
    assert.ok(placed.x+placed.width<=opened.x+1||placed.y+placed.height<=opened.y+1||placed.y>=opened.y+opened.height-1,'Default position avoids the expanded column');
    await capture(`${theme}-default`);
    const headerPoint={x:geometry.panel.x+80,y:geometry.panel.y+18};
    const overlapX=Math.min(opened.x+opened.width-geometry.panel.width+20,layout.viewport[0]-geometry.panel.width-4);
    await gesture(headerPoint,{x:headerPoint.x+overlapX-placed.x,y:headerPoint.y});
    const overlap=await rect(panel),connections=await evaluate("[...document.querySelectorAll('.column-connection')].map(n=>n.getBoundingClientRect().toJSON())");
    for(const [name,r] of [['expanded column',opened],...connections.map(r=>['drawer connector',r])]){
      const x=Math.max(overlap.x+8,r.x)+Math.max(0,Math.min(overlap.x+overlap.width-8,r.x+r.width)-Math.max(overlap.x+8,r.x))/2;
      const y=Math.max(overlap.y+8,r.y)+Math.max(0,Math.min(overlap.y+overlap.height-8,r.y+r.height)-Math.max(overlap.y+8,r.y))/2;
      if(x<overlap.x+overlap.width-8&&y<overlap.y+overlap.height-8&&x<r.x+r.width&&y<r.y+r.height){
        assert.equal(await evaluate(`document.elementFromPoint(${x},${y})?.closest('${panel}')?.id`),'pen-tilt-dialog',`Utility controls stay above the ${name}`);
        const before=await pixel(x,y);
        await evaluate("window.tiltHiddenChrome=[...document.querySelectorAll('.collapsed-column,.column-connection,.dock-group:not(.utility-panel)')].map(node=>({node,visibility:node.style.visibility}));for(const {node} of tiltHiddenChrome)node.style.visibility='hidden'");
        const without=await pixel(x,y);
        await evaluate("for(const {node,visibility} of tiltHiddenChrome)node.style.visibility=visibility;delete window.tiltHiddenChrome");
        assert.deepEqual(before,without,`Utility pixels above the ${name} remain opaque`);
      }
    }
    await capture(`${theme}-stacking`);
    const stacked=await points();await contact(`${panel} .utility-sensitivity button:last-child`,'mouse');
    assert.notDeepEqual(await points(),stacked,'The overlapped utility receives native control contacts');await click(`${panel} .utility-actions button:first-child`);
    if(process.env.LAYER_TILT_MOTION==='1')await measureMotion(theme);
    for(const device of ['mouse','touch','pen']){
      const before=await rect(panel),start={x:before.x+80,y:before.y+18};await gesture(start,{x:start.x-20,y:start.y+10},device);
      const moved=await rect(panel);assert.ok(Math.abs(moved.x-before.x+20)<2&&Math.abs(moved.y-before.y-10)<2,`${device} moves the utility`);
      const point=await graphPoint(.4,.4);await gesture(point,point,device);assert.equal((await points()).length,3,`${device} adds a curve control`);
      const control=await graphPoint(...(await points())[1]),previous=await points();await gesture(control,{x:control.x+8,y:control.y-8},device);
      assert.notDeepEqual(await points(),previous,`${device} edits a curve control`);
      await contact(`${panel} .utility-actions button:first-child`,device);assert.deepEqual((await read()).axes,axes);
      for(const [label,key] of [['Reverse horizontal','reverse_horizontal'],['Reverse vertical','reverse_vertical'],['Swap axes','swap']]){
        await contact(`${panel} [aria-label="${label}"]`,device);axes[key]=true;
        assert.deepEqual((await read()).axes,axes,`${device}: ${label} toggles independently`);
      }
      await contact(`${panel} .utility-actions button:first-child`,device);axes.swap=axes.reverse_horizontal=axes.reverse_vertical=false;
    }
    await sensor(`${panel} .utility-direction`);await click(`${panel} .utility-sensitivity button:last-child`);
    const more=await points();assert.ok(more.at(-1)[0]<1&&more.at(-1)[1]===1,'More sensitive reaches full tilt earlier');
    await click(`${panel} .utility-sensitivity button:first-child`);assert.deepEqual(await points(),[[0,0],[1,1]]);
    await click(`${panel} .utility-sensitivity button:first-child`);assert.ok((await points()).at(-1)[1]<1,'Less sensitive lowers the endpoint from neutral');
    await click(`${panel} [aria-label="Swap axes"]`);
    await send({type:'invoke',command:'brush'});await wait('layerApp.app.brush_ready()');
    const paintBefore=await evaluate('layerApp.state().layers.map(l=>Number(l.paint_revision))');
    const canvasPoint=await evaluate('(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x+r.width*.25,y:r.y+r.height*.6}})()');
    await gesture(canvasPoint,{x:canvasPoint.x+40,y:canvasPoint.y+10},'pen');
    await wait(`JSON.stringify(layerApp.state().layers.map(l=>Number(l.paint_revision)))!==${JSON.stringify(JSON.stringify(paintBefore))}`);
    assert.equal(await evaluate("document.querySelector('#status').textContent"),'','Canvas pen contact paints while calibration is open');
    const edited=await response(),target=(await read()).device;
    await sensor(`${panel} .utility-direction`,{id:8002,identity:secondary,tiltX:-15,tiltY:35});
    assert.equal((await read()).device,target,'An open editor retains its original pen');assert.deepEqual(await response(),edited);
    const baseline=await saved();await click(`${panel} .utility-actions button:last-child`);assert.equal(await read(),null);
    const committed=await saved();
    assert.deepEqual(committed['web:pen'],edited,'Apply saves the shared browser pen profile');
    assert.deepEqual(Object.fromEntries(Object.entries(committed).filter(([key])=>key!=='web:pen')),Object.fromEntries(Object.entries(baseline).filter(([key])=>key!=='web:pen')),'Apply leaves other host profiles unchanged');
    assert.deepEqual(await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1')).tilt_calibrations"),committed,'Apply persists profiles');
    await reload();await send({type:'set_theme',theme});await send({type:'invoke',command:'lasso'});await open();
    await sensor(`${panel} .utility-direction`,{id:9001,identity:primary+999});assert.deepEqual(await response(),edited,'Reload and changed pointer/device IDs retain the browser pen profile');
    for(const close of ['cancel','close','escape']){
      await click(`${panel} .utility-actions button:first-child`);
      if(close==='cancel')await click(`${panel} .utility-actions button:nth-last-child(2)`);
      else if(close==='close')await click(`${panel} .utility-close`);
      else {await evaluate(`document.querySelector('${panel} .curve-editor').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}))`);await settle();}
      assert.equal(await read(),null,`${close} closes the editor`);assert.deepEqual(await saved(),committed,`${close} discards changes`);await open();
    }
    await click(`${panel} .utility-actions button:nth-last-child(2)`);
    await sensor('#canvas',{id:9002,identity:secondary+999});await open();assert.deepEqual(await response(),edited,'The browser profile follows later pen pointer IDs');
    await click(`${panel} .utility-actions button:nth-last-child(2)`);assert.deepEqual(await saved(),committed,'Transient browser identifiers create no extra profiles');
    await send({type:'invoke',command:'pen_pressure'});await wait("!!document.querySelector('#pen-pressure-dialog')");
    const pressure=await rect('#pen-pressure-dialog .curve-editor');assert.ok(Math.abs(pressure.width-geometry.graph.width)<1&&Math.abs(pressure.height-geometry.graph.height)<1,'Pressure and tilt use matched plot dimensions');
    await send({type:'invoke',command:'pen_tilt'});assert.equal(await evaluate('layerApp.state().pressure_calibration??null'),null,'Opening tilt closes pressure');await wait("!!document.querySelector('#pen-tilt-dialog')");
    await capture(`${theme}-profiles`);await click(`${panel} .utility-close`);
    await sensor('#canvas',{id:9002,identity:secondary+999});
    const previous=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
    await send({type:'invoke',command:'new_document'});await evaluate(createDrawing({width:256,height:256}));
    await wait(`Number(layerApp.app.document_tabs(0).selected)!==${previous}&&layerApp.app.brush_ready()&&!layerApp.documents.busy()`);
    await sensor('#canvas',{id:9002,identity:secondary+999});await open();
    assert.equal((await read()).ready,true,'The same pen pointer identifies itself after drawing activation');
    assert.deepEqual(await response(),edited,'A new drawing retains the browser calibration');await click(`${panel} .utility-close`);
    await evaluate(`layerApp.documents.select(BigInt(${previous}))`);await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()');
    await shortViewport(theme);
    assert.equal(await evaluate("document.querySelector('#status').textContent"),'','Tilt journey leaves no error notice');
  }
  await send({type:'restore_workspace',workspace:savedWorkspace});
  if(motion.length){const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/tilt-calibration/web';await mkdir(directory,{recursive:true});await writeFile(`${directory}/motion.json`,JSON.stringify(motion,null,2));console.log('Web tilt desktop callback diagnostics:',JSON.stringify(motion.map(({samples,...row})=>row)));}
  console.log('PASS: Pen tilt calibration in light/dark themes, first Lasso sensing, mouse/touch/pen, independent axes and browser profile persistence');
}
