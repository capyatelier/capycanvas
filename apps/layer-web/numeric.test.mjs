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

test('native page numbers can omit their displayed unit without changing expression input',t=>{
  const h=harness(t),control={...h.options.control,kind:'number',unit:'ppi',max:10000};
  const number=createNumberField({...h.options,control,resolve:request=>{const value=h.options.resolve(request);return {...value,text:value.text+' ppi'};}});
  number.update(300);assert.equal(number.entry.value,'300 ppi');
  const reads=h.operations.length;number.setShowUnits(false);assert.equal(h.operations.length,reads);assert.equal(number.entry.value,'300');
  h.failure(null);number.entry.focus();number.entry.value='400 ppi';assert.equal(number.commit(),true);
  assert.equal(h.operations.at(-1).control.unit,'ppi');assert.equal(h.operations.at(-1).operation.text,'400 ppi');
  assert.equal(number.entry.value,'7');number.setShowUnits(true);assert.equal(number.entry.value,'7 ppi');
});
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

function contactElements(number){
  const find=(node,cls)=>node.classList?.contains(cls)?node:node.children.map(child=>find(child,cls)).find(Boolean);
  for(const node of [number.valueButton,find(number,'number-dial-face')].filter(Boolean)){
    node.captured=new Set();node.setPointerCapture=id=>node.captured.add(id);node.hasPointerCapture=id=>node.captured.has(id);node.releasePointerCapture=id=>node.captured.delete(id);
    node.getBoundingClientRect=()=>({x:100,y:40,width:28,height:28});
  }
  return find(number,'number-dial-face');
}

test('a panel dial sits in a slider row and one press or drag relative to its centre is one gesture',async t=>{
  const h=harness(t),control={...h.options.control,kind:'dial',min:0,max:360,step:1,default_value:225},phases=[],listeners={};
  globalThis.window={addEventListener:(type,listener)=>(listeners[type]??=new Set()).add(listener),removeEventListener:(type,listener)=>listeners[type]?.delete(listener)};
  const number=createNumberField({...h.options,control,label:'Light'});number.onEditPhase=phase=>phases.push([phase,h.operations.length]);
  const dial=contactElements(number);
  const contact=(type,extra)=>{if(type==='pointerdown')number.dispatch(type,{target:dial,...extra});dial.dispatch(type,extra);if(type==='pointerup')for(const listener of [...listeners.pointerup??[]])listener(extra);};
  assert.ok(number.classList.contains('number-panel'),'the dial uses the panel slider row');
  assert.equal(number.valueButton.hidden,false,'the value stays a plain tappable readout');
  assert.equal(number.querySelector('.number-step'),null,'panel dials have no step buttons');
  assert.deepEqual(['role','aria-label','aria-valuemin','aria-valuemax'].map(name=>dial.getAttribute(name)),['slider','Light','0','360']);
  assert.equal(dial.tabIndex,0);
  h.operations.length=0;
  contact('pointerdown',{button:0,pointerId:3,clientX:100,clientY:40});
  assert.ok(dial.captured.has(3),'the dial keeps the contact after it leaves the face');assert.equal(h.doc.activeElement,dial);
  contact('pointermove',{pointerId:3,clientX:300,clientY:54});
  contact('pointermove',{pointerId:4,clientX:0,clientY:0});
  contact('pointerup',{pointerId:3});
  contact('pointermove',{pointerId:3,clientX:0,clientY:0});
  await new Promise(resolve=>setTimeout(resolve,0));
  assert.deepEqual(h.operations.map(r=>r.operation),[{type:'dial',x:-14,y:-14,radius:14},{type:'dial',x:186,y:0,radius:14}]);
  assert.deepEqual(phases,[['down',0],['up',2]],'the press opens the gesture before the first dial operation and release closes it');
  h.operations.length=0;phases.length=0;const stopped=[];
  for(const [key,shiftKey] of [['ArrowRight',false],['ArrowDown',false],['ArrowUp',true],['PageDown',false],['a',false]]){
    const event={key,shiftKey,stopPropagation:()=>stopped.push(key)};
    number.dispatch('keydown',{target:dial,...event});dial.dispatch('keydown',event);number.dispatch('keyup',{target:dial,key});
  }
  assert.deepEqual(h.operations.map(r=>r.operation.steps),[1,-1,15,-45]);assert.deepEqual(stopped,['ArrowRight','ArrowDown','ArrowUp','PageDown']);
  assert.deepEqual(phases.map(([phase])=>phase),['down','up','down','up','down','up','down','up'],'each held key is one gesture');
  assert.equal(dial.getAttribute('aria-valuetext'),String(number.getValue()));
  number.setDisabled(true);h.operations.length=0;phases.length=0;
  contact('pointerdown',{button:0,pointerId:5,clientX:100,clientY:40});dial.dispatch('keydown',{key:'ArrowRight'});
  assert.deepEqual([h.operations,phases],[[],[]],'a disabled dial accepts no input');assert.equal(dial.tabIndex,-1);assert.equal(dial.getAttribute('aria-disabled'),'true');
  number.relabel('Licht');assert.deepEqual([dial.title,dial.getAttribute('aria-label')],['Licht','Licht']);
});

test('outside panels a dial keeps a plain value field',t=>{
  const h=harness(t),number=createNumberField({...h.options,control:{...h.options.control,kind:'dial',max:360},panel:false});
  assert.equal(number.classList.contains('number-panel'),false);assert.equal(contactElements(number),undefined);
  assert.equal(number.entry.hidden,false);assert.equal(number.entry.getAttribute('role'),'spinbutton');
});

test('Shift scrubs finely and changing it mid-drag continues from the current value without a jump',t=>{
  const h=harness(t),scrubs=[];
  const resolve=request=>{const {operation}=request;if(operation.type==='scrub')scrubs.push(operation);
    const value=operation.type==='scrub'?operation.origin+operation.pixels/(operation.fine?40:4):request.value;return {value,text:String(value),edit:String(value),fill:value/10};};
  const number=createNumberField({...h.options,control:{...h.options.control,max:1000},resolve}),phases=[];number.onEditPhase=phase=>phases.push(phase);
  contactElements(number);number.update(5);
  const move=(clientY,shiftKey=false)=>number.valueButton.dispatch('pointermove',{pointerId:1,clientY,shiftKey});
  number.valueButton.dispatch('pointerdown',{button:0,pointerId:1,clientY:100,shiftKey:false});
  move(97);move(80);move(40,true);move(0,true);move(-40);
  assert.equal(number.valueButton.textContent,'22','an active drag uses the shared readout without a second formatter');
  number.valueButton.dispatch('pointerup',{pointerId:1,clientY:-40,shiftKey:false});
  assert.deepEqual(scrubs,[
    {type:'scrub',origin:5,pixels:20,fine:false},
    {type:'scrub',origin:10,pixels:40,fine:true},
    {type:'scrub',origin:10,pixels:80,fine:true},
    {type:'scrub',origin:12,pixels:40,fine:false},
    {type:'scrub',origin:12,pixels:40,fine:false},
  ]);
  assert.equal(number.getValue(),22);assert.deepEqual(phases,['down','up']);
  scrubs.length=0;phases.length=0;
  number.valueButton.dispatch('pointerdown',{button:0,pointerId:2,clientY:0,shiftKey:true});
  number.valueButton.dispatch('pointermove',{pointerId:2,clientY:-80,shiftKey:false});
  number.valueButton.dispatch('pointercancel',{pointerId:2});
  assert.deepEqual(scrubs,[{type:'scrub',origin:22,pixels:80,fine:false}],'a press made with Shift switches to normal speed from the press point');
  assert.equal(number.getValue(),22,'cancelling restores the value from before the drag');assert.deepEqual(phases,['down','cancel']);
});

test('readout wheel commits text and uses shared stepping while disabled controls retain scrolling',t=>{
  const h=harness(t),number=createNumberField(h.options);
  h.failure(null);number.update(4);number.valueButton.dispatch('click');number.entry.value='7';
  let prevented=0;
  number.valueButton.dispatch('wheel',{deltaY:-120,preventDefault(){prevented++;}});
  assert.deepEqual(h.operations.slice(-2).map(r=>r.operation),[{type:'expression',text:'7'},{type:'step',steps:1}]);
  assert.equal(prevented,1);
  number.setDisabled(true);number.valueButton.dispatch('wheel',{deltaY:120,preventDefault(){prevented++;}});
  assert.equal(prevented,1);
});

test('standalone pen and touch scrubs use normalized position and cancel restores the original value',t=>{
  const h=harness(t),number=createNumberField({...h.options,panel:false,valueOnly:true,inline:true});
  contactElements(number);number.update(4);
  for(const pointerType of ['pen','touch']){
    number.valueButton.dispatch('pointerdown',{button:0,pointerId:1,pointerType,clientY:100});
    number.valueButton.dispatch('pointermove',{pointerId:1,clientY:60});
    assert.deepEqual(h.operations.at(-1).operation,{type:'position',position:.6000000000000001});
    number.valueButton.dispatch('pointercancel',{pointerId:1});
    assert.equal(number.getValue(),4);
  }
  const before=h.operations.length;
  number.valueButton.dispatch('pointerdown',{button:0,pointerId:2,pointerType:'mouse',clientY:100});
  number.valueButton.dispatch('pointermove',{pointerId:2,clientY:60});
  assert.equal(h.operations.length,before);
});

test('angle dial wheel uses shared Step without keyboard focus',t=>{
  const h=harness(t),number=createNumberField({...h.options,control:{...h.options.control,kind:'dial'}});
  number.querySelector('.number-dial-face').dispatch('wheel',{deltaY:1});
  assert.deepEqual(h.operations.at(-1).operation,{type:'step',steps:-1});
});

test('invalid numeric draft consumes readout wheel without stepping',t=>{
  const h=harness(t),number=createNumberField(h.options);number.valueButton.dispatch('click');
  number.entry.value='invalid';let consumed=false;
  number.entry.dispatch('wheel',{deltaY:1,preventDefault(){consumed=true;}});
  assert.equal(consumed,true);assert.equal(h.operations.at(-1).operation.type,'expression');
  assert.equal(number.entry.getAttribute('aria-invalid'),'true');
});
