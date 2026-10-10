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
  const shownItem=selector=>`(n=>!!n&&!n.closest('.canvas-action-bar-item,.canvas-action-bar-completion').hidden)(document.querySelector(${JSON.stringify(selector)}))`;
  const openMenu='.panel-context-menu:popover-open';
  const hasRow=label=>`[...document.querySelectorAll('${openMenu} .menu-label')].some(n=>n.textContent===${JSON.stringify(label)})`;
  const menuLabels=()=>evaluate(`[...document.querySelectorAll('${openMenu} .menu-label')].map(n=>n.textContent)`);
  const chooseRow=async(label,device)=>{
    await wait(hasRow(label));
    await tap(await evaluate(`(()=>{const r=[...document.querySelectorAll('${openMenu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),device);
  };
  const openMore=async device=>{await tap(await middle(`${bar} .canvas-action-bar-more`),device);await wait(`!!document.querySelector('${openMenu}')`);};
  const press=async(command,device)=>{
    const selector=`${bar} [data-command="${command}"]`;
    await wait(`!!document.querySelector('${selector}')&&${visible}`);
    if(await evaluate(shownItem(selector)))return tap(await middle(selector),device);
    await openMore(device);
    await chooseRow(await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').label`),device);
  };
  const openBarMenu=async(id,device)=>{
    const selector=`${bar} [data-canvas-bar-menu="${id}"]`;
    await wait(`!!document.querySelector('${selector}')&&${visible}`);
    if(!await evaluate(shownItem(selector))){
      await openMore(device);await chooseRow(await evaluate(`document.querySelector('${selector}').getAttribute('aria-label')`),device);return 'more';
    }
    await tap(await middle(selector),device);
    await wait(`document.querySelector('${openMenu}')?.menuOwner===document.querySelector('${selector}')`);
    assert.ok(await evaluate(`document.activeElement!==document.querySelector('${selector}')`),`${device}: a bar menu button never takes focus`);
    return 'bar';
  };
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
      layerApp.app.input=event=>{if(event.type==='pointer')barProbe.pointers++;const r=barProbe.input.call(layerApp.app,event);if(event.type==='pointer')(barProbe.log??=[]).push([event.kind,event.phase,layerApp.app.canvas_bar_hold(),r.handled,r.paint]);return r;};
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
        assert.equal(await evaluate(`(t=>t.matches(':popover-open')?t.textContent:null)(document.querySelector('#hover-tooltip'))`),
          'Select unlocked paint content or a layer mask',`${device}: a tap on a disabled item reveals its reason`);
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
      const during=await drag([at(-80,-50),at(-60,-40),at(-40,-30),at(-20,-20)],device,async()=>{
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
      const filters=['transform_nearest','transform_bilinear','transform_bicubic'],interpolation=`${bar} [data-toolbar-choice="transform-interpolation"] > button`;
      const chosen=()=>evaluate(`${JSON.stringify(filters)}.find(id=>layerApp.state().commands.find(c=>c.id===id).selected)`);
      if(index===0)assert.equal(await chosen(),'transform_bicubic','Distort resamples with Bicubic until a filter is chosen');
      const target=(await chosen())==='transform_nearest'?2:0;
      if(await evaluate(`document.querySelector('${interpolation}').closest('.canvas-action-bar-item').hidden`)){
        const row=label=>evaluate(`(()=>{const r=[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
        await tap(await middle(`${bar} .canvas-action-bar-more`),device);
        await wait(`!!document.querySelector('.panel-context-menu:popover-open')`);
        await tap(await row('Interpolation'),device);
        await wait(`[...document.querySelectorAll('.panel-context-menu:popover-open .menu-label')].some(n=>n.textContent==='Nearest')`);
        await tap(await row(['Nearest','Bilinear','Bicubic'][target]),device);
        await wait(`layerApp.state().commands.find(c=>c.id==='${filters[target]}').selected&&!document.querySelector('.panel-context-menu:popover-open')`);
      } else {
        await tap(await middle(interpolation),device);
        await wait(`!!document.querySelector('.toolbar-choice-menu')`);
        await tap(await middle(`.toolbar-choice-menu > button:nth-child(${target+1})`),device);
        await wait(`layerApp.state().commands.find(c=>c.id==='${filters[target]}').selected&&!document.querySelector('.toolbar-choice-menu')`);
      }
      assert.equal(await evaluate(`document.querySelector('${interpolation} .toolbar-choice-label').textContent`),
        await evaluate(`layerApp.state().canvas_bar.items.find(i=>i.option.Choice?.id==='transform-interpolation').option.Choice.items.find(i=>i.selected).label`),`${device}: the dropdown shows the chosen filter`);
      await tap(await middle(segment(3)),device);
      await wait(`layerApp.state().commands.find(c=>c.id==='transform_warp').selected&&!!document.querySelector('${bar} [data-toolbar-choice="transform-warp-grid"]')`);
      const hull=await anchor(),top={x:hull.x+(hull.right-hull.x)/3,y:hull.y},edge=(await state()).canvas_bar.anchor[1];
      await wait('layerApp.app.brush_ready()');
      touchId++;
      await pointer('mousePressed',top,device,1);
      for(const lift of [20,40]){await pointer('mouseMoved',{x:top.x,y:top.y-lift},device,1);await settle();}
      await pointer('mouseReleased',{x:top.x,y:top.y-40},device,1);await settle();
      await wait(visible);
      assert.ok((await state()).canvas_bar.anchor[1]<edge-10,`${device}: dragging a Warp edge node with one contact bends the edge`);
      await invoke('reset_transform');
      await wait(`layerApp.state().commands.find(c=>c.id==='transform_free').selected`);
      await tap(await middle(segment(0)),device);
      await wait(`!document.querySelector('${bar} [data-command="transform_perspective"]')&&layerApp.state().commands.find(c=>c.id==='transform_free').selected&&${visible}`);
      await tap(await middle(`${bar} .canvas-action-bar-more`),device);
      await wait(`!!document.querySelector('.panel-context-menu:popover-open')`);
      const labels=await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open .menu-label')].map(n=>n.textContent)`);
      assert.ok(labels.includes((await state()).commands.find(c=>c.id==='show_canvas_action_bar').label),`${device}: More offers the bar toggle: ${labels}`);
      const overflow=await evaluate(`(()=>{const items=layerApp.state().canvas_bar.items;return [...document.querySelectorAll('${bar} .canvas-action-bar-item')].flatMap((n,i)=>n.hidden?[items[i].menu?items[i].label:items[i].option.Action?.state.label??items[i].option.Choice?.label]:[])})()`);
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
        const menu=await evaluate(`[...document.querySelectorAll('${bar} .canvas-action-bar-item:not([hidden]) [data-canvas-bar-menu]')][0]?.dataset.canvasBarMenu`);
        assert.ok(menu,`A bar menu is shown on the tablet`);
        const menuButton=`${bar} [data-canvas-bar-menu="${menu}"]`,served=await evaluate(`layerApp.app.canvas_bar_choice_menu(layerApp.state().canvas_bar.context,'${menu}').sections.flat().map(i=>i.label)`);
        await osTap(menuButton,kind);
        await wait(`document.querySelector('${openMenu}')?.menuOwner===document.querySelector('${menuButton}')`);
        assert.deepEqual((await menuLabels()).filter(label=>served.includes(label)),served,`OS ${kind} taps open the ${menu} menu with its shared items`);
        await osTap(menuButton,kind);
        await wait(`!document.querySelector('${openMenu}')`);
        assert.equal(await evaluate('layerApp.state().canvas_bar?.context.kind??null'),'selection',`OS ${kind} taps close the ${menu} menu and keep the selection`);
        console.log(`OS ${kind} taps open and close the ${menu} menu`);
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
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
      await invoke('lasso');await settle();
      await drag([at(-140,-100),at(140,-100),at(140,90),at(-140,90),at(-140,-100)],'pen');
      await wait(`layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);
      const hard=(await state()).canvas_bar.anchor;
      await invoke('feather_selection');
      const slider='#selection-refine-value .number-slider';
      await wait(`!!document.querySelector('${slider}')&&layerApp.state().canvas_bar.anchor[0]<${hard[0]}`);
      const previewed=(await state()).canvas_bar.anchor,track=await rect(slider);
      const along=f=>physical({x:track.x+8+(track.width-16)*f,y:track.y+track.height/2});
      await shell('input','stylus','motionevent','DOWN',...along(.05));
      for(const f of [.15,.25,.35])await shell('input','stylus','motionevent','MOVE',...along(f));
      await wait(`layerApp.state().layer_tools.selection_resize?.radius>20&&layerApp.state().canvas_bar.anchor[0]<${previewed[0]}-.5`);
      await shell('input','stylus','motionevent','UP',...along(.35));
      await osTap('#selection-refine-panel .suggested-action','pen');
      await wait(`!document.querySelector('#selection-refine-panel')&&!layerApp.state().layer_tools.selection_resize`);
      await invoke('undo');
      await wait(`JSON.stringify(layerApp.state().canvas_bar?.anchor)===${JSON.stringify(JSON.stringify(hard))}`);
      await invoke('deselect');
      console.log('A real stylus drags the Refine value with a live preview and taps Apply; one undo restores the hard edge');
      for(const kind of ['pen','touch','mouse']) {
        if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
        await invoke('lasso');await settle();
        await drag([at(-140,-100),at(140,-100),at(140,90),at(-140,90),at(-140,-100)],'pen');
        await wait(`layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);
        const layer=await evaluate('String(layerApp.state().layers.find(l=>l.editing).id)'),pointers=await evaluate('barProbe.pointers');
        await evaluate('osInput.length=0');
        const mask=`${bar} [data-command="mask_selection"]`;
        if(await evaluate(shownItem(mask)))await osTap(mask,kind);
        else {
          const label=await evaluate(`layerApp.state().commands.find(c=>c.id==='mask_selection').label`);
          await osTap(`${bar} .canvas-action-bar-more`,kind);
          await wait(hasRow(label));
          await evaluate(`[...document.querySelectorAll('${openMenu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)}).dataset.osRow=''`);
          await osTap('[data-os-row]',kind);
          await evaluate(`document.querySelector('[data-os-row]')?.removeAttribute('data-os-row')`);
        }
        await wait(`layerApp.state().canvas_bar?.context.kind==='layer_mask' && ${visible}`);await settle();
        assert.ok(await atEdge(await rect(bar)),`OS ${kind}: the layer-mask bar sits on the bottom edge`);
        await osTap(`${bar} [data-command="layer_mask_enabled"]`,kind);
        await wait(`!layerApp.state().layers.find(l=>String(l.id)==='${layer}').mask_enabled&&document.querySelector('${bar} [data-command="layer_mask_enabled"] .toolbar-action-label')?.textContent==='Enable'`);
        await osTap(`${bar} [data-command="edit_layer_content"]`,kind);
        await wait(`!layerApp.state().canvas_bar&&!layerApp.state().layers.find(l=>String(l.id)==='${layer}').mask_selected`);
        const events=await evaluate('osInput');
        assert.ok(events.length&&events.every(e=>e.type===kind&&!e.canvas),`OS ${kind} taps reach the bars as ${kind}: ${JSON.stringify(events)}`);
        assert.equal(await evaluate('barProbe.pointers'),pointers,`OS ${kind} taps on the mode bar never reach the canvas`);
        await send({type:'layer',action:{op:'delete_mask',id:Number(layer)}});
        console.log(`OS ${kind} taps open the layer-mask bar with Mask, Disable the mask and leave with Edit Content`);
      }
    }
    const layerIds=()=>evaluate('layerApp.state().layers.map(l=>String(l.id))');
    const activeLayer=()=>evaluate('String(layerApp.state().layers.find(l=>l.editing).id)');
    const revision=id=>evaluate(`String(layerApp.state().layers.find(l=>String(l.id)==='${id}').paint_revision)`);
    const page=await evaluate('(({width,height})=>({width,height}))(layerApp.state().tabs.find(t=>t.active))');
    const thumbnail=(id,mask=false)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function check(){
      const layer=layerApp.state().layers.find(l=>String(l.id)==='${id}'),c=document.querySelectorAll('[data-layer="${id}"] .layer-thumbnail')[${mask?1:0}]?.querySelector('canvas');
      const current=layer&&String(layer.${mask?'mask_revision':'paint_revision'});
      if(c&&c.dataset.previewRevision?.split(':').at(-1)===current){const d=c.getContext('2d',{willReadFrequently:true}).getImageData(0,0,c.width,c.height).data;
        resolve({side:c.width,values:Array.from({length:d.length/4},(_,i)=>d[i*4+3]>128?d[i*4]:-1)});}
      else if(performance.now()>end)reject(Error('The thumbnail of layer ${id} did not update'));else setTimeout(check,60);}check();})`);
    const fit=image=>{const scale=image.side/Math.max(page.width,page.height);return{scale,x:(image.side-page.width*scale)/2,y:(image.side-page.height*scale)/2};};
    const painted=value=>value>=0&&value<100;
    const valueAt=(image,[x,y])=>{const f=fit(image);return image.values[Math.floor(f.y+y*f.scale)*image.side+Math.floor(f.x+x*f.scale)];};
    const frame=image=>{
      const found={x0:Infinity,y0:Infinity,x1:-1,y1:-1,count:0};
      image.values.forEach((value,i)=>{
        if(!painted(value))return;
        const x=i%image.side,y=Math.floor(i/image.side);
        Object.assign(found,{x0:Math.min(found.x0,x),y0:Math.min(found.y0,y),x1:Math.max(found.x1,x),y1:Math.max(found.y1,y),count:found.count+1});
      });
      return{width:found.x1-found.x0+1,height:found.y1-found.y0+1,count:found.count};
    };
    const holdsOnly=(image,box)=>{
      const found=frame(image),aspect=(box[2]-box[0])/(box[3]-box[1]);
      const [width,height]=aspect>=1?[image.side,image.side/aspect]:[image.side*aspect,image.side];
      return{...found,expected:[width,height],ok:found.count===found.width*found.height&&Math.abs(found.width-width)<=1.5&&Math.abs(found.height-height)<=1.5};
    };
    const middleOf=box=>[(box[0]+box[2])/2,(box[1]+box[3])/2],corner=[page.width*.02,page.height*.02];
    const selectRegion=async(device,origin=at)=>{
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
      await invoke('lasso');await settle();
      await drag([origin(-80,-140),origin(80,-140),origin(80,130),origin(-80,130),origin(-80,-140)],device==='touch'?'pen':device);
      await wait(`layerApp.state().canvas_bar?.context.kind==='selection' && ${visible}`);
      return (await state()).canvas_bar.anchor;
    };
    const tablet=device,viewport=width=>call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});
    for(const device of devices) {
      const wide=!tablet&&device==='mouse';
      let origin=at;
      if(wide) {
        await viewport(2560);await settle();await invoke('fit_canvas');await settle();
        const c=await camera();
        origin=(x,y)=>({x:c.r.x+(c.a[0]+c.a[2]/2)*c.r.width/c.v[0]+x,y:c.r.y+(c.a[1]+c.a[3]/2)*c.r.height/c.v[1]+y});
      }
      await invoke('select_all');await invoke('fill_selection');await invoke('deselect');await settle();
      const source=await activeLayer(),ids=await layerIds(),routes=[];
      let box=await selectRegion(device,origin);
      const context=(await state()).canvas_bar.context,shown=await rect(bar),window=await evaluate('innerWidth');
      assert.ok(shown.x>=0&&shown.right<=window,`${device}: the selection bar fits the window and overflows into More ${JSON.stringify({shown,window})}`);
      routes.push(await openBarMenu('copy_to_layer',device));
      assert.ok(await evaluate(hasRow('Cut Selection to New Layer')),`${device}: Copy to Layer offers Cut: ${await menuLabels()}`);
      await chooseRow('Copy Selection to New Layer',device);
      await wait(`layerApp.state().layers.length===${ids.length+1}&&!layerApp.state().layer_tools.has_selection`);
      const copy=await activeLayer();
      assert.ok(!ids.includes(copy),`${device}: the copy is a new, active layer`);
      const whole=[0,0,page.width,page.height],copied=holdsOnly(await thumbnail(copy),box);
      assert.ok(copied.ok,`${device}: the new layer holds only the selected pixels ${JSON.stringify({copied,box})}`);
      assert.ok(holdsOnly(await thumbnail(source),whole).ok,`${device}: copying keeps the source pixels`);
      assert.equal(await evaluate(`layerApp.app.canvas_bar_choice_menu(${JSON.stringify(context)},'copy_to_layer')??null`),null,`${device}: the consumed selection's menu is stale`);
      await invoke('undo');
      await wait(`layerApp.state().layers.length===${ids.length}`);
      assert.deepEqual(await layerIds(),ids,`${device}: one undo step removes the copy`);
      assert.ok(holdsOnly(await thumbnail(source),whole).ok,`${device}: the undo keeps the painted layer`);

      box=await selectRegion(device,origin);
      const before=await revision(source);
      routes.push(await openBarMenu('clear',device));
      assert.ok(await evaluate(hasRow('Clear Selected Pixels')),`${device}: Clear offers Clear Selected Pixels`);
      await chooseRow('Clear Outside Selection',device);
      await wait(`String(layerApp.state().layers.find(l=>String(l.id)==='${source}').paint_revision)!=='${before}'`);
      const kept=holdsOnly(await thumbnail(source),box);
      assert.ok(kept.ok,`${device}: Clear Outside keeps only the selected pixels ${JSON.stringify({kept,box})}`);
      await invoke('undo');
      assert.ok(holdsOnly(await thumbnail(source),whole).ok,`${device}: one undo step restores the cleared pixels`);
      assert.deepEqual(await layerIds(),ids);

      box=await selectRegion(device,origin);
      routes.push(await openBarMenu('adjust',device));
      await chooseRow('Tone',device);
      await chooseRow('Curves',device);
      await wait(`layerApp.state().layers.length===${ids.length+1}&&!layerApp.state().layer_tools.has_selection`);
      const effect=await evaluate('JSON.parse(JSON.stringify(layerApp.state().layers.find(l=>l.editing),(_,v)=>typeof v==="bigint"?String(v):v))');
      assert.equal(effect.label,'Curves',`${device}: Adjust › Tone › Curves adds a Curves layer`);
      assert.ok(effect.has_mask,`${device}: the Curves layer is masked`);
      const mask=await thumbnail(effect.id,true);
      assert.ok(valueAt(mask,middleOf(box))>200&&valueAt(mask,corner)>=0&&valueAt(mask,corner)<50,`${device}: the mask reveals the selection only ${JSON.stringify({inside:valueAt(mask,middleOf(box)),outside:valueAt(mask,corner)})}`);
      await invoke('undo');
      await wait(`layerApp.state().layers.length===${ids.length}&&layerApp.state().layer_tools.has_selection`);
      assert.deepEqual(await layerIds(),ids,`${device}: one undo step removes the Curves layer and restores the selection`);
      await invoke('deselect');

      const refine='#selection-refine-panel',field=`${refine} #selection-refine-value`;
      const selectionBox=async()=>(await state()).canvas_bar.anchor;
      const boxIs=box=>`JSON.stringify(layerApp.state().canvas_bar?.anchor)===${JSON.stringify(JSON.stringify(box))}`;
      const beyond=box=>`(a=>!!a&&a[0]<${box[0]}-.5&&a[1]<${box[1]}-.5&&a[2]>${box[2]}+.5&&a[3]>${box[3]}+.5)(layerApp.state().canvas_bar?.anchor)`;
      await selectRegion(device,origin);
      const hard=await selectionBox(),contacts=await evaluate('barProbe.pointers');
      routes.push(await openBarMenu('refine',device));
      for(const label of ['Grow…','Shrink…','Feather…','Border…','Smooth…','Transform Outline'])
        assert.ok(await evaluate(hasRow(label)),`${device}: Refine offers ${label}: ${await menuLabels()}`);
      await chooseRow('Feather…',device);
      await wait(`!!document.querySelector('${field}')&&layerApp.state().layer_tools.selection_resize?.kind==='feather'`);
      assert.equal(await evaluate(`document.querySelector('${refine} h2').textContent`),'Feather Selection');
      assert.equal(await evaluate(`document.querySelector('${field} .number-title').textContent`),'Feather radius',`${device}: the panel names the value`);
      assert.ok(await evaluate(`!document.querySelector('dialog[open], :modal')`),`${device}: the panel is not modal, so nothing dims the canvas`);
      assert.ok(await evaluate('document.hasFocus()'),`${device}: the panel keeps window focus`);
      const panel=await rect(refine),outlined=await anchor();
      assert.ok(panel.y>=outlined.bottom||panel.bottom<=outlined.y||panel.x>=outlined.right||panel.right<=outlined.x,
        `${device}: the panel leaves the selection visible ${JSON.stringify({panel,outlined})}`);
      await wait(beyond(hard));
      const previewed=await selectionBox(),track=await rect(`${field} .number-slider`);
      const along=f=>({x:track.x+8+(track.width-16)*f,y:track.y+track.height/2});
      touchId++;
      await pointer('mousePressed',along(.05),device);
      for(const f of [.15,.25,.35]){await pointer('mouseMoved',along(f),device);await settle();}
      await wait(`layerApp.state().layer_tools.selection_resize.radius>20&&${beyond(previewed)}`);
      await pointer('mouseReleased',along(.35),device);await settle();
      assert.equal(await evaluate('barProbe.pointers'),contacts,`${device}: panel contacts never reach the canvas`);
      if(device==='pen'&&!tablet)for(const name of ['light','dark']){
        await send({type:'set_theme',theme:name});await settle();await pause(200);
        assert.ok(await evaluate(`!!document.querySelector('${field}')`),`the Refine panel stays open in the ${name} theme`);
        const shot=await call('Page.captureScreenshot',{format:'png'});
        await writeFile(`${directory}/refine-feather-${name}.png`,Buffer.from(shot.data,'base64'));
      }
      await tap(await middle(`${refine} .suggested-action`),device);
      await wait(`!document.querySelector('${refine}')&&!layerApp.state().layer_tools.selection_resize`);
      const feathered=await selectionBox();
      await invoke('undo');
      await wait(boxIs(hard));
      await invoke('redo');
      await wait(boxIs(feathered));
      await invoke('deselect');

      await selectRegion(device,origin);
      const outline=await selectionBox(),untouched=await revision(source);
      routes.push(await openBarMenu('refine',device));
      await chooseRow('Transform Outline',device);
      await wait(`layerApp.state().canvas_bar?.context.kind==='transform'&&layerApp.state().canvas_bar.label==='Transform Outline'&&${visible}`);
      assert.equal(await evaluate(`!!document.querySelector('${bar} [data-toolbar-choice="transform-interpolation"]')`),false,`${device}: an outline has no interpolation`);
      const hull=await anchor(),handle={x:hull.right,y:(hull.y+hull.bottom)/2};
      await wait('layerApp.app.brush_ready()');
      touchId++;
      await pointer('mousePressed',handle,device);
      for(const dx of [20,50,80]){await pointer('mouseMoved',{x:handle.x+dx,y:handle.y},device);await settle();}
      await pointer('mouseReleased',{x:handle.x+80,y:handle.y},device);await settle();
      await wait(visible);
      const stretched=await anchor();
      assert.ok(Math.abs(stretched.right-hull.right-80)<3&&Math.abs(stretched.x-hull.x)<1,`${device}: dragging the edge handle stretches the outline ${JSON.stringify({hull,stretched})}`);
      await press('apply_transform',device);
      await wait(`layerApp.state().layer_tools.tool!=='transform'&&layerApp.state().canvas_bar?.context.kind==='selection'`);
      const applied=await selectionBox();
      assert.ok(applied[2]>outline[2]+10&&Math.abs(applied[0]-outline[0])<1,`${device}: Apply moves the outline ${JSON.stringify({outline,applied})}`);
      assert.equal(await revision(source),untouched,`${device}: the pixels stay where they are`);
      assert.ok(holdsOnly(await thumbnail(source),[0,0,page.width,page.height]).ok,`${device}: the layer keeps every pixel`);
      await invoke('undo');
      await wait(boxIs(outline));
      await invoke('deselect');
      if(wide) {
        assert.deepEqual(routes,['bar','bar','bar','bar','bar'],'A wide work area shows every menu on the bar');
        await viewport(1440);await settle();await invoke('fit_canvas');await settle();
      }
      console.log(`${device}: bar menus opened from ${routes.join(', ')}`);
    }
    const key=async name=>{
      const code={Delete:46,Backspace:8,Escape:27}[name];
      for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,code:name,windowsVirtualKeyCode:code,nativeVirtualKeyCode:code});
      await settle();
    };
    const source=await activeLayer();
    const box=await selectRegion('pen');
    const before=await revision(source);
    await key('Delete');
    await wait(`String(layerApp.state().layers.find(l=>String(l.id)==='${source}').paint_revision)!=='${before}'`);
    const cleared=await thumbnail(source);
    assert.ok(!painted(valueAt(cleared,middleOf(box)))&&painted(valueAt(cleared,corner)),'Delete clears the selected pixels only');
    await invoke('undo');
    assert.ok(holdsOnly(await thumbnail(source),[0,0,page.width,page.height]).ok,'One undo step restores the pixels Delete cleared');
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
      assert.ok(s.moves.glass<=most('glass')+1&&s.moves.layouts<=most('layouts')+1,`The hidden bar adds no glass or layout work to stroke samples ${JSON.stringify(strokes)}`);
    }
    console.log('Stroke metrics',JSON.stringify(strokes));
    await invoke('deselect');await settle();
    assert.equal(await evaluate(visible),false,'Deselect removes the bar');
    const drawings=()=>evaluate('layerApp.app.document_tabs(0).tabs.map(t=>String(t.id))');
    const ready=()=>wait('!layerApp.documents.busy()&&layerApp.app.brush_ready()&&!layerApp.state().document_file.busy&&layerApp.app.document_park_ready()');
    const [first]=await drawings();
    await invoke('new_document');
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Create')`);
    await evaluate(`(()=>{const dialog=document.querySelector('dialog[open]');for(const n of dialog.querySelectorAll('input[type=number]'))n.value=96;[...dialog.querySelectorAll('button')].find(b=>b.textContent==='Create').click();})()`);
    await wait('layerApp.app.document_tabs(0).tabs.length===2');await ready();
    const second=(await drawings()).find(id=>id!==first);
    await evaluate(`layerApp.documents.select(BigInt(${first}))`);await ready();
    await invoke('select_all');
    await wait(`layerApp.state().commands.find(c=>c.id==='clear_selected').enabled`);
    const kept=await revision(source);
    await evaluate(`document.querySelector('.drawing-tab[data-drawing-id="${second}"] .drawing-tab-pick').focus()`);
    await key('Delete');
    await wait('layerApp.app.document_tabs(0).tabs.length===1');await ready();
    assert.deepEqual(await drawings(),[first],'Delete on a focused drawing tab closes that drawing');
    assert.equal(await revision(source),kept,'Delete on a focused drawing tab clears no pixels in the active drawing');
    assert.ok(await evaluate('layerApp.state().layer_tools.has_selection'),'Delete on a focused drawing tab keeps the selection');
    await invoke('deselect');await settle();

    const barLabel=()=>evaluate(`(n=>n&&!n.hidden?n.textContent:null)(document.querySelector('${bar} .canvas-action-bar-label'))`);
    const itemLabel=command=>`document.querySelector('${bar} [data-command="${command}"] .toolbar-action-label')?.textContent`;
    const layer=id=>`layerApp.state().layers.find(l=>String(l.id)==='${id}')`;
    const modeBar=async(name,text,device)=>{
      await wait(`layerApp.state().canvas_bar?.context.kind==='${name}' && ${visible}`);await settle();
      assert.equal(await barLabel(),text,`${device}: the ${name} bar reads "${text}"`);
      const view=(await state()).canvas_bar,exit=view.completion[0].option.Action.state.id;
      assert.equal(view.placement,'bottom_edge',`${device}: the ${name} bar uses the bottom edge`);
      assert.ok(await atEdge(await rect(bar)),`${device}: the ${name} bar sits on the bottom edge ${JSON.stringify(await rect(bar))}`);
      assert.ok(await evaluate(`document.querySelector('${bar} [data-command="${exit}"]').classList.contains('suggested-action')`),`${device}: the ${name} exit uses the accent`);
    };
    const paint=await activeLayer(),paintName=await evaluate(`${layer(paint)}.label`);
    const saveSelectionLayer=async()=>{
      await invoke('select_all');await invoke('save_selection_layer');
      await send({type:'layer',action:{op:'cancel_rename'}});
      const saved=await activeLayer();
      assert.ok(await evaluate(`${layer(saved)}.selection_layer`),'Save as Selection Layer edits the new Selection Layer');
      return{saved,name:await evaluate(`${layer(saved)}.label`)};
    };
    const artwork=`String(layerApp.state().layers.find(l=>l.editing)?.id)==='${paint}'&&!${layer(paint)}.mask_selected&&!layerApp.state().layer_tools.quick_mask&&!layerApp.state().layer_tools.mask_editing`;
    for(const device of devices) {
      await invoke('select_all');await invoke('quick_mask');
      await modeBar('quick_mask','Quick Mask',device);
      const coverage=await evaluate('String(layerApp.state().layers.find(l=>l.quick_mask).paint_revision)');
      await press('invert_selection',device);
      await wait(`String(layerApp.state().layers.find(l=>l.quick_mask)?.paint_revision)!=='${coverage}'`);
      assert.equal(await kind(),'quick_mask',`${device}: Invert stays in Quick Mask`);
      await press('return_to_artwork',device);
      await wait(`${artwork}&&layerApp.state().canvas_bar?.context.kind!=='quick_mask'`);

      const {saved,name}=await saveSelectionLayer();
      await modeBar('selection_layer',`Editing ${name}`,device);
      const stored=await revision(saved);
      await press('invert_selection_layer',device);
      await wait(`String(${layer(saved)}.paint_revision)!=='${stored}'`);
      assert.equal(await activeLayer(),saved,`${device}: Invert stays on the Selection Layer`);
      assert.equal(await kind(),'selection_layer',`${device}: Invert keeps the Selection Layer bar`);
      await press('return_to_artwork',device);
      await wait(`${artwork}&&layerApp.state().canvas_bar?.context.kind!=='selection_layer'`);
      await send({type:'layer',action:{op:'delete',id:Number(saved)}});

      await invoke('select_all');await invoke('mask_selection');
      await modeBar('layer_mask',`Editing ${paintName} mask`,device);
      assert.equal(await evaluate(itemLabel('layer_mask_enabled')),'Disable',`${device}: an enabled mask offers Disable`);
      await press('layer_mask_enabled',device);
      await wait(`!${layer(paint)}.mask_enabled&&${itemLabel('layer_mask_enabled')}==='Enable'`);
      assert.equal(await kind(),'layer_mask',`${device}: Disable keeps mask editing`);
      await press('edit_layer_content',device);
      await wait(`${artwork}&&!layerApp.state().canvas_bar`);
      await send({type:'layer',action:{op:'delete_mask',id:Number(paint)}});
      console.log(`${device}: Quick Mask, Selection Layer and layer-mask bars left from their exits`);
    }

    await evaluate(`window.escapeProbe=[];window.addEventListener('keydown',escapeProbe.listener=e=>{if(e.key==='Escape')escapeProbe.push(e.defaultPrevented)})`);
    const escaped=async name=>{
      assert.ok(await evaluate('!document.activeElement?.matches("input,select,textarea,[contenteditable=true]")'),`${name}: no text field holds the focus`);
      await key('Escape');
      await wait(`${artwork}&&layerApp.state().canvas_bar?.context.kind!=='${name}'`,10000);
      assert.equal(await evaluate('escapeProbe.pop()'),true,`${name}: the session handles Escape`);
    };
    await invoke('select_all');await invoke('mask_selection');
    await modeBar('layer_mask',`Editing ${paintName} mask`,'keyboard');
    for(const column of await evaluate('layerApp.app.layout(innerWidth,innerHeight).collapsed.filter(c=>c.open).map(c=>c.open.column)'))
      await send({type:'customize',action:{type:'close_column',column}});
    await escaped('layer_mask');
    await send({type:'layer',action:{op:'delete_mask',id:Number(paint)}});
    const {saved}=await saveSelectionLayer();
    await modeBar('selection_layer',`Editing ${await evaluate(`${layer(saved)}.label`)}`,'keyboard');
    await escaped('selection_layer');
    await send({type:'layer',action:{op:'delete',id:Number(saved)}});
    await invoke('select_all');await invoke('quick_mask');
    await modeBar('quick_mask','Quick Mask','keyboard');
    await escaped('quick_mask');
    await evaluate(`window.removeEventListener('keydown',escapeProbe.listener);delete window.escapeProbe`);

    await invoke('select_all');await invoke('mask_selection');
    await modeBar('layer_mask',`Editing ${paintName} mask`,'notice');
    await invoke('move');
    await send({type:'layer',action:{op:'lock',id:Number(paint),value:true}});
    await wait('layerApp.app.brush_ready()');
    await tap(center,'pen');
    await wait(`(n=>!!n&&!n.hidden)(document.querySelector('.canvas-notice'))&&layerApp.state().notice?.text==='The active layer is locked'`,10000);
    await wait(visible,10000);await settle();
    const edge=await rect(bar),bubble=await rect('.canvas-notice');
    assert.equal(await kind(),'layer_mask','The mode bar stays through the refused contact');
    assert.ok(bubble.bottom<=edge.y-4,`The notice sits above the bottom-edge mode bar ${JSON.stringify({bubble,edge})}`);
    await screenshot('mask-mode-notice');
    await send({type:'layer',action:{op:'lock',id:Number(paint),value:false}});
    await invoke('edit_layer_content');
    await send({type:'layer',action:{op:'delete_mask',id:Number(paint)}});
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');

    await invoke('ruler');
    for(const device of devices) {
      const from=at(-160,-150),to=at(40,-110);
      await drag([from,at(-60,-130),to],device==='touch'?'pen':device);
      await wait(`layerApp.state().canvas_bar?.context.kind==='guide' && ${visible}`);await settle();
      const guide=await anchor(),box=await rect(bar);
      assert.equal(await barLabel(),null,`${device}: the guide bar has no label`);
      assert.ok(await beside(box,guide),`${device}: the guide bar sits below the guide's handles ${JSON.stringify({guide,box})}`);
      assert.ok(box.y>Math.max(from.y,to.y)+12,`${device}: the guide bar clears the lower handle`);
      await press('delete_ruler',device);
      await wait(`layerApp.state().canvas_bar?.context.kind!=='guide'&&!layerApp.state().commands.find(c=>c.id==='delete_ruler').enabled`);
      console.log(`${device}: the guide bar deletes the selected guide`);
    }
    await invoke('lasso');
    console.log(`PASS canvas action bar (${device?'tablet':'desktop'}): selection bar beside new selections, Transform, taps never paint, hide during drags, More, Apply/Cancel, completion-only, Zen, glass, Copy to Layer, Clear Outside and Adjust › Curves from bar menus, Refine ▾ › Feather with a live preview undone in one step, Transform Outline moving only the outline, Delete clearing a selection but not from a focused drawing tab, Quick Mask, Selection Layer and layer-mask bars with their exits and Escape, a notice above a mode bar, the guide bar's Delete, and screenshots in ${directory}`);
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
