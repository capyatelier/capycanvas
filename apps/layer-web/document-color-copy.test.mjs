import {dropdownFactory,chooseDropdown} from './dropdown-test-fixture.mjs';
import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement as SharedElement} from './fake-dom.mjs';
import {chooseDocumentColor} from './document-color.js';
import {dialogProjection} from './color-dialog-fixture.mjs';
import {refreshCopy,refreshBindings} from './localization.js';

class Element extends SharedElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag.toUpperCase(),ownerDocument:doc,children:[],attributes:new Map(),listeners:{},value:'',className:'',style:{setProperty(name,value){this[name]=value;}},dataset:{}});const classes=new Set();this.classList={add(...names){names.forEach(name=>classes.add(name));},contains:name=>classes.has(name),toggle(name,on){if(on)classes.add(name);else classes.delete(name);},remove(name){classes.delete(name);}};}
  prepend(...nodes){for(const node of nodes.reverse())this.insertBefore(node,this.firstChild);}
  get firstChild(){return this.children[0];}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join('');}
  set textContent(value){this.replaceChildren(this.ownerDocument.createTextNode(value));}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  replaceChildren(...nodes){for(const node of this.children)node.parentNode=null;this.children=[];this.append(...nodes);}
  querySelectorAll(selector){const tags=selector.split(',').map(tag=>tag.toUpperCase());return this.children.flatMap(node=>node.tagName?[...(tags.includes(node.tagName)?[node]:[]),...node.querySelectorAll(selector)]:[]);}
  querySelector(selector){return this.querySelectorAll(selector)[0]??null;}
  getContext(type,options){assert.equal(type,"2d");assert.deepEqual(options,{willReadFrequently:true});return {putImageData:image=>{this.image=image;}};}
  focus(){this.ownerDocument.activeElement=this;}
  closest(tag){for(let node=this;node;node=node.parentNode)if(node.tagName===tag.toUpperCase())return node;return null;}
  getBoundingClientRect(){return{x:0,y:0,width:0,height:0,left:0,right:0,top:0,bottom:0};}
  showModal(){this.open=true;}
  close(){this.open=false;this.dispatchEvent({type:'close'});}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
}
const settle=()=>new Promise(done=>setImmediate(done));
function harness(t,{source=false,history=false,operation='assign',comparisonRequired=true,missing=null,failure=null}={}) {
  const previous=globalThis.ImageData;globalThis.ImageData=class{constructor(data,width,height){Object.assign(this,{data,width,height});}};t.after(()=>{globalThis.ImageData=previous;});
  const doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};
  doc.createElement=tag=>new Element(tag,doc);doc.body=new Element('body',doc);
  doc.querySelector=selector=>selector==='dialog[open]'?doc.body.querySelectorAll('dialog').find(node=>node.open)??null:null;
  const previousDom={document:globalThis.document,window:globalThis.window,ResizeObserver:globalThis.ResizeObserver,innerWidth:globalThis.innerWidth,innerHeight:globalThis.innerHeight};
  Object.assign(globalThis,{document:doc,window:{addEventListener(){},removeEventListener(){}},ResizeObserver:class{observe(){}unobserve(){}disconnect(){}},innerWidth:1200,innerHeight:900});
  t.after(()=>Object.assign(globalThis,previousDom));
  let language='en',form,finish,resolver,intent,title,freed=false,reads=0,prepares=0;const choices=[];
  const fields=['depth_title','depth_8','depth_16','depth_float16','depth_float32','assign_native_help','convert_native_help','depth_native_help','source_baked_help','source_native_help','current_source','choose_destination','choose_different','compare_before_apply','clipped_comparison','layered_open','preparing_comparison','convert_layers','create_flattened','assign_title','assign_help','repair_title','repair_help','space','correct_profile','outside_gamut','preparing','preparing_title','preparing_help','prepared_composition','original_composition','before','after','preview','add_source','apply_profile'];
  const documentCopy=()=>({...Object.fromEntries(fields.map(field=>[field,`${language}:${field}`])),dialog:{frame:{kind:'alert',prefer_wide:true},role:'editor',modal:true,primary:'action'},common:{apply:`${language}:apply`,cancel:`${language}:cancel`},progress_dialog:{frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:null}});
  const app={...dialogProjection,bootstrap_view:()=>({common:{cancel:`${language}:cancel`}}),document_color:()=>({space:'Srgb',depth:'U8'}),document_color_copy:documentCopy,export_copy:()=>({profile:`${language}:profile`,depth:`${language}:depth`,reduce_banding:`${language}:reduce_banding`,dither_gradients:`${language}:dither_gradients`}),comparison_copy:()=>({gap:12,height:120,width:160,copy:{before:`${language}:before`,after:`${language}:after`}}),profile_picker_standard:()=>['Srgb','DisplayP3','AdobeRgb','ProPhoto'].map(value=>({profile:{Builtin:value},name:value})),profile_name_copy:name=>name,profile_picker_inventory:async()=>({document:false,current:false,saved:[],standard:true}),validate_profile_picker:async value=>value,document_color_dither:(depth,active)=>({available:depth==='U8',value:depth==='U8'&&active?'Stochastic8':'None'}),color_comparison_needed:()=>comparisonRequired,profile_library:async()=>[],source_dialog_context:()=>({baked:true,assumed:true,name:missing,purpose:{kind:'source',channels:'Rgb'}}),source_description:(name,assumed)=>`${name==null?language+':unnamed':name} / ${assumed}`,profile_copy:()=>({import:`${language}:import`,saved_dialog:`${language}:saved`}),
    capture_control:()=>{let cancelled=false,freed=false;return{cancelled:()=>cancelled,cancel(){assert.equal(freed,false);cancelled=true;},free(){freed=true;}};},color_feature_error_copy:error=>`${language}:${error.color_feature_error}`,
    source_profile_name_copy:value=>value==null?`${language}:unnamed`:value,
    prepare_color(_id,choice){choices.push(choice);prepares++;return new Promise((done,reject)=>{resolver=failure?()=>reject(failure):done;});},prepare_source(){prepares++;return new Promise(done=>{resolver=done;});},prepare_source_comparison:prepared=>prepared};
  const read=value=>{reads++;assert.equal(freed,false,'no binding may read a freed candidate');return value;};
  const candidate={clipped_channels:()=>read(0),is_copy:()=>read(false),source_profile:()=>read(missing),previews:()=>read([{extent:[1,1],pixels:[255,0,0,255]},{extent:[1,1],pixels:[0,255,0,255]}]),free(){freed=true;}};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bind(node,text);else node.textContent=text;}return node;};
  const bind=(node,reader)=>{bindCopy(node,reader);return node;};
  const button=(text,action,cls)=>{const node=element('button',cls,text);node.click=action;return node;};
  const result=chooseDocumentColor({app,element,button,icon:asset=>element('svg','',asset),dropdown:dropdownFactory({element,button}),id:7,request:history?{type:'color_history',redo:false}:source?{type:'repair_source_profile'}:{type:'document_color',operation},gpuOperation:run=>run(),dialog:(readTitle,build,viewIntent)=>new Promise(done=>{title=readTitle;intent=viewIntent;form=element('form');finish=done;build(form,done);})});
  return {app,doc,form,result,candidate,intent:()=>intent,title:()=>title(),switch(tag){language=tag;refreshCopy(app);form.localize();refreshBindings();},resolve(value=candidate){resolver(value);},stats:()=>({reads,prepares}),choices,buttons:()=>form.querySelectorAll('button'),finish};
}
import {bindCopy} from './localization.js';

test('prepared document-color copy retains controls, selected choices, focus and immutable preview pixels',async t=>{
  const h=harness(t),controls=h.form.querySelectorAll('button'),space=controls[0];space.focus();
  await settle();h.switch('tr');assert.equal(h.stats().prepares,1);
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'tr:preparing_comparison');h.resolve();await settle();
  const canvases=h.form.querySelectorAll('canvas'),pixels=canvases.map(node=>node.image),before=h.stats();h.switch('fr');
  assert.deepEqual(h.form.querySelectorAll('button'),controls);assert.deepEqual(h.form.querySelectorAll('canvas'),canvases);assert.deepEqual(canvases.map(node=>node.image),pixels);assert.equal(space.menuModel().sections[0].find(choice=>choice.selected).command,'Srgb');assert.equal(h.doc.activeElement,space);assert.deepEqual(h.stats(),before);
  assert.equal(space.ariaLabel,'fr:profile');assert.equal(canvases[0].ariaLabel,'fr:before');assert.equal(h.form.querySelectorAll('figcaption')[1].textContent,'fr:after');assert.equal(h.form.querySelectorAll('p').find(node=>node.className==='document-color-detail').textContent,'fr:compare_before_apply');assert.equal(canvases[0].parentNode.children[0].tagName,'FIGCAPTION');assert.equal(h.buttons().at(-1).textContent,'fr:apply');
  h.buttons().at(-1).click();assert.equal(await h.result,h.candidate);h.candidate.free();h.switch('vi');assert.equal(h.stats().reads,before.reads,'detached retained bindings use scalar snapshots after owner frees result');
});

test('source profile presentation preserves raw missing versus literal empty name and never repeats decoding',async t=>{
  for(const missing of [null,'']){
    const h=harness(t,{source:true,missing});await settle();h.resolve();await settle();const before=h.stats();
    h.switch('tr');assert.ok(h.form.querySelectorAll('p').some(node=>node.textContent===`${missing==null?'tr:unnamed':''} / true`));assert.equal(h.buttons().at(-1).textContent,'tr:add_source');assert.deepEqual(h.stats(),before);
    h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();await h.result;
  }
});

test('shared candidate failure is projected at publication without preparing again',async t=>{
  const h=harness(t,{failure:{color_feature_error:'ProfileReadLimit'}});await settle();h.resolve();await settle();assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:ProfileReadLimit');
  h.switch('ru');assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'ru:ProfileReadLimit');assert.equal(h.stats().prepares,1);h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();await h.result;
});

test('changed choices cancel and drain the active comparison before preparing the latest draft',async t=>{
  const h=harness(t);await settle();assert.equal(h.stats().prepares,1);
  const space=h.form.querySelectorAll('button').find(n=>n.menuModel);chooseDropdown(space,'DisplayP3');chooseDropdown(space,'ProPhoto');
  assert.equal(h.stats().prepares,1,'a successor cannot overlap its decoder');
  let discarded=false;h.resolve({free(){discarded=true;}});await settle();
  assert.equal(discarded,true);assert.equal(h.stats().prepares,2,'intermediate choices coalesce');
  h.resolve();await settle();h.buttons().at(-1).click();assert.equal(await h.result,h.candidate);
});


test('Cancel drains a superseded comparison without starting a queued successor or using released controls',async t=>{
  const h=harness(t);await settle();const space=h.form.querySelectorAll('button').find(n=>n.menuModel);chooseDropdown(space,'DisplayP3');
  h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();let discarded=false;h.resolve({free(){discarded=true;}});
  assert.equal(await h.result,null);assert.equal(discarded,true);await settle();assert.equal(h.stats().prepares,1);
  space.menuCommand('ProPhoto');assert.equal(h.stats().prepares,1,'retired menu callbacks cannot touch released control');
});

test('comparison preparation stays in its editor and never overlays its choices with another modal',async t=>{
  const h=harness(t);await settle();
  assert.deepEqual(h.intent(),{frame:{kind:'alert',prefer_wide:true},role:'editor',modal:true,primary:'action'});
  assert.equal(h.doc.body.querySelectorAll('dialog').length,0,'Preparation must not create a second modal over the retained editor');
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:preparing_comparison');
  const space=h.form.querySelectorAll('button').find(node=>node.menuModel);chooseDropdown(space,'DisplayP3');
  assert.equal(h.stats().prepares,1,'The changed draft cancels and drains the in-flight preparation');
  h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();h.resolve();assert.equal(await h.result,null);
});

test('color history uses one cancel-only progress surface without an underlying editor',async t=>{
  const h=harness(t,{history:true});await settle();
  assert.deepEqual(h.intent(),{frame:{kind:'alert',prefer_wide:false},role:'alert',modal:true,primary:null});
  assert.equal(h.title(),'en:preparing_title');
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:preparing_help');
  assert.equal(h.form.querySelectorAll('p').at(-1).className,'alert-description');
  assert.deepEqual(h.form.children.filter(node=>node.tagName!=='FOOTER').map(node=>node.tagName),['P']);
  assert.equal(h.doc.body.querySelectorAll('dialog').length,0,'The operation owns one surface');
  assert.equal(h.buttons().length,1);h.resolve();assert.equal(await h.result,h.candidate);h.candidate.free();
});


test('Bit Depth renders the native Reduce banding switch and forwards its shared dither value',async t=>{
  const h=harness(t,{operation:'depth'});await settle();
  const dither=h.form.querySelectorAll('input')[0];
  assert.equal(dither.getAttribute('role'),'switch');assert.equal(dither.ariaLabel,'en:reduce_banding');assert.equal(dither.disabled,false);
  assert.ok(h.form.querySelectorAll('p').some(node=>node.textContent==='en:dither_gradients'));
  assert.deepEqual(h.choices,[{Depth:{depth:'U8',dither:'None'}}]);
  dither.checked=true;dither.onchange();h.resolve({free(){}});await settle();
  assert.deepEqual(h.choices.at(-1),{Depth:{depth:'U8',dither:'Stochastic8'}});
  h.buttons().find(node=>node.textContent==='en:cancel').click();h.resolve();await h.result;
});


test('an unchanged color choice shows the shared explanation without starting a comparison',async t=>{
  const h=harness(t,{comparisonRequired:false});await settle();
  assert.equal(h.stats().prepares,0);assert.equal(h.form.querySelectorAll('canvas').length,0);
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:choose_different');assert.equal(h.buttons().at(-1).disabled,true);
  h.buttons().find(node=>node.textContent==='en:cancel').click();assert.equal(await h.result,null);
});

test('Repair invalidates its admitted comparison while a profile is reading or rejected',async t=>{
  const h=harness(t,{source:true});await settle();h.resolve();await settle();
  const apply=h.buttons().at(-1),picker=h.buttons().find(node=>node.menuModel);assert.equal(apply.disabled,false);
  let reject;h.app.validate_profile_picker=()=>new Promise((_,fail)=>{reject=fail;});
  chooseDropdown(picker,'standard:1');assert.equal(apply.disabled,true);assert.equal(h.form.querySelectorAll('canvas').length,0);
  await settle();reject({color_feature_error:'SourceChannels'});await settle();
  assert.equal(apply.disabled,true);assert.equal(h.stats().prepares,1);assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:SourceChannels');
  let finished=false;h.result.then(()=>{finished=true;});apply.click();await settle();assert.equal(finished,false);
  h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();assert.equal(await h.result,null);
});
