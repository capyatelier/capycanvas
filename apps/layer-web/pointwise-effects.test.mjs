import assert from 'node:assert/strict';
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {placementSave} from './image-placement-motion.test.mjs';
import {sourceIdentity,authoredIdentity,packageObject,packageOccurrences,packageResources,packageResourceIdentity,rasterIdentity} from './package-fixture.test.mjs';
import {png} from './clone-journey.test.mjs';

const selectedEffect=manifest=>packageObject(manifest,packageOccurrences(manifest).find(o=>o.data.content.effect).data.content.effect);
const effectKeys=manifest=>Object.keys(effectValues(manifest));
const effectValues=manifest=>selectedEffect(manifest).data.values??{};

export async function checkPointwiseEffects({call,evaluate,settle,motion=true,widths=[640,1100],effects=['invert','threshold','desaturate','brightness_to_opacity','photo_filter'],colorPages=false,localAdjustments=false}) {
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
  const canvasReady=async()=>{await wait('layerApp.app.brush_ready()');await settle();await evaluate('layerApp.app.wait_for_canvas()');await settle();};
  const capture=async name=>{await canvasReady();const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));};
  const canvasPixel=async([docX,docY]=[64,192])=>{
    await canvasReady();
    const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${docX}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${docY}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
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
      assert.deepEqual(actual.filter(r=>rasters.some(original=>original.id===r.id)),rasters);
      for(const added of actual.filter(r=>!rasters.some(original=>original.id===r.id))){assert.deepEqual(added.tiles,[]);assert.equal(added.material,undefined);}
      return current;
    };
    const reopen=async name=>{
      const expected=await unchanged(),editingIndex=await evaluate('layerApp.state().layers.findIndex(layer=>layer.editing)');
      assert.ok(editingIndex>=0,'Archive journey has a current property owner');const owner=packageOccurrences(expected)[editingIndex].id;
      await writeFile(`${directory}/${name}.capy`,Buffer.from(await evaluate('Array.from(placementTest.saved)')));
      await invoke('open_document');await idle();
      const actual=await unchanged();assert.deepEqual(authoredIdentity(actual),authoredIdentity(expected),'Native archive retains exact effect values, phases and sources');
      const index=packageOccurrences(actual).findIndex(occurrence=>occurrence.id===owner);assert.ok(index>=0,'Reopened archive retains the property owner');
      const layer=await evaluate(`String(layerApp.state().layers[${index}].id)`);await evaluate(`layerApp.dispatch({type:'select_layer',id:BigInt(${JSON.stringify(layer)})})`);await settle();
      assert.equal(String((await properties()).layer),layer,'Analysis and controls belong to the reopened occurrence');
    };
    if(localAdjustments) {
      await evaluate(`window.localAnalysisFrames=[];window.localAnalysisFrame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{const change=localAnalysisFrame(...args);localAnalysisFrames.push({time:performance.now(),change,properties:layerApp.state().layer_properties});return change};`);
      const samples=[];const sample=async()=>{
        await evaluate('layerApp.app.wait_for_canvas()');await settle();
        const points=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return [[32,32],[64,192],[192,64],[224,224]].map(([x,y])=>[r.x+(x*c.zoom+c.translation[0])*r.width/c.viewport[0],r.y+(y*c.zoom+c.translation[1])*r.height/c.viewport[1]])})()`);
        const shot=await call('Page.captureScreenshot',{format:'png'});
        return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return ${JSON.stringify(points)}.map(([x,y])=>{if(x<0||y<0||x>=image.width||y>=image.height)throw Error('Artwork probe outside screenshot '+[x,y,image.width,image.height]);return Array.from(context.getImageData(Math.floor(x),Math.floor(y),1,1).data)})})()`);
      };
      const analyzed=async()=>{const owner=(await properties()).layer,end=Date.now()+120000;while(Date.now()<end){await evaluate('new Promise(resolve=>setTimeout(resolve,100))');const current=await properties();assert.equal(current.layer,owner);assert.notEqual(current.description,'Could not update this adjustment.');if(current.description==='Updating…')continue;await evaluate('layerApp.app.wait_for_canvas()');await settle();if((await properties()).description!=='Updating…')return;}throw Error('Local adjustment did not publish a ready canvas: '+JSON.stringify(await properties()));};
      const authored=async()=>authoredIdentity(await save());
      for(const width of widths) {
        await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
        await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();await invoke('fit_canvas');
        for(const theme of ['light','dark']) {
          await send({type:'set_theme',theme});const original=await sample();await capture(`local-source-${width}-${theme}`);await writeFile(`${directory}/layout-${width}-${theme}.json`,JSON.stringify(await call('Page.getLayoutMetrics'),null,2));
          await send({type:'effect',action:{op:'insert',effect:'hue_saturation'}});const lower=(await properties()).layer;
          await send({type:'effect',action:{op:'insert',effect:'shadows_highlights'}});const shadows=(await properties()).layer;if((await properties()).description==='Updating…')await capture(`analysis-pending-${width}-${theme}`);await analyzed();assert.deepEqual((await properties()).controls.map(c=>c.key),['shadows','highlights']);
          await edit('shadows','65');await edit('highlights','45');await analyzed();const adjusted=await sample();await capture(`shadows-highlights-${width}-${theme}`);await writeFile(`${directory}/analysis-status-${width}-${theme}.json`,JSON.stringify(await evaluate('JSON.parse(JSON.stringify({properties:layerApp.state().layer_properties,stats:layerApp.app.renderer_stats(),camera:layerApp.app.camera(),notices:layerApp.state().notices,frames:localAnalysisFrames,scroll:[scrollX,scrollY],canvasRect:layerApp.canvas.getBoundingClientRect().toJSON()},(_,v)=>typeof v==="bigint"?Number(v):v))'),(_,v)=>typeof v==='bigint'?Number(v):v,2));assert.notDeepEqual(adjusted,original);
          const after=await authored();await invoke('undo');await analyzed();assert.equal((await value('highlights')).value,0);await invoke('redo');await analyzed();assert.deepEqual(await authored(),after);
          await send({type:'effect',action:{op:'insert',effect:'clarity'}});const clarity=(await properties()).layer;await analyzed();assert.deepEqual((await properties()).controls.map(c=>c.key),['amount']);
          await edit('amount','55');await analyzed();const positive=await sample();
          await click(`${selector('amount')} .number-value`);await evaluate(`document.querySelector('${selector('amount')} .number-entry').value='99.'`);const beforeCancel=await authored();await key('Escape',27);assert.deepEqual(await authored(),beforeCancel);
          await edit('amount','-55');await analyzed();const negative=await sample();assert.notDeepEqual(positive,negative);
          await invoke('undo');await analyzed();assert.equal((await value('amount')).value,55);await invoke('redo');await analyzed();assert.equal((await value('amount')).value,-55);
          await send({type:'effect',action:{op:'insert',effect:'dehaze'}});const dehaze=(await properties()).layer;await analyzed();assert.deepEqual((await properties()).controls.map(c=>c.key),['amount']);assert.equal((await value('amount')).value,0);assert.deepEqual(await sample(),negative,'Zero Dehaze preserves the composed source');
          assert.ok(await evaluate(`(()=>{const n=document.querySelector('${selector('amount')}'),r=n.getBoundingClientRect(),p=n.closest('.dock-group,.floating-panel').getBoundingClientRect();return r.width>50&&r.left>=p.left-1&&r.right<=p.right+1&&r.top>=p.top-1&&r.bottom<=p.bottom+1})()`),'Dehaze ordinary Amount fits the visible Properties panel');
          await edit('amount','55');await analyzed();const dehazePositive=await sample();assert.notDeepEqual(dehazePositive,negative);await capture(`dehaze-positive-${width}-${theme}`);
          await edit('amount','-55');await analyzed();const dehazeNegative=await sample();assert.notDeepEqual(dehazeNegative,dehazePositive);await capture(`dehaze-negative-${width}-${theme}`);
          await invoke('undo');await analyzed();assert.equal((await value('amount')).value,55);assert.deepEqual(await sample(),dehazePositive);
          await invoke('redo');await analyzed();assert.equal((await value('amount')).value,-55);assert.deepEqual(await sample(),dehazeNegative);
          await send({type:'effect',action:{op:'set',layer:lower,key:'lightness',value:{kind:'number',value:-20}}});await analyzed();const changed=await sample();assert.notDeepEqual(changed,dehazeNegative);await capture(`dehaze-stacked-${width}-${theme}`);
          await invoke('undo');await analyzed();assert.deepEqual(await sample(),dehazeNegative,'Undo lower source edit restores Dehaze analysis');await invoke('redo');await analyzed();assert.deepEqual(await sample(),changed);
          const deleted=packageOccurrences(await save()).filter(o=>o.data.content.effect).map(o=>o.id);
          await reopen(`local-adjustments-${width}-${theme}`);await analyzed();assert.deepEqual(await sample(),changed);
          const tabs=await evaluate('JSON.parse(JSON.stringify(layerApp.app.document_tabs(0),(_,v)=>typeof v==="bigint"?Number(v):v))'),other=tabs.tabs.find(tab=>tab.id!==tabs.selected);
          assert.ok(other);await evaluate(`layerApp.documents.select(BigInt(${other.id}))`);await idle();await wait('layerApp.app.document_park_ready()');
          await evaluate(`layerApp.documents.select(BigInt(${tabs.selected}))`);await idle();await analyzed();assert.deepEqual(await sample(),changed);await unchanged();
          await evaluate('layerApp.restartGpu()');await idle();await analyzed();assert.deepEqual(await sample(),changed);await unchanged();await capture(`local-recreated-${width}-${theme}`);
          samples.push({width,theme,original,adjusted,positive,negative,dehazePositive,dehazeNegative,changed});for(const portable of deleted){const manifest=await save(),rows=await evaluate('JSON.parse(JSON.stringify(layerApp.state().layers,(_,v)=>typeof v==="bigint"?Number(v):v))'),id=rows[packageOccurrences(manifest).findIndex(o=>o.id===portable)].id;await send({type:'layer',action:{op:'select',id,mask:false}});await send({type:'layer',action:{op:'delete_selected'}});}await unchanged();
          await send({type:'customize',action:{type:'set_panel_visible',panel:'adjustments',visible:true}});
          await send({type:'move_panel',panel:'adjustments',target:{kind:'edge',edge:'right',outer:false},viewport:[width,800]});
          await send({type:'filter_picker',action:{op:'category',category:null}});
          for(const [id,query] of [['shadows_highlights','Shadows'],['clarity','Clarity'],['dehaze','Dehaze']]) {
            await send({type:'filter_picker',action:{op:'search',query}});
            await wait(`(()=>{const c=[...document.querySelectorAll('[data-effect="${id}"] canvas')].find(c=>c.getBoundingClientRect().height>0);return c?.width>0&&c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i,a)=>i%4===0&&Math.max(a[i],a[i+1],a[i+2])-Math.min(a[i],a[i+1],a[i+2])>20)})()`);
            await capture(`catalog-${id}-${width}-${theme}`);
          }
          await send({type:'customize',action:{type:'set_panel_visible',panel:'adjustments',visible:false}});await unchanged();
        }
      }
      await writeFile(`${directory}/local-pixels.json`,JSON.stringify(samples,null,2));console.log('PASS: Shadows/Highlights, Clarity and Dehaze generic controls, stacked live analysis, Undo/source/archive/recreation in both themes and widths');return;
    }
    if(colorPages) {
      const samples=[];
      const nativeChoice=async(selector,index)=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);await key('Home',36);for(let i=0;i<index;i++)await key('ArrowDown',40);assert.equal(await evaluate(`Number(document.querySelector(${JSON.stringify(selector)}).value)`),index);};
      const authored=async()=>authoredIdentity(await save());
      for(const width of widths) {
        await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
        await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();await invoke('fit_canvas');
        for(const theme of ['light','dark']) {
          await send({type:'set_theme',theme});const original=await canvasPixel();assert.ok(original[1]>original[0]+60&&original[3]===255);
          await send({type:'effect',action:{op:'insert',effect:'selective_color'}});assert.deepEqual(await canvasPixel(),original);
          const pages=['reds','yellows','greens','cyans','blues','magentas','whites','neutrals','blacks'];assert.deepEqual((await properties()).pages.map(p=>p.id),pages);
          let owner=(await properties()).layer,before=await authored();assert.equal(effectKeys(await save()).length,37);
          for(const id of pages)await page(id);assert.deepEqual(await authored(),before);
          const inks=['cyan','magenta','yellow','black'];
          for(let i=0;i<pages.length;i++){await page(pages[i]);for(const [j,text] of [String((i+1)*2),'-1.25','1.5','0.5'].entries())await edit(`${pages[i]}_${inks[j]}`,text);}
          for(let i=0;i<pages.length;i++){await page(pages[i]);for(const [j,n] of [(i+1)*2,-1.25,1.5,.5].entries())assert.ok(Math.abs((await value(`${pages[i]}_${inks[j]}`)).value-n)<1e-6);}
          await page('cyans');const relative=await canvasPixel();assert.notDeepEqual(relative,original);await capture(`selective-relative-${width}-${theme}`);
          before=await authored();await nativeChoice(`${selector('mode')} select`,1);assert.equal((await value('mode')).value,1);const absolute=await canvasPixel();assert.notDeepEqual(absolute,relative);
          const after=await authored();await invoke('undo');assert.deepEqual(await authored(),before);await invoke('redo');assert.deepEqual(await authored(),after);
          await capture(`selective-absolute-${width}-${theme}`);await reopen(`selective-${width}-${theme}`);await send({type:'layer',action:{op:'delete_selected'}});
          await send({type:'effect',action:{op:'insert',effect:'channel_mixer'}});assert.deepEqual(await canvasPixel(),original);assert.deepEqual((await properties()).pages.map(p=>p.id),['red','green','blue']);
          owner=(await properties()).layer;before=await authored();assert.equal(effectKeys(await save()).length,17);for(const id of ['red','green','blue'])await page(id);assert.deepEqual(await authored(),before);
          for(const output of ['red','green','blue']){await page(output);for(const channel of ['red','green','blue'])await edit(`${output}_${channel}`,channel===output?'85':channel==='red'?'15':'-5');await edit(`${output}_constant`,'1.25');}
          const stored=effectValues(await save());const rgb=await canvasPixel();assert.notDeepEqual(rgb,original);await capture(`mixer-rgb-${width}-${theme}`);
          await evaluate(`document.querySelector('${selector('monochrome')} input').focus()`);await key(' ',32);assert.equal((await value('monochrome')).value,true);assert.equal((await properties()).page,'gray');assert.deepEqual((await properties()).pages.map(p=>p.id),['gray']);
          assert.ok(await evaluate(`document.activeElement===document.querySelector('${selector('monochrome')} input')&&document.querySelector('[data-properties-page]').hidden`));
          for(const [name,text]of [['gray_red','60'],['gray_green','20'],['gray_blue','20'],['gray_constant','1.25']])await edit(name,text);
          await click(`${selector('gray_constant')} .number-value`);await evaluate(`window.pointwiseFocus=document.querySelector('${selector('gray_constant')} .number-entry');pointwiseFocus.focus();pointwiseFocus.value='77.';pointwiseFocus.dispatchEvent(new Event('input',{bubbles:true}));`);
          await send({type:'effect',action:{op:'set',layer:(await properties()).layer,key:'gray_red',value:{kind:'number',value:65}}});
          assert.ok(await evaluate(`pointwiseFocus===document.activeElement&&pointwiseFocus===document.querySelector('${selector('gray_constant')} .number-entry')&&pointwiseFocus.value==='77.'`));
          const beforeCancel=(await properties()).controls.map(c=>({key:c.key,value:c.value}));await key('Escape',27);assert.deepEqual((await properties()).controls.map(c=>({key:c.key,value:c.value})),beforeCancel);assert.equal((await value('gray_constant')).value,1.25);
          await invoke('undo');assert.equal((await value('gray_red')).value,60);await invoke('redo');assert.equal((await value('gray_red')).value,65);
          const gray=await canvasPixel();assert.ok(gray[3]===255&&Math.max(...gray.slice(0,3))-Math.min(...gray.slice(0,3))<=2);await capture(`mixer-gray-${width}-${theme}`);
          await evaluate(`document.querySelector('${selector('monochrome')} input').focus()`);before=await authored();await key(' ',32);assert.equal((await value('monochrome')).value,false);assert.ok(await evaluate(`document.activeElement===document.querySelector('${selector('monochrome')} input')`));
          const current=effectValues(await save());assert.deepEqual(['red','green','blue'].flatMap(output=>['red','green','blue','constant'].map(input=>current[`${output}_${input}`])),['red','green','blue'].flatMap(output=>['red','green','blue','constant'].map(input=>stored[`${output}_${input}`])));assert.deepEqual(['red','green','blue','constant'].map(input=>current[`gray_${input}`]),[65,20,20,1.25]);assert.deepEqual(await canvasPixel(),rgb);
          const restored=await authored();await invoke('undo');assert.deepEqual(await authored(),before);await invoke('redo');assert.deepEqual(await authored(),restored);
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
        assert.equal(effectKeys(beforePages).length,42);
        for(const id of view.pages.map(p=>p.id))await page(id);
        assert.deepEqual(authoredIdentity(await save()),authoredIdentity(beforePages),'Page navigation never edits stored parameters or phases');
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
          const field=selector('reds_hue'),origin=(await value('reds_hue')).value;
          const geometry=await evaluate(`(()=>{const n=document.querySelector('${field}'),r=n.getBoundingClientRect(),t=n.slider.getBoundingClientRect();return {panel:n.panel,height:r.height,steps:n.querySelectorAll('.number-step').length,left:t.left-r.left,right:r.right-t.right,thumb:getComputedStyle(n.slider).getPropertyValue('--thumb-size')}})()`);
          assert.equal(geometry.panel,true);assert.equal(geometry.height,36);assert.equal(geometry.steps,0);assert.equal(geometry.left,36);assert.equal(geometry.right,88);assert.equal(geometry.thumb.trim(),'0px');
          await click(`${field} .number-value`);
          const editor=await evaluate(`(()=>{const n=document.querySelector('${field}'),e=n.entry.getBoundingClientRect(),t=n.slider.getBoundingClientRect();return {width:e.width,height:e.height,gap:e.left-t.right}})()`);
          assert.equal(editor.width,80);assert.equal(editor.height,34);assert.equal(editor.gap,8);await key('Escape',27);
          for(const device of ['mouse','pen','touch'])for(const cancel of [false,true]) {
            const p=await evaluate(`(()=>{const n=document.querySelector('${field} .number-value');n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`),end={x:p.x,y:p.y-9};
            const contact=async(type,point)=>device==='touch'?call('Input.dispatchTouchEvent',{type,touchPoints:type==='touchEnd'||type==='touchCancel'?[]:[{id:1,...point}]}):call('Input.dispatchMouseEvent',{type,...point,pointerType:device,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,force:.7});
            await contact(device==='touch'?'touchStart':'mousePressed',p);await contact(device==='touch'?'touchMove':'mouseMoved',{x:p.x,y:p.y-8});await settle();
            assert.match(await evaluate(`document.querySelector('${field} .number-value').textContent`),/29\.0/,'Scrub keeps decimal width for whole values');
            await contact(device==='touch'?'touchMove':'mouseMoved',end);await settle();
            assert.ok(Math.abs((await value('reds_hue')).value-origin-2.3)<1e-5,`${device} fine vertical scrub`);
            assert.match(await evaluate(`document.querySelector('${field} .number-value').textContent`),/29\.3/);
            if(cancel&&device!=='touch')await key('Escape',27);
            await contact(device==='touch'?(cancel?'touchCancel':'touchEnd'):'mouseReleased',end);await settle();
            if(cancel)assert.equal((await value('reds_hue')).value,origin,`${device} cancellation restores exact origin`);
            else {await invoke('undo');assert.equal((await value('reds_hue')).value,origin,`${device} scrub makes one Undo`);}
          }
          for(const device of ['mouse','pen','touch']) {
            const p=await evaluate(`(()=>{const r=document.querySelector('${field} .number-slider').getBoundingClientRect();return{x:r.x+r.width*.543,y:r.y+r.height/2}})()`);
            if(device==='touch'){await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...p}]});await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});}
            else for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,pointerType:device,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,force:.7});
            await settle();assert.equal((await value('reds_hue')).value,15,`${device} slider snaps degrees to integers`);await invoke('undo');assert.equal((await value('reds_hue')).value,origin,`${device} slider makes one Undo`);
          }
          const title=await evaluate(`(()=>{const r=document.querySelector('${field} .number-title').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
          for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...title,button:'left',buttons:type==='mousePressed'?1:0,clickCount:2});await settle();assert.equal((await value('reds_hue')).value,0);await invoke('undo');assert.equal((await value('reds_hue')).value,origin);
        }
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
          if(effect==='threshold') {
            const before=await value('threshold');await edit('threshold','0.378');await invoke('undo');assert.deepEqual(await value('threshold'),before);await invoke('redo');
            const pixel=await canvasPixel();assert.ok(pixel.every(v=>v>=250),'Threshold visibly classifies opaque artwork: '+pixel);
            const choice=async(name,index)=>{await click(`${selector(name)} select`);await key('Home',36);for(let i=0;i<index;i++)await key('ArrowDown',40);await key('Enter',13);await idle();assert.equal((await value(name)).value,index);};
            await choice('colors',1);await choice('transparency',1);await edit('alpha_threshold','37');
            await choice('transparency',0);assert.equal(await value('alpha_threshold'),undefined);
            await invoke('undo');assert.equal((await value('alpha_threshold')).value,37);
            await choice('colors',2);
          }
          else if(effect==='photo_filter') {
            await edit('density','37');await click(`${selector('preserve_luminance')} input[type=checkbox]`);
            await click(`${selector('color')} .property-color`);await wait(`!!document.querySelector('.color-dialog[open]')`);
            await evaluate(`(()=>{const d=document.querySelector('.color-dialog[open]');d.querySelector('.color-value[data-color-value="0-0"]').click();const n=d.querySelector('.color-value-input[data-color-value="0-0"]');n.value='31';n.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));})()`);
            await click('.color-dialog .suggested-action');assert.equal((await value('color')).value.space,'Srgb');
          } else assert.equal((await properties()).controls.length,0);
          await capture(`${effect}-${width}-${theme}`);await reopen(`${effect}-${width}-${theme}`);
          await send({type:'layer',action:{op:'delete_selected'}});
        }
      }
    }
    if(motion)await writeFile(`${directory}/hue-motion.json`,JSON.stringify({hardware:(await call('SystemInfo.getInfo',{},null)).gpu.devices,runs:reports},null,2));
    console.log(`PASS: Hue42 pages, Colorize retention/focus/artwork, ${effects.join(', ')}, Undo/source invariants/archive reopen at ${widths.join('/')} in both themes${motion?', native slider motion':''}`);
  } finally {await evaluate('if(window.localAnalysisFrame){layerApp.app.frame=localAnalysisFrame;delete window.localAnalysisFrame;}window.showOpenFilePicker=pointwiseFiles.open;window.showSaveFilePicker=pointwiseFiles.save;delete window.pointwiseFiles;delete window.placementTest');}
}

export async function checkLookupTransport({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/photo-editing-color/p23-web';await mkdir(directory,{recursive:true});
  const fixture=await readFile(process.env.LAYER_LOOKUP_FIXTURE??'artifacts/photo-editing-color/p23-gtk/lookup-1100-Light.capy');
  const wait=(condition,recovering=false)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){
    const status=document.querySelector('#status')?.textContent??'';
    const packageView=[...document.querySelectorAll('dialog[open] button')].some(button=>button.textContent.startsWith('Copy Original'));
    if(${recovering}&&(/Recovery (?:operation|capture) failed|Recovery unavailable/.test(status)||packageView))reject(Error('Recovery did not adopt the saved drawing: '+document.body.innerText.slice(-900)));
    else if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-900)));else setTimeout(poll,40);
  }poll()})`);
  const invoke=async command=>{await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);await settle();};
  const idle=()=>wait('!layerApp.state().document_file.busy&&!layerApp.documents.busy()&&layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null');
  const install=()=>evaluate(`window.placementTest={};window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(v){placementTest.saved=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){},async abort(){}}}});`);
  const save=placementSave({evaluate,invoke,idle});
  const pixel=async()=>{
    await evaluate('layerApp.app.wait_for_canvas()');await settle();
    const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(64*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(192*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{...point,width:1,height:1,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const c=document.createElement('canvas');c.width=c.height=1;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);return Array.from(x.getImageData(0,0,1,1).data)})()`);
  };
  await idle();await install();
  await evaluate(`window.lookupFixture=new Uint8Array(${JSON.stringify(Array.from(fixture))});window.showOpenFilePicker=async()=>[{name:'loaded-lookup.capy',getFile:async()=>new File([lookupFixture],'loaded-lookup.capy')}];`);
  await invoke('open_document');await idle();await invoke('fit_canvas');
  const click=async selector=>{
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
  };
  const key=async(name,code)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,windowsVirtualKeyCode:code});await settle();};
  const select=async index=>{await click('[data-property-resource]');await key('Home',36);for(let i=0;i<index;i++)await key('ArrowDown',40);await key('Enter',13);await idle();};
  const originalArchive=await save(),originalSources=sourceIdentity(originalArchive);
  const lookupIndex=packageOccurrences(originalArchive).findIndex(occurrence=>{const application=occurrence.data.content.effect&&packageObject(originalArchive,occurrence.data.content.effect);return application&&application.data.builtin==='color_lookup'});assert.ok(lookupIndex>=0);
  const lookupOccurrence=packageOccurrences(originalArchive)[lookupIndex].id;
  const selectLookup=async manifest=>{
    const index=packageOccurrences(manifest).findIndex(occurrence=>occurrence.id===lookupOccurrence);assert.ok(index>=0,'Reopened archive retains the lookup occurrence');
    const layer=await evaluate(`String(layerApp.state().layers[${index}].id)`);await evaluate(`layerApp.dispatch({type:'select_layer',id:BigInt(${JSON.stringify(layer)})})`);await settle();
    assert.equal(await evaluate('String(layerApp.state().layer_properties.layer)'),layer,'Lookup properties belong to the current occurrence');
  };
  await selectLookup(originalArchive);
  const cube='TITLE "Imported inverse"\nLUT_3D_SIZE 2\n'+Array.from({length:8},(_,i)=>`${1-(i&1)} ${1-((i>>1)&1)} ${1-((i>>2)&1)}`).join('\n');
  for(const width of [640,1100])for(const theme of ['light','dark']) {
    await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();await invoke('fit_canvas');
    await wait(`!!document.querySelector('[data-property-resource]')&&!document.querySelector('[data-property-resource]').closest('[hidden]')`);
    await select(0);const original=await pixel();
    for(const index of [1,2,3]) {
      await select(index);
      const changed=await pixel();await writeFile(`${directory}/lookup-selection-${index}-${width}-${theme}.json`,JSON.stringify(await evaluate('JSON.parse(JSON.stringify({properties:layerApp.state().layer_properties,host_error:layerApp.state().host_error,camera:layerApp.app.camera()},(_,v)=>typeof v==="bigint"?String(v):v))'),null,2));
      const diagnostic=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/lookup-selection-${index}-${width}-${theme}.png`,Buffer.from(diagnostic.data,'base64'));assert.equal(await evaluate('Number(layerApp.state().layer_properties.resource_selection)'),index,'Native selector commits the requested preset');assert.notDeepEqual(changed,original,'Built-in Look changes visible artwork');
      assert.equal(await evaluate(`layerApp.state().layer_properties.controls.some(c=>c.key==='color_space')`),false);
      assert.equal(await evaluate(`document.querySelector('[data-property-resource]').selectedOptions[0].textContent.includes(String.fromCharCode(10))`),false);
      const edited=await save();assert.deepEqual(sourceIdentity(edited),originalSources);
      await invoke('undo');assert.deepEqual(await pixel(),original);await invoke('redo');assert.deepEqual(await pixel(),changed);
      const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/lookup-preset-${index}-${width}-${theme}.png`,Buffer.from(shot.data,'base64'));
      await select(0);
    }
    await select(1);const beforeAmount=await evaluate("layerApp.state().layer_properties.controls.find(c=>c.key==='intensity').value.value"),nextAmount=beforeAmount===50?65:50;
    await click('[data-property-key="intensity"] .number-value');await evaluate(`(()=>{const n=document.querySelector('[data-property-key="intensity"] .number-entry');n.value=${JSON.stringify(String(nextAmount))};n.dispatchEvent(new Event('input',{bubbles:true}))})()`);await key('Enter',13);
    assert.equal(await evaluate("layerApp.state().layer_properties.controls.find(c=>c.key==='intensity').value.value"),nextAmount);await invoke('undo');assert.equal(await evaluate("layerApp.state().layer_properties.controls.find(c=>c.key==='intensity').value.value"),beforeAmount);await invoke('redo');assert.equal(await evaluate("layerApp.state().layer_properties.controls.find(c=>c.key==='intensity').value.value"),nextAmount);await select(0);
    const cancelled=await save();await evaluate(`window.showOpenFilePicker=async()=>{throw new DOMException('Cancelled','AbortError')}`);await click('[data-action="import-lookup"]');await idle();assert.deepEqual(authoredIdentity(await save()),authoredIdentity(cancelled));
    for(const oversized of [false,true]) {
      const beforeFailure=await save();
      await evaluate(`window.showOpenFilePicker=async()=>[{name:'invalid.cube',getFile:async()=>new File([${oversized?"' '.repeat(layerApp.app.lookup_text_limit()+1)":"'LUT_3D_SIZE 2\\n0 0 0'"}],'invalid.cube')}];`);
      await click('[data-action="import-lookup"]');await idle();assert.ok(await evaluate('layerApp.state().host_error'),'Rejected LUT reports a shared localized error');
      assert.deepEqual(authoredIdentity(await save()),authoredIdentity(beforeFailure),'Rejected LUT creates no history edit');
    }
    const owner=await evaluate('String(layerApp.state().layer_properties.layer)'),other=await evaluate(`String(layerApp.state().layers.find(l=>String(l.id)!==${JSON.stringify(owner)}).id)`),beforeStale=await save();
    await evaluate(`window.showOpenFilePicker=()=>new Promise(resolve=>window.lookupPickerRelease=resolve)`);await click('[data-action="import-lookup"]');await wait('layerApp.state().document_file.busy');
    await evaluate(`layerApp.dispatch({type:'select_layer',id:BigInt(${JSON.stringify(other)})});lookupPickerRelease([{name:'stale.cube',getFile:async()=>new File([${JSON.stringify(cube)}],'stale.cube')}])`);await idle();
    assert.deepEqual(authoredIdentity(await save()),authoredIdentity(beforeStale),'Deferred import cannot mutate a retired property owner');
    await evaluate(`layerApp.dispatch({type:'select_layer',id:BigInt(${JSON.stringify(owner)})})`);await settle();
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'inverse.cube',getFile:async()=>new File([${JSON.stringify(cube)}],'inverse.cube')}];`);await click('[data-action="import-lookup"]');await idle();
    assert.equal(await evaluate('layerApp.state().layer_properties.resource_name'),'Imported inverse');assert.equal(await evaluate('layerApp.state().layer_properties.resource_selection??null'),null);
    assert.ok(await evaluate(`layerApp.state().layer_properties.controls.some(c=>c.key==='color_space')`));
    const imported=await save();assert.deepEqual(sourceIdentity(imported),originalSources);
    const importedPixel=await pixel();assert.notDeepEqual(importedPixel,original);
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'imported.capy',getFile:async()=>new File([placementTest.saved],'imported.capy')}];`);await invoke('open_document');await idle();await invoke('fit_canvas');assert.deepEqual(await pixel(),importedPixel,'Archive owns imported resource after file picker is replaced');const reopened=await save();assert.deepEqual(packageResourceIdentity(reopened),packageResourceIdentity(imported));await selectLookup(reopened);
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/lookup-import-${width}-${theme}.png`,Buffer.from(shot.data,'base64'));
  }
  await evaluate(`window.showOpenFilePicker=async()=>[{name:'loaded-lookup.capy',getFile:async()=>new File([lookupFixture],'loaded-lookup.capy')}];`);await invoke('open_document');await idle();await invoke('fit_canvas');
  const expected=await save();assert.equal(packageResources(expected,'capy.lut3d/1').length,1);assert.equal(packageOccurrences(expected).filter(o=>o.data.content.effect&&packageObject(expected,o.data.content.effect).data.values?.resource?.value?.resource).length,1);
  const lookup=packageResources(expected,'capy.lut3d/1')[0];assert.equal(Number(lookup.data.decoded_bytes??lookup.bytes),lookup.data.size**3*12);
  const compare=async()=>{
    const actual=await save();assert.deepEqual(packageResourceIdentity(actual),packageResourceIdentity(expected),'Worker packages retain LUT resource identities, descriptors and checksums');
    assert.deepEqual(actual.objects,expected.objects);assert.deepEqual(sourceIdentity(actual),sourceIdentity(expected));
  };
  const samples=[];
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();await invoke('fit_canvas');
    const before=await pixel();assert.equal(before[3],255);assert.ok(Math.max(...before.slice(0,3))-Math.min(...before.slice(0,3))>30);
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'loaded-lookup.capy',getFile:async()=>new File([placementTest.saved],'loaded-lookup.capy')}];`);await invoke('open_document');await idle();await invoke('fit_canvas');await compare();assert.deepEqual(await pixel(),before);
    await evaluate('layerApp.restartGpu()');await idle();assert.deepEqual(await pixel(),before,'Renderer recreation retains resolved LUT pixels');await compare();
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/lookup-worker-${theme}.png`,Buffer.from(shot.data,'base64'));samples.push({theme,pixel:before});
  }
  await writeFile(`${directory}/lookup-worker.capy`,Buffer.from(await evaluate('Array.from(placementTest.saved)')));
  const beforeRestart=await evaluate("JSON.parse(JSON.stringify(layerApp.state().document_file,(_,value)=>typeof value==='bigint'?Number(value):value))");
  await evaluate('layerApp.documents.autosave()');await call('Page.reload',{ignoreCache:true});await new Promise(resolve=>setTimeout(resolve,1000));
  await wait('window.layerApp?.app.brush_ready()',true);await evaluate('layerApp.documents.startRecovery()');await idle();
  assert.equal(await evaluate('layerApp.state().document_file.modified'),beforeRestart.location?true:beforeRestart.modified);assert.equal(await evaluate('layerApp.state().document_file.location?.name??null'),beforeRestart.location?.name??null);
  await install();await invoke('fit_canvas');await compare();const recoveredPixel=await pixel(),expectedPixel=samples.at(-1).pixel;assert.ok(recoveredPixel.every((value,i)=>Math.abs(value-expectedPixel[i])<=(i===3?0:1)),`IndexedDB recovery retains resolved LUT pixels within display quantization: ${recoveredPixel} versus ${expectedPixel}`);
  await writeFile(`${directory}/lookup-worker-pixels.json`,JSON.stringify(samples,null,2));console.log('PASS: native LUT selector/picker presets, cancel/error/stale-owner rejection and four layout/theme imports preserve source/history; worker save/open, GPU recreation and IndexedDB recovery retain resource payload and visible pixels');
}
