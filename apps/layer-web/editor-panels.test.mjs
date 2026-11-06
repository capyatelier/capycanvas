import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import test from 'node:test';
import {colorDialogDom} from './color-dialog-fixture.mjs';
import {createPageKit} from './page-kit.js';
import {bindCopy} from './localization.js';

const source=readFileSync(new URL('editor-panels.js',import.meta.url),'utf8');

test('tool category measurements reserve every fractional pixel before shared packing',()=>{
  const widths=[61.234375,78.359375,78.515625,70.28125,113.25,93.625,96.8125];
  const children=widths.map(width=>({style:{},offsetWidth:Math.round(width),getBoundingClientRect:()=>({width})}));
  const groups={children,style:{}},requests=[];
  const context={root:{clientWidth:380,querySelector:()=>groups},view:{groups_are_rows:false},dirty:true,layoutWidth:0,rows:[],
    app:{toolbar_ui(request){requests.push(request);return [121,138,117];}}};
  const begin=source.indexOf('    function arrange(){'),end=source.indexOf('    return () => {',begin);
  runInNewContext(source.slice(begin,end)+'\nglobalThis.arrange=arrange;',context);
  context.arrange();
  const natural=Array.from(requests[0].natural);
  assert.deepEqual(natural,[62,79,79,71,114,94,97]);
  assert.ok(natural.every((width,index)=>Number.isInteger(width)&&width>=widths[index]&&width-widths[index]<1));
  assert.ok(widths.some((width,index)=>children[index].offsetWidth<width));
  assert.ok(children.every(node=>node.style.width===''&&node.style.maxWidth===''));
  context.arrange();assert.equal(requests.length,1);
});

test('New Drawing acknowledges creation only when the shared dialog closes',()=>{
  const source=readFileSync(new URL('new-drawing.js',import.meta.url),'utf8');
  let drawing={},accepted,create=false;
  const context={state:()=>({new_drawing:drawing}),app:{dispatch(){if(create)drawing=null;return{};}},applyChange(){context.refresh();}};
  runInNewContext(source.slice(source.indexOf('  let ui = null, finish = null, creating = false;'),source.indexOf('  function build() {'))+
    '\nglobalThis.create=create;globalThis.refresh=refresh;globalThis.open=(dialog,done)=>{ui=dialog;finish=done;};',context);
  const dialog={commit:()=>true,report(){},render(){},close(){}};
  context.open(dialog,value=>accepted=value);context.create();
  assert.equal(accepted,undefined);
  drawing=null;context.refresh();assert.equal(accepted,false,'Cancel after a refused Create must not acknowledge a new drawing');
  drawing={};create=true;accepted=undefined;
  context.open(dialog,value=>accepted=value);context.create();
  assert.equal(accepted,true,'A successful Create acknowledges the synchronous close');
});

test('Palette sizing consumes shared rows before measuring and keeps history controls',()=>{
  const source=readFileSync(new URL('palettes.js',import.meta.url),'utf8');
  const nodes=Array.from({length:12},()=>({hidden:false,style:{}})),properties={},requests=[];
  const context={body:{clientWidth:226},root:{style:{setProperty:(name,value)=>properties[name]=String(value)}},history:{children:nodes},expanded:{hidden:true,clientHeight:0},columns:0,pitch:0,gap:0,gridWidth:0,changes:0,
    contentChanged(){context.changes++;},app:{color_ui(request){requests.push(request);return request.width===226?{columns:6n,pitch:38n,gap:4n}:{columns:8n,pitch:44n,gap:4n};}}};
  runInNewContext(source.slice(source.indexOf('    function layoutGrids()'),source.indexOf('    const resize =',source.indexOf('    function layoutGrids()')))+'\nglobalThis.arrange=layoutGrids;',context);
  context.arrange();assert.equal(properties['--palette-tile'],'34px');assert.equal(properties['--palette-columns'],'6');
  assert.deepEqual(nodes.slice(0,-1).map(node=>node.hidden),[false,false,false,false,false,true,true,true,true,true,true]);
  context.arrange();assert.equal(requests.length,1);assert.equal(context.changes,1);
  context.body.clientWidth=380;context.arrange();
  assert.equal(properties['--palette-tile'],'40px');assert.equal(properties['--palette-columns'],'8');assert.equal(requests.length,2);
  assert.deepEqual(nodes.slice(0,-1).map(node=>node.hidden),[false,false,false,false,false,false,false,true,true,true,true]);
});

test('Colour applies the shared fit and only queries changed allocations',()=>{
  const node=()=>({style:{}}),requests=[],frame={...node(),clientWidth:280,clientHeight:184},root=node(),stage=node(),wheel=node(),swap=node(),readout=node(),edit=node();
  const choices=['foreground','background','transparent'].map(slot=>({slot,node:node()})),shapes=[node(),node()];
  const fit={size:159,height:184,natural_height:300,wheel:[2,3,155,155],foreground:[4,5,32,32],background:[20,21,24,24],transparent:[40,50,24,24],shapes:[[8,9,20,20],[10,11,20,20]],swap:[12,13,20,20],readout:[14,15,40,20],edit:[16,17,20,20]};
  const context={frame,root,stage,wheel,swap,readout,edit,choices,shapes,fitted:null,view:{hdr:true},frameWidth:0,frameHeight:0,layoutWidth:0,layout:undefined,naturalAspect:'',naturalLayout:undefined,contentChanged(){},app:{color_ui(request){requests.push({...request});return request.type==='fit_layout'?{...fit}:{...fit,size:request.size,height:request.size===128?150:300};}}};
  const begin=source.indexOf('    const place=',source.indexOf('  function colorWheel(')),end=source.indexOf('      const width=layoutWidth;',begin);
  runInNewContext(source.slice(begin,end)+'\n}\nglobalThis.draw=draw;',context);
  context.draw();
  assert.equal(stage.style.width,'159px');assert.equal(stage.style.height,'184px');assert.equal(frame.style.aspectRatio,'280 / 300');
  assert.deepEqual(requests,[{type:'layout',size:128,hdr:true},{type:'layout',size:280,hdr:true},{type:'fit_layout',width:280,height:184,hdr:true}]);
  for(const [control,rect]of [[wheel,fit.wheel],...choices.map(({slot,node})=>[node,fit[slot]]),...shapes.map((node,index)=>[node,fit.shapes[index]]),[swap,fit.swap],[readout,fit.readout],[edit,fit.edit]])assert.deepEqual(control.style,Object.fromEntries(['left','top','width','height'].map((key,index)=>[key,`${rect[index]}px`])));
  context.draw();assert.equal(requests.length,3);
  frame.clientHeight=185;context.draw();
  assert.deepEqual(requests.at(-1),{type:'fit_layout',width:280,height:185,hdr:true});assert.equal(requests.length,4);
  context.fitted={...fit};context.frameWidth=0;context.draw();
  assert.equal(frame.style.aspectRatio,'auto');assert.equal(stage.style.height,'184px');
  assert.deepEqual(requests.slice(4),[{type:'layout',size:280,hdr:true}]);
  context.draw();assert.equal(requests.length,5);
});

function harness() {
  const navigators=new Map(),pending=new Map(),sizes=[],reflows=[];
  let next=0,wakes=0;
  const context={navigators,positioning:0,devicePixelRatio:2,
    requestAnimationFrame:fn=>{pending.set(++next,fn);return next;},
    cancelAnimationFrame:id=>pending.delete(id),wake:()=>wakes++,
    ResizeObserver:class {constructor(callback){this.callback=callback;}},
    app:{
      navigator_size(id,width,height,scale){sizes.push([id,width,height,scale]);return [512,256];},
      navigator_geometry(width,height){return {image:{x:1,y:2,width:width-2,height:height-4}};},
      reflow_navigators(){reflows.push(sizes.map(size=>[...size]));return true;},
    },
  };
  runInNewContext(source.slice(source.indexOf('  function measurePositions('),source.indexOf('  window.addEventListener("resize",queuePositions);'))+
    '\nglobalThis.deliver=entries=>navigatorSizes.callback(entries);',context);
  function add(id) {
    const overview={getBoundingClientRect:()=>assert.fail('Position updates must use observer geometry')};
    const record={id,overview,surface:{style:{}},hole:{style:{}},size:''};
    navigators.set(overview,record);return record;
  }
  return {context,navigators,sizes,reflows,pending,add,wakes:()=>wakes,
    deliver:(...records)=>context.deliver(records.map(([record,width,height])=>({target:record.overview,contentRect:{width,height}}))),
    flush(){const callbacks=[...pending.values()];pending.clear();callbacks.forEach(fn=>fn());},
  };
}

test('Navigator observers publish all current extents before one GPU reflow',()=>{
  const h=harness(),a=h.add(1),b=h.add(2);
  h.context.queuePositions();h.context.queuePositions();
  assert.equal(h.pending.size,1);
  h.deliver([a,220,164],[b,140,100]);
  assert.equal(h.pending.size,0);
  assert.deepEqual(h.sizes,[[1,220,164,2],[2,140,100,2]]);
  assert.equal(h.reflows.length,1);assert.equal(h.reflows[0].length,2);
  assert.equal(a.surface.style.width,'256px');assert.equal(a.surface.style.height,'128px');
  assert.equal(a.hole.style.width,'218px');assert.equal(b.hole.style.height,'96px');
  assert.equal(h.wakes(),1);
});

test('Queued camera updates retain the latest observed size and follow density changes',()=>{
  const h=harness(),a=h.add(1);
  h.deliver([a,220,164]);
  h.context.queuePositions();h.deliver([a,220,90]);h.flush();
  h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes,[[1,220,164,2],[1,220,90,2]]);
  assert.equal(h.reflows.length,2);
  assert.equal(a.hole.style.height,'86px');
  h.context.devicePixelRatio=3;h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes.at(-1),[1,220,90,3]);
  assert.equal(h.reflows.length,3);
});

test('Unmeasured and retired Navigator controls cannot publish stale extents',()=>{
  const h=harness(),a=h.add(1),b=h.add(2);
  h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes,[]);
  h.navigators.delete(a.overview);
  h.deliver([a,220,164],[b,140,100]);
  assert.deepEqual(h.sizes,[[2,140,100,2]]);
  assert.equal(a.hole.style.width,undefined);
});

function shortcutDom(t) {
  const dom=colorDialogDom(t),kit=createPageKit(dom),actions=[];
  const sheet=()=>{const content=kit.body();t.after(()=>content.dispose());return {title:dom.element('h2'),body:content.content,content};};
  const context={...dom,bindCopy,kit,group:kit.group,editor:sheet(),modifierSheet:sheet(),details:sheet(),
    common:{cancel:'Cancel'},copy:{add:'Add',reassign:'Reassign',open:'Open',press_hold_key:'Hold a key',no_differences:'No differences',defaults_help:'Defaults'},
    app:{native_caption:()=>''},send:action=>actions.push(action),present(){},focusRecording(){},
    keymapChoice:{update(){}},keymapOutdated:{},importTitle:dom.element('h2'),importBody:dom.element('div'),importSignature:'',importHost:{present(){},hide(){}}};
  const detailsIntro=kit.fields();context.details.content.root.prepend(detailsIntro);context.detailsIntro=detailsIntro;
  const code=readFileSync(new URL('shortcut-page.js',import.meta.url),'utf8');
  for(const [start,end] of [['  const flatButton =','  const navRow ='],['  function recordingRow(', '  function sheet('],['  function refreshEditor(', '  function focusRecording('],['  function refreshModifierSheet(', '  function refreshPicker(']])
    runInNewContext(code.slice(code.indexOf(start),code.indexOf(end)),context);
  const start=code.indexOf('  function refreshKeymap('),end=code.indexOf('  function refreshFilters(',start);
  runInNewContext(code.slice(start,end),context);
  return {...context,context,actions};
}

test('Preferences Color exposes a labelled library card and its real manage action',t=>{
  const h=colorDialogDom(t),kit=createPageKit(h),container=h.element('div'),requests=[];
  const code=readFileSync(new URL('preferences.js',import.meta.url),'utf8');
  const start=code.indexOf('      if(page.id==="color"){'),end=code.indexOf('\n    }\n  }\n  return function refresh',start);
  const profileCopy={manage:'Manage',library_title:'Color profiles',menu_help:'Installed profiles'};
  const app={};
  runInNewContext(code.slice(start,end),{...h,page:{id:'color'},container,profileCopy,app,dropdown(){},settingsGroup:kit.group,preferenceRow:kit.row,chooseProfileLibrary:request=>requests.push(request)});
  const section=container.children[0],row=section.children[0].children[0],manage=row.children[1],label=row.children[0].children[0];
  assert.equal(label.textContent,'Color profiles');assert.equal(label.htmlFor,manage.id);
  assert.equal(row.children[0].children[1].textContent,'Installed profiles');
  assert.equal(manage.textContent,'Manage');assert.equal(manage.id,'color-profile-library');
  let forwarded=true;manage.click();row.dispatchEvent({type:'click',target:label,preventDefault(){forwarded=false;}});
  assert.equal(forwarded,false,'The row consumes the label default instead of opening the library twice');
  assert.equal(requests.length,2);assert.ok(requests.every(request=>request.manage&&request.app===app));
});

test('Shortcut capture warning and modifier heading consume their projected icon and label',t=>{
  const h=shortcutDom(t),capture={id:'modifier',label:'Record Alt',shortcut:'Alt',notice:'Already assigned',conflict:true,chord:{},error:null};
  h.context.refreshModifierSheet({capture,capture_icon:'warning',dialog:{}});
  assert.equal(h.modifierSheet.title.textContent,'Record Alt');
  const row=h.modifierSheet.body.children[1].children[0].children[0],glyph=row.children[0];
  assert.equal(glyph.dataset.asset,'warning');assert.equal(glyph.classList.contains('warning'),true);
  assert.equal(row.children[1].children[1].textContent,'Already assigned');
  row.children[3].click();assert.deepEqual(JSON.parse(JSON.stringify(h.actions)),[{type:'confirm_shortcut',replace:true}]);
  capture.label='Record Shift';capture.shortcut='Shift';
  h.context.refreshModifierSheet({capture,capture_icon:'keyboard',dialog:{}});
  assert.equal(h.modifierSheet.title.textContent,'Record Shift');
  assert.equal(h.modifierSheet.body.children[1].children[0].children[0].children[0].dataset.asset,'keyboard');
});

test('Shortcut editor omits empty description and error without losing its binding card',t=>{
  const h=shortcutDom(t),spec={id:'undo',label:'Undo',description:'',defaults:[],overlaps:[],bindings:['Ctrl+Z'],gestures:[],modified:false,can_add:false,dialog:{}};
  h.context.refreshEditor({shortcut_editor:spec,capture:null,error:null});
  assert.deepEqual(h.editor.body.children.slice(0,2).map(node=>node.hidden),[true,true]);
  assert.equal(h.editor.body.children[2].children[0].children[0].children[0].textContent,'Ctrl+Z');
  spec.description='Undo the last stroke';
  h.context.refreshEditor({shortcut_editor:spec,capture:null,error:'Conflict'});
  assert.deepEqual(h.editor.body.children.slice(0,2).map(node=>node.hidden),[false,false]);
  assert.equal(h.editor.body.children[1].textContent,'Conflict');
});

test('Keymap details fixes its intro above the scrolling difference card in one outer body',t=>{
  const h=shortcutDom(t),keymap={title:'Default keys',selected:'default',presets:[{id:'default',title:'Default'}],outdated:false,source:'Built in',links:['https://example.test/key_map.html'],differences:[{trigger:'Ctrl+Z',note:'Undo'}],details:true,dialog:{}};
  h.context.refreshKeymap({keymap});
  const [source,links]=h.detailsIntro.children,card=h.details.body.children[0],outer=h.details.content.root,viewport=h.details.body.parentNode;
  assert.equal(outer.className,'preference-body');assert.equal(viewport.className,'preference-viewport');
  assert.equal(h.detailsIntro.parentNode,outer);assert.equal(outer.children[0],h.detailsIntro);
  assert.equal(viewport.parentNode,outer);assert.equal(viewport.contains(source),false);assert.equal(viewport.contains(links),false);
  assert.equal(source.textContent,'Built in');assert.equal(source.parentNode,h.detailsIntro);
  assert.equal(links.children[0].href,keymap.links[0]);assert.equal(links.parentNode,h.detailsIntro);
  assert.equal(h.details.body.children.length,1);assert.equal(card.parentNode,h.details.body);assert.equal(card.children[0].children[0].textContent,'Ctrl+ZUndo');
  assert.equal(outer.descendants().filter(node=>node.className==='preference-body').length,0);
  keymap.source='';keymap.links=[];h.context.refreshKeymap({keymap});
  assert.equal(h.detailsIntro.children[0].hidden,true);assert.equal(h.detailsIntro.children[1].hidden,true);
});

test('Keymap import presents the shared preview text without rebuilding its entries',t=>{
  const h=shortcutDom(t),preview={heading:'Import keys',body:'Added: 8\n• First\n• Last shown',added:Array.from({length:8},(_,i)=>`Entry ${i}`),changed:[],removed:[],unavailable:[]};
  h.copy.added='Added';
  const keymap={title:'Keys',selected:'default',presets:[],outdated:false,source:'',links:[],differences:[],import:preview,import_dialog:{}};
  h.context.refreshKeymap({keymap});
  assert.equal(h.importTitle.textContent,preview.heading);
  assert.equal(h.importBody.textContent,preview.body);
  assert.equal(h.importBody.children.length,1);
  preview.body='No changes';h.context.refreshKeymap({keymap});assert.equal(h.importBody.textContent,preview.body);
});
