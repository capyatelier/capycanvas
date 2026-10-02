import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const edit='.header-menu[data-menu="edit"]';
const dialog='dialog[open]';

export async function checkBlending({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/blending/web';
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const enabled=command=>`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`;
  const invoke=async command=>{await wait(enabled(command));await send({type:'invoke',command});};
  const selected=command=>`!!layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.selected`;
  const rect=expression=>evaluate(`(()=>{const r=(${expression}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async expression=>{const r=await rect(expression);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=1400;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const menuRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const calm=()=>evaluate('new Promise(resolve=>{let last=performance.now(),steady=0;const frame=t=>{steady=t-last<40?steady+1:0;last=t;if(steady>=3)resolve();else requestAnimationFrame(frame);};requestAnimationFrame(frame);})');
  const choose=async(label,kind)=>{
    await calm();
    await tap(await middle(`document.querySelector('${edit} > summary')`),kind);
    await wait(`document.querySelector('${edit}').open`);
    for(const row of ['Blending',label]) {
      await wait(`!!${menuRow(row)}&&!${menuRow(row)}.disabled`);
      await tap(await middle(menuRow(row)),kind);
    }
    await wait(`!document.querySelector('${edit}').open`);
  };
  const capture=async(file,expression)=>{
    const r=await rect(expression);
    const clip={x:Math.max(0,r.x-8),y:Math.max(0,r.y-8),width:r.width+16,height:r.height+16,scale:1};
    await writeFile(`${directory}/${file}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const field=id=>`document.querySelector('${dialog} [data-document-field=${JSON.stringify(id)}]')`;
  const note=`document.querySelector('${dialog} .document-note').textContent`;
  const choice=async(label,value)=>{await evaluate(`(n=>{n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change'));})(${field(label)})`);await settle();};
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  await wait(`(()=>{[...document.querySelectorAll('${dialog} button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('${dialog}');})()`);
  try {
    assert.equal(await evaluate(selected('blend_perceptual')),true,'a new 8-bit drawing blends perceptually');
    for(const [kind,label,command] of [['mouse','Linear Light Blending','blend_linear'],['touch','Perceptual Blending','blend_perceptual'],['pen','Linear Light Blending','blend_linear']]) {
      await choose(label,kind);
      await wait(selected(command));
    }
    await invoke('undo');
    await wait(selected('blend_perceptual'));
    assert.equal(await evaluate(selected('blend_linear')),false,'one undo step restores Perceptual');

    await invoke('document_properties');
    await wait(`[...document.querySelectorAll('${dialog} h3')].some(h=>h.textContent==='Blending'&&h.nextElementSibling?.textContent==='Perceptual')`);
    await capture('properties',`document.querySelector('${dialog}')`);
    await evaluate(`[...document.querySelectorAll('${dialog} button')].find(b=>b.textContent==='Done').click()`);
    await wait(`!document.querySelector('${dialog}')`);

    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      await invoke('new_document');
      await wait(`!!${field('blending')}`);
      assert.equal(await evaluate(`${field('blending')}.value`),'Perceptual');
      assert.equal(await evaluate(note),'Like Photoshop and Clip Studio Paint');
      await capture(`new-document-${name}`,`document.querySelector('${dialog}')`);
      await choice('depth','F16');
      assert.deepEqual(await evaluate(`[${field('blending')}.disabled,${field('blending')}.value,${note}]`),[true,'Linear','Float documents blend in linear light']);
      await capture(`new-document-float-${name}`,`document.querySelector('${dialog}')`);
      await choice('depth','U8');
      assert.deepEqual(await evaluate(`[${field('blending')}.disabled,${field('blending')}.value]`),[false,'Perceptual'],'the choice returns with an 8-bit depth');
      await evaluate(`document.querySelector('${dialog} [data-document-action=cancel]').click()`);
      await wait(`!document.querySelector('${dialog}')`);
    }
    await invoke('new_document');
    await wait(`!!${field('blending')}`);
    await choice('blending','Linear');
    assert.equal(await evaluate(note),'Physically based');
    await evaluate(`document.querySelector('${dialog} [data-document-action=create]').click()`);
    await wait(`!document.querySelector('${dialog}')&&${selected('blend_linear')}`);
  } finally {
    if(theme)await send({type:'set_theme',theme});
  }
}
