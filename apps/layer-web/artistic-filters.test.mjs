import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkArtisticFilters({call, evaluate, settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/artistic-filters/web';
  await mkdir(directory,{recursive:true});
  const wait=async expression=>{const end=Date.now()+120000;while(Date.now()<end){if(await evaluate(expression))return;await settle();await new Promise(r=>setTimeout(r,50));}throw Error(`Artistic filters timeout: ${expression}`);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const click=async selector=>{
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
  };
  const capture=async name=>{const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));};
  const show=async panel=>{
    await send({type:'customize',action:{type:'set_panel_visible',panel,visible:true}});
    await send({type:'customize',action:{type:'close_expanded'}});
    await evaluate(`(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}));if(g&&g.active!==${JSON.stringify(panel)})layerApp.dispatch({type:'select_panel_tab',group:g.id,panel:${JSON.stringify(panel)}});})()`);await settle();
  };
  const edit=async(key,value)=>{
    const selector=`[data-property-key="${key}"]`;
    const target=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});return n.querySelector('.number-value')?${JSON.stringify(selector+' .number-value')}:${JSON.stringify(selector+' input')};})()`);
    await click(target);
    await evaluate(`(()=>{const field=document.querySelector(${JSON.stringify(selector)}),n=field.querySelector('.number-entry')||field.querySelector('input');n.value=${JSON.stringify(String(value))};n.dispatchEvent(new Event('input',{bubbles:true}));})()`);
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();await evaluate('layerApp.app.wait_for_canvas()');
    await wait(`layerApp.state().layer_properties.controls.find(c=>c.key===${JSON.stringify(key)}).value.value===${value}`);
  };
  const canvasShot=async()=>{
    const clip=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x+r.width/2-100,y:r.y+r.height/2-100,width:200,height:200,scale:1}})()`);
    return (await call('Page.captureScreenshot',{format:'png',clip})).data;
  };
  await wait(`!layerApp.documents.busy()&&layerApp.app.brush_ready()`);
  await send({type:'invoke',command:'new_document'});await evaluate(createDrawing({width:256,height:256}));
  await wait(`!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()`);
  await evaluate(`(async()=>{const c=new OffscreenCanvas(256,256),g=c.getContext('2d');const image=g.createImageData(256,256);for(let y=0;y<256;y++)for(let x=0;x<256;x++){const i=(y*256+x)*4;image.data.set([x,y,255-x,255],i);}g.putImageData(image,0,0);const file=new File([await c.convertToBlob()],'Artistic gradient.png',{type:'image/png'});window.artisticPicker=window.showOpenFilePicker;window.showOpenFilePicker=async()=>[{getFile:async()=>file}];layerApp.dispatch({type:'invoke',command:'import_image'});})()`);
  await wait(`layerApp.state().commands.find(c=>c.id==='apply_transform').enabled&&!layerApp.documents.busy()`);
  await evaluate(`window.showOpenFilePicker=artisticPicker;delete window.artisticPicker`);await send({type:'invoke',command:'apply_transform'});
  await wait(`layerApp.state().layer_tools.editing_layer.object&&!layerApp.documents.busy()`);
  const filters=[['dither_spread','Dither Spread',[['spread',64],['cell_size',12],['density',65]]],['scatter','Scatter',[['spread',64],['cell_size',12],['density',65],['seed',7]]],['crystallize','Crystallize',[['cell_size',48],['stretch',60]]]];
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const [id,label,edits] of filters) {
      await show('adjustments');
      await send({type:'filter_picker',action:{op:'category',category:'artistic'}});
      await send({type:'filter_picker',action:{op:'search',query:label}});
      await wait(`(()=>{const n=document.querySelector('[data-effect="${id}"] img');return n&&n.complete&&n.naturalWidth>0})()`);
      assert.ok(await evaluate(`(()=>{const n=document.querySelector('[data-effect="${id}"] img'),c=document.createElement('canvas');c.width=n.naturalWidth;c.height=n.naturalHeight;const g=c.getContext('2d');g.drawImage(n,0,0);const p=g.getImageData(0,0,c.width,c.height).data;for(let i=0;i<p.length;i+=4)if(Math.max(...p.slice(i,i+3))-Math.min(...p.slice(i,i+3))>20)return true;return false})()`),`${id}: colored bundled thumbnail`);
      await capture(`catalog-${id}-${theme}`);
      await click(`[data-effect="${id}"]`);await show('properties');
      await wait(`layerApp.state().layer_properties.controls.some(c=>c.key==='cell_size')`);
      assert.deepEqual(await evaluate('layerApp.state().layer_properties.controls.map(c=>c.key)'),edits.map(([key])=>key));
      const before=await canvasShot();
      for(const [key,value] of edits)await edit(key,value);
      await new Promise(r=>setTimeout(r,250));await settle();
      assert.notEqual(await canvasShot(),before,`${id}: edits change the presented canvas`);
      const [key,value]=edits.at(-1);
      await send({type:'invoke',command:'undo'});
      assert.notEqual(await evaluate(`layerApp.state().layer_properties.controls.find(c=>c.key==='${key}').value.value`),value);
      await send({type:'invoke',command:'redo'});
      assert.equal(await evaluate(`layerApp.state().layer_properties.controls.find(c=>c.key==='${key}').value.value`),value);
      await capture(`properties-${id}-${theme}`);
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      await send({type:'invoke',command:'delete_layer'});
    }
  }
  console.log('PASS: Artistic filters picker, loaded thumbnails, numeric edits, presented output and Undo/Redo in both themes');
}
