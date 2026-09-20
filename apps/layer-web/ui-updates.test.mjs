import assert from 'node:assert/strict';

export async function checkUiUpdates({evaluate}) {
  const result=await evaluate(`(()=>{
    // A separate session leaves the editor's single incremental consumer intact.
    const app=layerApp.app.constructor.create(document.createElement('canvas'));
    const encode=value=>JSON.stringify(value,(_,v)=>typeof v==='bigint'?{bigint:String(v)}:v);
    try {
      const retained=app.state_update(), original=app.state();
      if(encode(retained)!==encode(original))throw Error('Initial snapshot differs');
      if(typeof retained.revision!=='bigint')throw Error('Revision lost BigInt type');
      if(Object.keys(app.state_update()).length)throw Error('Unchanged state was republished');
      const actions=[{type:'invoke',command:'settings'},
        {type:'preferences',action:{type:'page',page:'input'}},
        {type:'preferences',action:{type:'search',query:'pressure'}},
        {type:'set_theme',theme:'dark'},{type:'set_theme',theme:'light'},
        {type:'close_settings'},{type:'set_brush_size',value:32}];
      const fields=[];
      for(const action of actions){
        app.dispatch(action);const update=app.state_update();fields.push(Object.keys(update));
        Object.assign(retained,update);
        if(encode(retained)!==encode(app.state()))throw Error('Patch differs after '+encode(action));
        if(Object.keys(app.state_update()).length)throw Error('Repeated publication');
        const preferences=app.preferences_cached();
        if(encode(preferences)!==encode(app.preferences()))throw Error('Retained preferences differ');
        if(preferences!==app.preferences_cached())throw Error('Unchanged preferences were rebuilt');
      }
      // Full snapshots remain independent from both the session and patch cache.
      original.settings.theme='invalid';original.tool_set.groups.length=0;
      if(encode(retained)!==encode(app.state()))throw Error('Full snapshot mutation leaked');
      return {fields};
    } finally { app.free(); }
  })()`);
  assert(result.fields.slice(0,6).every(fields=>!fields.includes('tool_set')),'Settings retains tool catalogs');
  console.log('PASS: incremental UI transport matches full state, including BigInts and independent snapshots');
}

export async function checkSettingsUpdates({evaluate, settle}) {
  const saved = await evaluate('layerApp.state().settings');
  const send = action => evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const preference = action => send({type:'preferences',action});
  try {
    await send({type:'invoke',command:'settings'});
    await preference({type:'page',page:'input'});
    const control = await evaluate("layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).find(r=>r.id==='pressure').kind.control");
    for (const value of [control.min, control.max]) {
      await preference({type:'edit',id:'pressure',value});
      await settle();
      const actual = await evaluate(`(()=>{const root=document.querySelector('#setting-pressure');return{
        open:document.querySelector('#settings').open,
        value:layerApp.state().settings.pressure_gamma,
        steps:[...root.querySelectorAll('.number-step')].map(b=>b.disabled),
        shown:Number(root.querySelector('.number-entry').getAttribute('aria-valuenow')),
      }})()`);
      assert.equal(actual.open,true);
      assert.equal(actual.value,value);
      assert.equal(actual.shown,value*control.scale);
      assert.deepEqual(actual.steps,[value===control.min,value===control.max]);
      await send({type:'close_settings'});
      await settle();
      assert.equal(await evaluate("document.querySelector('#settings').open"),false);
      await send({type:'invoke',command:'settings'});
      await preference({type:'page',page:'input'});
      await settle();
      assert.deepEqual(await evaluate("[...document.querySelectorAll('#setting-pressure .number-step')].map(b=>b.disabled)"),actual.steps);
    }
    await preference({type:'search',query:'pressure'});
    assert(await evaluate("layerApp.app.preferences().search_results.length > 0"));
    for (const theme of ['dark','light']) {
      await send({type:'set_theme',theme});
      await settle();
      assert.equal(await evaluate('document.body.dataset.theme'),theme);
    }
    console.log('PASS: Settings updates, numeric boundaries, reopen, search and themes');
  } finally {
    await send({type:'close_settings'});
    await send({type:'restore_settings',settings:saved});
    await settle();
  }
}
