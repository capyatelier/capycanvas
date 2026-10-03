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


test('relabels an owned text node without detaching native input children',()=>{
  let value={label:'Width'};
  const app={catalog:()=>structuredClone(value)},copy=liveCopy(app,'catalog');
  const input={value:'１２+invalid',selectionStart:1,selectionEnd:4};
  const label={children:[input],ownerDocument:{createTextNode:nodeValue=>({nodeType:3,nodeValue})},get firstChild(){return this.children[0];},insertBefore(node,before){this.children.splice(this.children.indexOf(before),0,node);return node;},set textContent(value){this.children=[{nodeType:3,nodeValue:value}];}};
  assert.equal(bindCopy(label,()=>copy.label),label);
  const text=label.firstChild;
  value={label:'Largeur'};refreshCopy(app);
  assert.deepEqual(label.children,[text,input]);
  assert.equal(text.nodeValue,'Largeur');
  assert.deepEqual(input,{value:'１２+invalid',selectionStart:1,selectionEnd:4});
  bindCopy(label,()=>copy.label+' (px)');refreshCopy(app);
  assert.deepEqual(label.children,[text,input]);assert.equal(text.nodeValue,'Largeur (px)');
});
