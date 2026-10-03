import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkToolVariations({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await pause(180);};
  const wait=async expression=>{for(let i=0;i<200;i++){if(await evaluate(expression))return;await pause(100);}throw Error(expression);};
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});const r=n.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});const p={x:r.x+r.width/2,y:r.y+r.height/2},hit=document.elementFromPoint(p.x,p.y);if(!n.contains(hit))throw Error('Covered '+${JSON.stringify(selector)}+' by '+hit?.outerHTML);return p})()`);
  let device='mouse',point;
  const pointer=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...point}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...point,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
  };
  const click=async selector=>{await pointer('down',await rect(selector));await pointer('up');await settle();await pause(180);};
  const settleDrawer=()=>wait(`(()=>{const root=document.querySelector('.content-drawer[data-drawer="tool"]');if(!root||root.inert)return false;const viewport=document.querySelector('#workspace'),heights=[...root.querySelectorAll(':scope > .drawer-column')].map(column=>column.scrollHeight),target=layerApp.app.drawer({viewport:[viewport.clientWidth,viewport.clientHeight],column:null,heights,progress:1,from:null,closing:false})?.placement.bounds;if(!target)return false;const actual=root.getBoundingClientRect(),offset=viewport.getBoundingClientRect();return Math.abs(actual.x-offset.x-target.x)<.5&&Math.abs(actual.y-offset.y-target.y)<.5&&Math.abs(actual.width-target.width)<.5&&Math.abs(actual.height-target.height)<.5})()`);
  const menuOpen=()=>evaluate("!!document.querySelector('.panel-context-menu:popover-open')");
  const closeMenu=()=>evaluate("document.querySelector('.panel-context-menu').hidePopover()");
  const model=target=>evaluate(`layerApp.app.context_menu(${JSON.stringify(target)})`);
  const choose=async label=>{
    await evaluate(`(()=>{document.querySelector('[data-variation-choice]')?.removeAttribute('data-variation-choice');const b=[...document.querySelectorAll('.panel-context-menu button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)});if(!b)throw Error(${JSON.stringify(label)});b.dataset.variationChoice='true';})()`);
    await click('[data-variation-choice]');
  };
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/ui/tool-variations';await mkdir(directory,{recursive:true});
  const original=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  await call('Emulation.setDeviceMetricsOverride',{width:1400,height:1000,deviceScaleFactor:1,mobile:false});await settle();
  try {
    for(const theme of ['light','dark'])for(const [workspace,count,slot] of [['photographer',15,'lasso'],['illustrator',17,'manual_selection']]) {
      console.log(`Tool variations: ${workspace}/${theme}`);
      device='mouse';await click(`.workspace-switcher button[data-workspace-id="builtin:workspace:${workspace}"]`);
      await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');await send({type:'set_theme',theme});
      const fixture=await evaluate('layerApp.state().workspace');
      const tiles=fixture.layout.panels.find(p=>p.id==='toolbar').content.tiles;
      assert.equal(tiles.filter(t=>!['divider','color'].includes(t.control.kind)).length,count,`${theme}/${workspace}: compact tool count`);
      const tile=tiles.find(t=>t.control.slot===slot);assert.ok(tile,slot);
      const selector=`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tile.id}"]`,anchor={kind:'tile',panel:'toolbar',tile:tile.id};
      const variants=await model({kind:'tool_variants',anchor}),rows=variants.sections.flat();assert.ok(rows.length>1);
      assert.ok(rows.every(r=>r.action&&r.icon));
      const target={kind:'tile',panel:'toolbar',tile:tile.id},full=await model(target);
      assert.deepEqual(full.sections[0].map(r=>r.label),rows.map(r=>r.label));assert.ok(full.sections.length>1,'secondary menu keeps customization');
      for(device of ['mouse','touch','pen']) {
        await evaluate(`window.retainedVariationTile=document.querySelector(${JSON.stringify(selector)})`);
        await click(`${selector} > .tool-variations`);assert.ok(await menuOpen(),`${device}: corner menu`);
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.panel-context-menu .menu-label')].map(n=>n.textContent)"),rows.map(r=>r.label));
        const alternate=(await model({kind:'tool_variants',anchor})).sections.flat().find(r=>!r.selected);await choose(alternate.label);
        assert.equal(await menuOpen(),false);
        const view=await evaluate(`layerApp.app.panel_view('toolbar').tiles.find(t=>t.id===${tile.id})`);
        assert.equal(view.label,alternate.label);assert.equal(view.icon,alternate.icon);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)})===retainedVariationTile`),true,'variant retains pressed tile');
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector+' > button:first-child > svg')}).dataset.asset`),alternate.icon);
        await send({type:'customize',action:{type:'close_expanded'}});
        await click(`${selector} > button:first-child`);
        assert.ok(await evaluate('layerApp.state().customization.drawer!=null'),`${device}: active click opens drawer`);
        const drawer=await evaluate('layerApp.state().customization.drawer');
        assert.deepEqual(drawer.anchor,anchor);assert.deepEqual(drawer.columns,[['brushes'],['tool_settings']]);
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.content-drawer .brushes-control .tool-groups .tool-choice-name')].map(n=>n.textContent)"),rows.map(r=>r.label));
        await evaluate("window.retainedSlotDrawer=document.querySelector('.content-drawer .brushes-control')");
        for(let choice=0;choice<3;choice++) {
          await settleDrawer();
          const sibling=await evaluate('layerApp.state().customization.drawer.tool_set.groups.find(r=>!r.selected&&r.enabled)');
          await click(`.content-drawer .brushes-control .tool-groups [data-tool-choice="${sibling.label}"]`);
          assert.deepEqual(await evaluate('layerApp.state().customization.drawer.anchor'),anchor,'drawer sibling keeps its origin');
          assert.equal(await evaluate("document.querySelector('.content-drawer .brushes-control')===retainedSlotDrawer"),true,'drawer siblings retain native controls');
          assert.equal(await evaluate(`layerApp.app.panel_view('toolbar').tiles.find(t=>t.id===${tile.id}).icon`),sibling.icon,`${device}: full drawer choice updates tile`);
        }
        await click(`${selector} > button:first-child`);assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,`${device}: active click closes drawer`);
        await pointer('down',await rect(`${selector} > button:first-child`));await pause(620);
        assert.equal(await menuOpen(),device!=='mouse',`${device}: hold menu ownership`);
        await pointer('up');await settle();assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,'hold release suppresses activation');await closeMenu();
        const before=await evaluate('layerApp.state().workspace');
        const start=await rect(`${selector} > button:first-child`),destination=await rect(`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tiles.at(-1).id}"]`);
        await pointer('down',start);await pointer('move',destination);await pause(620);await pointer('up');await settle();
        assert.deepEqual(await evaluate('layerApp.state().workspace'),before,`${device}: motion before hold does not reorder`);
        await pointer('down',start);await pause(620);await pointer('move',destination);await settle();assert.equal(await menuOpen(),false,'drag dismisses menu');await pointer('up');await settle();
        const after=await evaluate('layerApp.state().workspace');assert.notDeepEqual(after,before,`${device}: held same contact reorders`);
        await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await evaluate('layerApp.state().workspace'),before,'one layout undo');
        await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await evaluate('layerApp.state().workspace'),after,'one layout redo');
        await send({type:'invoke',command:'undo_workspace'});
      }
      device='mouse';const p=await rect(`${selector} > button:first-child`);
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});await settle();
      assert.ok(await menuOpen());assert.ok(await evaluate("document.querySelectorAll('.panel-context-menu .menu-label').length")>rows.length);await closeMenu();
      const capture=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${workspace}-${theme}.png`,Buffer.from(capture.data,'base64'));
      await send({type:'restore_workspace',workspace:fixture});
    }
    device='mouse';await click('.workspace-switcher button[data-workspace-id="builtin:workspace:painter"]');await wait('!JSON.parse(layerApp.app.workspace_view()).busy');
    const saved=await evaluate('layerApp.state().workspace'),fixture=structuredClone(saved);
    const entry=fixture.layout.header.zones.flat().find(e=>e.item.control?.command==='drawing_brush');assert.ok(entry);
    entry.item.control={kind:'tool_slot',slot:'drawing'};
    for(const theme of ['light','dark']) {
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      const anchor={kind:'header',id:entry.id},rows=(await model({kind:'tool_variants',anchor})).sections.flat();
      await click(`[data-header-item="${entry.id}"] .tool-variations`);await choose(rows.find(r=>!r.selected).label);
      const view=await evaluate(`layerApp.app.header_view().items.find(i=>i.id===${entry.id})`);
      assert.equal(await evaluate(`document.querySelector('[data-header-item="${entry.id}"] .header-tool > svg').dataset.asset`),view.icon);
      assert.equal(await evaluate(`document.querySelector('[data-header-item="${entry.id}"] .header-tool').getAttribute('aria-label')`),view.label);
    }
    await send({type:'restore_workspace',workspace:saved});
    console.log('PASS: Photo 15/Paint 17 tools, corner/secondary menus, retained icons and sibling drawers, mouse/touch/pen hold/reorder and header variants in both themes');
  } finally {device='mouse';await closeMenu();await click(`.workspace-switcher button[data-workspace-id="${original}"]`);}
}
