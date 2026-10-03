import assert from 'node:assert/strict';
import {test} from 'node:test';
import {readFileSync} from 'node:fs';
import {FakeElement} from './fake-dom.mjs';
import {createNumberField} from './numeric.js';
import {createRangeControl} from './range-control.js';

class Element extends FakeElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag,ownerDocument:doc,children:[],attributes:new Map(),listeners:{},dataset:{},style:{setProperty(){}},value:'',title:''});this.classList={add:(...names)=>this.classes(...names),remove:(...names)=>this.classes(...names.map(name=>'!'+name)),contains:name=>(this.className??'').split(' ').includes(name)};}
  classes(...names){const values=new Set((this.className??'').split(' ').filter(Boolean));for(const name of names)name.startsWith('!')?values.delete(name.slice(1)):values.add(name);this.className=[...values].join(' ');}
  removeAttribute(name){this.attributes.delete(name);}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  replaceChildren(...nodes){for(const child of this.children)child.parentNode=null;this.children=[];this.append(...nodes);}
  contains(node){return node===this||this.children.some(child=>child.contains(node));}
  querySelector(selector){for(const child of this.children){if(selector.startsWith('.')?child.classList.contains(selector.slice(1)):child.tagName===selector)return child;const found=child.querySelector(selector);if(found)return found;}return null;}
  dispatch(type,extra={}){for(const handler of this.listeners[type]??[])handler({target:this,preventDefault(){},stopPropagation(){},...extra});}
  focus(){this.ownerDocument.activeElement=this;this.dispatch('focus');}
  blur(){this.ownerDocument.activeElement=null;this.dispatch('blur');}
  select(){this.setSelectionRange(0,this.value.length);}
  setSelectionRange(start,end){this.selectionStart=start;this.selectionEnd=end;}
}
function harness(t){
  const previous={document:globalThis.document,window:globalThis.window};
  const doc={createElement:tag=>new Element(tag,doc)};globalThis.document=doc;globalThis.window={addEventListener(){},removeEventListener(){}};
  t.after(()=>{for(const[key,value]of Object.entries(previous))if(value===undefined)delete globalThis[key];else globalThis[key]=value;});
  const control={kind:'slider',min:0,max:10,soft_min:0,soft_max:10,scale:1,step:1,digits:1},operations=[],captions=[],changes=[];
  let language='en',failure={numeric_error:{reason:'invalid_expression'},message:'en:invalid_expression'};
  const format=value=>({value,text:String(value),edit:String(value),fill:value/10});
  const app={numeric_labels:label=>({edit:`${language}:edit:${label}`,decrease:`${language}:-:${label}`,increase:`${language}:+:${label}`}),number_input:request=>{operations.push(structuredClone(request));if(request.operation.type==='expression'&&failure!=null)throw failure;return format(request.operation.type==='expression'?7:request.value);},native_caption:request=>{captions.push(request);return `${language}:${request.reason.reason}`;}};
  const options={control,label:'Width',labels:label=>app.numeric_labels(label),resolve:request=>app.number_input(request),errorCaption:reason=>app.native_caption({type:'numeric_error',reason}),onChange:value=>changes.push(value),icon:()=>doc.createElement('svg')};
  return {app,doc,options,operations,captions,changes,switch(tag){language=tag;},failure(next){failure=next;}};
}
function refuse(number,text='é雪{draft}'){number.entry.focus();number.entry.value=text;number.entry.dispatch('input');number.entry.setSelectionRange(1,4);assert.equal(number.commit(),false);return number.entry;}
function publication(h,number,label='Largeur'){
  const entry=number.entry,value=entry.value,selection=[entry.selectionStart,entry.selectionEnd],operations=h.operations.length,changes=h.changes.length;
  h.switch('fr');number.relabel(label);
  assert.equal(number.entry,entry);assert.equal(entry.value,value);assert.deepEqual([entry.selectionStart,entry.selectionEnd],selection);assert.equal(h.doc.activeElement,entry);assert.equal(h.operations.length,operations,'copy publication does not resolve an expression or format a draft');assert.equal(h.changes.length,changes,'copy publication does not commit');
}

test('known numeric refusal relabels the retained reason after a new draft without expression resolution',t=>{
  const h=harness(t),number=createNumberField(h.options),entry=refuse(number);
  assert.equal(entry.title,'en:invalid_expression');assert.equal(entry.getAttribute('aria-invalid'),'true');
  entry.value='2 + untouched é draft';entry.dispatch('input');entry.setSelectionRange(3,8);
  publication(h,number);assert.equal(entry.title,'fr:invalid_expression');assert.equal(entry.getAttribute('aria-label'),'Largeur');assert.equal(number.valueButton.getAttribute('aria-label'),'fr:edit:Largeur');assert.deepEqual(h.captions,[{type:'numeric_error',reason:{reason:'invalid_expression'}}]);assert.deepEqual(h.changes,[]);
  h.failure(null);assert.equal(number.commit(),true);assert.equal(entry.title,'');assert.equal(entry.getAttribute('aria-invalid'),null);assert.deepEqual(h.changes,[7]);const captions=h.captions.length;number.relabel('Breite');assert.equal(h.captions.length,captions,'success retires the retained refusal');
});

test('range refusal captions project only retained structured payloads and cancel clears them',t=>{
  const h=harness(t),reason={reason:'range',label:'literal é雪{user}',min:0,max:10};h.failure({numeric_error:reason,message:'en:range'});
  const number=createNumberField(h.options),entry=refuse(number);assert.equal(entry.title,'en:range');publication(h,number);assert.equal(h.captions[0].reason,reason);assert.equal(entry.title,'fr:range');
  assert.equal(number.cancelEditing(),true);assert.equal(entry.title,'');assert.equal(entry.getAttribute('aria-invalid'),null);const captions=h.captions.length;number.relabel('Breite');assert.equal(h.captions.length,captions);assert.deepEqual(h.changes,[]);
});

test('literal JSON-looking diagnostics remain literal and are not decoded or resolved on relabel',t=>{
  const h=harness(t),literal='{"numeric_error":{"reason":"invalid_expression"},"message":"literal é雪"}';h.failure(literal);
  const number=createNumberField(h.options),entry=refuse(number);assert.equal(entry.title,literal);publication(h,number);assert.equal(entry.title,literal);assert.deepEqual(h.captions,[]);
  h.failure(new Error('native diagnostic é雪'));assert.equal(number.commit(),false);assert.equal(entry.title,'Error: native diagnostic é雪');publication(h,number,'Breite');assert.equal(entry.title,'Error: native diagnostic é雪');assert.deepEqual(h.captions,[]);
});

test('the production app numberField factory wires current typed error captions',t=>{
  const h=harness(t),source=readFileSync(new URL('./app.js',import.meta.url),'utf8').match(/^function numberField\([^]*?^}/m)[0];
  const factory=Function('createNumberField','app','icon','bindCopy',`${source};return numberField;`)(createNumberField,h.app,h.options.icon,()=>{});
  const number=factory(h.options.control,'Width',value=>h.changes.push(value)),entry=refuse(number);assert.equal(entry.title,'en:invalid_expression');publication(h,number);assert.equal(entry.title,'fr:invalid_expression');assert.equal(h.captions.length,1);
});

test('the production range consumer retains refused numeric entries during range relabel',t=>{
  const h=harness(t),bounds=['lower','upper'].map((id,index)=>({id,label:index?'Upper':'Lower',value:index?8:2,numeric:h.options.control}));
  const range=createRangeControl({app:h.app,bounds,label:'Tonal range',icon:h.options.icon,onChange:(index,value)=>h.changes.push([index,value])}),number=range.children[0],entry=refuse(number);
  const operations=h.operations.length,selection=[entry.selectionStart,entry.selectionEnd],value=entry.value;h.switch('fr');range.relabel(bounds.map(field=>({...field,label:field.id==='lower'?'Bas':'Haut'})),'Plage tonale');
  assert.equal(range.children[0],number);assert.equal(number.entry,entry);assert.equal(entry.value,value);assert.deepEqual([entry.selectionStart,entry.selectionEnd],selection);assert.equal(h.doc.activeElement,entry);assert.equal(h.operations.length,operations);assert.equal(entry.title,'fr:invalid_expression');assert.equal(entry.getAttribute('aria-label'),'Bas — Plage tonale');assert.deepEqual(h.changes,[]);assert.equal(h.captions.length,1);range.dispose();
});

test('accepting an unchanged presented value clears a prior refusal without resolving again',t=>{
  const h=harness(t),number=createNumberField(h.options);number.update(4,'canonical presented text');const entry=refuse(number);
  entry.value='canonical presented text';entry.dispatch('input');const operations=h.operations.length;assert.equal(number.commit(),true);assert.equal(h.operations.length,operations);assert.equal(entry.title,'');assert.equal(entry.getAttribute('aria-invalid'),null);number.relabel('Breite');assert.deepEqual(h.captions,[]);assert.deepEqual(h.changes,[]);
});
