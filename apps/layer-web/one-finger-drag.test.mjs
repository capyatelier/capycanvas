import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkOneFingerDrag({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/one-finger-drag/web';
  await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+15000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30)}check()})`);
  const point=selector=>evaluate(`(()=>{const node=document.querySelector(${JSON.stringify(selector)});if(!node)throw Error('Missing '+${JSON.stringify(selector)});node.scrollIntoView({block:'nearest'});const r=node.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  const click=async(selector,button='left')=>{
    const p=await point(selector);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button,buttons:type==='mousePressed'?(button==='left'?1:2):0,clickCount:1});await settle();
  };
  const menu='.panel-context-menu:popover-open';
  const labels=()=>evaluate(`[...document.querySelectorAll('${menu} button')].map(n=>n.menuItem?.label??n.textContent.trim())`);
  const choose=async label=>{
    await evaluate(`(()=>{const n=[...document.querySelectorAll('${menu} button')].find(n=>(n.menuItem?.label??n.textContent.trim())===${JSON.stringify(label)});if(!n)throw Error('Missing menu row '+${JSON.stringify(label)});n.dataset.fingerChoice='true'})()`);
    await click('[data-finger-choice]');
  };
  const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();};
  const shot=async name=>{await settle();await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));};
  const picker=async()=>{
    await wait('layerApp.app.tool_picker()!=null');
    await evaluate(`(()=>{const n=document.querySelector('#tool-search');n.value='One-finger drag';n.dispatchEvent(new Event('input',{bubbles:true}))})()`);await settle();
    assert.deepEqual(await evaluate('layerApp.app.tool_picker().choices.map(c=>c.control)'),[{kind:'command',command:'one_finger_drag'}]);
    await click('.tool-choice input');await click('#confirm-tools');
  };
  const original=await evaluate('layerApp.state().workspace');
  for(const theme of ['light','dark']) {
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
    await send({type:'restore_workspace',workspace:original});await send({type:'set_theme',theme});
    await send({type:'invoke',command:'search_commands'});await click('#command-search');
    await call('Input.insertText',{text:'One-finger drag'});await settle();
    assert.equal(await evaluate('layerApp.state().command_search.results[0].id'),'command.one_finger_drag');
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    await wait('layerApp.app.preferences()?.page==="input"&&document.querySelector("[data-preference=finger_drawing]")!=null');
    assert.equal(await evaluate('document.querySelector("[data-preference=finger_drawing] label").textContent'),'One-finger drag');
    await shot(`settings-${theme}`);
    await click('[data-preference=finger_drawing] summary');
    assert.deepEqual(await evaluate('[...document.querySelectorAll("[data-preference=finger_drawing] [data-choice]")].map(n=>n.textContent)'),['Auto','Draw','Move canvas','Off']);
    await shot(`settings-dropdown-${theme}`);await click('[data-preference=finger_drawing] [data-choice="2"]');
    assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),'move_canvas');await click('#close-settings');
    await click('#header','right');await choose('Customize Title Bar…');
    const from=await point('#header-component-tools');
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...from,button:'left',buttons:1,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:540,y:20,button:'left',buttons:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:540,y:20,button:'left',buttons:0,clickCount:1});await settle();await picker();
    await click('#header-edit-done');
    const id=await evaluate('layerApp.state().workspace.layout.header.zones.flat().find(e=>e.item.control?.command==="one_finger_drag").id');
    const header=`#header-item-${id} .header-tool`;
    await click('.toolbar-controls .tool-tile button','right');await choose('Insert Tools…');await picker();
    const tile=await evaluate('layerApp.state().workspace.layout.panels.find(p=>p.id==="toolbar").content.tiles.find(t=>t.control.command==="one_finger_drag").id');
    const toolbar=`.toolbar-controls [data-tile="${tile}"] button`;
    for(const [size,style] of [['small','small'],['medium','medium'],['large','large']]) {
      await send({type:'customize',action:{type:'header',action:{type:'set_size',size}}});
      await send({type:'customize',action:{type:'set_tile_style',panel:'toolbar',style}});
      for(const [owner,selector] of [['header',header],['toolbar',toolbar]]) {
        for(const [index,value,icon] of [[0,'auto','finger-touch'],[1,'on','finger-draw'],[2,'move_canvas','hand'],[3,'off','finger-touch']]) {
          await click(selector);assert.deepEqual(await labels(),['Auto','Draw','Move canvas','Off']);
          const checked=await evaluate(`[...document.querySelectorAll('${menu} button')].filter(n=>n.getAttribute('aria-checked')==='true').length`);assert.equal(checked,1);
          if(index===0)await shot(`${owner}-${size}-menu-${theme}`);
          await choose(['Auto','Draw','Move canvas','Off'][index]);
          assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),value);
          assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).querySelector('svg').dataset.asset`),icon);
          await shot(`${owner}-${size}-${value}-${theme}`);
        }
        await click(selector,'right');assert.deepEqual((await labels()).slice(0,4),['Auto','Draw','Move canvas','Off']);
        assert.ok((await labels()).includes(owner==='header'?'Customize Title Bar…':'Remove Tool'));await shot(`${owner}-${size}-context-${theme}`);await escape();
        if(size==='medium'){
          const p=await point(selector);await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:3,...p}]});
          await wait(`!!document.querySelector('${menu}')`);await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
          assert.deepEqual((await labels()).slice(0,4),['Auto','Draw','Move canvas','Off']);await escape();
        }
      }
    }
    await call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:10});
    await call('Emulation.setDeviceMetricsOverride',{width:412,height:891,screenWidth:412,screenHeight:891,deviceScaleFactor:1,mobile:true});await settle();
    const phoneWorkspace=await evaluate('layerApp.state().workspace');phoneWorkspace.layout.phone=[];
    await send({type:'restore_workspace',workspace:phoneWorkspace});
    const background=await point('#phone-bar-tools');
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:4,...background}]});
    await wait(`!!document.querySelector('${menu}')`);await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
    await choose('Add Tools…');await picker();
    const phoneTile=await evaluate('layerApp.state().workspace.layout.phone.find(t=>t.control.command==="one_finger_drag").id');
    await shot(`phone-toolbar-${theme}`);await click(`#phone-tool-${phoneTile}`);
    await wait('layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page?.kind==="one_finger_drag"');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('.menu-page:not([hidden]):not([inert]) [data-position]')].map(n=>n.textContent.trim())`),['Auto','Draw','Move canvas','Off']);
    await shot(`phone-toolbar-page-${theme}`);
    await click('.menu-page:not([hidden]):not([inert]) [data-position="[0,2]"]');
    assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),'move_canvas');
    assert.equal(await evaluate('layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page==null'),true);
    assert.equal(await evaluate(`document.querySelector('#phone-tool-${phoneTile} svg').dataset.asset`),'hand');
    await click('#zen-capy');await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  }
  console.log('PASS: One-finger drag command search to Settings, native title-bar Add Tools drag, toolbar picker, primary and secondary menus, all choices/icons/sizes, holds and phone toolbar Add Tools/page in both themes');
}
