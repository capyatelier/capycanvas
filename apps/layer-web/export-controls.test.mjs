import {dropdownFactory,dropdownChoices,chooseDropdown} from './dropdown-test-fixture.mjs';
import { bindCopy,refreshCopy,refreshBindings } from "./localization.js";
import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createProfilePicker, chooseSourceProfile, chooseExport, chooseProfileLibrary, localizeProfileLibraries } from "./export-controls.js";
import {colorDialogDom,dialogProjection} from "./color-dialog-fixture.mjs";

const ownerDocument={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};
class FakeElement extends SharedElement {
  constructor(tag, className = "", text = "") {
    super();
    this.tagName = tag.toUpperCase(); this.className = className; this.ownerDocument=ownerDocument;this.children = []; this.parentNode = null; this.attributes = new Map(); this.listeners = {};
    this.dataset={}; this.hidden = false; this.disabled = false; this.value = ""; this.style = {setProperty(name,value){this[name]=value;}};this.textContent=text??"";
  }
  focus(){ownerDocument.activeElement=this;}
  contains(node){for(let n=node;n;n=n.parentNode)if(n===this)return true;return false;}
  get classList(){const read=()=>new Set(this.className.split(/\s+/).filter(Boolean)),write=values=>{this.className=[...values].join(' ');};return {add(...names){const values=read();for(const name of names)values.add(name);write(values);},remove(...names){const values=read();for(const name of names)values.delete(name);write(values);},toggle(name,on){const values=read();on??=!values.has(name);if(on)values.add(name);else values.delete(name);write(values);return on;},contains:name=>read().has(name)};}
  descendants(){return this.querySelectorAll("dialog,form,header,button,input,p,section,div,h2,h3");}
  get childNodes(){return this.children;}
  append(...nodes){for(const node of nodes){if(node.parentNode)node.parentNode.children.splice(node.parentNode.children.indexOf(node),1);node.parentNode=this;this.children.push(node);}}
  prepend(...nodes){for(const node of nodes.reverse())this.insertBefore(node,this.firstChild);}
  getBoundingClientRect(){return {x:0,y:0,left:0,top:0,right:0,bottom:0,width:0,height:0};}
  showModal(){this.open=true;}
  close(){this.open=false;for(const listener of this.listeners.close??[])listener();}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  get firstChild(){return this.children[0];}
  get lastChild(){return this.children.at(-1);}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join("");}
  set textContent(value){this.replaceChildren(...(value===""?[]:[this.ownerDocument.createTextNode(value)]));}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  getContext(){return {putImageData:image=>{this.image=image;}};}
  set ariaLabel(value){this.setAttribute("aria-label",value);}
  get ariaLabel(){return this.getAttribute("aria-label");}
  replaceChildren(...nodes) { for(const node of this.children)node.parentNode=null; this.children = []; this.append(...nodes); }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.tagName === selector.toUpperCase()) return n; return null; }
  querySelector(selector){return this.querySelectorAll(selector)[0]??null;}
  querySelectorAll(selector) {
    const tags = selector.split(",").map(s => s.trim().toUpperCase()), found = [];
    const walk = node => { for (const child of node.children??[]) { if (tags.includes(child.tagName)) found.push(child); walk(child); } };
    walk(this); return found;
  }
  reportValidity() { return true; }
  click() { for (const listener of this.listeners.click ?? []) listener({ target: this }); }
}

const element = (tag, className, text) => {const node=new FakeElement(tag,className,typeof text==="function"?text():text);if(typeof text==="function")bindCopy(node,text);return node;};
const button = (text, action, className = "") => { const node = element("button", className, text); node.addEventListener("click", action); return node; };
const numberField = (control, label, changed) => {
  const root = element("div"), entry = element("input"); root.append(entry); root.entry = entry;
  root.setAttribute = (name, value) => entry.setAttribute(name, value);
  root.update = value => { entry.value = String(value); };
  root.setShowUnits = () => {};
  root.cancelEditing = () => { entry.composing = false; };
  root.commit = () => {
    if (entry.composing || !/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(entry.value)) return false;
    const value = Number(entry.value);
    if (!Number.isFinite(value) || value < control.min || value > control.max) return false;
    changed(value); return true;
  };
  return root;
};
const catalog = readFileSync(new URL("../../assets/locales/en/color-features.ftl", import.meta.url), "utf8") + "\n" + readFileSync(new URL("../../assets/locales/en/common.ftl", import.meta.url), "utf8");
const source = readFileSync(new URL("../../crates/layer-ui/src/color_feature_copy.rs", import.meta.url), "utf8");
function sharedCopy(name) {
  const implementation = source.split(`impl ${name} {`)[1].split("\n}")[0];
  const copy = {};
  for (const [, field, id] of implementation.matchAll(/([a-z_]+): localizer\.text\(MessageId::([A-Z_0-9]+)\)/g)) {
    const key = id.toLowerCase().replaceAll("_", "-");
    const label = catalog.match(new RegExp(`^${key} = (.*)$`, "m"));
    assert.ok(label, `${key} exists in the shared English catalog`);
    copy[field] = label[1];
  }
  copy.dialog={frame:{kind:"editor"},role:"editor",modal:true,primary:"action"};
  copy.library_dialog={frame:{kind:"editor"},role:"editor",modal:true,primary:"close"};
  copy.common = { save:"Save", cancel: "Cancel", done: "Done", apply: "Apply" };
  return copy;
}
const exportCopy = sharedCopy("ExportCopy");
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const profile={name:'Fixture profile',profile:{Icc:[1,2,3]},channels:'Rgb'};
function exportView(overrides={}) {
  const choice=(value,label=value)=>({value,label});
  return {dialog:{frame:{kind:'editor'},role:'editor',modal:true,primary:'action'},copy:{...exportCopy,preset_dialog:{frame:{kind:'alert',prefer_wide:false},role:'editor',modal:true,primary:'action'}},numeric:{dimension:{min:1,max:32768},ppi:{min:1,max:65535},quality:{min:1,max:100}},page:'root',
    fields:{format:'Png',profile:0n,depth:'U16',background:'Preserve',jpeg_quality:90,resolution:'Master',metadata:{keep:'All',remove_location:true},encoding:{conversion:{intent:'RelativeColorimetric'}}},
    profile_revision:1,preview_revision:1,output:'sdr',preview_sdr:false,renditions:[choice(false,'HDR'),choice(true,'SDR')],outputs:[choice('sdr','Shared SDR')],formats:[choice('Png','Shared PNG'),choice('Webp','Shared WebP')],backgrounds:[choice('Preserve','Shared transparent')],depths:[choice('U16','Shared depth')],intents:[choice('RelativeColorimetric','Shared intent')],resolutions:[choice('Master','Shared resolution')],metadata:{label:'Shared metadata',choices:[choice('All','Shared all')],remove_location:'Shared location',note:null},
    bounds:[321,123],enlarge:false,ppi:287,flatten:false,dither:false,clip:false,show_output:true,show_size:false,show_ppi:false,show_color:true,show_background:true,show_depth:true,depth_enabled:true,show_conversion:true,show_dither:true,show_quality:false,show_flatten:false,show_rendition:false,show_clip:false,show_metadata:false,show_location:false,show_metadata_note:false,show_print_profile:false,profile_ready:true,profile_import:true,jpeg_note:false,precision_note:null,compression_note:false,size_summary:'Shared size summary',size_subtitle:'Shared size',color_title:'Shared color',color_subtitle:'Shared profile',preset:0,destination:0,preset_names:['Shared preset'],preset_enabled:[true],preset_labels:['Shared save','Shared update','Shared remove','Shared reset'],preset_save:true,preset_update:false,preset_remove:false,preset_reset:true,preset_busy:false,preset_status:null,preset_error:false,name:'',name_valid:false,error:null,can_export:true,comparison_labels:['Shared master','Shared output'],...overrides};
}
async function openExport(t,initial={},options={}) {
  colorDialogDom(t);const oldImageData=globalThis.ImageData;globalThis.ImageData=class {constructor(pixels,width,height){Object.assign(this,{pixels,width,height});}};t.after(()=>globalThis.ImageData=oldImageData);
  let view=exportView(initial),form,controls,finish;const prompts=[],calls={actions:[],controls:[],previews:[],writes:[],profiles:[]};
  const output={previews:[0,1].map(()=>({extent:[1,1],pixels:[10,20,30,255]})),transparent:false,range_exceeded:false,status:'Prepared fixture'};
  const app={...dialogProjection,dialog_presentation:()=>({kind:options.presentation??'page'}),comparison_copy:()=>({width:160,height:120,gap:12}),load_export_dialog:async()=>new Uint8Array([1]),begin_export_dialog:(id,bytes)=>{assert.equal(id,7);assert.deepEqual([...bytes],[1]);return view;},export_dialog_view:()=>view,export_dialog_preview_copy:()=>'Localized fixture status',
    export_dialog_action:(id,action)=>{assert.equal(id,7);calls.actions.push(action);view=options.action?.(action,view)??view;return view;},export_dialog_profile:(id,index)=>{assert.equal(typeof index,'number');calls.profiles.push([id,index]);return profile;},export_dialog_print_profile:()=>[{Builtin:'DisplayP3'},'Deskproof','Perceptual'],export_dialog_delivery:()=>({format:'Png',destination:4}),
    capture_control:()=>{const control={wasCancelled:false,cancel(){this.wasCancelled=true;},cancelled(){return this.wasCancelled;},free(){this.freed=true;}};calls.controls.push(control);return control;},export_image:(...args)=>{calls.previews.push(args);return options.preview?options.preview(args):Promise.resolve(output);},
    profile_copy:()=>({read_bytes:1024,current:'Current',standard:'Standard',choose:'Choose…',add_profile_dialog:'Import',manage:'Manage'}),profile_name_copy:name=>name,profile_picker_standard:()=>[profile],profile_picker_inventory:async()=>({document:false,current:true,saved:[],standard:false}),profile_library:async()=>[],validate_profile_picker:async value=>({...value,channels:value.channels??'Rgb'}),color_feature_error_copy:String,
    begin_export_preset_write:(id,operation)=>{calls.writes.push([id,operation]);return {store:async()=>{if(options.storeError)throw options.storeError;},free(){calls.freedWrite=true;}};},finish_export_preset_write:(id,write,error)=>{calls.writeResult=error;calls.consumedWrite=true;view=options.finished??view;return view;}};
  const result=chooseExport({app,element,button,icon:name=>element('svg',name),dropdown:dropdownFactory({element,button}),numberField,id:7,gpuOperation:run=>run(),dialog:(_title,build,intent)=>new Promise(resolve=>{
    const body=element('form'),root=element('dialog'),header=element('header'),caption=element('h2');header.append(caption);root.append(header,body);const actions=build(body,resolve,{header,caption,layout(){},dismiss:()=>resolve(null)});
    if(!form){form=body;controls=actions;finish=resolve;}else prompts.push({body,actions,intent,caption});
  })});
  await tick();const find=id=>form.parentNode.querySelectorAll('button,input,div').find(node=>node.id===id);
  return {app,calls,result,form,prompts,find,get controls(){return controls;},publish(next){view={...view,...next};form.localize();},cancel(){controls.onCancel();},finish};
}

test('Export projects shared choices, flags and refusal without deriving format policy',async t=>{
  const d=await openExport(t,{can_export:false,error:'Shared refusal',depth_enabled:false,show_print_profile:true});
  assert.deepEqual(dropdownChoices(d.find('export-format')).map(o=>[o.command,o.label]),[['Png','Shared PNG'],['Webp','Shared WebP']]);
  assert.equal(d.find('export-preset-update').hidden,true);assert.equal(d.find('export-preset-remove').hidden,true);assert.equal(d.find('export-preset-save').hidden,false);
  d.publish({preset_labels:['Shared save after edit','Shared update after edit','Shared remove after edit','Shared reset after edit']});assert.equal(d.find('export-preset-save').textContent,'Shared save after edit');
  const primary=d.find('export-confirm');assert.equal(primary.parentNode.tagName,'HEADER');assert.equal(d.controls.primary(),primary);d.publish({page:'size'});assert.equal(d.controls.primary(),primary);assert.equal(primary.hidden,false);
  assert.equal(d.find('export-depth').disabled,true);assert.equal(d.find('export-confirm').disabled,true);assert.ok(d.form.textContent.includes('Shared refusal'));
  chooseDropdown(d.find('export-format'),'Webp');assert.deepEqual(d.calls.actions.at(-1),{type:'edit',action:{type:'format',value:'Webp'}});
  assert.equal(d.find('export-depth').value,'U16','the host waits for the next shared view rather than enforcing WebP depth');
  d.app.dialog_presentation=()=>({kind:'floating'});assert.equal(d.controls.primary(),null);const close=d.form.parentNode.querySelectorAll('button').find(node=>node.className==='export-window-close');assert.equal(close.hidden,false);d.publish({page:'root'});assert.equal(d.controls.primary(),primary);assert.equal(close.hidden,true);
  d.cancel();assert.equal(await d.result,null);assert.ok(d.calls.controls.every(c=>c.freed));
});

test('Export forwards numeric edits and blocks acceptance of unfinished native text',async t=>{
  const d=await openExport(t,{show_size:true,page:'size'}),width=d.find('export-width').entry;
  width.value='unfinished';d.find('export-confirm').click();assert.equal(d.calls.actions.some(a=>a.type==='bound'),false);
  width.value='444';d.find('export-confirm').click();assert.deepEqual(await d.result,{format:'Png',destination:4});assert.ok(d.calls.actions.some(a=>a.type==='bound'&&a.axis===0&&a.value===444));
});

test('Export publishes only the latest prepared comparison and retires cancelled work',async t=>{
  const pending=[],d=await openExport(t,{}, {preview:()=>new Promise(resolve=>pending.push(resolve))});
  d.publish({preview_revision:2});assert.equal(d.calls.controls[0].wasCancelled,true);
  const output=status=>({previews:[0,1].map(()=>({extent:[1,1],pixels:[0,0,0,255]})),transparent:false,range_exceeded:false,status});
  pending.shift()(output('stale'));await tick();assert.equal(d.form.textContent.includes('stale'),false);assert.equal(pending.length,1);
  pending.shift()(output('latest'));await tick();assert.ok(d.form.textContent.includes('latest'));assert.equal(d.form.querySelectorAll('canvas').length,2);assert.deepEqual(d.calls.previews.map(args=>[args[0],args[1],args[3]]),[[7,null,true],[7,null,true]]);
  const canvases=d.form.querySelectorAll('canvas');d.publish({comparison_labels:['Localized master','Localized output']});assert.deepEqual(d.form.querySelectorAll('canvas'),canvases);assert.ok(d.form.textContent.includes('Localized fixture status'));
  d.cancel();assert.equal(await d.result,null);assert.ok(d.calls.controls.every(c=>c.freed));
});

test('Export applies the shared print profile and intent through typed actions',async t=>{
  const d=await openExport(t,{page:'color',show_print_profile:true,print_profile_name:'Deskproof'});
  d.form.querySelectorAll('button').find(node=>node.textContent.includes(exportCopy.use_print_profile)).click();await tick();
  assert.ok(d.calls.actions.some(a=>a.type==='intent'&&a.value==='Perceptual'));assert.ok(d.calls.actions.some(a=>a.type==='profile'&&a.result.Ok?.name==='Deskproof'&&a.result.Ok.channels==='Rgb'));
  d.cancel();await d.result;
});

test('Export preset writes retain shared errors and consume the storage candidate',async t=>{
  for(const presentation of ['page','floating']){
  const d=await openExport(t,{page:'presets',preset_update:true},{presentation,storeError:'Storage refused',action:(action,view)=>action.type==='begin_preset'?{...view,preset_busy:true,page:'save_preset'}:action.type==='name'?{...view,name:action.value,name_valid:true}:view,finished:exportView({page:'presets',preset_status:'Shared write failure',preset_error:true})});
  d.find('export-preset-save').click();await tick();const prompt=d.prompts[0];assert.equal(prompt.caption.autofocus,presentation==='page');assert.equal(prompt.caption.tabIndex,-1);const entry=prompt.body.querySelectorAll('input')[0];assert.equal(entry.autofocus,presentation==='floating');assert.equal(prompt.actions.primary.disabled,true);entry.value='Literal preset';entry.oninput();assert.equal(prompt.actions.primary.disabled,false);prompt.actions.primary.click();await tick();assert.deepEqual(d.calls.writes,[[7,'save']]);assert.equal(d.calls.writeResult,'Storage refused');assert.equal(d.calls.consumedWrite,true);assert.equal(d.calls.freedWrite,undefined);assert.ok(d.form.textContent.includes('Shared write failure'));
  d.cancel();await d.result;
  }
});

test('Export original profile selection keeps admitted ICC bytes and shared captions',async t=>{
  const d=await openExport(t,{page:'color',original_profiles:[{value:0n,label:'Original: literal 雪'}]}),picker=d.form.querySelectorAll('button').find(node=>node.ariaLabel===exportCopy.profile);
  assert.ok(dropdownChoices(picker).some(choice=>choice.command==='original:0'&&choice.label==='Original: literal 雪'));
  chooseDropdown(picker,'original:0');await tick();
  assert.ok(d.calls.actions.some(action=>action.type==='profile'&&action.result.Ok?.profile===profile.profile));
  d.publish({original_profiles:[{value:0n,label:'Origine: literal 雪'}]});refreshBindings();assert.ok(dropdownChoices(picker).some(choice=>choice.command==='original:0'&&choice.label==='Origine: literal 雪'));
  d.cancel();await d.result;
});

test('profile-library search uses shared metadata matching and Escape waits for its active write',async t=>{
  const {doc}=colorDialogDom(t),body=element('body');doc.body=body;
  const entries=Array.from({length:8},(_,index)=>({id:String(index),entry_copy_source:{id:String(index)},visible:true}));
  let release,language='en';const queries=[];
  const app={...dialogProjection,profile_copy:()=>({library_size:[480,500],search:'Search profiles',manage_title:'Profiles',menu_help:'Help',remove:'Remove',show:'Show',hide:'Hide',import:'Import',use_profile:'Use profile',library_dialog:{frame:{kind:'editor'},role:'editor',modal:true,primary:'close'},common:{done:'Done'}}),language_tag:()=>language,
    profile_options_copy:name=>`Options for ${name}`,profile_entry_copy:entry=>({name:`${language}:profile-${entry.id}`,details:'Details'}),
    profile_library_matches:(entry,query)=>{queries.push([entry.id,query]);return !query||entry.id===query;},
    profile_library:async(type)=>{if(type==='list')return entries;if(type==='remove')await new Promise(done=>release=done);},color_feature_error_copy:String};
  const result=chooseProfileLibrary({app,element,button,icon:()=>element('svg'),dropdown:dropdownFactory({element,button}),manage:true});await new Promise(done=>setImmediate(done));
  const root=body.children.find(node=>node.tagName==='DIALOG'),search=root.querySelectorAll('input')[0];assert.equal(search.hidden,false);
  search.value='3';search.dispatchEvent({type:'input'});assert.equal(root.querySelectorAll('label').length,1);assert.equal(root.querySelectorAll('label')[0].textContent,'en:profile-3');assert.ok(queries.some(([id,query])=>id==='3'&&query==='3'));
  language='fr';localizeProfileLibraries();assert.equal(root.querySelectorAll('label')[0].textContent,'fr:profile-3');assert.equal(search.value,'3');
  const more=root.querySelectorAll('button').find(node=>node.menuModel);assert.deepEqual(more.menuModel().sections.map(section=>section[0].command),['show','remove']);more.menuCommand('remove');
  const cancel={type:'cancel',defaultPrevented:false,preventDefault(){this.defaultPrevented=true;}};root.dispatchEvent(cancel);assert.equal(cancel.defaultPrevented,true);assert.equal(root.open,true);
  release();await new Promise(done=>setImmediate(done));assert.equal(root.querySelectorAll('button').find(node=>node.textContent==='Done').disabled,false);
  root.querySelectorAll('button').find(node=>node.textContent==='Done').click();assert.equal(await result,null);assert.equal(body.children.some(node=>node.tagName==='DIALOG'),false);
  const chosen={name:'Chosen profile',profile:{Builtin:'Srgb'}},library=app.profile_library;app.profile_library=async operation=>operation==='get'?chosen:library(operation);
  const picking=chooseProfileLibrary({app,element,button,icon:()=>element('svg'),dropdown:dropdownFactory({element,button}),manage:false});await new Promise(done=>setImmediate(done));
  const chooser=body.children.find(node=>node.tagName==='DIALOG'),use=chooser.querySelectorAll('button').find(node=>node.textContent===app.profile_copy().use_profile);
  assert.equal(use.tagName,'BUTTON','Saved-profile activation remains a native keyboard-activatable control');use.click();assert.equal(await picking,chosen);

});

test('the common profile picker uses shared channel admission and ignores a retired library read',async t=>{
  const {doc,element,button,icon}=colorDialogDom(t),purpose={kind:'source',channels:'Gray'},accepted=[],changes=[];
  const profile={name:'Literal İı 雪 {profile}',profile:{Icc:[1,2,3]},channels:'Gray'},builtin={name:'Standard',profile:{Builtin:'Srgb'},channels:'Rgb'};
  let pending;const errors=[],bad={name:'Wrong source channels',profile:{Icc:[4]},channels:'Rgb'},context={kind:'interpretation',source:{channels:'Gray'}},validations=[];
  const entries=[{id:'ok',channels:'Gray',entry_copy_source:{name:profile.name}},{id:'incompatible',channels:'Rgb'},
    {id:'hidden',visible:false,channels:'Gray'},{id:'invalid',issue:'Invalid',channels:'Gray'}];
  const app={profile_copy:()=>({read_bytes:1024,document:'Document',saved:'Saved',standard:'Standard',choose:'Choose…',reading:'Reading',add_profile_dialog:'Add Profile…',manage:'Manage Profiles…'}),
    profile_picker_standard:value=>{assert.deepEqual(value,purpose);return[builtin];},
    profile_picker_inventory:async(current,document,value,source,entries)=>{accepted.push([value,entries]);return {document:false,current:false,saved:entries.length?[0]:[],standard:false};},
    profile_entry_copy:entry=>entry,profile_name_copy:name=>name,validate_profile_picker:async(value,kind,source)=>{validations.push([value,kind,source]);if(value===bad)throw bad;return value;},
    profile_library:async operation=>operation==='get'?profile:operation==='import'?bad:pending?await pending:entries};
  const picker=createProfilePicker({app,element,button,icon,dropdown:dropdownFactory({element,button,icon}),label:'Profile',purpose,context,change:value=>changes.push(value),onError:error=>errors.push(error)});
  await new Promise(resolve=>setImmediate(resolve));
  const choices=dropdownChoices(picker);assert.ok(choices.some(choice=>choice.command==='saved:ok'&&choice.label===profile.name));
  assert.equal(choices.some(choice=>choice.command==='saved:incompatible'||choice.command==='saved:hidden'||choice.command==='saved:invalid'),false);
  assert.deepEqual(accepted,[[purpose,entries.map(entry=>({id:entry.id,visible:entry.visible!==false,channels:entry.channels,valid:!entry.issue}))]]);
  assert.equal(choices.some(choice=>choice.is_heading&&choice.label==='Document'||choice.is_heading&&choice.label==='Standard'),false);
  chooseDropdown(picker,'saved:ok');await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(changes,[{pending:true,error:null},{pending:false,error:null}]);
  chooseDropdown(picker,'add');const input=doc.body.children.find(node=>node.tagName==='INPUT');assert.ok(input);
  input.files=[{size:1,arrayBuffer:async()=>new Uint8Array([4]).buffer}];await input.onchange();await new Promise(resolve=>setImmediate(resolve));
  assert.deepEqual(validations,[[profile,purpose,context],[bad,purpose,context]]);assert.deepEqual(errors,[bad]);assert.deepEqual(changes,[{pending:true,error:null},{pending:false,error:null},{pending:true,error:null},{pending:false,error:bad}]);assert.equal(picker.profileValue(),null,'Rejected imports block acceptance');
  chooseDropdown(picker,'saved:ok');assert.equal(picker.profileValue(),null,'Reading blocks acceptance before validation completes');await new Promise(resolve=>setImmediate(resolve));assert.equal(picker.profileValue(),profile,'A valid choice recovers acceptance');
  let admit;app.validate_profile_picker=()=>new Promise(resolve=>admit=resolve);chooseDropdown(picker,'saved:ok');await new Promise(resolve=>setImmediate(resolve));
  picker.updateProfile(builtin,true);await new Promise(resolve=>setImmediate(resolve));admit(profile);await new Promise(resolve=>setImmediate(resolve));
  assert.equal(picker.profileValue(),builtin,'A late profile read cannot replace an explicitly restored profile');
  const library=app.profile_library;app.color_feature_error_copy=error=>error;app.profile_library=async operation=>{if(operation==='list')throw 'Library unavailable';return library(operation);};
  await picker.refreshProfiles();assert.equal(picker.profileValue(),builtin,'A library listing error never revokes an admitted profile');
  assert.ok(dropdownChoices(picker).some(choice=>choice.label==='Library unavailable'&&!choice.enabled));app.profile_library=library;
  let release;pending=new Promise(resolve=>release=resolve);const reading=picker.refreshProfiles(),before=picker.textContent;picker.disposeProfilePicker();release([]);await reading;
  assert.equal(picker.textContent,before);assert.equal(picker.profileValue(),builtin);
});


test('Untagged accepts only an admitted profile and hides its empty error row',async t=>{
  const {element,button,icon}=colorDialogDom(t),profiles=['Srgb','AdobeRgb'].map(space=>({profile:{Builtin:space},name:space,channels:'Rgb'}));
  let form,release,failed=false,finished=false;
  const app={profile_copy:()=>({interpret_title:'Untagged image',interpret_help:'Choose its profile',interpret_as:'Interpret as',use_profile:'Use profile',common:{cancel:'Cancel'},dialog:{frame:{kind:'alert',prefer_wide:true},role:'editor',modal:true,primary:'action'}}),
    profile_picker_standard:()=>profiles,profile_name_copy:name=>name,profile_library:async()=>[],profile_picker_inventory:async()=>({document:false,current:false,saved:[],standard:true}),
    validate_profile_picker:()=>new Promise((resolve,reject)=>{release=()=>failed?reject('Wrong profile'):resolve(profiles[0]);}),color_feature_error_copy:error=>error};
  const result=chooseSourceProfile({app,element,button,icon,dropdown:dropdownFactory({element,button,icon}),interpretation:{channels:'Rgba',profile:{Builtin:'Srgb'}},
    dialog:(_title,build)=>new Promise(resolve=>{form=element('form');build(form,resolve);})});result.then(()=>{finished=true;});
  await new Promise(resolve=>setImmediate(resolve));
  const error=form.descendants().find(node=>node.className==='preferences-error'),picker=form.descendants().find(node=>node.menuModel),use=form.descendants().find(node=>node.textContent==='Use profile');
  assert.equal(error.hidden,true);chooseDropdown(picker,'standard:1');assert.equal(use.disabled,true);
  await new Promise(resolve=>setImmediate(resolve));failed=true;release();await new Promise(resolve=>setImmediate(resolve));
  assert.equal(error.hidden,false);assert.equal(use.disabled,true);use.click();await new Promise(resolve=>setImmediate(resolve));assert.equal(finished,false);
  chooseDropdown(picker,'standard:0');await new Promise(resolve=>setImmediate(resolve));failed=false;release();await new Promise(resolve=>setImmediate(resolve));
  assert.equal(error.hidden,true);assert.equal(use.disabled,false);use.click();assert.deepEqual(await result,{Builtin:'Srgb'});
});
