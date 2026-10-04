import {histogramJourney} from './histogram-journey.mjs';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const control='.layer-blend',menu='.panel-context-menu';
const opened=`!!document.querySelector('${menu}:popover-open')`;

export async function checkPassThrough({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/pass-through/web';
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const rect=expression=>evaluate(`(()=>{const r=(${expression}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async expression=>{const r=await rect(expression);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const row=label=>`[...document.querySelectorAll('${menu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  let touchId=240;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const blend=()=>evaluate('layerApp.state().layer_tools.editing_layer.blend_label');
  const active=()=>evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  const groups=()=>evaluate(`(()=>{const out=[[]];for(const n of document.querySelector('${menu}').children){if(n.tagName==='HR')out.push([]);else out.at(-1).push(n.querySelector('.menu-label')?.textContent);}return out;})()`);
  const peaks=async()=>{
    await wait('!layerApp.documents.busy()');
    const h=await histogramJourney({evaluate,settle}).exact();
    const channels=h.channels.slice(0,2).map(ch=>Array.from(ch.bins,Number));
    return channels.map(bins=>bins.indexOf(Math.max(...bins)));
  };
  const capture=async(file,...boxes)=>{
    const x=Math.max(0,Math.min(...boxes.map(b=>b.x))-16),y=Math.max(0,Math.min(...boxes.map(b=>b.y))-16);
    const clip={x,y,width:Math.max(...boxes.map(b=>b.x+b.width))+16-x,height:Math.max(...boxes.map(b=>b.y+b.height))+16-y,scale:1};
    await writeFile(`${directory}/${file}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const escape=async()=>{
    await call('Input.dispatchKeyEvent',{type:'rawKeyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  };
  const saved=await evaluate('({theme:layerApp.state().settings.theme ?? null,pass:layerApp.state().settings.pass_through_groups})');
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  const open=async kind=>{await tap(await middle(`document.querySelector('${control}')`),kind);await wait(opened);};
  const settings=async id=>{
    const name=`document.querySelector('.layer-row[data-layer="${id}"] .layer-name')`;
    await evaluate(`${name}.scrollIntoView({block:'nearest'})`);
    const p=await middle(name);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'right',buttons:2,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'right',buttons:0,clickCount:1});
    await wait(opened);
    await tap(await middle(row('Layer Settings')),'mouse');
    await wait(`!!${row('Clip to Layer Below')}`);
  };
  const check=async(label,checked,asset)=>{
    assert.equal(await evaluate(`${row(label)}.getAttribute('aria-checked')`),String(checked));
    assert.ok(await evaluate(`${row(label)}.querySelector('svg[data-asset="${asset}"]')?.childElementCount>0`),'the menu uses a packaged icon');
    assert.equal(await evaluate(`!!${row(label)}.querySelector('.menu-check svg')`),checked);
  };
  try {
    await send({type:'set_color',rgba:[0.9,0.08,0.05,1]});
    await send({type:'effect',action:{op:'insert',effect:'solid_color'}});
    const fill=await active();
    await send({type:'layer',action:{op:'new',group:true,clipped:false}});
    const group=await active();
    assert.equal(await blend(),'Normal','new groups are isolated by default');
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    await send({type:'effect',action:{op:'insert',effect:'black_white'}});
    await send({type:'layer',action:{op:'select',id:group,mask:false}});
    await wait(`!!document.querySelector('${control}')&&!document.querySelector('${control}').disabled&&Number(layerApp.state().layer_tools.editing_layer.id)===${group}`);
    const [red,green]=await peaks();
    assert.ok(red-green>100,`inside an isolated group the adjustment leaves the red fill alone: ${red} ${green}`);
    await open('mouse');
    assert.deepEqual((await groups())[0],['Pass Through','Normal'],'a group\'s menu leads with Pass Through');
    await tap(await middle(row('Pass Through')),'touch');
    await wait(`!${opened}&&layerApp.state().layer_tools.editing_layer.blend_label==='Pass Through'`);
    assert.equal(await evaluate(`document.querySelector('${control} .layer-blend-label').textContent`),'Pass Through');
    const [gray,same]=await peaks();
    assert.ok(Math.abs(gray-same)<=1,`the adjustment now reaches the fill below the group: ${gray} ${same}`);
    await send({type:'invoke',command:'undo'});
    assert.equal(await blend(),'Normal','choosing Pass Through is one undo step');
    await send({type:'invoke',command:'redo'});
    assert.equal(await blend(),'Pass Through');
    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      await pause(150);
      await capture(`group-${name}`,await rect(`document.querySelector('.layer-header')`),await rect(`document.querySelector('.layer-list')??document.querySelector('.layer-header')`));
      await open('pen');
      await capture(`menu-${name}`,await rect(`document.querySelector('${menu}')`),await rect(`document.querySelector('${control}')`));
      await escape();
      await wait(`!${opened}`);
      for(const checked of [true,false]) {
        await settings(group);
        await check('Pass Through',checked,'group-pass-through');
        await check('Clip to Layer Below',false,'clip');
        assert.equal(await evaluate(`${row('Clip to Layer Below')}.disabled`),true,'the fill below cannot be a clipping base');
        await capture(`settings-${name}-pass-${checked}`,await rect(`document.querySelector('${menu}')`));
        await tap(await middle(row('Pass Through')),'mouse');
        await wait(`!${opened}&&layerApp.state().layer_tools.editing_layer.blend_label===${JSON.stringify(checked?'Normal':'Pass Through')}`);
        assert.equal(await evaluate(`document.querySelector('.layer-row[data-layer="${group}"] .layer-meta').textContent`),checked?'':'Pass Through');
      }
    }
    await send({type:'layer',action:{op:'select',id:fill,mask:false}});
    await wait(`Number(layerApp.state().layer_tools.editing_layer.id)===${fill}&&!document.querySelector('${control}').disabled`);
    await open('mouse');
    assert.equal(await evaluate(`!!${row('Pass Through')}`),false,'only groups offer Pass Through');
    await escape();
    await wait(`!${opened}`);

    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    const base=await active();
    await send({type:'layer',action:{op:'rename',id:base,name:'Base {ink} 🎨'}});
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    const clipped=await active();
    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      for(const checked of [false,true]) {
        await settings(clipped);
        await check('Clip to Layer Below',checked,'clip');
        assert.equal(await evaluate(`${row('Clip to Layer Below')}.disabled`),false);
        assert.equal(await evaluate(`document.querySelector('.layer-attachment').getAttribute('aria-label')`),'Clip to Layer Below');
        assert.equal(await evaluate(`document.querySelector('.layer-attachment').title`),`${checked?'Clipped':'Clip'} to Base {ink} 🎨`);
        await capture(`settings-${name}-clip-${checked}`,await rect(`document.querySelector('${menu}')`));
        await tap(await middle(row('Clip to Layer Below')),'mouse');
        await wait(`!${opened}&&layerApp.state().layer_tools.attachment.checked===${!checked}`);
      }
      await send({type:'invoke',command:'undo'});
      assert.equal(await evaluate('layerApp.state().layer_tools.attachment.checked'),true);
      await send({type:'invoke',command:'redo'});
      assert.equal(await evaluate('layerApp.state().layer_tools.attachment.checked'),false);
    }

    await send({type:'open_settings',page:'canvas'});
    await wait(`!!document.querySelector('#setting-pass-through-groups')`);
    assert.equal(await evaluate(`document.querySelector('#setting-pass-through-groups').getAttribute('aria-label')`),'Use Pass Through for new groups');
    if(await evaluate('layerApp.state().settings.pass_through_groups'))await send({type:'preferences',action:{type:'reset',id:'pass_through_groups'}});
    await tap(await middle(`document.querySelector('#setting-pass-through-groups')`),'mouse');
    await wait('layerApp.state().settings.pass_through_groups===true&&document.querySelector("#setting-pass-through-groups").checked');
    for(const name of ['light','dark']) {
      await send({type:'set_theme',theme:name});
      await pause(150);
      await capture(`preferences-${name}`,await rect(`document.querySelector('#setting-pass-through-groups').closest('section')??document.querySelector('#setting-pass-through-groups')`));
    }
    await send({type:'close_settings'});
    await send({type:'layer',action:{op:'new',group:true,clipped:false}});
    await wait(`layerApp.state().layer_tools.editing_layer.blend_label==='Pass Through'`);
    assert.equal(await evaluate(`document.querySelector('${control} .layer-blend-label').textContent`),'Pass Through','New Group passes through');
    console.log(`PASS pass through: fixed Layer Settings labels, packaged icons, checked states, default subtitles and clipping tooltips in both themes; blend behavior and new-group preference; screenshots in ${directory}`);
  } finally {
    await send({type:'preferences',action:{type:'edit',id:'pass_through_groups',value:saved.pass}});
    if(saved.theme)await send({type:'set_theme',theme:saved.theme});
  }
}
