import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {gzipSync} from 'node:zlib';

export const phoneAuditMatrix = [
  {viewport:[412,891],text_pt:11},
  {viewport:[891,412],text_pt:11},
  {viewport:[320,568],text_pt:11},
  {viewport:[320,320],text_pt:16},
  {viewport:[280,560],text_pt:11},
].flatMap(size=>['light','dark'].map(theme=>({...size,theme,dpr:1,text_scale:size.text_pt/11})));

export function measurePhoneAuditElements(specifications, root = document) {
  const elements=[],missing=[],ids=new Set();
  for(const {id,role,selector,optional=false} of specifications) {
    if(ids.has(id))throw Error(`Duplicate phone audit ID: ${id}`);
    ids.add(id);
    const nodes=[...root.querySelectorAll(selector)];
    if(nodes.length>1)throw Error(`Ambiguous phone audit element ${id}: ${nodes.length} nodes`);
    if(!nodes.length){if(!optional)missing.push(id);continue;}
    const node=nodes[0],style=getComputedStyle(node),r=node.getBoundingClientRect();
    const visible=!!node.getClientRects().length&&style.visibility!=='hidden'&&style.display!=='none'&&r.width>0&&r.height>0;
    const bounds=visible?[r.x,r.y,r.width,r.height]:null;
    if(bounds&&!bounds.every(Number.isFinite))throw Error(`Nonfinite phone audit bounds: ${id}`);
    const source={selector,dom_id:node.id||null,type:node.tagName.toLowerCase()};
    if(role==='drawer-panel')source.layout=[node,node.parentElement].filter(Boolean).map(node=>{
      const style=getComputedStyle(node),b=node.getBoundingClientRect();
      return{class:node.className,bounds:[b.x,b.y,b.width,b.height],requested_minimum:node.style.minHeight,
        display:style.display,height:style.height,minimum:style.minHeight,margin:[style.marginTop,style.marginBottom],padding:[style.paddingTop,style.paddingBottom],flex:style.flex,overflow:style.overflow};
    });
    elements.push({id,role,bounds,visible,source});
  }
  return {elements,missing};
}

export function phoneAuditDrawerSpecifications(root = document) {
  const specs=[];
  const selector=node=>{
    const path=[];
    for(let current=node;current;current=current.parentElement){
      if(current.id&&root.querySelectorAll(`#${CSS.escape(current.id)}`).length===1){path.unshift(`#${CSS.escape(current.id)}`);break;}
      path.unshift(`:nth-child(${[...current.parentElement.children].indexOf(current)+1})`);
    }
    return path.join(' > ');
  };
  const add=(id,role,node)=>{if(node)specs.push({id,role,selector:selector(node)});};
  for(const drawer of root.querySelectorAll('.content-drawer')){
    const id=drawer.dataset.drawer==='tool'?'drawer':`drawer.${drawer.dataset.drawer}`;add(id,'drawer',drawer);
    [...drawer.querySelectorAll(':scope > .drawer-contents > .drawer-column')].forEach((column,index)=>add(`${id}.column.${index}`,'drawer-column',column));
    for(const panel of drawer.querySelectorAll(':scope > .drawer-contents > .drawer-column > .panel')){
      const name=[...panel.classList].find(name=>name.endsWith('-panel'))?.slice(0,-6);
      if(name)add(`${id}.panel.${name.split('_').map(part=>part[0].toUpperCase()+part.slice(1)).join('')}`,'drawer-panel',panel.matches('.layers-panel,.palettes-panel,.adjustments-panel')?panel:panel.firstElementChild??panel);
    }
    const inspector=drawer.querySelector('.layer-inspector');
    if(inspector){add(`${id}.panel.Layers`,'drawer-panel',inspector);add(`${id}.panel.Properties`,'drawer-panel',inspector.querySelector('.properties-panel').firstElementChild);}
    add(`${id}.color.wheel`,'color-wheel',drawer.querySelector('.color-wheel-stage'));
    for(const [part,role,query]of [['body','panel-body','.palette-body'],['history','grid','.palette-history'],['scroll','scroll','.palette-scroll'],['footer','toolbar','.palette-footer'],['name','button','.palette-name'],['detail','label','.palette-detail']])add(`${id}.palettes.${part}`,role,drawer.querySelector(`.palettes-panel ${query}`));
    for(const list of drawer.querySelectorAll('.tool-groups,.tool-subtools')){
      const panel=list.closest('[data-control]').dataset.control,kind=list.classList.contains('tool-groups')?'groups':'subtools';
      [...list.children].forEach((row,index)=>{
        const key=`${id}.${panel}.${kind}.${index}`;add(key,'row',row);
        add(`${key}.icon`,'icon',row.querySelector('.tool-caption-icon,.tool-choice-icon'));add(`${key}.label`,'label',row.querySelector('.tool-caption-name,.tool-choice-name'));add(`${key}.preview`,'preview',row.querySelector('.brush-preview'));
      });
    }
    for(const [key,query]of [['layers','.layer-inspector'],['layers.rows','.layer-rows'],['layers.footer','.layer-inspector-footer'],['color','.color-wheel-control'],['color.disc','.color-wheel-control canvas']])
      add(`${id}.${key}`,'context',drawer.querySelector(query));
  }
  [...root.querySelectorAll('.drawer-bridge')].forEach((node,index)=>add(index===0?'drawer.bridge':`drawer.bridge.${index}`,'bridge',node));
  return specs;
}

export async function writeDrawerAudit(evaluate, path, state) {
  const actual=await evaluate(`(${measurePhoneAuditElements.toString()})((${phoneAuditDrawerSpecifications.toString()})())`);
  const metadata=await evaluate(`(()=>{const s=layerApp.state(),font=getComputedStyle(document.documentElement);return{viewport:[innerWidth,innerHeight],theme:s.theme,dpr:devicePixelRatio,text_scale:parseFloat(font.fontSize)/(11*4/3),phone_active:!!layerApp.app.layout(innerWidth,innerHeight).phone,preset:JSON.parse(layerApp.app.workspace_view()).id,font:font.fontFamily,font_px:parseFloat(font.fontSize)}})()`);
  await writeFile(path,JSON.stringify({schema_version:1,host:'web',state,...metadata,status:actual.missing.length?'missing':'captured',...actual,expected_differences:[]},null,2)+'\n');
}

export function phoneAuditFrameSpecifications(tiles) {
  return [
    ...[
      ['bar.tools','bar','#phone-bar-tools'],['bar.capy','bar','#phone-bar-capy'],
      ['chip.tools','chip','#phone-tools-chip'],['chip.capy','chip','#phone-sliders-chip'],
      ['capy','tile','#phone-frame #zen-capy'],['tile.settings','tile','#phone-tool-settings'],
    ].map(([id,role,selector])=>({id,role,selector})),
    ...tiles.filter(tile=>tile.control.kind!=='divider').map(tile=>({id:`tile.${tile.id}`,role:'tile',selector:`#phone-tool-${tile.id}`})),
    ...['size','strength'].flatMap(name=>{
      const selector=`#phone-frame #component-slider-${name}`;
      return [
        {id:`slider.${name}`,role:'slider',selector},
        {id:`slider.${name}.track`,role:'trough',selector:`${selector} .interval-trough, ${selector} .number-track`},
        {id:`slider.${name}.handle`,role:'slider',selector:`${selector} .interval-thumb.lower, ${selector} .toolbar-slider-thumb`},
      ];
    }),
  ];
}

const unavailable = [
  ['inventory-hidden',['bar.capy','chip.capy']],
  ['inventory-swapped',['bar.tools','bar.capy']],
  ['inventory-menu-root',['menu','menu.list','menu.separator','menu.tile.preferences','menu.tile.search','menu.tile.finger','menu.tile.zen','menu.tile.swap']],
  ['inventory-menu-drawings',['menu','menu.page','menu.back']],
  ['inventory-menu-file',['menu','menu.page','menu.back']],
  ['inventory-menu-workspaces',['menu','menu.page','menu.back']],
  ['inventory-capy-layers',['drawer','drawer.bridge']],
  ['slider-track',['slider.size','slider.size.preview']],
  ['slider-away',['slider.size','slider.size.preview']],
  ['finger-hold',['finger.loupe']],
  ['customize-insert-two',['page','page.header','page.primary']],
  ['customize-capacity',['page','page.header','page.primary']],
  ['customize-capacity-divider',['page','page.header','page.primary']],
  ['customize-offbar-cancelled',['bar.tools','chip.tools']],
  ['customize-phone-restore-confirm',['alert']],
  ['customize-phone-restored',['bar.tools','chip.tools']],
  ...[
    ['sketch',['adjustments','select','scalerotate','drawingbrush','sculpt','eraser','layers','color']],
    ['paint',['select','gradient','drawingbrush','blend','eraser','fill','layers','color']],
    ['photo',['crop','select','healing','drawingbrush','eraser','adjustments','layers','color']],
  ].flatMap(([preset,controls])=>controls.map(control=>[`inventory-${preset}-${control}`,['drawer','drawer.bridge']])),
  ...['drawingbrush','fill','select'].map(control=>[`inventory-settings-${control}`,['drawer','drawer.bridge']]),
];

const pages = [
  {id:'preferences-root',action:{type:'invoke',command:'settings'},selector:'#settings[open]',header:'.preferences-content > .dialog-header'},
  ...['appearance','canvas','color','input','shortcuts','about'].map(page=>({id:`preferences-${page}`,action:{type:'open_settings',page},selector:'#settings[open]',header:'.preferences-content > .dialog-header'})),
  {id:'new-drawing',action:{type:'invoke',command:'new_document'},selector:'#new-drawing-dialog[open]',header:'.dialog-header'},
  {id:'canvas-size',action:{type:'invoke',command:'canvas_size'},selector:'#canvas-size-dialog[open]',header:'header'},
  {id:'image-size',action:{type:'invoke',command:'image_size'},selector:'#image-size-dialog[open]',header:'header'},
  {id:'properties',action:{type:'invoke',command:'document_properties'},selector:'dialog.document-dialog[open]',header:'header'},
];

export async function checkPhoneAudit({call,evaluate,settle,frameOnly=false,drawersOnly=false}) {
  const {createDrawing}=await import('./new-drawing-fixture.mjs');
  const directory=process.env.LAYER_TEST_ARTIFACTS||'/mnt/localssd/capy9-phone-impl/web-phone-audit';
  await mkdir(directory,{recursive:true});
  const root=fileURLToPath(new URL('../..',import.meta.url));
  const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
  const wasm=await readFile(new URL('./pkg/layer_web_bg.wasm',import.meta.url));
  const servedWasm=await evaluate(`(async()=>{const url=new URL('pkg/layer_web_bg.wasm',location.href),response=await fetch(url,{cache:'no-store'});if(!response.ok)throw Error('Audit Wasm HTTP '+response.status);const bytes=await response.arrayBuffer(),digest=await crypto.subtle.digest('SHA-256',bytes);return{url:url.href,bytes:bytes.byteLength,sha256:[...new Uint8Array(digest)].map(b=>b.toString(16).padStart(2,'0')).join('')};})()`);
  assert.equal(servedWasm.sha256,hash(wasm),'Served Wasm must match the recorded local binary');
  const source={commit:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),
    tracked_diff_sha256:hash(execFileSync('git',['diff','HEAD','--binary'],{cwd:root})),
    wasm:servedWasm,
    audit_sha256:createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex'),
    runtime_source_attestation:'not recorded by the current Wasm build'};
  const report={schema_version:1,host:'web',source,cases:[],unreachable:[]};
  const original=await evaluate(`({settings:layerApp.state().settings,text:document.documentElement.style.getPropertyValue('--ui-text-size'),workspace:JSON.parse(layerApp.app.workspace_view()).id,documents:layerApp.app.document_tabs(0).tabs.map(tab=>({id:String(tab.id),modified:tab.modified}))})`);
  assert.equal(original.documents.length,1,'Audit starts with one isolated startup drawing');
  assert.equal(original.documents[0].modified,false,'Audit may only close an unmodified startup drawing');
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else requestAnimationFrame(check);}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)});null`);await settle();};
  const close=async()=>{
    if(await evaluate('!!document.querySelector("dialog[open]")')){
      await evaluate('document.querySelector("dialog[open]").requestClose()');
      await settle();
    }
    await wait('!document.querySelector("dialog[open]")');
    await wait('!layerApp.state().requests.length');
  };
  const collect=specifications=>evaluate(`(${measurePhoneAuditElements.toString()})(${JSON.stringify(specifications)})`);
  const capture=async (test,state,specifications,availability,requested_page=null)=>{
    const actual=await collect(specifications);
    const context=actual.elements.filter(element=>element.role==='context');
    actual.elements=actual.elements.filter(element=>element.role!=='context');
    const metadata=await evaluate(`(()=>{const s=layerApp.state(),font=getComputedStyle(document.documentElement),preferences=layerApp.app.preferences(),visible=selector=>{const n=document.querySelector(selector);return !!n?.getClientRects().length&&getComputedStyle(n).display!=='none';};return {viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,theme:s.theme,font_px:parseFloat(font.fontSize),font:font.fontFamily,phone_active:!!layerApp.app.layout(innerWidth,innerHeight).phone,workspace:JSON.parse(layerApp.app.workspace_view()).id,url:location.href,preferences:preferences?{page:preferences.page,page_requested:preferences.page_requested??null,sidebar_visible:visible('.preferences-sidebar'),content_visible:visible('.preferences-content')}:null}})()`);
    assert.deepEqual(metadata.viewport,test.viewport,'Capture logical viewport');assert.equal(metadata.dpr,1,'Capture DPR');
    assert.equal(metadata.theme,test.theme,'Capture theme');
    metadata.phone_tiles=await evaluate('layerApp.state().workspace.layout.phone??[]');
    if(frameOnly)metadata.presented_tools=await evaluate('layerApp.app.phone_tools()');
    const document=await evaluate(`(()=>{const s=layerApp.state(),tab=s.tabs.find(t=>t.active);return {extent:tab?[tab.width,tab.height]:null,color:layerApp.app.document_color(),count:layerApp.app.document_tabs(0).tabs.length,busy:s.document_file.busy,host_busy:layerApp.documents.busy()};})()`);
    assert.equal(document.count,1,'Captured drawing count');
    assert.deepEqual(document.extent,[1200,1600],'Captured active drawing extent');
    assert.deepEqual(document.color,{space:'Srgb',depth:'U8'},'Captured active drawing color');
    metadata.document=document;
    metadata.camera=await evaluate(`JSON.parse(JSON.stringify(layerApp.state().camera,(_,value)=>typeof value==='bigint'?String(value):value))`);
    if(drawersOnly)metadata.drawer=await evaluate(`(()=>{const s=layerApp.state(),d=s.customization.drawer;if(!d)return null;return JSON.parse(JSON.stringify({model:d,measure:layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}),tools:d.columns.flat().filter(p=>['brushes','brush_sets','sculpt_sets','tools'].includes(p)).map(panel=>({panel,view:layerApp.app.tool_panel(panel,d.anchor)}))},(_,value)=>typeof value==='bigint'?String(value):value));})()`);
    assert.ok(Math.abs(metadata.font_px-test.text_pt*4/3)<.05,`Measured ${metadata.font_px}px does not match ${test.text_pt}pt`);
    const issues=[];
    if(requested_page){
      assert.ok(metadata.preferences,'Requested Settings page must have a live model');
      if(requested_page==='list'){
        if(!metadata.preferences.sidebar_visible||metadata.preferences.content_visible)issues.push('Plain Settings did not land on the phone page list');
      } else {
        assert.equal(metadata.preferences.page,requested_page,'Requested Settings model page');
        assert.equal(metadata.preferences.content_visible,true,'Requested Settings content must be rendered');
      }
    }
    const stem=`phone-${test.viewport.join('x')}-${test.theme}-${test.text_pt}pt-${state}`;
    const record={schema_version:1,host:'web',state,viewport:metadata.viewport,theme:metadata.theme,dpr:metadata.dpr,text_scale:test.text_scale,preset:metadata.workspace,phone_active:metadata.phone_active,
      status:!metadata.phone_active?'gated':actual.missing.length||issues.length?'missing':'captured',availability,requested_page,issues,
      source:{...source,...metadata,text_pt:test.text_pt,context},...actual,expected_differences:[]};
    const png=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/${stem}.png`,Buffer.from(png.data,'base64'));
    await writeFile(`${directory}/${stem}.allocations.json`,JSON.stringify(record,null,2)+'\n');
    report.cases.push({state,viewport:test.viewport,theme:test.theme,text_scale:test.text_scale,status:record.status,json:`${stem}.allocations.json`,png:`${stem}.png`});
    await writeFile(`${directory}/index.json`,JSON.stringify(report,null,2)+'\n');
    return record;
  };
  await send({type:'invoke',command:'new_document'});
  const setup=await evaluate(createDrawing({width:1200,height:1600,resolution:300,space:'Srgb',depth:'U8',paper:'white',relief:0}));
  assert.equal(setup.resolution,300,'New drawing recipe resolution');
  assert.deepEqual(setup.color,{space:'Srgb',depth:'U8'},'New drawing recipe color');
  assert.deepEqual(setup.paper.color.rgba,[1,1,1,1],'New drawing recipe white paper');
  assert.equal(setup.paper.transparent??false,false,'New drawing recipe opaque paper');
  assert.equal(setup.paper.relief,0,'New drawing recipe relief');
  source.document_recipe=setup;
  await wait(`!layerApp.state().document_file.busy&&!layerApp.documents.busy()&&layerApp.app.brush_ready()&&((tab)=>tab?.width===1200&&tab.height===1600)(layerApp.state().tabs.find(t=>t.active))`);
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'Srgb',depth:'U8'},'Created drawing color');
  await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
  assert.deepEqual(await evaluate('layerApp.state().settings.new_document'),original.settings.new_document,'New drawing future defaults preserved');
  const remaining=await evaluate('layerApp.app.document_tabs(0).tabs.map(tab=>({id:String(tab.id),modified:tab.modified}))');
  assert.ok(remaining.length===1||remaining.length===2,'New drawing opens at most one additional tab');
  if(remaining.length===2){
    const startup=remaining.find(tab=>tab.id===original.documents[0].id);
    assert.ok(startup,'The only old tab must be the isolated startup drawing');
    assert.equal(startup.modified,false,'Startup drawing stayed unmodified');
    await evaluate(`layerApp.documents.close(BigInt(${JSON.stringify(startup.id)}))`);
  }
  await wait('layerApp.app.document_tabs(0).tabs.length===1&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await send({type:'workspace_manager',command:{type:'switch',id:'builtin:workspace:painter'}});
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:painter'&&!JSON.parse(layerApp.app.workspace_view()).busy`);
  const click=async selector=>{
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n||n.disabled)throw Error('Unavailable '+${JSON.stringify(selector)});const b=n.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const closeDrawer=async()=>{
    await send({type:'customize',action:{type:'close_expanded'}});
    if(await evaluate('layerApp.state().commands.find(c=>c.id==="cancel_transform")?.enabled'))await send({type:'invoke',command:'cancel_transform'});
    await wait('!layerApp.state().customization.drawer&&!document.querySelector(".content-drawer")');
  };
  const drawerReady=()=>wait(`(()=>{const d=layerApp.state().customization.drawer,r=document.querySelector('.content-drawer[data-drawer="tool"]');if(!d||!r)return false;const p=layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}).measure;return p&&r.getBoundingClientRect().width>=p.placement.bounds.width-.5;})()`);
  if(drawersOnly){
    await send({type:'invoke',command:'brush'});await wait('layerApp.app.brush_ready()');
    const center=await evaluate('(()=>{const b=document.querySelector("#canvas").getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()');
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...center,button:'left',buttons:1,clickCount:1});
    for(let i=1;i<=4;i++)await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:center.x+5*i,y:center.y,button:'left',buttons:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:center.x+20,y:center.y,button:'left',buttons:0,clickCount:1});
    await wait('layerApp.app.document_park_ready()');
  }
  for(const test of phoneAuditMatrix){
    await close();
    await wait('layerApp.state().workspace_ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
    await call('Emulation.setDeviceMetricsOverride',{width:test.viewport[0],height:test.viewport[1],deviceScaleFactor:1,mobile:false});
    await send({type:'set_theme',theme:test.theme});
    await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(`${test.text_pt}pt`)});null`);
    await settle();
    await send({type:'invoke',command:'fit_canvas'});
    const frame=phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]'));
    const active=await evaluate('!!layerApp.app.layout(innerWidth,innerHeight).phone');
    await capture(test,'rest',[...frame,
      {id:'current.header',role:'context',selector:'#header'},
      {id:'current.canvas',role:'context',selector:'#canvas'},
      {id:'current.capy',role:'context',selector:'#zen-capy'}],active?'phone':'desktop fallback: Web phone presentation is not enabled');
    if(drawersOnly){
      const captureDrawer=async name=>{
        try {await drawerReady();}
        catch(error){
          const stem=`${directory}/phone-${test.viewport.join('x')}-${test.theme}-${name}-failure`;
          const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${stem}.png`,Buffer.from(png.data,'base64'));
          await writeDrawerAudit(evaluate,`${stem}.allocations.json`,name);
          throw Error(`${name}: drawer did not settle`,{cause:error});
        }
        await settle();
        const actual=await evaluate(`(${phoneAuditDrawerSpecifications.toString()})()`);
        const record=await capture(test,name,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),...actual],'existing shared drawer owner');
        assert.equal(record.status,'captured');assert.deepEqual(record.missing,[]);
        const drawer=record.elements.find(e=>e.id==='drawer');
        assert.ok(drawer?.visible&&record.elements.some(e=>e.id==='drawer.bridge'&&e.visible),'The opener retains a visible drawer and bridge');
        assert.equal(record.elements.filter(e=>e.role==='drawer-column').length,(await evaluate('layerApp.state().customization.drawer')).columns.length);
        const naturals=await evaluate(`(()=>{const minimums=layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}).minimums;return [...document.querySelectorAll('.content-drawer[data-drawer="tool"] > .drawer-contents > .drawer-column')].flatMap((column,i)=>[...column.children].map((panel,j)=>{const s=getComputedStyle(panel);return{minimum:minimums[i][j],height:panel.getBoundingClientRect().height+parseFloat(s.marginTop)+parseFloat(s.marginBottom)};}));})()`);
        for(const {minimum,height}of naturals)if(minimum)assert.ok(Math.abs(height-minimum)<=1,`A scroll body keeps its requested natural extent: ${height} versus ${minimum}`);
        return record;
      };
      for(const [preset,tag]of [['painter','sketch'],['illustrator','paint'],['photographer','photo']]){
        await closeDrawer();await send({type:'workspace_manager',command:{type:'switch',id:`builtin:workspace:${preset}`}});
        await wait(`JSON.parse(layerApp.app.workspace_view()).id===${JSON.stringify(`builtin:workspace:${preset}`)}&&!JSON.parse(layerApp.app.workspace_view()).busy`);
        const tiles=await evaluate('layerApp.state().workspace.layout.phone');
        for(const tile of tiles.filter(tile=>tile.control.kind!=='divider')){
          const name=tile.control.command??tile.control.slot??tile.control.panel??tile.control.kind;
          const selector=`#phone-tool-${tile.id}`;
          await click(selector);
          if(!await evaluate('!!layerApp.state().customization.drawer')){
            await wait(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-pressed')==='true'`);
            await click(selector);
          }
          await captureDrawer(`inventory-${tag}-${name.replaceAll('_','')}`);
          const color=await evaluate(`(()=>{const n=document.querySelector('.content-drawer .color-wheel'),c=n?.closest('.drawer-column');if(!n||!c)return null;return{wheel:n.getBoundingClientRect().toJSON(),column:c.getBoundingClientRect().toJSON()}})()`);
          if(color)assert.ok(color.wheel.left>=color.column.left-1&&color.wheel.right<=color.column.right+1&&color.wheel.top>=color.column.top-1&&color.wheel.bottom<=color.column.bottom+1,'The Color wheel stays inside its live column');
          if(color){
            const scroll=await evaluate(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents'),r=n.getBoundingClientRect();return{shared:n.classList.contains('shared-scroll'),extent:n.scrollHeight,height:n.clientHeight,x:r.x+2,y:r.y+r.height/2};})()`);
            assert.equal(scroll.shared,true,'Color uses the shared whole-drawer scroll owner');
            if(scroll.extent>scroll.height+1){
              await call('Input.dispatchMouseEvent',{type:'mouseWheel',x:scroll.x,y:scroll.y,deltaX:0,deltaY:scroll.extent});
              await wait(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents');return n.scrollTop+n.clientHeight>=n.scrollHeight-1;})()`);
              await captureDrawer(`inventory-${tag}-${name.replaceAll('_','')}-bottom`);
              assert.equal(await evaluate(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents').getBoundingClientRect(),f=document.querySelector('.content-drawer .palette-footer').getBoundingClientRect();return f.top>=n.top-1&&f.bottom<=n.bottom+1;})()`),true,'The Color footer is reachable at the whole drawer bottom');
            }
          }
          const rows=await evaluate('document.querySelectorAll(".content-drawer .tool-subtools .tool-choice-button").length');
          if(rows){
            await evaluate(`(()=>{const n=document.querySelector('.content-drawer .tool-subtools .tool-choice-button:last-child').closest('.drawer-column');n.scrollTop=n.scrollHeight;})()`);await settle();
            await captureDrawer(`inventory-${tag}-${name.replaceAll('_','')}-bottom`);
          }
          if(name==='sculpt'){
            const previous=await evaluate(`document.querySelector('.content-drawer .tool-groups button[aria-pressed="true"]').getAttribute('aria-label')`);
            const edge=await evaluate(`(()=>{for(const n of document.querySelectorAll('.content-drawer .tool-groups button[aria-pressed="false"]:not(:disabled)')){n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect(),x=r.right-2,y=r.y+r.height/2;if(n.contains(document.elementFromPoint(x,y)))return{x,y,label:n.getAttribute('aria-label')};}throw Error('No unobscured unselected group edge');})()`);
            for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:edge.x,y:edge.y,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
            await wait(`[...document.querySelectorAll('.content-drawer .tool-groups button')].some(n=>n.getAttribute('aria-label')===${JSON.stringify(edge.label)}&&n.getAttribute('aria-pressed')==='true')`);
            await click(`.content-drawer .tool-groups button[aria-label=${JSON.stringify(previous)}]`);
            await wait(`[...document.querySelectorAll('.content-drawer .tool-groups button')].some(n=>n.getAttribute('aria-label')===${JSON.stringify(previous)}&&n.getAttribute('aria-pressed')==='true')`);
          }
          if(tile.control.panel==='layers'){
            await click('.content-drawer .layer-properties-disclosure');
            await wait(`getComputedStyle(document.querySelector('.content-drawer .layer-properties-pages > .layers-panel')).visibility==='hidden'`);
            await captureDrawer(`inventory-${tag}-layers-properties`);
            await click('.content-drawer .layer-properties-disclosure');
            await wait(`getComputedStyle(document.querySelector('.content-drawer .layer-properties-pages > .properties-panel')).visibility==='hidden'`);
          }
          await closeDrawer();
        }
        await click('#phone-tool-settings');await captureDrawer(`inventory-${tag}-settings`);
        assert.deepEqual((await evaluate('layerApp.state().customization.drawer')).columns,[['tool_settings']]);
      }
      await closeDrawer();
      for(const panel of ['stats','histogram']){
        await send({type:'phone',action:{kind:'panel',panel}});await captureDrawer(`inventory-window-${panel}`);await closeDrawer();
      }
      const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
      for(const swapped of [false,true]){
        if(await evaluate('layerApp.state().phone.swapped')!==swapped)await send({type:'phone',action:{kind:'swap_sides'}});
        for(const material of [0,2]){
          await send({type:'preferences',action:{type:'edit',id:'transparency',value:material}});
          await send({type:'phone',action:{kind:'panel',panel:'layers'}});
          await captureDrawer(`drawer-capy-layers-${swapped?'swapped':'normal'}-${material?'glass':'opaque'}`);
          const anchor=await evaluate('layerApp.state().customization.drawer.anchor');
          assert.equal(anchor.kind,'phone');assert.equal(anchor.anchor.kind,'capy');
          await closeDrawer();await click('#phone-tool-settings');
          await captureDrawer(`drawer-settings-${swapped?'swapped':'normal'}-${material?'glass':'opaque'}`);
          await closeDrawer();
        }
      }
      await send({type:'phone',action:{kind:'swap_sides'}});
      await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
      await send({type:'phone',action:{kind:'panel',panel:'layers'}});
      await drawerReady();await settle();
      const count=await evaluate('layerApp.state().layers.length');
      const originalHeight=await evaluate('document.querySelector(".content-drawer").getBoundingClientRect().height');
      for(let i=0;i<24;i++){
        await click('.content-drawer .layer-footer [data-footer-button="new_layer"]');
        await wait(`layerApp.state().layers.length===${count+i+1}`);
      }
      await wait(`(()=>{const rows=document.querySelector('.content-drawer .layer-rows'),id=layerApp.state().layer_tools.editing_layer.id,row=rows.querySelector('[data-layer="'+id+'"]');if(!row)return false;const b=rows.getBoundingClientRect(),r=row.getBoundingClientRect();return rows.scrollHeight>rows.clientHeight&&r.top>=b.top-1&&r.bottom<=b.bottom+1;})()`);
      const grown=await captureDrawer('layers-live-capped');
      const grownHeight=grown.elements.find(e=>e.id==='drawer').bounds[3];
      const budget=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone;return p.edge==='top'?p.work_area.height:p.bars[0].height;})()`);
      assert.ok(Math.abs(grownHeight-budget)<=1,'Many layers reach the shared live phone budget');
      assert.ok(originalHeight>=budget-1||grownHeight>originalHeight,'Layers grows when its initial content is below the cap');
      for(let i=0;i<24;i++){
        await send({type:'invoke',command:'undo'});
        await wait(`layerApp.state().layers.length===${count+23-i}`);
      }
      await closeDrawer();
      continue;
    }
    if(frameOnly){
      assert.equal(active,true,'Web phone capability is presented');
      const check=async(name)=>{
        const p=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone');
        const record=await capture(test,name,phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),'shared phone frame');
        assert.deepEqual(record.missing,[],`${name}: every frame element is present`);
        const expected=new Map([
          ['bar.tools',p.bars[0]],['bar.capy',p.bars[1]],['chip.tools',p.chips[0]],['chip.capy',p.chips[1]],
          ['capy',p.capy],['tile.settings',p.settings],['slider.size',p.size],['slider.strength',p.strength],
          ...record.source.phone_tiles.filter(t=>t.control.kind!=='divider').map(t=>[`tile.${t.id}`,p.tools[record.source.phone_tiles.indexOf(t)]]),
        ]);
        for(const [id,bounds]of expected){
          const actual=record.elements.find(e=>e.id===id);assert.ok(actual,id);
          const visible=bounds.width>0&&bounds.height>0;assert.equal(actual.visible,visible,`${name}/${id}: visibility`);
          if(visible)[bounds.x,bounds.y,bounds.width,bounds.height].forEach((v,i)=>assert.ok(Math.abs(actual.bounds[i]-v)<=1,`${name}/${id}: ${actual.bounds} vs ${JSON.stringify(bounds)}`));
        }
        for(const name of ['size','strength']){
          const slider=record.elements.find(e=>e.id===`slider.${name}`),track=record.elements.find(e=>e.id===`slider.${name}.track`);
          if(!slider.visible)continue;
          const [x,y,w,h]=slider.bounds,[tx,ty,tw,th]=track.bounds;
          if(p.edge==='top')assert.ok(Math.abs(tx-x)<=1&&Math.abs(tw-w)<=1&&Math.abs(ty+th/2-y-h/2)<=1,'The horizontal trough spans and centers on its slider allocation');
          else assert.ok(Math.abs(ty-y)<=1&&Math.abs(th-h)<=1&&Math.abs(tx+tw/2-x-w/2)<=1,'The vertical trough spans and centers on its slider allocation');
        }
        assert.equal(await evaluate('document.querySelector("#header").getClientRects().length'),0,'Phone hides the desktop header');
        const selected=await evaluate('[...document.querySelectorAll("#phone-frame .phone-tool[aria-pressed=true]")].filter(n=>n.checkVisibility()).map(n=>getComputedStyle(n).backgroundColor)');
        assert.ok(selected.every(color=>color!=='rgba(0, 0, 0, 0)'),'Selected phone tools have a visible selection surface');
        assert.deepEqual(await evaluate('[layerApp.state().phone.native,layerApp.state().phone.native_counts,layerApp.state().phone.movable]'),[[0,0],[0,0],false]);
      };
      await check('frame-rest');
      await send({type:'phone',action:{kind:'swap_sides'}});await check('frame-swapped');
      await send({type:'invoke',command:'zen_mode'});await check('frame-hidden');
      await evaluate('document.querySelector("#zen-capy").click()');await settle();
      assert.equal(await evaluate('layerApp.state().phone.hidden'),false,'The same Capy reveals the bars');
      await check('frame-revealed');
      await send({type:'phone',action:{kind:'swap_sides'}});
      const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
      for(const value of [0,3]){
        await send({type:'preferences',action:{type:'edit',id:'transparency',value}});
        await check(value===0?'frame-opaque':'frame-glass');
      }
      await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
      await evaluate('window.phoneFrameWorkspace=layerApp.state().workspace;null');
      await send({type:'phone',action:{kind:'insert_tools',before:null}});
      for(const command of ['drawing_brush','eraser'])await send({type:'customize',action:{type:'picker_select',control:{kind:'command',command},selected:true}});
      await send({type:'customize',action:{type:'confirm_tools'}});
      assert.equal(await evaluate('layerApp.state().workspace.layout.phone.length'),10);
      await check('frame-ten-tools');
      await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:{...phoneFrameWorkspace,layout:{...phoneFrameWorkspace.layout,phone:[]}}});null');await settle();
      await check('frame-empty');
      await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:phoneFrameWorkspace});delete window.phoneFrameWorkspace;null');await settle();
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:19,right:7,bottom:13,left:5}});
      await wait('JSON.stringify(layerApp.state().phone.insets)===JSON.stringify([19,7,13,5])');
      await check('frame-safe-insets');
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:13,right:5,bottom:19,left:7}});
      await wait('JSON.stringify(layerApp.state().phone.insets)===JSON.stringify([13,5,19,7])');
      await check('frame-safe-redistributed');
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:0,right:0,bottom:0,left:0}});
      await wait('layerApp.state().phone.insets.every(v=>v===0)');
      for(const name of ['size','strength']){
        const selector=`#component-slider-${name} input.number-slider`;
        const b=await evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
        const p=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone');
        const vertical=p.edge!=='top',start={x:b.x+(vertical?2:b.width/2),y:b.y+(vertical?b.height/2:2)};
        const allocation=p[name];
        assert.ok(Math.abs((vertical?b.width:b.height)-(vertical?allocation.width:allocation.height))<=1,'Slider input covers the full bar thickness');
        assert.equal(await evaluate(`document.elementFromPoint(${start.x},${start.y})?.matches('input.number-slider')`),true,'The bar-edge contact reaches the slider');
        const mouse=(type,point)=>call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
        await mouse('mousePressed',start);await mouse('mouseReleased',start);await settle();
        await wait('!!document.querySelector(".toolbar-brush-preview:popover-open")');
        const preview={id:`slider.${name}.preview`,role:'preview',selector:'.toolbar-brush-preview:popover-open'};
        await capture(test,`slider-${name}-tap`,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),preview],'existing shared slider preview');
        const strip=await evaluate(`document.querySelector(${JSON.stringify(`#component-slider-${name}`)}).getBoundingClientRect().toJSON()`);
        const end=vertical?{x:strip.right+2*p.tile,y:test.viewport[1]*.65}:{x:test.viewport[0]*.65,y:strip.y-2*p.tile};
        await mouse('mousePressed',start);await mouse('mouseMoved',end);await settle();
        const actual=await evaluate(`layerApp.app.toolbar_component({kind:${JSON.stringify(name==='size'?'brush_size_slider':'brush_strength_slider')}}).numeric`);
        const expected=await evaluate(`layerApp.app.number_input({control:layerApp.app.toolbar_component({kind:${JSON.stringify(name==='size'?'brush_size_slider':'brush_strength_slider')}}).numeric.numeric,value:${actual.value},operation:{type:'position',position:${vertical ? .35 : .65}}}).value`);
        assert.ok(Math.abs(actual.value-expected)<=Math.max(.001,Math.abs(expected)*.001),`${name}: far mapping uses the window extent, ${actual.value} vs ${expected}`);
        await capture(test,`slider-${name}-away`,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),preview],'shared window-extent slider mapping');
        await mouse('mouseReleased',end);await settle();
        assert.equal(await evaluate('!!document.querySelector(".toolbar-brush-preview:popover-open")'),false,'A drag lift closes its preview');
      }
      continue;
    }
    for(const page of pages){
      await send(page.action);
      await wait(`!!document.querySelector(${JSON.stringify(page.selector)})`);
      const settings=page.selector.startsWith('#settings');
      const header=settings?await evaluate(`getComputedStyle(document.querySelector('.preferences-content')).display==='none'?'.preferences-sidebar > .dialog-header':'.preferences-content > .dialog-header'`):page.header;
      if(page.action.page)assert.equal(await evaluate(`document.querySelector('.preferences-pages > :not([hidden])')?.getAttribute('aria-label')`),
        await evaluate(`layerApp.app.preferences().pages.find(p=>p.id===${JSON.stringify(page.action.page)}).title`),'Requested Settings DOM page');
      await capture(test,page.id,[...frame,
        {id:'page',role:'page',selector:page.selector},
        {id:'page.header',role:'page-header',selector:`${page.selector} ${header}`},
        {id:settings||page.id==='properties'?'page.done':'page.primary',role:'page-action',selector:`${page.selector} ${header} .suggested-action`},
        {id:'page.back',role:'page-action',selector:`${page.selector} ${header} .preferences-back`,optional:true}],
      'existing Web editor; phone page/header parity not established',settings?page.action.page??'list':null);
      await close();
    }
    for(const [state,missing]of unavailable){
      report.unreachable.push({state,viewport:test.viewport,theme:test.theme,dpr:1,text_scale:test.text_scale,preset:'builtin:workspace:painter',phone_active:active,status:active?'unavailable':'gated',missing,
        reason:active?'The existing phone audit has no interaction visitor for this state yet':'Web phone presentation is GTK-gated; target view is not reachable',png:null});
    }
    await writeFile(`${directory}/index.json`,JSON.stringify(report,null,2)+'\n');
  }
  if(frameOnly){
    const settings=await evaluate('layerApp.state().settings');
    await send({type:'restore_settings',settings:{...settings,finger_drawing:'on'}});
    await send({type:'invoke',command:'brush'});
    await wait('layerApp.app.brush_ready()');
    const origin=await evaluate('(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone.work_area;return{x:p.x+p.width*.4,y:p.y+p.height*.5};})()');
    const touch=(type,touchPoints=[])=>call('Input.dispatchTouchEvent',{type,touchPoints});
    for(const cancelled of [false,true]){
      const history=await evaluate('layerApp.state().commands.filter(c=>c.id==="undo"||c.id==="redo").map(c=>[c.id,c.enabled])');
      await evaluate('layerApp.app.start_stroke_recording();null');
      await touch('touchStart',[{id:100,...origin}]);
      for(let i=1;i<=4;i++)await touch('touchMove',[{id:100,x:origin.x+5*i,y:origin.y}]);
      await wait('Number(layerApp.app.stroke_recording_status().raw_events)>=3');
      if(cancelled)await touch('touchStart',[{id:100,x:origin.x+20,y:origin.y},{id:101,x:origin.x+60,y:origin.y}]);
      await touch('touchEnd');
      await wait('layerApp.app.document_park_ready()');
      const data=await evaluate('(()=>{layerApp.app.stop_stroke_recording();return Array.from(layerApp.app.stroke_recording_data());})()');
      const path=`${directory}/finger-${cancelled?'withdrawn':'paint'}.capystrokes`;
      await writeFile(path,Buffer.concat([Buffer.from('CAPYPEN4'),gzipSync(Buffer.from(data))]));
      const records=execFileSync('cargo',['run','--locked','-q','-p','layer-engine','--features','prediction-bench','--example','stroke-recording','--','dump',path],{cwd:root,encoding:'utf8'}).trim().split('\n').map(line=>JSON.parse(line));
      const events=records.flatMap(record=>record.Raw?[record.Raw.event]:[]);
      assert.ok(events.length>=3,'Native browser touch passes the packed Wasm paint ABI');
      assert.ok(events.every(event=>event.tool==='Finger'&&event.pressure===1),'Every decoded browser paint sample is Finger at full pressure');
      assert.equal(events.filter(event=>event.phase==='Cancel').length,cancelled?1:0,'Withdrawal cancels exactly once');
      const ends=records.flatMap(record=>record.End?[record.End]:[]);
      assert.equal(ends.length,1,'One engine stroke ends per native contact');
      assert.equal(ends[0].cancelled,cancelled,'Withdrawal rolls back the provisional stroke');
      assert.equal(ends[0].interrupted,false,'The recording includes the engine stroke end before it stops');
      if(!cancelled)assert.equal(events.at(-1).phase,'Up');
      await writeFile(`${path}.json`,JSON.stringify(records,null,2)+'\n');
      await evaluate('layerApp.app.stroke_recording_saved();null');
      if(!cancelled){
        assert.equal(await evaluate('layerApp.state().commands.find(c=>c.id==="undo").enabled'),true,'The completed stroke is undoable');
        await send({type:'invoke',command:'undo'});
        await wait('layerApp.app.document_park_ready()');
        assert.equal(await evaluate('layerApp.state().commands.find(c=>c.id==="redo").enabled'),true,'Undo completes before the next recorded contact');
      }
      else assert.deepEqual(await evaluate('layerApp.state().commands.filter(c=>c.id==="undo"||c.id==="redo").map(c=>[c.id,c.enabled])'),history,'Withdrawal preserves artwork history');
    }
    await send({type:'restore_settings',settings});
    const durable=await evaluate('JSON.stringify(layerApp.app.workspace_persistence())');
    await evaluate('window.phoneFrameCapy=document.querySelector("#zen-capy");phoneFrameCapy.focus();null');
    await call('Emulation.setDeviceMetricsOverride',{width:600,height:700,deviceScaleFactor:1,mobile:false});await settle();
    await wait('!layerApp.app.layout(innerWidth,innerHeight).phone&&document.querySelector("#header").getClientRects().length>0&&!document.querySelector("#phone-frame").getClientRects().length');
    assert.equal(await evaluate('document.activeElement===phoneFrameCapy'),true,'Returning to desktop preserves Capy focus');
    assert.equal(await evaluate('JSON.stringify(layerApp.app.workspace_persistence())'),durable,'Leaving phone mode preserves the authored workspace');
    await call('Emulation.setDeviceMetricsOverride',{width:412,height:891,deviceScaleFactor:1,mobile:false});await settle();
    await wait('!!layerApp.app.layout(innerWidth,innerHeight).phone&&document.querySelector("#phone-frame").getClientRects().length>0');
    assert.equal(await evaluate('document.querySelector("#zen-capy")===phoneFrameCapy&&document.activeElement===phoneFrameCapy'),true,'The same focused Capy returns to phone mode');
    assert.equal(await evaluate('JSON.stringify(layerApp.app.workspace_persistence())'),durable,'Entering phone mode preserves the authored workspace');
    await evaluate('delete window.phoneFrameCapy;null');
  }
  await close();
  await send({type:'workspace_manager',command:{type:'switch',id:original.workspace}});
  await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(original.text)});null`);
  await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
  await call('Emulation.clearDeviceMetricsOverride');
  console.log(`PHONE ${frameOnly?'FRAME':drawersOnly?'DRAWERS':'AUDIT'}: ${report.cases.length} captures; ${report.unreachable.length} unreachable states; ${directory}/index.json ${frameOnly?'(shared allocation checks passed; cross-host pixels pending)':'(not a parity pass)'}`);
}
