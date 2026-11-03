import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

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
    elements.push({id,role,bounds,visible,source:{selector,dom_id:node.id||null,type:node.tagName.toLowerCase()}});
  }
  return {elements,missing};
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

export async function checkPhoneAudit({call,evaluate,settle}) {
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
    const document=await evaluate(`(()=>{const s=layerApp.state(),tab=s.tabs.find(t=>t.active);return {extent:tab?[tab.width,tab.height]:null,color:layerApp.app.document_color(),count:layerApp.app.document_tabs(0).tabs.length,busy:s.document_file.busy,host_busy:layerApp.documents.busy()};})()`);
    assert.equal(document.count,1,'Captured drawing count');
    assert.deepEqual(document.extent,[1200,1600],'Captured active drawing extent');
    assert.deepEqual(document.color,{space:'Srgb',depth:'U8'},'Captured active drawing color');
    metadata.document=document;
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
  for(const test of phoneAuditMatrix){
    await close();
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
  await close();
  await send({type:'workspace_manager',command:{type:'switch',id:original.workspace}});
  await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(original.text)});null`);
  await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
  await call('Emulation.clearDeviceMetricsOverride');
  console.log(`PHONE AUDIT: ${report.cases.length} captures; ${report.unreachable.length} unreachable states; ${directory}/index.json (not a parity pass)`);
}
