import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement as SharedElement} from './fake-dom.mjs';
import {createWorkspaceManager} from './workspace-manager.js';
import {refreshCopy,refreshBindings} from './localization.js';

class Element extends SharedElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag.toUpperCase(),ownerDocument:doc,children:[],attributes:new Map(),listeners:{},dataset:{},style:{},className:'',open:false,scrollTop:0});}
  get firstChild(){return this.children[0];}
  get firstElementChild(){return this.children.find(n=>n.tagName);}
  get parentElement(){return this.parentNode;}
  get textContent(){return this.children.map(n=>n.nodeType===3?n.nodeValue:n.textContent).join('');}
  set textContent(value){this.replaceChildren(this.ownerDocument.createTextNode(value));}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  get isConnected(){return !!this.parentNode;}
  get classList(){return {add:(...names)=>{this.className+=' '+names.join(' ');},remove:()=>{}};}
  insertBefore(node,before){if(node.parentNode)node.remove();node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  prepend(node){return this.insertBefore(node,this.firstChild);}
  after(node){const siblings=this.parentNode.children;node.parentNode=this.parentNode;siblings.splice(siblings.indexOf(this)+1,0,node);}
  replaceChildren(...nodes){for(const node of this.children)node.parentNode=null;this.children=[];this.append(...nodes);}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  matches(selector){if(selector===':popover-open')return !!this.popoverOpen;if(selector.startsWith('.'))return this.className.split(/\s+/).includes(selector.slice(1));if(selector.startsWith('#'))return this.id===selector.slice(1);return this.tagName===selector.toUpperCase();}
  closest(selector){for(let node=this;node;node=node.parentNode)if(node.matches?.(selector))return node;return null;}
  querySelectorAll(selector){return this.children.flatMap(node=>node.tagName?[...(node.matches(selector)?[node]:[]),...node.querySelectorAll(selector)]:[]);}
  querySelector(selector){return this.querySelectorAll(selector)[0]??null;}
  focus(){this.ownerDocument.activeElement=this;}
  showModal(){this.open=true;}
  close(){this.open=false;for(const listener of this.listeners.close??[])listener();}
}
const headerFields=['retry','workspaces','new_workspace','owned_elsewhere','save_as_new_workspace','drag_to_reorder','current_workspace','shown_top','show_top','move_up','move_down'];
function harness(t,{failure=false}={}) {
  const doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)}),addEventListener:()=>{}};doc.body=new Element('body',doc);doc.activeElement=doc.body;const title=new Element('span',doc);title.id='document-title';doc.body.append(title);doc.querySelector=selector=>doc.body.querySelector(selector);
  const saved={document:globalThis.document,window:globalThis.window,sessionStorage:globalThis.sessionStorage,BroadcastChannel:globalThis.BroadcastChannel};Object.assign(globalThis,{document:doc,window:{addEventListener:()=>{}},sessionStorage:{getItem:()=>null,setItem:()=>{}},BroadcastChannel:class {addEventListener(){}postMessage(){}}});t.after(()=>Object.assign(globalThis,saved));
  const timers=[];t.mock.method(globalThis,'setInterval',fn=>{timers.push(fn);return timers.length;});
  let tag='en';const inputs=[];let ticks=0,observes=0;
  const row=()=>({id:'literal-id',title:`${tag}:row-title`,subtitle:`${tag}:row-subtitle`,current:true,actions:[]});
  const view=()=>({ready:true,busy:false,switcher_busy:false,dirty:false,id:'literal-id',name:'İı ไทย Tiếng Việt { $name } 🎨',page:failure?null:'workspaces',rows:failure?[]:[row()],switcher:[],switcher_display:[],switcher_options_label:`${tag}:workspace-options`,switcher_options:{title:`${tag}:workspace-options`,sections:[]},switcher_menu:{title:`${tag}:workspaces`,sections:[]},order:['literal-id'],switcher_revision:1,selected:'literal-id',title:`${tag}:title`,intro:`${tag}:intro`,primary:`${tag}:primary`,enabled:true,error:failure?`${tag}:write-failed`:null,switcher_error:null,focus_window:null,retry:failure,prompt:failure?null:{title:`${tag}:prompt-title`,message:`${tag}:prompt-message`,name:'Original literal name',confirm:`${tag}:confirm`,destructive:false},prompt_action:failure?null:{type:'rename',value:'literal-id'}});
  const app={catalog:()=>({native_copy:{header:Object.fromEntries(headerFields.map(name=>[name,`${tag}:${name}`]))}}),bootstrap_view:()=>({common:Object.fromEntries(['close','cancel','name','keep_open'].map(name=>[name,`${tag}:${name}`]))}),workspace_view:()=>JSON.stringify(view()),workspace_start:()=>{},workspace_tick:()=>{ticks++;return {};},workspace_observe:()=>{observes++;},workspace_input:value=>{inputs.push(JSON.parse(value));return {};},native_caption:({type,title})=>`${tag}:${type}:${title}`};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null)node.textContent=text;return node;};const button=(text,run,cls)=>{const node=element('button',cls,text);node.click=run;return node;};
  const manager=createWorkspaceManager({app,store:{execute:()=>{},holdOwner:()=>{}},applyChange:()=>{},element,button,icon:()=>element('span','icon'),message:()=>{}});
  return {manager,doc,inputs,switch(next){tag=next;refreshCopy(app);manager.localize();refreshBindings();},tick(){timers[0]();},stats:()=>({ticks,observes}),nodes:()=>({dialog:doc.body.querySelector('.workspace-manager'),form:doc.body.querySelectorAll('.workspace-form').find(node=>node.open)??doc.body.querySelector('.workspace-form'),row:doc.body.querySelector('.workspace-row'),input:doc.body.querySelector('input')})};
}

test('workspace prompt and rows publish copy without replacing dirty names, selection, focus or native controls',t=>{
  const h=harness(t),original=h.nodes(),input=original.input;input.value='İı ไทย Tiếng Việt { $name } 🎨 draft';input.selectionStart=2;input.selectionEnd=8;input.composing=true;input.focus();const initialStats=h.stats();
  h.switch('fr');assert.deepEqual(h.nodes(),original);assert.equal(input.value,'İı ไทย Tiếng Việt { $name } 🎨 draft');assert.equal(input.selectionStart,2);assert.equal(input.selectionEnd,8);assert.equal(input.composing,true);assert.equal(h.doc.activeElement,input);assert.deepEqual(h.stats(),initialStats);assert.deepEqual(h.inputs,[]);
  assert.equal(original.form.querySelector('h2').textContent,'fr:prompt-title');assert.equal(original.form.querySelector('p').textContent,'fr:prompt-message');assert.equal(input.ariaLabel,'fr:name');assert.equal(original.form.querySelector('.suggested-action').textContent,'fr:confirm');
  assert.equal(h.doc.body.querySelector('.workspace-switcher').ariaLabel,'fr:workspaces');assert.equal(h.doc.body.querySelector('.workspace-switcher-options').ariaLabel,'fr:workspace-options');
  assert.equal(original.dialog.querySelector('h2').textContent,'fr:title');assert.equal(original.row.querySelector('.workspace-row-title').textContent,'fr:row-title');assert.equal(original.row.querySelector('.workspace-row-subtitle').textContent,'fr:row-subtitle');
  h.tick();assert.deepEqual(h.nodes(),original);assert.equal(h.doc.activeElement,input);assert.equal(input.value,'İı ไทย Tiếng Việt { $name } 🎨 draft');
});

test('workspace recovery displays the complete shared failure and relabels retained recovery controls',t=>{
  const h=harness(t,{failure:true}),recovery=h.nodes().form,trigger=h.doc.body.querySelector('.workspace-recovery'),controls=recovery.querySelectorAll('button');
  assert.equal(trigger.textContent,'en:write-failed');h.switch('tr');assert.equal(h.nodes().form,recovery);assert.deepEqual(recovery.querySelectorAll('button'),controls);assert.equal(trigger.textContent,'tr:write-failed');assert.equal(recovery.ariaLabel,'tr:workspaces');assert.equal(recovery.querySelector('p').textContent,'tr:write-failed');assert.deepEqual(controls.map(node=>node.textContent),['tr:keep_open','tr:retry','tr:save_as_new_workspace']);assert.deepEqual(h.inputs,[]);
});
