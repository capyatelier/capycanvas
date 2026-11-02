import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {vectorHarness} from './vector-journey-fixture.mjs';
import {readPackage,authoredIdentity} from './package-fixture.test.mjs';


export async function checkVectorEdges({call,evaluate,settle}) {
  const findings=[];
  const {pause,send,invoke,tool,state,until,idle,camera,screen,size,line,mouseEvent,draw,eraserEnd,hover,key,chord,shot,darkest,inked,park,expect,refusalCursor,capture,click,noticeShape,brush,activeVector,vectorLayers,toolIs}=await vectorHarness({call,evaluate,settle});
  const [pencil,gpen,smudge,monoline]=[await brush('Pencil'),await brush('G-Pen'),await brush('Smudge'),await brush('Monoline')];
  const fresh=async()=>{
    for(const id of await state('s.layers.filter(l=>!l.paper).map(l=>Number(l.id))')){await send({type:'layer',action:{op:'select',id,mask:false}});await send({type:'layer',action:{op:'delete_selected'}});}
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    await send({type:'layer',action:{op:'new_vector'}});
    await until('a vector layer',activeVector);
    await invoke('fit_canvas');
    await send({type:'set_color',rgba:[.1,.1,.12,1]});
    await send({type:'select_brush',id:gpen});
    await send({type:'set_brush_size',value:16});
  };

  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});await settle();
    await fresh();
    const [w,h]=await size();
    const rows=[.3,.45,.6].map(f=>h*f), x0=w*.25, x1=w*.75, mid=w*.5;

    await draw('pen',line([x0,rows[0]],[x1,rows[0]]));
    await expect(`${theme} pen line`,[mid,rows[0]]);
    await draw('mouse',line([x0,rows[1]],[x1,rows[1]]));
    await expect(`${theme} mouse line`,[mid,rows[1]]);
    const view=JSON.stringify(await camera());
    await draw('touch',line([x0,rows[2]],[x1,rows[2]],8));
    await expect(`${theme} one finger draws nothing`,[mid,rows[2]],false);
    if(JSON.stringify(await camera())!==view)findings.push(`${theme}: one finger moved the view on a vector layer`);
    await draw('pen',line([x0,rows[2]],[x1,rows[2]]));
    await expect(`${theme} third pen line`,[mid,rows[2]]);
    await capture(`edges-${theme}-input`);

    await send({type:'set_brush_size',value:48});
    await eraserEnd(line([mid,rows[0]-80],[mid,rows[2]+80],20));
    for(const row of rows)await expect(`${theme} the pen's eraser end cuts the line at ${row}`,[mid,row],false);
    await expect(`${theme} the eraser end keeps the rest`,[w*.35,rows[0]]);
    if(await state('s.brush.preset')!==gpen)findings.push(`${theme}: the pen's eraser end left another brush chosen`);
    await capture(`edges-${theme}-eraser-end`);
    await invoke('undo');
    await expect(`${theme} undoing the eraser end`,[mid,rows[0]]);
    await send({type:'set_brush_size',value:16});

    const empty=[w*.85,h*.85];
    for(const [name,choose,refused] of [
      ['G-Pen',()=>send({type:'select_brush',id:gpen}),false],
      ['Monoline',()=>send({type:'select_brush',id:monoline}),false],
      ['Pencil',()=>send({type:'select_brush',id:pencil}),true],
      ['Smudge',()=>send({type:'select_brush',id:smudge}),true],
      ['Eraser',()=>invoke('eraser'),false],
      ['Figure',()=>tool({figure:{shape:'star',paint:'both'}}),false],
      ['Fill',()=>tool({region:{fill:true,source:'visible'}}),false],
      ['Lasso Fill',()=>tool('lasso_fill'),false],
      ['Enclose Fill',()=>tool({enclose_fill:{source:'visible'}}),false],
      ['Paint unfilled',()=>tool({paint_unfilled:{source:'visible'}}),false],
      ['Gradient',()=>tool({gradient:{shape:'Linear'}}),true],
      ['Move',()=>invoke('move'),false],
      ['Edit Points',()=>invoke('edit_points'),false],
      ['Lasso',()=>tool('select'),false],
    ]){
      await choose();await pause(150);
      for(const device of ['pen','mouse']){
        const seen=await refusalCursor(device,empty);
        if(seen!==refused)findings.push(`${theme} ${name} by ${device}: refusal cursor ${seen}, expected ${refused}`);
      }
      if(name==='Gradient')await capture(`edges-${theme}-cursor-refused`);
    }
    await park();

    await invoke('move');
    for(const [index,device] of ['pen','mouse','touch'].entries()){
      const start=[w*.4,rows[index]];
      await draw(device,line(start,[start[0],start[1]+h*.05],6));
      await park();
      const moved=!inked(await darkest(start))&&inked(await darkest([start[0],start[1]+h*.05]));
      if(!moved)findings.push(`${theme} Move by ${device}: the path did not follow the drag`);
      if(moved)await invoke('undo');
    }
    await expect(`${theme} undone drags`,[w*.4,rows[0]]);

    await send({type:'select_brush',id:gpen});
    const view2=JSON.stringify(await camera());
    const fingers=async()=>{
      const [f,g]=[await screen([w*.8,h*.2]),await screen([w*.9,h*.3])];
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...f,id:11},{...g,id:12}]});
      for(let k=1;k<=4;k++){await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:f.x-20*k,y:f.y+10*k,id:11},{x:g.x+20*k,y:g.y+20*k,id:12}]});await settle();}
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
    };
    await draw('pen',line([x0,h*.75],[x1,h*.75],20),{at:8,run:fingers});
    await expect(`${theme} the pen line drawn under two fingers`,[w*.7,h*.75]);
    if(JSON.stringify(await camera())!==view2)findings.push(`${theme}: two fingers moved the view while the pen drew`);
    await invoke('fit_canvas');
    await capture(`edges-${theme}-two-fingers`);

    await chord('a');
    await until('Ctrl+A selects the paths and switches to Move',`s.commands.find(c=>c.id==="deselect").enabled&&${toolIs('move')}`);
    await key('Escape','Escape',27);
    await until('Escape clears the paths','!s.commands.find(c=>c.id==="deselect").enabled');
    await draw('mouse',[[mid,rows[0]],[mid,rows[0]]]);
    await until('a click selects a path','s.commands.find(c=>c.id==="deselect").enabled');
    await key('Enter','Enter',13);
    await until('Enter opens Edit Points',toolIs('edit_points'));
    await capture(`edges-${theme}-edit-points`);
    await key('Escape','Escape',27);
    await until('Escape returns to Move with the path selected',`${toolIs('move')}&&s.commands.find(c=>c.id==="deselect").enabled`);
    await key('Delete','Delete',46);
    await expect(`${theme} Delete removes the selected path`,[mid,rows[0]],false);
    await chord('z');
    await expect(`${theme} Ctrl+Z restores the path`,[mid,rows[0]]);

    await draw('mouse',line([mid,rows[1]],[mid,rows[1]+h*.08],6),{at:4,run:()=>key('Escape','Escape',27)});
    await expect(`${theme} Escape mid-drag leaves the path in place`,[mid,rows[1]]);
    await expect(`${theme} Escape mid-drag leaves no preview`,[mid,rows[1]+h*.08],false);
    await draw('mouse',line([mid,rows[1]],[mid,rows[1]+h*.08],6),{at:3,run:()=>chord('z')});
    await expect(`${theme} the drag that saw Ctrl+Z lands`,[mid,rows[1]+h*.08]);
    if(!inked(await darkest([mid,rows[2]])))findings.push(`${theme}: Ctrl+Z during a path drag undid an earlier line`);
    await chord('z');
    await expect(`${theme} Ctrl+Z after the drag undoes it`,[mid,rows[1]]);

    await draw('pen',[[mid,rows[2]],[mid,rows[2]]]);
    await invoke('edit_points');
    await draw('touch',line([x0,rows[2]],[x0,rows[2]+h*.1],8));
    await park();
    if(!inked(await darkest([x0,rows[2]+h*.1])))findings.push(`${theme}: Edit Points by touch did not move the anchor`);
    await capture(`edges-${theme}-touch-points`);

    const merged=await state(vectorLayers);
    await send({type:'layer',action:{op:'new_vector'}});
    await send({type:'select_brush',id:gpen});
    await draw('pen',line([x0,h*.9],[x1,h*.9]));
    await chord('e');
    await until('Ctrl+E merges vector layers as paths',`${vectorLayers}===${merged}&&!s.notice`);
    await chord('e');
    await until('Ctrl+E onto paint asks first','(s.notice?.actions??[]).some(a=>a.id==="merge")');
    await capture(`edges-${theme}-merge-notice`);
    await click(`[...document.querySelectorAll('.canvas-notice-action')].find(e=>e.checkVisibility()&&e.textContent.trim()===${JSON.stringify(await state('s.notice.actions.find(a=>a.id==="merge").label'))})`);
    await until('the merge bakes the paths',`${vectorLayers}===${merged-1}`);
    await expect(`${theme} merged pixels`,[w*.7,h*.9]);
    console.log(`Vector edges ${theme}: input kinds, eraser end, cursors, keys, two fingers and merges`);

    await fresh();
    await draw('pen',line([x0,rows[0]],[x1,rows[0]]));
    const vectorId=await state('Number(s.layers.find(l=>l.selected).id)');
    await send({type:'select_brush',id:pencil});
    await draw('mouse',line([x0,rows[1]],[x1,rows[1]],6));
    await until('the vector refusal','(s.notice?.actions??[]).some(a=>a.id==="new_paint_layer")&&!document.querySelector(".canvas-notice").hidden');
    const noticeId=await state('s.notice.id');
    findings.push(...await noticeShape(`${theme} vector refusal`));
    const bubble=()=>evaluate(`(()=>{const n=document.querySelector('.canvas-notice'),s=getComputedStyle(n);return{background:s.backgroundColor,hidden:n.hidden,label:n.getAttribute('aria-label'),tab:[...n.querySelectorAll('.canvas-notice-action')].map(b=>b.tabIndex)}})()`);
    const shown=await bubble();
    await send({type:'set_theme',theme:theme==='light'?'dark':'light'});
    const switched=await bubble();
    if(switched.hidden||await state('s.notice?.id')!==noticeId)findings.push(`${theme}: switching the theme hid the notice`);
    if(switched.background===shown.background)findings.push(`${theme}: the notice kept ${shown.background} after the theme switch`);
    await capture(`edges-${theme}-notice-switched`);
    await send({type:'set_theme',theme});
    if(shown.tab.some(t=>t<0))findings.push(`${theme}: notice actions take no keyboard focus (tabIndex ${shown.tab})`);
    if(shown.label!==await state('s.notice?.name'))findings.push(`${theme}: the notice is named ${JSON.stringify(shown.label)}, not ${JSON.stringify(await state('s.notice?.name'))}`);
    const paint=await state('Number(s.layers.find(l=>!l.selected&&!l.paper&&l.content_icon!=="layer-path-symbolic").id)');
    await send({type:'layer',action:{op:'select',id:paint,mask:false}});
    if(!(await bubble()).hidden&&await state('!!s.notice')){
      const count=await state('s.layers.length');
      await click(`[...document.querySelectorAll('.canvas-notice-action')].find(e=>e.checkVisibility()&&e.textContent.trim()===${JSON.stringify(await state('s.notice.actions.find(a=>a.id==="new_paint_layer").label'))})`);
      await pause(300);
      const stale=await state(`s.notice&&s.notice.id!==${noticeId}&&s.notice.actions.length===0&&!document.querySelector('.canvas-notice').hidden&&document.querySelector('.canvas-notice-text').textContent===s.notice.text`);
      if(await state('s.layers.length')!==count)findings.push(`${theme}: a stale notice action added a layer`);
      if(!stale)findings.push(`${theme}: a stale notice action showed nothing`);
      await capture(`edges-${theme}-notice-stale`);
    }
    await send({type:'layer',action:{op:'select',id:vectorId,mask:false}});

    await evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:JSON.stringify({...layerApp.state().settings,language:{Explicit:'de'}})})`);
    await until('German','!layerApp.app.language_pending()&&document.documentElement.lang==="de"');
    await send({type:'set_theme',theme});
    const fits=(scope,place)=>evaluate(`(()=>{const out=[];const box=n=>n.getBoundingClientRect();const inside=(a,b)=>a.left>=b.left-1&&a.top>=b.top-1&&a.right<=b.right+1&&a.bottom<=b.bottom+1;
      const viewport={left:0,top:0,right:innerWidth,bottom:innerHeight};
      for(const root of document.querySelectorAll(${JSON.stringify(scope)})){if(!root.checkVisibility())continue;const r=box(root);if(!r.width)continue;
        if(!inside(r,viewport))out.push(${JSON.stringify(place)}+' '+root.className+' leaves the window '+JSON.stringify([r.left,r.top,r.right,r.bottom]));
        for(const n of root.querySelectorAll('button,span,.tool-choice-name')){const b=box(n);if(!b.width||!n.checkVisibility())continue;
          if(!inside(b,r))out.push(${JSON.stringify(place)}+' '+(n.textContent.trim()||n.getAttribute('aria-label'))+' leaves '+root.className);
          if(n.children.length===0&&n.textContent.trim()&&n.scrollWidth>n.clientWidth+1)out.push(${JSON.stringify(place)}+' '+n.textContent.trim()+' is cut to '+n.clientWidth+' of '+n.scrollWidth);
          if(n.children.length===0&&n.textContent.trim()&&n.scrollHeight>n.clientHeight+1)out.push(${JSON.stringify(place)}+' '+n.textContent.trim()+' is cut to '+n.clientHeight+' of '+n.scrollHeight+' px high');}}
      return out;})()`);
    const overflowMenu=async place=>{
      const hidden=await evaluate(`(()=>{const f=[...document.querySelectorAll('.layer-footer')].find(n=>n.checkVisibility());if(!f)return [];const w=f.getBoundingClientRect().width;return (layerApp.state().layer_tools.footer??[]).filter(s=>w+.5<s.min_width).map(s=>s.item.label)})()`);
      if(!hidden.length)return [];
      await click(`[...document.querySelectorAll('.layer-footer .layer-more')].find(n=>n.checkVisibility())`);
      await until('the Layers actions menu',`!!document.querySelector('.panel-context-menu:popover-open')`);
      const shown=await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button .menu-label')].map(n=>n.textContent)`);
      await key('Escape','Escape',27);
      return hidden.filter(label=>!shown.includes(label)).map(label=>`${place}: the actions menu does not list ${label}`);
    };
    const visible=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});return !!n&&n.getBoundingClientRect().width>0&&n.checkVisibility()})()`);
    const openHeader=async(match,shown)=>{
      for(let attempt=0;attempt<3&&!(await evaluate(shown));attempt++){
        const id=await state(`s.workspace.layout.header.zones.flat().find(e=>${match}).id`);
        for(const end=Date.now()+5000;Date.now()<end&&!(await visible(`[data-header-item="${id}"] .header-tool`));)await pause(100);
        if(await visible(`[data-header-item="${id}"] .header-tool`))await click(`document.querySelector('[data-header-item="${id}"] .header-tool')`);
        else for(const zone of await evaluate(`[...document.querySelectorAll('.header-overflow')].filter(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0).map(n=>n.id)`)){
          await click(`document.querySelector('#${zone} > summary')`);await pause(300);
          if(await evaluate(`!!document.querySelector('#${zone} [data-header-overflow-item="${id}"]')`)){await click(`document.querySelector('#${zone} [data-header-overflow-item="${id}"]')`);break;}
          await key('Escape','Escape',27);
        }
        await pause(400);
      }
      if(!(await evaluate(shown)))await capture(`edges-header-failure`);
      assert.ok(await evaluate(shown),`the drawer for ${match} did not open`);
    };
    const footerShown='[...document.querySelectorAll(".layer-footer")].some(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0)';
    const modesShown=async()=>`[...document.querySelectorAll('[data-tool-choice=${JSON.stringify(await state('s.commands.find(c=>c.id==="erase_whole_paths").label'))}]')].some(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0)`;
    const modes=async place=>{
      for(const id of ['erase_touched_part','erase_whole_paths','erase_to_intersection']){
        const label=await state(`s.commands.find(c=>c.id===${JSON.stringify(id)}).label`);
        const found=await evaluate(`[...document.querySelectorAll('[data-tool-choice=${JSON.stringify(label)}]')].filter(n=>n.checkVisibility()&&n.getBoundingClientRect().width>0).map(n=>{const t=n.querySelector('.tool-choice-name')??n;return{cut:t.scrollWidth>t.clientWidth+1,width:t.clientWidth,need:t.scrollWidth}})`);
        if(!found.length)findings.push(`${theme} ${place}: no visible ${label}`);
        for(const f of found)if(f.cut)findings.push(`${theme} ${place}: ${label} is cut to ${f.width} of ${f.need}`);
      }
      await capture(`edges-${theme}-${place}-eraser`);
    };
    const refuse=async place=>{
      await send({type:'select_brush',id:pencil});
      const [w2,h2]=await size();
      await draw('mouse',line([w2*.3,h2*.5],[w2*.6,h2*.5],6));
      await until('the German refusal','(s.notice?.actions??[]).length===3&&!document.querySelector(".canvas-notice").hidden');
      await park();
      findings.push(...await fits('.canvas-notice',`${theme} ${place} notice`),...await noticeShape(`${theme} ${place} notice`));
      await capture(`edges-${theme}-${place}-notice`);
    };
    const workspace=async id=>{
      await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:${id}'}));null`);
      await until(`the ${id} workspace`,`JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:${id}"&&!JSON.parse(layerApp.app.workspace_view()).busy`);
      await pause(300);
    };
    for(const width of [1440,1280,640]){
      await call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await settle();await pause(300);
      await workspace('painter');await invoke('fit_canvas');
      await openHeader('e.item.control?.panel==="layers"',footerShown);
      findings.push(...await fits('.layer-footer',`${theme} ${width} drawer footer`));
      findings.push(...await overflowMenu(`${theme} ${width} drawer footer`));
      await capture(`edges-${theme}-${width}-drawer-footer`);
      await key('Escape','Escape',27);
      await invoke('eraser');
      await openHeader('e.item.control?.command==="eraser"',await modesShown());
      await modes(`${width}-drawer`);
      await key('Escape','Escape',27);
      await refuse(`${width}-drawer`);
      await workspace('illustrator');await invoke('fit_canvas');
      if(await evaluate(footerShown))findings.push(...await fits('.layer-footer',`${theme} ${width} docked footer`));
      await invoke('eraser');await pause(300);
      if(await evaluate(await modesShown()))await modes(`${width}-docked`);
      await refuse(`${width}-docked`);
    }
    await workspace('painter');
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});await settle();
    await evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:JSON.stringify({...layerApp.state().settings,language:{Explicit:'en'}})})`);
    await until('English','!layerApp.app.language_pending()&&document.documentElement.lang==="en"');
    await send({type:'set_theme',theme});
    console.log(`Vector edges ${theme}: notices, theme switch, stale actions and German labels at 1440, 1280 and 640`);
  }
  console.log(`Vector edges findings:\n${findings.join('\n')}`);
  assert.deepEqual(findings,[],`findings:\n${findings.join('\n')}`);
}

export async function checkVectorFiles({call,evaluate,settle}) {
  const findings=[];
  const {pause,send,invoke,tool,state,until,idle,camera,screen,size,line,mouseEvent,draw,key,chord,darkest,inked,park,expect,capture,click,brush,activeVector,vectorLayers}=await vectorHarness({call,evaluate,settle});
  const gpen=await brush('G-Pen');
  const menuButton=label=>`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const rowMenu=async(id,label)=>{
    const p=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${id}"] .layer-name').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});
    await settle();
    await until('the row menu',`!!document.querySelector('.panel-context-menu:popover-open')`);
    const shared=await evaluate(`layerApp.app.layer_menu(${id}n,false).sections.flat().map(i=>i.label)`);
    const shown=await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button .menu-label')].map(n=>n.textContent)`);
    if(label){
      if(!(await evaluate(`!!${menuButton(label)}`))){
        const parent=await evaluate(`(()=>{const find=(s,trail)=>{for(const i of s.flat()){if(i.label===${JSON.stringify(label)})return trail;const r=find(i.sections??[],[...trail,i.label]);if(r)return r}};return find(layerApp.app.layer_menu(${id}n,false).sections,[])})()`);
        for(const step of parent??[]){await click(menuButton(step));await pause(200);}
      }
      await click(menuButton(label));
    }
    return {shared,shown};
  };
  const viewport=width=>call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    await viewport(1440);await settle();
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:illustrator'}));null`);
    await until('the docked workspace',`JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:illustrator"&&!JSON.parse(layerApp.app.workspace_view()).busy`);
    for(const id of await state('s.layers.filter(l=>!l.paper).map(l=>Number(l.id))')){await send({type:'layer',action:{op:'select',id,mask:false}});await send({type:'layer',action:{op:'delete_selected'}});}
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    const label=await evaluate('layerApp.app.catalog().native_copy.layers.new_vector_layer');
    await click(`[...document.querySelectorAll('[aria-label=${JSON.stringify(label)}]')].find(n=>n.checkVisibility())`);
    await until('a vector layer from the docked footer',activeVector);
    await invoke('fit_canvas');
    await invoke('pen');
    await send({type:'select_brush',id:gpen});
    await send({type:'set_color',rgba:[.85,.35,.1,1]});
    await send({type:'set_brush_size',value:16});
    const [w,h]=await size();
    const probe=[w*.5,h*.3];
    await draw('mouse',line([w*.25,h*.3],[w*.75,h*.3]));
    await expect(`${theme} the mouse line`,probe);
    await tool({figure:{shape:'rectangle',paint:'both'}});
    await draw('mouse',line([w*.25,h*.45],[w*.45,h*.65],6));
    await tool({figure:{shape:'star',paint:'outline'}});
    await draw('mouse',line([w*.55,h*.45],[w*.75,h*.75],6));
    const layer=await state('Number(s.layers.find(l=>l.selected).id)');
    await send({type:'effect',action:{op:'set',layer,key:'path_outline_color',value:{kind:'color',value:{space:'Srgb',rgba:[.1,.3,.9,1]}}}});
    await send({type:'effect',action:{op:'set',layer,key:'path_width',value:{kind:'number',value:12}}});
    await expect(`${theme} the star`,[w*.65,h*.45]);

    const {shared,shown}=await rowMenu(layer,null);
    for(const item of shared)if(!shown.includes(item))findings.push(`${theme}: the vector row's menu lacks ${item}`);
    const rasterize=await state('s.commands.find(c=>c.id==="rasterize_layer").label');
    if(!(await evaluate(`(()=>{const out=[];const visit=s=>{for(const i of s.flat()){out.push(i.label);visit(i.sections??[])}};visit(layerApp.app.layer_menu(${layer}n,false).sections);return out})()`)).includes(rasterize))findings.push(`${theme}: the vector row's menu has no ${rasterize}`);
    await capture(`files-${theme}-row-menu`);
    await key('Escape','Escape',27);
    const duplicate=await evaluate(`(()=>{const find=s=>{for(const i of s.flat()){if(JSON.stringify(i.action??null,(k,v)=>typeof v==='bigint'?String(v):v).includes('"duplicate'))return i.label;const r=find(i.sections??[]);if(r)return r}};return find(layerApp.app.layer_menu(${layer}n,false).sections)})()`);
    await rowMenu(layer,duplicate);
    await until('Duplicate adds a vector layer',`${vectorLayers}===2`);
    const copy=await state('Number(s.layers.find(l=>l.selected).id)');
    await click(`document.querySelector('.layer-row[data-layer="${copy}"] .layer-name')`);
    const name=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${copy}"] .layer-name').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...name,button:'left',buttons:1,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...name,button:'left',buttons:0,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...name,button:'left',buttons:1,clickCount:2});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...name,button:'left',buttons:0,clickCount:2});
    await until('renaming',`s.layer_tools.rename_layer!=null`);
    await chord('a');
    await call('Input.insertText',{text:'Ink'});
    await key('Enter','Enter',13);
    await until('the rename',`s.layers.some(l=>Number(l.id)===${copy}&&l.label==="Ink")`);
    await click(`document.querySelector('.layer-row[data-layer="${copy}"] button')`);
    await until('hiding the copy',`s.layers.some(l=>Number(l.id)===${copy}&&!l.visible)`);
    await expect(`${theme} the original still shows`,probe);
    await click(`document.querySelector('.layer-row[data-layer="${layer}"] button')`);
    await expect(`${theme} both hidden`,probe,false);
    await capture(`files-${theme}-hidden`);
    for(const id of [layer,copy])await click(`document.querySelector('.layer-row[data-layer="${id}"] button')`);
    await expect(`${theme} shown again`,probe);
    const before=await state('s.layers.map(l=>Number(l.id))');
    const from=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${copy}"] .layer-name').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    const below=before[before.indexOf(layer)+1];
    const to=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${below}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height-3}})()`);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...from,button:'left',buttons:1,clickCount:1});
    for(let i=1;i<=8;i++){await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:from.x+(to.x-from.x)*i/8,y:from.y+(to.y-from.y)*i/8,button:'left',buttons:1});await settle();}
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...to,button:'left',buttons:0,clickCount:1});await settle();
    await until('dragging the vector row down',`JSON.stringify(s.layers.map(l=>Number(l.id)))!==${JSON.stringify(JSON.stringify(before))}`);
    const after=await state('s.layers.map(l=>Number(l.id))');
    if(after.indexOf(copy)<after.indexOf(layer))findings.push(`${theme}: the dragged vector row did not move below ${JSON.stringify(before)} -> ${JSON.stringify(after)}`);
    await capture(`files-${theme}-reordered`);
    await chord('z');
    await until('Ctrl+Z restores the order',`JSON.stringify(s.layers.map(l=>Number(l.id)))===${JSON.stringify(JSON.stringify(before))}`);
    await send({type:'layer',action:{op:'select',id:copy,mask:false}});
    await send({type:'layer',action:{op:'lock',id:copy,value:true}});
    await send({type:'select_brush',id:gpen});
    await draw('pen',line([w*.25,h*.85],[w*.75,h*.85]));
    await until('a locked vector layer refuses the pen','!!s.notice');
    await expect(`${theme} the locked layer stays empty`,[w*.5,h*.85],false);
    await send({type:'layer',action:{op:'lock',id:copy,value:false}});
    await click(`[...document.querySelectorAll('.layer-footer button')].find(n=>n.checkVisibility()&&n.getAttribute('aria-label')===layerApp.app.catalog().native_copy.layers.delete_selected)`);
    await until('deleting the copy',`${vectorLayers}===1`);
    await chord('z');
    await until('Ctrl+Z brings the copy back','s.layers.some(l=>l.label==="Ink")');
    await send({type:'layer',action:{op:'delete_selected'}});
    await send({type:'layer',action:{op:'select',id:layer,mask:false}});

    await invoke('move');
    await key('Escape','Escape',27);
    const grab=[w*.5,h*.3],target=[w*.5+w*.05,h*.3+h*.08];
    const a=await screen(grab);
    await mouseEvent('mousePressed',a,'mouse');
    await mouseEvent('mouseMoved',{x:a.x+10,y:a.y+10},'mouse');await settle();
    await viewport(1000);await settle();await pause(300);
    await mouseEvent('mouseMoved',{x:a.x+20,y:a.y+20},'mouse');await settle();
    await viewport(1440);await settle();await pause(300);
    const b=await screen(target);
    await mouseEvent('mouseMoved',b,'mouse');await settle();
    await mouseEvent('mouseReleased',b,'mouse');await settle();
    await park();await pause(500);
    await capture(`files-${theme}-resized-drag`);
    const landed=inked(await darkest(target)),left=!inked(await darkest(grab));
    if(landed!==left)findings.push(`${theme}: a path dragged across a window resize landed at the pointer ${landed}, left its old place ${left}`);
    console.log(`Vector files ${theme}: a drag across a window resize ${landed?'follows the pointer':'is cancelled'}`);
    if(landed){await chord('z');await expect(`${theme} undoing the resized drag`,grab);}

    await invoke('eraser');
    const whole=await state('s.commands.find(c=>c.id==="erase_whole_paths").label');
    await evaluate(`(()=>{const n=[...document.querySelectorAll('[data-tool-choice=${JSON.stringify(whole)}]')].find(n=>n.checkVisibility());if(n)n.click();else layerApp.dispatch({type:'invoke',command:'erase_whole_paths'});})()`);
    await until('Erase whole paths','s.settings.vector_eraser==="whole_paths"');
    await evaluate(`window.vectorFiles={save:window.showSaveFilePicker,open:window.showOpenFilePicker};window.showSaveFilePicker=async()=>{const d=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-vector-edges',{create:true});return vectorFiles.handle=await d.getFileHandle(crypto.randomUUID()+'.capy',{create:true});};`);
    try{
      const save=async()=>{
        await until('save enabled','s.commands.find(c=>c.id==="save_document_as")?.enabled&&!layerApp.documents.busy()');
        const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:'save_document_as'})`,userGesture:true});
        assert.equal(reply.exceptionDetails,undefined);await settle();await idle();
        await evaluate('(async()=>{vectorFiles.saved=new Uint8Array(await(await vectorFiles.handle.getFile()).arrayBuffer())})()');
        return readPackage(evaluate,'vectorFiles.saved');
      };
      const first=await save();
      await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([vectorFiles.saved],'vector-edges.capy')}}]`);
      const tabs=await state('s.tabs.length');
      const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:'open_document'})`,userGesture:true});
      assert.equal(reply.exceptionDetails,undefined);await settle();
      for(const end=Date.now()+30000;Date.now()<end;await pause(200)){
        if(await evaluate('!layerApp.documents.busy()&&!layerApp.state().document_file.busy'))break;
        if(await evaluate('!!document.querySelector("dialog[open]")'))break;
      }
      const dialog=await evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent.trim().slice(0,200)).join(' | ')`);
      if(dialog){
        await capture(`files-${theme}-reopen-refused`);
        await writeFile(`${process.env.LAYER_TEST_ARTIFACTS??'artifacts/vector-edges/web'}/files-${theme}-saved.capy`,Buffer.from(await evaluate('Array.from(vectorFiles.saved)')));
        findings.push(`${theme}: reopening the saved vector drawing refused it: ${dialog}; resources ${JSON.stringify(first.resources.map(r=>[r.type,r.encoding]))}`);
        await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>/Close|Schließen/.test(b.textContent))?.click()`);
        await idle();
      }else{
        await until('the reopened vector layer',`s.layers.some(l=>l.content_icon==="layer-path-symbolic")`);
        await invoke('fit_canvas');
        await expect(`${theme} the reopened line`,probe);
        await expect(`${theme} the reopened star`,[w*.65,h*.45]);
        await capture(`files-${theme}-reopened`);
        const second=await save();
        assert.deepEqual(authoredIdentity(second),authoredIdentity(first),'paths, styles and live shapes survive save and reopen');
      }
    }finally{
      await evaluate('if(window.vectorFiles){window.showSaveFilePicker=vectorFiles.save;window.showOpenFilePicker=vectorFiles.open;delete window.vectorFiles}');
    }
    console.log(`Vector files ${theme}: row menu, duplicate, rename, hide, reorder, lock, delete, resized drag, save and reopen`);
  }
  await call('Page.reload',{ignoreCache:true});await pause(1000);
  await evaluate(`new Promise((resolve,reject)=>{const started=performance.now();function check(){if(window.layerApp&&document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready())resolve(true);else if(performance.now()-started>30000)reject(Error('reload'));else setTimeout(check,100);}check();})`);
  await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+30000;function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v?.ready&&!v.busy)resolve();else if(performance.now()>deadline)reject(Error('workspace'));else setTimeout(check,100);}check();})`);
  for(let i=0;i<20&&await evaluate('!!document.querySelector("dialog[open]")');i++){await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>/Keep|Later|Behalten/.test(b.textContent))?.click()`);await pause(300);}
  if(await state('s.settings.vector_eraser')!=='whole_paths')findings.push(`the eraser mode did not survive a reload: ${await state('s.settings.vector_eraser')}`);
  await send({type:'restore_saved_settings',saved:await evaluate('JSON.stringify({...layerApp.state().settings,vector_eraser:"touched_part"})')});
  console.log(`Vector files findings:\n${findings.join('\n')}`);
  assert.deepEqual(findings,[],`findings:\n${findings.join('\n')}`);
}
