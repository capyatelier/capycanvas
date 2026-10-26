import {mkdir,readFile,writeFile} from "node:fs/promises";
import assert from 'node:assert/strict';

// Full labels, the retained compact menu, and whole-item overflow share menus.
export async function checkHeaderControls({call,evaluate,settle}) {
  const click=async(selector,pointerType='mouse')=>{const p=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)}),r=e.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,pointerType});await settle();};
  const select=async(label,container)=>{await evaluate(`(()=>{document.querySelectorAll('[data-header-test]').forEach(button=>delete button.dataset.headerTest);const b=[...document.querySelectorAll(${JSON.stringify(container+' button')})].find(b=>(b.querySelector('.menu-label')?.textContent||b.textContent)===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.dataset.headerTest='true';})()`);await click('[data-header-test]');};
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:870,deviceScaleFactor:1,mobile:false});await settle();};
  await resize(1440);
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/localization-expansion/web';await mkdir(dir,{recursive:true});
  const menus=await evaluate('layerApp.app.editor_models(0,0).application_menus');assert.deepEqual(menus.map(menu=>menu.id),['file','edit','image','layer','select','filter','view','window']);
  const openMenus=()=>evaluate("[...document.querySelectorAll('.header-menu-labels details[open]')].map(menu=>menu.dataset.menu)");
  const hover=async(id,pointerType)=>{
    const point=await evaluate(`(()=>{const r=document.querySelector('[data-menu="${id}"] > summary').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,buttons:0,pointerType});await settle();
  };
  for(const theme of ['light','dark'])for(const device of ['mouse','pen']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);await settle();
    await hover('edit',device);assert.deepEqual(await openMenus(),[],`${theme} ${device}: hover keeps closed menus closed`);
    await click('[data-menu="file"] > summary',device);
    for(const id of ['edit','view','file']) {
      await hover(id,device);assert.deepEqual(await openMenus(),[id],`${theme} ${device}: hover switches one open menu`);
      assert.deepEqual(await evaluate(`[...document.querySelectorAll('[data-menu="${id}"] .menu-label')].map(node=>node.textContent)`),menus.find(menu=>menu.id===id).model.sections.flat().map(item=>item.label));
      await hover(id,device);assert.deepEqual(await openMenus(),[id],`${theme} ${device}: repeated hover retains the menu`);
      if(id==='view') {
        const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/menu-hover-${theme}-${device}.png`,Buffer.from(shot.data,'base64'));
      }
    }
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27,nativeVirtualKeyCode:27});await settle();
    assert.deepEqual(await openMenus(),[],`${theme} ${device}: Escape dismisses the menu`);
    await hover('view',device);assert.deepEqual(await openMenus(),[],`${theme} ${device}: hover after dismissal stays closed`);
  }
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);await settle();
    for(const m of menus){await click(`[data-menu="${m.id}"] > summary`);assert.equal(await evaluate(`document.querySelector('[data-menu="${m.id}"]').open`),true);}
    for(const [width,selector] of [[600,'.header-menu-labels-compact'],[240,'#header-overflow-0']]) {
      await resize(width);
      assert.equal(await evaluate("document.querySelectorAll('.header-item[hidden] details[open],.header-menu-labels[hidden] details[open]').length"),0,'Resizing closes hidden menus');
      assert.ok(await evaluate(`document.activeElement.closest('${selector}')!==null`),'Focus follows the visible menu');
      await click(`${selector} > summary`);
      if(width===240)await select('Menu Labels',`${selector} .popover`);
      assert.deepEqual(await evaluate(`[...document.querySelectorAll('${selector} .menu-label')].map(n=>n.textContent)`),menus.map(m=>m.label));
      for(const m of menus){await select(m.label,`${selector} .popover`);assert.deepEqual(await evaluate(`[...document.querySelectorAll('${selector} .menu-label')].map(n=>n.textContent)`),m.model.sections.flat().map(i=>i.label));await click(`${selector} .submenu-back`);}
      const zoom=await evaluate('layerApp.state().camera.zoom');await select('View',`${selector} .popover`);
      await select(menus.find(m=>m.id==='view').model.sections.flat().find(i=>i.action?.command==='zoom_in').label,`${selector} .popover`);
      assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom);assert.equal(await evaluate(`document.querySelector('${selector}').open`),false);
      await click(`${selector} > summary`);await resize(1440);
      assert.equal(await evaluate("document.querySelectorAll('.header-overflow[open],.header-menu-labels-compact[open]').length"),0);
      const restored=await evaluate(`(()=>{const node=document.activeElement,r=node.getBoundingClientRect();return{tag:node.tagName,id:node.id,className:node.className,item:node.closest('.header-item')?.dataset.kind??null,hidden:!!node.closest('[hidden],[inert]'),disabled:!!node.disabled,bounds:{x:r.x,y:r.y,width:r.width,height:r.height}}})()`);
      await writeFile(`${dir}/header-focus-${theme}-${width}.local.json`,JSON.stringify({theme,width,restored},null,2));
      assert.ok(await evaluate(`document.activeElement.closest('${width===600?'.header-menu-labels':'.header-item:not([hidden])'}')!==null`),'Expansion restores a visible control');
      assert.ok(!restored.hidden&&!restored.disabled&&restored.bounds.width>0&&restored.bounds.height>0,`${theme}/${width}: restored focus is an enabled visible control: ${JSON.stringify(restored)}`);
    }
  }
  const tags=await evaluate('layerApp.app.bootstrap_view().shipped_tags'),expected={};
  for(const tag of tags){const source=await readFile(new URL(`../../assets/locales/${tag}/workspace.ftl`,import.meta.url),'utf8');const text=key=>{const line=source.split('\n').find(line=>line.startsWith(`${key} = `));assert.ok(line,`${tag} ${key}`);return line.slice(key.length+3);};expected[tag]={drag:text('workspace-header-drag-component'),size:text('workspace-header-size'),tools:text('workspace-add-tools-menu'),clock:text('workspace-header-clock'),battery:text('workspace-header-battery')};}
  const restore=async tag=>{await evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:JSON.stringify({...layerApp.state().settings,language:{Explicit:${JSON.stringify(tag)}}})})`);await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if(!layerApp.app.language_pending()&&document.documentElement.lang===${JSON.stringify(tag)})resolve();else if(performance.now()>end)reject(Error('Header language timeout'));else setTimeout(poll,20);}poll();})`);await settle();};
  for(const theme of ['light','dark']){await restore('en');await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();await evaluate(`layerApp.dispatch({type:'customize',action:{type:'header',action:{type:'edit',editing:true}}})`);await settle();await writeFile(`${dir}/header-setup-${theme}.local.json`,JSON.stringify(await evaluate('({customization:layerApp.state().customization,header:layerApp.app.header_view(),bank:document.querySelector("#header-bank")?.outerHTML,headerRoot:document.querySelector("#header").className,dialogs:[...document.querySelectorAll("dialog[open]")].map(node=>node.className),body:document.body.innerText.slice(-1800)})'),null,2));await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function poll(){if(document.querySelector('#header-canvas-info'))resolve();else if(performance.now()>end)reject(Error('Header editor setup '+JSON.stringify({customization:layerApp.state().customization,header:layerApp.app.header_view(),dialogs:[...document.querySelectorAll('dialog[open]')].map(node=>node.className),alerts:[...document.querySelectorAll('.toast,.message')].map(node=>node.textContent)})));else setTimeout(poll,20);}poll();})`);await evaluate(`window.headerLocaleChips=[...document.querySelectorAll('.header-component')];window.headerLocaleSize=document.querySelector('.header-size-choices');window.headerLocaleFooter=document.querySelector('#header-canvas-info');headerLocaleFooter.focus();`);
    for(const tag of tags){await restore(tag);const state=await evaluate(`({same:headerLocaleChips.every((chip,index)=>chip===document.querySelectorAll('.header-component')[index])&&headerLocaleSize===document.querySelector('.header-size-choices')&&headerLocaleFooter===document.querySelector('#header-canvas-info'),focused:document.activeElement===headerLocaleFooter,size:headerLocaleSize.ariaLabel,tools:document.querySelector('#header-component-tools').ariaLabel,chips:headerLocaleChips.map(chip=>({label:chip.ariaLabel,title:chip.title})),status:[...document.querySelectorAll('.header-status-placeholder')].map(node=>node.textContent)})`);assert.ok(state.same&&state.focused,`${tag} retained header bank/native controls`);assert.equal(state.size,expected[tag].size);assert.equal(state.tools,expected[tag].tools);for(const chip of state.chips)assert.equal(chip.title,expected[tag].drag.replace('{ $item }',chip.label),`${tag} typed header drag caption`);assert.ok(state.status.includes(expected[tag].clock)&&state.status.includes(expected[tag].battery),`${tag} localized native-status placeholders`);await evaluate('document.fonts.ready.then(()=>new Promise(resolve=>setTimeout(resolve,150)))');const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/header-bank-${theme}-${tag}.png`,Buffer.from(shot.data,'base64'));}
    await evaluate(`document.querySelector('#header-edit-cancel').click()`);await settle();}
  await restore('en');
  const savedWorkspace=await evaluate('layerApp.app.workspace_persistence()');
  let vectorId=null;
  try {
    await evaluate(`(()=>{const workspace=layerApp.app.workspace_persistence(),header=workspace.layout.header,id=header.next_id++;header.zones=[[{id,item:{kind:'tool',control:{kind:'command',command:'scale_rotate'}}}],[],[]];layerApp.dispatch({type:'restore_workspace',workspace});layerApp.dispatch({type:'layer',action:{op:'new_vector'}});})()`);await settle();
    vectorId=await evaluate('String(layerApp.state().layer_tools.editing_layer.id)');
    const lock=async value=>{await evaluate(`layerApp.dispatch({type:'layer',action:{op:'lock',id:BigInt(${JSON.stringify(vectorId)}),value:${value}}})`);await settle();};
    const editing=async value=>{await evaluate(`layerApp.dispatch({type:'customize',action:{type:'header',action:{type:'edit',editing:${value}}}})`);await settle();};
    const snapshot=()=>evaluate(`(()=>{const spec=layerApp.app.header_view().items.find(item=>item.resolved_control?.kind==='command'&&item.resolved_control.command==='scale_rotate');if(!spec)throw Error('Missing shared ScaleRotate header item');const node=document.querySelector('[data-header-item="'+spec.id+'"] button.header-tool');if(!node)throw Error('Missing ScaleRotate header button');const bounds=node.getBoundingClientRect();return{tooltip:layerApp.state().commands.find(command=>command.id===node.dataset.command)?.tooltip??null,spec:{enabled:spec.enabled,reason:spec.disabled_reason??null,label:spec.label},title:node.title,disabled:node.disabled,visible:!node.closest('[hidden]')&&bounds.width>0&&bounds.height>0};})()`);
    for(const theme of ['light','dark']) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await lock(true);
      const locked=await snapshot();
      assert.ok(locked.visible,`${theme}: the real vector header control is visible`);
      assert.equal(locked.spec.enabled,false);assert.ok(locked.spec.reason,`${theme}: the shared locked item supplies a reason`);
      assert.equal(locked.disabled,true);assert.equal(locked.title,locked.spec.reason,`${theme}: the operational header presents the shared refusal`);
      await editing(true);
      const edit=await snapshot();
      assert.equal(edit.spec.enabled,false);assert.equal(edit.spec.reason,locked.spec.reason);
      assert.equal(edit.disabled,false);assert.equal(edit.title,edit.tooltip||edit.spec.label,`${theme}: layout editing retains its ordinary item hint`);
      await editing(false);
      const restored=await snapshot();
      assert.equal(restored.disabled,true);assert.equal(restored.title,locked.spec.reason,`${theme}: leaving layout editing restores the refusal`);
      await lock(false);
      const unlocked=await snapshot();
      assert.equal(unlocked.spec.enabled,true);assert.equal(unlocked.spec.reason,null);
      assert.equal(unlocked.disabled,false);assert.equal(unlocked.title,unlocked.tooltip||unlocked.spec.label,`${theme}: unlocking restores the ordinary hint`);
      await writeFile(`${dir}/header-shared-reason-${theme}.json`,JSON.stringify({locked,editing:edit,restored,unlocked},null,2));
      const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/header-shared-reason-${theme}.png`,Buffer.from(shot.data,'base64'));
    }
  } finally {
    await evaluate(`layerApp.dispatch({type:'customize',action:{type:'header',action:{type:'edit',editing:false}}})`);await settle();
    if(vectorId!==null){await evaluate(`layerApp.dispatch({type:'layer',action:{op:'delete',id:BigInt(${JSON.stringify(vectorId)})}})`);await settle();}
    await evaluate(`layerApp.dispatch({type:'restore_workspace',workspace:${JSON.stringify(savedWorkspace)}})`);await settle();
  }

  console.log('PASS: all eight full/compact/overflow menus, real Zoom In, resize closure and focus restoration');
}
