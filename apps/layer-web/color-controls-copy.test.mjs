import assert from 'node:assert/strict';
import {test} from 'node:test';
import {FakeElement} from './fake-dom.mjs';
import {chooseColor} from './color-controls.js';
import {createEffectPanels} from './effects.js';
import {createEditorPanels} from './editor-panels.js';
import {createSelectionUi} from './selection-masks.js';
import {createDocuments} from './documents.js';
import {bindCopy,refreshCopy,refreshBindings,liveCopy} from './localization.js';

class Element extends FakeElement {
  constructor(tag,doc){super();Object.assign(this,{tagName:tag.toUpperCase(),ownerDocument:doc,children:[],attributes:new Map(),listeners:{},value:'',style:{},dataset:{},selectionStart:0,selectionEnd:0});const classes=new Set();this.classList={add:name=>classes.add(name),remove:name=>classes.delete(name),toggle:(name,on)=>on?classes.add(name):classes.delete(name),contains:name=>classes.has(name)};}
  get firstChild(){return this.children[0];}
  get options(){return this.children.filter(node=>node.tagName==='OPTION');}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join('');}
  set textContent(value){this.children.forEach(node=>node.parentNode=null);this.children=[];this.append(this.ownerDocument.createTextNode(value));}
  get ariaLabel(){return this.getAttribute('aria-label');}
  set ariaLabel(value){this.setAttribute('aria-label',value);}
  insertBefore(node,before){node.parentNode?.children.splice(node.parentNode.children.indexOf(node),1);node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  remove(){this.parentNode?.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  replaceChildren(...nodes){this.children.forEach(node=>node.parentNode=null);this.children=[];this.append(...nodes);}
  focus(){this.ownerDocument.activeElement=this;}
  setSelectionRange(start,end){this.selectionStart=start;this.selectionEnd=end;}
  prepend(...nodes){for(const node of nodes.reverse())this.insertBefore(node,this.firstChild);}
  replaceWith(node){const parent=this.parentNode;parent.insertBefore(node,this);this.remove();}
  getBoundingClientRect(){return{left:0,top:0,right:242,bottom:100,width:242,height:100};}
  showModal(){}
  close(){for(const listener of this.listeners.close??[])listener();}
  querySelectorAll(tags){const wanted=tags.split(',').map(tag=>tag.toUpperCase());return this.children.flatMap(node=>node.tagName?[...(wanted.includes(node.tagName)?[node]:[]),...node.querySelectorAll(tags)]:[]);}
  querySelector(tag){return this.querySelectorAll(tag)[0]??null;}
}
function harness(t,hdr){
  const previous=globalThis.document,doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};doc.body=new Element('body',doc);globalThis.document=doc;t.after(()=>globalThis.document=previous);
  let language='en',forms=0,copies=0,lastForm,lastRequest;const cachedValidation={space:'Srgb',display_space:'Srgb',outside_document:false,outside_display:false,above_white:true};
  const project=copy=>({models:[['document_rgb',`${language}:document_rgb`],['srgb_hex',`${language}:srgb_hex`]],labels:[0,1,2,3].map(i=>`${language}:field${i}`),description:`${language}:${copy.model}`,validation:`${language}:cached-gamut`,error:copy.error?`${language}:invalid-intensity`:null});
  const app={state:()=>({document_file:{epoch:1},colors:{hdr_depth:hdr?'F32':'U8'},layer_tools:{}}),color_panel:()=>({rgb_space:'Srgb',hdr,rendition:null}),language_tag:()=>language,
    catalog:()=>({native_copy:{color:Object.fromEntries(['edit','model','intensity_ev','base','adjusted','use_color'].map(key=>[key,`${language}:${key}`]))}}),bootstrap_view:()=>({common:{cancel:`${language}:cancel`}}),
    color_ui(request){
      if(request.type==='form_copy'){copies++;assert.equal(request.copy,lastForm.copy);return project(request.copy);}
      assert.equal(request.type,'form');forms++;lastRequest=request.request;const invalid=!!request.request.change_intensity_text&&!Number.isFinite(Number(request.request.change_intensity_text));
      const copy={model:request.request.change_model??request.request.model,document_space:'Srgb',validation:cachedValidation,error:invalid?{type:'intensity',detail:{reason:'invalid_number'}}:null};
      lastForm={draft:{...request.request,model:copy.model,fields:request.request.fields??['0.25','0.5','0.75','1'],intensity:hdr?1:null,change_intensity_text:invalid?request.request.change_intensity_text:null},copy,...project(copy),value:{space:'Srgb',rgba:[.25,.5,.75,1]},preview:{rgba:[.25,.5,.75,1]},base_preview:hdr?{rgba:[.1,.2,.3,1]}:null};return lastForm;
    }};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bindCopy(node,text);else node.textContent=text;}return node;};
  const button=(text,action,cls)=>{const node=element('button',cls,text);node.click=()=>{if(!node.disabled)action();};return node;};
  const result=chooseColor({app,color:{space:'Srgb',rgba:[.25,.5,.75,1]},element,button,intensity:hdr?1:null});
  const root=doc.body.children[0],form=root.querySelectorAll('form')[0];return {app,doc,root,form,result,stats:()=>({forms,copies}),view:()=>lastForm,request:()=>lastRequest,switch(tag){language=tag;refreshCopy(app);}};
}

test('color form publication projects cached copy without parsing, replacing controls or altering dirty HDR input',async t=>{
  const h=harness(t,true),controls=h.form.querySelectorAll('select,input,button'),model=controls[0],intensity=controls[1],options=[...model.options];
  intensity.value='１é🎨 invalid';intensity.focus();intensity.oninput();intensity.setSelectionRange(1,6);model.focus();
  assert.equal(h.request().change_intensity_text,intensity.value,'intensity text goes to shared parsing');
  const before=h.stats(),view=h.view();assert.ok(view.value,'failed intensity retains the prior parsed color');const draft=JSON.stringify(view.draft),preview=JSON.stringify(view.preview),text=intensity.value;
  h.switch('tr');assert.equal(h.stats().forms,before.forms);assert.ok(h.stats().copies>before.copies);assert.equal(h.view(),view);assert.equal(JSON.stringify(view.draft),draft);assert.equal(JSON.stringify(view.preview),preview);
  assert.deepEqual(h.form.querySelectorAll('select,input,button'),controls);assert.deepEqual([...model.options],options);assert.equal(model.options[0].textContent,'tr:document_rgb');assert.equal(intensity.value,text);assert.deepEqual([intensity.selectionStart,intensity.selectionEnd],[1,6]);assert.equal(h.doc.activeElement,model);
  assert.equal(h.form.querySelectorAll('p').at(-1).textContent,'tr:invalid-intensity');assert.equal(h.form.querySelectorAll('button').at(-1).disabled,true);
  h.root.close();await h.result;
});

test('SDR form preserves literal fields, selected model and cached preview while relabeling retained options',async t=>{
  const h=harness(t,false),model=h.form.querySelectorAll('select')[0],field=h.form.querySelectorAll('input')[1];model.value='srgb_hex';model.onchange();field.value='#é雪{draft}';field.focus();field.setSelectionRange(2,5);
  const view=h.view(),before=h.stats(),options=[...model.options],preview=h.form.querySelectorAll('div').find(node=>node.className==='color-form-preview').style.background;
  h.switch('vi');assert.equal(h.stats().forms,before.forms);assert.equal(h.view(),view);assert.equal(model.value,'srgb_hex');assert.deepEqual([...model.options],options);assert.equal(model.options[1].textContent,'vi:srgb_hex');assert.equal(field.value,'#é雪{draft}');assert.deepEqual([field.selectionStart,field.selectionEnd],[2,5]);assert.equal(h.doc.activeElement,field);assert.equal(h.form.querySelectorAll('div').find(node=>node.className==='color-form-preview').style.background,preview);
  h.root.close();await h.result;
});

test('effect publication retains page options, color controls and semantic actions while refreshing captions',t=>{
  const previous={document:globalThis.document,setInterval:globalThis.setInterval,clearInterval:globalThis.clearInterval,cancelAnimationFrame:globalThis.cancelAnimationFrame};
  const doc={hidden:true,createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)}),createElementNS:(_,tag)=>new Element(tag,doc),addEventListener(){},removeEventListener(){}};doc.body=new Element('body',doc);let effects;Object.assign(globalThis,{document:doc,setInterval:()=>0,clearInterval(){},cancelAnimationFrame(){}});t.after(()=>{try{effects?.dispose();}finally{Object.assign(globalThis,previous);}});
  let language='en',previews=0;const actions=[],color={space:'Srgb',rgba:[.2,.4,.6,1]},bucketAction={type:'effect',action:{op:'use_color',layer:7,key:'filter_color'}},state={document_file:{epoch:2},filter_catalog_revision:0,localization:{generation:0},filter_categories:[],filter_picker:{search:null,category:null,selected:null,search_label:'Search',empty_label:'Empty'},adjustments:[],layer_properties:{layer:7,enabled:true,title:'Properties',description:'Effect',pages:[{id:'rgb',label:'en:RGB'},{id:'reds',label:'en:Reds'}],page:'reds',controls:[{key:'filter_color',label:'en:Color',section_id:null,kind:{kind:'color'},value:{value:color},color_action:bucketAction},{key:'mode',label:'en:Mode',section_id:null,kind:{kind:'choice',options:['en:First','en:Second']},value:{value:1}}]}};
  const app={catalog:()=>({native_copy:{color:{use_selected:`${language}:Use selected color`}}}),bootstrap_view:()=>({common:{cancel:`${language}:Cancel`}}),state:()=>state,
    color_ui(request){assert.equal(request.type,'preview');previews++;return[{rgba:color.rgba,in_gamut:true}];},poll_filter_previews:()=>({key:0,retained:[],atlas:null,wait_ms:200}),shader_work_pending:()=>false,stroke_recording_status:()=>({label:'Record',recording:false,ready:false,elapsed_seconds:0,raw_events:0})};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bindCopy(node,text);else node.textContent=text;}return node;};
  const button=(text,action,cls)=>{const node=element('button',cls,text);node.click=()=>{if(!node.disabled)action();};return node;},icon=asset=>{const node=element('svg');node.dataset.asset=asset;return node;};
  const panels=new Map(['adjustments','properties','stats'].map(id=>[id,element('div')]));effects=createEffectPanels({app,state:()=>state,panels,element,button,icon,dispatch:action=>actions.push(action),wake(){},catalog:{},contentChanged(){}});effects.refresh();
  const selects=panels.get('properties').querySelectorAll('select'),page=selects[0],choice=selects[1],pageOptions=[...page.options],choiceOptions=[...choice.options],buttons=panels.get('properties').querySelectorAll('button'),bucket=buttons.find(node=>node.dataset.action==='filter-color-bucket'),colorButton=buttons.find(node=>node.className==='property-color'),colorChildren=[...colorButton.children],bucketChildren=[...bucket.children];page.focus();
  language='fr';state.localization.generation++;state.layer_properties.pages.forEach(p=>p.label=`fr:${p.id}`);state.layer_properties.controls[0].label='fr:Color';state.layer_properties.controls[1].label='fr:Mode';state.layer_properties.controls[1].kind.options=['fr:First','fr:Second'];refreshCopy(app);effects.refresh();
  assert.deepEqual(panels.get('properties').querySelectorAll('select'),selects);assert.deepEqual([...page.options],pageOptions);assert.deepEqual([...choice.options],choiceOptions);assert.equal(page.options[1].textContent,'fr:reds');assert.equal(choice.options[1].textContent,'fr:Second');assert.equal(page.value,'reds');assert.equal(choice.value,1);assert.equal(doc.activeElement,page);assert.deepEqual(panels.get('properties').querySelectorAll('button'),buttons);assert.deepEqual(colorButton.children,colorChildren);assert.deepEqual(bucket.children,bucketChildren);assert.equal(colorButton.ariaLabel,'fr:Color');assert.equal(colorButton.title,'fr:Color');assert.equal(bucket.title,'fr:Use selected color');assert.equal(bucket.ariaLabel,'fr:Use selected color');assert.equal(previews,1);assert.deepEqual(actions,[]);bucket.click();assert.equal(actions[0],bucketAction);
  page.value='rgb';page.onchange();assert.deepEqual(actions[1],{type:'effect',action:{op:'select_page',layer:7,page:'rgb'}});state.layer_properties.pages=[{id:'master',label:'fr:Master'}];state.layer_properties.page='master';effects.refresh();assert.equal(page.options.length,1);assert.notEqual(page.options[0],pageOptions[0]);assert.equal(page.value,'master');
});

test('sampler and selection menu publication retains native controls and semantic choices',t=>{
  const previous={document:globalThis.document,window:globalThis.window};
  const doc={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};doc.body=new Element('body',doc);Object.assign(globalThis,{document:doc,window:{addEventListener(){}}});t.after(()=>Object.assign(globalThis,previous));
  let language='en',changes=0;const actions=[],state={document_file:{epoch:1},layer_tools:{tool:'pick_visible',editing_layer:{id:7,mask_selected:false,mask_id:null,description:'en:Layer',blend_label:'en:Normal',load_selection_tooltip:'en:selection',paint_revision:3}},color_picker:{layer:false,can_sample_layer:true,sample_width:5,sample_sizes:[1,5,15,51,101]},tool_settings:[],tool_extra:[],tool_actions:[],commands:[]};
  const app={action_tooltip:(label)=>`${language}:${label}:hint`,catalog:()=>({native_copy:{sampler:{source:`${language}:source`,sample_size:`${language}:size`,visible_color:`${language}:visible`,selected_layer:`${language}:selected`,sizes:state.color_picker.sample_sizes.map(width=>[width,`${language}:${width}`])},tool_controls:{selection_menu:`${language}:Select`,selection_mode:`${language}:mode`}}}),bootstrap_view:()=>({common:{}})};
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bindCopy(node,text);else node.textContent=text;}return node;};
  const button=(text,action,cls)=>{const node=element('button',cls,text);node.click=action;return node;},icon=()=>element('svg'),dispatch=action=>actions.push(action);
  const selectionUi=createSelectionUi({app,state:()=>state,element,button,icon,dispatch});const editor=createEditorPanels({selectionUi,app,state:()=>state,element,button,icon,dispatch,asset:path=>path,contentChanged:()=>changes++});const root=editor.control('tool_settings'),selects=root.querySelectorAll('select'),source=selects[0],size=selects[1],options=selects.map(node=>[...node.options]),children=[...root.children];size.focus();
  state.color_picker.layer=true;state.color_picker.sample_width=51;editor.refresh();assert.deepEqual(root.children,children);assert.equal(source.value,'true');assert.equal(size.value,'51');assert.equal(changes,1);
  language='vi';Object.assign(state.layer_tools.editing_layer,{description:'vi:Layer',blend_label:'vi:Normal',load_selection_tooltip:'vi:selection',paint_revision:4});refreshCopy(app);editor.refresh();assert.deepEqual(root.querySelectorAll('select'),selects);assert.deepEqual(selects.map(node=>[...node.options]),options);assert.deepEqual(root.children,children);assert.equal(doc.activeElement,size);assert.equal(source.ariaLabel,'vi:source');assert.equal(size.ariaLabel,'vi:size');assert.equal(source.options[1].textContent,'vi:selected');assert.equal(size.options[3].textContent,'vi:51');assert.deepEqual(actions,[]);assert.equal(changes,1);
  size.value='15';size.onchange();source.value='false';source.onchange();assert.deepEqual(actions,[{type:'set_color_sample_size',width:15},{type:'color_picker',action:{kind:'source',layer:false}}]);
  state.layer_tools.tool='select';state.tool_actions=[{command:'selection_new'}];state.commands=[{id:'selection_new',label:'vi:new',icon:'selection',enabled:true,tooltip:'vi:hint'}];editor.refresh();const menu=root.querySelectorAll('button').find(node=>node.className==='selection-menu-button'),menuChildren=[...menu.children];menu.focus();language='fr';Object.assign(state.layer_tools.editing_layer,{description:'fr:Layer',blend_label:'fr:Normal',load_selection_tooltip:'fr:selection',paint_revision:5});state.commands[0].label='fr:new';refreshCopy(app);editor.refresh();assert.equal(root.querySelectorAll('button').find(node=>node.className==='selection-menu-button'),menu);assert.deepEqual(menu.children,menuChildren);assert.equal(menu.textContent,'fr:Select');assert.equal(menu.title,'fr:Select');assert.equal(doc.activeElement,menu);assert.equal(root.children[0].ariaLabel,'fr:mode');
  const literal={type:'invoke',command:'brush',name:'Literal Éİı 雪 {palette}'},items={groups:[{label:'fr:Brush',icon:'brush',action:literal,selected:true,preview:null}],subtools:[{label:'fr:Pen',icon:'pen',action:{type:'invoke',command:'pencil'},selected:true,preview:3}]};state.theme='light';state.tool_panels={tools:items};const tools=editor.control('tools'),toolButtons=tools.querySelectorAll('button'),toolNames=tools.querySelectorAll('span'),images=tools.querySelectorAll('img');toolButtons[1].focus();language='th';state.theme='dark';items.groups[0].label='th:Brush';items.groups[0].selected=false;items.subtools[0].label='th:Pen';refreshCopy(app);editor.refresh();assert.deepEqual(tools.querySelectorAll('button'),toolButtons);assert.deepEqual(tools.querySelectorAll('span'),toolNames);assert.deepEqual(tools.querySelectorAll('img'),images);assert.equal(doc.activeElement,toolButtons[1]);assert.equal(toolButtons[1].ariaLabel,'th:Pen');assert.equal(toolButtons[1].title,'th:th:Pen:hint');assert.equal(toolButtons[1].textContent,'th:Pen');assert.equal(images[0].src,'brush-previews/3-dark.png');assert.equal(toolButtons[0].getAttribute('aria-pressed'),'false');toolButtons[0].click();assert.equal(actions.at(-1),literal);items.groups[0].action={...literal,name:'Another literal'};editor.refresh();assert.notEqual(tools.querySelectorAll('button')[0],toolButtons[0]);items.subtools=[];editor.refresh();refreshCopy(app);assert.equal(tools.querySelectorAll('button').length,1);
});


function documentsHarness(t){
  const previous=Object.fromEntries(['document','window','navigator','innerWidth','innerHeight','ResizeObserver','matchMedia','setInterval','clearInterval'].map(key=>[key,Object.getOwnPropertyDescriptor(globalThis,key)]));
  const doc={hidden:true,createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)}),addEventListener(){},querySelectorAll:()=>[],querySelector:selector=>doc.getElementById(selector.slice(1)),getElementById:id=>[doc.body,...doc.body.querySelectorAll('div,canvas')].find(node=>node.id===id)};doc.body=new Element('body',doc);
  const element=(tag,cls,text)=>{const node=new Element(tag,doc);node.className=cls??'';if(text!=null){if(typeof text==='function')bindCopy(node,text);else node.textContent=text;}return node;},button=(text,action,cls)=>{const node=element('button',cls,text);node.click=action;return node;};
  for(const id of ['document-title','canvas-status','canvas']){const node=element('div');node.id=id;doc.body.append(node);}
  Object.assign(globalThis,{document:doc,window:{addEventListener(){}},innerWidth:1200,innerHeight:900,ResizeObserver:class{observe(){}},matchMedia:()=>({matches:false,addEventListener(){}}),setInterval:()=>0,clearInterval(){}});Object.defineProperty(globalThis,'navigator',{configurable:true,value:{}});
  t.after(()=>{for(const[key,descriptor]of Object.entries(previous))if(descriptor)Object.defineProperty(globalThis,key,descriptor);else delete globalThis[key];});
  let language='en',inspections=0,copyCalls=0,resolveInspection,rejectInspection;const info={sources:[{name:'Literal Éİı 雪 {draft}'}]},requests=[],completed=[],responses=[],messages=[];
  const app={catalog:()=>({native_copy:{color:{},header:{}}}),bootstrap_view:()=>({preparing_document:language+':Preparing',common:{cancel:language+':Cancel',save:language+':Save',done:language+':Done'}}),document_delivery_copy:()=>({cancelling:language+':Cancelling'}),export_copy:()=>({}),document_color_copy:()=>({}),proof_copy:()=>({}),profile_copy:()=>({}),proof_choices_copy:()=>({}),gpu_ready:()=>false,proof_status:()=>({text:'',needed:false}),state:()=>({requests,document_file:{epoch:1},customization:{header_editing:false}}),document_tabs:()=>({selected:1,tabs:[],compact:false}),
    editor_models:()=>({document_options:{unsaved_description:language+':Unsaved',discard_label:language+':Discard'}}),
    document_properties(){inspections++;return new Promise((resolve,reject)=>{resolveInspection=resolve;rejectInspection=reject;});},document_properties_copy(value){assert.equal(value,info);copyCalls++;return{title:language+':Properties',done:language+':Done',rows:[[language+':Canvas','96 × 96 px']],sources:[[info.sources[0].name,language+':Source']]};},
    finish_document(id,success,error){assert.ok(requests.some(request=>request.id===id));completed.push({id,success,error});requests.splice(requests.findIndex(request=>request.id===id),1);return{};},respond_document(id,decision){responses.push({id,decision});return{};}};
  const docs=createDocuments({app,bootstrap:liveCopy(app,'bootstrap_view'),delivery:liveCopy(app,'document_delivery_copy'),state:app.state,canvas:element('canvas'),element,button,icon:()=>element('svg'),dispatch(){},applyChange(){},wake(){},message:error=>messages.push(error),gpuOperation:fn=>fn(),rasterWorker:async()=>{},contentChanged(){}});
  return{app,docs,doc,info,requests,completed,responses,messages,resolve:()=>resolveInspection(info),reject:error=>rejectInspection(error),stats:()=>({inspections,copyCalls}),switch(next){language=next;requests.filter(request=>request.kind.request.type==='confirm_close').forEach(request=>request.kind.request.title=language+':Close Literal Éİı 雪 {draft}');refreshCopy(app);docs.localize();refreshBindings();}};
}

test('document properties retain inspected metadata and native rows while close decisions use current pending copy',async t=>{
  const h=documentsHarness(t),request={id:7,kind:{type:'document',request:{type:'properties'}}};h.requests.push(request);const task=h.docs.handle(request);h.switch('fr');h.resolve();await new Promise(resolve=>setImmediate(resolve));
  const root=h.doc.body.querySelectorAll('dialog')[0],controls=root.querySelectorAll('h2,h3,p,button');assert.equal(controls[0].textContent,'fr:Properties');assert.equal(controls[3].textContent,'Literal Éİı 雪 {draft}');controls.at(-1).focus();h.switch('th');assert.deepEqual(root.querySelectorAll('h2,h3,p,button'),controls);assert.equal(h.doc.activeElement,controls.at(-1));assert.equal(controls[0].textContent,'th:Properties');assert.equal(controls[3].textContent,'Literal Éİı 雪 {draft}');assert.equal(h.stats().inspections,1);controls.at(-1).click();await task;assert.deepEqual(h.completed,[{id:7,success:true,error:undefined}]);
  const close={id:8,kind:{type:'document',request:{type:'confirm_close',title:'th:Close Literal Éİı 雪 {draft}'}}};h.requests.push(close);const closing=h.docs.handle(close),dialog=h.doc.body.querySelectorAll('dialog')[0],buttons=dialog.querySelectorAll('button');buttons[1].focus();h.switch('vi');assert.deepEqual(dialog.querySelectorAll('button'),buttons);assert.equal(h.doc.activeElement,buttons[1]);assert.equal(dialog.querySelector('h2').textContent,'vi:Close Literal Éİı 雪 {draft}');assert.equal(dialog.querySelector('p').textContent,'vi:Unsaved');assert.equal(buttons[1].textContent,'vi:Discard');buttons[0].click();await closing;assert.deepEqual(h.responses,[{id:8,decision:'cancel'}]);
});

test('document async completion retains nominal color failures, literal diagnostics, cancellation and stale ownership',async t=>{
  const h=documentsHarness(t);let id=20;
  for(const error of [{color_feature_error:'ExportDimensions'},{document_host_error:{type:'delivery',reason:{type:'clipboard_empty'}}},{document_host_error:{type:'transport',reason:'switch_dialog'}},'{"document_host_error":{"type":"transport","reason":"switch_dialog"}}',new Error('Literal OS diagnostic Éİı 雪 {draft}'),new DOMException('Literal cancellation','AbortError')]){const request={id:id++,kind:{type:'document',request:{type:'properties'}}};h.requests.push(request);const task=h.docs.handle(request);h.switch('tr');h.reject(error);await task;assert.equal(h.completed.at(-1).error,error?.name==='AbortError'?undefined:error);assert.equal(h.completed.at(-1).success,false);}
  const completed=h.completed.length,error={color_feature_error:'ExportDimensions'},request={id:id++,kind:{type:'document',request:{type:'properties'}}};h.requests.push(request);const task=h.docs.handle(request);h.requests.length=0;h.reject(error);await task;assert.equal(h.messages.at(-1),error);assert.equal(h.completed.length,completed);const retired={id:id++,kind:{type:'document',request:{type:'properties'}}};h.requests.push(retired);const inspected=h.docs.handle(retired);h.requests.length=0;h.resolve();await inspected;assert.equal(h.completed.length,completed);assert.equal(h.doc.body.querySelectorAll('dialog').length,0);
});


test('known clipboard delivery guards and active-document transport refusals retain explicit shared reasons',async t=>{
  const h=documentsHarness(t);h.app.clip_nonce=()=>null;h.app.capture_image_import=()=>({free(){}});const formats='Literal Éİı ไทย 雪 { $name }';h.app.photo_formats=()=>[{name:formats,mime_types:['image/png'],extensions:['png']}];
  const cases=[
    [undefined,{type:'clipboard_unavailable'}],
    [{read:async()=>[]},{type:'clipboard_empty'}],
    [{read:async()=>[{types:['text/plain']}]},{type:'clipboard_formats',formats}],
    [{read:async()=>[{types:['image/png'],getType:async()=>({size:512*1024*1024+1})}]},{type:'clipboard_too_large'}],
  ];
  for(const[index,[clipboard,reason]]of cases.entries()){
    navigator.clipboard=clipboard;const request={id:40+index,kind:{type:'document',request:{type:'paste'}}};h.requests.push(request);await h.docs.handle(request);
    assert.deepEqual(h.completed.at(-1).error,{document_host_error:{type:'delivery',reason}});assert.equal(h.completed.at(-1).success,false);
  }
  let cancelCalls=0,prepareCalls=0,rejectPreparation;h.app.document_delivery_message=()=> 'Literal Éİı 雪.png';h.app.capture_control=()=>({cancel(){cancelCalls++;},cancelled:()=>cancelCalls>0,free(){}});h.app.prepare_images=()=>{prepareCalls++;return new Promise((resolve,reject)=>{rejectPreparation=reject;});};
  navigator.clipboard={read:async()=>[{types:['image/png'],getType:async()=>new Blob(['x'],{type:'image/png'})}]};const importing={id:49,kind:{type:'document',request:{type:'paste'}}};h.requests.push(importing);const preparation=h.docs.handle(importing);await new Promise(resolve=>setImmediate(resolve));
  const progress=h.doc.body.querySelectorAll('aside')[0],label=progress.querySelector('span'),cancel=progress.querySelector('button');cancel.focus();h.switch('fr');assert.equal(progress.querySelector('span'),label);assert.equal(progress.querySelector('button'),cancel);assert.equal(h.doc.activeElement,cancel);assert.equal(label.textContent,'fr:Preparing');assert.equal(cancel.textContent,'fr:Cancel');assert.equal(prepareCalls,1);
  cancel.click();h.switch('th');assert.equal(label.textContent,'th:Cancelling');assert.equal(cancel.textContent,'th:Cancel');assert.equal(h.doc.activeElement,cancel);assert.equal(cancelCalls,1);assert.equal(prepareCalls,1);rejectPreparation(new Error('Literal native cancellation'));await preparation;assert.equal(h.completed.at(-1).error,undefined);
  const request={id:50,kind:{type:'document',request:{type:'properties'}}};h.requests.push(request);const task=h.docs.handle(request);
  const reason={document_host_error:{type:'transport',reason:'switch_dialog'}};await assert.rejects(h.docs.select(2),error=>{assert.deepEqual(error,reason);return true;});await assert.rejects(h.docs.openFiles([]),error=>{assert.deepEqual(error,{document_host_error:{type:'transport',reason:'open_drawings_operation'}});return true;});
  h.requests.length=0;h.reject(reason);await task;assert.deepEqual(h.messages.at(-1),reason);
});


test('trusted storage status reader preserves original literal detail and reprojects without repeating transport work',async t=>{
  const h=documentsHarness(t),error=new Error('Literal Éİı ไทย 雪 { $name }');let spills=0,results=0,formats=0;
  h.app.spill_document_tiles=async()=>{spills++;throw error;};h.app.document_storage_result=detail=>{results++;assert.equal(detail,'Error: Literal Éİı ไทย 雪 { $name }');};
  h.app.document_storage_retained=detail=>{formats++;return h.app.bootstrap_view().common.cancel.split(':')[0]+':Storage '+detail;};h.app.dispatch=()=>{throw new DOMException('Literal native cancellation','AbortError');};
  await h.docs.openFiles([new File(['x'],'Literal Éİı 雪.capy')]);const reader=h.messages.find(message=>typeof message==='function');assert.ok(reader);error.message='Changed later';h.switch('fr');assert.equal(reader(),'fr:Storage Error: Literal Éİı ไทย 雪 { $name }');h.switch('vi');assert.equal(reader(),'vi:Storage Error: Literal Éİı ไทย 雪 { $name }');assert.equal(spills,1);assert.equal(results,1);assert.equal(formats,2);
});


test('Copy Cut and Copy Merged progress reads shared title after the capture handle is consumed',async t=>{
  const h=documentsHarness(t);let cancels=0,runs=0,rejectCopy;
  h.app.capture_control=()=>({cancel(){cancels++;},cancelled:()=>true,free(){}});
  for(const [index,flags]of [{merged:false,cut:false},{merged:false,cut:true},{merged:true,cut:false}].entries()){
    const request={id:70+index,kind:{type:'document',request:{type:'copy',...flags}}};assert.equal(Object.hasOwn(request.kind.request,'title'),false);h.requests.push(request);let consumed=false;
    h.app.document_request_title=id=>h.requests.some(request=>request.id===id)?h.app.bootstrap_view().common.cancel.split(':')[0]+':'+['Copy','Cut','Copy Merged'][index]:undefined;
    h.app.capture_clip=()=>({large(){assert.equal(consumed,false);return true;},run(){consumed=true;runs++;return new Promise((resolve,reject)=>{rejectCopy=reject;});}});
    const task=h.docs.handle(request);await new Promise(resolve=>setImmediate(resolve));const progress=h.doc.body.querySelectorAll('aside')[0],label=progress.querySelector('span'),cancel=progress.querySelector('button');cancel.focus();
    h.switch('fr');assert.equal(label.textContent,'fr:'+['Copy','Cut','Copy Merged'][index]);assert.equal(h.doc.activeElement,cancel);h.switch('th');assert.equal(label.textContent,'th:'+['Copy','Cut','Copy Merged'][index]);assert.equal(progress.querySelector('span'),label);assert.equal(progress.querySelector('button'),cancel);assert.equal(consumed,true);
    cancel.click();h.switch('vi');assert.equal(label.textContent,'vi:Cancelling');assert.equal(cancel.textContent,'vi:Cancel');rejectCopy(new Error('Literal cancelled capture'));await task;refreshBindings();assert.equal(h.completed.at(-1).error,undefined);
  }
  assert.equal(runs,3);assert.equal(cancels,3);
});
