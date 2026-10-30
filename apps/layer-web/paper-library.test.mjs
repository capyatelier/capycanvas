import assert from 'node:assert/strict';
import {checkTextEntry} from './control-style-fixture.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
import {checkBlobReads} from './blob-read-fixture.mjs';

export async function checkPaperLibrary({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/texture-dialog/web';
  await mkdir(directory,{recursive:true});
  const blobReads=await checkBlobReads(evaluate);
  try{
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const wait=(condition,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20)}poll()})`);
  const texture="layerApp.state().layer_properties.controls.find(c=>c.key==='texture')";
  const library=`(${texture}?.paper?.tiles??[]).flatMap(t=>t.actions).map(a=>a.action).filter(a=>a.op==='rename_paper')`;
  const idle="!layerApp.state().requests.some(r=>r.kind.type==='paper')";
  const revision=()=>evaluate('Number(layerApp.state().document_file.revision)');
  const dialog='#texture-dialog[open]',opened=`!!document.querySelector('${dialog}')`,closed=`!document.querySelector('${dialog}')`;
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const press=async(find,button='left')=>{
    const r=await evaluate(`(()=>{const n=${find};n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mouseMoved','mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...r,button:type==='mouseMoved'?'none':button,buttons:type==='mousePressed'?(button==='right'?2:1):0,clickCount:1});
    await settle();
  };
  const select=selector=>`document.querySelector(${JSON.stringify(selector)})`;
  const caption=label=>`[...document.querySelectorAll('${dialog} .texture-tile')].find(n=>n.querySelector('.texture-caption').textContent===${JSON.stringify(label)})`;
  const tiles=()=>evaluate(`[...document.querySelectorAll('${dialog} .texture-tile')].map(n=>({label:n.querySelector('.texture-caption').textContent,checked:n.getAttribute('aria-checked')==='true',preview:!!n.querySelector('canvas')}))`);
  const tile=find=>evaluate(`(t=>({id:t.id??null,label:t.label,actions:t.actions.map(a=>({op:a.action.op,label:a.label}))}))(${texture}.paper.tiles${find})`);
  const menu=async tile=>{
    await press(tile,'right');await wait(`!!document.querySelector('${dialog} [role=menu]:popover-open')`);
    return evaluate(`[...document.querySelectorAll('${dialog} [role=menu] button')].map(n=>n.textContent)`);
  };
  const choose=label=>press(`[...document.querySelectorAll('${dialog} [role=menu] button')].find(n=>n.textContent===${JSON.stringify(label)})`);
  const paper=await evaluate(`(()=>{for(const l of layerApp.state().layers){layerApp.dispatch({type:'select_layer',id:l.id});if(${texture})return Number(l.id);}return null})()`);
  assert.ok(paper!==null,'The default drawing has a Paper layer');
  await wait(`!!${texture}?.paper?.import`);
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const action of await evaluate(library))await send({type:'effect',action:{op:'delete_paper',id:action.id}});
    await wait(`${idle}&&${library}.length===0`);
    await evaluate(`(async()=>{
      const canvas=new OffscreenCanvas(64,48),context=canvas.getContext('2d');
      for(let y=0;y<48;y++)for(let x=0;x<64;x++){const v=(x*7+y*3)%256;context.fillStyle='rgb('+v+','+v+','+v+')';context.fillRect(x,y,1,1);}
      const file=new File([await canvas.convertToBlob({type:'image/png'})],'Linen scan.png',{type:'image/png'});
      const click=HTMLInputElement.prototype.click;delete window.paperChooser;
      HTMLInputElement.prototype.click=function(){
        if(this.type!=='file'||this.accept!=='image/*')return click.call(this);
        HTMLInputElement.prototype.click=click;window.paperChooser=this.accept;
        const transfer=new DataTransfer();transfer.items.add(file);this.files=transfer.files;this.dispatchEvent(new Event('change'));
      };
    })()`);
    await press(select('#property-texture'));await wait(opened);
    const footer=await evaluate(`(()=>{const f=document.querySelector('${dialog} .dialog-footer'),b=document.querySelector('${dialog} #texture-import'),r=b.getBoundingClientRect(),cancel=document.querySelector('${dialog} #texture-cancel').getBoundingClientRect();return{text:b.textContent,left:r.left-f.getBoundingClientRect().left,height:r.height,first:[...f.querySelectorAll('button')].find(n=>!n.hidden)===b,apart:cancel.left-r.right>8}})()`);
    assert.deepEqual(footer,{text:await evaluate(`${texture}.paper.import.label`),left:20,height:34,first:true,apart:true},'Import… sits at the bottom left of Choose Texture');
    await capture(`texture-library-import-${theme}`);
    await press(select(`${dialog} #texture-import`));
    assert.equal(await evaluate(opened),false,'Import… closes Choose Texture');
    await wait(`${idle}&&${library}.length===1`);
    assert.equal(await evaluate('window.paperChooser'),'image/*','Import chooses an image');
    assert.equal(await evaluate('layerApp.state().document_file.modified'),true,'Import applies the new paper');
    assert.equal(await evaluate(`${texture}.paper.tiles.some(t=>t.label==='Linen scan')`),true,'The imported file name names the paper');
    assert.equal(await evaluate(`${texture}.paper.tiles[${texture}.value.value].label`),'Linen scan','The imported paper becomes the texture');
    const id=(await evaluate(library))[0].id,imported=await revision();
    await press(select('#property-texture'));await wait(opened);
    await evaluate(`window.textureLibraryDialog=${select(dialog)}`);
    assert.deepEqual((await tiles()).find(t=>t.label==='Linen scan'),{label:'Linen scan',checked:true,preview:true},'The imported paper is the current tile, with its preview');
    const linen=(await tile(".find(t=>t.label==='Linen scan')")).actions;
    assert.deepEqual(linen.map(a=>a.op),['rename_paper','delete_paper']);
    assert.deepEqual(await menu(caption('Linen scan')),linen.map(a=>a.label),'Imported papers offer Rename and Delete in their menu');
    await capture(`texture-library-menu-${theme}`);
    await choose(linen.find(a=>a.op==='rename_paper').label);
    const prompt='#paper-rename-dialog[open]',copy="layerApp.app.catalog().native_copy.paper";
    await wait(`!!document.querySelector('${prompt}')`);
    await checkTextEntry(evaluate,'#paper-rename-name',{capture:theme=>capture(`paper-name-${theme}`)});
    const field=()=>evaluate(`(()=>{const f=document.querySelector('${prompt}'),n=f.querySelector('#paper-rename-name'),save=f.querySelector('.suggested-action');return{title:f.querySelector('h2').textContent===${copy}.rename_title,save:save.textContent===${copy}.rename,enabled:!save.disabled,error:f.querySelector('.workspace-error').textContent,value:n.value,focused:document.activeElement===n,selected:n.selectionStart===0&&n.selectionEnd===n.value.length}})()`);
    assert.deepEqual(await field(),{title:true,save:true,enabled:true,error:'',value:'Linen scan',focused:true,selected:true},'Rename opens with the paper name selected');
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});await settle();
    const empty=await field();
    assert.deepEqual([empty.value,empty.enabled,empty.error===await evaluate(`layerApp.app.native_caption({type:'paper_name_error',name:''})`)&&empty.error!==''],['',false,true],'An empty name cannot be saved');
    await capture(`texture-library-rename-${theme}`);
    await call('Input.insertText',{text:'Cotton rag'});await settle();
    assert.deepEqual([(await field()).value,(await field()).enabled],['Cotton rag',true]);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    await wait(`!document.querySelector('${prompt}')`);
    await wait(`${idle}&&${texture}.paper.tiles.some(t=>t.label==='Cotton rag')`);
    assert.equal(await evaluate(`textureLibraryDialog===${select(dialog)}&&textureLibraryDialog.contains(document.activeElement)`),true,'Rename returns to the open dialog');
    await evaluate('layerApp.documents.loadPapers()');
    await wait(`${idle}&&${texture}.paper.tiles.some(t=>t.label==='Cotton rag')`);
    await wait(`${caption('Cotton rag')}?.getAttribute('aria-checked')==='true'`);
    assert.equal(await evaluate(`textureLibraryDialog===${select(dialog)}`),true,'The open dialog follows library changes');
    const paperActions=(await tile(".find(t=>t.label==='Cotton rag')")).actions;
    assert.deepEqual(await menu(caption('Cotton rag')),paperActions.map(a=>a.label),'The renamed paper keeps its menu');
    await choose(paperActions.find(a=>a.op==='delete_paper').label);
    await wait(`${idle}&&${library}.length===0`);
    await wait(`!${caption('Cotton rag')}`);
    assert.equal(await revision(),imported,'Library edits leave the drawing unchanged');
    const own=await tile('[0]');
    assert.deepEqual([own.id,own.actions.map(a=>a.op)],[null,['add_paper']],'Deleting the applied paper leaves the drawing its own copy');
    assert.deepEqual((await tiles())[0],{label:own.label,checked:true,preview:false},'The pending choice falls back to the drawing’s own paper');
    assert.equal(await evaluate(`textureLibraryDialog===${select(dialog)}&&textureLibraryDialog.contains(document.activeElement)`),true,'The dialog stays open with focus on its tiles');
    assert.deepEqual(await menu(`document.querySelector('${dialog} .texture-tile[data-texture-tile="0"]')`),own.actions.map(a=>a.label),'The drawing’s own paper offers Add to Library');
    await capture(`texture-library-own-${theme}`);
    await choose(own.actions[0].label);
    await wait(`${idle}&&${library}.length===1`);
    await wait(`${texture}.paper.tiles[0].id!=null`);
    const added=await tile(`[${texture}.value.value]`);
    assert.deepEqual((await tiles()).filter(t=>t.checked),[{label:added.label,checked:true,preview:true}],'Add to Library keeps the drawing’s paper chosen as a library tile');
    await menu(caption(added.label));await choose(added.actions.find(a=>a.op==='delete_paper').label);
    await wait(`${idle}&&${library}.length===0`);
    await press(select(`${dialog} #texture-cancel`));await wait(closed);
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  }
  console.log('PASS: paper library Import… from Choose Texture, tile-menu Rename and relist in the open dialog, tile-menu Delete and Add to Library, bounded Blob reads in both themes',await blobReads.verify());
  }finally{await blobReads.close();}
}
