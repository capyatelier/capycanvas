import assert from 'node:assert/strict';
import {vectorHarness} from './vector-journey-fixture.mjs';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkVectorMilestone({call,evaluate,settle}) {
  const {send,invoke,tool,state,stroke,line,key:chord,color,capture,click,noticeShape,until,brush,activeVector}=await vectorHarness({call,evaluate,settle},{directory:'artifacts/vector-milestone/web',lineSteps:20,keyDown:'keyDown'});
  const tap=at=>stroke([at,at]);
  const byText=(scope,text)=>`[...document.querySelectorAll(${JSON.stringify(scope)})].find(e=>e.offsetParent&&e.textContent.trim()===${JSON.stringify(text)})`;
  const dark=([r,g,b])=>Math.max(r,g,b)<100;
  const marked=([r,g,b])=>r+g+b<600;
  const green=([r,g,b])=>g-Math.max(r,b)>40;
  const expect=async(name,at,ink,wanted=true)=>{
    const end=Date.now()+20000;let seen;
    do{seen=await color(at);if(ink(seen)===wanted)return;await new Promise(r=>setTimeout(r,100));}while(Date.now()<end);
    assert.equal(ink(seen),wanted,`${name}: ${seen}`);
  };
  const noticeAction=async id=>{
    await until(`notice ${id}`,`(s.notice?.actions??[]).some(a=>a.id===${JSON.stringify(id)})`);
    const label=await state(`s.notice.actions.find(a=>a.id===${JSON.stringify(id)}).label`);
    await click(byText('.canvas-notice-action',label));
  };
  const shown=selector=>`[...document.querySelectorAll(${JSON.stringify(selector)})].some(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0)`;
  const headerTool=async match=>`document.querySelector('[data-header-item="${await state(`s.workspace.layout.header.zones.flat().find(e=>${match}).id`)}"] .header-tool')`;
  const steady=async selector=>{
    const end=Date.now()+20000;let last=null;
    for(;;){
      const now=await state(`(n=>n&&JSON.stringify(n.getBoundingClientRect()))([...document.querySelectorAll(${JSON.stringify(selector)})].find(n=>n.checkVisibility()))`);
      if(now&&now===last)return;
      assert.ok(Date.now()<end,`${selector} keeps moving`);
      last=now;await new Promise(r=>setTimeout(r,100));
    }
  };
  const inDrawer=async(match,selector,name,act)=>{
    const drawer=!(await state(shown(selector)));
    if(drawer){await click(await headerTool(match));await until(`the drawer for ${name}`,shown(selector));await steady(selector);}
    await act();
    if(drawer){await click(await headerTool(match));await until(`the drawer for ${name} closes`,`!${shown(selector)}`);}
  };
  const fromDrawer=(match,selector,name,done)=>inDrawer(match,selector,name,async()=>{
    await click(`[...document.querySelectorAll(${JSON.stringify(selector)})].find(n=>n.checkVisibility())`);
    await until(name,done);
  });
  const layersPanel='e.item.control?.panel==="layers"';
  const newVectorLayer=(label,name)=>fromDrawer(layersPanel,`.layer-footer [aria-label=${JSON.stringify(label)}]`,name,activeVector);
  const [pencil,gpen,rough]=[await brush('Pencil'),await brush('G-Pen'),await brush('Rough G-Pen')];
  const items='[s.tool_set,...Object.values(s.tool_panels??{}),s.customization?.drawer?.tool_set].filter(Boolean).flatMap(v=>[...v.groups,...v.subtools])';
  const noticeShown=id=>`(s.notice?.actions??[]).some(a=>a.id===${JSON.stringify(id)})&&!document.querySelector('.canvas-notice').hidden`;
  const visibleIn=(scope,selector)=>`[...document.querySelectorAll(${JSON.stringify(scope)})].filter(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0).flatMap(n=>[...n.querySelectorAll(${JSON.stringify(selector)})])`;
  const switchTo=async id=>{
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:${JSON.stringify(id)}}));null`);
    await until(`the ${id} workspace`,`JSON.parse(layerApp.app.workspace_view()).id===${JSON.stringify(id)}&&!JSON.parse(layerApp.app.workspace_view()).busy`);
    await invoke('fit_canvas');
  };
  const opener=command=>headerTool(`e.item.control?.command===${JSON.stringify(command)}`);
  const dimmedPen=async theme=>{
    const row=`[...document.querySelectorAll('.tool-choice-button[data-brush="${rough}"]')].find(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0)`;
    await until('the dimmed Rough G-Pen row',`!!${row}`);
    const reason=await state(`${items}.find(i=>i.preview===${rough})?.disabled_reason??null`);
    assert.ok(reason,'a pen that cannot draw paths carries a reason');
    assert.deepEqual(await evaluate(`(n=>({disabled:n.disabled,dimmed:n.classList.contains('unavailable-tool'),title:n.title}))(${row})`),{disabled:false,dimmed:true,title:reason},'the dimmed pen stays choosable and says why');
    const at=await evaluate(`(n=>{n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})(${row})`);
    for(const dx of [-12,0])await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:at.x+dx,y:at.y,button:'none',buttons:0,pointerType:'mouse'});
    await until('hovering the dimmed pen shows why',`document.querySelector('#hover-tooltip').matches(':popover-open')&&document.querySelector('#hover-tooltip').textContent===${JSON.stringify(reason)}`,5000);
    await capture(`milestone-${theme}-dimmed-tooltip`);
    await click(row);
    await until('choosing the dimmed pen explains at once',`s.brush.preset===${rough}&&${noticeShown('use_vector_pen')}`);
    assert.deepEqual(await noticeShape(`${theme} dimmed Rough G-Pen`),[]);
    await capture(`milestone-${theme}-dimmed-chosen`);
    await noticeAction('use_vector_pen');
    await until('Use G-Pen from the dimmed pen',`s.brush.preset===${gpen}`);
  };
  const sketchPaths=async theme=>{
    const original=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
    await switchTo('builtin:workspace:painter');
    await click(await opener('scale_rotate'));
    await until('Scale/rotate picks paths','s.layer_tools.tool==="move"&&s.commands.find(c=>c.id==="scale_rotate").selected');
    await tap([650,315]);
    await until('a tap selects the line','s.commands.find(c=>c.id==="deselect").enabled');
    await capture(`milestone-${theme}-sketch-scale-rotate`);
    await invoke('deselect');
    await invoke('select');
    await click(await opener('select'));
    const choices=visibleIn('.content-drawer','.tool-subtools .tool-choice-button');
    await until('the Select drawer',`${choices}.length>2`);
    await steady('.content-drawer');
    const paths=await state(`[${items}.find(i=>i.action?.type==='invoke'&&i.action.command==='move')?.label,s.commands.find(c=>c.id==='edit_points').label]`);
    assert.deepEqual(await evaluate(`${choices}.slice(0,2).map(n=>n.dataset.toolChoice)`),paths,'the Select drawer starts with Select paths and Edit points');
    await capture(`milestone-${theme}-sketch-select-drawer`);
    await click(`${choices}[0]`);
    await until('Select paths picks the path tool','s.layer_tools.tool==="move"&&s.commands.find(c=>c.id==="scale_rotate").selected');
    await send({type:'customize',action:{type:'close_expanded'}});
    await until('the Select drawer closes',`${choices}.length===0`);
    await click(await opener('sculpt'));
    await until('Sculpt explains at once',noticeShown('use_vector_pen'));
    assert.deepEqual(await noticeShape(`${theme} Sculpt`),[]);
    await capture(`milestone-${theme}-sketch-sculpt`);
    await noticeAction('use_vector_pen');
    await until('Use G-Pen after Sculpt',`s.brush.preset===${gpen}&&!s.notice`);
    const vector=await state('Number(s.layers.find(l=>l.selected).id)');
    await send({type:'layer',action:{op:'lock',id:vector,value:true}});
    const greyed=await evaluate(`(()=>{const out=[];let seen=0;
      for(const r of document.querySelectorAll('.tool-tile:not(.toolbar-component)')){const t=r.tileView,b=r.querySelector(':scope > button');if(!t||!b||t.enabled||!t.disabled_reason)continue;seen++;if(b.title!==t.disabled_reason)out.push('tile '+t.label+' says '+b.title);}
      for(const i of layerApp.app.header_view().items){const b=document.querySelector('[data-header-item="'+i.id+'"] .header-tool');if(!b||i.enabled||!i.disabled_reason)continue;seen++;if(b.title!==i.disabled_reason)out.push('header '+i.label+' says '+b.title);}
      return{out,seen};})()`);
    assert.ok(greyed.seen>0,'a locked layer greys a tile or title-bar item with a reason');
    assert.deepEqual(greyed.out,[],'greyed tiles and title-bar items say why');
    await send({type:'layer',action:{op:'lock',id:vector,value:false}});
    await switchTo(original);
  };
  await invoke('new_document');
  await evaluate(createDrawing({width:2000,height:1400}));
  await until('the vector drawing is ready','!layerApp.documents.busy()&&!s.document_file.busy&&layerApp.app.brush_ready()',60000);
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    await invoke('fit_canvas');
    await send({type:'set_color',rgba:[.1,.1,.12,1]});
    await send({type:'select_brush',id:pencil});
    await send({type:'set_brush_size',value:16});
    const label=await evaluate('layerApp.app.catalog().native_copy.layers.new_vector_layer');
    await newVectorLayer(label,'the footer makes a vector layer');
    assert.equal(await state('s.brush.preset'),gpen,'a pen that draws paths is chosen');
    await send({type:'set_brush_size',value:16});
    await stroke(line([400,300],[900,330]));
    await expect(`${theme} inked line`,[650,315],dark);
    await capture(`milestone-${theme}-discover`);
    await dimmedPen(theme);
    await sketchPaths(theme);

    await send({type:'select_brush',id:pencil});
    await tap([650,515]);
    await noticeAction('use_vector_pen');
    await until('Use G-Pen',`s.brush.preset===${gpen}`);
    await stroke(line([400,500],[900,530]));
    await expect(`${theme} pen line`,[650,515],dark);
    await send({type:'select_brush',id:pencil});
    await tap([650,715]);
    await capture(`milestone-${theme}-wrong-brush`);
    await noticeAction('new_paint_layer');
    await until('a paint layer above',`!(${activeVector})`);
    await send({type:'set_brush_size',value:30});
    await stroke(line([400,700],[900,730]));
    await expect(`${theme} pencil on paint`,[650,715],marked);

    await newVectorLayer(label,'another vector layer');
    await tool({figure:{shape:'ellipse',paint:'outline'}});
    await stroke(line([1100,300],[1400,500],8));
    await send({type:'set_color',rgba:[.15,.7,.25,1]});
    await tool({region:{fill:true,source:'visible'}});
    await tap([1250,400]);
    await expect(`${theme} filled inside the outline`,[1250,450],green);
    await capture(`milestone-${theme}-color`);
    await send({type:'set_color',rgba:[.1,.1,.12,1]});
    await newVectorLayer(label,'a vector layer for crossing lines');
    await invoke('pen');
    await send({type:'select_brush',id:gpen});
    await send({type:'set_brush_size',value:16});
    for(const [from,to] of [[[300,1000],[1100,1000]],[[500,850],[500,1150]],[[900,850],[900,1150]]])await stroke(line(from,to));
    await chord('a','KeyA',65,2);
    const deleteLabel=await state('s.commands.find(c=>c.id==="clear_selected").label');
    await until('Select All selects the paths',`s.commands.find(c=>c.id==="clear_selected").label!==${JSON.stringify(deleteLabel)}||s.commands.find(c=>c.id==="deselect").enabled`);
    await capture(`milestone-${theme}-select-all`);
    await chord('Escape','Escape',27);
    await invoke('eraser');
    await send({type:'set_brush_size',value:16});
    const intersection=await evaluate('layerApp.state().commands.find(c=>c.id==="erase_to_intersection").label');
    await fromDrawer('e.item.control?.command==="eraser"',`[data-tool-choice=${JSON.stringify(intersection)}]`,'Erase up to intersection','s.commands.find(c=>c.id==="erase_to_intersection").selected');
    await stroke(line([700,980],[700,1020],4));
    await expect(`${theme} span between crossings`,[780,1000],dark,false);
    await expect(`${theme} kept left part`,[400,1000],dark);
    await expect(`${theme} kept right part`,[1000,1000],dark);
    await capture(`milestone-${theme}-erase`);

    const lines=await state('Number(s.layers.find(l=>l.selected).id)');
    await send({type:'layer',action:{op:'new_below'}});
    await send({type:'layer',action:{op:'select',id:lines,mask:false}});
    const vectors=await state('s.layers.filter(l=>l.content_icon==="layer-path-symbolic").length');
    await invoke('merge_down');
    await noticeAction('merge');
    await until('the lines become pixels',`s.layers.filter(l=>l.content_icon==="layer-path-symbolic").length===${vectors-1}`);
    await expect(`${theme} merged pixels`,[400,1000],dark);
    await capture(`milestone-${theme}-merged`);
    for(const id of await state('s.layers.filter(l=>!l.paper).map(l=>Number(l.id))')){
      await send({type:'layer',action:{op:'select',id,mask:false}});
      await send({type:'layer',action:{op:'delete_selected'}});
    }
  }
}
