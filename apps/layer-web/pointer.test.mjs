import {nativeScreenEvents} from './native-input-fixture.mjs';
import {observePenInput} from './pen-input.js';
import {observeButtonFeedback} from './button-feedback.js';
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");

test('button contacts clear on cancellation, removal, disabled state and pointer takeover', () => {
  const listeners = new Map(); let mutation;
  const root = {document:{documentElement:{}}, addEventListener(type, handler) { listeners.set(type, handler); },
    MutationObserver:class { constructor(callback) { mutation = callback; } observe() {} disconnect() {} }};
  const attributes = new Set(), parent = {};
  const button = {isConnected:true, parentNode:parent, disabled:false, hidden:false,
    matches() { return this.disabled; }, closest(selector) { return selector === 'button,summary' ? this : this.hidden ? {} : null; },
    setAttribute(name) { attributes.add(name); }, removeAttribute(name) { attributes.delete(name); },
    toggleAttribute(name, on) { if (on) attributes.add(name); else attributes.delete(name); },
    getBoundingClientRect() { return {left:10, top:10, right:50, bottom:50}; }};
  observeButtonFeedback(root);
  const emit = (type, options = {}) => listeners.get(type)({target:button, pointerId:1, pointerType:'touch', button:0, isPrimary:true, clientX:20, clientY:20, ...options});
  const held = () => attributes.has('data-pressed');
  for (const pointerType of ['mouse','pen','touch']) {
    emit('pointerdown',{pointerType}); assert.ok(held());
    emit('pointermove',{clientX:100}); assert.ok(!held());
    emit('pointermove'); assert.ok(held());
    emit('pointerup',{pointerId:2}); assert.ok(held());
    emit('pointercancel'); assert.ok(!held());
  }
  for (const event of ['pointerup','lostpointercapture','blur','resize','scroll','dragstart']) {
    emit('pointerdown'); assert.ok(held()); emit(event); assert.ok(!held(), event);
  }
  emit('pointerdown'); emit('pointerdown',{pointerId:2,isPrimary:false}); assert.ok(!held());
  emit('pointerdown'); emit('keydown',{key:'Escape'}); assert.ok(!held());
  for (const property of ['disabled','hidden']) {
    emit('pointerdown'); button[property] = true; mutation(); assert.ok(!held(), property);
    emit('pointerdown'); assert.ok(!held(), property); button[property] = false;
  }
  emit('pointerdown'); button.parentNode = {}; mutation(); assert.ok(!held());
  emit('pointerdown'); button.isConnected = false; mutation(); assert.ok(!held());
});

function penObserver() {
  const listeners=new Map(),actions=[],canvas={};let owner=0;
  observePenInput({canvas,workspace:{addEventListener(type,listener,options){assert.equal(options.capture,true);listeners.set(type,listener);}},dispatch:action=>actions.push(action),position:e=>[e.clientX*2,e.clientY*2],owner:()=>owner});
  return {canvas,actions,activate(){owner++;},send(type,event={}){listeners.get(type)({pointerType:'pen',pointerId:-7,target:canvas,clientX:20,clientY:30,pressure:.6,tiltX:30,tiltY:-15,twist:45,timeStamp:12,buttons:0,...event});}};
}

test('pen observer registers canvas pointers without duplicating their native sample stream',()=>{
  const h=penObserver();
  h.send('pointermove',{pointerType:'mouse'});h.send('pointermove',{pointerType:'touch'});
  h.send('pointermove',{target:{closest:()=>null}});assert.equal(h.actions.length,0);
  h.send('pointermove',{persistentDeviceId:45});h.send('pointerdown');h.send('pointerup');assert.equal(h.actions.length,1);
  assert.equal(h.actions[0].type,'pen_input');assert.equal(h.actions[0].sample,null);
  assert.deepEqual(h.actions[0].device,{id:BigInt(-7>>>0),key:'web:pen',label:'',native_to_surface:{reverse_horizontal:false,reverse_vertical:false,swap:false}});
  h.send('pointermove',{pointerId:17,persistentDeviceId:998});assert.equal(h.actions.length,2);
  assert.equal(h.actions[1].device.key,h.actions[0].device.key,'Browser pointer and session identifiers do not create orphaned persistent profiles');
});

test('utility pen sensing preserves raw pressure and tilt without dispatching canvas contacts',()=>{
  for(const dialog of ['#pen-pressure-dialog','#pen-tilt-dialog']){
    const h=penObserver(),target={closest:selector=>selector.includes(dialog)?{}:null};
    for(const [type,buttons,phase] of [['pointermove',0,0],['pointerdown',1,1],['pointermove',1,2],['pointerup',0,3],['pointercancel',0,4]]){
      h.send(type,{target,buttons});const action=h.actions.at(-1);
      assert.equal(action.type,'pen_input');assert.equal(action.sample.length,9);
      assert.deepEqual(action.sample.slice(0,3),[40,60,.6]);
      for(const [actual,expected] of action.sample.slice(3,6).map((n,i)=>[n,[Math.PI/6,-Math.PI/12,Math.PI/4][i]]))assert.ok(Math.abs(actual-expected)<1e-12);
      assert.deepEqual(action.sample.slice(6),[0,12000000,phase]);
    }
  }
});

test('the same canvas pen registers again when another drawing activates',()=>{
  const h=penObserver();h.send('pointermove');h.send('pointerdown');assert.equal(h.actions.length,1);
  h.activate();h.send('pointermove');assert.equal(h.actions.length,2);
  assert.deepEqual(h.actions[1],h.actions[0]);
  h.send('pointerdown');assert.equal(h.actions.length,2,'Ordinary samples reuse registration within the activated drawing');
  h.activate();h.send('pointerdown');assert.equal(h.actions.length,3);
});

test("canvas wheel owns browser zoom even when a scroll sequence targets an ancestor", () => {
  const listeners = [], scrolls = [], changes = [];
  const canvas = { width: 1600, clientWidth: 800, clientHeight: 600, addEventListener: (type, handler, options) => listeners.push({target: canvas, type, handler, options}) };
  let hit = canvas;
  const window = {addEventListener: (type, handler, options) => listeners.push({target: window, type, handler, options})};
  const context = {canvas, window, document: {elementFromPoint: () => hit}, position: () => [400, 300],
    app: {scroll: (...args) => { scrolls.push(args); return 'camera'; }}, applyChange: change => changes.push(change), message: assert.fail};
  runInNewContext(source.slice(source.indexOf('canvas.addEventListener("contextmenu"'), source.indexOf('function keyInput(')), context);
  const wheel = listeners.find(listener => listener.type === 'wheel');
  assert.equal(wheel.target, window);
  assert.equal(wheel.options.capture, true);
  assert.equal(wheel.options.passive, false);
  for (const [deltaMode, unit] of [[0, 1], [1, 16], [2, 600]]) {
    let prevented = false;
    wheel.handler({target: {}, clientX: 200, clientY: 150, deltaMode, deltaX: 2, deltaY: -3,
      ctrlKey: true, shiftKey: true, preventDefault() { prevented = true; }});
    assert.equal(prevented, true);
    assert.deepEqual(scrolls.at(-1), [400, 300, 2 * unit, -3 * unit, 2, 3]);
  }
  assert.equal(changes.length, 3);
  hit = {};
  wheel.handler({target: hit, clientX: 200, clientY: 150, ctrlKey: true, preventDefault: () => assert.fail('panel wheel remains native')});
  assert.equal(scrolls.length, 3);
});

test("cursor hover preserves mouse, pen, and eraser device kinds", () => {
  const canvas = {}, samples = [], inputs = [], changes = [];
  const context = {
    gpuReady: true, canvas, lastPenEvent: null, workspaceGesture: null,
    document: { elementFromPoint: () => canvas },
    position: () => [20, 30], wake() {},
    applyChange: change => changes.push(change),
    app: {
      cursor_input: sample => samples.push(Array.from(sample)),
      input: event => (inputs.push(event.type), { change: "cursor-left" }),
    },
  };
  runInNewContext(source.slice(source.indexOf("let canvasCursorActive"), source.indexOf("function wake()")), context);
  for (const [pointerType, buttons, kind] of [["mouse", 0, 1], ["pen", 0, 0], ["pen", 32, 2]]) {
    context.cursorInput({ pointerType, buttons, pointerId: 1, target: canvas, clientX: 20,
      clientY: 30, timeStamp: 12, pressure: .5 });
    assert.equal(samples.at(-1).length, 11);
    assert.equal(samples.at(-1)[9], kind);
  }
  context.cursorInput({ pointerType: "touch" });
  assert.equal(samples.length, 3);
  assert.deepEqual(inputs, ["cursor_leave"]);
  assert.deepEqual(changes, ["cursor-left"]);
});

test("workspace dragging clears the canvas cursor without hit-testing moving panels", () => {
  const canvas = {}, samples = [], inputs = [];
  let hits = 0;
  const context = {
    gpuReady: true, canvas, lastPenEvent: null, workspaceGesture: null,
    document: { elementFromPoint() { hits++; return canvas; } },
    position: () => [20, 30], wake() {}, applyChange() {},
    app: {
      cursor_input: sample => samples.push(Array.from(sample)),
      input: event => (inputs.push(event.type), {change: {}}),
    },
  };
  runInNewContext(source.slice(source.indexOf("let canvasCursorActive"), source.indexOf("function wake()")), context);
  const event = {pointerType: "mouse", pointerId: 1, target: {}, clientX: 20, clientY: 30};
  context.cursorInput(event);
  context.workspaceGesture = {started: true};
  for (let i = 0; i < 60; i++) context.cursorInput(event);
  assert.equal(hits, 1);
  assert.equal(samples.length, 1);
  assert.deepEqual(inputs, ["cursor_leave"]);
  context.workspaceGesture = null;
  context.cursorInput(event);
  assert.equal(hits, 2);
  assert.equal(samples.length, 2);
});
function harness({ raw = false, prediction = false, paint: allowPaint = true } = {}) {
  const listeners = new Map(), windowListeners = new Map(), records = [], phases = [], buttons = [], cursors = [], captures = [], timers = [], attributes = new Set();
  const listen = (target, name, fn) => target.set(name, [...target.get(name) ?? [], fn]);
  const emit = (target, type, event) => {
    let stopped = false;
    const e = {type, ...event, stopImmediatePropagation() { stopped = true; }};
    for (const listener of target.get(type) ?? []) { listener(e); if (stopped) break; }
  };
  let contact = null;
  const context = {
    canvas: {
      width: 800, height: 600,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
      focus() {}, setPointerCapture() {},
      addEventListener: (name, fn) => listen(listeners, name, fn),
    },
    lastPenEvent: null, pending: [], notice: null, state: { camera: { revision: 1 }, settings: { feedback: prediction, platform_prediction: prediction } },
    app: { pen(batch) { records.push(...batch); return batch.length / 11; } },
    input(event) {
      if (event.type === "pen_button") {
        buttons.push(event);
        return { paint: false, handled: true };
      }
      phases.push(event);
      if (event.phase === "down" && contact === null && event.kind !== "touch")
        contact = { id: event.id, paint: allowPaint && event.button === "primary" };
      const paint = contact !== null && contact.id === event.id && contact.paint;
      const handled = contact?.id === event.id;
      if (contact?.id === event.id && ["up", "cancel"].includes(event.phase)) contact = null;
      return { paint, handled };
    },
    cursorInput(event) { cursors.push(event?.type ?? null); }, edgeReply() {}, wake() {},
    heldPointers: new Map(), workspaceGesture: null,
    workspace: {setPointerCapture: id => captures.push(id),classList:{contains:()=>false}},
    document: {documentElement: {hasAttribute: name => attributes.has(name), toggleAttribute: (name, value) => value ? attributes.add(name) : attributes.delete(name)}, addEventListener() {}},
    chromeEvents:[],chromeInput: event => (context.chromeEvents.push(event),{handled: event.kind === 'contact' && !!context.consumeChrome}),
    zenUpdates:0,updateZen() {context.zenUpdates++;}, closeMenusOutside() {}, requestAnimationFrame() {}, endWorkspaceGesture() {}, queuePen() {},
    window: { addEventListener: (name, fn) => listen(windowListeners, name, fn) }, setTimeout: fn => timers.push(fn), clearTimeout() {},
  };
  if (raw) context.onpointerrawupdate = null;
  runInNewContext([
    source.slice(source.indexOf('for (const type of ["pointerdown", "pointermove"])'), source.indexOf("for (const type of ['pointerdown','pointermove'")),
    source.slice(source.indexOf('function pointerStyle('), source.indexOf('canvas.addEventListener("contextmenu"')),
    source.slice(source.indexOf('window.addEventListener("blur", () => {\n  endWorkspaceGesture'), source.indexOf('function tabHits(')),
  ].join('\n'), context);
  let timeStamp = 0;
  return {
    send(type, overrides = {}) {
      emit(listeners, type, { pointerId: -7, pointerType: "pen", button: 0,
        buttons: 1, pressure: 0.6, clientX: 40, clientY: 50,
        timeStamp: ++timeStamp, cancelable: type !== "pointerrawupdate", preventDefault() {}, ...overrides });
    },
    samples: () => Array.from({ length: records.length / 11 }, (_, i) => records.slice(i * 11, i * 11 + 11)),
    phases, buttons, cursors,
    chrome(type, overrides = {}) {
      let prevented = false;
      emit(windowListeners, type, {pointerId: 1, pointerType: 'mouse', buttons: 1, detail: 1,
        target: {closest: selector => selector === '#header' ? {} : null},
        preventDefault() { prevented = true; }, ...overrides});
      return prevented;
    },
    context, captures,flushTimers:()=>timers.splice(0).forEach(fn=>fn()),
  };
}

for (const ending of ["pointerup", "lostpointercapture", "pointermove"]) {
  test(`pen stroke survives ${ending} and finishes exactly once`, () => {
    const h = harness();
    h.send("pointerdown");
    h.send("pointermove", { clientX: 100 });
    h.send(ending, { pointerType: ending === 'lostpointercapture' ? '' : 'pen', clientX: 700, clientY: 500, buttons: 0, pressure: 0 });
    if (ending === 'lostpointercapture') assert.equal(h.cursors.at(-1), null);
    h.send("lostpointercapture", { pointerType: "", buttons: 0 });
    h.send("pointerup", { buttons: 0, pressure: 0 });
    h.send("pointermove", { buttons: 0, pressure: 0 });
    const samples = h.samples();
    assert.deepEqual(samples.map(s => s[1]), [1, 2, 3]);
    assert.equal(samples[2][0], -7 >>> 0);
    if (ending !== "pointerup") {
      assert.deepEqual(samples[2].slice(2, 5), [100, 50, 0.6]);
    }
    h.send("pointerdown");
    h.send("pointerup", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 3]);
  });
}

test("normal mouse and pen release preserves hover after capture ends", () => {
  for (const pointerType of ["mouse", "pen"]) {
    const h = harness();
    h.send("pointerdown", { pointerType });
    h.send("pointerup", { pointerType, pressure: 0, buttons: 0 });
    h.send("lostpointercapture", { pointerType, pressure: 0, buttons: 0 });
    assert.equal(h.cursors.at(-1), "pointerup");
  }
});

test("foreign pointers cannot end the active stroke", () => {
  const h = harness();
  h.send("pointerdown");
  h.send("pointercancel", { pointerId: 9 });
  h.send("lostpointercapture", { pointerId: 9 });
  h.send("pointermove");
  h.send("pointerup");
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});

test("header contacts retire independently and preserve a reused pointer", () => {
  const h = harness();
  for (const ending of ['pointerup', 'pointercancel', 'lostpointercapture', 'pointermove']) {
    h.chrome('pointerdown');
    h.chrome('pointerdown', {pointerId: 2, pointerType: 'touch'});
    assert.equal(h.context.document.documentElement.hasAttribute('data-touch'), true, 'pointer style and header capture both receive the contact');
    assert.deepEqual([...h.context.heldPointers.keys()], [1, 2]);
    h.chrome(ending, {pointerId: 2, buttons: 0});
    if (ending === 'pointerup' || ending === 'lostpointercapture')
      assert.deepEqual([...h.context.heldPointers.keys()], [1, 2], 'release leaves chrome held through native click dispatch');
    h.flushTimers();
    assert.deepEqual([...h.context.heldPointers.keys()], [1], ending);
    h.chrome('pointerup');
    h.flushTimers();
    assert.equal(h.context.heldPointers.size, 0);
  }
  h.chrome('pointerdown');h.chrome('pointerup');h.chrome('lostpointercapture');
  h.chrome('pointerdown');h.flushTimers();
  assert.deepEqual([...h.context.heldPointers.keys()],[1],'old release callbacks cannot clear a reused pointer');
  h.chrome('pointercancel');h.flushTimers();
  const updates=h.context.zenUpdates;
  h.chrome('pointerup');h.chrome('lostpointercapture');h.flushTimers();
  assert.equal(h.context.zenUpdates,updates,'release without an owned contact does not publish chrome');
  h.context.consumeChrome = true;
  for (const pointerId of [1, 2]) h.chrome('pointerdown', {pointerId});
  assert.deepEqual(h.captures, [1, 2], 'consumed contacts capture the stable workspace');
  assert.equal(h.context.heldPointers.size, 0, 'consumed contacts cannot leave a pin');
  assert.equal(h.chrome('click', {pointerId: 1, detail: 0}), false, 'keyboard click is independent');
  h.chrome('pointercancel', {pointerId: 2});
  h.chrome('blur');
  assert.equal(h.context.heldPointers.size, 0);
});

test('floating pen utilities receive contacts above expanded workspace chrome',()=>{
  for(const pointerType of ['mouse','touch','pen']){
    const h=harness();h.context.consumeChrome=true;
    const target={closest:selector=>selector==='.utility-panel'?{}:null};
    assert.equal(h.chrome('pointerdown',{target,pointerType}),false,'Workspace chrome cannot consume a utility control press');
    h.chrome('pointermove',{target,pointerType,buttons:0});
    assert.deepEqual(h.context.chromeEvents,[],'Utility motion never hit-tests the workspace underneath');
    assert.deepEqual(h.captures,[]);assert.equal(h.context.heldPointers.size,0);
    assert.equal(h.context.zenUpdates,0);
  }
});

test('captured workspace motion reaches its owner without refreshing hover chrome',()=>{
  const h=harness();
  h.context.workspaceGesture={id:1,started:true};
  for(let i=0;i<60;i++)assert.equal(h.chrome('pointermove'),false);
  assert.deepEqual(h.context.chromeEvents,[]);
  assert.equal(h.cursors.length,60);
  assert.ok(h.cursors.every(event=>event===null));
  h.chrome('pointermove',{pointerId:2});
  assert.equal(h.context.chromeEvents.length,1,'A foreign pointer retains its own hover routing');
  h.context.workspaceGesture=null;
  h.chrome('pointermove');
  assert.equal(h.context.chromeEvents.length,2,'Ordinary hover resumes after release');
});

test("zero pressure with tip or eraser contact is not a lift", () => {
  for (const buttons of [1, 32]) {
    const h = harness();
    h.send("pointerdown", { buttons });
    h.send("pointermove", { buttons, pressure: 0 });
    h.send("pointerup", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
  }
});

test("mouse cancellation and touch/pan routing retain their semantics", () => {
  const mouse = harness();
  mouse.send("pointerdown", { pointerType: "mouse" });
  mouse.send("pointercancel", { pointerType: "mouse" });
  assert.deepEqual(mouse.samples().map(s => s[1]), [1, 4]);
  for (const overrides of [{ pointerType: "touch" }, { pointerType: "mouse", button: 2, buttons: 2 }]) {
    const h = harness();
    h.send("pointerdown", overrides);
    h.send("pointercancel", overrides);
    assert.deepEqual(h.samples(), []);
    assert.equal(h.phases.at(-1).phase, "cancel");
  }
});

for (const raw of [false, true]) {
  for (const [button, mask] of [[1, 4], [2, 2]]) {
    test(`pen side button ${button} preserves the contact (raw=${raw})`, () => {
      const h = harness({ raw });
      const move = (overrides) => {
        if (raw) h.send("pointerrawupdate", overrides);
        h.send("pointermove", overrides);
      };
      h.send("pointerdown");
      move({ button, buttons: 1 | mask, clientX: 80 });
      move({ button: -1, buttons: 1 | mask, clientX: 100 });
      move({ button, buttons: 1, clientX: 120 });
      h.send("pointerup", { buttons: 0, pressure: 0 });
      assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 2, 2, 3]);
      assert.ok(h.phases.every(p => p.button === "primary"));
      assert.deepEqual(h.buttons.map(p => [p.button, p.pressed]),
        [[mask === 2 ? "primary" : "secondary", true], [mask === 2 ? "primary" : "secondary", false]]);
    });

    test(`pen tip can draw and lift while side button ${button} stays held (raw=${raw})`, () => {
      const h = harness({ raw });
      h.send("pointerdown", { button, buttons: mask, pressure: 0 });
      assert.equal(h.phases.length, 0, "hover button must not start navigation");
      assert.deepEqual(h.buttons.map(p => [p.button, p.pressed]),
        [[mask === 2 ? "primary" : "secondary", true]]);
      for (const tip of [1, 32]) {
        h.send("pointermove", { button: tip === 1 ? 0 : 5, buttons: tip | mask });
        if (raw) h.send("pointerrawupdate", { button: -1, buttons: tip | mask, clientX: 100 });
        h.send("pointermove", { button: -1, buttons: tip | mask, clientX: 100 });
        if (raw) h.send("pointerrawupdate", { buttons: mask, pressure: 0 });
        h.send("pointermove", { button: tip === 1 ? 0 : 5, buttons: mask, pressure: 0 });
      }
      h.send("pointerup", { button, buttons: 0, pressure: 0 });
      assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 2, 3]);
      assert.deepEqual(h.samples().map(s => s[10]), [0, 0, 0, 2, 2, 2]);
      assert.deepEqual(h.buttons.map(p => p.pressed), [true, false]);
    });
  }
}

test("tip lift wins over fallback pressure while a barrel button stays active", () => {
  const h = harness();
  h.send("pointerdown", { buttons: 3 });
  h.send("pointermove", { button: -1, buttons: 3, clientX: 100 });
  h.send("pointermove", { button: 0, buttons: 2, pressure: .5 });
  h.send("pointermove", { button: -1, buttons: 2, pressure: .5 });
  h.send("pointerup", { button: 2, buttons: 0, pressure: 0 });
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});

test("pen navigation also follows tip boundaries with a held barrel button", () => {
  const h = harness({ paint: false });
  h.send("pointerdown", { button: 2, buttons: 2, pressure: 0 });
  h.send("pointermove", { button: 0, buttons: 3 });
  h.send("pointermove", { button: -1, buttons: 3 });
  h.send("pointermove", { button: 0, buttons: 2, pressure: 0 });
  assert.deepEqual(h.phases.map(p => p.phase), ["down", "move", "up"]);
  assert.deepEqual(h.samples(), []);
});

for (const ending of ["pointerup", "lostpointercapture", "pointerrawupdate", "pointermove"]) {
  test(`raw pen contact is not duplicated and survives ${ending}`, () => {
    const h = harness({ raw: true });
    h.send("pointerdown");
    h.send("pointerrawupdate", { clientX: 100 });
    h.send("pointermove", { clientX: 100 });
    h.send(ending, { clientX: 700, clientY: 500, buttons: 0, pressure: 0 });
    h.send("lostpointercapture", { buttons: 0, pressure: 0 });
    h.send("pointermove", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
    h.send("pointerdown");
    h.send("pointermove", { clientX: 200 });
    h.send("pointerup");
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 2, 3]);
  });
}
test("raw coalesced samples arrive once, with later native predictions from pointermove", () => {
  const h = harness({ raw: true, prediction: true });
  const sample = (x, timeStamp) => ({ pointerId: -7, pointerType: "pen", buttons: 1,
    clientX: x, clientY: 50, pressure: .6, timeStamp });
  h.send("pointerdown");
  const history = [sample(80, 2), sample(100, 3)];
  h.send("pointerrawupdate", { timeStamp: 3, clientX: 100, getCoalescedEvents: () => history });
  h.send("pointermove", { timeStamp: 3, clientX: 100, getCoalescedEvents: () => history,
    getPredictedEvents: () => [sample(90, 2), sample(120, 4)] });
  h.send("pointerup");
  const records = h.samples();
  assert.deepEqual(records.map(s => s[9]), [2, 2, 2, 3, 2]);
  assert.deepEqual(records.slice(1, 4).map(s => s[2]), [80, 100, 120]);
});
test("raw hover, foreign pointers, touch and mouse do not enter the pen stream", () => {
  const h = harness({ raw: true });
  h.send("pointerrawupdate");
  h.send("pointerdown");
  h.send("pointerrawupdate", { pointerId: 9 });
  h.send("pointerrawupdate", { pointerType: "touch" });
  h.send("pointerrawupdate", { pointerType: "mouse" });
  h.send("pointermove");
  h.send("pointerup");
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});

for (const raw of [false, true]) test(`explicit pen cancellation stays Cancel (raw=${raw})`, () => {
  const h = harness({raw});
  h.send('pointerdown'); h.send(raw ? 'pointerrawupdate' : 'pointermove', {clientX: 100});
  h.send('pointercancel', {clientX: 700, buttons: 0, pressure: 0});
  h.send('lostpointercapture', {pointerType: '', buttons: 0});
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 4]);
  assert.equal(h.phases.at(-1).phase, 'cancel');
  h.send('pointerdown');h.send('pointerup');
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 4, 1, 3]);
});

test('native browser fixtures map content contacts past browser chrome and preserve key-only records',()=>{
  const events=[{point:[1555.45,116],down:true},{touch:'move',point:[100,200]},{key:32,down:true},{down:false}];
  const geometry={screenX:0,screenY:0,outerWidth:1600,outerHeight:1000,innerWidth:1600,innerHeight:913};
  assert.deepEqual(nativeScreenEvents(events,geometry),[{point:[1555.45,203],down:true},{touch:'move',point:[100,287]},{key:32,down:true},{down:false}]);
  assert.deepEqual(events[0].point,[1555.45,116],'native transport leaves DOM measurement unchanged');
  assert.deepEqual(nativeScreenEvents(events,{...geometry,innerHeight:1000}),events,'fullscreen content needs no offset');
});
