import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {placementSave,sourceIdentity} from './image-placement-motion.test.mjs';
import {png} from './clone-journey.test.mjs';

export async function checkPointwiseEffects({call,evaluate,settle,motion=true,widths=[640,1100],effects=['invert','threshold','desaturate','photo_filter'],colorPages=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/photo-editing-color/p21-web';
  await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-900)));else setTimeout(poll,40)}poll()})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await send({type:'invoke',command});};
  const properties=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v==="bigint"?Number(v):v))');
  const value=async key=>(await properties()).controls.find(c=>c.key===key)?.value;
  const selector=key=>`[data-property-key="${key}"]`;
  const click=async selector=>{
    const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await pointer('mousePressed',p);await pointer('mouseReleased',p);await settle();
  };
  const pointer=(type,p)=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  const key=async(name,code)=>{const physical=name===' '?'Space':name;await call('Input.dispatchKeyEvent',{type:'keyDown',key:name,code:physical,windowsVirtualKeyCode:code});await call('Input.dispatchKeyEvent',{type:'keyUp',key:name,code:physical,windowsVirtualKeyCode:code});await settle();};
  const page=async id=>{await evaluate(`(()=>{const n=document.querySelector('[data-properties-page]');n.value=${JSON.stringify(id)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);await settle();assert.equal((await properties()).page,id);};
  const edit=async(key,text)=>{
    const input=`${selector(key)} .number-entry`;
    if(await evaluate(`document.querySelector(${JSON.stringify(input)}).hidden`))await click(`${selector(key)} .number-value`);else await click(input);
    await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(input)});n.value=${JSON.stringify(text)};n.dispatchEvent(new Event('input',{bubbles:true}))})()`);await keyPress();
    assert.ok(Math.abs((await value(key)).value-Number(text))<1e-6);
  };
  const keyPress=()=>key('Enter',13);
  const capture=async name=>{await evaluate('layerApp.app.wait_for_canvas()');await settle();const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));};
  const canvasPixel=async()=>{
    const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(64*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(192*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{...point,width:1,height:1,scale:1}});
    const pixel=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const canvas=document.createElement('canvas');canvas.width=1;canvas.height=1;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return Array.from(context.getImageData(0,0,1,1).data)})()`);
    return pixel;
  };
  await wait('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
  await evaluate(`window.pointwiseFiles={open:window.showOpenFilePicker,save:window.showSaveFilePicker};window.placementTest={};
    window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(v){placementTest.saved=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){},async abort(){}}}});
    window.showOpenFilePicker=async()=>[{name:'pointwise.capy',async getFile(){return new File([placementTest.saved],'pointwise.capy')}}];`);
  const idle=()=>wait('!layerApp.state().document_file.busy&&!layerApp.documents.busy()&&layerApp.app.brush_ready()');
  const save=placementSave({evaluate,invoke,idle});
  const rasterIdentity=m=>m.rasters.map(r=>({...r,tiles:r.tiles.map(t=>({...t,blob:m.blobs[t.blob].digest}))}));
  const reports=[];
  try {
    const original=png(256,256,(x,y)=>[x,y,255-x,255],4);
    await writeFile(`${directory}/original.png`,original);
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'pointwise.png',getFile:async()=>new File([new Uint8Array(${JSON.stringify(Array.from(original))})],'pointwise.png',{type:'image/png'})}];layerApp.dispatch({type:'invoke',command:'open_document'});`);
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('!layerApp.documents.busy()&&layerApp.state().tabs.some(t=>t.width===256&&t.height===256)&&layerApp.app.brush_ready()');
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'pointwise.capy',async getFile(){return new File([placementTest.saved],'pointwise.capy')}}];`);
    const baseline=await save(),sources=sourceIdentity(baseline),rasters=rasterIdentity(baseline);
    const unchanged=async()=>{
      const current=await save(),actual=rasterIdentity(current);
      assert.deepEqual(sourceIdentity(current),sources);
      assert.deepEqual(actual.filter(r=>rasters.some(original=>original.target===r.target)),rasters);
      for(const added of actual.filter(r=>!rasters.some(original=>original.target===r.target)))assert.deepEqual(added,{target:added.target,tiles:[],watercolor:null});
      return current;
    };
    const reopen=async name=>{
      const expected=await unchanged();
      await writeFile(`${directory}/${name}.capy`,Buffer.from(await evaluate('Array.from(placementTest.saved)')));
      await invoke('open_document');await idle();
      const actual=await unchanged();assert.deepEqual(actual.document.layers,expected.document.layers,'Native archive retains exact effect values and sources');
    };
    if(colorPages) {
      const samples=[];
      const nativeChoice=async(selector,index)=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);await key('Home',36);for(let i=0;i<index;i++)await key('ArrowDown',40);assert.equal(await evaluate(`Number(document.querySelector(${JSON.stringify(selector)}).value)`),index);};
      const layers=async()=>(await save()).document.layers;
      for(const width of widths) {
        await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
        await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();await invoke('fit_canvas');
        for(const theme of ['light','dark']) {
          await send({type:'set_theme',theme});const original=await canvasPixel();assert.ok(original[1]>original[0]+60&&original[3]===255);
          await send({type:'effect',action:{op:'insert',effect:'selective_color'}});assert.deepEqual(await canvasPixel(),original);
          const pages=['reds','yellows','greens','cyans','blues','magentas','whites','neutrals','blacks'];assert.deepEqual((await properties()).pages.map(p=>p.id),pages);
          let owner=(await properties()).layer,before=await layers();assert.equal(before.find(l=>l.id===owner)?.effect.values.length,37);
          for(const id of pages)await page(id);assert.deepEqual(await layers(),before);
          const inks=['cyan','magenta','yellow','black'];
          for(let i=0;i<pages.length;i++){await page(pages[i]);for(const [j,text] of [String((i+1)*2),'-1.25','1.5','0.5'].entries())await edit(`${pages[i]}_${inks[j]}`,text);}
          for(let i=0;i<pages.length;i++){await page(pages[i]);for(const [j,n] of [(i+1)*2,-1.25,1.5,.5].entries())assert.ok(Math.abs((await value(`${pages[i]}_${inks[j]}`)).value-n)<1e-6);}
          await page('cyans');const relative=await canvasPixel();assert.notDeepEqual(relative,original);await capture(`selective-relative-${width}-${theme}`);
          before=await layers();await nativeChoice(`${selector('mode')} select`,1);assert.equal((await value('mode')).value,1);const absolute=await canvasPixel();assert.notDeepEqual(absolute,relative);
          const after=await layers();await invoke('undo');assert.deepEqual(await layers(),before);await invoke('redo');assert.deepEqual(await layers(),after);
          await capture(`selective-absolute-${width}-${theme}`);await reopen(`selective-${width}-${theme}`);await send({type:'layer',action:{op:'delete_selected'}});
          await send({type:'effect',action:{op:'insert',effect:'channel_mixer'}});assert.deepEqual(await canvasPixel(),original);assert.deepEqual((await properties()).pages.map(p=>p.id),['red','green','blue']);
          owner=(await properties()).layer;before=await layers();assert.equal(before.find(l=>l.id===owner)?.effect.values.length,17);for(const id of ['red','green','blue'])await page(id);assert.deepEqual(await layers(),before);
          for(const output of ['red','green','blue']){await page(output);for(const channel of ['red','green','blue'])await edit(`${output}_${channel}`,channel===output?'85':channel==='red'?'15':'-5');await edit(`${output}_constant`,'1.25');}
          const stored=(await layers()).find(l=>l.id===owner).effect.values.slice();const rgb=await canvasPixel();assert.notDeepEqual(rgb,original);await capture(`mixer-rgb-${width}-${theme}`);
          await evaluate(`document.querySelector('${selector('monochrome')} input').focus()`);await key(' ',32);assert.equal((await value('monochrome')).value,true);assert.equal((await properties()).page,'gray');assert.deepEqual((await properties()).pages.map(p=>p.id),['gray']);
          assert.ok(await evaluate(`document.activeElement===document.querySelector('${selector('monochrome')} input')&&document.querySelector('[data-properties-page]').hidden`));
          for(const [name,text]of [['gray_red','60'],['gray_green','20'],['gray_blue','20'],['gray_constant','1.25']])await edit(name,text);
          await click(`${selector('gray_constant')} .number-value`);await evaluate(`window.pointwiseFocus=document.querySelector('${selector('gray_constant')} .number-entry');pointwiseFocus.focus();pointwiseFocus.value='77.';pointwiseFocus.dispatchEvent(new Event('input',{bubbles:true}));`);
          await send({type:'effect',action:{op:'set',layer:(await properties()).layer,key:'gray_red',value:{kind:'number',value:65}}});
          assert.ok(await evaluate(`pointwiseFocus===document.activeElement&&pointwiseFocus===document.querySelector('${selector('gray_constant')} .number-entry')&&pointwiseFocus.value==='77.'`));
          const beforeCancel=(await properties()).controls.map(c=>({key:c.key,value:c.value}));await key('Escape',27);assert.deepEqual((await properties()).controls.map(c=>({key:c.key,value:c.value})),beforeCancel);assert.equal((await value('gray_constant')).value,1.25);
          await invoke('undo');assert.equal((await value('gray_red')).value,60);await invoke('redo');assert.equal((await value('gray_red')).value,65);
          const gray=await canvasPixel();assert.ok(gray[3]===255&&Math.max(...gray.slice(0,3))-Math.min(...gray.slice(0,3))<=2);await capture(`mixer-gray-${width}-${theme}`);
          await evaluate(`document.querySelector('${selector('monochrome')} input').focus()`);before=await layers();await key(' ',32);assert.equal((await value('monochrome')).value,false);assert.ok(await evaluate(`document.activeElement===document.querySelector('${selector('monochrome')} input')`));
          const current=(await layers()).find(l=>l.id===owner).effect.values;assert.deepEqual(current.slice(0,12),stored.slice(0,12));assert.deepEqual(current.slice(12,16).map(v=>v.value),[65,20,20,1.25]);assert.deepEqual(await canvasPixel(),rgb);
          const restored=await layers();await invoke('undo');assert.deepEqual(await layers(),before);await invoke('redo');assert.deepEqual(await layers(),restored);
          await reopen(`mixer-${width}-${theme}`);await send({type:'layer',action:{op:'delete_selected'}});samples.push({width,theme,original,relative,absolute,rgb,gray});
        }
      }
      await writeFile(`${directory}/color-pages-pixels.json`,JSON.stringify(samples,null,2));console.log('PASS: Selective Color nine pages/modes and Channel Mixer RGB/Gray, native keyboard/drafts, Undo/source/archive/artwork in both themes and widths');return;
    }
    for(const width of widths) {
      await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
      await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();
      await send({type:'invoke',command:'fit_canvas'});
      for(const theme of ['light','dark']) {
        await send({type:'set_theme',theme});
        const pixel=await canvasPixel();assert.ok(pixel[1]>pixel[0]+60&&pixel[2]>pixel[0]+60,'Imported gradient is visibly rendered: '+pixel);
        await send({type:'effect',action:{op:'insert',effect:'hue_saturation'}});
        let view=await properties();assert.deepEqual(view.pages.map(p=>p.id),['rgb','reds','yellows','greens','cyans','blues','magentas']);
        const beforePages=await save();
        assert.equal(beforePages.document.layers.find(l=>l.id===beforePages.document.active_layer).effect.values.length,42);
        for(const id of view.pages.map(p=>p.id))await page(id);
        assert.deepEqual((await save()).document.layers,beforePages.document.layers,'Page navigation never edits stored parameters');
        await page('reds');
        for(const [name,text] of [['reds_hue','27'],['reds_center','350'],['reds_width','60'],['reds_feather','0']])await edit(name,text);
        const retained=(await properties()).controls.map(c=>({key:c.key,value:c.value}));
        await page('blues');await edit('blues_saturation','-32');await page('reds');
        for(const c of retained)assert.deepEqual(await value(c.key),c.value);
        await capture(`ranges-${width}-${theme}`);
        await evaluate(`document.querySelector('${selector('colorize')} input[type=checkbox]').focus()`);
        await key(' ',32);
        const toggleFocus=await evaluate(`({key:document.activeElement.closest('[data-property-key]')?.dataset.propertyKey??null,active:document.activeElement.outerHTML.slice(0,300)})`);
        assert.equal(toggleFocus.key,'colorize',JSON.stringify(toggleFocus));
        view=await properties();assert.deepEqual(view.pages.map(p=>p.id),['rgb']);assert.equal(view.page,'rgb');
        assert.ok(await evaluate(`document.querySelector('[data-properties-page]').hidden`),'Colorize hides the redundant page chooser');
        assert.ok(view.controls.every(c=>!c.key.startsWith('reds_')&&c.key!=='hue'&&c.key!=='saturation'));
        await key(' ',32);assert.equal((await value('colorize')).value,false);
        assert.ok(await evaluate(`document.activeElement===document.querySelector('${selector('colorize')} input[type=checkbox]')`),'Repeated Space keeps the toggle focused');
        await page('reds');await evaluate(`document.querySelector('${selector('colorize')} input[type=checkbox]').focus()`);await key(' ',32);
        for(const [name,text] of [['colorize_hue','210'],['colorize_saturation','55'],['lightness','12']])await edit(name,text);
        await click(`${selector('lightness')} .number-value`);
        await evaluate(`window.pointwiseFocus=document.querySelector('${selector('lightness')} .number-entry');pointwiseFocus.focus();pointwiseFocus.value='12.';pointwiseFocus.dispatchEvent(new Event('input',{bubbles:true}));`);
        const layer=(await properties()).layer;
        await send({type:'effect',action:{op:'set',layer,key:'colorize_hue',value:{kind:'number',value:211}}});
        assert.ok(await evaluate(`document.activeElement===pointwiseFocus&&pointwiseFocus===document.querySelector('${selector('lightness')} .number-entry')&&pointwiseFocus.value==='12.'`),'Native pending number/focus retained across publication');
        for(const enabled of [false,true]) {
          await send({type:'effect',action:{op:'set',layer,key:'colorize',value:{kind:'toggle',value:enabled}}});
          assert.equal((await value('colorize')).value,enabled);
          assert.ok(await evaluate(`document.activeElement===pointwiseFocus&&pointwiseFocus===document.querySelector('${selector('lightness')} .number-entry')&&pointwiseFocus.value==='12.'`),'Common pending number/focus retained when conditional fields change');
        }
        await key('Escape',27);await capture(`colorize-${width}-${theme}`);
        const colorized=await canvasPixel();assert.ok(colorized[2]>colorized[0]+20&&colorized[3]===255,'Colorize visibly changes opaque artwork: '+colorized);
        await click(`${selector('colorize')} input[type=checkbox]`);await page('reds');
        for(const c of retained)if(c.key.startsWith('reds_'))assert.deepEqual(await value(c.key),c.value);
        if(motion) {
          const before=await value('reds_hue'),slider=`${selector('reds_hue')} .number-slider`;
          const box=await evaluate(`(()=>{const n=document.querySelector('${slider}');n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x,y:r.y+r.height/2,width:r.width}})()`);
          await evaluate(`window.pointwiseMotion={frames:[],frame:layerApp.app.frame.bind(layerApp.app)};layerApp.app.frame=(...args)=>{const t=performance.now();try{return pointwiseMotion.frame(...args)}finally{pointwiseMotion.frames.push([args[0],t,performance.now()-t])}};`);
          const began=performance.now(),delivered=[];
          await evaluate('pointwiseMotion.start_ms=performance.now()');
          try {
            await pointer('mousePressed',{x:box.x+box.width/2,y:box.y});
            while(performance.now()-began<5000){const elapsed=performance.now()-began;delivered.push(pointer('mouseMoved',{x:box.x+box.width*(.5+.3*Math.sin(elapsed/250)),y:box.y}));await new Promise(resolve=>setTimeout(resolve,8));}
            await Promise.all(delivered);await evaluate('pointwiseMotion.end_ms=performance.now()');await pointer('mouseReleased',{x:box.x+box.width*.7,y:box.y});await settle();
          } finally {await evaluate('layerApp.app.frame=pointwiseMotion.frame');}
          const frameReport=await evaluate(`({frames:pointwiseMotion.frames.filter(f=>f[1]>=pointwiseMotion.start_ms&&f[1]<=pointwiseMotion.end_ms),motion_window_ms:[pointwiseMotion.start_ms,pointwiseMotion.end_ms],stats:JSON.parse(JSON.stringify(layerApp.app.renderer_stats(),(_,v)=>typeof v==='bigint'?Number(v):v))})`);
          reports.push({width,theme,input_count:delivered.length,...frameReport,frame_fields:['animation_ms','start_ms','cpu_frame_ms'],measurement:'Host callbacks during native slider motion; GPU/CPU telemetry histories may contain earlier work; no compositor presentation feedback',reference_tier_qualification:false});
          assert.notDeepEqual(await value('reds_hue'),before);await invoke('undo');assert.deepEqual(await value('reds_hue'),before,'One native slider drag makes one Undo');
        }
        await reopen(`hue-${width}-${theme}`);
        await send({type:'layer',action:{op:'delete_selected'}});
        for(const effect of effects) {
          await send({type:'effect',action:{op:'insert',effect}});
          if(effect==='threshold') {const before=await value('threshold');await edit('threshold','0.378');await invoke('undo');assert.deepEqual(await value('threshold'),before);await invoke('redo');const pixel=await canvasPixel();assert.ok(pixel.every(v=>v>=250),'Threshold visibly classifies opaque artwork: '+pixel);}
          else if(effect==='photo_filter') {
            await edit('density','37');await click(`${selector('preserve_luminance')} input[type=checkbox]`);
            await click(`${selector('color')} .property-color`);await wait(`!!document.querySelector('.color-dialog[open]')`);
            await evaluate(`(()=>{const n=document.querySelector('[data-color-field="0"]');n.value='0.1234567';n.dispatchEvent(new Event('input',{bubbles:true}));const alpha=document.querySelector('[data-color-field="3"]');alpha.value='37';alpha.dispatchEvent(new Event('input',{bubbles:true}))})()`);
            await click('.color-dialog .suggested-action');assert.equal((await value('color')).value.space,'Srgb');
          } else assert.equal((await properties()).controls.length,0);
          await capture(`${effect}-${width}-${theme}`);await reopen(`${effect}-${width}-${theme}`);
          await send({type:'layer',action:{op:'delete_selected'}});
        }
      }
    }
    if(motion)await writeFile(`${directory}/hue-motion.json`,JSON.stringify({hardware:(await call('SystemInfo.getInfo',{},null)).gpu.devices,runs:reports},null,2));
    console.log(`PASS: Hue42 pages, Colorize retention/focus/artwork, ${effects.join(', ')}, Undo/source invariants/archive reopen at ${widths.join('/')} in both themes${motion?', native slider motion':''}`);
  } finally {await evaluate('window.showOpenFilePicker=pointwiseFiles.open;window.showSaveFilePicker=pointwiseFiles.save;delete window.pointwiseFiles;delete window.placementTest');}
}
