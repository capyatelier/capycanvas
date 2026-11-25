import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

function menuGeometry() {
  const q=(selector,...roots)=>(roots.length?roots[0]:document)?.querySelector(selector);
  const rect=node=>node?.getClientRects().length?node.getBoundingClientRect().toJSON():null;
  const textRect=(node,start=0,end=node?.textContent.length)=>{
    if(!node?.firstChild||node.firstChild.nodeType!==Node.TEXT_NODE)return null;
    const range=document.createRange();range.setStart(node.firstChild,start);range.setEnd(node.firstChild,end);
    return range.getBoundingClientRect().toJSON();
  };
  const list=q('#phone-menu-scroll'),page=q('.menu-page:not([hidden]):not([inert])'),l=rect(list);
  const drawing=q('#phone-drawing',page),dr=rect(drawing),caption=q('.menu-caption',drawing),text=textRect(caption);
  const row=q('#phone-menu-switcher-row',page),well=q('#phone-menu-switcher',row),choices=q('.phone-menu-switcher-choices',well);
  const pill=q('.phone-menu-switch',well),options=q('#phone-menu-switcher-options',well),caret=q('.submenu-arrow',row),lead=q('.menu-switcher-lead',row);
  const sr=rect(row),wr=rect(well),pr=rect(pill),cr=rect(caret),lr=rect(lead),or=rect(options);
  const padding=node=>node?['Top','Right','Bottom','Left'].map(side=>parseFloat(getComputedStyle(node)[`padding${side}`])):null;
  const strips=['preferences','search','finger','zen','swap'].map(name=>{
    const tile=q(`#phone-${name}`);return {name,tile:rect(tile),icon:rect(q('svg',tile)),asset:tile?.dataset.icon};
  });
  const back=q('.submenu-back .menu-back-arrow',page),backIcon=q('svg',back),br=rect(back),bi=rect(backIcon);
  const drawings=[...(page?.querySelectorAll('.menu-drawing-row')??[])].map(line=>{
    const row=q('.menu-row',line),gutter=q('.menu-check',row),check=q('svg',gutter),cap=q('.menu-caption',row),close=q('.menu-close',line),cross=q('svg',close);
    const gr=rect(gutter),ci=rect(check),xr=rect(cross),cl=rect(close),tr=textRect(cap);
    const modified=cap?.textContent.startsWith('• '),dot=modified?textRect(cap,0,1):null,name=modified?textRect(cap,2):tr;
    return {id:row.id,label:cap?.textContent,checked:row.getAttribute('aria-checked'),row:rect(row),gutter:gr,check:ci,caption:rect(cap),text:tr,close:cl,cross:xr,dot,name,
      check_gutter_x_from_list:gr?gr.left-l.left:null,check_icon_x_from_list:ci?ci.left-l.left:null,back_arrow_x_from_list:br?br.left-l.left:null,
      back_icon_x_from_list:bi?bi.left-l.left:null,close_trailing_inset:l.right-(xr?.right??l.right),close_hit_trailing_inset:l.right-(cl?.right??l.right),
      unsaved_dot_to_name_gap:dot&&name?name.left-dot.right:null};
  });
  return {viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,theme:layerApp.state().theme,workspace:JSON.parse(layerApp.app.workspace_view()).id,
    route:layerApp.state().phone.menu_route,menu:rect(q('#phone-menu')),list:l,page:rect(page),drawing_row:dr,drawing_text:text,
    switcher_row:sr,well:wr,lead:lr,caret:cr,options:or,pill_rect:pr,pill_padding:padding(pill),well_padding:padding(well),
    list_width:l?.width??null,row_height:dr?.height??rect(q('.menu-row',page))?.height??null,
    switcher_row_height:sr?.height??null,well_height:wr?.height??null,pill_height:pr?.height??null,pill_horizontal_padding:pill?[padding(pill)[3],padding(pill)[1]]:null,
    gap_below_switcher:sr&&dr?dr.top-sr.bottom:null,leading_spacer:lr?.width??null,
    well_to_caret_gap:wr&&cr?cr.left-wr.right:null,well_to_caret_reserved:caret?parseFloat(getComputedStyle(caret).marginInlineStart):null,
    caret_x_from_list_left:cr&&l?cr.left-l.left:null,caret_x_from_list_right:cr&&l?l.right-cr.left:null,caret_trailing_inset:cr&&l?l.right-cr.right:null,
    drawing_row_text_x:dr&&text&&l?text.left-l.left:null,back_arrow:br,back_arrow_icon:bi,drawings,
    strip_tile_size:strips[0].tile?{width:strips[0].tile.width,height:strips[0].tile.height}:null,
    strip_icon_size:strips[0].icon?{width:strips[0].icon.width,height:strips[0].icon.height}:null,strip:strips,
    choices:choices?{bounds:rect(choices),scroll_left:choices.scrollLeft,scroll_width:choices.scrollWidth,client_width:choices.clientWidth,
      pills:[...choices.children].map(p=>({id:p.id,label:p.textContent,selected:p.getAttribute('aria-pressed'),bounds:rect(p)}))}:null};
}

export async function checkPhoneSwitcher({call,evaluate,settle}) {
  const match=/^(412x891|891x412)-(light|dark)-(mouse|touch)$/.exec(process.env.LAYER_PHONE_SWITCHER_CASE??'');
  assert.ok(match,'LAYER_PHONE_SWITCHER_CASE must identify an exact logical size, theme and pointer type');
  const [,size,theme,device]=match,viewport=size.split('x').map(Number),output=process.env.LAYER_TEST_ARTIFACTS;
  assert.ok(output,'LAYER_TEST_ARTIFACTS must be a private capture directory');await mkdir(output,{recursive:true});
  const report={schema:1,host:'web',units:'CSS logical px',viewport,theme,device,measurements:{},steps:[],failures:[]};
  const save=()=>writeFile(`${output}/measurements.json`,JSON.stringify(report,null,2)+'\n');
  const wait=(expression,ms=15000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${ms};function check(){try{if(${expression})return resolve(true);}catch{}if(performance.now()>end)return reject(Error(${JSON.stringify(expression)}));requestAnimationFrame(check);}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)});null`);await settle();};
  const view=()=>evaluate('JSON.parse(layerApp.app.workspace_view())');
  const idle=()=>wait('(()=>{const v=JSON.parse(layerApp.app.workspace_view());return v.ready&&!v.busy&&!v.switcher_busy&&!v.dirty;})()');
  const workspaceInput=async input=>{await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(input))});null`);await idle();await settle();};
  const rootOpen='layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page==null&&!!document.querySelector("#phone-menu-switcher-row")?.getClientRects().length';
  const pointer=async(point)=>{
    if(device==='touch'){
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...point}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    } else for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,pointerType:'mouse'});
    await settle();
  };
  const click=async selector=>{
    await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n||n.disabled||!n.getClientRects().length)throw Error('Unavailable '+${JSON.stringify(selector)});n.scrollIntoView({block:'nearest',inline:'nearest'});})()`);await settle();
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2,hit=document.elementFromPoint(x,y);if(!n.contains(hit)&&!(n.matches('.submenu-arrow,.menu-switcher-lead')&&hit===n.parentElement))throw Error('Obscured '+${JSON.stringify(selector)}+' by '+hit?.outerHTML.slice(0,120));return{x,y};})()`);
    await pointer(point);
  };
  const key=async key=>{
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code:key,windowsVirtualKeyCode:{ArrowDown:40,Home:36,Escape:27}[key]});await settle();
  };
  const closeMenu=async()=>{if(await evaluate('layerApp.state().phone.menu')){await click('#zen-capy');await wait('!layerApp.state().phone.menu');}};
  const openRoot=async()=>{
    if(await evaluate('layerApp.state().phone.menu')&&!await evaluate(rootOpen))await closeMenu();
    if(!await evaluate('layerApp.state().phone.menu'))await click('#zen-capy');await wait(rootOpen);await settle();
  };
  const backToRoot=async()=>{
    for(let depth=0;depth<3&&!await evaluate(rootOpen);depth++)await click('.menu-page:not([hidden]):not([inert]) .submenu-back');
    await wait(rootOpen);
  };
  const shot=async name=>{
    if(device==='mouse'){
      const point=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone;return p.edge==='top'?{x:layerApp.state().phone.swapped?p.capy.x-4:p.capy.x+p.tile+4,y:p.capy.y+p.tile*.5}:{x:p.capy.x+p.tile*.5,y:p.capy.y+p.tile+4};})()`);
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,button:'none',buttons:0});await settle();
    }
    const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${output}/${name}.png`,Buffer.from(data,'base64'));
    const geometry=await evaluate(`(${menuGeometry.toString()})()`);
    assert.deepEqual(geometry.viewport,viewport);assert.equal(geometry.dpr,1);assert.equal(geometry.theme,theme);
    report.measurements[name]=geometry;await save();return geometry;
  };
  const stage=async(name,run)=>{
    try{await run();report.steps.push({name,status:'passed'});}
    catch(error){const failure={name,message:error.message,stack:error.stack};report.failures.push(failure);report.steps.push({name,status:'failed'});console.error(name,error.stack);try{await shot(`${name}-failure`);}catch{};}
    await save();
  };
  const choiceId=id=>evaluate(`(()=>{const i=layerApp.app.phone_menu().switcher.choices.findIndex(c=>c.action.action.action.command.id===${JSON.stringify(id)});if(i<0)throw Error('missing switcher choice');return '#phone-menu-switch-'+i;})()`);
  const activeTiles=()=>evaluate('layerApp.state().workspace.layout.phone.map(t=>t.control)');
  const sketch='builtin:workspace:painter',photo='builtin:workspace:photographer';
  await call('Emulation.setDeviceMetricsOverride',{width:viewport[0],height:viewport[1],deviceScaleFactor:1,mobile:false});
  await call('Emulation.setTouchEmulationEnabled',{enabled:device==='touch',maxTouchPoints:10});
  await send({type:'set_theme',theme});await wait('!!layerApp.app.layout(innerWidth,innerHeight).phone');
  await send({type:'workspace_manager',command:{type:'switch',id:sketch}});await idle();assert.equal((await view()).id,sketch);
  const startup=await evaluate('layerApp.app.document_tabs(0).tabs.map(t=>({id:String(t.id),modified:t.modified}))');
  assert.equal(startup.length,1);assert.equal(startup[0].modified,false);
  await send({type:'invoke',command:'new_document'});
  await evaluate(createDrawing({width:1200,height:1600,resolution:300,space:'Srgb',depth:'U8',paper:'white',relief:0,name:'Phone drawing'}));
  await wait('!layerApp.state().document_file.busy&&!layerApp.documents.busy()&&layerApp.app.brush_ready()');
  if(await evaluate('layerApp.app.document_tabs(0).tabs.length===2'))await evaluate(`layerApp.documents.close(BigInt(${JSON.stringify(startup[0].id)}))`);
  await wait('layerApp.app.document_tabs(0).tabs.length===1&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await send({type:'select_brush',id:2});await wait('layerApp.app.brush_ready()');
  await stage('root',async()=>{
    await openRoot();const g=await shot('root');
    assert.equal(await evaluate('document.querySelector(".menu-page:not([hidden])").firstElementChild.id'),'phone-menu-switcher-row');
    assert.deepEqual(g.choices.pills.map(p=>p.label),['Sketch','Paint','Photo']);
    assert.equal(g.choices.pills.find(p=>p.selected==='true')?.label,'Sketch');
    assert.equal(g.strip.find(item=>item.name==='swap').asset,'layer-swap-sides-symbolic');
    assert.equal(g.row_height,g.switcher_row_height,'Switcher and menu rows have equal measured heights');
    assert.equal(g.list_width,viewport[0]>viewport[1]?320:396,'The normal menu list width is unchanged');
  });
  await stage('keyboard',async()=>{
    await openRoot();const expected=await evaluate('[...document.querySelectorAll("#phone-menu-switcher button")].filter(b=>!b.disabled).map(b=>b.id).concat("phone-drawing")');
    await evaluate('document.querySelector("#phone-menu-switch-0").focus()');await key('Home');
    const actual=[await evaluate('document.activeElement.id')];for(let i=1;i<expected.length;i++){await key('ArrowDown');actual.push(await evaluate('document.activeElement.id'));}
    report.keyboard={expected,actual};await save();await shot('keyboard-drawing-focus');assert.deepEqual(actual,expected,'ArrowDown order traverses pills, options, then Drawings');
  });
  await stage('pill-switch',async()=>{
    await openRoot();const before=await activeTiles();await click(await choiceId(photo));await idle();
    assert.equal((await view()).id,photo,'Photo pill switches workspace');await shot('root-photo');assert.ok(await evaluate(rootOpen),'Pill switch keeps Capy menu open on root');
    const after=await activeTiles();assert.notDeepEqual(after,before,'Pill switch publishes a different toolbar');
    assert.deepEqual(after,[{kind:'panel',panel:'adjustments'},{kind:'command',command:'select'},{kind:'tool_slot',slot:'operation'},{kind:'command',command:'drawing_brush'},{kind:'tool_slot',slot:'photo_retouch'},{kind:'command',command:'eraser'},{kind:'panel',panel:'layers'},{kind:'color'}]);
    assert.equal(await evaluate('document.querySelector("#phone-menu-switcher [aria-pressed=true]").textContent'),'Photo');
    await click(await choiceId(sketch));await idle();assert.ok(await evaluate(rootOpen),'Return pill leaves the root menu open');assert.deepEqual(await activeTiles(),before);
  });
  if((await view()).id!==sketch){await closeMenu();await workspaceInput({type:'switch',id:sketch});}
  await stage('padding',async()=>{
    await openRoot();const before=(await view()).id;
    const point=await evaluate(`(()=>{const well=document.querySelector('#phone-menu-switcher'),r=well.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+1.5,n=document.elementFromPoint(x,y);if(!well.contains(n)||n.closest('button'))throw Error('Not a well-padding hit');return{x,y};})()`);
    await pointer(point);await idle();assert.ok(await evaluate(rootOpen),'Well padding press does nothing');assert.equal((await view()).id,before);
  });
  for(const [name,selector] of [['options','#phone-menu-switcher-options'],['caret','#phone-menu-switcher-row > .submenu-arrow'],['row-background','#phone-menu-switcher-row > .menu-switcher-lead']])await stage(name,async()=>{
    await openRoot();const before=await evaluate('document.querySelector("#phone-menu").getBoundingClientRect().toJSON()');
    await click(selector);await wait('layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page?.kind==="workspaces"');await shot(`workspaces-${name}`);
    const after=await evaluate('document.querySelector("#phone-menu").getBoundingClientRect().toJSON()');
    for(const axis of ['x','y','width','height'])assert.ok(Math.abs(after[axis]-before[axis])<=1,`${name}: Workspaces opens in the original menu shell (${axis})`);
    await backToRoot();
  });
  await stage('swap',async()=>{
    await openRoot();const before=await evaluate('layerApp.state().phone.swapped');await click('#phone-swap');await wait(`layerApp.state().phone.swapped===${!before}`);
    assert.ok(await evaluate(rootOpen));const capture=await shot('root-swapped');assert.equal(capture.strip.find(item=>item.name==='swap').asset,'layer-swap-sides-symbolic');
    await click('#phone-swap');await wait(`layerApp.state().phone.swapped===${before}`);
  });
  await stage('drawings',async()=>{
    await closeMenu();const first=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
    await send({type:'invoke',command:'add_layer'});await wait('layerApp.app.document_tabs(0).tabs[0].modified&&layerApp.app.document_park_ready()');
    await openRoot();await click('#phone-drawing');await wait('layerApp.state().phone.menu_route.page?.kind==="drawings"');
    await click('.menu-page:not([hidden]):not([inert]) [data-position="[1,0]"]');await wait('!layerApp.state().phone.menu&&!!document.querySelector("#new-drawing-dialog[open]")');
    await evaluate(createDrawing({width:128,height:96,resolution:300,space:'Srgb',depth:'U8',paper:'white',relief:0,name:'Menu drawing'}));
    await wait('layerApp.app.document_tabs(0).tabs.length===2&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
    const second=await evaluate('Number(layerApp.app.document_tabs(0).selected)');assert.notEqual(first,second);
    await openRoot();await shot('root-two-drawings');await click('#phone-drawing');await wait('layerApp.state().phone.menu_route.page?.kind==="drawings"');
    const firstCapture=await shot('drawings-two');assert.equal(firstCapture.drawings.length,2);assert.ok(firstCapture.back_arrow);
    assert.equal(firstCapture.drawings.filter(d=>d.checked==='true').length,1);assert.equal(firstCapture.drawings.filter(d=>d.dot).length,1);
    assert.ok(firstCapture.drawings.every(d=>d.gutter&&d.cross));
    await backToRoot();await click('#phone-drawing');await click(`#phone-drawing-${first}`);
    await wait(`!layerApp.state().phone.menu&&Number(layerApp.app.document_tabs(0).selected)===${first}&&!layerApp.documents.busy()`);
    await openRoot();await shot('root-unsaved-drawing');await click('#phone-drawing');await wait('layerApp.state().phone.menu_route.page?.kind==="drawings"');
    const current=await shot('drawings-current');assert.equal(current.drawings.find(d=>d.id===`phone-drawing-${first}`).checked,'true');
    await click(`#phone-drawing-${first} + .menu-close`);await wait('!layerApp.state().phone.menu&&!!document.querySelector("dialog[open]")');
    await shot('drawings-close-confirmation');
    const label=await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label');
    const target=await evaluate(`(()=>{const button=[...document.querySelectorAll('dialog[open] footer button')].find(n=>n.textContent===${JSON.stringify(label)});if(!button)throw Error('Missing discard confirmation');button.dataset.auditDiscard='true';return '[data-audit-discard]';})()`);
    await click(target);await wait(`layerApp.app.document_tabs(0).tabs.length===1&&Number(layerApp.app.document_tabs(0).selected)===${second}&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy`);
  });
  await stage('overflow',async()=>{
    await closeMenu();
    const created=[];for(let i=0;i<6;i++){
      await workspaceInput({type:'form',action:{type:'new'}});await workspaceInput({type:'submit',name:`Wide pinned workspace ${i+1}`});created.push((await view()).id);
    }
    await workspaceInput({type:'dismiss'});await wait('!document.querySelector("dialog[open]")');assert.ok(created.length>=5&&created.every(id=>id));
    const selected=created.at(-1);assert.equal((await view()).id,selected);await openRoot();const g=await shot('root-overflow-active');
    report.overflow={created,selected,before:g.choices};await save();
    const visible=choices=>{const p=choices.pills.find(p=>p.selected==='true');return p&&p.bounds.left>=choices.bounds.left-1&&p.bounds.right<=choices.bounds.right+1;};
    const visibleRect=(inner,outer)=>inner&&inner.width>0&&inner.height>0&&inner.left>=outer.left-1&&inner.right<=outer.right+1&&inner.top>=outer.top-1&&inner.bottom<=outer.bottom+1;
    const geometry=measurement=>{
      const {list,well,options,caret,well_to_caret_gap}=measurement;
      assert.ok(visibleRect(well,list)&&visibleRect(options,well)&&visibleRect(caret,list),'The well, options and caret remain fully visible');
      assert.equal(well_to_caret_gap,12,'The overflowing well leaves the 12 px caret gap');
      if(viewport[0]>viewport[1]){
        assert.ok(list.width<=480,`The landscape menu list is at most 480 px, measured ${list.width}`);
        assert.equal(list.width,480,'The overflowing landscape menu list reaches its 480 px cap');
      }else{
        assert.equal(list.width,396,'Portrait overflow preserves the prior menu width');
        assert.equal(well.width,306,'Portrait overflow preserves the prior well width');
      }
    };
    const fixedControls=measurement=>{
      for(const key of ['options','caret'])assert.deepEqual(measurement[key],g[key],`${key} stays fixed while pills scroll or the menu reopens`);
    };
    assert.equal(g.choices.pills.find(p=>p.selected==='true')?.label,'Wide pinned workspace 6');
    assert.ok(g.choices.scroll_width>g.choices.client_width,'Extra pinned workspaces overflow the choices viewport');
    await stage('overflow-geometry',async()=>geometry(g));
    await stage('overflow-active-reveal',async()=>{
      assert.ok(visible(g.choices),`Opening the menu reveals selected pill completely; scrollLeft=${g.choices.scroll_left}, scrollWidth=${g.choices.scroll_width}, visible width=${g.choices.client_width}`);
    });
    await stage('overflow-pointer-scroll',async()=>{
      const r=g.choices.bounds,backward=g.choices.scroll_left>r.width/2;
      if(device==='touch'){
        const x=r.left+r.width*(backward?.2:.8),y=r.top+r.height/2;await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:2,x,y}]});
        for(let i=1;i<=5;i++)await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:2,x:x+(backward?1:-1)*r.width*.6*i/5,y}]});
        await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
      }else await call('Input.dispatchMouseEvent',{type:'mouseWheel',x:r.left+r.width/2,y:r.top+r.height/2,deltaX:(backward?-1:1)*Math.max(120,r.width),deltaY:0});
      await evaluate(`new Promise((resolve,reject)=>{const n=document.querySelector('.phone-menu-switcher-choices'),end=performance.now()+5000;let previous=n.scrollLeft,quiet=performance.now();function check(){const now=performance.now();if(n.scrollLeft!==previous){previous=n.scrollLeft;quiet=now;}if(now-quiet>=200)return resolve(true);if(now>end)return reject(Error('Horizontal scroll has not settled'));requestAnimationFrame(check);}check();})`);
      await settle();const scrolled=await shot('root-overflow-scrolled');report.overflow.after=scrolled.choices;await save();
      assert.ok(await evaluate(rootOpen),'Scrolling never activates a workspace or opens Workspaces');assert.equal((await view()).id,selected);
      assert.ok(Math.abs(scrolled.choices.scroll_left-g.choices.scroll_left)>1,`${device} horizontally scrolls pinned workspaces (before ${g.choices.scroll_left}, after ${scrolled.choices.scroll_left})`);
      fixedControls(scrolled);
    });
    await stage('overflow-reopened-reveal',async()=>{
      await closeMenu();await openRoot();const reopened=await shot('root-overflow-reopened');assert.ok(visible(reopened.choices),`Selected workspace is revealed again after reopening; scrollLeft=${reopened.choices.scroll_left}`);
      fixedControls(reopened);
    });
  });
  await save();
  assert.deepEqual(report.failures,[],`Phone switcher journey failures in ${size}-${theme}-${device}`);
  console.log(`PASS: ${size} ${theme} ${device}, phone switcher, keyboard order, drawings, overflow and logical geometry`);
}
