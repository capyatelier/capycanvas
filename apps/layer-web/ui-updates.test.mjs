import assert from 'node:assert/strict';

export async function checkUiUpdates({evaluate}) {
  const result=await evaluate(`(()=>{
    // A separate session leaves the editor's single incremental consumer intact.
    const saved={...layerApp.state().settings,language:{Explicit:'en'},pan_speed:1.5};
    const app=layerApp.app.constructor.create(document.createElement('canvas'), JSON.stringify(saved), ['ja-JP','zh-TW','ko','en']);
    const encode=value=>JSON.stringify(value,(_,v)=>typeof v==='bigint'?{bigint:String(v)}:v&&typeof v==='object'&&!Array.isArray(v)?Object.fromEntries(Object.entries(v).sort(([a],[b])=>a.localeCompare(b))):v);
    const snapshot=value=>{const copy={...value};for(const key of ['histogram','waveform','tonal_histogram'])if(copy[key]){copy[key]={...copy[key]};delete copy[key].plot;delete copy[key].image;}delete copy.scope_colors;delete copy.localization;return copy;};
    try {
      const retained=app.state_update(), original=app.state();
      if(retained.settings.pan_speed!==1.5||retained.settings.language.Explicit!=='en')throw Error('Launch settings were restored after the first view');
      const language=app.language_tag();
      if(language!=='en')throw Error('Launch advertised an unshipped language');
      const bootstrap=app.bootstrap_view();
      if(bootstrap.active_tag!==language||!bootstrap.shipped_tags.includes('en'))throw Error('Startup metadata differs from the session');
      if(!bootstrap.starting_canvas||!bootstrap.drawing_canvas_help)throw Error('Startup labels are missing');
      if(!retained.histogram?.plot||!retained.waveform?.plot||!retained.tonal_histogram?.plot||!('image' in retained.waveform)||!retained.scope_colors||!retained.localization)throw Error('Scope transport omitted derived plots or localization');
      if(encode(snapshot(retained))!==encode(original))throw Error('Initial snapshot differs: '+Object.keys({...retained,...original}).filter(key=>encode(snapshot(retained)[key])!==encode(original[key])).join(','));
      if(typeof retained.revision!=='bigint')throw Error('Revision lost BigInt type');
      if(Object.keys(app.state_update()).length)throw Error('Unchanged state was republished');
      const actions=[{type:'invoke',command:'settings'},
        {type:'preferences',action:{type:'page',page:'canvas'}},
        {type:'preferences',action:{type:'search',query:'pan'}},
        {type:'set_theme',theme:'dark'},{type:'set_theme',theme:'light'},
        {type:'close_settings'},{type:'set_brush_size',value:32},
        {type:'restore_saved_settings',saved:JSON.stringify({...saved,language:'System'})}];
      const fields=[];
      for(const action of actions){
        app.dispatch(action);const update=app.state_update();fields.push(Object.keys(update));
        Object.assign(retained,update);
        if(encode(snapshot(retained))!==encode(app.state()))throw Error('Patch differs after '+encode(action));
        if(Object.keys(app.state_update()).length)throw Error('Repeated publication');
        if(app.language_tag()!==language)throw Error('Preferences replaced the launch language');
        const preferences=app.preferences_cached();
        if(encode(preferences)!==encode(app.preferences()))throw Error('Retained preferences differ');
        if(preferences!==app.preferences_cached())throw Error('Unchanged preferences were rebuilt');
      }
      // Full snapshots remain independent from both the session and patch cache.
      original.settings.theme='invalid';original.tool_set.groups.length=0;
      if(encode(snapshot(retained))!==encode(app.state()))throw Error('Full snapshot mutation leaked');
      return {fields};
    } finally { app.free(); }
  })()`);
  assert(result.fields.slice(0,6).every(fields=>!fields.includes('tool_set')),'Settings retains tool catalogs');
  console.log('PASS: launch settings before first view, English shipping gate, immutable context, and incremental UI transport with BigInts and independent snapshots');
}

export async function checkSettingsUpdates({evaluate, settle}) {
  const saved = await evaluate('layerApp.state().settings');
  const send = action => evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const preference = action => send({type:'preferences',action});
  try {
    await send({type:'invoke',command:'settings'});
    await preference({type:'page',page:'canvas'});
    await checkRangeCommitGuards({evaluate});
    const control = await evaluate("layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).find(r=>r.id==='pan_speed').kind.control");
    for (const value of [control.min, control.max]) {
      await preference({type:'edit',id:'pan_speed',value});
      await settle();
      const actual = await evaluate(`(()=>{const root=document.querySelector('#setting-pan-speed');return{
        open:document.querySelector('#settings').open,
        value:layerApp.state().settings.pan_speed,
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
      await preference({type:'page',page:'canvas'});
      await settle();
      assert.deepEqual(await evaluate("[...document.querySelectorAll('#setting-pan-speed .number-step')].map(b=>b.disabled)"),actual.steps);
    }
    const beforePreedit = await evaluate('layerApp.state().settings.pan_speed');
    await evaluate(`(() => { const number=document.querySelector('#setting-pan-speed');
      number.valueButton.click(); number.entry.value=${JSON.stringify(String(control.min * control.scale))};
      number.entry.dispatchEvent(new CompositionEvent('compositionstart',{data:number.entry.value,bubbles:true}));
      number.entry.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',isComposing:false,bubbles:true,cancelable:true})); })()`);
    assert.equal(await evaluate('layerApp.state().settings.pan_speed'),beforePreedit,'numeric candidate confirmation does not commit the setting');
    assert.equal(await evaluate("document.querySelector('#setting-pan-speed .number-entry').hidden"),false,'numeric preedit remains editable');
    await evaluate(`(() => { const entry=document.querySelector('#setting-pan-speed .number-entry');
      entry.dispatchEvent(new CompositionEvent('compositionend',{data:entry.value,bubbles:true}));
      entry.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true})); })()`);
    assert.equal(await evaluate('layerApp.state().settings.pan_speed'),beforePreedit);
    const malformed='１２＋';
    await evaluate(`(() => { const number=document.querySelector('#setting-pan-speed');
      number.valueButton.click(); number.entry.value=${JSON.stringify(malformed)};
      number.entry.dispatchEvent(new Event('input',{bubbles:true}));
      number.querySelector('.number-step').click();
      number.slider.dispatchEvent(new PointerEvent('pointerdown',{pointerId:987,button:0,bubbles:true,cancelable:true,clientX:0,clientY:0})); })()`);
    assert.deepEqual(await evaluate(`(()=>{const number=document.querySelector('#setting-pan-speed');return [layerApp.state().settings.pan_speed,number.entry.value,number.entry.getAttribute('aria-invalid')];})()`),[beforePreedit,malformed,'true'],'steps and slider contacts preserve a rejected draft and setting');
    await evaluate(`(() => { const number=document.querySelector('#setting-pan-speed');
      number.entry.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
      number.valueButton.click(); number.entry.value='１２＋３';
      number.entry.dispatchEvent(new CompositionEvent('compositionstart',{data:number.entry.value,bubbles:true}));
      number.querySelector('.number-step').click();
      number.slider.value=0; number.slider.dispatchEvent(new Event('input',{bubbles:true})); })()`);
    assert.deepEqual(await evaluate(`(()=>{const number=document.querySelector('#setting-pan-speed');return [layerApp.state().settings.pan_speed,number.entry.value,number.entry.hidden];})()`),[beforePreedit,'１２＋３',false],'steps and range input retain active composition without changing the setting');
    await evaluate(`(() => { const entry=document.querySelector('#setting-pan-speed .number-entry');
      entry.dispatchEvent(new CompositionEvent('compositionend',{data:entry.value,bubbles:true}));
      entry.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true})); })()`);
    await preference({type:'search',query:'pan'});
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

export async function checkRangeCommitGuards({evaluate}) {
  const result=await evaluate(`(async()=>{
    const {createRangeControl}=await import('./range-control.js');
    const control=layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).find(r=>r.id==='pan_speed').kind.control;
    const changes=[],bounds=[{id:'lower',label:'Lower',numeric:control,value:control.min},{id:'upper',label:'Upper',numeric:control,value:control.max}];
    const root=createRangeControl({app:layerApp.app,bounds,label:'Range',icon:()=>document.createElement('span'),onChange:(i,v)=>changes.push([i,v])});
    document.body.append(root);
    try {
      const number=root.querySelector('.number-control'),track=root.querySelector('.interval-track'),handle=root.querySelector('.interval-input');
      const retained=[];
      for(const preedit of [false,true]) {
        const raw=preedit?'１２＋３':'１２＋';number.valueButton.click();number.entry.value=raw;number.entry.dispatchEvent(new Event('input',{bubbles:true}));
        if(preedit)number.entry.dispatchEvent(new CompositionEvent('compositionstart',{data:raw,bubbles:true}));
        handle.value=control.max;handle.dispatchEvent(new Event('input',{bubbles:true}));
        track.dispatchEvent(new PointerEvent('pointerdown',{pointerId:991,button:0,clientX:50,bubbles:true,cancelable:true}));
        retained.push([changes.length,number.entry.value,number.entry.hidden]);
        if(preedit)number.entry.dispatchEvent(new CompositionEvent('compositionend',{data:raw,bubbles:true}));number.cancelEditing();
      }
      return retained;
    } finally {root.dispose();root.remove();}
  })()`);
  assert.deepEqual(result,[[0,'１２＋',false],[0,'１２＋３',false]],'range handles and contacts retain rejected or composing drafts');
}
