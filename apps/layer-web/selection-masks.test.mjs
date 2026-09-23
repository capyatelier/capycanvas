import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Run inside the isolated selection-tools fixture on desktop or Android Chrome.
export async function checkPaintableSelections({call,evaluate,settle,send,invoke,point,at}) {
  const view=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_tools,(_,v)=>typeof v==="bigint"?String(v):v))');
  const colors=await evaluate('JSON.stringify(layerApp.state().colors,(_,v)=>typeof v==="bigint"?String(v):v)');
  await send({type:'select_brush',id:1});
  await send({type:'set_brush_size',value:70});
  await invoke('quick_mask');
  assert.equal((await view()).quick_mask,true);
  await invoke('quick_mask');
  assert.equal((await view()).has_selection,false,'Leaving an untouched Quick Mask retains no selection');
  await invoke('quick_mask');
  assert.equal((await view()).mask_editing.gray,0);
  assert.ok(await evaluate('document.querySelector("#selection-mask-actions").getBoundingClientRect().height>0'));
  assert.ok(await evaluate('[...document.querySelectorAll(".selection-mask-controls > button")].every(n=>n.getBoundingClientRect().height>=44)'));
  await point(at(-45,0));
  for(let x=-30;x<=45;x+=15)await point(at(x,0),'pen','mouseMoved');
  await point(at(45,0),'pen','mouseReleased');
  await send({type:'invoke',command:'swap_mask_colors'});
  assert.equal((await view()).mask_editing.gray,1);
  assert.equal(await evaluate('JSON.stringify(layerApp.state().colors,(_,v)=>typeof v==="bigint"?String(v):v)'),colors,'Mask colors leave artwork colors intact');
  await invoke('swap_mask_colors');
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/selection-web';
  await mkdir(directory,{recursive:true});
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/quick-mask-${theme}.png`,Buffer.from(shot.data,'base64'));
    const center=at(0,0);
    const mask=shot;
    const red=await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${mask.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d');g.drawImage(i,0,0);const scale=c.width/innerWidth;const p=g.getImageData((${center.x}-90)*scale,(${center.y}-35)*scale,180*scale,70*scale).data;let red=0;for(let n=0;n<p.length;n+=4)if(p[n]>p[n+1]+40&&p[n]>p[n+2]+40)red++;return red;})()`);
    assert.ok(red>100,`${theme}: painted mask reaches presented pixels (${red})`);
  }
  await evaluate('document.querySelector("#selection-mask-done").click()');await settle();
  assert.equal((await view()).quick_mask,false);
  assert.equal((await view()).has_selection,true);
  await invoke('save_selection_layer');
  await send({type:'layer',action:{op:'cancel_rename'}});
  const id=await evaluate('String(layerApp.state().layers.find(l=>l.selection_layer).id)');
  // BigInt IDs follow the same native serialization used by real layer buttons.
  await evaluate(`layerApp.dispatch({type:'selection',action:{op:'edit_layer',id:BigInt(${JSON.stringify(id)})}})`);await settle();
  assert.equal(String((await evaluate('String(layerApp.state().layer_tools.mask_editing.layer)'))),id);
  await invoke('clear_selection_mask');
  await invoke('return_to_artwork');
  await evaluate(`layerApp.dispatch({type:'selection',action:{op:'load_layer',id:BigInt(${JSON.stringify(id)}),mode:'new',inverted:false}})`);await settle();
  assert.equal((await view()).has_selection,true,'Loading an empty stored mask retains an explicitly empty selection');
  await invoke('deselect');
  await invoke('reselect');
  assert.equal((await view()).has_selection,true);
  await invoke('deselect');
  console.log('PASS Quick Mask pen coverage, light/dark overlay, independent colors, saved selection edit/load, reselect');
}
