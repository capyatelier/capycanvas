import {dropdownChoice,captureDropdowns} from './dropdown-fixture.mjs';
import {checkCheckboxes} from './control-style-fixture.mjs';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {png} from './clone-journey.test.mjs';
import {checkProofKeys,checkProofPattern} from './proof-parity.test.mjs';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function chooseToolActionGroup({call,evaluate,settle},group,command) {
  const selector=`[data-tool-action-group="${group}"]`;
  await evaluate(`window.toolGroupControl=document.querySelector(${JSON.stringify(selector)})`);
  await evaluate(dropdownChoice(selector,command));await settle();
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)})===window.toolGroupControl`),true,`${group} dropdown retains identity across choices`);
  assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).value`),command);
  assert.equal(await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}).selected`),true);
}

export async function checkPanelControls({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/panel-controls-all-platforms/web';
  await mkdir(directory,{recursive:true});
  const reports=[];
  const metrics=await evaluate('layerApp.app.catalog().panel_metrics');
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,30)}poll()})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await send({type:'invoke',command});};
  const closeOperation=async()=>{if(await evaluate('layerApp.state().commands.find(c=>c.id==="cancel_transform")?.enabled'))await invoke('cancel_transform');};
  const near=(actual,expected,message)=>assert.ok(Math.abs(actual-expected)<.1,`${message}: ${actual}, expected ${expected}`);
  const float=async(panel,width)=>{
    await send({type:'customize',action:{type:'set_panel_visible',panel,visible:true}});
    await send({type:'move_panel',panel,target:{kind:'float',position:[350,100]}});
    await evaluate(`(()=>{const w=layerApp.state().workspace,g=w.layout.floating.find(g=>g.root.panels?.includes(${JSON.stringify(panel)}));if(!g)throw Error('Floating '+${JSON.stringify(panel)});g.width=${width};g.height=760;layerApp.dispatch({type:'restore_workspace',workspace:w})})()`);await settle();
    near(await evaluate(`document.querySelector('.dock-group[data-panel="${panel}"]').getBoundingClientRect().width`),width,`${panel} requested width`);
  };
  const measure=selector=>evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(selector)});if(!root)throw Error(${JSON.stringify(selector)});const rect=n=>n.getBoundingClientRect().toJSON(),visible=n=>n.checkVisibility()&&n.getBoundingClientRect().width>0&&n.getBoundingClientRect().height>0;const panel=root.closest('.panel');const controls=[...root.querySelectorAll('button,select,input.number-entry,input.palette-entry,.number-control,.number-labels,h3,.toolbar-option-label,.picker-setting>span,.tool-action-choice>span,.property-row>span:first-child,.property-section')].filter(visible).map(n=>({tag:n.tagName,classes:n.className,id:n.dataset.toolSetting??n.dataset.toolAction??n.dataset.propertyKey??n.dataset.pickerSetting??'',rect:rect(n),paddingLeft:getComputedStyle(n).paddingLeft,paddingRight:getComputedStyle(n).paddingRight,radius:getComputedStyle(n).borderTopLeftRadius,shape:getComputedStyle(n).cornerShape,special:!!n.closest('.choice-grid,.gradient-stops,.curve-plot')||['property-color','proof-dial-reset','palette-tile','palette-toggle','palette-choice'].some(c=>n.classList.contains(c))}));const rows=[...root.querySelectorAll('.tool-action-row,.property-toolbar,.property-actions,.gradient-controls,.scope-toolbar,.curve-coordinates,.proof-modes,.palette-search-row')].filter(visible).map(n=>({classes:n.className,rect:rect(n),children:[...n.children].filter(visible).map(rect)}));const divider=document.createElement('span');divider.style.color='var(--divider-line)';document.body.append(divider);const dividerColor=getComputedStyle(divider).color;divider.remove();const checks=[...root.querySelectorAll('label > input[type=checkbox]')].filter(visible).map(n=>{const s=getComputedStyle(n),caption=[...n.parentElement.children].find(c=>c!==n&&visible(c)&&c.textContent.trim());return{label:n.parentElement.textContent.trim(),box:rect(n),caption:caption&&rect(caption),appearance:s.appearance,radius:s.borderTopLeftRadius,border:n.checked?null:s.borderTopWidth+' '+s.borderTopColor}}).filter(c=>c.caption&&c.caption.left>c.box.right);return{root:rect(root),panel:rect(panel),controls,rows,dividerColor,checks,fit:Number(getComputedStyle(root).getPropertyValue('--corner-fit')),labels:[...root.querySelectorAll('.number-panel .number-title,.picker-setting>span,.toolbar-option-label,.property-section')].filter(visible).map(rect),fields:[...root.querySelectorAll('.number-panel')].filter(visible).map(rect),numbers:[...root.querySelectorAll('.number-panel')].filter(visible).map(n=>{const value=n.querySelector('.number-value'),range=document.createRange();range.selectNodeContents(value);return{field:rect(n),label:rect(n.querySelector('.number-title')),track:rect(n.querySelector('.number-slider')),value:visible(value)?rect(value):null,glyph:visible(value)?range.getBoundingClientRect().toJSON():null}})}})()`);
  const capture=async(name,selector)=>{
    await wait('layerApp.app.brush_ready()');await settle();await evaluate('layerApp.app.wait_for_canvas()');await settle();
    if(selector==='.tool-settings-control')for(const field of await evaluate('layerApp.state().tool_settings'))assert.equal(!field.group_id,!field.group,`${name} ${field.id} category identity matches its caption`);
    const geometry=await measure(selector),record={name,...geometry};reports.push(record);
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));
    if(name.endsWith('-242'))await captureDropdowns({call,evaluate,settle},selector,directory,name);
    for(const field of geometry.fields){near(field.x,geometry.panel.x+8,`${name} field leading inset`);near(field.right,geometry.panel.right-8,`${name} field trailing inset`);near(field.height,36,`${name} slider row`);}
    for(const number of geometry.numbers){near(number.label.top,number.field.top,`${name} caption top`);near(number.label.bottom,number.track.top,`${name} caption/track boundary`);near(number.track.left-number.field.left,36,`${name} slider leading reserve`);near(number.field.right-number.track.right,76,`${name} slider trailing reserve`);near(number.track.height,16,`${name} slider hit height`);if(number.value){near(number.value.bottom,number.field.bottom,`${name} readout bottom`);near(number.value.right,number.field.right,`${name} readout right`);near(number.value.width,number.glyph.width+12,`${name} natural readout width`);near(number.glyph.right,number.value.right-6,`${name} suffix anchor`);}}
    for(const label of geometry.labels)near(label.x,geometry.panel.x+8,`${name} plain label leading inset`);
    for(const control of geometry.controls){
      assert.ok(control.rect.left>=geometry.panel.left+8-.1&&control.rect.right<=geometry.panel.right-8+.1,`${name} control containment: ${JSON.stringify(control)}`);
      if(control.classes.split(' ').includes('dropdown'))near(control.rect.height,metrics.control_height,`${name} select height`);
      if(control.tag==='BUTTON'&&!control.special)near(control.rect.height,metrics.control_height,`${name} ${control.classes} button height`);
      if(control.classes.split(' ').includes('number-labels'))near(Number.parseFloat(control.paddingLeft),0,`${name} plain numeric label padding`);
      if(control.classes.split(' ').some(c=>['number-value','number-entry'].includes(c))){near(Number.parseFloat(control.paddingLeft),6,`${name} numeric left padding`);near(Number.parseFloat(control.paddingRight),6,`${name} numeric right padding`);}
      if(control.classes.split(' ').includes('dropdown')||control.classes.includes('number-entry')||control.classes.includes('palette-entry'))near(Number.parseFloat(control.radius),metrics.control_radius*geometry.fit,`${name} control radius`);
      if(control.classes.split(' ').includes('dropdown')&&geometry.fit===1)assert.ok(['squircle','superellipse(2)'].includes(control.shape),`${name} squircle shape: ${control.shape}`);
    }
    for(const row of geometry.rows)for(let i=1;i<row.children.length;i++)near(row.children[i].left-row.children[i-1].right,4,`${name} ${row.classes} peer gap`);
    for(const check of geometry.checks){
      near(check.box.width,16,`${name} ${check.label} GTK checkbox width`);near(check.box.height,16,`${name} ${check.label} GTK checkbox height`);
      assert.equal(check.appearance,'none',`${name} ${check.label} draws the GTK checkbox`);assert.equal(check.radius,'4px',`${name} ${check.label} GTK checkbox corner`);
      if(check.border)assert.equal(check.border,`2px ${geometry.dividerColor}`,`${name} ${check.label} GTK unchecked border`);
      near(check.caption.left-check.box.right,4,`${name} ${check.label} GTK checkbox caption gap`);
    }
    if(selector.startsWith('[data-scope=')){
      const target=await evaluate(`document.querySelector(${JSON.stringify(selector)}).querySelector('.scope-log').getBoundingClientRect().height`);
      near(target,metrics.control_height,`${name} Log counts target`);
    }
    if(selector==='.effect-properties'){
      const gaps=await evaluate(`(()=>{const root=document.querySelector('.effect-properties'),heading=root.querySelector('h3'),metadata=root.querySelector('.properties-header'),following=[...root.children].find(n=>n!==heading&&n!==metadata&&n.checkVisibility()&&n.getBoundingClientRect().height>0);return metadata.checkVisibility()?{afterHeading:metadata.getBoundingClientRect().top-heading.getBoundingClientRect().bottom,beforeControls:following?.getBoundingClientRect().top-metadata.getBoundingClientRect().bottom}:null})()`);
      if(gaps){near(gaps.afterHeading,4,`${name} heading-after gap`);near(gaps.beforeControls,6,`${name} metadata/control group gap`);}
      const addFilter=await evaluate(`(n=>n.checkVisibility()?n.getBoundingClientRect().width:null)(document.querySelector('#properties-add-filter'))`);
      if(addFilter!=null)assert.equal(addFilter,Math.ceil(addFilter),`${name} Add Filter rounds its natural width up to a whole pixel like GTK`);
    }
    if(selector==='.tool-settings-control'){
      const booleans=await evaluate(`(()=>{const root=document.querySelector('.tool-settings-control');return layerApp.state().tool_actions.filter(a=>a.checkable&&!a.group).map(a=>{const row=root.querySelector('[data-tool-action="'+a.command+'"]'),input=row?.querySelector('input[type=checkbox]'),command=layerApp.state().commands.find(c=>c.id===a.command);return{id:a.command,checkbox:!!input,visible:input?.checkVisibility()??false,checked:input?.checked,selected:command.selected,disabled:input?.disabled,enabled:command.enabled,height:row?.getBoundingClientRect().height}})})()`);
      record.booleans=booleans;
      for(const field of booleans){assert.equal(field.checkbox,true,`${name} ${field.id} uses a native checkbox`);assert.equal(field.visible,true,`${name} ${field.id} checkbox is visible`);assert.equal(field.checked,field.selected,`${name} ${field.id} checked state`);assert.equal(field.disabled,!field.enabled,`${name} ${field.id} enabled state`);near(field.height,metrics.control_height,`${name} ${field.id} checkbox row height`);}
    }
    return geometry;
  };
  const click=async selector=>{
    const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
  };
  const key=async(name,vk,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,code:name==='a'?'KeyA':name,windowsVirtualKeyCode:vk,modifiers});await settle();};
  const editor=async(name,selector)=>{
    await click(`${selector} .number-value`);
    const bounds=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),e=n.querySelector('.number-entry'),r=e.getBoundingClientRect(),t=n.querySelector('.number-slider')?.getBoundingClientRect();return{width:r.width,height:r.height,bottom:r.bottom,right:r.right,field:n.getBoundingClientRect().toJSON(),gap:t?r.left-t.right:null,focused:document.activeElement===e}})()`);
    near(bounds.width,68,`${name} editor width`);near(bounds.bottom,bounds.field.bottom,`${name} editor bottom`);near(bounds.right,bounds.field.right,`${name} editor right`);near(bounds.height,metrics.control_height,`${name} editor height`);assert.equal(bounds.focused,true,`${name} native editor focus`);if(bounds.gap!==null)assert.ok(bounds.gap>=8-.1,`${name} editor/slider clearance`);
    await capture(`${name}-editing`,selector.startsWith('.tool')?'.tool-settings-control':'.effect-properties');await key('Escape',27);
  };
  const dryPrimary=['size','flow','grain_depth','material_hardness'];
  const family=async(name,primary,dry=false,selector='.tool-settings-control')=>{
    const profile=await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(selector)}),fields=layerApp.state().tool_settings;return{fields,rendered:[...root.querySelectorAll('[data-tool-setting]')].map(n=>n.dataset.toolSetting),headings:fields.filter(f=>f.group_id).map(f=>{const n=root.querySelector('[data-tool-setting="'+f.id+'"]'),heading=n.previousElementSibling;return{id:f.id,tag:heading?.tagName,text:heading?.textContent}}),rects:fields.filter(f=>!f.group_id).map(f=>root.querySelector('[data-tool-setting="'+f.id+'"]').getBoundingClientRect().toJSON())}})()`);
    const ids=profile.fields.map(f=>f.id);
    assert.equal(new Set(ids).size,ids.length,`${name} shared fields are unique`);
    assert.deepEqual(profile.rendered,ids,`${name} scoped DOM matches the current family without stale or duplicate fields`);
    assert.deepEqual(profile.fields.filter(f=>!f.group_id).map(f=>f.id),[...primary,'smoothing_strength'],`${name} ungrouped field order`);
    assert.ok(!ids.includes('contact_softness'),`${name} Contact softness is hidden`);
    for(let i=1;i<profile.rects.length;i++)near(profile.rects[i].top-profile.rects[i-1].bottom,4,`${name} primary peer gap`);
    if(dry){
      assert.deepEqual(ids,[...dryPrimary,'smoothing_strength','angle','opacity'],`${name} seven dry fields`);
      for(const[id,group]of[['angle','tool-setting-group-tip'],['opacity','tool-setting-group-blending']]){
        const field=profile.fields.find(f=>f.id===id),heading=profile.headings.find(h=>h.id===id);
        assert.equal(field.group_id,group,`${name} stable ${id} category`);assert.equal(heading.tag,'H3',`${name} ${id} category heading`);assert.equal(heading.text,field.group,`${name} published category label`);
      }
      const smoothing=profile.fields.find(f=>f.id==='smoothing_strength');assert.deepEqual([smoothing.numeric.min,smoothing.numeric.max,smoothing.numeric.scale],[0,1,100],`${name} shared Smoothing numeric range`);
    }
    if(primary.includes('strength'))assert.ok(!ids.includes('flow')&&!ids.includes('opacity'),`${name} one Liquify Strength and no paint Opacity`);
    reports.push({name,profile});
  };
  const numericEdit=async(name,id,text,value,primary,dry=false,root='.tool-settings-control')=>{
    const selector=`${root} [data-tool-setting="${id}"]`,stored=()=>evaluate(`layerApp.state().tool_settings.find(f=>f.id===${JSON.stringify(id)}).value`),originalValue=await stored();
    await evaluate(`window.panelControlsNumber=document.querySelector(${JSON.stringify(selector)})`);
    await click(`${selector} .number-value`);
    assert.equal(await evaluate(`document.activeElement===document.querySelector(${JSON.stringify(selector)}).querySelector('.number-entry')`),true,`${name} ${id} native editor focus`);
    await key('a',65,2);await call('Input.insertText',{text});await key('Enter',13);
    await wait(`Math.abs(layerApp.state().tool_settings.find(f=>f.id===${JSON.stringify(id)})?.value-${value})<.00001`);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)})===window.panelControlsNumber`),true,`${name} ${id} retains field identity`);
    await family(`${name}-${id}-committed`,primary,dry,root);
    await click(`${selector} .number-value`);await key('a',65,2);await call('Input.insertText',{text:value===0?'37':'0'});
    assert.ok(Math.abs((await stored())-value)<.00001,`${name} ${id} draft does not publish`);
    await key('Escape',27);
    assert.ok(Math.abs((await stored())-value)<.00001,`${name} ${id} cancellation preserves the commit`);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).querySelector('.number-entry').hidden`),true,`${name} ${id} Escape closes editor`);
    await send({type:'set_tool_setting',id,value:originalValue});
  };
  const families=[
    ...[['pencil',2],['pointy-pencil',25],['shading-pencil',26],['charcoal',27],['chalk',6],['pastel',17],['crayon',43]].map(([name,id])=>({name:`dry-${name}`,id,primary:dryPrimary,dry:true})),
    {name:'wet-round',id:11,primary:['size','flow','paint','pull'],zero:'paint',edit:['pull','63',.63]},
    {name:'watercolor',id:20,primary:['size','flow','water_load','wet_edge'],zero:'water_load',edit:['wet_edge','37',.37]},
    {name:'bristles',id:35,primary:['size','flow','bristle_load','bristle_scale'],zero:'bristle_load',edit:['bristle_scale','125',1.25]},
    ...[['twirl',13],['pinch',37],['crystals',39]].map(([name,id])=>({name:`liquify-${name}`,id,primary:['size','strength']})),
  ];
  await wait('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  await call('Emulation.setDeviceMetricsOverride',{width:1600,height:1000,deviceScaleFactor:1,mobile:false});await settle();
  await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
  const fixture=png(256,256,(x,y)=>[x,y,255-x,255],4);
  await evaluate(`window.panelControlsPicker=window.showOpenFilePicker;window.showOpenFilePicker=async()=>[{name:'Panel controls.png',getFile:async()=>new File([new Uint8Array(${JSON.stringify(Array.from(fixture))})],'Panel controls.png',{type:'image/png'})}];layerApp.dispatch({type:'invoke',command:'open_document'});`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.state().tabs.some(t=>t.active&&t.width===256&&t.height===256)&&layerApp.app.brush_ready()');
  const original=await evaluate('layerApp.state().workspace'),theme=await evaluate('layerApp.state().settings.theme');
  const tools=['pen','wet-mixing','fill','lasso_fill','enclose_fill','gradient','rectangle_select','polygon_select','auto_select','color_select','selection_brush','tonal_select','pick_visible','zoom','liquify','clone','heal','crop','scale_rotate'];
  try {
    await float('adjustments',320);
    const category='.dock-group[data-panel="adjustments"] .filter-picker-header .dropdown';
    for(const current of ['light','dark']) {
      await send({type:'set_theme',theme:current});
      await captureDropdowns({call,evaluate,settle},'.dock-group[data-panel="adjustments"]',directory,`filters-${current}`);
      await evaluate(`document.querySelector(${JSON.stringify(category)}).click()`);
      assert.equal(await evaluate(`document.querySelector('.panel-context-menu:popover-open [aria-checked="true"]').menuItem.command===document.querySelector(${JSON.stringify(category)}).value`),true,'Filters checks the current category');
      await key('End',35);assert.equal(await evaluate(`document.activeElement===document.querySelector('.panel-context-menu:popover-open button:last-child')`),true,'End reaches the last category');
      await key('Home',36);await key('ArrowDown',40);await key('Escape',27);
      const next=await evaluate('layerApp.state().filter_categories.find(c=>c.id!==layerApp.state().filter_picker.category).id');
      await evaluate(dropdownChoice(category,next??''));await settle();
      assert.equal(await evaluate('layerApp.state().filter_picker.category??null'),next??null,'Picking a category changes shared Filters state');
      assert.equal(await evaluate(`document.querySelectorAll('.panel select').length`),0,'Workspace panels contain no native selects');
    }
    await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','tool_settings'].includes(id)}})`);await settle();
    for(const width of [242,320])for(const current of ['light','dark']){
      await send({type:'set_theme',theme:current});await float('tool_settings',width);
      for(const tool of tools){
        await closeOperation();await invoke(tool==='wet-mixing'?'pen':tool==='pick_visible'?'eyedropper':tool);
        if(tool==='pen'||tool==='wet-mixing')await send({type:'select_brush',id:tool==='pen'?1:24});
        if(tool==='crop')await wait(`!!document.querySelector('.tool-settings-control [data-tool-setting="crop_width"]')`);
        if(tool==='scale_rotate')await wait(`!!document.querySelector('.tool-settings-control [data-tool-action="transform_flip_horizontal"]')`);
        const name=`tool-${tool}-${current}-${width}`,geometry=await capture(name,'.tool-settings-control');
        if(tool==='pen'){
          const primary=['size','flow','hardness'];await family(name,primary);
          if(width===242)await numericEdit(name,'hardness','37',.37,primary);
        }
        if(tool==='wet-mixing'){
          const primary=['size','flow','pull','spread'];await family(name,primary);
          if(width===242)await numericEdit(name,'pull','0',0,primary);
        }
        if(tool==='liquify'){
          const primary=['size','strength'];await family(name,primary);
          if(width===242)await numericEdit(name,'strength','0',0,primary);
        }
        if(tool==='zoom'){
          const rows=await evaluate(`(()=>{const root=document.querySelector('.tool-settings-control');return ['[data-toolbar-choice="zoom-click"]','[data-toolbar-choice="zoom-drag"]','[data-tool-action="center_zoom_clicks"]'].map(s=>root.querySelector(s).getBoundingClientRect().toJSON())})()`);
          for(let i=1;i<rows.length;i++)near(rows[i].top-rows[i-1].bottom,4,`${name} vertical peer gap`);
        }
        if(tool==='pick_visible'){
          const fields=geometry.controls.filter(c=>c.classes.split(' ').includes('dropdown'));assert.equal(fields.length,2);near(fields[0].rect.left,fields[1].rect.left,`${name} shared label column`);near(fields[0].rect.right,fields[1].rect.right,`${name} aligned field ends`);
        }
        if(tool==='crop')for(const command of ['crop_delete_cropped_pixels','crop_straighten']){
          const selected=()=>evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}).selected`),before=await selected();
          const selector=`.tool-settings-control [data-tool-action="${command}"] input[type=checkbox]`;
          await click(selector);assert.equal(await selected(),!before,`${name} ${command} native checkbox activation`);
          await click(selector);assert.equal(await selected(),before,`${name} ${command} native checkbox restoration`);
        }
        if(tool==='pen')await editor(name,'.tool-settings-control [data-tool-setting="size"]');
        if(tool==='tonal_select'){
          await click('.tool-settings-control [data-toolbar-segment="tonal-tones-5"]');
          await capture(`tool-tonal-range-${current}-${width}`,'.tool-settings-control');
          near(await evaluate(`document.querySelector('.tool-settings-control .range-control').getBoundingClientRect().height`),28,`${name} specialized tonal interval`);
        }
        if(tool==='scale_rotate'){
          const rows=geometry.rows.filter(r=>r.classes.includes('tool-action-row'));assert.ok(rows.length>=3,'Transform pairs share rows');near(rows[1].rect.top-rows[0].rect.bottom,4,`${name} vertical/horizontal action gap`);
          await invoke('transform_warp');await wait(`!!document.querySelector('.tool-settings-control [data-tool-action-group="transform-warp-grid"]')`);await capture(`tool-warp-${current}-${width}`,'.tool-settings-control');
        }
      }
      for(const spec of families){
        await closeOperation();await send({type:'select_brush',id:spec.id});
        assert.equal(await evaluate('layerApp.state().brush.preset'),spec.id,`${spec.name} active preset`);
        const name=`tool-${spec.name}-${current}-${width}`;
        await capture(name,'.tool-settings-control');await family(name,spec.primary,!!spec.dry);
        if(width===242&&spec.name==='dry-pencil'){
          for(const[id,text,value]of[['size','73',73],['flow','37',.37],['grain_depth','200',2],['material_hardness','40',.4],['angle','45',Math.PI/4],['opacity','63',.63],['grain_depth','0',0]])await numericEdit(name,id,text,value,spec.primary,true);
        }
        if(width===242&&spec.zero)await numericEdit(name,spec.zero,'0',0,spec.primary);
        if(width===242&&spec.edit)await numericEdit(name,...spec.edit,spec.primary);
      }
    }
    const overflowWorkspace=await evaluate('layerApp.state().workspace');
    await send({type:'customize',action:{type:'set_panel_visible',panel:'tool_settings',visible:false}});
    if(!await evaluate('!!document.querySelector("[data-toolbar-component=tool_options]")')){
      await send({type:'customize',action:{type:'insert_tools',panel:'toolbar',before:null}});
      await send({type:'customize',action:{type:'picker_select',control:{kind:'tool_options'},selected:true}});
      await send({type:'customize',action:{type:'confirm_tools'}});
    }
    for(const current of ['light','dark']){
      await send({type:'set_theme',theme:current});await send({type:'select_brush',id:2});
      await call('Emulation.setDeviceMetricsOverride',{width:680,height:1000,deviceScaleFactor:1,mobile:false});await settle();
      const more='[data-toolbar-component=tool_options] > .toolbar-more';await wait(`document.querySelector(${JSON.stringify(more)})?.checkVisibility()`);
      const inline=await evaluate(`(()=>{const root=document.querySelector('[data-toolbar-component=tool_options]');return [...root.querySelectorAll('[data-toolbar-setting]')].filter(n=>n.checkVisibility()).map(n=>n.dataset.toolbarSetting)})()`);
      assert.ok(inline.length<6,`${current} narrow options overflow instead of clipping all six dry fields`);
      await click(more);await wait('!!document.querySelector(".content-drawer[data-drawer=tool]")');
      await evaluate('new Promise((resolve,reject)=>{let last="",stable=0;const end=performance.now()+30000;function poll(){const n=document.querySelector(".content-drawer[data-drawer=tool]");if(n&&!n.inert){const current=JSON.stringify(n.getBoundingClientRect().toJSON());stable=current===last?stable+1:0;last=current;if(stable>=3)return resolve()}if(performance.now()>end)return reject(Error("More drawer geometry did not settle"));requestAnimationFrame(poll)}poll()})');
      const drawer='.content-drawer[data-drawer=tool] .tool-settings-control';
      await family(`dry-overflow-${current}`,dryPrimary,true,drawer);
      await numericEdit(`dry-overflow-${current}`,'opacity','63',.63,dryPrimary,true,drawer);
      const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/dry-overflow-${current}.png`,Buffer.from(shot.data,'base64'));
      await send({type:'customize',action:{type:'close_expanded'}});
    }
    await call('Emulation.setDeviceMetricsOverride',{width:1600,height:1000,deviceScaleFactor:1,mobile:false});
    await send({type:'restore_workspace',workspace:overflowWorkspace});
    await closeOperation();await invoke('hand');await send({type:'customize',action:{type:'set_panel_visible',panel:'tool_settings',visible:false}});
    for(const width of [242,320])for(const current of ['light','dark']){
      await send({type:'set_theme',theme:current});await float('properties',width);
      await capture(`properties-layer-${current}-${width}`,'.effect-properties');
      for(const effect of ['levels','curves','hue_saturation','gradient_fill']){
        await send({type:'effect',action:{op:'insert',effect}});await wait('layerApp.state().layer_properties.controls.length>0');
        await capture(`properties-${effect}-${current}-${width}`,'.effect-properties');
        if(effect==='hue_saturation') {
          await checkCheckboxes(evaluate,'.effect-properties');
          await click('.effect-properties [data-property-key="colorize"] input');
          assert.equal(await evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="colorize").value.value'),true);
          await checkCheckboxes(evaluate,'.effect-properties');
          await capture(`properties-${effect}-checked-${current}-${width}`,'.effect-properties');
          await invoke('undo');
          await editor(`properties-${effect}-${current}-${width}`,'.effect-properties [data-property-key="hue"]');
        }
        await invoke('undo');
      }
      for(const panel of ['histogram','waveform']){
        await send({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:false}});await float(panel,width);
        await capture(`${panel}-${current}-${width}`,`[data-scope="${panel}"]`);await send({type:'customize',action:{type:'set_panel_visible',panel,visible:false}});
      }
    }
    await invoke('new_document');
    await evaluate(createDrawing({width:256,height:256,depth:'F16'}));
    await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.proof_form().hdr&&layerApp.app.brush_ready()');
    for(const width of [242,320])for(const current of ['light','dark']){
      await send({type:'set_theme',theme:current});await float('proof',width);await invoke('soft_proof_setup');
      await wait(`!!document.querySelector('.proof-controls')`);
      await click('.proof-modes button[value="sdr"]');
      await wait('layerApp.app.tone_status().ready||layerApp.app.tone_status().error');assert.equal(await evaluate('layerApp.app.tone_status().error??null'),null);
      await checkProofPattern({evaluate});
      const sdr=await capture(`proof-sdr-${current}-${width}`,'.proof-controls');
      near(sdr.root.left,sdr.panel.left+8,'Proof leading inset');near(sdr.root.right,sdr.panel.right-8,'Proof trailing inset');
      await checkProofKeys({call,evaluate,settle,invoke});
      await click('.proof-modes button[value="print"]');
      await capture(`proof-print-${current}-${width}`,'.proof-controls');
      const print=await evaluate(`(()=>{const root=document.querySelector('.proof-controls'),rect=n=>n.getBoundingClientRect().toJSON();return{modes:rect(root.querySelector('.proof-modes')),body:rect(root.querySelector('.proof-print')),rows:[...root.querySelectorAll('.proof-print .document-size')].map(rect),selects:[...root.querySelectorAll('.proof-print .dropdown')].map(rect),checks:[...root.querySelectorAll('.proof-print input[type=checkbox]')].map(n=>({row:rect(n.closest('label')),rect:rect(n)}))}})()`);
      reports.at(-1).proofPrint=print;
      near(print.body.top-print.modes.bottom,4,'Proof modes/body peer gap');
      for(let i=1;i<print.rows.length;i++)near(print.rows[i].top-print.rows[i-1].bottom,4,'Proof print vertical peer gap');
      for(const select of print.selects){near(select.left,print.selects[0].left,'Proof shared label column');near(select.right,print.selects[0].right,'Proof aligned field ends');}
      for(const check of print.checks)near(check.row.height,metrics.control_height,'Proof checkbox row');
      const bpc='.proof-print input[aria-label="Black point compensation"]',before=await evaluate(`document.querySelector(${JSON.stringify(bpc)}).checked`);
      await click(bpc);assert.equal(await evaluate(`document.querySelector(${JSON.stringify(bpc)}).checked`),!before,'Proof native checkbox activation');
      await click(bpc);assert.equal(await evaluate(`document.querySelector(${JSON.stringify(bpc)}).checked`),before,'Proof native checkbox restoration');
      await click('.proof-modes button[value="off"]');await send({type:'customize',action:{type:'set_panel_visible',panel:'proof',visible:false}});
      await float('palettes',width);
      await wait(`!!document.querySelector('.palettes-panel .palette-swatches>.palette-tile[data-id]')`);
      await click('.palettes-panel .palette-swatches>.palette-tile[data-id]');
      await click('.palettes-panel .palette-name');await call('Input.insertText',{text:'Review Blue'});await key('Enter',13);
      const palette=await capture(`palette-normal-${current}-${width}`,'.palettes-panel');
      const normal=await evaluate(`(()=>{const root=document.querySelector('.palettes-panel'),rect=n=>n.getBoundingClientRect().toJSON(),name=root.querySelector('.palette-name'),label=name.querySelector('span'),range=document.createRange();range.selectNodeContents(label);return{body:rect(root.querySelector('.palette-body')),footer:rect(root.querySelector('.palette-footer')),name:rect(name),glyph:range.getBoundingClientRect().toJSON(),text:label.textContent,font:['fontSize','fontFamily','fontWeight','fontStyle','letterSpacing'].map(k=>getComputedStyle(label)[k]),detail:rect(root.querySelector('.palette-detail')),selector:rect(root.querySelector('.palette-selector')),padding:getComputedStyle(name).paddingRight,tiles:[...root.querySelectorAll('.palette-swatches>.palette-tile')].filter(n=>n.checkVisibility()).slice(0,12).map(rect)}})()`);
      near(normal.body.left,palette.panel.left+8,'Palette leading inset');near(normal.body.right,palette.panel.right-8,'Palette trailing inset');
      near(normal.name.right-normal.glyph.right,6,'Palette readout glyph padding');near(normal.detail.right,normal.name.right-6,'Palette detail/readout glyph edge');
      const adjacent=normal.tiles.filter(t=>Math.abs(t.top-normal.tiles[0].top)<.1);for(let i=1;i<adjacent.length;i++)near(adjacent[i].left-adjacent[i-1].right,4,'Palette horizontal tile gaps');
      const next=normal.tiles.find(t=>t.top>normal.tiles[0].top+.1);assert.ok(next,'Palette has consecutive rows');near(next.top-normal.tiles[0].bottom,4,'Palette vertical tile gap');
      await click('.palettes-panel .palette-name');
      await capture(`palette-name-${current}-${width}-editing`,'.palettes-panel');
      const edit=await evaluate(`(()=>{const n=document.querySelector('.palettes-panel .palette-editor'),s=getComputedStyle(n),r=n.getBoundingClientRect();return{right:r.right,height:r.height,value:n.value,padding:Number.parseFloat(s.paddingRight),align:s.textAlign,font:['fontSize','fontFamily','fontWeight','fontStyle','letterSpacing'].map(k=>s[k]),focused:document.activeElement===n}})()`);
      reports.at(-1).paletteNameAlignment={normal,edit};
      near(edit.height,metrics.control_height,'Palette editor height');near(edit.padding,6,'Palette editor horizontal padding');near(edit.right-edit.padding,normal.glyph.right,'Palette readout/editor glyph edge');assert.equal(edit.align,'right');assert.deepEqual(edit.font,normal.font);assert.equal(edit.value,normal.text);assert.equal(edit.focused,true);
      await key('Escape',27);assert.equal(await evaluate(`document.querySelector('.palettes-panel .palette-name>span').textContent`),normal.text,'Palette native name cancellation');
      await click('.palettes-panel .palette-selector');await capture(`palette-chooser-${current}-${width}`,'.palettes-panel');
      await key('Escape',27);await send({type:'customize',action:{type:'set_panel_visible',panel:'palettes',visible:false}});
    }
    console.log(`PASS: ${reports.length} panel geometry and family records, both themes at 242/320px, numeric commits/cancellation, grouped dry fields and toolbar overflow`);
  } finally {
    await writeFile(`${directory}/controls.json`,JSON.stringify(reports,null,2));
    await evaluate('window.showOpenFilePicker=panelControlsPicker');
    await closeOperation();await send({type:'restore_workspace',workspace:original});await send({type:'set_theme',theme});
  }
}
