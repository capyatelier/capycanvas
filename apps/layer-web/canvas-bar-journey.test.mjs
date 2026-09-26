import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const bar = '.canvas-action-bar';
const visible = `(()=>{const b=document.querySelector('${bar}');return !!b&&!b.hidden&&!b.classList.contains('suppressed')})()`;

export async function checkCanvasBar({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/canvas-bar/web-tablet':'artifacts/canvas-bar/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const kind=()=>evaluate('layerApp.state().canvas_bar?.context.kind??null');
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=40;
  const pointer=async(type,p,device,fingers=1)=>{
    if(device==='touch')return call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],
      touchPoints:type==='mouseReleased'?[]:Array.from({length:fingers},(_,i)=>({id:touchId*2+i,x:p.x,y:p.y+90*i}))});
    return call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:device,force:type==='mouseReleased'?0:.6});
  };
  const tap=async(p,device)=>{touchId++;await pointer('mousePressed',p,device);await pointer('mouseReleased',p,device);await settle();await pause(60);};
  const press=async(command,device)=>{await wait(`!!document.querySelector('${bar} [data-command="${command}"]')&&${visible}`);await tap(await middle(`${bar} [data-command="${command}"]`),device);};
  const drag=async(points,device,during)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;const fingers=device==='touch'?2:1;
    await pointer('mousePressed',points[0],device,fingers);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,device,fingers);await settle();}
    const result=await during?.();
    await pointer('mouseReleased',points.at(-1),device,fingers);await settle();
    return result;
  };
  const camera=async()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,rotation:c.rotation,t:c.translation,v:c.viewport,a:c.work_area,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const anchor=async()=>{
    const c=await camera(),[x0,y0,x1,y1]=(await state()).canvas_bar.anchor;
    const map=(x,y)=>({x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]});
    const a=map(x0,y0),b=map(x1,y1);return{x:a.x,y:a.y,right:b.x,bottom:b.y};
  };
  const centred=async(box,shape)=>{
    const c=await camera(),left=c.r.x+c.a[0]*c.r.width/c.v[0]+12,right=c.r.x+(c.a[0]+c.a[2])*c.r.width/c.v[0]-12;
    const x=Math.max(left,Math.min((shape.x+shape.right)/2-box.width/2,right-box.width));
    return Math.abs(box.x-x)<2;
  };
  const atEdge=async box=>{
    const c=await camera(),status=await evaluate('layerApp.app.layout(innerWidth,innerHeight).status');
    const floor=Math.min(c.r.y+(c.a[1]+c.a[3])*c.r.height/c.v[1],status.height>0?status.y:Infinity);
    return Math.abs(box.bottom-(floor-12))<2;
  };
  const narrow=async()=>{const c=await camera();return c.a[2]*c.r.width/c.v[0]<600;};
  const beside=async(box,shape)=>await narrow()?atEdge(box):box.y>=shape.bottom&&await centred(box,shape);
  const glassBoxes=()=>evaluate('(()=>{const c=layerApp.canvas.getBoundingClientRect(),b=barProbe.boxes;return Array.from({length:b.length/9},(_,i)=>[b[i*9]+c.x,b[i*9+1]+c.y,b[i*9+2],b[i*9+3]])})()');
  const barBox=async()=>{const r=await rect(bar);return[r.x,r.y,r.width,r.height];};
  const hasBox=(boxes,box)=>boxes.some(b=>b.every((v,i)=>Math.abs(v-box[i])<1));
  const screenshot=async name=>{
    const r=await rect(bar),clip={x:Math.max(0,r.x-40),y:Math.max(0,r.y-160),width:r.width+80,height:r.height+200,scale:1};
    const shot=await call('Page.captureScreenshot',{format:'png',clip});
    await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
  const devices=['pen','touch','mouse'];
  try {
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    await send({type:'preferences',action:{type:'edit',id:'transparency',value:3}});
    await evaluate(`window.barProbe={set:layerApp.app.set_glass,pen:layerApp.app.pen,input:layerApp.app.input,boxes:[],pens:0,pointers:0,shown:0};
      layerApp.app.set_glass=(boxes,...rest)=>{barProbe.boxes=Array.from(boxes);barProbe.glassSets=(barProbe.glassSets??0)+1;return barProbe.set.call(layerApp.app,boxes,...rest);};
      layerApp.app.pen=(...args)=>{barProbe.pens++;return barProbe.pen.apply(layerApp.app,args);};
      layerApp.app.input=event=>{if(event.type==='pointer')barProbe.pointers++;const r=barProbe.input.call(layerApp.app,event);if(event.type==='pointer')(barProbe.log??=[]).push([event.kind,event.phase,r.canvas_bar_hidden,r.handled,r.paint]);return r;};
      barProbe.observer=new MutationObserver(()=>{const b=document.querySelector('${bar}'),on=!b.hidden&&!b.classList.contains('suppressed');if(on&&!barProbe.on)barProbe.shown++;barProbe.on=on;});
      barProbe.observer.observe(document.querySelector('${bar}'),{attributes:true,attributeFilter:['class','hidden']});`);
    await invoke('fit_canvas');
    const c=await camera(),center={x:c.r.x+(c.a[0]+c.a[2]/2)*c.r.width/c.v[0],y:c.r.y+(c.a[1]+c.a[3]/2)*c.r.height/c.v[1]};
    const at=(x,y)=>({x:center.x+x,y:center.y+y});
    for(const [index,device] of devices.entries()) {
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
      await invoke('lasso');await settle();
      const hidden=await evaluate(`${visible}`);
      assert.equal(hidden,false,'No bar before a selection exists');
      const glassBefore=(await glassBoxes()).length;
      await drag([at(-160,-120),at(0,-130),at(160,-120),at(170,0),at(160,100),at(0,110),at(-160,100),at(-160,-120)],device==='touch'?'pen':device);
      await wait(`layerApp.state().layer_tools.has_selection && layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);
      await settle();
      const shape=await anchor(),box=await rect(bar);
      assert.ok(await beside(box,shape),`${device}: the selection bar sits below the selection, centred within the work area ${JSON.stringify({shape,box})}`);
      const boxes=await glassBoxes();
      assert.equal(boxes.length,glassBefore+1,`${device}: the glass region count rises by one while the bar is shown`);
      assert.ok(hasBox(boxes,await barBox()),`${device}: the bar registers its glass region`);
      const before=await evaluate('({pens:barProbe.pens,pointers:barProbe.pointers})');
      for(const tapper of devices) {
        await tap({x:box.x+3,y:box.y+box.height/2},tapper);
        await tap({x:box.x+box.width/2,y:box.y+2},tapper);
      }
      assert.deepEqual(await evaluate('({pens:barProbe.pens,pointers:barProbe.pointers})'),before,`${device}: bar taps never reach the canvas`);
      assert.equal(await kind(),'selection',`${device}: bar taps keep the bar`);
      const transform=`${bar} [data-command="scale_rotate"]`;
      if(await evaluate(`document.querySelector('${transform}').getAttribute('aria-disabled')==='true'`)) {
        assert.equal(await evaluate(`document.querySelector('${transform}').title`),'Select unlocked paint content or a layer mask','A disabled item shows its reason');
        await tap(await middle(transform),device);
        assert.equal(await kind(),'selection','A disabled item does nothing');
        await press('fill_selection',device);
        await wait(`document.querySelector('${transform}')?.getAttribute('aria-disabled')==='false' && ${visible}`);
      }
      await press('scale_rotate',devices[(index+1)%3]);
      await wait(`layerApp.state().layer_tools.tool==='transform' && layerApp.state().canvas_bar?.context.kind==='transform' && ${visible}`);
      for(const id of ['transform_flip_horizontal','reset_transform','cancel_transform','apply_transform'])
        assert.ok(await evaluate(`!!document.querySelector('${bar} [data-command="${id}"]')`),`${device}: the transform bar offers ${id}`);
      const mode=`${bar} [data-toolbar-choice="transform-mode"]`,segment=i=>`${mode} [data-toolbar-segment="transform-mode-${i}"]`;
      assert.ok(await evaluate(`(n=>!!n&&!n.hidden&&n.getBoundingClientRect().width<400)(document.querySelector('${mode}'))`),`${device}: the mode choice is shown at its natural width`);
      assert.ok(await evaluate(`document.querySelector('${bar} [data-command="apply_transform"]').classList.contains('suggested-action')`),'Apply uses the accent style');
      const transformBox=await anchor(),transformBar=await rect(bar);
      assert.ok(await beside(transformBar,transformBox),`${device}: the transform bar sits below the transform box`);
      const shown=await evaluate('barProbe.shown');
      const during=await drag([at(0,0),at(20,10),at(40,20),at(60,30)],device,async()=>{
        await settle();
        return {hidden:!await evaluate(visible),boxes:(await glassBoxes()).length};
      });
      assert.ok(during.hidden,`${device}: the bar hides during a canvas drag ${await evaluate('JSON.stringify(barProbe.log.slice(-12))')}`);
      assert.equal(during.boxes,glassBefore,`${device}: hiding the bar removes its glass region`);
      await wait(visible);await pause(250);
      assert.equal(await evaluate('barProbe.shown'),shown+1,`${device}: the bar returns once after the drag`);
      const moved=await anchor(),movedBar=await rect(bar);
      assert.ok(moved.x>transformBox.x+20,`${device}: the drag moved the transform or panned the canvas`);
      assert.ok(await beside(movedBar,moved),`${device}: the bar follows the transform box ${JSON.stringify({moved,movedBar,camera:await camera()})}`);
      await tap(await middle(segment(1)),device);
      await wait(`layerApp.state().commands.find(c=>c.id==='transform_uniform').selected&&document.querySelector('${segment(1)}').getAttribute('aria-checked')==='true'`);
      await tap(await middle(segment(2)),device);
      await wait(`!!document.querySelector('${bar} [data-command="transform_perspective"]')&&document.querySelector('${segment(2)}').getAttribute('aria-checked')==='true'`);
      await tap(await middle(segment(0)),device);
      await wait(`!document.querySelector('${bar} [data-command="transform_perspective"]')&&layerApp.state().commands.find(c=>c.id==='transform_free').selected&&${visible}`);
      await tap(await middle(`${bar} .canvas-action-bar-more`),device);
      await wait(`!!document.querySelector('.panel-context-menu:popover-open')`);
      const labels=await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open .menu-label')].map(n=>n.textContent)`);
      assert.ok(labels.includes((await state()).commands.find(c=>c.id==='show_canvas_action_bar').label),`${device}: More offers the bar toggle: ${labels}`);
      const overflow=await evaluate(`[...document.querySelectorAll('${bar} .canvas-action-bar-item')].filter(n=>n.hidden).map(n=>n.querySelector('button').getAttribute('aria-label'))`);
      assert.ok(overflow.every(label=>labels.includes(label)),`${device}: More lists every overflowed item: ${overflow} in ${labels}`);
      assert.equal(await evaluate('layerApp.state().layer_tools.tool'),'transform',`${device}: opening More keeps the transform`);
      assert.equal(await evaluate('document.hasFocus()'),true,`${device}: the menu does not take window focus`);
      const pens=await evaluate('barProbe.pens');
      await wait('layerApp.app.brush_ready()');
      await tap(at(-300,0),devices[(index+2)%3]);
      await wait(`!document.querySelector('.panel-context-menu:popover-open')`);
      assert.equal(await evaluate('barProbe.pens'),pens,`${device}: a canvas contact with More open only closes the menu`);
      assert.equal(await evaluate('layerApp.state().layer_tools.tool'),'transform',`${device}: the transform survives the menu`);
      if(index===0) {
        await invoke('show_canvas_action_bar');
        await wait(`layerApp.state().canvas_bar?.items.length===0 && ${visible}`);
        await settle();
        assert.equal(await evaluate(`document.querySelectorAll('${bar} .canvas-action-bar-item').length`),0,'Only completion items remain while the bar is off');
        for(const id of ['cancel_transform','apply_transform'])assert.ok(await evaluate(`!!document.querySelector('${bar} [data-command="${id}"]')`));
        assert.ok(await atEdge(await rect(bar)),'The completion-only bar sits at the bottom edge');
        assert.equal((await state()).canvas_bar.placement,'bottom_edge');
        await screenshot('completion-only');
        await invoke('show_canvas_action_bar');
        await wait(`layerApp.state().canvas_bar?.items.length>0 && ${visible}`);
        await invoke('zen_mode');
        await wait(`document.querySelector('#workspace').classList.contains('zen-hidden')`);await settle();await pause(300);
        assert.ok(await evaluate(visible),'Zen keeps the bar');
        const z=await middle(`${bar} [data-command="apply_transform"]`);
        assert.ok(await evaluate(`!!document.elementFromPoint(${z.x},${z.y})?.closest('${bar}')`),'The bar stays interactive in Zen');
        assert.equal(await evaluate(`getComputedStyle(document.querySelector('${bar}')).opacity`),'1');
        assert.ok(hasBox(await glassBoxes(),await barBox()),'The bar keeps its glass in Zen');
        await screenshot('zen');
        await invoke('zen_mode');await settle();
        for(const name of ['light','dark']) {
          await send({type:'set_theme',theme:name});await wait(visible);await settle();await pause(200);
          await screenshot(`transform-${name}`);
        }
      }
      if(index===1) {
        await press('cancel_transform',device);
        await wait(`layerApp.state().layer_tools.tool!=='transform'`);
      } else {
        await press('apply_transform',device);
        await wait(`layerApp.state().layer_tools.tool!=='transform'`);
        assert.ok((await state()).commands.find(c=>c.id==='undo').enabled);
      }
      assert.notEqual(await kind(),'transform',`${device}: completion ends the transform bar`);
    }
    if(device&&process.env.CAPY_ANDROID_SERIAL) {
      const shell=(...args)=>promisify(execFile)(process.env.ADB??'adb',['-s',process.env.CAPY_ANDROID_SERIAL,'shell',...args]);
      const screen=await evaluate('({ratio:devicePixelRatio,width:screen.width,height:screen.height})');
      const probe=[Math.round(screen.width*screen.ratio/2),Math.round(screen.height*screen.ratio/2)];
      await evaluate(`(()=>{const o=document.createElement('div');o.id='stylus-calibration';o.style.cssText='position:fixed;inset:0;z-index:2147483647';
        o.addEventListener('pointerdown',e=>{window.stylusCalibration={x:e.clientX,y:e.clientY};e.preventDefault();e.stopPropagation();});document.body.append(o);})()`);
      await shell('input','stylus','tap',...probe.map(String));
      await wait('window.stylusCalibration');
      const calibration=await evaluate('stylusCalibration');
      await evaluate(`document.querySelector('#stylus-calibration').remove();delete window.stylusCalibration`);
      const offset=[probe[0]-calibration.x*screen.ratio,probe[1]-calibration.y*screen.ratio];
      const physical=p=>[Math.round(offset[0]+p.x*screen.ratio),Math.round(offset[1]+p.y*screen.ratio)].map(String);
      const source={pen:'stylus',touch:'touchscreen',mouse:'mouse'};
      await evaluate(`window.osInput=[];document.addEventListener('pointerdown',e=>osInput.push({type:e.pointerType,bar:!!e.target.closest('${bar}'),canvas:e.target===layerApp.canvas}),true)`);
      const osTap=async(selector,kind)=>{
        await wait(`!!document.querySelector('${selector}')&&${visible}`);
        await shell('input',source[kind],'tap',...physical(await middle(selector)));await settle();await pause(150);
      };
      for(const kind of ['pen','touch','mouse']) {
        if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
        await invoke('lasso');await settle();
        await drag([at(-140,-100),at(140,-100),at(140,90),at(-140,90),at(-140,-100)],'pen');
        await wait(`layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);
        const pointers=await evaluate('barProbe.pointers');
        await evaluate('osInput.length=0');
        await osTap(`${bar} [data-command="scale_rotate"]`,kind);
        await wait(`layerApp.state().layer_tools.tool==='transform' && ${visible}`);
        const events=await evaluate('osInput');
        assert.ok(events.length&&events.every(e=>e.type===kind&&e.bar),`OS ${kind} taps reach the bar as ${kind}: ${JSON.stringify(events)}`);
        assert.equal(await evaluate('barProbe.pointers'),pointers,`OS ${kind} taps on the bar never reach the canvas`);
        if(kind==='pen') {
          const shown=await evaluate('barProbe.shown'),path=[at(0,0),at(25,10),at(50,20),at(70,30)].map(physical);
          await wait('layerApp.app.brush_ready()');
          await shell('input','stylus','motionevent','DOWN',...path[0]);
          for(const p of path.slice(1))await shell('input','stylus','motionevent','MOVE',...p);
          await settle();
          assert.equal(await evaluate(visible),false,'A real stylus drag on the canvas hides the bar');
          await shell('input','stylus','motionevent','UP',...path.at(-1));
          await wait(visible);await pause(300);
          assert.equal(await evaluate('barProbe.shown'),shown+1,'The bar returns once after a real stylus drag');
          await osTap(`${bar} .canvas-action-bar-more`,kind);
          await wait(`!!document.querySelector('.panel-context-menu:popover-open')`);
          await osTap(`${bar} .canvas-action-bar-more`,kind);
          await wait(`!document.querySelector('.panel-context-menu:popover-open')`);
          assert.equal(await evaluate('layerApp.state().layer_tools.tool'),'transform','A real stylus More tap toggles its menu and keeps the transform');
        }
        await osTap(`${bar} [data-command="${kind==='touch'?'cancel_transform':'apply_transform'}"]`,kind);
        await wait(`layerApp.state().layer_tools.tool!=='transform'`);
      }
    }
    const stroke=async withBar=>{
      if(withBar){await drag([at(-120,-80),at(120,-80),at(120,80),at(-120,-80)],'pen');await wait(`layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);await pause(300);}
      else {await invoke('deselect');await settle();await pause(300);}
      await invoke('lasso');await wait('layerApp.app.brush_ready()');
      const metrics=async()=>Object.fromEntries((await call('Performance.getMetrics')).metrics.filter(m=>['LayoutCount','RecalcStyleCount','LayoutDuration','RecalcStyleDuration'].includes(m.name)).map(m=>[m.name,m.value]));
      const points=Array.from({length:61},(_,i)=>at(-200+400*i/60,-220+40*Math.sin(i/6)));
      const sample=async()=>({...await metrics(),glass:await evaluate('barProbe.glassSets??0')});
      const delta=(a,b)=>({layouts:b.LayoutCount-a.LayoutCount,styles:b.RecalcStyleCount-a.RecalcStyleCount,
        layout_ms:+((b.LayoutDuration-a.LayoutDuration)*1000).toFixed(3),style_ms:+((b.RecalcStyleDuration-a.RecalcStyleDuration)*1000).toFixed(3),glass:b.glass-a.glass});
      const start=await sample();
      touchId++;await pointer('mousePressed',points[0],'pen');await settle();
      const down=await sample();
      for(const p of points.slice(1)){await pointer('mouseMoved',p,'pen');}
      await settle();
      const moved=await sample(),result={bar:withBar,hidden:!await evaluate(visible),samples:points.length-1,down:delta(start,down),moves:delta(down,moved)};
      await pointer('mouseReleased',points.at(-1),'pen');await settle();
      return result;
    };
    await call('Performance.enable');
    const strokes=[await stroke(false),await stroke(true),await stroke(false),await stroke(true)];
    await writeFile(`${directory}/stroke-metrics.json`,JSON.stringify(strokes,null,2));
    const plain=strokes.filter(s=>!s.bar),most=key=>Math.max(...plain.map(s=>s.moves[key])),down=key=>Math.max(...plain.map(s=>s.down[key]));
    for(const s of strokes.filter(s=>s.bar)) {
      assert.ok(s.hidden,`The bar is hidden while the lasso stroke continues ${JSON.stringify(strokes)}`);
      assert.ok(s.down.glass<=down('glass')+1&&s.down.layouts<=down('layouts')+1,`Hiding at pen-down republishes glass once and lays out at most once ${JSON.stringify(strokes)}`);
      assert.ok(s.moves.glass<=most('glass')&&s.moves.layouts<=most('layouts'),`The hidden bar adds no glass or layout work to stroke samples ${JSON.stringify(strokes)}`);
    }
    console.log('Stroke metrics',JSON.stringify(strokes));
    await invoke('deselect');await settle();
    assert.equal(await evaluate(visible),false,'Deselect removes the bar');
    console.log(`PASS canvas action bar (${device?'tablet':'desktop'}): selection bar beside new selections, Transform, taps never paint, hide during drags, More, Apply/Cancel, completion-only, Zen, glass and screenshots in ${directory}`);
  } catch(error) {
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/failure.png`,Buffer.from(shot.data,'base64'));
    console.error('Canvas bar state',await evaluate('JSON.stringify({tool:layerApp.state().layer_tools.tool,bar:layerApp.state().canvas_bar},(_,v)=>typeof v==="bigint"?String(v):v)'));
    throw error;
  } finally {
    await evaluate(`if(window.barProbe){layerApp.app.set_glass=barProbe.set;layerApp.app.pen=barProbe.pen;layerApp.app.input=barProbe.input;barProbe.observer.disconnect();delete window.barProbe;}`);
    await send({type:'set_theme',theme});
    await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
  }
}
