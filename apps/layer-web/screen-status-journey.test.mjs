import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const chip=`document.querySelector('#screen-status')`,popup=`document.querySelector('.screen-details')`;

export async function checkScreenStatus({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/screen-status/web';
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){try{if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+': '+JSON.stringify(layerApp.state().screen)));else setTimeout(check,30);}catch(e){reject(e)}}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await send({type:'invoke',command});};
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.click()})()`);
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const fill=async rgba=>{
    await send({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'DisplayP3',rgba}}});
    await invoke('select_all');await invoke('fill_selection');await invoke('deselect');
  };
  await call('Emulation.setEmulatedMedia',{features:[{name:'color-gamut',value:'srgb'},{name:'dynamic-range',value:'standard'}]});
  await wait('window.layerApp&&layerApp.startupTimes.complete!==null&&layerApp.app.brush_ready()',60000);
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready&&!JSON.parse(layerApp.app.workspace_view()).busy',60000);
  await invoke('new_document');await wait('!!document.querySelector("dialog[open]")');
  if(await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Discard Changes")'))await click('Discard Changes');
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U8';})()`);
  await click('Create');
  await wait('!layerApp.state().document_file.busy&&layerApp.app.document_color().space==="DisplayP3"&&layerApp.app.brush_ready()',60000);
  await wait(`layerApp.state().screen.assessment.basis==='System'`);
  await fill([0,1,0,1]);
  await wait(`!${chip}.hidden&&${chip}.textContent==='Colors clipped'`);
  assert.equal(await evaluate(`getComputedStyle(${chip}.querySelector('svg')).display`),'block');
  await capture('clipped');
  await evaluate(`${chip}.click()`);
  await wait(`${popup}.matches(':popover-open')`);
  assert.equal(await evaluate(`document.querySelector('.screen-details-headline').textContent`),'Some colors can’t be shown accurately on this screen');
  assert.equal(await evaluate(`document.querySelector('.screen-details-body').hidden`),true);
  assert.equal(await evaluate(`${popup}.querySelector('input').checked`),false);
  await capture('clipped-details');
  await evaluate(`${popup}.querySelector('input').click()`);
  await wait('layerApp.state().screen.show_clipped');
  await settle();
  await capture('clipped-highlighted');
  await evaluate(`${popup}.querySelector('input').click()`);
  await wait('!layerApp.state().screen.show_clipped');
  await evaluate(`${popup}.hidePopover()`);
  await fill([.5,.5,.5,1]);
  await wait(`layerApp.state().screen.clipped===false&&${chip}.hidden`);
  await capture('fits');
}
