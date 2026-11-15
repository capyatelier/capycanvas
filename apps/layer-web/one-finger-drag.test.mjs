import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readyCanvas} from './canvas-pixels-fixture.mjs';

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
  const choices=['Auto','Draw','Move canvas','Off'];
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
  const searchSettings=async(query,title,page)=>{
    if(!await evaluate('layerApp.app.preferences().searching'))await click('.preferences-search-toggle');
    await click('#settings-search');
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await call('Input.insertText',{text:query});await wait(`layerApp.app.preferences().query===${JSON.stringify(query)}`);
    const index=await evaluate(`layerApp.app.preferences().search_results.findIndex(row=>row.title===${JSON.stringify(title)})`);
    assert.ok(index>=0,`Settings search ${query} contains ${title}`);
    await click(`.preferences-search-results > button:nth-child(${index+1})`);
    await wait(`layerApp.app.preferences().page===${JSON.stringify(page)}&&document.querySelector('.preferences-page:not([hidden])').dataset.page===${JSON.stringify(page)}`);
  };
  await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"switch",id:"builtin:workspace:illustrator"}));null');
  await wait('JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:illustrator"&&!JSON.parse(layerApp.app.workspace_view()).busy');await settle();
  const original=await evaluate('layerApp.state().workspace');
  for(const theme of ['light','dark']) {
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
    await send({type:'restore_workspace',workspace:original});await send({type:'set_theme',theme});
    const commands=await evaluate('layerApp.state().workspace.layout.panels.find(p=>p.id==="commands").content.tiles');
    assert.deepEqual(commands.slice(0,2).map(tile=>tile.control),[{kind:'command',command:'one_finger_drag'},{kind:'divider'}]);
    assert.equal(await evaluate('document.querySelector("[data-panel=commands] .tool-tile:not([hidden])").dataset.tile'),String(commands[0].id));
    assert.equal(await evaluate('document.querySelector("[data-panel=commands] .tool-tile:not([hidden]) svg").dataset.asset'),'finger-draw');
    await evaluate('document.fonts.ready.then(()=>true)');await readyCanvas({evaluate,settle});
    await wait(`(()=>{const layout=layerApp.app.layout(innerWidth,innerHeight);return layout.measurement_requests.every(request=>request.tab_measured)&&layout.groups.every(group=>{const node=document.querySelector('.dock-group[data-group="'+group.id+'"]');if(!node)return false;const bounds=node.getBoundingClientRect();return ['x','y','width','height'].every(key=>Math.abs(bounds[key]-group.bounds[key])<=1/devicePixelRatio);});})()`);
    await shot(`paint-top-bar-${theme}`);
    await send({type:'invoke',command:'search_commands'});await click('#command-search');
    await call('Input.insertText',{text:'One-finger drag'});await settle();
    assert.equal(await evaluate('layerApp.state().command_search.results[0].id'),'command.one_finger_drag');
    await shot(`command-search-${theme}`);
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    await wait('layerApp.app.preferences()?.page==="touch"&&document.querySelector("[data-preference=finger_drawing]")!=null');
    assert.equal(await evaluate('document.querySelector("[data-preference=finger_drawing] label").textContent'),'One-finger drag');
    assert.deepEqual(await evaluate('layerApp.app.preferences().pages.map(page=>[page.id,page.title])'),[['appearance','Appearance'],['color','Color'],['touch','Touch'],['input','Pen & Mouse'],['shortcuts','Keyboard Shortcuts'],['about','About']]);
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".preferences-page[data-page=touch] .settings-group > h3")].map(node=>node.textContent)'),['Touch gestures','Navigation']);
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".preferences-page[data-page=touch] [data-preference]")].map(node=>node.dataset.preference)'),['finger_drawing','rotate_with_two_fingers','quick_pinch_fit','quick_pinch_restore']);
    assert.deepEqual(await evaluate('[...document.querySelector("#triggers-touch-gestures > .preference-group").children].map(node=>node.dataset.preference??node.dataset.trigger)'),['finger_drawing','touch.tap.2','touch.tap.3','touch.tap.4']);
    await shot(`settings-touch-${theme}`);
    await click('[data-settings-page=input]');
    assert.ok(await evaluate('document.querySelector("#setting-wheel-behavior").closest("[data-preference]").checkVisibility()&&document.querySelector("#setting-pan-speed").checkVisibility()&&document.querySelector("#setting-zoom-speed").checkVisibility()'));
    assert.deepEqual(await evaluate('layerApp.app.preferences().shortcut_page.triggers.map(trigger=>[trigger.id,trigger.page]).sort((a,b)=>a[0].localeCompare(b[0]))'),await evaluate('[...document.querySelectorAll("[data-trigger]")].map(node=>[node.dataset.trigger,node.closest(".preferences-page").dataset.page]).sort((a,b)=>a[0].localeCompare(b[0]))'));
    await shot(`settings-pen-and-mouse-${theme}`);
    await point('#triggers-pen-buttons');await shot(`settings-pen-and-mouse-bottom-${theme}`);
    for(const [query,title,page] of [['pinch','Quick pinch to fit','touch'],['two-finger tap','Two-finger tap','touch'],['pan speed','Scroll pan speed','input'],['pass through','Use Pass Through for new groups','color']])await searchSettings(query,title,page);
    await click('[data-settings-page=touch]');
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
      await shot(`title-bar-${size}-${theme}`);
      for(const [owner,selector] of [['header',header],['toolbar',toolbar]]) {
        for(const [index,value] of [[0,'auto'],[1,'on'],[2,'move_canvas'],[3,'off']]) {
          await click(selector);assert.deepEqual(await labels(),[...choices,'More Touch Settings…']);
          assert.equal(await evaluate(`document.querySelectorAll('${menu} hr').length`),1);
          const checked=await evaluate(`[...document.querySelectorAll('${menu} button')].filter(n=>n.getAttribute('aria-checked')==='true').length`);assert.equal(checked,1);
          if(index===0)await shot(`${owner}-${size}-menu-${theme}`);
          await choose(['Auto','Draw','Move canvas','Off'][index]);
          assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),value);
          assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).querySelector('svg').dataset.asset`),'finger-draw');
          await shot(`${owner}-${size}-${value}-${theme}`);
        }
        if(size==='medium'){
          await click(selector);await choose('More Touch Settings…');
          await wait('document.querySelector("#settings").open&&layerApp.app.preferences().page==="touch"');
          await shot(`${owner}-more-touch-settings-${theme}`);await click('#close-settings');
        }
        await click(selector,'right');
        for(const label of choices)assert.ok(!(await labels()).includes(label),`${owner} customization omits ${label}`);
        assert.ok((await labels()).includes(owner==='header'?'Customize Title Bar…':'Remove Tool'));await shot(`${owner}-${size}-context-${theme}`);await escape();
        if(size==='medium'){
          const p=await point(selector);await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:3,...p}]});
          await wait(`!!document.querySelector('${menu}')`);await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
          for(const label of choices)assert.ok(!(await labels()).includes(label),`${owner} hold customization omits ${label}`);
          assert.ok((await labels()).includes(owner==='header'?'Customize Title Bar…':'Remove Tool'));
          await shot(`${owner}-${size}-hold-${theme}`);await escape();
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
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('.menu-page:not([hidden]):not([inert]) [data-position]')].map(n=>n.textContent.trim())`),[...choices,'More Touch Settings…']);
    await shot(`phone-toolbar-page-${theme}`);
    await click('.menu-page:not([hidden]):not([inert]) [data-position="[0,2]"]');
    assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),'move_canvas');
    assert.equal(await evaluate('layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page==null'),true);
    assert.equal(await evaluate(`document.querySelector('#phone-tool-${phoneTile} svg').dataset.asset`),'finger-draw');
    assert.equal(await evaluate('document.querySelector("#phone-finger").dataset.icon'),'layer-finger-draw-symbolic');
    await shot(`phone-capy-strip-${theme}`);await click('#phone-finger');
    await wait('layerApp.state().phone.menu_route.page?.kind==="one_finger_drag"');
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".menu-page:not([hidden]):not([inert]) [data-position]")].map(node=>JSON.parse(node.dataset.position))'),[[0,0],[0,1],[0,2],[0,3],[1,0]]);
    await shot(`phone-capy-five-rows-${theme}`);
    await click('.menu-page:not([hidden]):not([inert]) [data-position="[1,0]"]');
    await wait('document.querySelector("#settings").open&&layerApp.app.preferences().page==="touch"');
    await shot(`phone-more-touch-settings-${theme}`);await click('#settings [data-dialog-part=end]');
    if(await evaluate('layerApp.state().phone.menu'))await click('#zen-capy');
    await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  }
  console.log('PASS: Touch Settings pages/search, fresh Paint leading tile, five-row primary menus and More Touch Settings, fixed icons/all modes/sizes, customization-only context/holds and phone page in both themes');
}
