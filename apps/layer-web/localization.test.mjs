import assert from 'node:assert/strict';
import {test} from 'node:test';
import {bindCopy,liveCopy,refreshCopy} from './localization.js';

test('publishes retained semantic copy and bindings together',()=>{
  let value={title:'Settings',actions:[{label:'Cancel'}],optional:'old'};
  const app={catalog:()=>structuredClone(value)},copy=liveCopy(app,'catalog');
  const actions=copy.actions,action=actions[0],node={};
  bindCopy(node,()=>action.label);
  value={title:'設定',actions:[{label:'キャンセル'}]};
  refreshCopy(app);
  assert.equal(liveCopy(app,'catalog'),copy);
  assert.equal(copy.actions,actions);
  assert.equal(copy.actions[0],action);
  assert.equal(node.textContent,'キャンセル');
  assert.equal(copy.title,'設定');
  assert.equal(Object.hasOwn(copy,'optional'),false);
});

test('removes superseded array entries and replaces an existing binding',()=>{
  let value={choices:['One','Two']};
  const app={catalog:()=>structuredClone(value)},copy=liveCopy(app,'catalog'),node={};
  bindCopy(node,()=>copy.choices[1]);
  bindCopy(node,()=>copy.choices[0]);
  value={choices:['一']};refreshCopy(app);
  assert.deepEqual(copy.choices,['一']);
  assert.equal(node.textContent,'一');
});
