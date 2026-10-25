import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkLayerTouchGestures({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+25000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,20)}check()})`);
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const mouse=async(type,point,device='mouse')=>call('Input.dispatchMouseEvent',{type,...point,pointerType:device,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,force:device==='pen'?.8:0});
  const drag=async(points,device='mouse')=>{await mouse('mousePressed',points[0],device);for(const point of points.slice(1)){await pause(25);await mouse('mouseMoved',point,device);}await mouse('mouseReleased',points.at(-1),device);await settle();};
  const key=async(key,code,windowsVirtualKeyCode)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode});await settle();};
  const bounds=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Missing visible '+${JSON.stringify(selector)});return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  let id=2000;
  const touch=(type,points=[])=>call('Input.dispatchTouchEvent',{type,touchPoints:points,timestamp:Date.now()/1000});
  const contacts=async(selector,count)=>{const r=await bounds(selector);return Array.from({length:count},(_,i)=>({id:++id,x:r.x+r.width*.44+i*34,y:r.y+r.height/2}));};
  const gesture=async(points,{dx=0,hold=60,cancel=false}={})=>{
    await touch('touchStart',points);await pause(hold);
    if(dx)for(let step=1;step<=4;step++){await touch('touchMove',points.map(p=>({...p,x:p.x+dx*step/4})));await pause(18);}
    await touch(cancel?'touchCancel':'touchEnd');await settle();await pause(180);
  };
  const saved=await evaluate('layerApp.app.workspace_persistence()');
  const fixture=structuredClone(saved);fixture.layout.bubbles={bubbles:[],rings:[],kept:[]};
  fixture.layout.header={size:'medium',zones:[[{id:1,item:{kind:'capy'}}],[{id:2,item:{kind:'tool',control:{kind:'panel',panel:'layers'}}}],[]],next_id:3};
  const output=process.env.LAYER_TEST_ARTIFACTS??'artifacts/layer-touch-gestures/web';await mkdir(output,{recursive:true});
  try {
    for(const theme of ['light','dark'])for(const mode of ['full','simple']) {
      await send({type:'restore_workspace',workspace:fixture});
      await send({type:'set_theme',theme});
      await invoke('new_document');await evaluate(createDrawing({width:640,height:480}));
      await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()');
      let root='.layers-panel',bubble;
      if(mode==='full'){
        await send({type:'customize',action:{type:'set_panel_visible',panel:'layers',visible:true}});
        await send({type:'move_panel',panel:'layers',target:{kind:'float',position:[1080,260]}});
        await pause(350);
      }
      if(mode==='simple') {
        await pause(300);
        assert.ok(await evaluate("layerApp.app.bubble_grab({type:'grab',grab:{kind:'header',id:2},held:false})"),'Layers header tool can tear out');
        const r=await bounds('[data-header-item="2"] .header-tool'),from={x:r.x+r.width/2,y:r.y+r.height/2};
        await drag([from,{x:from.x,y:from.y+10},{x:from.x,y:from.y+80},{x:1060,y:370},{x:1060,y:400}]);
        await wait("layerApp.app.bubbles().some(p=>p.view.kind==='layers'&&p.view.card&&!p.view.card.full)");
        bubble=await evaluate("Number(layerApp.app.bubbles().find(p=>p.view.kind==='layers').view.id)");
        root=`.tool-bubble[data-bubble="${bubble}"] .layers-panel`;
        await pause(450);
      }
      await invoke('reset_view');await invoke('pen');await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:32});
      const center=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
      const paint=async(color,vertical,offset)=>{
        await send({type:'set_color',rgba:color});
        const revision=await evaluate('String(layerApp.state().layers.find(l=>l.editing).paint_revision)');
        const a=vertical?{x:center.x+offset,y:center.y-65}:{x:center.x-65,y:center.y+offset};
        const b=vertical?{x:center.x+offset,y:center.y+65}:{x:center.x+65,y:center.y+offset};
        await drag(Array.from({length:9},(_,i)=>({x:a.x+(b.x-a.x)*i/8,y:a.y+(b.y-a.y)*i/8})),'pen');
        await wait(`String(layerApp.state().layers.find(l=>l.editing).paint_revision)!==${JSON.stringify(revision)}&&!layerApp.app.canvas_work_pending()`);
      };
      const bottom=(await state()).layers.find(l=>l.editing).id;
      await paint([.1,.2,.9,1],false,0);
      await invoke('add_layer');await paint([.9,.7,.1,1],false,40);
      await invoke('add_layer');const top=(await state()).layers.find(l=>l.editing).id;
      await paint([.9,.1,.2,1],true,0);
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:10,y:10,buttons:0,pointerType:'mouse'});await settle();
      if(mode==='simple'){const r=await bounds(`.tool-bubble[data-bubble="${bubble}"] .bubble-face`),p={x:r.x+r.width/2,y:r.y+r.height/2};await mouse('mousePressed',p);await mouse('mouseReleased',p);await settle();await pause(400);}
      const row=layer=>`${root} .layer-row[data-layer="${layer}"]`;
      const layer=async layer=>(await state()).layers.find(l=>l.id===layer);
      let sampleIndex=0;
      const sample=async()=>{
        await wait('!layerApp.app.canvas_work_pending()');await settle();
        const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:Math.round(center.x)-90,y:Math.round(center.y)-90,width:180,height:180,scale:1}});
        await writeFile(`${output}/${theme}-${mode}-sample-${sampleIndex++}.png`,Buffer.from(shot.data,'base64'));
        return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const c=document.createElement('canvas');c.width=c.height=180;const x=c.getContext('2d');x.drawImage(image,0,0);const p=x.getImageData(0,0,180,180).data;return{pixels:Array.from(p),center:Array.from(p.slice((90*180+90)*4,(90*180+90)*4+4)),outside:Array.from(p.slice((90*180+40)*4,(90*180+40)*4+4))}})()`);
      };
      const samePixels=(actual,expected,label)=>{let maximum=0;for(let i=0;i<expected.pixels.length;i++)maximum=Math.max(maximum,Math.abs(actual.pixels[i]-expected.pixels[i]));assert.ok(maximum<=2,`${label}: maximum presented channel difference ${maximum}/255`);};
      const label=`${theme}/${mode}`;
      assert.equal(await evaluate(`!!document.querySelector(${JSON.stringify(row(top))})`),true);
      if(mode==='simple')assert.equal(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(row(top)+' > .layer-icon:nth-child(2)')})).display`),'none',`${label}: selection checkbox is hidden`);
      await gesture(await contacts(row(top),2));
      await wait(`Number(layerApp.state().layer_tools.opacity_popup)===${top}`);
      assert.equal(await evaluate("document.querySelectorAll('.layer-opacity-popup:popover-open').length"),1,`${label}: only originating list shows opacity`);
      await evaluate("(()=>{const n=document.querySelector('.layer-opacity-popup:popover-open input.number-slider');n.value='.4';n.dispatchEvent(new Event('input',{bubbles:true}));n.dispatchEvent(new Event('change',{bubbles:true}));})()");await settle();
      assert.ok(Math.abs((await layer(top)).opacity-.4)<.01,`${label}: existing opacity popup edits target`);
      await key('Escape','Escape',27);await invoke('undo');assert.equal((await layer(top)).opacity,1,'one Undo restores opacity');
      const before=await sample();
      await gesture(await contacts(row(top),1),{dx:65});
      assert.equal((await layer(top)).alpha_locked,true,`${label}: one finger right locks alpha`);
      const selected=(await layer(top)).selected;
      await gesture(await contacts(row(top),2),{dx:65});
      assert.equal((await layer(top)).selected,!selected,`${label}: two fingers right toggle selection`);
      assert.equal((await layer(top)).alpha_locked,true,'two-finger swipe preserves Alpha Lock');
      await gesture(await contacts(row(top),2),{dx:65});
      assert.equal((await layer(top)).selected,selected,'second two-finger swipe restores selection');
      await gesture(await contacts(row(top),1),{dx:65});assert.equal((await layer(top)).alpha_locked,false);
      await gesture(await contacts(row(top),2),{hold:700});
      await wait('layerApp.state().layer_tools.has_selection');
      await send({type:'set_color',rgba:[.1,.9,.2,1]});await invoke('fill_selection');
      const filled=await sample();assert.ok(filled.center[1]>filled.center[0]+60,`${label}: hold loads authored content coverage`);
      assert.deepEqual(filled.outside,before.outside,'coverage excludes the lower layer outside the held thumbnail');
      await invoke('undo');await invoke('deselect');samePixels(await sample(),before,'Undo restores pixels after coverage fill');
      const order=(await state()).layers.map(l=>l.id),initial=await layer(top);
      for(const options of [{},{dx:65}]) {
        await gesture(await contacts(row(top),2),{...options,cancel:true});await pause(650);
        assert.equal((await state()).layer_tools.opacity_popup??null,null,'cancelled tap cannot open opacity');
        assert.equal((await layer(top)).selected,initial.selected,'cancelled swipe cannot toggle row selection');
        assert.equal((await state()).layer_tools.has_selection,false,'cancelled hold cannot load coverage');
      }
      const a=(await contacts(row(top),1))[0],b=(await contacts(row(bottom),1))[0];
      assert.ok(Math.abs(b.y-a.y)>60,'pinch covers three visible rows');
      const pinch=async cancel=>{
        await touch('touchStart',[a,b]);
        for(let step=1;step<=4;step++){await pause(25);await touch('touchMove',[a,b].map(p=>({...p,y:p.y+((a.y+b.y)/2-p.y)*step/4*.8})));}
        await touch(cancel?'touchCancel':'touchEnd');await settle();
      };
      await pinch(true);await pause(350);
      assert.deepEqual((await state()).layers.map(l=>l.id),order,'cancelled pinch cannot merge layers');
      await pinch(false);
      await wait(`layerApp.state().layers.length===${order.length-2}&&!layerApp.app.canvas_work_pending()`);
      samePixels(await sample(),before,`${label}: inclusive three-row pinch merge preserves pixels`);
      await invoke('undo');await wait(`layerApp.state().layers.length===${order.length}`);
      assert.deepEqual((await state()).layers.map(l=>l.id),order,'one Undo restores merged range and order');
      samePixels(await sample(),before,'Undo restores merged pixels');
      console.log(`PASS: ${label} opacity/edit/Undo, coverage, one/two-finger swipes, cancellation, inclusive merge pixels/Undo`);
    }
  }catch(error){await writeFile(`${output}/failure-dom.json`,JSON.stringify(await evaluate(`({surfaceType:typeof layerApp.state().layer_tools.opacity_surface,popups:[...document.querySelectorAll('.layer-opacity-popup')].map(n=>({connected:n.isConnected,open:n.matches(':popover-open'),parent:n.parentElement?.className,html:n.outerHTML}))})`),null,2));await writeFile(`${output}/failure-state.json`,JSON.stringify(await state(),null,2));const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${output}/failure.png`,Buffer.from(shot.data,'base64'));throw error;}
  finally{await send({type:'restore_workspace',workspace:saved});}
}
