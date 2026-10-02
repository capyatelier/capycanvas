import assert from 'node:assert/strict';
import {test} from 'node:test';
import {captureTextComposition, composingKey} from './text-input.js';

function editor() {
  const root = new EventTarget(), field = {}, dialog = {};
  root.activeElement = field;
  captureTextComposition(root);
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
