import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

export function histogramJourney({evaluate,settle}) {
  const root='[data-scope="histogram"]';
  const json=expression=>evaluate(`JSON.parse(JSON.stringify(${expression},(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const owner=()=>evaluate(`JSON.stringify([String(layerApp.state().document_file.epoch),String(layerApp.state().document_file.revision)])`);
  const poll=async condition=>{const end=Date.now()+150000;while(Date.now()<end){if(await evaluate(condition))return;await settle();await new Promise(r=>setTimeout(r,30));}const state=await json(`(()=>{const s=layerApp.state(),h=s.histogram;return {busy:layerApp.documents.busy(),host_error:s.host_error,histogram:{source:h.source,captured_source:h.captured_source,captured_time:h.captured_time,status:h.status,has_data:h.data!=null,pixels:h.data?.pixels},owner:[String(s.document_file.epoch),String(s.document_file.revision)]};})()`);throw Error(`Histogram subscription timeout: ${condition}; state=${JSON.stringify(state)}`);};
  const visible=async enabled=>{await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'histogram',visible:${enabled}}})`);await settle();};
  const reveal=async()=>{
    await poll('!layerApp.documents.busy()&&layerApp.startupTimes.complete!==null');
    await visible(true);await evaluate(`layerApp.dispatch({type:'customize',action:{type:'close_expanded'}})`);await settle();
    await evaluate(`(()=>{const group=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('histogram'));if(group&&group.active!=='histogram')layerApp.dispatch({type:'select_panel_tab',group:group.id,panel:'histogram'});})()`);await settle();
    await poll(`!!document.querySelector('${root} [data-scope-control="source"]')`);
    await evaluate(`(()=>{const select=document.querySelector('${root} [data-scope-control="source"]');if(select.value!=='0'){select.value='0';select.dispatchEvent(new Event('change',{bubbles:true}));}})()`);await settle();
  };
  const exact=async()=>{
    await reveal();const identity=await owner();const tag=await evaluate('document.documentElement.lang');
    const text=await readFile(new URL(`../../assets/locales/${tag}/resources.ftl`,import.meta.url),'utf8');
    const status=text.split('\n').find(line=>line.startsWith('resources-histogram-exact = '))?.split(' = ').slice(1).join(' = ');assert.ok(status,`${tag} Exact caption exists`);
    await poll(`!layerApp.documents.busy()&&!layerApp.state().host_error&&layerApp.state().histogram.source===0&&layerApp.state().histogram.captured_source==='Visible'&&layerApp.state().histogram.data!=null&&layerApp.state().histogram.status===${JSON.stringify(status)}`);
    assert.equal(await owner(),identity,'Histogram subscription preserves document owner and revision');
    return json('layerApp.state().histogram.data');
  };
  const hide=async()=>{
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'waveform',visible:false}})`);await settle();
    await visible(false);await poll('layerApp.state().histogram.data==null');
  };
  const cancel=async()=>{
    await hide();const identity=await owner();const start=Date.now();await reveal();
    const before=await json('layerApp.state().histogram');await hide();
    await evaluate('new Promise(resolve=>setTimeout(resolve,300))');await settle();
    assert.equal(await evaluate('layerApp.state().histogram.data==null'),true,'Retired hidden subscription rejects late publication');
    assert.equal(await owner(),identity,'Subscription cancellation preserves source document');
    return {ms:Date.now()-start,status_before_hide:before.status,had_data_before_hide:before.data!=null,retired:true};
  };
  const retire=async replaceOwner=>{
    await hide();const epoch=await evaluate('String(layerApp.state().document_file.epoch)');await reveal();await replaceOwner();await settle();
    assert.notEqual(await evaluate('String(layerApp.state().document_file.epoch)'),epoch,'Fixture replaces the actual document owner');
    const data=await exact();return data;
  };
  return {root,reveal,exact,hide,cancel,retire};
}
