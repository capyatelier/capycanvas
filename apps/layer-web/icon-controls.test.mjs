import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

// Discover controls from shared models, then inspect their production DOM. An
// asset sheet alone cannot catch a host ignoring icons or a category fallback.
export async function checkIconControls({call,evaluate,settle}, output) {
  await mkdir(`${output}/controls`, {recursive:true});
  const wait = condition => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(${condition})resolve();else if(performance.now()-start>25000)reject(Error(${JSON.stringify(condition)}));else setTimeout(check,40);}check();})`);
  const dispatch = async action => {
    if(action.type==="invoke")assert.ok(await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(action.command)}).enabled`),`${action.command} must be available before invoking`);
    await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  await wait("JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy && layerApp.app.brush_ready()");
  const saved = await evaluate("({theme:layerApp.state().settings.theme,preset:layerApp.state().brush.preset,group:layerApp.state().tool_set.groups.find(g=>g.selected)?.action})");
  console.log("Icon control audit: workspace ready");
  const catalog = await evaluate("layerApp.app.catalog()");
  assert.equal(new Set(catalog.brush_categories.map(c=>c.icon)).size, catalog.brush_categories.length, "Every medium has its own icon");
  const records=[];
  const cancelTransform=async()=>{if(await evaluate("layerApp.state().commands.some(c=>c.id==='cancel_transform'&&c.enabled)"))await dispatch({type:"invoke",command:"cancel_transform"});};
  const show = async panel => {
    await evaluate(`(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}));if(!g)throw Error('Missing panel');if(g.active!==${JSON.stringify(panel)})layerApp.dispatch({type:'select_panel_tab',group:g.id,panel:${JSON.stringify(panel)}});})()`);
    await settle();
  };
  const shot = async (name,selector) => {
    const clip = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:Math.max(0,r.x),y:Math.max(0,r.y),width:Math.min(innerWidth,r.right)-Math.max(0,r.x),height:Math.min(innerHeight,r.bottom)-Math.max(0,r.y),scale:1};})()`);
    assert.ok(clip.width>0&&clip.height>0,name);
    const image=await call("Page.captureScreenshot",{format:"png",clip});
    await writeFile(`${output}/controls/${name}.png`,Buffer.from(image.data,"base64"));
  };
  const check = async name => {
    const view=await evaluate("layerApp.state().tool_set");
    for(const kind of ["groups","subtools"]) {
      const plain=view[kind].filter(i=>i.preview==null);
      assert.equal(new Set(plain.map(i=>i.icon)).size,plain.length,`${name}: ${kind} differentiate choices`);
      for(const item of view[kind]) {
        const node=await evaluate(`(()=>{const b=[...document.querySelectorAll('.dock-group .tool-${kind} [data-tool-choice]')].find(b=>b.dataset.toolChoice===${JSON.stringify(item.label)});if(!b)return null;b.scrollIntoView({block:'nearest'});const svg=b.querySelector('svg[data-asset]'),r=svg?.getBoundingClientRect(),t=b.getBoundingClientRect();return {icon:svg?.dataset.asset,w:r?.width,h:r?.height,inside:r&&r.left>=t.left&&r.right<=t.right&&r.top>=t.top&&r.bottom<=t.bottom,preview:b.querySelector('img')?.complete,label:b.getAttribute('aria-label')};})()`);
        assert.ok(node,`${name}: ${item.label} is rendered`);
        assert.equal(node.icon,item.icon,`${name}: ${item.label} uses model glyph`);
        assert.ok(node.w>=15.9&&Math.abs(node.w-node.h)<.1&&node.inside,`${name}: ${item.label} visible square icon fits its row`);
        assert.equal(node.label,item.label);
        if(item.preview!=null)assert.equal(node.preview,true,`${item.label}: stroke preview retained`);
        records.push({name,kind,label:item.label,icon:item.icon});
      }
    }
    await evaluate("document.querySelector('.dock-group .tool-groups')?.scrollIntoView({block:'nearest'})");
    await settle();
  };
  try {
    for (const theme of ["light","dark"]) {
      await dispatch({type:"set_theme",theme}); await show("brushes");
      for(const category of catalog.brush_categories) {
        await dispatch({type:"select_brush",id:category.brushes[0].id});
        await wait("layerApp.app.brush_ready()");
        const click=async (kind,label)=>{await evaluate(`(()=>{const b=[...document.querySelectorAll('.dock-group .tool-${kind} [data-tool-choice]')].find(b=>b.dataset.toolChoice===${JSON.stringify(label)});b.click();})()`);await settle();await wait("layerApp.app.brush_ready()");};
        await click("groups",category.label);
        const selected=await evaluate("layerApp.state().tool_set.groups.find(g=>g.selected)");
        assert.equal(selected.icon,category.icon);
        for(const brush of category.brushes) {await click("subtools",brush.label);assert.equal(await evaluate("layerApp.state().brush.preset"),brush.id);}
        await check(`${theme}-${category.label}`);
        await shot(`${theme}-medium-${category.icon}`,'.dock-group .brushes-control');
      }
      for(const command of catalog.tool_commands) {
        if(["pen","pencil","brush","airbrush","decoration","blend","liquify","eraser"].includes(command))continue;
        await cancelTransform();
        if(!await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}).enabled`)) {
          assert.equal(command,"scale_rotate","Only content transforms are unavailable on blank artwork");continue;
        }
        await dispatch({type:"invoke",command});
        const groups=await evaluate("layerApp.state().tool_set.groups");
        for(const group of groups) {
          await cancelTransform();
          if(group.action.type==="invoke"&&!await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(group.action.command)}).enabled`)) {
            assert.equal(group.action.command,"scale_rotate");await check(`${theme}-${command}-disabled-transform`);continue;
          }
          await dispatch(group.action);await check(`${theme}-${command}-${group.label}`);
          const subtools=await evaluate("layerApp.state().tool_set.subtools");
          for(const item of subtools) {await dispatch(item.action);assert.ok(await evaluate("layerApp.state().tool_set.subtools.some(i=>i.selected)"));}
          await shot(`${theme}-${command}-${group.icon}`,'.dock-group .brushes-control');
        }
      }
      await dispatch({type:"layer",action:{op:"tool",tool:"lasso_fill"}});
      await check(`${theme}-lasso-fill`);await shot(`${theme}-lasso-fill`,'.dock-group .brushes-control');
      await show("adjustments");
      const categories=await evaluate("layerApp.state().filter_categories.filter(c=>c.id!=null)");
      for(const category of categories) {
        await dispatch({type:"filter_picker",action:{op:"category",category:category.id}});
        const choices=await evaluate("layerApp.state().adjustments");
        for(const choice of choices) {
          assert.notEqual(choice.icon,"adjustments",`${choice.label}: specific effect identity`);
          const found=await evaluate(`(()=>{const b=document.querySelector('[data-effect="'+${JSON.stringify(choice.id)}+'"]');b.scrollIntoView({block:'nearest'});return [...b.querySelectorAll('svg')].map(s=>s.dataset.asset);})()`);
          assert.ok(found.includes(choice.icon),`${choice.label}: filter glyph shown`);
          records.push({name:theme,kind:"filter",label:choice.label,icon:choice.icon});
        }
        await evaluate("document.querySelector('.filter-picker-list').scrollTop=0");await settle();
        await shot(`${theme}-filters-${category.id}`,'.filter-picker');
      }
    }
    for(const panel of ["brushes","adjustments"]) {
      const group=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)})).id`);
      await dispatch({type:"customize",action:{type:"set_column_collapsed",group,collapsed:true}});
      const column=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.groups.some(g=>g.icons.some(i=>i.panel===${JSON.stringify(panel)})))`);
      await dispatch({type:"customize",action:{type:"set_column_collapsed",group:column.id,collapsed:false}});await settle();
      assert.ok(await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.some(g=>g.panels.includes(${JSON.stringify(panel)}))`));
    }
    await writeFile(`${output}/controls.json`,JSON.stringify(records,null,2));
    console.log(`PASS: actual tool category, preset, non-painting mode, and filter controls (${records.length} checks in two themes)`);
  } finally {
    await dispatch({type:"filter_picker",action:{op:"category",category:null}});
    await dispatch({type:"select_brush",id:saved.preset});
    if(saved.group)await dispatch(saved.group);
    await dispatch({type:"set_theme",theme:saved.theme}); await show("brushes");
  }
}
