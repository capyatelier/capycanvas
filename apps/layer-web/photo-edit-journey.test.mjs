import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const COLORS={mouse:[.85,.08,.05,1],touch:[.05,.6,.1,1],pen:[.1,.2,.85,1]};
const TINTS={mouse:[.2,.5,.1,1],touch:[.7,.3,.9,1],pen:[.9,.7,.1,1]};

export async function checkPhotoEdit({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/photo-edit/web-tablet':'artifacts/photo-edit/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const control=async key=>(await state()).layer_properties.controls.find(c=>c.key===key);
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=120;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(points,kind)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',points[0],kind);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,kind);await settle();}
    await pointer('mouseReleased',points.at(-1),kind);await settle();
  };
  const menuRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const choose=async(menu,path,kind)=>{
    const summary=`.header-menu[data-menu="${menu}"] > summary`;
    await tap(await middle(summary),kind);
    await wait(`document.querySelector('.header-menu[data-menu="${menu}"]').open`);
    for(const label of path) {
      await wait(`!!${menuRow(label)}&&!${menuRow(label)}.disabled`);
      await tap(await evaluate(`(()=>{const r=${menuRow(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
    }
    await wait(`!document.querySelector('.header-menu[data-menu="${menu}"]').open`);
  };
  const sample=async(p,test,label)=>{
    if(await evaluate("layerApp.state().layer_tools.tool!=='pick_visible'"))await invoke('eyedropper');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:'mouse'});await settle();
    let last;
    for(const end=Date.now()+10000;Date.now()<end;await pause(100)) {
      last=await evaluate('(p=>p&&Array.from(p.rgba))(layerApp.state().color_picker.preview)');
      if(last&&test(last))return last;
    }
    assert.fail(`${label}: ${JSON.stringify(last)}`);
  };
  const near=(value,expected)=>value.every((v,i)=>Math.abs(v-expected[i])<1e-5);
  const showProperties=async kind=>{
    await send({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:true}});
    if(!await evaluate(`document.querySelector('.effect-properties')?.getBoundingClientRect().height>0`))
      await tap(await middle('.dock-tab[data-panel="properties"],.column-tab[data-panel="properties"]'),kind);
    await wait(`document.querySelector('.effect-properties').getBoundingClientRect().height>0`);
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    const c=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{v:c.viewport,a:c.work_area,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
    const center={x:c.r.x+(c.a[0]+c.a[2]/2)*c.r.width/c.v[0],y:c.r.y+(c.a[1]+c.a[3]/2)*c.r.height/c.v[1]};
    const at=(x,y)=>({x:center.x+x,y:center.y+y});
    const inside=at(0,-10),outside=at(Math.min(260,c.a[2]*c.r.width/c.v[0]/2-40),-Math.min(160,c.a[3]*c.r.height/c.v[1]/2-40));

    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      await send({type:'set_color',rgba:COLORS.mouse});
      for(const label of ['Solid Color','Gradient Fill']) {
        const before=await state();
        await choose('filter',['Fill',label],'mouse');
        await wait(`layerApp.state().layer_properties.title===${JSON.stringify(label)}`);
        const after=await state(),fill=after.layers.find(l=>l.editing);
        assert.equal(after.layers.length,before.layers.length+1,`${name}: ${label} adds one layer`);
        assert.equal(fill.has_mask,false,`${name}: ${label} starts without a mask`);
        if(label==='Solid Color')for(const point of [inside,outside])
          await sample(point,p=>p.every((v,i)=>Math.abs(v-COLORS.mouse[i])<.02),`${name}: the unmasked fill covers the canvas`);
        await invoke('undo');
        assert.equal((await state()).layers.length,before.layers.length);
        await invoke('redo');
        assert.equal((await state()).layers.find(l=>l.editing).has_mask,false);
        await invoke('undo');
      }
    }

    for(const kind of ['mouse','touch','pen']) {
      const rgba=COLORS[kind];
      await send({type:'set_color',rgba});
      await invoke('lasso');
      await drag([at(-140,-100),at(0,-110),at(140,-100),at(150,0),at(140,90),at(0,100),at(-140,90),at(-140,-100)],kind==='touch'?'pen':kind);
      await wait('layerApp.state().layer_tools.has_selection');
      const before=await state(),base=before.layers.find(l=>l.editing);
      await choose('filter',['Fill','Solid Color'],kind);
      await wait(`layerApp.state().layer_properties.title==='Solid Color'`);
      const after=await state(),fill=after.layers.find(l=>l.editing);
      assert.equal(after.layers.length,before.layers.length+1,`${kind}: Filter › Fill › Solid Color adds one layer`);
      assert.equal(after.layers.indexOf(fill)+1,after.layers.findIndex(l=>l.id===base.id),`${kind}: the fill sits directly above the active layer`);
      assert.equal(fill.has_mask,true,`${kind}: the selection becomes the fill's mask`);
      assert.equal(after.layer_tools.has_selection,false,`${kind}: the selection moves into the mask`);
      assert.ok(near((await control('color')).value.value.rgba,rgba),`${kind}: the fill uses the current colour`);
      await sample(inside,p=>p.every((v,i)=>Math.abs(v-rgba[i])<.02),`${kind}: the current colour fills the selection`);
      await sample(outside,p=>p.slice(0,3).every(v=>v>.95),`${kind}: the paper shows outside the selection`);
      await invoke('undo');
      const undone=await state();
      assert.equal(undone.layers.length,before.layers.length,`${kind}: one undo step removes the fill`);
      assert.equal(undone.layers.find(l=>l.editing).id,base.id);
      await sample(inside,p=>p.slice(0,3).every(v=>v>.95),`${kind}: the paper shows again after undo`);
      if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    }

    const blackWhite=await evaluate(`(()=>{const a=layerApp.state().adjustments.find(a=>a.id==='black_white');return{category:a.category_label,label:a.label}})()`);
    const bucket='.effect-properties [data-action="tint-color-bucket"]';
    for(const kind of ['mouse','touch','pen']) {
      const count=(await state()).layers.length;
      await choose('filter',[blackWhite.category,blackWhite.label],kind);
      await wait(`layerApp.state().layer_properties.title===${JSON.stringify(blackWhite.label)}`);
      assert.equal((await state()).layers.length,count+1,`${kind}: the Filter menu inserts Black & White`);
      await showProperties(kind);
      await evaluate(`document.querySelector('${bucket}').scrollIntoView({block:'center'})`);await settle();
      const row=await evaluate(`(()=>{const b=document.querySelector('${bucket}'),row=b.closest('.property-row'),swatch=row.querySelector('.property-color'),s=swatch.getBoundingClientRect(),r=b.getBoundingClientRect();
        return{label:row.firstChild.textContent,swatch:swatch.getAttribute('aria-label'),title:b.title,order:!!(swatch.compareDocumentPosition(b)&Node.DOCUMENT_POSITION_FOLLOWING),line:r.top<s.bottom&&s.top<r.bottom}})()`);
      assert.deepEqual(row,{label:'Tint color',swatch:'Tint color',title:'Use selected color',order:true,line:true},`${kind}: Tint keeps a labelled row with its swatch and bucket on one line`);
      if(kind==='mouse')for(const name of ['light','dark']) {
        await send({type:'set_theme',theme:name});await pause(150);
        assert.equal((await control('tint_color')).kind.opaque,true);
        await tap(await middle('.effect-properties .property-color'),kind);
        await wait(`!!document.querySelector('.color-dialog[open]')`);
        assert.equal(await evaluate(`document.querySelector('.color-dialog [data-color-field="3"]').closest('label').hidden`),true,`${name}: Tint has no alpha control`);
        await evaluate(`[...document.querySelectorAll('.color-dialog button')].find(b=>b.textContent==='Use Color').click()`);
        await wait(`!document.querySelector('.color-dialog[open]')`);
        assert.equal((await control('tint_color')).value.value.rgba[3],1);

        const r=await evaluate(`(()=>{const r=document.querySelector('${bucket}').closest('.property-row').getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
        await writeFile(`${directory}/tint-row-${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{x:r.x-8,y:r.y-120,width:r.width+16,height:r.height+128,scale:1}})).data,'base64'));
      }
      const original=(await control('tint_color')).value.value;
      await send({type:'set_color',rgba:TINTS[kind]});
      await tap(await middle(bucket),kind);
      await wait(`(c=>c.value.value.rgba.every((v,i)=>Math.abs(v-${JSON.stringify(TINTS[kind])}[i])<1e-5))(layerApp.state().layer_properties.controls.find(c=>c.key==='tint_color'))`);
      const current=await evaluate('JSON.parse(JSON.stringify(layerApp.state().colors.foreground))');
      assert.deepEqual((await control('tint_color')).value.value,current,`${kind}: the bucket applies the current colour to Tint`);
      await invoke('undo');
      assert.deepEqual((await control('tint_color')).value.value,original,`${kind}: one undo step restores the Tint`);
      await invoke('undo');
      assert.equal((await state()).layers.length,count,`${kind}: undo removes the Black & White layer`);
    }
    console.log(`PASS photo edit (${device?'tablet':'desktop'}): Filter › Fill creates maskless fills in both themes and masks a lasso selection in the current colour (sampled inside and outside) with one undo step, and Black & White's labelled Tint row applies the current colour, with mouse, touch and pen; screenshots in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Photo edit state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,status:document.querySelector('#status').textContent,title:layerApp.state().layer_properties.title,
      tool:layerApp.state().layer_tools.tool,layers:layerApp.state().layers.map(l=>[l.id,l.label,l.editing,l.has_mask])},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme});
  }
}
