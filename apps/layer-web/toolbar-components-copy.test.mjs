import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement} from './fake-dom.mjs';
import {createToolbarComponent} from './toolbar-components.js';
import {refreshCopy} from './localization.js';

class Element extends FakeElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag,ownerDocument:doc,children:[],attributes:new Map(),listeners:{},dataset:{},style:{setProperty(){}},value:'',isConnected:true});this.classList={add:(...values)=>this.classes(...values),remove:(...values)=>this.classes(...values.map(value=>'!'+value)),contains:value=>(this.className??'').split(' ').includes(value),toggle:(value,on)=>this.classes((on??!this.classList.contains(value))?value:'!'+value)};}
  classes(...values){const classes=new Set((this.className??'').split(' ').filter(Boolean));for(const value of values)value.startsWith('!')?classes.delete(value.slice(1)):classes.add(value);this.className=[...classes].join(' ');}
  get firstChild(){return this.children[0];}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join('');}
  set textContent(value){this.replaceChildren(this.ownerDocument.createTextNode(value));}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  replaceChildren(...nodes){this.children.forEach(node=>node.parentNode=null);this.children=[];this.append(...nodes);}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;this.isConnected=false;}
  removeAttribute(name){this.attributes.delete(name);}
  toggleAttribute(name,on){on?this.setAttribute(name,''):this.removeAttribute(name);}
  dispatch(type,extra={}){for(const handler of this.listeners[type]??[])handler({target:this,preventDefault(){},stopPropagation(){},...extra});}
  click(){this.dispatch('click');}
  focus(){this.ownerDocument.activeElement=this;this.dispatch('focus');}
  select(){this.setSelectionRange(0,this.value.length);}
  setSelectionRange(start,end){this.selectionStart=start;this.selectionEnd=end;}
  contains(node){return this===node||this.children.some(child=>child.contains?.(node));}
  matches(selector){return selector===':popover-open'?!!this.open:selector.startsWith('.')?this.classList.contains(selector.slice(1)):this.tagName===selector;}
  querySelectorAll(selector){return this.children.flatMap(node=>node.tagName?[...(node.matches(selector)?[node]:[]),...node.querySelectorAll(selector)]:[]);}
  querySelector(selector){return this.querySelectorAll(selector)[0]??null;}
  getBoundingClientRect(){return {x:10,y:10,right:110,bottom:110,width:100,height:100};}
  showPopover(){this.open=true;}
  getContext(){return {createImageData:(w,h)=>({data:new Uint8ClampedArray(w*h*4)}),putImageData(){},resetTransform(){},clearRect(){},scale(){},save(){},beginPath(){},rect(){},clip(){},drawImage(){},fillRect(){},restore(){},measureText:text=>({width:text.length*6})};}
}
function harness(t,standalone=false){
  const globals=['document','window','getComputedStyle','innerWidth','innerHeight','devicePixelRatio'],original=Object.fromEntries(globals.map(key=>[key,globalThis[key]])),doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)}),addEventListener(){},removeEventListener(){}};
  doc.createElement=tag=>new Element(tag,doc);Object.assign(globalThis,{document:doc,window:{addEventListener(){},removeEventListener(){}},innerWidth:1000,innerHeight:800,devicePixelRatio:1,getComputedStyle:()=>({color:'#fff',backgroundColor:'#333',fontWeight:'400',fontSize:'12px',fontFamily:'sans'})});
  let root;t.after(()=>{root?.disposeComponent();for(const[key,value]of Object.entries(original))if(value===undefined)delete globalThis[key];else globalThis[key]=value;});
  let language='en',stamps=0,layouts=0,expressions=0;const actions=[],numeric={kind:'slider',min:0,max:100,soft_min:0,soft_max:100,scale:1,step:1,digits:0};
  const formatted=value=>({value,text:String(value),edit:String(value),fill:value/100});
  const app={native_caption:request=>{assert.equal(request.type,'numeric_error');assert.equal(request.reason.reason,'invalid_expression');return`${language}:invalid`;},catalog:()=>({native_copy:{tool_controls:Object.fromEntries(['more_options','remove_bookmark','bookmark_value'].map(key=>[key,`${language}:${key}`]))}}),numeric_labels:label=>({edit:`${language}:edit:${label}`,decrease:'-',increase:'+'}),number_input:request=>formatted(request.value),toolbar_stamp:()=>{stamps++;return{size:1,alpha:[255],extent:[1,1]};},toolbar_ui:request=>{
    if(request.type==='options_layout'){layouts++;return{more:{},fields:language==='en'?request.sizes.map(()=>({width:100,height:30})):request.sizes.map(()=>null)};}if(request.type==='style')return{size:[100,30],labeled:true,gap:4};if(request.type==='numeric_info')return{samples:['0','100'],icon:'size'};if(request.type==='number'){if(request.request.operation.type==='expression'){expressions++;throw{numeric_error:{reason:'invalid_expression'},message:`${language}:invalid`};};return formatted(request.request.value);}if(request.type==='slider_preview')return{side:100,radius:4,caption:{},bookmark:{},icon:16,text:'5 px',viewport:{x:0,y:0,width:100,height:100},opacity:1,stamp:{x:0,y:0,width:100,height:100}};
    throw Error(request.type);
  }};
  const element=(tag,cls,text)=>{const node=doc.createElement(tag);node.className=cls??'';if(text!=null)node.textContent=text;return node;},button=(text,action,cls)=>{const node=element('button',cls,text);node.addEventListener('click',action);return node;},icon=name=>element('svg',name);
  const tile=()=>({id:'test',label:`${language}:slider`,control:{kind:standalone?'brush_size_slider':'tool_options'},component:{context:{type:'brush'},numeric:standalone?{id:'size',label:`${language}:size`,group:`${language}:group`,numeric,value:5}:null,bookmarks:[{value:5,fill:.05,selected:true}],options:standalone?[]:[{Numeric:{id:'size',label:`${language}:size`,group:`${language}:group`,numeric,value:5}},{Choice:{id:'mode',label:`${language}:mode`,items:[{label:`${language}:first`,icon:'a',selected:true,action:{type:'set_name',name:'literal label',label:'literal English user content'}},{label:`${language}:second`,icon:'b',selected:false,action:{type:'set_name',name:'literal second'}}]}},{Action:{checkable:false,state:{id:'run',icon:'run',label:`${language}:run`,tooltip:`${language}:help`,enabled:true,selected:false}}}]}});
  root=createToolbarComponent({app,tile:tile(),view:{tile_style:'small'},element,button,icon,dispatch:action=>actions.push(action),draggable(){},target(){},place(){},panel:'tools'});
  return {app,root,tile,doc,actions,expressions:()=>expressions,layouts:()=>layouts,stamps:()=>stamps,switch(tag){language=tag;refreshCopy(app);root.updateComponent(tile());}};
}

test('toolbar caption publication retains dirty numeric editors, option popup rows and literal action payloads',t=>{
  const h=harness(t);h.root.layoutComponent({width:800,height:30},'horizontal');const number=h.root.querySelector('.number-control'),entry=number.entry,choice=h.root.querySelector('.toolbar-choice');number.valueButton.click();entry.value='1é雪{draft}';entry.dispatch('input');entry.setSelectionRange(1,5);entry.focus();assert.equal(number.commit(),false);const expressions=h.expressions();choice.querySelector('button').click();
  const popup=h.root.querySelector('.toolbar-editor-popover'),rows=popup.querySelectorAll('button'),controls=h.root.querySelectorAll('input'),caption=choice.querySelector('.toolbar-choice-label');
  const layouts=h.layouts();h.switch('fr');assert.equal(h.expressions(),expressions,'publication never reparses retained numeric error');assert.equal(entry.title,'fr:invalid');assert.equal(h.layouts(),layouts,'caption publication does not refit away the focused draft');assert.ok(h.root.querySelector('.number-control')===number,'numeric native control retained');assert.ok(h.root.querySelector('.toolbar-editor-popover')===popup,'native options popup retained');assert.deepEqual(popup.querySelectorAll('button'),rows);assert.deepEqual(h.root.querySelectorAll('input'),controls);assert.equal(entry.value,'1é雪{draft}');assert.deepEqual([entry.selectionStart,entry.selectionEnd],[1,5]);assert.equal(h.doc.activeElement,entry);assert.equal(entry.getAttribute('aria-label'),'fr:size');assert.equal(number.valueButton.getAttribute('aria-label'),'fr:edit:fr:size');assert.ok(choice.querySelector('.toolbar-choice-label')===caption);assert.equal(caption.textContent,'fr:first');assert.equal(rows[1].textContent,'fr:second');assert.equal(choice.getAttribute('aria-label'),'fr:mode');assert.equal(h.root.querySelector('.toolbar-action').querySelector('button').getAttribute('aria-label'),'fr:run');assert.deepEqual(h.actions,[]);
  rows[1].click();assert.deepEqual(h.actions,[{type:'toolbar_edit',context:{type:'brush'},action:{type:'set_name',name:'literal second'}}]);
  const changed=h.tile();changed.component.options[1].Choice.items[0].action.label='literal changed English user content';h.root.updateComponent(changed);assert.ok(h.root.querySelector('.number-control')!==number,'literal action payload changes still rebuild');
  const old=h.root.querySelector('.number-control');h.root.updateComponent({...h.tile(),component:{...h.tile().component,context:{type:'different'}}});assert.ok(h.root.querySelector('.number-control')!==old,'semantic context change still rebuilds');
});

test('an open toolbar number popup retains its draft, selection and native editor across captions',t=>{
  const h=harness(t);h.root.layoutComponent({width:100,height:300},'vertical');h.root.querySelector('.toolbar-number-face').click();const popup=h.root.querySelector('.toolbar-editor-popover'),number=popup.querySelector('.number-control'),entry=number.entry;
  number.valueButton.click();entry.value='12é雪 invalid';entry.dispatch('input');entry.focus();entry.setSelectionRange(2,6);h.switch('de');assert.ok(h.root.querySelector('.toolbar-editor-popover')===popup);assert.ok(popup.querySelector('.number-control')===number);assert.equal(entry.value,'12é雪 invalid');assert.deepEqual([entry.selectionStart,entry.selectionEnd],[2,6]);assert.equal(h.doc.activeElement,entry);assert.equal(entry.getAttribute('aria-label'),'de:size');assert.equal(number.valueButton.getAttribute('aria-label'),'de:edit:de:size');assert.deepEqual(h.actions,[]);
});

test('toolbar bookmark preview keeps its native popup and stamp while captions follow current shared copy',t=>{
  const h=harness(t,true),cap=h.root.querySelector('.toolbar-slider-cap');cap.click();const popup=h.root.querySelector('.toolbar-brush-preview'),bookmark=popup.querySelector('.toolbar-preview-bookmark'),slider=h.root.querySelector('.number-slider');
  assert.equal(bookmark.title,'en:remove_bookmark');h.switch('ru');assert.equal(h.root.querySelector('.toolbar-brush-preview'),popup);assert.equal(popup.querySelector('.toolbar-preview-bookmark'),bookmark);assert.equal(h.root.querySelector('.number-slider'),slider);assert.equal(h.stamps(),1);assert.equal(bookmark.title,'ru:remove_bookmark');assert.equal(bookmark.getAttribute('aria-label'),'ru:remove_bookmark');assert.equal(h.root.querySelector('.toolbar-more').title,'ru:more_options');assert.equal(cap.title,'ru:size');assert.equal(slider.getAttribute('aria-label'),'ru:size');assert.deepEqual(h.actions,[]);
  const next=h.tile();next.component.bookmarks[0].selected=false;h.root.updateComponent(next);assert.equal(bookmark.title,'ru:bookmark_value');bookmark.click();assert.equal(h.actions[0].action.type,'toggle_slider_bookmark');
});
