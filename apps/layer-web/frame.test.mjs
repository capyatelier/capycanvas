import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";
import {headerButtonHint, presentCapy} from './header.js';
import {colorDialogDom} from './color-dialog-fixture.mjs';
import {latinKey} from './text-input.js';
import {bindCopy,refreshBindings} from './localization.js';

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");
test('glass CSS preserves the shared straight RGBA channels',()=>{
  const styles=new Map(),context={appliedTheme:null,glass:null,document:{
    body:{dataset:{},style:{setProperty:(key,value)=>styles.set(key,value)}},
    documentElement:{style:{}},querySelector:()=>({})}};
  runInNewContext(source.slice(source.indexOf('function applyTheme('),source.indexOf('function element(')),context);
  const zero=[0,0,0,0],settings={card_shade:zero,card_ring:zero,card_shadow:zero,card_far_shadow:zero};
  for(const color of [[0.9426783323287964,0.9426783323287964,0.9426783323287964,0.9399999976158142],
    [0.2571895718574524,0.2571895718574524,0.2571895718574524,0.9599999785423279],
    [0.2,0.4,0.6,0.001]]){
    context.applyTheme('light',{link_oklab:[.5,.1,.2],glass:{panel:color},control_fill:zero,controls:{fill:color},dialogs:{scrim:color,press_shade:zero,destructive_oklab:[.5,.1,.2]},lines:{settings,panel:{border:zero},sidebar_border:zero,popover_border:color}});
    const values=styles.get('--glass-panel').match(/[-+]?(?:\d*\.)?\d+(?:e[-+]?\d+)?/gi).map(Number);
    const actual=[...values.slice(0,3).map(value=>value/255),values[3]];
    actual.forEach((value,index)=>assert.ok(Math.abs(value-color[index])<1e-7,`${index}: ${value} retains ${color[index]}`));
    assert.equal(styles.get('--widget-fill'),styles.get('--glass-panel'));
    assert.equal(styles.get('--popover-border'),styles.get('--glass-panel'));
    assert.equal(styles.get('--link-color'),'oklab(0.5 0.1 0.2)');
    assert.equal(styles.get('--dialog-scrim'),styles.get('--glass-panel'));assert.equal(styles.get('--dialog-destructive'),'oklab(0.5 0.1 0.2)');
  }
});

test('shared symbolic icon names resolve to the existing Web asset bank',()=>{
  const glyph={cloneNode:()=>glyph},context={icons:new Map([['settings',glyph]])};
  runInNewContext(source.slice(source.indexOf('function icon(name,'),source.indexOf('function iconButton(')),context);
  assert.equal(context.icon('layer-settings-symbolic'),glyph);
  assert.equal(context.icon('settings'),glyph);
});
test('document transitions retain canvas bar measurement and reveal reports',()=>{
  const actions=[],context={documents:{interactionBusy:()=>true},workspaceGesture:null,
    app:{allowed_during_document_interaction:action=>action.type!=='invoke',dispatch(action){actions.push(action);return{};}},applyChange(){},message:assert.fail};
  runInNewContext(source.slice(source.indexOf('function dispatch(action)'),source.indexOf('let languageTimer')),context);
  for(const type of ['invoke','measure_canvas_bar','reveal_canvas_bar'])context.dispatch({type});
  assert.deepEqual(actions.map(action=>action.type),['measure_canvas_bar','reveal_canvas_bar']);
});

test('phone inset reports apply while busy and deduplicate against shared state',()=>{
  const phoneSource=readFileSync(new URL('phone.js',import.meta.url),'utf8');
  const state={workspace_ready:true,phone:{insets:[0,0,0,0]}};
  let busy=true,observer,refuse=false;
  const attempts=[],delivered=[];
  const context={documents:{interactionBusy:()=>busy},workspaceGesture:null,window:{addEventListener(){}},
    app:{allowed_during_document_interaction:action=>action.type==='phone'&&action.action.kind==='metrics',
      dispatch(action){if(!refuse){delivered.push(action);state.phone.insets=[...action.action.insets];}return{};}},
    applyChange(){},message:assert.fail,
    ResizeObserver:class{constructor(callback){this.callback=callback;this.targets=[];observer=this;}observe(target){this.targets.push(target);}}};
  runInNewContext(source.slice(source.indexOf('function dispatch(action)'),source.indexOf('let languageTimer')),context);
  runInNewContext(phoneSource.replace(/^import .*;\n/gm,'').replace('export function createPhone','function createPhone'),context);
  const element=()=>({style:{setProperty(){}},children:[],append(...nodes){this.children.push(...nodes);},addEventListener(){}});
  const presenter=context.createPhone({app:context.app,state:()=>state,workspace:element(),capy:element(),header:{},element,button:element,
    customization:{target(){}},dispatch:action=>{attempts.push(action);context.dispatch(action);}});
  const measure=insets=>observer.callback(observer.targets.map((target,index)=>({target,contentRect:{width:insets[index]}})));
  measure([19,7,13,5]);
  assert.deepEqual([...state.phone.insets],[19,7,13,5]);
  assert.equal(delivered.length,1);assert.equal(attempts.length,1);
  busy=false;context.app.allowed_during_document_interaction=assert.fail;presenter.refresh();
  assert.equal(delivered.length,1);assert.equal(attempts.length,1,'Busy completion does not redispatch applied facts');
  refuse=true;measure([20,7,13,5]);
  assert.deepEqual([...state.phone.insets],[19,7,13,5]);
  refuse=false;presenter.refresh();
  assert.deepEqual([...state.phone.insets],[20,7,13,5]);
  assert.equal(delivered.length,2);assert.equal(attempts.length,3,'Refused facts heal on the next refresh');
  state.phone.insets=[0,0,0,0];presenter.refresh();
  assert.deepEqual([...state.phone.insets],[20,7,13,5],'A stale shared publication heals without a new native measurement');
  presenter.refresh();assert.equal(attempts.length,4);
  const physical={left:3,top:17,right:11,bottom:29};
  observer.callback(observer.targets.map(target=>({target,contentRect:{width:physical[target.style.width.match(/inset-(\w+)/)[1]]}})));
  assert.deepEqual([...state.phone.insets],[3,17,11,29],'CSS side facts use the shared left/top/right/bottom order');
});

test('retained workspace placement keeps bubble presentation without scheduling GPU work',()=>{
  let refreshes=0,wakes=0,model=1;
  const context={app:{language_pending:()=>false,workspace_update:()=>({model_revision:model,content_revision:1}),state_update:assert.fail},
    bubbles:{refresh(){refreshes++;wakes++;}},canvasBar:null,workspaceManager:{observe(){}},editor:{refreshColorPreview(){}},
    fillGesture:null,phone:null,workspaceModelRevision:1,workspaceContentRevision:1,workspaceGesture:{started:true},
    queueWorkspacePresentation(){},queueWorkspaceLayout(){},refreshColorEditors(){},refreshTextureDialogs(){},wake(){wakes++;}};
  runInNewContext(source.slice(source.indexOf('function applyChange(change)'),source.indexOf('let canvasCursorActive')),context);
  for(const regions of [129,129|32768])context.applyChange({regions,canvas_wake:false});
  assert.deepEqual([refreshes,wakes],[0,0]);
  model=2;context.applyChange({regions:129,canvas_wake:false});
  assert.deepEqual([refreshes,wakes],[1,1],'A new shared model still refreshes bubbles');
  model=1;
  for(const regions of [2,4,8,16,256,8192])context.applyChange({regions,canvas_wake:false});
  assert.deepEqual([refreshes,wakes],[7,7],'Brush, document, command, settings, preview and bubble changes remain live');
});

test('fill gesture publishes through the color preview fast path without a workspace patch',()=>{
  const received=[],gesture={transaction:7,pill:{label:'Shared label',value:'Shared value'}};
  const context={app:{language_pending:()=>false,color_preview:()=>({fill_gesture:gesture}),state_update:assert.fail},
    fillGesture:{refresh:value=>received.push(value)},phone:null,editor:null,refreshColorEditors(){},bubbles:null,wake(){}};
  runInNewContext(source.slice(source.indexOf('function applyChange(change)'),source.indexOf('let canvasCursorActive')),context);
  context.applyChange({regions:256,canvas_wake:false});
  assert.equal(received[0],gesture);
  context.app.color_preview=()=>({fill_gesture:null});context.applyChange({regions:256,canvas_wake:false});
  assert.equal(received[1],null);
});

test('header buttons present shared refusal hints without replacing layout-editing hints', () => {
  const spec={label:'Shared label',enabled:false,disabled_reason:'Shared refusal'},command={tooltip:'Command hint'};
  assert.equal(headerButtonHint(spec,command,false),'Shared refusal');
  assert.equal(headerButtonHint(spec,command,true),'Command hint');
  assert.equal(headerButtonHint({...spec,enabled:true},command,false),'Shared refusal');
  assert.equal(headerButtonHint({...spec,disabled_reason:null},command,false),'Command hint');
  assert.equal(headerButtonHint({...spec,disabled_reason:null},undefined,false),'Shared label');
});

test('compiler input hooks wait for app publication and retain contact and resume timing', () => {
  const listeners=new Map(),timers=[];
  let inputs=0,wait=3.2,wakes=0;
  const events=owner=>({addEventListener(type,listener){listeners.set(`${owner}:${type}`,listener);}});
  const context={window:events('window'),document:{...events('document'),hidden:false,querySelector:()=>null},
    app:undefined,gpuReady:false,compilerResumeTimer:undefined,pending:[],documents:{busy:()=>false},hasModalDialog:()=>false,
    setTimeout(callback,delay){timers.push({callback,delay});return timers.length;},wake(){wakes++;}};
  const contacts=source.slice(source.indexOf('const compilerContacts = new Set();'),source.indexOf('for (const type of ["pointerdown", "pointermove"])'));
  const compiler=source.slice(source.indexOf('function deferOptionalCompiler()'),source.indexOf('function scheduleCompiler()'));
  runInNewContext(contacts+compiler+'\nglobalThis.contactCount=()=>compilerContacts.size;',context);
  const emit=(owner,type)=>listeners.get(`${owner}:${type}`)({isTrusted:true,pointerId:42});
  for(const type of ['pointerdown','pointermove','pointerup','pointercancel','keydown','wheel','blur'])emit('window',type);
  for(const type of ['visibilitychange','toggle','close'])emit('document',type);
  assert.equal(timers.length,0,'Pre-publication input does not schedule a compiler timer');
  emit('window','pointerdown');assert.equal(context.contactCount(),1);
  context.app={shader_input(){inputs++;},shader_wait_ms(){return wait;}};
  assert.equal(context.optionalCompilerReady(),false,'A contact begun before publication remains held');
  emit('window','pointerup');emit('window','pointermove');
  assert.equal(inputs,2,'A published app receives input even before GPU attachment');
  assert.equal(context.contactCount(),0);
  assert.equal(timers.length,1,'Repeated input retains one resume timer');
  assert.equal(timers[0].delay,4);
  wait=2.1;timers[0].callback();assert.equal(timers[1].delay,3);assert.equal(wakes,0);
  wait=0;timers[1].callback();assert.equal(context.compilerResumeTimer,null);assert.equal(wakes,1);
});

test('live preferences publish while document dialogs retain exclusive editing input', () => {
  let busy=true;
  const received=[],published=[],change={regions:128};
  const context={documents:{interactionBusy:()=>busy},app:{allowed_during_document_interaction:action=>action.type!=='invoke',dispatch(action){received.push(action);return change;}},
    applyChange:value=>published.push(value),message:assert.fail};
  runInNewContext(source.slice(source.indexOf('function dispatch(action)'),source.indexOf('let languageTimer;')),context);
  const restore={type:'restore_saved_settings',saved:JSON.stringify({language:{Explicit:'ja'}})};
  context.dispatch(restore);
  assert.deepEqual(received,[restore]);assert.deepEqual(published,[change]);
  const motion={type:'system_motion_changed',reduced:true};context.dispatch(motion);
  const invoke={type:'invoke',command:'pen'};
  context.dispatch(invoke);
  assert.deepEqual(received,[restore,motion],'the open dialog keeps ownership of editing commands');
  busy=false;context.app.allowed_during_document_interaction=assert.fail;context.dispatch(invoke);
  assert.deepEqual(received,[restore,motion,invoke]);assert.deepEqual(published,[change,change,change]);
});

test('native key input retains dialog ownership, text and Latin candidates', () => {
  const received = [];
  class Element { closest() { return null; } }
  let composing=false,nativePaste=false,modal=true;
  const context = {Element,
    composingKey:() => composing, hasModalDialog:() => modal, latinKey, updateZen() {}, keyboardFocused:new WeakSet(),
    app:{control_takes_key:() => false}, documents:{allowNativePaste:() => nativePaste},
    input:event => { received.push(event); return {handled:true}; }};
  runInNewContext(source.slice(source.indexOf('function keyInput('), source.indexOf('const keyboardFocused =')), context);
  const key = (pressed, properties = {}, fromDialog = false) => {
    const event = {target:new Element(), key:'Escape', defaultPrevented:false, stopped:false,
      ...properties,
      preventDefault() { this.defaultPrevented=true; }, stopPropagation() { this.stopped=true; }};
    context.keyInput(event, pressed, null, fromDialog);
    return event;
  };
  assert.equal(key(true).defaultPrevented, false);
  assert.equal(received.length, 0, 'A blurred global press cannot bypass the modal dialog');
  assert.equal(key(true, {}, true).defaultPrevented, true);
  assert.equal(received.length, 1, 'The common host forwards its owned press exactly once');
  key(false);
  assert.equal(received.at(-1).pressed, false, 'Release retires the shared recording key');
  modal=false;
  composing=true;assert.equal(key(true).defaultPrevented,false);assert.equal(received.at(-1).editing,true);
  composing=false;nativePaste=true;assert.equal(key(true,{key:'v',code:'KeyV',ctrlKey:true}).defaultPrevented,false);
  nativePaste=false;
  for (const [name, code, shiftKey, latin] of [['я','KeyZ',false,'z'], ['Я','KeyZ',true,'Z'],
    ['и','KeyB',false,'b'], ['é','Digit2',false,'2'], ['х','BracketLeft',true,'{'],
    ['a','KeyQ',false,'q'], ['z','KeyZ',false,null], ['Escape','Escape',false,null]]) {
    for (const pressed of [true, false]) {
      key(pressed, {key:name, code, shiftKey});
      assert.equal(received.at(-1).key, name);
      assert.equal(received.at(-1).latin, latin, `${name}/${code}/shift=${shiftKey}/pressed=${pressed}`);
      assert.equal(received.at(-1).pressed, pressed);
    }
  }
});

test('canvas panel measurement excludes intentional header overflow and previous allocation',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),dialog=element('dialog'),header=element('header'),cancel=element('button'),primary=element('button');
  header.append(cancel,primary);dialog.append(header);doc.body.append(dialog);
  dialog.scrollWidth=361;dialog.scrollHeight=212;dialog.style.width='500px';dialog.style.height='212px';
  dialog.getBoundingClientRect=()=>({width:360,height:123});
  const measurements=[];
  const app={dialog_presentation:()=>({kind:'canvas_panel'}),dialog_header:()=>({start:'cancel',end:'primary'}),
    dialog_bounds:(_role,_viewport,_visible,measured)=>{assert.equal(dialog.style.width,'');assert.equal(dialog.style.height,'');measurements.push(measured);return{x:26,y:704,width:measured[0],height:measured[1]};}};
  const host=createDialogHost({app,dialog,header,cancel,primary,icon,copy:{cancel:'Cancel'},onCancel(){}});
  host.present({frame:{kind:'editor'},role:'canvas_panel',modal:true,primary:'action'});host.layout();
  assert.deepEqual(measurements,[[360,123],[360,123]],'Decoration overflow and the previous fixed height never become natural content');
  assert.equal(dialog.style.width,'360px');assert.equal(dialog.style.height,'123px');
  host.hide();host.dispose();
});

test('dialog headers retain their supplied native skin when navigation changes the active header',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t);
  for(const headerStyle of ['flat','raised']){
    const dialog=element('dialog'),first=element('header'),second=element('header'),cancel=element('button');
    first.append(cancel);dialog.append(first,second);doc.body.append(dialog);let active=first;
    const app={dialog_presentation:()=>({kind:'page',bounds:{x:0,y:0,width:412,height:891}}),dialog_header:()=>({start:'cancel',end:null})};
    const host=createDialogHost({app,dialog,header:()=>active,headerStyle,cancel,icon,copy:{cancel:'Cancel'},onCancel(){}});
    host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:null});
    assert.equal(first.dataset.headerStyle,headerStyle);
    active=second;host.layout();
    assert.equal(first.dataset.headerStyle,undefined);assert.equal(second.dataset.headerStyle,headerStyle);
    host.hide();host.dispose();dialog.remove();
  }
});

test('floating editor measures its natural border box and releases a stale width when the window grows',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),dialog=element('dialog'),header=element('header'),cancel=element('button');
  header.append(cancel);dialog.append(header);doc.body.append(dialog);
  const previous={innerWidth:globalThis.innerWidth,getComputedStyle:globalThis.getComputedStyle};
  globalThis.innerWidth=500;globalThis.getComputedStyle=()=>({boxSizing:'content-box',paddingLeft:'12px',paddingRight:'12px',borderLeftWidth:'1px',borderRightWidth:'1px'});
  t.after(()=>Object.assign(globalThis,previous));
  dialog.getBoundingClientRect=()=>({width:dialog.style.width?Number.parseFloat(dialog.style.width)+26:420,height:100});
  const measurements=[];
  const app={dialog_presentation:()=>({kind:'floating'}),dialog_header:()=>({start:'cancel',end:null}),
    floating_dialog_width:(window,natural)=>{measurements.push([window,natural]);return Math.min(window-140,natural);}};
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:{cancel:'Cancel'},onCancel(){}});
  host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:null});
  assert.deepEqual(measurements,[[500,420]]);assert.equal(dialog.style.width,'334px','CSS content width excludes border and padding');
  globalThis.innerWidth=900;host.layout();
  assert.deepEqual(measurements,[[500,420],[900,420]],'The previous fixed width is cleared before measuring');
  assert.equal(dialog.style.width,'394px');host.hide();host.dispose();
});

test('common dialog host forwards owned keys once and registers actual modal state',async t=>{
  const {createDialogHost,hasOpenDialog,hasModalDialog}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),dialog=element('dialog'),header=element('header'),cancel=element('button');
  header.append(cancel);dialog.append(header);doc.body.append(dialog);dialog.show=()=>{dialog.open=true;};
  const forwarded=[];
  const app={dialog_presentation:()=>({kind:'floating'}),floating_dialog_width:(_window,natural)=>natural,dialog_header:()=>({start:'cancel',end:null}),dialog_escape:()=> 'cancel'};
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:{cancel:'Cancel'},onCancel(){},onKey:event=>forwarded.push(event.key)});
  const key=()=>{let stopped=0;dialog.dispatchEvent({type:'keydown',key:'a',stopPropagation(){stopped++;},preventDefault(){}});return stopped;};
  assert.equal(hasOpenDialog(),false);assert.equal(hasModalDialog(),false);
  host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:null});
  assert.equal(hasOpenDialog(),true);assert.equal(hasModalDialog(),true);assert.equal(key(),1);assert.deepEqual(forwarded,['a']);
  host.hide();assert.equal(hasOpenDialog(),false);assert.equal(hasModalDialog(),false);
  host.present({frame:{kind:'editor'},role:'editor',modal:false,primary:null});
  assert.equal(hasOpenDialog(),true);assert.equal(hasModalDialog(),false);assert.equal(key(),0);assert.deepEqual(forwarded,['a','a']);
  host.dispose();assert.equal(hasOpenDialog(),false);assert.equal(hasModalDialog(),false);
});

test('a consumed child Escape leaves its dialog open until the terminal Escape',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),dialog=element('dialog'),header=element('header'),cancel=element('button');
  header.append(cancel);dialog.append(header);doc.body.append(dialog);
  let cancels=0;
  const app={dialog_presentation:()=>({kind:'floating'}),floating_dialog_width:(_window,natural)=>natural,dialog_header:()=>({start:'cancel',end:null}),dialog_escape:()=> 'cancel'};
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:{cancel:'Cancel'},onCancel(){cancels++;host.hide();}});
  host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:null});
  const key=defaultPrevented=>dialog.dispatchEvent({type:'keydown',key:'Escape',defaultPrevented,preventDefault(){this.defaultPrevented=true;},stopPropagation(){}});
  key(true);assert.equal(dialog.open,true);assert.equal(cancels,0,'The child menu or swatch sheet already handled Escape');
  key(false);assert.equal(dialog.open,false);assert.equal(cancels,1);host.dispose();
});

test('a nonmodal canvas panel leaves keyboard and native pen delivery available after hide',async t=>{
  const {createDialogHost,hasModalDialog}=await import('./dialog-host.js');
  const {observePenInput}=await import('./pen-input.js');
  const {doc,element,button,icon}=colorDialogDom(t),dialog=element('dialog'),header=element('header'),cancel=element('button'),canvas=element('canvas');
  header.append(cancel);dialog.append(header);doc.body.append(dialog,canvas);dialog.show=()=>{dialog.open=true;};
  const actions=[],listeners=new Map(),context={Element:canvas.constructor,hasModalDialog,composingKey:()=>false,latinKey,updateZen(){},keyboardFocused:new WeakSet(),app:{control_takes_key:()=>false},documents:{allowNativePaste:()=>false},input:event=>{actions.push(event);return{handled:true};}};
  runInNewContext(source.slice(source.indexOf('function keyInput('),source.indexOf('const keyboardFocused =')),context);
  observePenInput({workspace:{addEventListener:(type,handler)=>listeners.set(type,handler)},canvas,position:()=>[10,20],dispatch:action=>actions.push(action)});
  const app={dialog_presentation:()=>({kind:'floating'}),dialog_header:()=>({start:'cancel',end:null})};
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:{cancel:'Cancel'},onCancel(){}});
  host.present({frame:{kind:'editor'},role:'canvas_panel',modal:false,primary:'action'});
  const opened=[],admission={active:new Set(),batching:false,changing:false,closing:false,hasModalDialog,openBatch:files=>opened.push(files),transportFailure:assert.fail};
  const documentSource=readFileSync(new URL('documents.js',import.meta.url),'utf8');
  runInNewContext(documentSource.slice(documentSource.indexOf('async function openFiles('),documentSource.indexOf('const handleStore=')),admission);
  const file={name:'owned.capy'};await admission.openFiles([file]);assert.equal(opened[0][0].file,file,'A nonmodal calibration panel does not block opening a drawing');
  const {fakeColorApp}=await import('./color-dialog-fixture.mjs'),{chooseColor}=await import('./color-editor.js');
  const color=chooseColor({app:fakeColorApp().app,slot:'foreground',element,button});
  const colorDialog=doc.body.children.find(node=>node!==dialog&&node.tagName==='DIALOG');
  assert.equal(colorDialog.descendants().find(node=>node.className==='color-pick').hidden,false,'A nonmodal calibration panel leaves canvas picking available');
  colorDialog.close();await color;
  const facts=[],chrome={hasModalDialog,workspaceViewport:[1200,900],customization:{placement:()=>null},workspaceChrome:{facts:()=>({})},heldPointers:new Map(),dragItem:null,
    document:{querySelector:selector=>selector.includes('dialog[open]')?dialog:null,elementFromPoint:()=>canvas},CSS:{supports:()=>false},input:event=>{facts.push(event.facts);return{paint:!event.facts.popup_open};}};
  runInNewContext(source.slice(source.indexOf('function chromeInput('),source.indexOf('function updateZen(')),chrome);
  for(const retire of [()=>{},()=>host.hide(),()=>host.dispose()]){
    retire();assert.equal(hasModalDialog(),false);
    assert.equal(chrome.chromeInput({kind:'contact',position:[10,20]}).paint,true,'Nonmodal calibration never reports a blocking popup');
    context.keyInput({target:canvas,key:'b',defaultPrevented:false,preventDefault(){},stopPropagation(){}},true);
    listeners.get('pointerdown')({target:canvas,pointerType:'pen',pointerId:actions.length+1});
    assert.deepEqual(actions.slice(-2).map(action=>action.type),['key','pen_input']);
  }
  assert.ok(facts.every(fact=>fact.popup_open===false));
});

function harness(moving = false) {
  const calls = [];
  const events = [];
  let current = 0;
  const context = {
    scheduled: false,
    state: { settings_open: false },
    startupTimes: { complete: 1 },
    pending: [],
    performance: { now: () => current },
    app: { frame(...args) { calls.push(args); return {}; } },
    flushWorkspacePresentation() {},
    bubbles: { tick(time) { events.push(['bubble',time]); return moving; } },
    glass: { flush() { events.push(['glass']); } },
    applyChange() {},
    refreshStartup() {},
    scheduleCompiler() {},
    wake() { events.push(['wake']); },
    stopGpu(error) { throw error; },
  };
  runInNewContext(source.slice(
    source.indexOf("const frameIntervals = []"),
    source.indexOf("function refreshStartup()"),
  ), context);
  return {
    events,
    tick(time, now = time + 2) {
      current = now;
      context.frame(time);
      return calls.at(-1);
    },
  };
}

test('bubble motion precedes glass publication and continues only while moving', () => {
  const moving = harness(true);
  moving.tick(16);
  assert.deepEqual(moving.events,[['bubble',16],['wake'],['glass']]);
  moving.tick(32);
  assert.deepEqual(moving.events.slice(3),[['bubble',32],['wake'],['glass']]);
  const settled = harness();
  settled.tick(48);
  assert.deepEqual(settled.events,[['bubble',48],['glass']]);
});

test("prediction uses current input time and follows a 90 Hz display despite missed frames", () => {
  const h = harness(), interval = 1000 / 90;
  for (let i = 0; i < 40; i++) h.tick(i * interval);
  const [now, presentation] = h.tick(42 * interval, 42 * interval + 5);
  assert.equal(now, 42 * interval + 5);
  assert.ok(Math.abs(presentation - 43 * interval) < .01);
  const late = h.tick(43 * interval, 45 * interval + 1);
  assert.equal(late[0], 45 * interval + 1);
  assert.ok(late[1] > late[0] && late[1] - late[0] <= interval + .01);
});

test("display estimate adapts after moving to a 60 Hz screen and excludes idle gaps", () => {
  const h = harness();
  for (let i = 0; i < 40; i++) h.tick(i * 1000 / 120);
  for (let i = 0; i < 40; i++) h.tick(1000 + i * 1000 / 60);
  const [, presentation] = h.tick(1000 + 40 * 1000 / 60);
  assert.ok(Math.abs(presentation - (1000 + 41 * 1000 / 60)) < .01);
  const idle = h.tick(10000, 10002);
  assert.ok(idle[1] > 10002 && idle[1] < 10020);
});

test('shared chrome replies retain the same Capy control and contact state', t => {
  const {doc,element}=colorDialogDom(t),workspace=element('div'),capy=element('button');
  capy.id='zen-capy';workspace.append(capy);doc.body.append(workspace);capy.focus();
  capy.setAttribute('data-pressed','');
  let geometry={capy_item:0,size:'medium'},editing=false,reply,hidden=false;
  const writes=[];
  Object.defineProperty(capy,'hidden',{get:()=>hidden,set:value=>{writes.push(value);hidden=value;}});
  const context={workspace,document:doc,canvas:{style:{}},$:(id)=>id==='zen-capy'?capy:null,
    app:{input:()=>reply},applyChange(){},message:assert.fail,
    header:{interaction:value=>presentCapy(capy,geometry,value.chrome_hidden,value.keep_zen_button,editing)}};
  runInNewContext(source.slice(source.indexOf('function input(event)'),source.indexOf('function chromeInput(event)')),context);
  const same=()=>{
    assert.equal(doc.activeElement,capy);
    assert.equal(workspace.firstChild,capy);
    assert.equal(capy.parentNode,workspace);
    assert.equal(capy.id,'zen-capy');
    assert.ok(capy.hasAttribute('data-pressed'));
  };
  for(const [chrome_hidden,keep_zen_button] of [[false,false],[true,true],[false,false],[true,true],[false,false]]){
    reply={chrome_hidden,keep_zen_button,change:{}};
    context.input({type:'chrome'});same();
    geometry={capy_item:0,size:'large'};
    presentCapy(capy,geometry,chrome_hidden,keep_zen_button,editing);same();
    assert.equal(capy.hidden,false);
  }
  assert.deepEqual(writes,[],'eligible mode/geometry publications never hide the retained control');
  for(const [present,chrome_hidden,keep_zen_button,edit,expected] of [
    [true,true,false,false,true],[true,false,false,false,false],[true,false,false,true,true],
    [false,false,false,false,true],[false,true,true,false,false],[false,true,false,false,true],
  ]){
    geometry=present?{capy_item:0,size:'small'}:{capy_item:null,size:'small'};editing=edit;
    reply={chrome_hidden,keep_zen_button,change:{}};context.input({type:'chrome'});
    assert.equal(capy.hidden,expected);assert.equal(workspace.firstChild,capy);
    const count=writes.length;
    presentCapy(capy,geometry,chrome_hidden,keep_zen_button,editing);
    assert.equal(writes.length,count,'unchanged visibility does not write hidden again');
  }
  const headerSource=readFileSync(new URL('header.js',import.meta.url),'utf8');
  const accepted={size:'medium',capy_item:0,items:[],zones:[],bars:[],overflow:[],hidden:[]};
  const native={...context,capy,root:element('div'),geometry:accepted,preview:{...accepted,capy_item:null},
    view:{sizes:[{id:'medium'}]},records:new Map(),bars:[],overflow:[],zones:[],place(){},reconcileCapy(){}};
  native.root.style.setProperty=workspace.style.setProperty=()=>{};
  runInNewContext(headerSource.slice(headerSource.indexOf('  function present('),headerSource.indexOf('  function allocate('))+'present(preview,true);',native);
  assert.equal(native.geometry,accepted,'drag presentation preserves the accepted layout for cancellation');
});

test('panel frames keep one scroll owner through body overflow, scroll movement and thumb dragging', () => {
  const callbacks=[],resizes=[],mutations=[];
  class Node {
    constructor(className='') {
      this.classes=new Set(className.split(' '));this.classList={add:name=>this.classes.add(name)};
      this.children=[];this.listeners={};this.top=0;this.clientHeight=100;this.extent=100;
      this.style=new Proxy({}, {set:(style,key,value)=>{style[key]=value;for(const observer of mutations)if(observer.options.attributes&&observer.options.subtree)observer.callback();return true;}});
    }
    get scrollHeight(){return Math.max(this.clientHeight,this.extent,...this.children.filter(child=>child.className==='scroll-thumb'&&!child.hidden).map(child=>(parseFloat(child.style.top)||0)+(parseFloat(child.style.height)||0)));}
    get scrollTop(){return this.top;}
    set scrollTop(value){this.top=Math.max(0,Math.min(value,this.scrollHeight-this.clientHeight));}
    append(child){this.children.push(child);child.parentElement=this;}
    setAttribute(){}
    addEventListener(type,listener){this.listeners[type]=listener;}
    emit(type,event={}){this.listeners[type]?.(event);}
    setPointerCapture(id){this.captured=id;}
    hasPointerCapture(id){return this.captured===id;}
    get offsetHeight(){return parseFloat(this.style.height)||0;}
    getBoundingClientRect(){return{top:this.rectTop??0,right:this.rectRight??100};}
  }
  const context={document:{createElement:()=>new Node()},
    getComputedStyle:node=>({overflowY:node.overflowY??'auto'}),
    element:(_,className)=>new Node(className),
    requestAnimationFrame(callback){callbacks.push(callback);return callbacks.length;},
    ResizeObserver:class{constructor(callback){this.callback=callback;this.nodes=[];resizes.push(this);}observe(node){this.nodes.push(node);}},
    MutationObserver:class{constructor(callback){this.callback=callback;mutations.push(this);}observe(node,options){this.node=node;this.options=options;}}};
  const scrollbar=readFileSync(new URL('scrollbars.js',import.meta.url),'utf8').replace('export function addScrollbar','function addScrollbar');
  const panelFrame=source.slice(source.indexOf('function panelFrame('),source.indexOf('function commandButton('));
  runInNewContext(scrollbar+panelFrame+'globalThis.panelFrame=panelFrame;',context);
  const panel=new Node(),frame=context.panelFrame(panel),thumb=frame.children[1],viewport=[frame,...frame.children].find(node=>node.listeners.scroll);
  assert.ok(frame.classes.has('scrollable'));
  assert.notEqual(viewport,frame);assert.equal(thumb.parentElement,frame,'scrollbar chrome stays outside the content viewport');
  assert.deepEqual(resizes[0].nodes,[viewport,panel],'both viewport and inner minimum-body changes refresh the same scrollbar');
  const flush=()=>{assert.equal(callbacks.length,1);callbacks.shift()();assert.equal(callbacks.length,0,'thumb style writes do not request another animation frame');};
  viewport.extent=300;panel.clientHeight=300;resizes[0].callback();flush();
  assert.equal(thumb.hidden,false);assert.ok(Math.abs(parseFloat(thumb.style.height)-100/3)<1e-8);
  viewport.scrollTop=60;viewport.emit('scroll');flush();
  assert.ok(Math.abs(parseFloat(thumb.style.top)-frame.scrollTop-20)<1e-8,'viewport thumb stays visible while its parent scrolls');
  thumb.emit('pointerdown',{pointerType:'mouse',pointerId:4,clientY:30,preventDefault(){}});
  thumb.emit('pointermove',{pointerType:'mouse',pointerId:4,clientY:40});
  assert.ok(Math.abs(viewport.scrollTop-90)<1e-8);assert.equal(panel.scrollTop,0,'the natural-size inner panel never becomes a second scroll owner');
  viewport.extent=500;panel.clientHeight=500;resizes[0].callback();flush();
  assert.equal(parseFloat(thumb.style.height),28,'content resize refreshes overflow without resizing the frame');
  viewport.scrollTop=400;viewport.emit('scroll');flush();
  viewport.clientHeight=490;viewport.extent=490;viewport.scrollTop=viewport.scrollTop;panel.clientHeight=490;resizes[0].callback();flush();
  assert.equal(thumb.hidden,true,'after scrolling, fitted content cannot retain overflow from the scrollbar itself');
  assert.equal(viewport.scrollHeight,viewport.clientHeight,'chrome contributes no extent to the content viewport');
  frame.rectTop=10;frame.rectRight=250;viewport.rectTop=57;viewport.rectRight=238;
  viewport.clientHeight=100;viewport.extent=300;viewport.scrollTop=200;resizes[0].callback();flush();
  assert.ok(Math.abs(parseFloat(thumb.style.top)+parseFloat(thumb.style.height)-147)<1e-8,'the overlay track ends at the body, above a sibling footer');
  assert.equal(thumb.style.right,'12px','the indicator follows the body inset rather than the full dialog edge');
  viewport.clientHeight=12;resizes[0].callback();flush();assert.equal(thumb.style.height,'12px','a tiny body cannot draw its indicator into the footer');
  viewport.overflowY='visible';resizes[0].callback();flush();assert.equal(thumb.hidden,true,'shared drawer scrolling does not leave a second column indicator');
  const excluded=context.panelFrame(new Node(),false);assert.equal(excluded.children.length,1);assert.ok(!excluded.classes.has('scrollable'),'dedicated layer and toolbar scroll policies remain excluded');
});

test('dialog and drawer scrollers cannot expose classic browser scrollbars',()=>{
  const css=readFileSync(new URL('style.css',import.meta.url),'utf8'),rules=[...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)];
  const body=selector=>rules.filter(rule=>rule[1].trim()===selector).at(-1)?.[2]??'';
  const scope=rules.find(rule=>rule[1].includes('dialog *')&&rule[1].includes('.content-drawer *'));
  assert.ok(scope,'all dialog and drawer descendants are covered');assert.match(scope[2],/scrollbar-width:\s*none\b/);
  assert.doesNotMatch(css,/scrollbar-width:\s*(auto|thin)\b/);
  assert.doesNotMatch(body('.scroll-thumb'),/min-height:\s*[1-9]/,'CSS cannot enlarge a thumb past its measured body');
  assert.match(body('dialog'),/overflow:\s*hidden\b/);
  assert.match(body('dialog[open]'),/display:\s*flex\b/);
  assert.match(body('.dialog-content'),/overflow:\s*hidden\b/);
  assert.match(body('.dialog-page > .dialog-content'),/overflow:\s*hidden\b/);
  assert.doesNotMatch(body('.dialog-page > form'),/overflow:\s*(auto|scroll)\b/);
});

test('drawer plans follow exact native tile facts and retire missing sources',()=>{
  const chrome=readFileSync(new URL('workspace-chrome.js',import.meta.url),'utf8');
  const native={column:41,anchor:{panel:'toolbar',tile:2},bounds:{x:12,y:24,width:36,height:36}};
  let plans=0,placements=0,disposed=0;
  const body={style:{},children:[],querySelectorAll:()=>[]},record={contents:{classList:{toggle(){}}},column:null,drawer:{anchor:{kind:'tile',panel:'toolbar',tile:2}},bodies:[body],root:{style:{},querySelector:()=>null},shadow:{style:{}},started:0};
  const bounds={x:0,y:0,width:242,height:120},drawers=new Map([['tool',record]]);
  const context={drawers,markedSources:new Set(),tiles:[structuredClone(native)],animating:false,catalog:{panel_expansion_ms:200},previousAnimationFrame:null,frameInterval:()=>16,
    workspace:{clientWidth:1200,clientHeight:900,querySelectorAll:()=>[]},
    app:{drawer_measure(){plans++;return{measure:context.tiles.length?{placement:{columns:[bounds]}}:null,toolbars:[]};},drawer(){placements++;return{placement:{bounds,columns:[bounds],direction:'right'},connection:null};}},
    contentHeight:()=>120,matchMedia:()=>({matches:false}),place(){},dispose(){disposed++;},dispatch(){},measureColumnDrawers(){},editor:{queuePositions(){}},
    requestAnimationFrame(){assert.fail('A completed drawer does not schedule another callback');}};
  const start=chrome.indexOf('  function animate('),end=chrome.indexOf('  workspace.addEventListener("scroll"',start);
  assert.ok(start>=0&&end>start);runInNewContext(chrome.slice(start,end)+'\nglobalThis.step=()=>animate(200);',context);
  const step=facts=>{context.tiles=facts?[structuredClone(facts)]:[];context.step();};
  step(native);assert.equal(plans,1);
  step(native);assert.equal(plans,1,'Identical native facts retain the width plan');
  for(const key of ['column','x','y','width','height']){
    const changed=structuredClone(native);if(key==='column')changed.column++;else changed.bounds[key]++;
    const before=plans;step(changed);assert.equal(plans,before+1,`${key} changes invalidate the old plan`);
    step(changed);assert.equal(plans,before+1,`${key} unchanged across placements retains the plan`);
  }
  const before=placements;step(null);assert.equal(disposed,1);assert.equal(drawers.has('tool'),false,'A missing native source leaves no drawer record');assert.equal(placements,before,'Missing sources are not placed from a stale plan');
});

test('drawer sources resolve typed phone anchors without a fabricated tile id',()=>{
  const chrome=readFileSync(new URL('workspace-chrome.js',import.meta.url),'utf8'),queries=[];
  const node={isConnected:true};
  const context={workspace:{querySelector(selector){queries.push(selector);return node;},querySelectorAll:assert.fail}};
  const start=chrome.indexOf('  function refreshSource('),end=chrome.indexOf('  function bridge(',start);
  runInNewContext(chrome.slice(start,end)+'globalThis.resolveSource=refreshSource;',context);
  const record={drawer:{anchor:{kind:'phone',anchor:{kind:'capy'}}}};
  for(const [anchor,selector]of [[{kind:'capy'},'#zen-capy'],[{kind:'tool_settings'},'#phone-tool-settings'],[{kind:'tool',id:104},'#phone-tool-104']]){
    record.drawer.anchor.anchor=anchor;context.resolveSource(record);assert.equal(record.source,node);assert.equal(queries.at(-1),selector);
    const count=queries.length;context.resolveSource(record);assert.equal(queries.length,count,'An unchanged source is retained');
  }
  record.drawer.anchor={kind:'missing'};context.resolveSource(record);assert.equal(record.source,null);
});

test('drawer ticks clear owned marks without scanning the workspace',()=>{
  const chrome=readFileSync(new URL('workspace-chrome.js',import.meta.url),'utf8');
  const bounds={x:10,y:10,left:10,top:10,right:30,bottom:30,width:20,height:20};
  let queries=0;
  const workspace={clientWidth:1200,clientHeight:900,querySelectorAll(){queries++;return [visible];}};
  const ancestor={dataset:{},parentElement:workspace,getBoundingClientRect:()=>bounds};
  const source={dataset:{},parentElement:ancestor,isConnected:true,getBoundingClientRect:()=>bounds,closest:()=>null};
  let visible=source;
  const placement={bounds:{x:40,y:10,width:100,height:100},columns:[],direction:'right'};
  const record={contents:{classList:{toggle(){}}},column:null,drawer:{anchor:{kind:'tile',panel:'toolbar',tile:2}},bodies:[],root:{style:{},querySelector:()=>null},shadow:{style:{}},started:null,source};
  const context={workspace,markedSources:new Set(),drawers:new Map([['tool',record]]),tiles:[{column:1,anchor:{panel:'toolbar',tile:2},bounds}],
    animating:true,previousAnimationFrame:null,frameInterval:()=>16,catalog:{panel_expansion_ms:200},
    getComputedStyle:()=>({borderTopLeftRadius:'8px',borderTopRightRadius:'8px',borderBottomRightRadius:'8px',borderBottomLeftRadius:'8px'}),
    app:{toolbar_ui(request){assert.equal(request.type,'drawer_source_corners');return[false,true,true,false];},drawer_measure:()=>({measure:{placement},toolbars:[]}),drawer:()=>({placement,connection:null})},
    matchMedia:()=>({matches:false}),place(){},dispatch(){},measureColumnDrawers(){},editor:{queuePositions(){}},requestAnimationFrame(){}};
  const start=chrome.indexOf('  function markSource('),end=chrome.indexOf('  workspace.addEventListener("scroll"',start);
  runInNewContext(chrome.slice(start,end)+'\nglobalThis.mark=markSource;globalThis.tick=animate;',context);
  for(const time of [100,116]){
    context.mark(source,'right');assert.equal(source.dataset.drawerFacing,'right');assert.ok(ancestor.dataset.drawerSourceCorners);
    context.tick(time);
    assert.equal(queries,0,'Animation ticks do not query the whole workspace');
    assert.equal(source.dataset.drawerFacing,undefined);assert.equal(ancestor.dataset.drawerSourceCorners,undefined);
    assert.equal(context.markedSources.size,0,'Cleared nodes are released');
  }
  runInNewContext('globalThis.resolveSource=refreshSource;',context);
  context.resolveSource(record);context.resolveSource(record);assert.equal(queries,1,'An unchanged anchor retains its source');
  record.drawer.anchor.tile=3;context.resolveSource(record);assert.equal(queries,2,'An anchor change resolves its source once');
  source.isConnected=false;visible={...source,isConnected:true};context.resolveSource(record);
  assert.equal(queries,3);assert.equal(record.source,visible,'A replaced source is reacquired on refresh');
});

test('drawer progress follows frame deltas through delayed callbacks, close and idle restart',()=>{
  const chrome=readFileSync(new URL('workspace-chrome.js',import.meta.url),'utf8');
  const fixture=(closing=false,switching=false)=>{
    const queries=[],callbacks=[],body={style:{},children:[],querySelectorAll:()=>[]},root={style:{},querySelector:()=>null,querySelectorAll:()=>[]},bounds={x:48,y:48,width:242,height:120};
    const record={contents:{classList:{toggle(){}}},column:46,drawer:{anchor:{kind:'column',column:46,origin:'brushes'}},bodies:[body],root,shadow:{style:{}},started:null,closing,switching};
    const context={drawers:new Map([[46,record]]),markedSources:new Set(),tiles:[],animating:true,previousAnimationFrame:null,catalog:{panel_expansion_ms:200},frameInterval:()=>16,entryTime:101.6,
      performance:{now:()=>context.entryTime},workspace:{clientWidth:1200,clientHeight:900,querySelectorAll:()=>[],querySelector:()=>null},
      app:{drawer_measure:()=>({measure:{placement:{columns:[bounds]}},toolbars:[]}),drawer(query){queries.push(query);const width=bounds.width*(query.closing?1-query.progress:Math.max(0,query.progress)),placement={...bounds,width};return{placement:{bounds:placement,columns:[placement],direction:'right'},connection:null};}},
      contentHeight:()=>120,matchMedia:()=>({matches:false}),place:(node,box)=>Object.assign(node.style,box),dispose(){},dispatch(){},measureColumnDrawers(){},editor:{queuePositions(){}},requestAnimationFrame:callback=>callbacks.push(callback)};
    const start=chrome.indexOf('  function animate('),end=chrome.indexOf('  workspace.addEventListener("scroll"',start);
    assert.ok(start>=0&&end>start);runInNewContext(chrome.slice(start,end)+'\nglobalThis.onFrame=animate;',context);
    return{context,record,root,queries,callbacks,bounds};
  };
  for(const closing of [false,true]){
    const f=fixture(closing),times=[100,116,133,149],entry=[101.6,120.8,146.4,152.5];
    times.forEach((time,i)=>{f.context.entryTime=entry[i];(i?f.callbacks.shift():f.context.onFrame)(time);if(!i){assert.ok(f.root.style.width>0,'The first callback publishes positive width');assert.equal(f.callbacks.length,1,'Only the next animation callback is queued');}});
    for(let i=1;i<times.length;i++)assert.ok(Math.abs((f.queries[i].progress-f.queries[i-1].progress)*200-(times[i]-times[i-1]))<1e-10,'Progress advances exactly by the frame timestamp delta');
    assert.ok(f.queries[0].progress>0&&f.queries[0].progress<1,'The first callback shows one frame of motion');
    assert.ok(Math.abs(f.queries[0].progress-.08)<1e-10);assert.ok(f.root.style.width>0);
    assert.equal(f.queries.length,4,'Each real callback queries placement once');
    f.context.entryTime=19000;f.callbacks.shift()(400);assert.equal(f.callbacks.length,0);
    f.record.started=null;f.record.closing=false;f.context.drawers.set(46,f.record);f.context.onFrame(10000);
    assert.ok(Math.abs(f.queries.at(-1).progress-.08)<1e-10,'An idle gap is not a frame interval');
  }
  const switched=fixture(false,true);switched.context.onFrame(100);assert.equal(switched.queries[0].progress,1,'Switching an opener keeps its immediate placement');
  switched.record.started=null;switched.record.closing=true;switched.context.onFrame(1000);
  assert.ok(Math.abs(switched.queries.at(-1).progress-.08)<1e-10,'Closing does not inherit the switch offset');
});

test('dialog presentation retains controls and ignores a retired native close',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),events=new Map(),viewportEvents=new Map();
  const previous=Object.fromEntries(['window','innerWidth','innerHeight'].map(key=>[key,globalThis[key]]));
  const emitter=events=>({addEventListener:(type,handler)=>events.set(type,handler),removeEventListener:type=>events.delete(type)});
  Object.assign(globalThis,{window:{...emitter(events),visualViewport:{...emitter(viewportEvents),width:412,height:891,offsetLeft:0,offsetTop:0}},innerWidth:412,innerHeight:891});
  t.after(()=>Object.assign(globalThis,previous));
  const dialog=element('dialog'),header=element('header'),cancel=element('button'),primary=element('button'),search=element('input'),shortcutSearch=element('input');
  const inner=element('header'),originalIcon=icon('window-close');primary.append(originalIcon);primary.setAttribute('aria-label','Original close');
  header.append(cancel,primary);dialog.append(header,inner,search,shortcutSearch);doc.body.append(dialog);
  const focus=element('button');doc.body.append(focus);focus.isConnected=true;focus.focus();
  let page=true,clears=0,cancels=0,backs=0,sourceCancels=0,subpage=false,currentHeader=header,currentPrimary=primary,clearedNode=null;
  const app={dialog_presentation:(_role,_w,_h,visible)=>page?{kind:'page',bounds:visible}:{kind:'floating'},floating_dialog_width:(_window,natural)=>natural,
    alert_dialog_short:()=>false,alert_dialog_layout:()=>({width:300,short:false,stacked:false}),
    dialog_header:(intent,back)=>({start:back?'back':intent.primary==='close'?null:'cancel',end:intent.primary==='close'?'close':'primary'}),
    dialog_escape:(back,focused,text)=>focused&&text?'clear_search':back?'back':'cancel'};
  const host=createDialogHost({app,dialog,header:()=>currentHeader,cancel,primary:()=>currentPrimary,search:()=>doc.activeElement===shortcutSearch?shortcutSearch:search,icon,copy:{cancel:'Cancel',back:'Back',done:'Done',close:'Close'},
    subpage:()=>subpage,onBack:()=>{backs++;subpage=false;currentHeader=header;host.layout();},
    onCancel:()=>{cancels++;host.hide();},clearSearch:node=>{clears++;clearedNode=node;node.value='';}});
  const intent={frame:{kind:'editor'},role:'editor',modal:true,primary:'action'};
  assert.equal(host.present(intent),true);search.value='retained draft';search.setSelectionRange(2,7);search.focus();search.scrollTop=24;
  window.visualViewport.height=480;viewportEvents.get('resize')();
  assert.equal(dialog.style.height,'480px');assert.equal(doc.activeElement,search);assert.equal(search.value,'retained draft');
  assert.deepEqual([search.selectionStart,search.selectionEnd,search.scrollTop],[2,7,24]);
  page=false;events.get('resize')();assert.equal(dialog.style.height,'');assert.equal(doc.activeElement,search);
  const escape=(composing=false)=>dialog.dispatchEvent({type:'keydown',key:'Escape',isComposing:composing,preventDefault(){},stopPropagation(){}});
  escape(true);assert.equal(clears,0);escape();assert.equal(clears,1);assert.equal(cancels,0);
  search.value='';escape();assert.equal(cancels,1);assert.equal(doc.activeElement,focus);
  dialog.dispatchEvent({type:'cancel',preventDefault(){}});assert.equal(cancels,1,'A retired surface cannot cancel again');
  dialog.close=()=>{dialog.open=false;};host.present(intent);host.hide();host.present(intent);
  dialog.dispatchEvent({type:'close'});assert.equal(cancels,1,'An old queued close cannot cancel the reopened form');
  assert.equal(dialog.open,true);
  page=true;host.present({...intent,primary:'close'});
  assert.equal(cancel.hidden,true);assert.equal(primary.hidden,true);
  const done=header.children.find(node=>node.dataset.dialogPart==='end');
  assert.ok(done);assert.equal(done.textContent,'Done');assert.deepEqual(primary.childNodes,[originalIcon],'The projected Done leaves the original primary untouched');
  subpage=true;currentHeader=inner;host.layout();
  assert.equal(header.classList.contains('page-header'),false);assert.equal(inner.classList.contains('page-header'),true);
  assert.equal(done.parentNode,inner,'The single projected action follows the active header');
  const back=inner.children.find(node=>node.dataset.dialogPart==='start');
  assert.notEqual(back,cancel);assert.equal(back.getAttribute('aria-label'),'Back');assert.equal(cancel.hidden,true);
  shortcutSearch.value='retained subpage query';shortcutSearch.focus();escape();
  assert.equal(backs,0);assert.equal(clears,2);assert.equal(clearedNode,shortcutSearch);assert.equal(shortcutSearch.value,'');assert.equal(subpage,true);
  escape();assert.equal(backs,1);assert.equal(cancels,1);
  subpage=true;host.dismiss();assert.equal(backs,2);
  subpage=true;page=false;host.layout();assert.equal(back.getAttribute('aria-label'),'Back');assert.equal(back.hidden,false);assert.equal(cancel.hidden,true);assert.equal(back.parentNode,header);
  subpage=false;
  primary.textContent='Relabelled original';
  page=false;host.layout();assert.equal(primary.textContent,'Relabelled original');
  assert.equal(primary.hidden,false);assert.equal(done.hidden,true);
  assert.equal(primary.getAttribute('aria-label'),'Original close');assert.equal(cancel.hidden,true);
  cancel.addEventListener('click',()=>{sourceCancels++;host.hide();});
  for(const skin of [true,false]){page=skin;subpage=true;host.layout();assert.equal(cancel.hidden,true);assert.equal(back.parentNode,currentHeader);back.click();}
  assert.equal(backs,4,'Shared Back activates through its projected control in both skins');assert.equal(sourceCancels,0);
  page=true;host.present(intent);const close=header.children.find(node=>node.dataset.dialogPart==='start');
  assert.notEqual(close,cancel);assert.equal(close.firstElementChild.dataset.asset,'window-close');assert.equal(cancel.hidden,true);
  close.click();assert.equal(cancels,2);assert.equal(sourceCancels,0,'The projected X invokes shell cancellation, not the original callback');
  host.present(intent);subpage=true;host.layout();cancel.click();assert.equal(sourceCancels,1,'Programmatic original Cancel remains terminal on a subpage');assert.equal(backs,4);
  subpage=false;page=false;host.present(intent);assert.equal(cancel.hidden,false);cancel.click();assert.equal(sourceCancels,2,'The original Cancel callback still closes at the root');
  page=true;host.present({...intent,primary:'close'});done.click();assert.equal(cancels,3);
  const footer=element('footer'),apply=element('button'),after=element('span');footer.append(apply,after);apply.nextSibling=after;dialog.append(footer);
  let applied=0,closedByOriginal=0;apply.addEventListener('click',()=>{applied++;});primary.addEventListener('click',()=>{closedByOriginal++;});
  currentPrimary=apply;host.present(intent);
  assert.equal(apply.parentNode,header,'Workspaces Apply is the live projected primary');assert.equal(primary.hidden,true);
  apply.click();assert.equal(applied,1);
  currentPrimary=primary;host.present({...intent,primary:'close'});
  assert.equal(apply.hidden,true,'The previous primary is retired when the toolbar Close takes over');assert.equal(done.hidden,false);
  page=false;host.layout();
  assert.deepEqual(footer.children,[apply,after],'Returning to floating restores the original Apply slot');
  assert.equal(primary.parentNode,header);assert.equal(primary.hidden,false);primary.click();assert.equal(closedByOriginal,1,'The original Close node keeps its callback');
  page=true;currentPrimary=apply;host.present(intent);apply.click();assert.equal(applied,2,'Returning to Apply keeps its original node and callback');
  page=false;host.layout();assert.deepEqual(footer.children,[apply,after]);
  page=false;subpage=false;currentPrimary=primary;host.present({frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:'action'});
  assert.equal(currentHeader.dataset.dialogPart,undefined,'A floating alert has a heading and response buttons, without a page header');
  assert.equal(cancel.dataset.dialogPart,undefined);assert.equal(primary.dataset.dialogPart,undefined);
  host.dispose();assert.equal(events.size,0);assert.equal(viewportEvents.size,0);
});

test('alert layout forwards intrinsic content and font measurements and applies the shared projection',async t=>{
  const {createDialogHost,createProgressDialog}=await import('./dialog-host.js');
  const {doc,element,button,icon}=colorDialogDom(t),calls=[],changes=[],frames=[];
  globalThis.requestAnimationFrame=callback=>{frames.push(callback);return frames.length;};
  globalThis.MutationObserver=class{constructor(callback){this.callback=callback;changes.push(this);}observe(node){this.node=node;}disconnect(){this.disconnected=true;}};
  const dialog=element('dialog'),header=element('header'),footer=element('footer'),cancel=element('button'),primary=element('button');
  const title=element('h2','','Confirm');title.getBoundingClientRect=()=>({width:150});header.append(title);
  dialog.append(header,footer);footer.append(cancel,primary);doc.body.append(dialog);
  dialog.getBoundingClientRect=()=>({width:dialog.style.width==='min-content'?96:420});
  let widths=[70,118];cancel.getBoundingClientRect=()=>({width:widths[0]});primary.getBoundingClientRect=()=>({width:widths[1]});
  let fontPixels=44/3,projection={width:300,short:false,stacked:true};
  globalThis.getComputedStyle=node=>({boxSizing:'border-box',fontSize:`${fontPixels}px`,paddingLeft:node===dialog?'24px':'0px',paddingRight:node===dialog?'24px':'0px'});
  const app={dialog_presentation:()=>({kind:'floating'}),dialog_header:intent=>({start:'cancel',end:intent.primary?'primary':null}),dialog_escape:()=> 'cancel',bootstrap_view:()=>({common:{cancel:'Cancel'}}),
    alert_dialog_short:()=>projection.short,alert_dialog_layout:(viewport,metrics,font,preferWide)=>{calls.push({viewport,metrics,font,preferWide});return projection;}};
  const host=createDialogHost({app,dialog,header,cancel,primary,icon,copy:{cancel:'Cancel',back:'Back',done:'Done'},onCancel:()=>host.hide()});
  host.present({frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:'action'});
  assert.equal(header.dataset.headerStyle,undefined,'Alert headings keep their native bold label style');
  assert.deepEqual(calls.at(-1),{viewport:[1200,900],metrics:{minimum:96,natural:420,heading:198,responses:[70,118]},font:44/3,preferWide:false});assert.equal(dialog.style.width,'300px');assert.equal(footer.classList.contains('alert-responses-stacked'),true);
  projection={width:352,short:false,stacked:false};host.layout();assert.equal(dialog.style.width,'352px');assert.equal(footer.classList.contains('alert-responses-stacked'),false);
  widths=[70,140];changes[0].callback();frames.shift()();assert.deepEqual(calls.at(-1).metrics.responses,[70,140]);assert.equal(changes[0].node,dialog,'heading and body copy changes also invalidate natural sizing');
  fontPixels=64/3;projection={width:300,short:true,stacked:false};host.layout();assert.equal(calls.at(-1).font,64/3);assert.equal(dialog.classList.contains('short'),true);
  host.hide();host.dispose();assert.equal(changes[0].disconnected,true);
  const progress=createProgressDialog({app,element,button,icon,intent:{frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:null},title:'Preparing',body:'Working',onCancel:()=>{}});
  assert.deepEqual(calls.at(-1).metrics.responses,[0]);progress.remove();
  const copying=createProgressDialog({app,element,button,icon,intent:{frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:null},title:'Copy Merged',onCancel:()=>{}});
  assert.deepEqual(copying.title.parentNode.parentNode.children.map(node=>node.tagName),['HEADER','FOOTER']);copying.remove();
  const exporting=createProgressDialog({app,element,button,icon,intent:{frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:null},title:'Exporting',body:'Preparing export',progressBar:true,onCancel:()=>{}});
  const track=exporting.label.parentNode.children[1];assert.equal(track.getAttribute('role'),'progressbar');assert.equal(track.getAttribute('aria-labelledby'),exporting.label.id);assert.equal(track.getAttribute('aria-valuenow'),null);exporting.complete();assert.equal(track.getAttribute('aria-valuenow'),'100');exporting.remove();
});


test('dialog Cancel keeps its bound text through page, floating and language changes',async t=>{
  const {createDialogHost}=await import('./dialog-host.js');
  const {doc,element,icon}=colorDialogDom(t),previous={window:globalThis.window,innerWidth:globalThis.innerWidth,innerHeight:globalThis.innerHeight};
  Object.assign(globalThis,{window:{addEventListener(){},removeEventListener(){}},innerWidth:412,innerHeight:891});
  t.after(()=>Object.assign(globalThis,previous));
  const dialog=element('dialog'),header=element('header'),cancel=element('button'),primary=element('button'),copy={cancel:'Cancel',back:'Back',done:'Done'};
  bindCopy(cancel,()=>copy.cancel);const bound=cancel.firstChild;
  const footer=element('footer');footer.append(cancel,primary);dialog.append(header,footer);doc.body.append(dialog);
  let page=true;
  const app={dialog_presentation:()=>page?{kind:'page',bounds:{x:0,y:0,width:412,height:891}}:{kind:'floating'},floating_dialog_width:(_window,natural)=>natural,dialog_header:()=>({start:'cancel',end:'primary'})};
  const host=createDialogHost({app,dialog,header,cancel,primary,icon,copy,onCancel(){}});
  host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:'action'});
  const close=header.children.find(node=>node.dataset.dialogPart==='start');
  assert.notEqual(close,cancel);assert.deepEqual(header.children,[close,primary]);assert.deepEqual(footer.children,[cancel]);assert.equal(cancel.hidden,true);assert.equal(cancel.firstChild,bound);
  page=false;host.layout();
  assert.deepEqual(footer.children,[cancel,primary]);assert.deepEqual(header.children,[close]);assert.equal(close.hidden,true);assert.equal(cancel.hidden,false);
  copy.cancel='Abbrechen';refreshBindings();
  assert.equal(bound.nodeValue,'Abbrechen');
  assert.equal(cancel.textContent,'Abbrechen','The original bound text remains visible after returning to floating');
  assert.equal(bound.parentNode.hidden,false);assert.equal(cancel.contains(bound),true);
  host.dispose();
});

test('phone contacts preview locally, commit once and retire on cancellation',t=>{
  const {element,icon,doc}=colorDialogDom(t),listeners=new Map(),timers=new Map(),sent=[],inputs=[],freed=[];
  const tiles=[{id:8,control:{kind:'brush'}},{id:9,control:{kind:'eraser'}}];
  const bounds={x:0,y:0,width:32,height:32},geometry={edge:'top',bars:[bounds,bounds],chips:[bounds,bounds],tools:[bounds,{...bounds,x:40}],tool_hits:[bounds,{...bounds,x:40}],
    capy_target:bounds,capy:bounds,capy_hit:bounds,settings_hit:bounds,settings:bounds,size:bounds,strength:bounds,icon:16,capy_icon:24};
  const model={workspace_ready:true,phone:{insets:[0,0,0,0]},workspace:{layout:{phone:tiles}}};
  const view={enabled:true,label:'Tool',icon:'tool',selected:false};
  let next=0,valid=true,outcome={kind:'pending'},capture;
  const app={phone_tools:()=>[view,view],phone_settings:()=>view,toolbar_component:()=>({}),header_view:()=>({capy:view,sizes:[{id:'medium',tile:32}]}),capy_caption:()=>'',
    touch_hold_ms:()=>500,phone_bar_contact:()=>{const id=++next;return{id,free:()=>freed.push(id)};},phone_bar_valid:()=>valid,
    phone_bar_input:(bar,input)=>{inputs.push([bar.id,input.kind]);return{outcome,geometry:geometry.tools,lifted:outcome.kind==='held'||outcome.kind==='dragging'?0n:null};}};
  const workspace=element('div'),capy=element('button');doc.body.append(workspace);workspace.append(capy);
  const createElement=(...args)=>{const n=element(...args);n.style.setProperty=()=>{};n.querySelector=()=>null;n.hasPointerCapture=id=>capture===id;n.setPointerCapture=id=>capture=id;n.releasePointerCapture=()=>capture=null;return n;};
  const context={window:{addEventListener:(type,listener)=>{const list=listeners.get(type)??[];list.push(listener);listeners.set(type,list);}},document:doc,
    ResizeObserver:class{observe(){}},setTimeout:fn=>{const id=timers.size+1;timers.set(id,fn);return id;},clearTimeout:id=>timers.delete(id),
    browserDragThreshold:8,createToolbarComponent:()=>Object.assign(createElement('div'),{layoutComponent(){},closeComponent(){},updateComponent(){}})};
  const source=readFileSync(new URL('phone.js',import.meta.url),'utf8').replace(/^import .*;\n/gm,'').replace('export function createPhone','function createPhone');
  runInNewContext(source,context);
  const host=context.createPhone({app,state:()=>model,workspace,capy,header:{phone(){}},element:createElement,button:()=>createElement('button'),icon,
    customization:{target(){},variationMarker:()=>element('span'),showContext:()=>sent.push('menu')},dispatch:action=>sent.push(action),place(){},changed(){}});
  host.layout({phone:geometry});
  const root=workspace.children.find(n=>n.id==='phone-frame'),tool=doc.body.descendants().find(n=>n.id==='phone-tool-8');
  const event=(type,x=2,y=2)=>{for(const fn of listeners.get(type)??[])fn({target:tool,button:0,isPrimary:true,pointerId:1,clientX:x,clientY:y,preventDefault(){},stopPropagation(){}});};
  const arm=()=>{outcome={kind:'held'};for(const fn of [...timers.values()])fn();};
  event('pointerdown');arm();assert.equal(root.dataset.contact,'held');
  assert.equal(tool.parentNode.classList.contains('header-drag-preview'),true,'A Wasm usize index marks the lifted control');
  outcome={kind:'dragging'};event('pointermove',42);assert.equal(sent.length,0);assert.equal(root.dataset.contact,'dragging');
  outcome={kind:'move',value:{tile:8,before:null}};event('pointerup',42);event('pointerup',42);
  assert.equal(sent.length,1);assert.equal(sent[0].action.kind,'move_tool');assert.deepEqual(freed,[1]);
  for(const cancellation of ['pointercancel','blur','resize']){
    event('pointerdown');arm();
    if(cancellation==='resize'){valid=false;host.refresh();valid=true;}
    else event(cancellation);
    event('pointerup');assert.equal(sent.length,1,'Cancelled contacts cannot commit');
  }
  assert.deepEqual(freed,[1,2,3,4]);assert.equal(root.dataset.contact,undefined);
  assert.equal(inputs.filter(([,kind])=>kind==='finish').length,1,'Only a live contact reaches shared Finish');
});

test('an Editor role uses its Alert frame only when floating and forwards prefer-wide',async t=>{
  const {createDialogHost}=await import('./dialog-host.js'),{doc,element,icon}=colorDialogDom(t);
  const dialog=element('dialog'),header=element('header'),footer=element('footer'),cancel=element('button'),primary=element('button');
  header.append(element('h2','','Assign Profile'));footer.append(cancel,primary);dialog.append(header,footer);doc.body.append(dialog);
  let page=false;const roles=[],calls=[];
  const app={dialog_presentation:role=>{roles.push(role);return page?{kind:'page',bounds:{x:0,y:0,width:412,height:891}}:{kind:'floating'};},
    dialog_header:()=>({start:'cancel',end:'primary'}),alert_dialog_short:()=>false,
    alert_dialog_layout:(_viewport,_metrics,_font,preferWide)=>{calls.push(preferWide);return{width:372,short:false,stacked:false};}};
  const host=createDialogHost({app,dialog,header,cancel,primary,icon,copy:{cancel:'Cancel'},onCancel(){}});
  host.present({frame:{kind:'alert',prefer_wide:true},role:'editor',modal:true,primary:'action'});
  assert.equal(dialog.getAttribute('role'),'dialog');assert.equal(dialog.dataset.dialogFrame,'alert');assert.equal(dialog.style.width,'372px');
  assert.equal(header.dataset.dialogPart,undefined);assert.deepEqual(calls,[true]);
  page=true;host.layout();assert.equal(dialog.dataset.dialogFrame,'editor');assert.equal(header.dataset.dialogPart,'header');
  assert.equal(primary.parentNode,header);assert.equal(dialog.style.width,'412px');assert.deepEqual(calls,[true]);assert.deepEqual(roles,['editor','editor']);
  page=false;host.layout();assert.equal(primary.parentNode,footer);assert.equal(dialog.dataset.dialogFrame,'alert');assert.deepEqual(calls,[true,true]);
  host.hide();host.dispose();
});

test('a declared body Done remains alongside its projected page Done',async t=>{
  const {createDialogHost}=await import('./dialog-host.js'),{doc,element,icon}=colorDialogDom(t);
  const dialog=element('dialog'),header=element('header'),body=element('form'),primary=element('button','','Done');
  header.append(element('h2','','Manage Color Profiles'));body.append(primary);dialog.append(header,body);doc.body.append(dialog);
  const app={dialog_presentation:()=>({kind:'page',bounds:{x:0,y:0,width:412,height:891}}),dialog_header:()=>({start:null,end:'close'})};
  const host=createDialogHost({app,dialog,header,cancel:primary,primary,retainPrimary:true,icon,copy:{done:'Done'},onCancel(){}});
  host.present({frame:{kind:'editor'},role:'editor',modal:true,primary:'close'});
  assert.equal(primary.parentNode,body);assert.equal(primary.hidden,false);assert.equal(header.children.filter(node=>node.textContent==='Done'&&!node.hidden).length,1);
  host.hide();host.dispose();
});
