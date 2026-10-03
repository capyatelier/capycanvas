import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement as SharedElement} from './fake-dom.mjs';
import {chooseDocumentColor} from './document-color.js';
import {refreshCopy,refreshBindings} from './localization.js';

class Element extends SharedElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag.toUpperCase(),ownerDocument:doc,children:[],attributes:new Map(),listeners:{},value:'',className:'',style:{}});}
  get firstChild(){return this.children[0];}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join('');}
  set textContent(value){this.replaceChildren(this.ownerDocument.createTextNode(value));}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  replaceChildren(...nodes){for(const node of this.children)node.parentNode=null;this.children=[];this.append(...nodes);}
  querySelectorAll(selector){const tags=selector.split(',').map(tag=>tag.toUpperCase());return this.children.flatMap(node=>node.tagName?[...(tags.includes(node.tagName)?[node]:[]),...node.querySelectorAll(selector)]:[]);}
  getContext(type,options){assert.equal(type,"2d");assert.deepEqual(options,{willReadFrequently:true});return {putImageData:image=>{this.image=image;}};}
  focus(){this.ownerDocument.activeElement=this;}
}
const settle=()=>new Promise(done=>setImmediate(done));
function harness(t,{source=false,missing=null,failure=null}={}) {
  const previous=globalThis.ImageData;globalThis.ImageData=class{constructor(data,width,height){Object.assign(this,{data,width,height});}};t.after(()=>{globalThis.ImageData=previous;});
  const doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};let language='en',form,finish,resolver,freed=false,reads=0,prepares=0;
  const fields=['assign_title','assign_help','repair_title','repair_help','space','correct_profile','composition_preview','outside_gamut','preparing','preview_help','prepared_composition','original_composition','before','after','preview','add_source','apply_profile'];
  const documentCopy=()=>({...Object.fromEntries(fields.map(field=>[field,`${language}:${field}`])),common:{apply:`${language}:apply`,cancel:`${language}:cancel`}});
  const app={document_color:()=>({space:'Srgb',depth:'U8'}),document_color_copy:documentCopy,export_copy:()=>({}),profile_copy:()=>({import:`${language}:import`,saved_dialog:`${language}:saved`}),
    capture_control:()=>({cancelled:()=>false,cancel(){},free(){}}),color_feature_error_copy:error=>`${language}:${error.color_feature_error}`,
    source_profile_name_copy:value=>value==null?`${language}:unnamed`:value,color_source_preview:(name,preview,adds)=>`${name} / ${preview} / ${adds}`,
    prepare_color(){prepares++;return new Promise((done,reject)=>{resolver=failure?()=>reject(failure):done;});},prepare_source(){prepares++;return new Promise(done=>{resolver=done;});},prepare_source_comparison:prepared=>prepared};
  const read=value=>{reads++;assert.equal(freed,false,'no binding may read a freed candidate');return value;};
  const candidate={clipped_channels:()=>read(0),is_copy:()=>read(false),adds_layer:()=>read(true),source_profile:()=>read(missing),previews:()=>read([{extent:[1,1],pixels:[255,0,0,255]},{extent:[1,1],pixels:[0,255,0,255]}]),free(){freed=true;}};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bind(node,text);else node.textContent=text;}return node;};
  const bind=(node,reader)=>{bindCopy(node,reader);return node;};
  const button=(text,action,cls)=>{const node=element('button',cls,text);node.click=action;return node;};
  const result=chooseDocumentColor({app,element,button,id:7,request:source?{type:'repair_source_profile'}:{type:'document_color',operation:'assign'},gpuOperation:run=>run(),dialog:(title,build)=>new Promise(done=>{form=element('form');finish=done;build(form,done);})});
  return {app,doc,form,result,candidate,switch(tag){language=tag;refreshCopy(app);form.localize();refreshBindings();},resolve(){resolver(candidate);},stats:()=>({reads,prepares}),buttons:()=>form.querySelectorAll('button'),finish};
}
import {bindCopy} from './localization.js';

test('prepared document-color copy retains controls, selected options, focus and immutable preview pixels',async t=>{
  const h=harness(t),controls=h.form.querySelectorAll('select,button'),space=controls[0];space.value='ProPhoto';space.focus();
  h.buttons().find(node=>node.textContent==='en:preview').click();h.switch('tr');assert.equal(h.stats().prepares,1);
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'tr:preparing');h.resolve();await settle();
  const canvases=h.form.querySelectorAll('canvas'),pixels=canvases.map(node=>node.image),before=h.stats();h.switch('fr');
  assert.deepEqual(h.form.querySelectorAll('select,button'),controls);assert.deepEqual(h.form.querySelectorAll('canvas'),canvases);assert.deepEqual(canvases.map(node=>node.image),pixels);assert.equal(space.value,'ProPhoto');assert.equal(h.doc.activeElement,space);assert.deepEqual(h.stats(),before);
  assert.equal(space.ariaLabel,'fr:space');assert.equal(canvases[0].ariaLabel,'fr:original_composition');assert.equal(h.form.querySelectorAll('figcaption')[1].textContent,'fr:after');assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'fr:composition_preview');assert.equal(h.buttons().at(-1).textContent,'fr:apply');
  h.buttons().at(-1).click();assert.equal(await h.result,h.candidate);h.candidate.free();h.switch('vi');assert.equal(h.stats().reads,before.reads,'detached retained bindings use scalar snapshots after owner frees result');
});

test('source profile presentation preserves raw missing versus literal empty name and never repeats decoding',async t=>{
  for(const missing of [null,'']){
    const h=harness(t,{source:true,missing});h.buttons().find(node=>node.textContent==='en:preview').click();h.resolve();await settle();const before=h.stats();
    h.switch('tr');assert.equal(h.form.querySelectorAll('p').at(-1).textContent,`${missing==null?'tr:unnamed':''} / tr:composition_preview / true`);assert.equal(h.buttons().at(-1).textContent,'tr:add_source');assert.deepEqual(h.stats(),before);
    h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();await h.result;
  }
});

test('shared candidate failure is projected at publication without preparing again',async t=>{
  const h=harness(t,{failure:{color_feature_error:'ProfileReadLimit'}});h.buttons().find(node=>node.textContent==='en:preview').click();h.resolve();await settle();assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'en:ProfileReadLimit');
  h.switch('ru');assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'ru:ProfileReadLimit');assert.equal(h.stats().prepares,1);h.buttons().find(node=>node.textContent.endsWith(':cancel')).click();await h.result;
});
