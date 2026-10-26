import assert from 'node:assert/strict';
import test from 'node:test';

const listeners=new Map();
globalThis.window={addEventListener:(type,f)=>listeners.set(type,f),removeEventListener:type=>listeners.delete(type)};
globalThis.devicePixelRatio=2;
const {colorTouchInput}=await import('./color-touch.js');

function harness(claimAt=2) {
  const inputs=[],changes=[];let active=0;
  const app={input(input){inputs.push(input);if(input.phase==='down')active++;if(['up','cancel'].includes(input.phase))active--;return {handled:active>=claimAt,change:{regions:1}};}};
  const touches=colorTouchInput({app,applyChange:change=>changes.push(change),wake(){}});
  const event=(type,pointerId,x=10,pointerType='touch')=>({type,pointerId,pointerType,clientX:x,clientY:5,timeStamp:1.5});
  return {inputs,changes,touches,event};
}

test('touches that start inside a wheel are forwarded with physical positions and nanosecond times',()=>{
  const h=harness();
  assert.equal(h.touches.route(h.event('pointerdown',1,10,'mouse'),true),false);
  assert.equal(h.touches.route(h.event('pointerdown',2),false),false);
  assert.equal(h.touches.route(h.event('pointermove',2),true),false,'a touch that started outside stays native');
  assert.equal(h.inputs.length,0);
  assert.equal(h.touches.route(h.event('pointerdown',3),true),false);
  assert.deepEqual(h.inputs[0],{type:'color_touch',surface:h.inputs[0].surface,id:3,phase:'down',position:[20,10],time_ns:1500000});
  assert.equal(h.changes.length,1);
});

test('a claimed gesture keeps every contact until the last one lifts',()=>{
  const h=harness();
  h.touches.route(h.event('pointerdown',1),true);
  assert.equal(h.touches.route(h.event('pointerdown',2,40),true),true);
  assert.equal(h.touches.route(h.event('pointerup',1),false),true,'lifting one finger stays claimed');
  assert.equal(h.touches.route(h.event('pointerup',2,40),false),true);
  assert.equal(h.touches.route(h.event('pointerdown',4),true),false,'the next gesture starts unclaimed');
});

test('window blur and disposal cancel forwarded contacts',()=>{
  const h=harness();
  h.touches.route(h.event('pointerdown',1),true);
  listeners.get('blur')();
  assert.deepEqual(h.inputs.at(-1).phase,'cancel');
  assert.equal(h.touches.route(h.event('pointermove',1),true),false);
  h.touches.route(h.event('pointerdown',5),true);
  h.touches.dispose();
  assert.equal(h.inputs.at(-1).id,5);
  assert.equal(h.inputs.at(-1).phase,'cancel');
});
