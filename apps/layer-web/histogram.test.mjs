import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement as SharedElement} from './fake-dom.mjs';
import {createHistogram} from './histogram.js';
import {refreshCopy} from './localization.js';

class Element extends SharedElement {
  constructor(tag,doc) {
    super();this.tagName=tag.toUpperCase();this.ownerDocument=doc;this.children=[];this.attributes=new Map();this.listeners={};this.open=false;this._value='';this.drawn=[];
  }
  get firstChild(){return this.children[0];}
  get textContent(){return this.children.map(n=>n.nodeType===3?n.nodeValue:n.textContent).join('');}
  set textContent(value){this.children=[this.ownerDocument.createTextNode(value)];}
  get options(){return this.children.filter(n=>n.tagName==='OPTION');}
  get value(){return this._value||(this.tagName==='SELECT'?this.options[0]?.value??'':'');}
  set value(value){this._value=String(value);}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  get isConnected(){return !!this.parentNode;}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  prepend(...nodes){for(const node of nodes.reverse())this.insertBefore(node,this.firstChild);}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  show(){this.open=true;}
  close(){this.open=false;for(const listener of this.listeners.close??[])listener();}
  focus(){this.focused=true;}
  getContext(kind,options){this.contextOptions=options;const noop=()=>{};return {clearRect:noop,beginPath:noop,moveTo:noop,lineTo:noop,closePath:noop,fill:noop,stroke:noop,setLineDash:noop,fillText:text=>this.drawn.push(text)};}
}
const fields=['histogram','channel','red','green','blue','luminance','log_scale','auto_update','sdr_white','inspection_preparing','inspection_updating','inspection_current','inspection_failed','inspection_help','inspection_hdr_help','refresh'];
function harness(t) {
  const doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};doc.body=new Element('body',doc);
  t.mock.method(globalThis,'setInterval',()=>1);t.mock.method(globalThis,'clearInterval',()=>{});
  const prior=globalThis.document;globalThis.document=doc;t.after(()=>{globalThis.document=prior;});
  let tag='en',revision=2;
  const jobs=[],controls=[],captions=[];
  const app={language_tag:()=>tag,catalog:()=>({native_copy:{color:Object.fromEntries(fields.map(name=>[name,`${tag}:${name}`]))}}),bootstrap_view:()=>({common:{close:`${tag}:close`}}),document_color_copy:()=>Object.fromEntries(['depth_8','depth_16','depth_float16','depth_float32'].map(name=>[name,`${tag}:${name}`])),
    state:()=>({document_file:{epoch:1,revision}}),capture_control(){const control={cancelledValue:false,cancel(){this.cancelledValue=true;},cancelled(){return this.cancelledValue;},free(){this.freed=true;}};controls.push(control);return control;},
    histogram:()=>new Promise((resolve,reject)=>jobs.push({resolve,reject})),native_caption(value){captions.push(value);return `${tag}:${value.type}:${Object.entries(value).filter(([key])=>key!=='type').map(([key,value])=>`${key}=${value}`).join(',')}`;}};
  const element=(name,cls,text)=>{const node=new Element(name,doc);node.className=cls;if(text!=null)node.textContent=text;return node;};
  const button=(text,action)=>{const node=element('button','',text);node.click=action;return node;};
  const histogram=createHistogram({app,element,button});
  return {histogram,app,doc,jobs,controls,captions,switch(next){tag=next;refreshCopy(app);histogram.localize();},change(){revision++;},nodes(){const root=doc.body.children[0],canvas=root.children[2],select=root.children[1].children[0],log=root.children[1].children[1].children[0],automatic=root.children[1].children[2].children[0];return {root,canvas,select,log,automatic};}};
}
function result(depth='F16',revision=2) {return {epoch:1,revision,sampled_time:null,axis:{bins:[0,4],stops:depth==='F16'?[-2,1]:null,white:depth==='F16'?0.5:null},histogram:{color:{space:'DisplayP3',depth},pixels:21n,transparent:5n,channels:Array.from({length:4},()=>({bins:[0,1,3,1],below:2n,above:1n,black:3n,white:4n}))}};}
const settle=()=>new Promise(resolve=>setImmediate(resolve));

test('language publication retains controls/options and does not repeat a pending or completed inspection',async t=>{
  const h=harness(t);h.histogram.open();const nodes=h.nodes();
  nodes.select.value='2';nodes.log.checked=true;nodes.automatic.checked=false;
  h.switch('tr');assert.equal(h.jobs.length,1);assert.equal(h.controls.length,1);assert.equal(h.controls[0].cancelled(),false);
  assert.equal(nodes.root.ariaLabel,'tr:histogram');assert.equal(nodes.select.ariaLabel,'tr:channel');assert.equal(nodes.select.options[2].textContent,'tr:green');assert.equal(nodes.root.children[5].textContent,'tr:inspection_updating');
  h.jobs[0].resolve(result());await settle();
  assert.deepEqual(nodes.canvas.contextOptions,{willReadFrequently:true});
  assert.match(nodes.root.children[3].textContent,/tr:depth_float16/);assert.match(nodes.root.children[3].textContent,/tr:inspection_pixels:sampled=21,transparent=5/);
  assert.match(nodes.root.children[4].textContent,/G: tr:inspection_channel:below=2,above=1,black=3,white=4/);
  assert.match(nodes.root.children[6].textContent,/tr:inspection_range:start=-2,end=1/);assert.match(nodes.canvas.title,/tr:inspection_hdr_help/);
  assert.equal(nodes.canvas.ariaLabel,'tr:inspection_graph:channel=tr:green');assert.match(nodes.canvas.drawn.at(-1),/tr:sdr_white/);
  h.switch('fr');assert.deepEqual(h.nodes(),nodes);assert.equal(nodes.select.value,'2');assert.equal(nodes.log.checked,true);assert.equal(nodes.automatic.checked,false);assert.equal(h.jobs.length,1);
  assert.match(nodes.root.children[3].textContent,/fr:inspection_pixels/);assert.equal(nodes.root.children[5].textContent,'fr:inspection_current');assert.equal(nodes.canvas.ariaLabel,'fr:inspection_graph:channel=fr:green');
  h.change();h.histogram.localize();assert.match(nodes.root.children[5].textContent,/fr:inspection_changed:status=fr:inspection_current/);assert.equal(h.jobs.length,1);
  await h.histogram.retire();assert.equal(h.doc.body.children.length,0);
});

test('a closed inspection cannot publish pixels into a reopened dialog',async t=>{
  const h=harness(t);h.histogram.open();const first=h.nodes().root;first.close();h.histogram.open();const second=h.nodes();
  h.switch('vi');h.jobs[0].resolve(result());await settle();
  assert.notEqual(second.root,first);assert.equal(second.root.children[3].textContent,'');assert.equal(second.root.children[5].textContent,'vi:inspection_preparing');
  second.root.children[1].children[3].click();assert.equal(h.jobs.length,2);h.jobs[1].resolve(result('U8'));await settle();
  assert.equal(second.canvas.title,'vi:inspection_help');assert.match(second.root.children[3].textContent,/vi:depth_8/);assert.equal(second.root.children[6].textContent,'vi:inspection_help');
  await h.histogram.retire();
});

test('an inspection failure relabels from semantic state without exposing platform diagnostics or rereading pixels',async t=>{
  const h=harness(t),diagnostics=[];t.mock.method(console,'error',error=>diagnostics.push(error));h.histogram.open();h.switch('fr');
  const error=new Error('An untranslated platform diagnostic');h.jobs[0].reject(error);await settle();
  assert.equal(h.nodes().root.children[5].textContent,'fr:inspection_failed');assert.deepEqual(diagnostics,[error]);
  h.switch('ru');assert.equal(h.nodes().root.children[5].textContent,'ru:inspection_failed');assert.equal(h.jobs.length,1);assert.equal(h.controls[0].freed,true);
  await h.histogram.retire();
});
