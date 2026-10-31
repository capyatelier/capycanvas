import assert from 'node:assert/strict';
import test from 'node:test';
import {createDropdown} from './dropdown.js';
import {FakeElement} from './fake-dom.mjs';
import {refreshBindings} from './localization.js';

function harness(label=()=> 'Category') {
  const element=(tag,className='')=>Object.assign(new FakeElement(),{tagName:tag.toUpperCase(),className,children:[],attributes:new Map(),listeners:{}});
  const button=(text,action,className)=>{const node=element('button',className);node.click=action;return node;};
  const menu=element('div'),changes=[];
  const node=createDropdown({element,button,icon:()=>element('svg'),openMenu:node=>{menu.model=node.menuModel();menu.menuOwner=node;return menu;}},label,value=>changes.push(value));
  return {node,changes,menu,caption:node.children[0]};
}

test('dropdown choices keep their trigger and caption while values and language change',()=>{
  let label='Category',first='All';const h=harness(()=>label),choices=[{value:'',label:()=>first},{value:'color',label:'Color',enabled:false}];
  h.node.update({choices,value:''});const children=[...h.node.children];
  h.node.click();const model=h.node.menuModel();assert.equal(model.sections[0][0].selected,true);assert.equal(model.sections[0][1].enabled,false);
  h.node.update({value:'color'});assert.equal(h.caption.textContent,'Color');assert.equal(h.node.menuModel().sections[0][1].selected,true,'menu queries follow the current value');
  h.node.menuCommand('');assert.deepEqual(h.changes,['']);assert.equal(h.caption.textContent,'All');
  label='Catégorie';first='Tous';refreshBindings();assert.equal(h.node.ariaLabel,label);assert.equal(h.caption.textContent,first);assert.deepEqual(h.node.children,children);
});

test('Rust menu queries follow live state without replacing the trigger',()=>{
  const h=harness();let reads=0,model={title:'Blend',sections:[[{label:'Normal',selected:true,enabled:true,action:{type:'set_blend',blend:'Normal'}}]]};
  h.node.update({text:'Normal',menu:()=>{reads++;return model;}});assert.equal(reads,0);
  h.node.click();assert.equal(reads,1);assert.equal(h.menu.model,model);
  const children=[...h.node.children];model={title:'Blend',sections:[[{label:'Multiply',selected:true,enabled:true,action:{type:'set_blend',blend:'Multiply'}}]]};
  h.node.update({text:'Multiply'});assert.equal(h.node.menuModel(),model);assert.equal(h.caption.textContent,'Multiply');assert.deepEqual(h.node.children,children);
  h.node.listeners.pointerdown[0]();h.node.click();assert.equal(reads,2,'re-click does not reopen the dismissed menu');
});
