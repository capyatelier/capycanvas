import assert from 'node:assert/strict';

// Keyboard-emulating remotes use ordinary key events. Gamepads use the standard
// Gamepad API mapping; the test replaces only the browser's device list.
export async function checkInputDevices({call,evaluate,settle}) {
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const enabled=async id=>(await state()).commands.find(c=>c.id===id).enabled;
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+15000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,20)}check()})`);
  const record=async(id,key,code)=>{
    await send({type:'invoke',command:'keyboard_shortcuts'});
    await send({type:'preferences',action:{type:'begin_shortcut',id}});
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code});await settle();
    await send({type:'preferences',action:{type:'confirm_shortcut',replace:true}});
    await send({type:'close_settings'});
  };
  await send({type:'invoke',command:'add_layer'});
  assert.equal(await enabled('undo'),true);
  await record('command.Undo','AudioVolumeDown','AudioVolumeDown');
  assert.ok((await state()).settings.shortcuts['command.Undo'].some(c=>c.key==='volumedown'));
  for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'AudioVolumeDown',code:'AudioVolumeDown'});await settle();
  await wait("layerApp.state().commands.find(c=>c.id==='redo').enabled");
  await send({type:'preferences',action:{type:'reset_shortcut',id:'command.Undo'}});
  await send({type:'invoke',command:'redo'});

  await evaluate(`(()=>{
    const buttons=[...Array(17)].map(()=>({pressed:false,value:0}));
    window.fakePad={index:0,id:'Capy test pad',connected:true,mapping:'standard',buttons,axes:[0,0,0,0],timestamp:0};
    navigator.getGamepads=()=>[window.fakePad];
    window.dispatchEvent(Object.assign(new Event('gamepadconnected'),{gamepad:window.fakePad}));
  })()`);
  await send({type:'invoke',command:'keyboard_shortcuts'});
  await send({type:'preferences',action:{type:'begin_shortcut',id:'tool_setting.size.increase'}});
  await evaluate('fakePad.buttons[5].pressed=true');await settle();
  await evaluate('fakePad.buttons[5].pressed=false');await settle();
  await send({type:'preferences',action:{type:'confirm_shortcut',replace:true}});
  await send({type:'close_settings'});
  assert.ok((await state()).settings.shortcuts['tool_setting.size.increase'].some(c=>c.key==='gamepad_r1'),'gamepad buttons record like keys');
  await send({type:'invoke',command:'brush'});
  const size=(await state()).brush.diameter;
  await evaluate('fakePad.buttons[5].pressed=true');
  await new Promise(r=>setTimeout(r,800));
  await evaluate('fakePad.buttons[5].pressed=false');await settle();
  const grown=(await state()).brush.diameter;
  assert.ok(grown>size+1,'a held gamepad button repeats its step');
  const camera=(await state()).camera;
  await evaluate('fakePad.axes=[0.9,0,0,-1]');
  await new Promise(r=>setTimeout(r,300));
  await evaluate('fakePad.axes=[0.05,0,0,0.04]');await settle();
  const moved=(await state()).camera;
  assert.ok(moved.translation[0]<camera.translation[0],'the left stick pans');
  assert.ok(moved.zoom>camera.zoom,'the right stick zooms');
  await new Promise(r=>setTimeout(r,200));
  assert.deepEqual((await state()).camera,moved,'stick drift inside the dead zone is ignored');
  await evaluate('fakePad.axes=[1,0,0,0];fakePad.buttons[4].pressed=true');await settle();
  await evaluate(`fakePad.connected=false;navigator.getGamepads=()=>[];window.dispatchEvent(Object.assign(new Event('gamepaddisconnected'),{gamepad:fakePad}))`);await settle();
  const released=(await state()).camera;
  await new Promise(r=>setTimeout(r,200));
  assert.deepEqual((await state()).camera,released,'disconnecting releases the sticks');
  await send({type:'preferences',action:{type:'reset_shortcut',id:'tool_setting.size.increase'}});
  await send({type:'invoke',command:'keyboard_shortcuts'});
  await wait("document.querySelector('#keymap-preset')?.options.length>1");
  await evaluate(`(()=>{const s=document.querySelector('#keymap-preset');s.value='photoshop';s.dispatchEvent(new Event('change'));})()`);await settle();
  assert.equal((await state()).settings.keymap.id,'photoshop');
  assert.ok(await evaluate(`!document.querySelector('#keymap-differences').hidden&&document.querySelector('#keymap-differences summary').textContent.includes('Photoshop')`));
  await send({type:'preferences',action:{type:'edit_shortcut',id:'command.Move'}});
  assert.ok(await evaluate(`document.querySelector('#shortcut-editor').textContent.includes('Photoshop-inspired')`),'the editor names the binding source');
  await send({type:'preferences',action:{type:'close_shortcut_editor'}});
  await evaluate(`window.keymapDownload=null;const create=URL.createObjectURL;URL.createObjectURL=blob=>{window.keymapBlob=blob;return create(blob)};const click=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){if(this.download){window.keymapDownload={name:this.download};return;}return click.call(this)};null`);
  await evaluate(`document.querySelector('#keymap-export-button').click()`);await settle();
  await wait('window.keymapDownload');
  const exported=await evaluate('keymapBlob.text()');
  assert.equal(JSON.parse(exported).keymap.id,'photoshop');
  assert.equal(await evaluate('keymapDownload.name'),'capycanvas-keymap.json');
  await evaluate(`(()=>{const s=document.querySelector('#keymap-preset');s.value='capy';s.dispatchEvent(new Event('change'));})()`);await settle();
  assert.equal((await state()).settings.keymap,undefined);
  await send({type:'preferences',action:{type:'import_keymap',text:exported}});
  await wait("document.querySelector('#keymap-import').open");
  assert.ok(await evaluate(`document.querySelector('#keymap-import').textContent.includes('Photoshop-inspired')`));
  await evaluate(`document.querySelector('#confirm-keymap-import').click()`);await settle();
  await wait("!document.querySelector('#keymap-import').open");
  assert.equal((await state()).settings.keymap.id,'photoshop','importing restores the exported keymap');
  await evaluate(`(()=>{const s=document.querySelector('#keymap-preset');s.value='capy';s.dispatchEvent(new Event('change'));})()`);await settle();
  await send({type:'close_settings'});
  console.log('PASS: remote volume keys, gamepad button capture/repeat, stick navigation, dead zones, disconnect and keymap presets/import/export');
}
