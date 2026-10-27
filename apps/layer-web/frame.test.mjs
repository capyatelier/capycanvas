import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";
import {headerButtonHint, presentCapy} from './header.js';
import {colorDialogDom} from './color-dialog-fixture.mjs';

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");
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
    app:undefined,gpuReady:false,compilerResumeTimer:undefined,pending:[],documents:{busy:()=>false},
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
  const context={documents:{interactionBusy:()=>busy},app:{dispatch(action){received.push(action);return change;}},
    applyChange:value=>published.push(value),message:assert.fail};
  runInNewContext(source.slice(source.indexOf('function dispatch(action)'),source.indexOf('let languageTimer;')),context);
  const restore={type:'restore_saved_settings',saved:JSON.stringify({language:{Explicit:'ja'}})};
  context.dispatch(restore);
  assert.deepEqual(received,[restore]);assert.deepEqual(published,[change]);
  const invoke={type:'invoke',command:'pen'};
  context.dispatch(invoke);
  assert.deepEqual(received,[restore],'the open dialog keeps ownership of editing commands');
  busy=false;context.dispatch(invoke);
  assert.deepEqual(received,[restore,invoke]);assert.deepEqual(published,[change,change]);
});

test('nested dialogs own keys after recording even without a focused descendant', () => {
  const received = [];
  let modal = {};
  class Element { closest() { return null; } }
  const context = {Element, document:{querySelector:() => modal}, state:{preferences:{capture:null}},
    composingKey:() => false, updateZen() {}, keyboardFocused:new WeakSet(),
    app:{control_takes_key:() => false}, documents:{allowNativePaste:() => false},
    input:event => { received.push(event); return {handled:true}; }};
  runInNewContext(source.slice(source.indexOf('function keyInput('), source.indexOf('const keyboardFocused =')), context);
  const key = pressed => {
    const event = {target:new Element(), key:'Escape', defaultPrevented:false, stopped:false,
      preventDefault() { this.defaultPrevented=true; }, stopPropagation() { this.stopped=true; }};
    context.keyInput(event, pressed);
    return event;
  };
  assert.equal(key(true).defaultPrevented, false);
  assert.equal(received.length, 0, 'The visible sheet owns Escape when focus has been blurred');
  context.state.preferences.capture = {};
  assert.equal(key(true).defaultPrevented, true);
  assert.equal(received.length, 1, 'Recording still receives and consumes the chord');
  context.state.preferences.capture = null;
  key(false);
  assert.equal(received.at(-1).pressed, false, 'Release retires the shared recording key');
  assert.equal(key(true).defaultPrevented, false);
  assert.equal(received.length, 2, 'The next Escape returns to native dismissal');
  modal = null;
  assert.equal(key(true).defaultPrevented, true);
  assert.equal(received.length, 3, 'Main Preferences retains shared key handling');
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
