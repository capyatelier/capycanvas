import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkBlenderControls({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/blender-controls/web';
  await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const wait=async expression=>{for(let i=0;i<600;i++){if(await evaluate(expression))return;await new Promise(resolve=>setTimeout(resolve,50));}throw Error(`Timed out: ${expression}`);};
  await wait('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  const saved=await evaluate('layerApp.app.workspace_persistence()'),theme=await evaluate('layerApp.state().settings.theme');
  const presets=await evaluate(`layerApp.app.catalog().brush_categories.filter(c=>['blend','wet_paint','effects'].includes(c.id)).flatMap(c=>c.brushes)`);
  assert.equal(presets.length,20,'all registered Blend families are covered');
  const fixture=structuredClone(saved);fixture.layout.bubbles={bubbles:[],rings:[],kept:[]};
  const reports=[];
  const packets=()=>evaluate('JSON.parse(JSON.stringify(layerApp.app.bubbles(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const take=async preset=>{
    const viewport=await evaluate('[innerWidth,innerHeight]');
    for(const phase of ['down','move','up'])await send({type:'bubble',action:{type:'drag',grab:{kind:'variant',variant:{type:'brush_preset',id:preset.id}},phase,position:[900,450],viewport}});
    const packet=(await packets()).find(p=>p.view.selected&&p.view.kind==='brush');
    assert.ok(packet,`${preset.label}: selected brush bubble exists`);
    return packet;
  };
  try {
    for(const current of ['light','dark']) {
      await send({type:'restore_workspace',workspace:fixture});
      await send({type:'set_theme',theme:current});
      await send({type:'customize',action:{type:'set_panel_visible',panel:'tool_settings',visible:true}});
      await send({type:'move_panel',panel:'tool_settings',target:{kind:'float',position:[20,140]}});
      await evaluate(`(()=>{const workspace=layerApp.state().workspace,panel=workspace.layout.floating.find(g=>g.root.panels?.includes('tool_settings'));panel.width=320;panel.height=760;layerApp.dispatch({type:'restore_workspace',workspace})})()`);await settle();
      for(const preset of presets) {
        await send({type:'select_brush',id:preset.id});
        const packet=await take(preset);
        const fields=await evaluate('layerApp.state().tool_settings'),ids=fields.map(f=>f.id);
        const primary=fields.filter(f=>!f.group_id).map(f=>f.id);
        assert.deepEqual(primary.slice(0,2),['size','flow']);
        assert.equal(primary.at(-1),'smoothing_strength');
        assert.deepEqual(packet.view.rings.map(r=>r.setting),primary.slice(2,-1),`${preset.label}: primary controls match default rings`);
        const blender=fields.findIndex(f=>f.group_id==='tool-setting-group-blender'),tip=fields.findIndex(f=>f.group_id==='tool-setting-group-tip');
        if(blender>=0&&tip>=0)assert.ok(blender<tip,`${preset.label}: Blender precedes Tip`);
        assert.ok(!fields.some(f=>f.group_id==='tool-setting-group-mixing'),`${preset.label}: secondary controls use Blender`);
        if(!ids.includes('average_pickup'))for(const inert of ['paint','dilution'])assert.ok(!ids.includes(inert),`${preset.label}: ${inert} is absent`);
        const rendered=await evaluate(`(()=>{const root=[...document.querySelectorAll('.tool-settings-control')].find(n=>n.getBoundingClientRect().width>0);return {fields:[...root.querySelectorAll('[data-tool-setting]')].map(n=>n.dataset.toolSetting),rings:document.querySelectorAll('.tool-bubble[data-bubble="${packet.view.id}"] .bubble-halo .bubble-band').length}})()`);
        assert.deepEqual(rendered.fields,ids,`${preset.label}: native DOM order matches the model`);
        assert.equal(rendered.rings,packet.view.rings.length,`${preset.label}: every default ring renders`);
        reports.push({theme:current,preset,primary,fields:fields.map(f=>({id:f.id,group:f.group_id})),rings:packet.view.rings.map(r=>r.setting)});
        if(preset.id===presets.at(-1).id){await wait('layerApp.app.brush_ready()');await settle();const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${current}.png`,Buffer.from(shot.data,'base64'));}
        await send({type:'bubble',action:{type:'put_back',id:packet.view.id}});
      }
    }
    const preset=presets[0];await send({type:'select_brush',id:preset.id});
    const primary=await evaluate('layerApp.state().tool_settings.filter(f=>!f.group_id).map(f=>f.id).slice(2,-1)');
    for(const setting of primary)await send({type:'bubble',action:{type:'toggle_ring_setting',brush:preset.id,setting}});
    let persisted=await evaluate('layerApp.app.workspace_persistence()');
    assert.deepEqual(persisted.layout.bubbles.rings.find(r=>r.brush===preset.id).settings,[],'explicit empty ring override persists');
    await send({type:'restore_workspace',workspace:persisted});
    let packet=await take(preset);assert.deepEqual(packet.view.rings,[],'empty override keeps the default rings absent');
    await send({type:'bubble',action:{type:'put_back',id:packet.view.id}});
    await send({type:'bubble',action:{type:'toggle_ring_setting',brush:preset.id,setting:'hardness'}});
    persisted=await evaluate('layerApp.app.workspace_persistence()');
    assert.deepEqual(persisted.layout.bubbles.rings.find(r=>r.brush===preset.id).settings,['hardness'],'custom ring override survives empty restoration');
    await send({type:'restore_workspace',workspace:persisted});
    assert.deepEqual(await evaluate(`layerApp.app.workspace_persistence().layout.bubbles.rings.find(r=>r.brush===${preset.id}).settings`),['hardness']);
    packet=await take(preset);assert.deepEqual(packet.view.rings.map(r=>r.setting),['hardness'],'custom ring survives profile defaults');
    await writeFile(`${directory}/profiles.json`,JSON.stringify(reports,null,2));
    console.log('PASS Blender profiles: 20 presets, both themes, rendered controls/rings, custom and empty overrides');
  } finally {await send({type:'restore_workspace',workspace:saved});await send({type:'set_theme',theme});}
}
