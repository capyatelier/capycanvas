import assert from 'node:assert/strict';
import {test} from 'node:test';
import {captureTextComposition, composingKey} from './text-input.js';
import {colorDialogDom, fakeColorApp} from './color-dialog-fixture.mjs';
import {chooseColor} from './color-editor.js';
import {createPreviewPanel} from './preview-panel.js';

function editor(field = {}, releaseKey, receiver = field) {
  const root = new EventTarget(), dialog = {};
  root.activeElement = field;
  captureTextComposition(root, releaseKey);
  for (const type of ['keydown', 'keyup']) root.addEventListener(type, event => receiver.dispatchEvent?.(event));
  function send(type, properties = {}, target = field) {
    const event = new Event(type, {cancelable:true});
    for (const [key, value] of Object.entries({target, ...properties}))
      Object.defineProperty(event, key, {value});
    root.dispatchEvent(event);
    return event;
  }
  return {root, field, dialog, send};
}

for (const key of ['Enter', 'Escape', 'ArrowDown']) {
  test(`${key} belongs to composition until its native release`, () => {
    const {send} = editor();
    send('compositionstart', {timeStamp:10});
    assert.ok(composingKey(send('keydown', {key:'Process', code:key, keyCode:229, timeStamp:20})));
    send('compositionend', {timeStamp:22});
    assert.ok(composingKey(send('keydown', {key, code:key, timeStamp:20})));
    assert.ok(composingKey(send('keydown', {key, code:key, repeat:true, timeStamp:25})));
    assert.ok(composingKey(send('keyup', {key, code:key, timeStamp:30})));
    assert.ok(!composingKey(send('keydown', {key, code:key, timeStamp:40})));
  });
}

test('a delayed native key retains composition ownership without a Process event', () => {
  const {send} = editor();
  send('compositionstart', {timeStamp:10});
  send('compositionend', {timeStamp:22});
  assert.ok(composingKey(send('keydown', {key:'Enter', code:'Enter', timeStamp:20})));
  send('keyup', {key:'Enter', code:'Enter', timeStamp:30});
  assert.ok(!composingKey(send('keydown', {key:'Enter', code:'Enter', timeStamp:40})));
});

test('candidate Escape suppresses the dialog default cancellation until release', () => {
  const {send, dialog, field} = editor();
  send('compositionstart', {timeStamp:10});
  send('compositionend', {timeStamp:22});
  send('keydown', {key:'Escape', code:'Escape', timeStamp:20});
  assert.ok(composingKey({target:field}));
  assert.ok(send('cancel', {timeStamp:25}, dialog).defaultPrevented);
  send('keyup', {key:'Escape', code:'Escape', timeStamp:30});
  assert.ok(!composingKey({target:field}));
  assert.ok(!send('cancel', {timeStamp:45}, dialog).defaultPrevented);
});

test('pointer confirmation does not consume the next ordinary key', () => {
  const {send} = editor();
  send('compositionstart', {timeStamp:10});
  send('compositionend', {timeStamp:20});
  assert.ok(!composingKey(send('keydown', {key:'Enter', code:'Enter', timeStamp:30})));
});

test('focus changes discard keys whose release belongs to a different editor', () => {
  const {send, root, field} = editor();
  send('compositionstart', {timeStamp:10});
  send('keydown', {key:'Enter', code:'Enter', timeStamp:20});
  send('focusout', {timeStamp:30});
  root.activeElement = {};
  assert.ok(!composingKey({target:root.activeElement}));
  root.activeElement = field;
  assert.ok(!composingKey(send('keydown', {key:'Enter', code:'Enter', timeStamp:40})));
});

test('an IME-consumed Process key needs no DOM keyup before ordinary Enter', () => {
  const {send, field} = editor();
  send('compositionstart', {timeStamp:10});
  send('keydown', {key:'Process', code:'Enter', keyCode:229, timeStamp:20});
  send('compositionend', {timeStamp:22});
  assert.ok(!composingKey({target:field}));
  assert.ok(!composingKey(send('keydown', {key:'Enter', code:'Enter', repeat:false, timeStamp:40})));
});

test('a new native press or pointer action clears an unreported key release', () => {
  const {send, field} = editor();
  send('compositionstart', {timeStamp:10});
  send('compositionend', {timeStamp:22});
  send('keydown', {key:'Enter', code:'Enter', timeStamp:20});
  assert.ok(!composingKey(send('keydown', {key:'Enter', code:'Enter', repeat:false, timeStamp:40})));
  send('compositionstart', {timeStamp:50});
  send('keydown', {key:'Enter', code:'Enter', timeStamp:60});
  send('compositionend', {timeStamp:62});
  send('pointerdown', {timeStamp:70});
  assert.ok(!composingKey({target:field}));
});

for (const key of ['Enter', 'Escape', 'ArrowDown']) test(`new fields inherit candidate ${key} protection without cancelling native input`, () => {
  const {root, send} = editor();
  let actions = 0;
  root.addEventListener('keydown', () => actions++);
  root.addEventListener('keyup', () => actions++);
  send('compositionstart', {timeStamp:10});
  assert.equal(send('keydown', {key, code:key, timeStamp:20}).defaultPrevented, false);
  send('compositionend', {timeStamp:22});
  send('keydown', {key, code:key, repeat:true, timeStamp:25});
  send('keyup', {key, code:key, timeStamp:30});
  assert.equal(actions, 0);
  send('keydown', {key, code:key, timeStamp:40});
  assert.equal(actions, 1);
});

for (const blur of [false, true]) test(`shared holds receive owned releases before field handlers (blur=${blur})`, () => {
  let releases = 0;
  const {root, field, send} = editor({}, event => {
    releases++;
    if (blur) send('focusout');
    assert.ok(composingKey(event));
  });
  let ordinary = 0;
  root.addEventListener('keyup', () => ordinary++);
  send('compositionstart', {timeStamp:10});
  send('keydown', {key:' ', code:'Space', timeStamp:20});
  send('compositionend', {timeStamp:22});
  assert.equal(send('keyup', {key:' ', code:'Space', timeStamp:30}).defaultPrevented, false);
  assert.equal(releases, 1);
  assert.equal(ordinary, 0);
  assert.ok(!composingKey({target:field, timeStamp:40}));
  send('keyup', {key:' ', code:'Space', timeStamp:40});
  assert.equal(releases, 1);
  assert.equal(ordinary, 1);
});

for (const key of ['Enter', 'Escape']) for (const keyCode of [0, 229]) {
  test(`color ${key}/${keyCode} leaves candidate text with its native editor`, async t => {
    const h = colorDialogDom(t), {app, stats} = fakeColorApp();
    const result = chooseColor({app, slot:'foreground', element:h.element, button:h.button});
    const dialog = h.doc.body.children.find(node => node.tagName === 'DIALOG'), nodes = dialog.descendants();
    try {
      const field = nodes.find(node => node.tagName === 'INPUT' && node.dataset.colorValue === '0-0');
      nodes.find(node => node.tagName === 'BUTTON' && node.dataset.colorValue === '0-0').click();
      field.value = '120';
      const {send} = editor(field), before = stats.actions.length;
      if (!keyCode) send('compositionstart', {timeStamp:10});
      send('keydown', {key, code:key, keyCode, isComposing:false, timeStamp:20});
      assert.equal(stats.actions.length, before);
      assert.equal(field.hidden, false);
      if (!keyCode) {
        send('compositionend', {timeStamp:22});
        send('keydown', {key, code:key, repeat:true, timeStamp:25});
        assert.equal(field.hidden, false);
      }
      send('keyup', {key, code:key, timeStamp:30});
      send('keydown', {key, code:key, timeStamp:40});
      assert.equal(field.hidden, true);
      assert.equal(stats.actions.length, before + (key === 'Enter' ? 1 : 0));
    } finally { dialog.close(); await result; }
  });

  test(`preview ${key}/${keyCode} waits for the ordinary press`, t => {
    const h = colorDialogDom(t), app = {bootstrap_view:() => ({common:{apply:'Apply', cancel:'Cancel'}})};
    const element = (...args) => {
      const node = h.element(...args);
      node.after = other => node.parentNode.insertBefore(other, node.parentNode.children[node.parentNode.children.indexOf(node) + 1]);
      return node;
    };
    let applied = 0, cancelled = 0;
    const panel = createPreviewPanel({app, name:'candidate', view:() => ({title:'Refine', numeric:{kind:'number'}, label:'Radius', radius:1}),
      kind:() => 'grow', value() {}, apply:() => applied++, cancel:() => cancelled++, element, button:h.button,
      numberField:() => Object.assign(element('div'), {update() {}}), workspace:h.doc.body, layout:() => null, bar:() => null});
    panel.refresh();
    const node = h.doc.body.children.find(node => node.id === 'candidate-panel'), field = element('input');
    node.append(field);
    const {send} = editor(field, undefined, node);
    if (!keyCode) send('compositionstart', {timeStamp:10});
    send('keydown', {key, code:key, keyCode, isComposing:false, timeStamp:20});
    assert.deepEqual([applied, cancelled], [0, 0]);
    if (!keyCode) {
      send('compositionend', {timeStamp:22});
      send('keydown', {key, code:key, repeat:true, timeStamp:25});
      assert.deepEqual([applied, cancelled], [0, 0]);
    }
    send('keyup', {key, code:key, timeStamp:30});
    send('keydown', {key, code:key, timeStamp:40});
    assert.deepEqual([applied, cancelled], key === 'Enter' ? [1, 0] : [0, 1]);
  });
}
