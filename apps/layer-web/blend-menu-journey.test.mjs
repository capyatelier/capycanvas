import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const control='.layer-blend',menu='.panel-context-menu';
const opened=`!!document.querySelector('${menu}:popover-open')`;
const GROUPS=[
  ['Normal'],
  ['Darken','Multiply','Color Burn','Linear Burn'],
  ['Lighten','Screen','Color Dodge','Add'],
  ['Overlay','Soft Light','Hard Light','Vivid Light','Linear Light','Pin Light','Hard Mix'],
  ['Difference','Exclusion','Subtract','Divide'],
  ['Hue','Saturation','Color','Luminosity'],
];

export async function checkBlendMenu({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/blend-menu/web';
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const rect=expression=>evaluate(`(()=>{const r=(${expression}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async expression=>{const r=await rect(expression);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const row=label=>`[...document.querySelectorAll('${menu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  let touchId=140;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const blend=()=>evaluate('layerApp.state().layer_tools.editing_layer.blend_label');
  const groups=()=>evaluate(`(()=>{const out=[[]];for(const n of document.querySelector('${menu}').children){if(n.tagName==='HR')out.push([]);else out.at(-1).push(n.querySelector('.menu-label')?.textContent);}return out;})()`);
  const checked=()=>evaluate(`[...document.querySelectorAll('${menu} [aria-checked=true] .menu-label')].map(n=>n.textContent)`);
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  await send({type:'layer',action:{op:'new',group:false,clipped:false}});
  await wait(`!!document.querySelector('${control}')&&document.querySelector('${control}').getBoundingClientRect().width>0&&!document.querySelector('${control}').disabled`);
  const open=async kind=>{await tap(await middle(`document.querySelector('${control}')`),kind);await wait(opened);};
  try {
    for(const [kind,mode] of [['mouse','Multiply'],['touch','Luminosity'],['pen','Linear Light']]) {
      const before=await blend();
      await open(kind);
      assert.deepEqual(await groups(),GROUPS,`${kind}: the shared groups`);
      assert.deepEqual(await checked(),[before],`${kind}: the current mode is checked`);
      await tap(await middle(row(mode)),kind);
      await wait(`!${opened}&&layerApp.state().layer_tools.editing_layer.blend_label===${JSON.stringify(mode)}`);
      assert.equal(await evaluate(`document.querySelector('${control} .layer-blend-label').textContent`),mode,`${kind}: the control shows the mode`);
    }
    await open('mouse');
    await tap(await middle(`document.querySelector('${control}')`),'mouse');
    await wait(`!${opened}`);
    assert.equal(await evaluate(opened),false,'a second press closes the menu');
    await send({type:'invoke',command:'undo'});
    assert.equal(await blend(),'Luminosity','each choice is one undo step');
    const capture=async(file,...boxes)=>{
      const x=Math.max(0,Math.min(...boxes.map(b=>b.x))-16),y=Math.max(0,Math.min(...boxes.map(b=>b.y))-16);
      const clip={x,y,width:Math.max(...boxes.map(b=>b.x+b.width))+16-x,height:Math.max(...boxes.map(b=>b.y+b.height))+16-y,scale:1};
      await writeFile(`${directory}/${file}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
    };
    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      await pause(150);
      await capture(`blend-control-${name}`,await rect(`document.querySelector('.layer-header')`));
      await open('mouse');
      await capture(`blend-menu-${name}`,await rect(`document.querySelector('${menu}')`),await rect(`document.querySelector('${control}')`));
      await call('Input.dispatchKeyEvent',{type:'rawKeyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await wait(`!${opened}`);
    }
  } finally {
    if(theme)await send({type:'set_theme',theme});
  }
}
