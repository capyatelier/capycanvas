import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {globSync} from 'node:fs';
import {execFile,execFileSync} from 'node:child_process';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {gzipSync} from 'node:zlib';
import {dropdownChoice} from './dropdown-fixture.mjs';

export function takeStrokeRecording(app) {
  app.stop_stroke_recording();
  const data=Array.from(app.stroke_recording_data());
  app.stroke_recording_saved();
  return data;
}

export const phoneAuditMatrix = [
  {viewport:[412,891],text_pt:11},
  {viewport:[891,412],text_pt:11},
  {viewport:[320,568],text_pt:11},
  {viewport:[320,320],text_pt:16},
  {viewport:[280,560],text_pt:11},
].flatMap(size=>['light','dark'].map(theme=>({...size,theme,dpr:1,text_scale:size.text_pt/11})));

export const dialogAuditMatrix=[600,700,800,1024].flatMap(width=>['light','dark'].map(theme=>({viewport:[width,900],text_pt:11,text_scale:1,dpr:1,theme})));

export function dialogScrollerIssues(root=document) {
  const issues=[],owners=[...root.querySelectorAll('dialog[open],.content-drawer')];
  for(const owner of owners){
    const chrome=[...owner.querySelectorAll('[data-dialog-part="header"],footer')];
    for(const node of [owner,...owner.querySelectorAll('*')]){
      if(!node.getClientRects().length)continue;
      const style=getComputedStyle(node);if(!/auto|scroll/.test(style.overflowY))continue;
      const selector=node.id||node.className||node.tagName;
      if(style.scrollbarWidth!=='none')issues.push({selector,kind:'classic'});
      if(chrome.some(part=>node.contains(part)))issues.push({selector,kind:'scrolling-chrome'});
      if(node.scrollHeight>node.clientHeight+1&&!node.classList.contains('overlay-scroll'))issues.push({selector,kind:'missing-overlay'});
    }
  }
  return issues;
}

export const auditModes={
  audit:{matrix:phoneAuditMatrix},frame:{matrix:phoneAuditMatrix},drawers:{matrix:phoneAuditMatrix},
  menus:{matrix:phoneAuditMatrix},input:{matrix:phoneAuditMatrix,mobile:true},customize:{matrix:phoneAuditMatrix},
  'dialogs-phone':{matrix:phoneAuditMatrix,dialogMode:'phone',defaultCases:['412x891-light','891x412-dark']},
  'dialogs-desktop':{matrix:dialogAuditMatrix,dialogMode:'desktop'},
};

export function selectPhoneDrawerStates(value) {
  if(value===undefined)return null;
  const states=new Set(value.split(',').map(state=>state.trim()).filter(Boolean));
  assert.ok(states.size,'LAYER_PHONE_DRAWER_STATES needs at least one state');
  return states;
}

export async function openPhoneDrawerTile(click,hasDrawer,selector) {
  await click(selector);
  if(!await hasDrawer())await click(selector);
}

export function measurePhoneAuditElements(specifications, root = document) {
  const elements=[],missing=[],ids=new Set();
  for(const {id,role,selector,optional=false} of specifications) {
    if(ids.has(id))throw Error(`Duplicate phone audit ID: ${id}`);
    ids.add(id);
    const nodes=[...root.querySelectorAll(selector)];
    if(nodes.length>1)throw Error(`Ambiguous phone audit element ${id}: ${nodes.length} nodes`);
    if(!nodes.length){if(!optional)missing.push(id);continue;}
    const node=nodes[0],style=getComputedStyle(node),r=node.getBoundingClientRect();
    const visible=!!node.getClientRects().length&&style.visibility!=='hidden'&&style.display!=='none'&&r.width>0&&r.height>0;
    const bounds=visible?[r.x,r.y,r.width,r.height]:null;
    if(bounds&&!bounds.every(Number.isFinite))throw Error(`Nonfinite phone audit bounds: ${id}`);
    const source={selector,dom_id:node.id||null,type:node.tagName.toLowerCase()};
    if((role==='page-action'||role==='preference-value')&&style.font)source.text_style={font:style.font,weight:style.fontWeight,size:style.fontSize,padding:[style.paddingTop,style.paddingRight,style.paddingBottom,style.paddingLeft]};
    if(role==='preference-value'&&visible){source.text=node.textContent;const range=node.ownerDocument.createRange();range.selectNodeContents(node);const text=range.getBoundingClientRect();source.text_bounds=[text.x,text.y,text.width,text.height];}
    if(role==='drawer-panel')source.layout=[node,node.parentElement].filter(Boolean).map(node=>{
      const style=getComputedStyle(node),b=node.getBoundingClientRect();
      return{class:node.className,bounds:[b.x,b.y,b.width,b.height],requested_minimum:node.style.minHeight,
        display:style.display,height:style.height,minimum:style.minHeight,margin:[style.marginTop,style.marginBottom],padding:[style.paddingTop,style.paddingBottom],flex:style.flex,overflow:style.overflow};
    });
    elements.push({id,role,bounds,visible,source});
  }
  return {elements,missing};
}

export function phoneAuditDrawerSpecifications(root = document) {
  const specs=[];
  const selector=node=>{
    const path=[];
    for(let current=node;current;current=current.parentElement){
      if(current.id&&root.querySelectorAll(`#${CSS.escape(current.id)}`).length===1){path.unshift(`#${CSS.escape(current.id)}`);break;}
      path.unshift(`:nth-child(${[...current.parentElement.children].indexOf(current)+1})`);
    }
    return path.join(' > ');
  };
  const add=(id,role,node)=>{if(node)specs.push({id,role,selector:selector(node)});};
  for(const drawer of root.querySelectorAll('.content-drawer')){
    const id=drawer.dataset.drawer==='tool'?'drawer':`drawer.${drawer.dataset.drawer}`;add(id,'drawer',drawer);
    [...drawer.querySelectorAll(':scope > .drawer-contents > .drawer-column')].forEach((column,index)=>add(`${id}.column.${index}`,'drawer-column',column));
    for(const panel of drawer.querySelectorAll(':scope > .drawer-contents > .drawer-column > .panel')){
      const name=[...panel.classList].find(name=>name.endsWith('-panel'))?.slice(0,-6);
      if(name)add(`${id}.panel.${name.split('_').map(part=>part[0].toUpperCase()+part.slice(1)).join('')}`,'drawer-panel',panel.matches('.layers-panel,.palettes-panel,.adjustments-panel')?panel:panel.firstElementChild??panel);
    }
    const inspector=drawer.querySelector('.layer-inspector');
    if(inspector){add(`${id}.panel.Layers`,'drawer-panel',inspector);add(`${id}.panel.Properties`,'drawer-panel',inspector.querySelector('.properties-panel').firstElementChild);}
    add(`${id}.color.wheel`,'color-wheel',drawer.querySelector('.color-wheel-stage'));
    for(const [part,role,query]of [['body','panel-body','.palette-body'],['history','grid','.palette-history'],['scroll','scroll','.palette-scroll'],['footer','toolbar','.palette-footer'],['name','button','.palette-name'],['detail','label','.palette-detail']])add(`${id}.palettes.${part}`,role,drawer.querySelector(`.palettes-panel ${query}`));
    for(const list of drawer.querySelectorAll('.tool-groups,.tool-subtools')){
      const panel=list.closest('[data-control]').dataset.control,kind=list.classList.contains('tool-groups')?'groups':'subtools';
      [...list.children].forEach((row,index)=>{
        const key=`${id}.${panel}.${kind}.${index}`;add(key,'row',row);
        add(`${key}.icon`,'icon',row.querySelector('.tool-caption-icon,.tool-choice-icon'));add(`${key}.label`,'label',row.querySelector('.tool-caption-name,.tool-choice-name'));add(`${key}.preview`,'preview',row.querySelector('.brush-preview'));
      });
    }
    [...drawer.querySelectorAll('.number-control')].forEach((number,index)=>{
      const key=`${id}.number.${index}`;add(key,'number',number);
      for(const [part,query]of [['track','.number-track'],['slider','.number-slider'],['value','.number-value'],['entry','.number-entry'],['title','.number-title']])
        add(`${key}.${part}`,part,number.querySelector(query));
    });
    [...drawer.querySelectorAll('.tool-action-choice,.picker-setting')].forEach((choice,index)=>{
      const key=`${id}.choice.${index}`;
      add(`${key}.label`,'label',choice.querySelector(':scope > span'));
      add(`${key}.control`,'button',choice.querySelector(':scope > .dropdown'));
      add(`${key}.value`,'label',choice.querySelector(':scope > .dropdown .dropdown-label'));
    });
    for(const [key,query]of [['layers','.layer-inspector'],['layers.rows','.layer-rows'],['layers.footer','.layer-inspector-footer'],['color','.color-wheel-control'],['color.disc','.color-wheel-control canvas']])
      add(`${id}.${key}`,'context',drawer.querySelector(query));
  }
  [...root.querySelectorAll('.drawer-bridge')].forEach((node,index)=>add(index===0?'drawer.bridge':`drawer.bridge.${index}`,'bridge',node));
  return specs;
}

export async function writeDrawerAudit(evaluate, path, state) {
  const actual=await evaluate(`(${measurePhoneAuditElements.toString()})((${phoneAuditDrawerSpecifications.toString()})())`);
  const metadata=await evaluate(`(()=>{const s=layerApp.state(),font=getComputedStyle(document.documentElement);return{viewport:[innerWidth,innerHeight],theme:s.theme,dpr:devicePixelRatio,text_scale:parseFloat(font.fontSize)/(11*4/3),phone_active:!!layerApp.app.layout(innerWidth,innerHeight).phone,preset:JSON.parse(layerApp.app.workspace_view()).id,font:font.fontFamily,font_px:parseFloat(font.fontSize)}})()`);
  await writeFile(path,JSON.stringify({schema_version:1,host:'web',state,...metadata,status:actual.missing.length?'missing':'captured',...actual,expected_differences:[]},null,2)+'\n');
}

export function phoneAuditFrameSpecifications(tiles) {
  return [
    ...[
      ['bar.tools','bar','#phone-bar-tools'],['bar.capy','bar','#phone-bar-capy'],
      ['chip.tools','chip','#phone-tools-chip'],['chip.capy','chip','#phone-sliders-chip'],
      ['capy','tile','#phone-frame #zen-capy'],['tile.settings','tile','#phone-tool-settings'],
    ].map(([id,role,selector])=>({id,role,selector})),
    ...tiles.filter(tile=>tile.control.kind!=='divider').map(tile=>({id:`tile.${tile.id}`,role:'tile',selector:`#phone-tool-${tile.id}`})),
    ...['size','strength'].flatMap(name=>{
      const selector=`#phone-frame #component-slider-${name}`;
      return [
        {id:`slider.${name}`,role:'slider',selector},
        {id:`slider.${name}.track`,role:'trough',selector:`${selector} .interval-trough, ${selector} .number-track`},
        {id:`slider.${name}.handle`,role:'slider',selector:`${selector} .interval-thumb.lower, ${selector} .toolbar-slider-thumb`},
      ];
    }),
  ];
}

export const dialogPhoneSpecifications=(frame,parts,phoneActive)=>phoneActive?[...frame,...parts]:parts;

export async function verifyPickerAllocations(reference,candidate){
  const source=fileURLToPath(new URL('../..',import.meta.url));
  const script='import json,sys;from pathlib import Path;from tools.visual.phone_audit import compare,load;a,b=map(Path,sys.argv[1:3]);result=compare(a,b);result["gtk_ids"]=list(load(a)[1]);result["web_ids"]=list(load(b)[1]);print(json.dumps(result))';
  const output=await new Promise((resolve,reject)=>execFile('python3',['-c',script,reference,candidate],{cwd:source,encoding:'utf8'},(error,stdout)=>error?reject(error):resolve(stdout)));
  const report=JSON.parse(output);
  const required=['page','page.header','page.start','page.end','picker.body','picker.catalog','picker.search','picker.choices','picker.row.0','picker.row.0.title','picker.row.0.subtitle'];
  const missing=required.filter(id=>!report.gtk_ids.includes(id)||!report.web_ids.includes(id));
  const picked=id=>id==='page'||id.startsWith('page.')||id.startsWith('picker.');
  const differences=report.allocation_differences.filter(({id})=>picked(id));
  const absent=report.missing.filter(picked);
  assert.deepEqual({metadata:report.metadata_mismatches,unavailable:report.unavailable,missing:[...new Set([...missing,...absent])],differences},
    {metadata:[],unavailable:[],missing:[],differences:[]},`GTK picker allocations: ${reference} versus ${candidate}`);
  return report;
}

const preference=action=>({type:'preferences',action});
const shortcuts={type:'invoke',command:'keyboard_shortcuts'};
async function drawingPresetMenu({evaluate,call,click,settle},action){
  const point=await evaluate(`(()=>{const view=layerApp.state().new_drawing,index=view.drawer.tiles.findIndex(tile=>tile.own&&tile.name==='Canvas study'),node=document.querySelector('[data-new-drawing-preset="'+index+'"]');if(index<0||!node)throw Error('Authored drawing preset is unavailable');node.scrollIntoView({block:'nearest'});const b=node.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});await settle();
  const index=await evaluate(`[...document.querySelectorAll('#new-drawing-dialog .new-drawing-menu:popover-open button')].findIndex(node=>node.firstChild.textContent===layerApp.state().new_drawing.copy[${JSON.stringify(action)}])`);
  assert.ok(index>=0,`Drawing preset offers ${action}`);await click(`#new-drawing-dialog .new-drawing-menu:popover-open button:nth-child(${index+1})`);
}
async function drawingState(evaluate,expected){
  const actual=await evaluate('(()=>{const drawer=layerApp.state().new_drawing?.drawer;return{kind:drawer?.kind??null,save:!!drawer?.save};})()');
  assert.deepEqual(actual,expected,'The requested New Drawing drawer and naming form are actually presented');
}
async function managerInput({evaluate,wait,settle},input){
  await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(input))});layerApp.wake();null`);await settle();
  await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy&&!JSON.parse(layerApp.app.workspace_view()).saving');
}
async function managerPage(context,page,details=false){
  await managerInput(context,{type:'open',page});
  if(page==='toolbar_library'&&!await context.evaluate('JSON.parse(layerApp.app.workspace_view()).rows.length')){
    await managerOffered(context,'this_workspace','save_toolbar');await context.wait('!!document.querySelector("#workspace-item-name")');
    await context.evaluate(`(()=>{const input=document.querySelector('#workspace-item-name');input.value='Canvas tools';input.dispatchEvent(new Event('input',{bubbles:true}));document.querySelector('.workspace-form .suggested-action').click();})()`);
    await managerInput(context,{type:'open',page});
  }
  if(details){
    const id=await context.evaluate('JSON.parse(layerApp.app.workspace_view()).rows[0]?.id');assert.ok(id,'Manager details have an offered row');
    const selector=`.workspace-manager[open] .workspace-choice[data-id=${JSON.stringify(id)}]`;
    await context.wait(`document.querySelector(${JSON.stringify(selector)})?.checkVisibility()`);await context.click(selector);
  }
}
async function managerClose(context){await managerInput(context,{type:'cancel'});await managerInput(context,{type:'dismiss'});}
async function managerWorkspace(context){
  await managerPage(context,'workspaces');
  if(!await context.evaluate('JSON.parse(layerApp.app.workspace_view()).rows.some(row=>row.title==="Canvas study")')){
    await managerInput(context,{type:'form',action:{type:'new'}});await managerInput(context,{type:'submit',name:'Canvas study',description:null,choice:null});
  }
  const id=await context.evaluate('JSON.parse(layerApp.app.workspace_view()).rows.find(row=>row.title==="Canvas study")?.id');assert.ok(id,'Authored workspace is offered');
  await managerInput(context,{type:'select',id});return id;
}
async function managerOffered(context,page,type){
  await managerPage(context,page,true);
  const offered=await context.evaluate(`(()=>{const view=JSON.parse(layerApp.app.workspace_view());return [...(view.details?.actions??[]),...view.rows.flatMap(row=>row.actions)].find(button=>button.enabled&&button.action.type===${JSON.stringify(type)});})()`);
  assert.ok(offered,`Manager offers ${type}`);
  if(offered.primary){await context.click(`.workspace-details [data-action="${type}"]`);return;}
  await context.click('.workspace-details .workspace-more');
  await context.wait(`!![...document.querySelectorAll('.panel-context-menu:popover-open button')].find(row=>row.menuItem?.command===${JSON.stringify(JSON.stringify(offered.action))})`);
  await context.evaluate(`(()=>{const row=[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(row=>row.menuItem?.command===${JSON.stringify(JSON.stringify(offered.action))});row.dataset.workspaceAuditAction='';})()`);
  await context.click('.panel-context-menu:popover-open [data-workspace-audit-action]');
}

async function photoAuditOpen({send,evaluate,wait}){
  await evaluate(`(async()=>{window.phoneAuditOriginal=String(layerApp.app.document_tabs(0).selected);window.phoneAuditPicker=window.showOpenFilePicker;
    const bytes=new Uint8Array(await(await fetch('/pkg/prophoto16.png')).arrayBuffer());window.phoneAuditPhotoBytes=bytes;
    window.showOpenFilePicker=async()=>[{name:'prophoto16.png',async getFile(){return new File([bytes],'prophoto16.png')}}];})()`);
  await send({type:'invoke',command:'open_document'});
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&layerApp.app.document_color().space==="ProPhoto"&&layerApp.app.document_color().depth==="U16"');
  await evaluate('window.phoneAuditPhotoCount=layerApp.app.document_tabs(0).tabs.length');
}
function photoAuditDraft(app,state){
  if(state==='rasterize-source')return null;
  const color=app.document_color_copy(),output=app.export_copy();
  if(state==='change-bit-depth')return{label:output.depth,value:'U8'};
  if(state!=='repair-source-profile')return{label:output.profile,value:'Srgb'};
  const request=app.state().requests.find(item=>item.kind.type==='document'&&item.kind.request.type==='repair_source_profile');
  if(!request)throw Error('Missing source-profile request');
  const choices=app.profile_picker_standard(app.source_dialog_context(request.id).purpose),index=choices.findIndex(profile=>profile.profile.Builtin==='Srgb');
  if(index<0)throw Error('Source profile picker has no sRGB choice');
  return{label:color.correct_profile,value:`standard:${index}`};
}

function colorPresentationAudit(state){
  const app=layerApp.app,color=app.document_color_copy(),profile=app.profile_copy(),output=app.export_copy(),issues=[],bounds=node=>{const r=node.getBoundingClientRect();return[r.x,r.y,r.width,r.height];},check=(condition,message)=>{if(!condition)issues.push(message);};
  if(state==='soft-proof'){
    const root=document.querySelector('.content-drawer .proof-panel'),copy=app.proof_copy(),model=app.proof_form(),panel=app.proof_panel_copy(),picker=root.querySelector(`.dropdown[aria-label=${JSON.stringify(copy.profile_choice)}]`),gamut=root.querySelector(`input[aria-label=${JSON.stringify(copy.gamut_warning)}]`);
    check(!!picker&&!!gamut,'Proof profile and gamut controls exist');
    check(gamut?.disabled===!panel.gamut_available,'Gamut sensitivity follows the shared proof view');
    if(!model.print_settings.profile)check(picker?.textContent.trim()===profile.choose_short,'An empty proof profile uses the native Choose caption');
    return{issues,mode:model.mode,gamut_available:panel.gamut_available,gamut_disabled:gamut?.disabled,profile_caption:picker?.textContent.trim()};
  }
  if(state==='profile-library'){
    const root=document.querySelector('.profile-library[open]'),title=root.querySelector('h2')?.textContent,help=root.querySelector('.settings-description')?.textContent;
    check(title===profile.manage_title,'Profile library uses the native title');check(help===profile.menu_help,'Profile library uses the native explanation');
    return{issues,title,help,rows:[...root.querySelectorAll('.preference-row')].map(node=>({text:node.textContent,bounds:bounds(node)}))};
  }
  if(state==='untagged-source-profile'){
    const root=[...document.querySelectorAll('dialog[open]')].find(node=>node.querySelector(`.dropdown[aria-label=${JSON.stringify(profile.interpret_as)}]`)),row=root.querySelector('.preference-combo-row'),help=root.querySelector('form > p')?.textContent;
    check(help===profile.interpret_help,'Untagged uses the native explanation');check(row?.querySelector('.preference-combo-title')?.textContent===profile.interpret_as,'Untagged has the shared interpretation card');
    return{issues,help,row:row?{text:row.textContent,bounds:bounds(row)}:null};
  }
  const root=document.querySelector('dialog[open] .document-color-content'),comparison=app.comparison_copy(),help=root.querySelector('.document-color-help')?.textContent,detail=root.querySelector('.document-color-detail')?.textContent,figures=[...root.querySelectorAll('.color-comparison figure')],rows=[...root.querySelectorAll('.preference-combo-title')].map(node=>node.textContent);
  const request=app.state().requests.find(item=>item.kind.type==='document'&&['repair_source_profile','rasterize_source'].includes(item.kind.request.type));
  const context=request?app.source_dialog_context(request.id):null;
  const expectedHelp=state==='assign-profile'?color.assign_native_help:state==='convert-color-space'?color.convert_native_help:state==='change-bit-depth'?color.depth_native_help:state==='rasterize-source'?app.rasterize_details():context?.baked?color.source_baked_help:color.source_native_help;
  check(help===expectedHelp,'Color page uses the native help');
  const expectedRows=state==='assign-profile'?[output.profile]:state==='convert-color-space'?[output.profile,color.result,output.intent]:state==='change-bit-depth'?[output.depth]:state==='repair-source-profile'?[color.correct_profile]:[];
  check(JSON.stringify(rows)===JSON.stringify(expectedRows),'Color page has the native card labels');
  check(figures.length===0||figures.length===2,'A color comparison has two previews or the shared unchanged state');
  if(!figures.length)check([...root.querySelectorAll('p')].some(node=>!node.hidden&&node.textContent===color.choose_different)&&root.closest('dialog[open]').querySelector('.suggested-action')?.disabled,'An unchanged color choice shows its shared explanation without an applicable candidate');
  const previews=figures.map((figure,index)=>{const caption=figure.querySelector('figcaption'),canvas=figure.querySelector('canvas'),label=caption?.textContent,captionBounds=caption?bounds(caption):null,canvasBounds=canvas?bounds(canvas):null;
    check(label===(index?comparison.copy.after:comparison.copy.before),'Comparison captions use shared Before/After');
    check(captionBounds&&canvasBounds&&captionBounds[1]+captionBounds[3]<=canvasBounds[1]+.5,'Comparison caption is above its preview');
    check(canvasBounds&&Math.abs(canvasBounds[3]-Number(comparison.height))<=.5,'Comparison preview has the native height');
    return{label,caption:captionBounds,canvas:canvasBounds};});
  if(previews.length===2){check(Math.abs(previews[0].canvas[2]-previews[1].canvas[2])<=.5,'Comparison preview columns have equal widths');check(Math.abs(previews[1].canvas[0]-previews[0].canvas[0]-previews[0].canvas[2]-Number(comparison.gap))<=.5,'Comparison uses the native column gap');}
  if(state!=='repair-source-profile')check([color.compare_before_apply,color.clipped_comparison,color.layered_open,color.rasterize_compare,color.source_clipped,color.rasterize_comparison,color.choose_destination].includes(detail),'Comparison sentence uses the shared native copy');
  return{issues,help,rows,detail,previews};
}
async function photoAuditClose({evaluate,wait}){
  await evaluate('document.querySelector("dialog.document-dialog:not(.file-progress)[open]")?.requestClose()');
  await wait('!document.querySelector("dialog.document-dialog:not(.file-progress)[open]")&&!layerApp.documents.busy()');
  await evaluate('layerApp.documents.close(layerApp.app.document_tabs(0).selected);null');
  const discard=await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label');
  await wait(`[...document.querySelectorAll('dialog[open] button')].some(button=>button.textContent===${JSON.stringify(discard)})`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(button=>button.textContent===${JSON.stringify(discard)}).click()`);
  await wait('layerApp.app.document_tabs(0).tabs.length===phoneAuditPhotoCount-1&&String(layerApp.app.document_tabs(0).selected)===phoneAuditOriginal&&!layerApp.documents.busy()');
  await evaluate('window.showOpenFilePicker=phoneAuditPicker;delete window.phoneAuditPicker;delete window.phoneAuditPhotoBytes;delete window.phoneAuditPhotoCount;delete window.phoneAuditOriginal');
}
async function untaggedAuditOpen({send,evaluate,wait}){
  await evaluate(`(async()=>{window.phoneAuditPicker=window.showOpenFilePicker;window.phoneAuditMissingProfile=layerApp.state().settings.missing_profile;
    const bytes=new Uint8Array(await(await fetch('/pkg/prophoto16.png')).arrayBuffer()),parts=[bytes.slice(0,8)];
    for(let offset=8;offset<bytes.length;){const size=new DataView(bytes.buffer,bytes.byteOffset+offset,4).getUint32(0),tag=new TextDecoder().decode(bytes.slice(offset+4,offset+8));
      if(!['iCCP','sRGB','gAMA','cHRM','cICP'].includes(tag))parts.push(bytes.slice(offset,offset+size+12));offset+=size+12;}
    window.showOpenFilePicker=async()=>[{name:'untagged.png',async getFile(){return new File(parts,'untagged.png')}}];})()`);
  await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:1}});
  await send({type:'invoke',command:'open_document'});
  await wait('!!document.querySelector(`dialog[open] .dropdown[aria-label="Interpret as"]`)');
}
async function untaggedAuditClose({send,evaluate,wait}){
  await evaluate('document.querySelector(`dialog[open]:has(.dropdown[aria-label="Interpret as"])`)?.requestClose()');
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:await evaluate('phoneAuditMissingProfile')}});
  await evaluate('window.showOpenFilePicker=phoneAuditPicker;delete window.phoneAuditPicker;delete window.phoneAuditMissingProfile');
}

async function auditPanel(context,panel){
  await context.send({type:'phone',action:{kind:'panel',panel}});
  await context.wait('!!document.querySelector(".content-drawer")');
  await auditLayerPreviews(context);
}
async function auditLayerPreviews({evaluate,wait}){
  if(await evaluate('!!document.querySelector(".content-drawer .layer-rows")'))
    await wait(`(()=>{const rows=document.querySelector('.content-drawer .layer-rows'),top=Math.max(0,rows.getBoundingClientRect().top);return layerApp.state().layers.filter(layer=>layer.has_thumbnail).every(layer=>{const row=rows.querySelector('.layer-row[data-layer="'+String(layer.id)+'"]');if(!row||!row.checkVisibility())return true;const rect=row.getBoundingClientRect();if(rect.bottom<top||rect.top>innerHeight)return true;return row.querySelector('.layer-thumbnail canvas')?.dataset.previewRevision===String(layerApp.state().document_file.epoch)+':'+String(layer.paint_revision);});})()`);
}
async function auditPopup({click,evaluate,call,settle},selector,context=false){
  if(!context)return click(selector);
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);await settle();
  const point=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});await settle();
}
async function auditSurfaceClose({send,evaluate,settle}){
  await evaluate('[...document.querySelectorAll(":popover-open")].forEach(node=>node.hidePopover())');
  await send({type:'customize',action:{type:'close_expanded'}});
  await evaluate('[...document.querySelectorAll("dialog[open]")].reverse().forEach(node=>node.requestClose())');await settle();
}
async function auditToolClose(c){
  if(await c.evaluate('layerApp.state().commands.find(item=>item.id==="cancel_transform")?.enabled'))await c.send({type:'invoke',command:'cancel_transform'});
  await c.send({type:'select_brush',id:2});await auditSurfaceClose(c);
}
async function auditLayerKind(c,kind){
  if(kind!=='image')await c.evaluate('window.phoneAuditLayerIds=layerApp.state().layers.map(layer=>String(layer.id))');
  if(kind==='image')await auditOwnedImage(c);
  else if(kind==='group')await c.send({type:'layer',action:{op:'new',group:true,clipped:false}});
  else if(kind==='fill'){
    await c.send({type:'effect',action:{op:'insert',effect:'solid_color'}});
  }else if(kind==='mask'){
    await c.send({type:'invoke',command:'select_all'});await c.send({type:'invoke',command:'mask_selection'});
  }else if(kind==='selection'){await c.send({type:'invoke',command:'select_all'});await c.send({type:'invoke',command:'save_selection_layer'});}
  else await c.send({type:'invoke',command:'new_vector_layer'});
  await c.send({type:'layer',action:{op:'cancel_rename'}});await auditPanel(c,'layers');
}
async function auditLayerClose(c){
  if(await c.evaluate('layerApp.state().commands.find(item=>item.id==="return_to_artwork")?.enabled'))await c.send({type:'invoke',command:'return_to_artwork'});
  await auditSurfaceClose(c);await c.send({type:'invoke',command:'undo'});
  assert.deepEqual(await c.evaluate('layerApp.state().layers.map(layer=>String(layer.id))'),await c.evaluate('phoneAuditLayerIds'),'The one authored fixture action is undone');
  if(await c.evaluate('layerApp.state().commands.some(command=>command.id==="deselect"&&command.enabled)'))await c.send({type:'invoke',command:'deselect'});
  await c.send({type:'select_brush',id:2});await c.evaluate('delete window.phoneAuditLayerIds');
}
async function auditEffect(c,effect){
  await c.evaluate('window.phoneAuditLayerIds=layerApp.state().layers.map(layer=>String(layer.id))');
  await c.send({type:'effect',action:{op:'insert',effect}});await auditPanel(c,'properties');
}
async function auditHeldDrag(c,selector,delta,touch=false){
  await c.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);await c.settle();
  const start=await c.evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),end={x:start.x+delta[0],y:start.y+delta[1]};
  if(touch){await c.call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:704,...start}]});await c.call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:704,...end}]});await c.call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});}
  else{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',...start,button:'left',buttons:1,clickCount:1});await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...end,button:'left',buttons:1});}
  await c.settle();
}
async function auditDragClose(c){
  await c.evaluate('window.dispatchEvent(new Event("blur"))');await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',x:2,y:2,button:'left',buttons:0,clickCount:1});
  await auditSurfaceClose(c);
}
async function auditCanvasPoint(c,point){
  return c.evaluate(`(()=>{const camera=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${point[0]}*camera.zoom+camera.translation[0])*r.width/camera.viewport[0],y:r.y+(${point[1]}*camera.zoom+camera.translation[1])*r.height/camera.viewport[1]}})()`);
}
async function auditCanvasDrag(c,from,to,held=false){
  await c.wait('layerApp.app.brush_ready()');
  const start=await auditCanvasPoint(c,from),end=await auditCanvasPoint(c,to);
  await c.call('Input.dispatchMouseEvent',{type:'mousePressed',...start,button:'left',buttons:1,clickCount:1});
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...end,button:'left',buttons:1});
  if(!held)await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',...end,button:'left',buttons:0,clickCount:1});await c.settle();
}
async function auditPlacement(c){
  await c.evaluate(`(async()=>{window.phoneAuditImagePicker=window.showOpenFilePicker;const canvas=new OffscreenCanvas(64,64),context=canvas.getContext('2d'),image=context.createImageData(64,64);for(let y=0;y<64;y++)for(let x=0;x<64;x++)image.data.set((Math.floor(x/16)+Math.floor(y/16))%2===0?[76,112,144,255]:[208,176,128,255],(y*64+x)*4);context.putImageData(image,0,0);const file=new File([await canvas.convertToBlob({type:'image/png'})],'Inventory marks.png',{type:'image/png'});window.showOpenFilePicker=async()=>[{name:file.name,getFile:async()=>file}];})()`);
  await c.send({type:'invoke',command:'import_image'});await c.wait('!layerApp.documents.busy()&&layerApp.state().canvas_bar?.context.kind==="placement"');
}
async function auditOwnedImage(c){
  await c.evaluate('window.phoneAuditLayerIds=layerApp.state().layers.map(layer=>String(layer.id))');
  await auditPlacement(c);await c.send({type:'invoke',command:'apply_transform'});
  await c.wait('!layerApp.documents.busy()&&layerApp.app.document_park_ready()');
  await c.evaluate('window.showOpenFilePicker=phoneAuditImagePicker;delete window.phoneAuditImagePicker');
}
async function auditPlacementClose(c){
  await c.send({type:'invoke',command:'cancel_transform'});await c.wait('!layerApp.documents.busy()&&!layerApp.state().requests.length');
  await c.evaluate('window.showOpenFilePicker=phoneAuditImagePicker;delete window.phoneAuditImagePicker');await auditSurfaceClose(c);
}
async function auditCanvasBar(c,kind){
  if(kind==='selection'||kind==='quickmask'){await c.send({type:'invoke',command:'select'});await c.send({type:'invoke',command:'select_all'});if(kind==='quickmask')await c.send({type:'invoke',command:'quick_mask'});}
  else if(kind==='selectionlayer')await auditLayerKind(c,'selection');
  else if(kind==='layermask')await auditLayerKind(c,'mask');
  else if(kind==='picker'){
    await auditEffect(c,'levels');
    await c.evaluate('layerApp.dispatch({type:"effect",action:layerApp.state().layer_properties.actions.find(item=>item.action.op==="calibrate").action})');
  }else if(kind==='guide'){await c.send({type:'invoke',command:'ruler'});await auditCanvasDrag(c,[300,300],[500,380]);}
  await c.send({type:'customize',action:{type:'close_expanded'}});
  await c.wait('!!layerApp.state().canvas_bar&&!document.querySelector(".canvas-action-bar").classList.contains("suppressed")');
}
async function auditBarMenu(c,menu){
  if(menu==='more')return c.click('.canvas-action-bar-more');
  const selector=`[data-canvas-bar-menu="${menu}"]`;
  if(await c.evaluate(`document.querySelector(${JSON.stringify(selector)}).closest('.canvas-action-bar-item').hidden`)){
    await c.click('.canvas-action-bar-more');const label=await c.evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-label')`);
    await c.evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(node=>node.querySelector('.menu-label')?.textContent===${JSON.stringify(label)}).click()`);
  }else await c.click(selector);
}
async function auditCanvasClose(c){
  const kind=await c.evaluate('layerApp.state().canvas_bar?.context.kind');
  if(kind==='picker')await c.send({type:'invoke',command:'eyedropper'});
  if(['selection_layer','layer_mask','quick_mask'].includes(kind))await c.send({type:'invoke',command:'return_to_artwork'});
  if(kind==='guide')await c.send({type:'invoke',command:'undo'});
  if(await c.evaluate('typeof phoneAuditLayerIds!=="undefined"'))await auditLayerClose(c);
  else{await c.send({type:'invoke',command:'deselect'});await c.send({type:'select_brush',id:2});await auditSurfaceClose(c);}
}
export function installPhoneAuditWorkerBoundary({channel,operation,failure=null,createKind=null}){
  const prototype=Worker.prototype,post=prototype.postMessage;
  let active=true,resolveStarted,captured;
  const held=[],started=new Promise(resolve=>resolveStarted=resolve);
  function intercepted(data,transfer){
    let request=data?.request;
    if(channel==='workspace'&&typeof request==='string')request=JSON.parse(request);
    else if(channel!=='raster'||!request||typeof request!=='object')return post.call(this,data,transfer);
    if(!active||(channel==='workspace'?request.type:request.operation)!==operation||createKind&&!request.batch?.writes.some(write=>write.create&&write.metadata?.kind===createKind))return post.call(this,data,transfer);
    captured??=data.request;resolveStarted();
    if(failure)queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:{id:data.id,error:JSON.stringify(failure)}})));
    else held.push(()=>post.call(this,data,transfer));
  }
  prototype.postMessage=intercepted;
  const release=()=>{active=false;for(const send of held.splice(0))send();};
  return{started,get request(){return captured;},release,restore(){release();if(prototype.postMessage===intercepted)prototype.postMessage=post;}};
}
async function auditBoundary(context,options){
  await context.evaluate(`window.phoneAuditBoundary=(${installPhoneAuditWorkerBoundary.toString()})(${JSON.stringify(options)});null`);
}
async function auditBoundaryRestore({evaluate,wait}){
  const opening=await evaluate(`(()=>{const opening='phoneAuditOpeningTabs'in window;if(opening)document.querySelector('.file-progress[open] button')?.click();window.phoneAuditBoundary?.restore();delete window.phoneAuditBoundary;if('phoneAuditPicker'in window){window.showOpenFilePicker=phoneAuditPicker;delete window.phoneAuditPicker;}if('phoneAuditBrushSize'in window){layerApp.dispatch({type:'set_brush_size',value:phoneAuditBrushSize});delete window.phoneAuditBrushSize;if(JSON.parse(layerApp.app.workspace_view()).status?.retry)layerApp.app.workspace_input(JSON.stringify({type:'retry'}));}return opening;})()`);
  if(opening){await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');await evaluate('delete window.phoneAuditOpeningTabs');}
}
export function failedSavePlacementIssues({status,controls,notice}){
  const issues=controls.filter(control=>control.overlap).map(control=>`overlap:${control.id||'slider'}`);
  for(const [name,actual,expected] of [['center',status.x+status.width/2,notice.x],['width',status.width,notice.max_width],['bottom',status.bottom,notice.y]])if(Math.abs(actual-expected)>1)issues.push(`${name}:${actual}!=${expected}`);
  return issues;
}
async function auditSaveFailureOpen(context){
  const {evaluate,send}=context;
  await evaluate('window.phoneAuditBrushSize=layerApp.state().brush.diameter');
  await auditBoundary(context,{channel:'workspace',operation:'commit',failure:{kind:'failed_write',message:'Owned audit writer failed'}});
  await send({type:'set_brush_size',value:83});
  await evaluate('phoneAuditBoundary.started.then(()=>true)');
  await context.wait('JSON.parse(layerApp.app.workspace_view()).status?.retry&&!JSON.parse(layerApp.app.workspace_view()).saving');
  const bounds=await evaluate(`(()=>{const status=document.querySelector('.workspace-status').getBoundingClientRect(),controls=[...document.querySelectorAll('#zen-capy,.toolbar-slider')].filter(node=>node.checkVisibility()).map(node=>{const b=node.getBoundingClientRect();return{id:node.id,bounds:b.toJSON(),overlap:Math.min(status.right,b.right)>Math.max(status.left,b.left)&&Math.min(status.bottom,b.bottom)>Math.max(status.top,b.top)};});return{status:status.toJSON(),controls,notice:layerApp.app.layout(innerWidth,innerHeight).notice};})()`);
  const presentation=await evaluate(`(()=>{const status=document.querySelector('.workspace-status'),label=status.querySelector(':scope > span'),actions=status.querySelector('.workspace-status-actions'),range=document.createRange();range.selectNodeContents(label);const lines=[...new Set([...range.getClientRects()].filter(rect=>rect.width>0&&rect.height>0).map(rect=>Math.round(rect.top*2)/2))];const probe=document.createElement('span');probe.hidden=true;status.append(probe);const role=name=>{probe.style.backgroundColor='var('+name+')';return getComputedStyle(probe).backgroundColor;},background=role('--bg'),fill=role('--widget-fill'),error=role('--error');probe.remove();return{viewport:[innerWidth,innerHeight],label:{bounds:label.getBoundingClientRect().toJSON(),lines:lines.length,font:parseFloat(getComputedStyle(label).fontSize)},actions:actions.getBoundingClientRect().toJSON(),buttons:[...actions.querySelectorAll('button')].filter(node=>!node.hidden).map(node=>({bounds:node.getBoundingClientRect().toJSON(),font:parseFloat(getComputedStyle(node).fontSize),fill:getComputedStyle(node).backgroundColor})),background:getComputedStyle(status).backgroundColor,backgroundRole:background,fillRole:fill,errorRole:error,foreground:getComputedStyle(label).color,font:parseFloat(getComputedStyle(status).fontSize)};})()`);
  bounds.presentation=presentation;
  const issues=failedSavePlacementIssues(bounds),directory=process.env.LAYER_TEST_ARTIFACTS,name=await evaluate('`${innerWidth}x${innerHeight}-${layerApp.state().settings.theme}`');
  if(presentation.viewport[0]===412&&(presentation.label.lines!==1||presentation.actions.top<presentation.label.bounds.bottom))issues.push('portrait:text-above-actions');
  if(presentation.viewport[0]===891&&Math.min(presentation.label.bounds.bottom,presentation.actions.bottom)<=Math.max(presentation.label.bounds.top,presentation.actions.top))issues.push('landscape:inline');
  if(Math.abs(presentation.actions.right-bounds.status.right)>1)issues.push('actions:end-alignment');
  if(presentation.background!==presentation.backgroundRole)issues.push('background:palette-role');
  if(presentation.foreground!==presentation.errorRole)issues.push('foreground:error-role');
  if(presentation.buttons.some(button=>button.fill!==presentation.fillRole||button.font!==presentation.font)||presentation.label.font!==presentation.font)issues.push('buttons:fill-and-inherited-font');
  await writeFile(`${directory}/workspace-save-status-placement-${name}.json`,JSON.stringify(bounds,null,2));
  if(issues.length){
    const {data}=await context.call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/workspace-save-status-placement-${name}.png`,Buffer.from(data,'base64'));
  }
  assert.deepEqual(issues,[],'Failed-save status consumes the resolved notice rectangle without covering Capy or sliders');
}
async function auditSaveFailureClose(context){
  await context.evaluate('phoneAuditBoundary.release()');await context.click('.workspace-status button:not([hidden])');
  await context.wait('!JSON.parse(layerApp.app.workspace_view()).status?.retry&&!JSON.parse(layerApp.app.workspace_view()).dirty&&!JSON.parse(layerApp.app.workspace_view()).saving');
  await context.evaluate("layerApp.dispatch({type:'set_brush_size',value:phoneAuditBrushSize});delete window.phoneAuditBrushSize");
}
async function auditOpeningProgressOpen(context,command='open_document'){
  const {evaluate,send}=context;
  await evaluate(`(async()=>{window.phoneAuditPicker=window.showOpenFilePicker;window.phoneAuditOpeningTabs=layerApp.app.document_tabs(0).tabs.length;const file=new File([await(await fetch('/pkg/prophoto16.png')).arrayBuffer()],'prophoto16.png');window.showOpenFilePicker=async()=>[{async getFile(){return file;}}];})()`);
  await auditBoundary(context,{channel:'raster',operation:'read'});
  await send({type:'invoke',command});await evaluate('phoneAuditBoundary.started.then(()=>true)');
}
async function auditOpeningProgressClose(context){
  await context.click('.file-progress[open] button');await context.evaluate('phoneAuditBoundary.release()');
  await context.wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&!document.querySelector(".file-progress[open]")');
  assert.equal(await context.evaluate('layerApp.app.document_tabs(0).tabs.length===phoneAuditOpeningTabs'),true,'Cancelling held opening preserves the original drawing tabs');
  await context.evaluate('window.showOpenFilePicker=phoneAuditPicker;delete window.phoneAuditPicker;delete window.phoneAuditOpeningTabs');
}
async function auditRecordingFailureOpen(context){
  await auditPanel(context,'stats');
  await context.evaluate(`window.phoneAuditRecordingPicker=window.showSaveFilePicker;window.phoneAuditRecordingStroke=false;window.showSaveFilePicker=async()=>({async createWritable(){throw Error('Owned recording provider failed');}})`);
  await context.click('.content-drawer [data-control="stroke-recording"]');
  await auditSurfaceClose(context);
  await auditCanvasDrag(context,[300,300],[310,305]);
  await context.evaluate('window.phoneAuditRecordingStroke=true');
  await auditPanel(context,'stats');
  await context.click('.content-drawer [data-control="stroke-recording"]');
  await context.wait('!!document.querySelector("dialog.document-dialog[open]")&&layerApp.app.stroke_recording_status().ready');
}
async function auditRecordingFailureClose({evaluate,wait}){
  assert.equal(await evaluate('layerApp.app.stroke_recording_status().ready'),true,'Provider failure retains the recording');
  await evaluate(`[...document.querySelectorAll('dialog.document-dialog[open] button')].find(node=>node.textContent===layerApp.app.bootstrap_view().ok).click()`);
  await wait('!document.querySelector("dialog.document-dialog[open]")');
  await evaluate(`window.showSaveFilePicker=async()=>({async createWritable(){return{async write(bytes){window.phoneAuditRecordingBytes=bytes.length;},async close(){},async abort(){}}}});document.querySelector('.content-drawer [data-control="stroke-recording"]').click()`);
  await wait('window.phoneAuditRecordingBytes>0&&!layerApp.app.stroke_recording_status().ready');
  await evaluate('window.showSaveFilePicker=phoneAuditRecordingPicker;delete window.phoneAuditRecordingPicker;delete window.phoneAuditRecordingBytes');
}
async function auditRecordingFailureRestore(context){
  await context.evaluate(`if('phoneAuditRecordingPicker'in window){window.showSaveFilePicker=phoneAuditRecordingPicker;delete window.phoneAuditRecordingPicker;}`);
  if(await context.evaluate('window.phoneAuditRecordingStroke&&layerApp.state().commands.some(command=>command.id==="undo"&&command.enabled)'))await context.send({type:'invoke',command:'undo'});
  await context.evaluate('delete window.phoneAuditRecordingStroke');
  await auditSurfaceClose(context);
}
async function auditExportProgressOpen(context){
  await context.send({type:'invoke',command:'export_document'});
  await context.wait('document.querySelectorAll("#export-options .native-comparison canvas").length===2&&!document.querySelector("#export-options [data-dialog-part=end]").disabled');
  await context.evaluate(`window.phoneAuditExportPicker=window.showSaveFilePicker;window.phoneAuditExportWrites=0;window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){return{async write(){phoneAuditExportWrites++;},async close(){},async abort(){}}}})`);
  await auditBoundary(context,{channel:'raster',operation:'output-band'});
  await context.click('#export-options [data-dialog-part=end]');
  await context.evaluate('phoneAuditBoundary.started.then(()=>true)');
}
async function auditExportProgressClose(context){
  await context.click('.file-progress[open] button');await context.evaluate('phoneAuditBoundary.release()');
  await context.wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  assert.equal(await context.evaluate('phoneAuditExportWrites'),0,'Cancelling export preparation never writes to the owned destination');
}
async function auditExportProgressRestore({evaluate,wait}){
  await evaluate('document.querySelector("#export-options[open]")?.requestClose();document.querySelector(".file-progress[open] button")?.click();window.phoneAuditBoundary?.restore();delete window.phoneAuditBoundary');
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await evaluate("if('phoneAuditExportPicker'in window)window.showSaveFilePicker=phoneAuditExportPicker;delete window.phoneAuditExportPicker;delete window.phoneAuditExportWrites");
}
export function installPhoneAuditColorHistoryProvider(app){
  const original=app.prepare_color;let start,releaseResult,active=true;
  const started=new Promise(resolve=>start=resolve),released=new Promise(resolve=>releaseResult=resolve);
  async function prepare(...args){
    const history=app.state().requests.some(request=>request.id===args[0]&&request.kind.type==='document'&&request.kind.request.type==='color_history');
    const candidate=await original.apply(this,args);
    if(history&&active){start();await released;}
    return candidate;
  }
  app.prepare_color=prepare;
  return{started,release:()=>releaseResult(),restore(){active=false;releaseResult();if(app.prepare_color===prepare)app.prepare_color=original;}};
}
async function auditColorProgressOpen(context){
  await context.send({type:'invoke',command:'change_bit_depth'});
  const label=await context.evaluate('layerApp.app.export_copy().depth'),selector=`dialog.document-dialog[open] .dropdown[aria-label=${JSON.stringify(label)}]`;
  await context.wait(`!!document.querySelector(${JSON.stringify(selector)})`);await context.evaluate(dropdownChoice(selector,'U16'));
  await context.wait('document.querySelectorAll(".document-color-content .color-comparison canvas").length===2&&!document.querySelector("dialog.document-dialog[open] .suggested-action").disabled');
  await context.click('dialog.document-dialog[open] .suggested-action');
  await context.wait('!layerApp.documents.busy()&&layerApp.app.document_color().depth==="U16"&&!document.querySelector("dialog.document-dialog[open]")');
  await context.send({type:'invoke',command:'undo'});
  await context.wait('!layerApp.documents.busy()&&layerApp.app.document_color().depth==="U8"&&!document.querySelector("dialog.document-dialog[open]")');
  await context.evaluate(`window.phoneAuditColorHistoryStamp=String(layerApp.app.session_stamp_for(layerApp.app.document_tabs(0).selected).revision);window.phoneAuditColorProvider=(${installPhoneAuditColorHistoryProvider.toString()})(layerApp.app)`);
  await context.send({type:'invoke',command:'redo'});await context.evaluate('phoneAuditColorProvider.started.then(()=>true)');
  assert.equal(await context.evaluate(`document.querySelector('.document-color-content').textContent.includes(layerApp.app.document_color_copy().preparing_help)`),true,'Actual color-history progress is shown while its real candidate is held');
}
async function auditColorProgressClose(context){
  await context.click('dialog.document-dialog[open] footer button');await context.evaluate('phoneAuditColorProvider.release()');
  await context.wait('!layerApp.documents.busy()&&!document.querySelector("dialog.document-dialog[open]")');
  assert.deepEqual(await context.evaluate('({color:layerApp.app.document_color(),revision:String(layerApp.app.session_stamp_for(layerApp.app.document_tabs(0).selected).revision)})'),{color:{space:'Srgb',depth:'U8'},revision:await context.evaluate('phoneAuditColorHistoryStamp')},'Cancelling color history preserves the authored drawing');
}
async function auditColorProgressRestore(context){
  await context.evaluate('document.querySelector("dialog.document-dialog[open] footer button")?.click();document.querySelector("dialog.document-dialog[open]")?.requestClose();window.phoneAuditColorProvider?.restore();delete window.phoneAuditColorProvider;delete window.phoneAuditColorHistoryStamp');
  await context.wait('!layerApp.documents.busy()&&!document.querySelector("dialog.document-dialog[open]")');
  if(await context.evaluate('layerApp.app.document_color().depth==="U16"')){await context.send({type:'invoke',command:'undo'});await context.wait('!layerApp.documents.busy()&&layerApp.app.document_color().depth==="U8"');}
}
async function auditInterruptedWorkspaceOpen(context){
  await managerPage(context,'workspaces');await context.click('.workspace-row-actions .workspace-add');
  await context.wait('!!document.querySelector("#workspace-item-name")');
  await auditBoundary(context,{channel:'workspace',operation:'commit',createKind:'workspace',failure:{kind:'failed_write',message:'Owned delivery interrupted before publication'}});
  await context.evaluate(`(()=>{const input=document.querySelector('#workspace-item-name');input.value='M9 interrupted changes';input.dispatchEvent(new Event('input',{bubbles:true}));document.querySelector('.workspace-form .suggested-action').click();})()`);
  await context.evaluate('phoneAuditBoundary.started.then(()=>true)');
  await context.wait('!JSON.parse(layerApp.app.workspace_view()).saving&&!JSON.parse(layerApp.app.workspace_view()).busy');
  await context.evaluate(`(async()=>{
    const request=phoneAuditBoundary.request,batch=JSON.parse(request).batch;
    if(!batch.writes.some(write=>write.create&&write.claim&&write.metadata?.kind==='workspace'))throw Error('Interrupted fixture requires the actual claimed workspace creation');
    const resources=performance.getEntriesByType('resource').map(row=>row.name),glue=await import(resources.find(name=>/layer_web\\.[a-f0-9]+\\.js$/.test(name))||'./pkg/layer_web.js');
    const database=await new Promise((resolve,reject)=>{const open=indexedDB.open('capycanvas.workspaces',1);open.onsuccess=()=>resolve(open.result);open.onerror=()=>reject(open.error);});
    try{await new Promise((resolve,reject)=>{const transaction=database.transaction('workspace','readwrite',{durability:'strict'}),store=transaction.objectStore('workspace'),read=store.get('database');let failure;
      read.onsuccess=()=>{try{const next=glue.workspace_database(read.result.snapshot,request,true,Date.now(),undefined,0,undefined);
        const receipt=JSON.parse(glue.workspace_database(next.snapshot,JSON.stringify({type:'receipt',operation_id:batch.operation_id}),false,Date.now(),undefined,0,undefined).response);
        const pending=JSON.parse(glue.workspace_database(next.snapshot,'{"type":"pending"}',false,Date.now(),undefined,0,undefined).response);
        if(receipt.value!=null||!pending.value.some(value=>value.operation_id===batch.operation_id))throw Error('Prepared delivery boundary not preserved');
        store.put({id:'database',snapshot:next.snapshot});sessionStorage.setItem('phoneAuditInterrupted',batch.operation_id);
      }catch(error){failure=error;transaction.abort();}};transaction.oncomplete=resolve;transaction.onabort=()=>reject(failure??transaction.error);
    });}finally{database.close();}
    await layerApp.documents.autosave(true);
  })()`);
  await context.reload();
  await context.wait('!!window.layerApp&&JSON.parse(layerApp.app.workspace_view()).interrupted>0&&!!JSON.parse(layerApp.app.workspace_view()).status?.action');
  await context.evaluate('layerApp.documents.startRecovery().then(()=>true)');
  await context.click('.workspace-status .workspace-recovery');
  await context.wait('!!document.querySelector("#workspace-item-choice")');
  await context.evaluate(dropdownChoice('#workspace-item-choice',await context.evaluate('sessionStorage.getItem("phoneAuditInterrupted")')));
  assert.deepEqual(await context.evaluate(`({weight:getComputedStyle(document.querySelector('#workspace-item-choice .dropdown-label')).fontWeight,bottom:getComputedStyle(document.querySelector('.workspace-form[open] .alert-description')).marginBottom})`),{weight:'700',bottom:'12px'},'Recover uses native standalone value weight and body margins');
}
async function auditInterruptedWorkspaceClose(context){
  await context.click('.workspace-form[open] .suggested-action');
  await context.wait('JSON.parse(layerApp.app.workspace_view()).interrupted===0&&!JSON.parse(layerApp.app.workspace_view()).busy&&!JSON.parse(layerApp.app.workspace_view()).saving');
  await context.evaluate('sessionStorage.removeItem("phoneAuditInterrupted")');await managerClose(context);
}
async function auditClipboardProgressOpen(context){
  await context.evaluate('window.phoneAuditClipOriginal=String(layerApp.app.document_tabs(0).selected)');
  await context.send({type:'invoke',command:'new_document'});
  const {createDrawing}=await import('./new-drawing-fixture.mjs');await context.evaluate(createDrawing({width:2048,height:1025,space:'Srgb',depth:'U8',paper:'white',relief:0}));
  await context.wait('!layerApp.documents.busy()&&layerApp.app.brush_ready()');
  await auditBoundary(context,{channel:'raster',operation:'output-band'});
  await context.evaluate('layerApp.canvas.focus()');
  for(const type of ['keyDown','keyUp'])await context.call('Input.dispatchKeyEvent',{type,modifiers:10,key:'C',code:'KeyC',windowsVirtualKeyCode:67});
  await context.evaluate('phoneAuditBoundary.started.then(()=>true)');
}
async function auditClipboardProgressClose(context){
  await context.click('.file-progress[open] button');await context.evaluate('phoneAuditBoundary.release()');
  await context.wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
}
async function auditClipboardProgressRestore(context){
  await auditBoundaryRestore(context);
  if(!await context.evaluate("'phoneAuditClipOriginal'in window"))return;
  if(await context.evaluate('String(layerApp.app.document_tabs(0).selected)!==phoneAuditClipOriginal')){
    await context.evaluate('layerApp.documents.close(layerApp.app.document_tabs(0).selected);null');
    await context.wait('String(layerApp.app.document_tabs(0).selected)===phoneAuditClipOriginal||!!document.querySelector("dialog.document-dialog[open]")');
    if(await context.evaluate('!!document.querySelector("dialog.document-dialog[open]")'))await context.evaluate(`(()=>{const label=layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label,button=[...document.querySelectorAll('dialog[open] button')].find(node=>node.textContent===label);if(!button)throw Error('Owned clipboard drawing has no Discard response');button.click();})()`);
    await context.wait('String(layerApp.app.document_tabs(0).selected)===phoneAuditClipOriginal&&!layerApp.documents.busy()');
  }
  await context.evaluate('delete window.phoneAuditClipOriginal');
}

const auditMenuRow=(id,prepare,selector,context=false,rootselector='.panel-context-menu:popover-open',closeaction=auditSurfaceClose)=>({batch:'m0',id,rootselector,openaction:async c=>{await prepare(c);await auditPopup(c,selector,context);},closeaction});

export const dialogAuditRows=[
  {batch:'m0',id:'properties-paint',collector:'drawer',rootselector:'.content-drawer',openaction:async c=>{await auditPanel(c,'layers');await c.click('.content-drawer .layer-properties-disclosure');},closeaction:auditSurfaceClose},
  {batch:'m0',id:'canvas-bar-clonesource',rootselector:'.canvas-action-bar:not(.suppressed)',openaction:async c=>{await c.send({type:'invoke',command:'clone'});await c.wait('layerApp.state().brush.tool==="clone"&&layerApp.app.brush_ready()');await c.send({type:'invoke',command:'clone_source_arm'});const point=await auditCanvasPoint(c,[300,300]);for(let tap=0;tap<2;tap++){for(const type of ['mousePressed','mouseReleased'])await c.call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await c.settle();if(tap===0)await c.wait('!layerApp.state().commands.find(item=>item.id==="clone_source_arm").selected');}await c.wait('layerApp.state().canvas_bar?.context.kind==="clone_source"');},variants:[{id:'overlay-clone-source',openaction:async()=>{}}],closeaction:async c=>{await c.send({type:'select_brush',id:2});await auditSurfaceClose(c);}},
  {batch:'m0',id:'overlay-vector',rootselector:'#canvas',openaction:async c=>{await auditLayerKind(c,'vector');await c.send({type:'customize',action:{type:'close_expanded'}});await c.send({type:'invoke',command:'figure'});await auditCanvasDrag(c,[300,300],[500,380]);await c.send({type:'invoke',command:'edit_points'});},closeaction:async c=>{await c.send({type:'invoke',command:'undo'});await auditLayerClose(c);}},
  {batch:'m0',id:'canvas-bar-menu-warp',rootselector:'.panel-context-menu:popover-open',openaction:async c=>{await c.send({type:'invoke',command:'select_all'});await c.send({type:'invoke',command:'fill_selection'});await c.wait('layerApp.state().commands.find(item=>item.id==="scale_rotate")?.enabled');await c.send({type:'invoke',command:'scale_rotate'});await c.wait('layerApp.state().layer_tools.tool==="transform"&&layerApp.state().canvas_bar?.context.kind==="transform"');await c.send({type:'invoke',command:'transform_warp'});await c.wait('!!document.querySelector("[data-canvas-bar-menu=warp_grid]")');await auditBarMenu(c,'warp_grid');},closeaction:async c=>{await auditSurfaceClose(c);await c.send({type:'invoke',command:'cancel_transform'});await c.send({type:'invoke',command:'undo'});await c.send({type:'invoke',command:'deselect'});await c.send({type:'select_brush',id:2});}},

  {batch:'m0',id:'canvas-bar-placement',rootselector:'.canvas-action-bar:not(.suppressed)',openaction:auditPlacement,variants:[{id:'overlay-placed-image',openaction:async()=>{}}],closeaction:auditPlacementClose},
  {batch:'m0',id:'canvas-bar-polygon',rootselector:'.canvas-action-bar:not(.suppressed)',openaction:async c=>{await c.send({type:'invoke',command:'polygon_select'});for(const point of [[300,300],[500,300]]){const position=await auditCanvasPoint(c,point);for(const type of ['mousePressed','mouseReleased'])await c.call('Input.dispatchMouseEvent',{type,...position,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});}await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...await auditCanvasPoint(c,[500,500]),button:'none',buttons:0});await c.settle();},closeaction:async c=>{await c.send({type:'invoke',command:'cancel_selection'});await auditCanvasClose(c);}},
  {batch:'m0',id:'overlay-rulers',rootselector:'#canvas',openaction:c=>auditCanvasBar(c,'guide'),closeaction:auditCanvasClose},
  {batch:'m0',id:'overlay-fill-trail',rootselector:'.fill-gesture-overlay:not([hidden])',openaction:async c=>{await c.send({type:'invoke',command:'fill'});await auditCanvasDrag(c,[300,300],[500,380],true);},closeaction:async c=>{await c.evaluate('window.dispatchEvent(new Event("blur"))');await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',x:2,y:2,button:'left',buttons:0,clickCount:1});await c.send({type:'select_brush',id:2});}},

  {batch:'m0',id:'layers-swipe-actions',collector:'drawer',rootselector:'.content-drawer',openaction:async c=>{await auditPanel(c,'layers');await auditHeldDrag(c,'.layer-row[data-layer]:not(.layer-row-unit) .layer-name',[-60,0],true);await c.wait('(()=>{const row=document.querySelector(".layer-row[data-layer]:not(.layer-row-unit)"),offset=parseFloat(row.parentElement.style.getPropertyValue("--swipe"));return offset>0&&row.getAnimations().every(animation=>["finished","idle"].includes(animation.playState))&&Math.abs(new DOMMatrix(getComputedStyle(row).transform).m41+offset)<1})()');},closeaction:auditDragClose},
  {batch:'m0',id:'layers-drag-pickup',rootselector:'.layer-drag-preview',openaction:async c=>{await c.evaluate('window.phoneAuditLayerIds=layerApp.state().layers.map(layer=>String(layer.id))');await c.send({type:'invoke',command:'add_layer'});await auditPanel(c,'layers');await auditHeldDrag(c,'.layer-row[data-layer]:not(.layer-row-unit) .layer-grip',[20,48]);},closeaction:async c=>{await auditDragClose(c);await auditLayerClose(c);}},
  {batch:'m0',id:'swatch-drag',rootselector:'.palette-drag-ghost',openaction:async c=>{await auditPanel(c,'color');await auditHeldDrag(c,'.palette-swatches .palette-tile',[40,20]);},closeaction:auditDragClose},
  {batch:'m0',id:'canvas-notice',rootselector:'.canvas-notice:not([hidden])',openaction:async c=>{await dialogAuditRows.find(row=>row.id==='overlay-vector').openaction(c);await c.send({type:'select_brush',id:2});await auditCanvasDrag(c,[300,300],[500,380]);},closeaction:async c=>{await c.send({type:'invoke',command:'undo'});await auditLayerClose(c);}},

  ...[
    ['numeric-popover','The numeric popover belongs to vertical desktop toolbar components; phone sliders omit its axis fields and ToolSettings uses NumberPopover=false. No existing phone route; desktop-only chrome is outside M0.'],
  ].map(([id,blocked])=>({batch:'m0',id,blocked})),
  {batch:'m0',id:'profile-picker-menu',collector:'menu',rootselector:'.panel-context-menu:popover-open',openaction:async c=>{
    await dialogAuditRows.find(row=>row.id==='soft-proof').openaction(c);
    const label=await c.evaluate('layerApp.app.proof_copy().profile_choice'),selector=`.proof-print .dropdown[aria-label=${JSON.stringify(label)}]`;
    await c.wait(`document.querySelector(${JSON.stringify(selector)}).menuModel().sections.flat().some(row=>row.command?.startsWith('standard:'))`);
    await auditPopup(c,selector);
  },closeaction:auditSurfaceClose},
  {batch:'m0',id:'new-drawing-preset-menu',rootselector:'.new-drawing-menu:popover-open',openaction:async c=>{await dialogAuditRows.find(row=>row.id==='rename-drawing-preset').openaction(c);await c.click('#new-drawing-save-cancel');const index=await c.evaluate('layerApp.state().new_drawing.drawer.tiles.findIndex(tile=>tile.own&&tile.name==="Canvas study")');await auditPopup(c,`[data-new-drawing-preset="${index}"]`,true);},closeaction:async c=>{await c.evaluate('document.querySelector(".new-drawing-menu:popover-open").hidePopover()');await drawingPresetMenu(c,'delete');await c.click('#new-drawing-browser-cancel');await c.click('#new-drawing-cancel');}},
  auditMenuRow('new-drawing-paper-color-menu',async c=>{await c.send({type:'invoke',command:'new_document'});await c.click('#new-drawing-paper-color');},'[data-new-drawing-paper-color="0"]',true),
  {batch:'m0',id:'paper-tile-menu',rootselector:'#texture-dialog .panel-context-menu:popover-open',openaction:async c=>{await dialogAuditRows.find(row=>row.id==='rename-paper').openaction(c);await c.evaluate('document.querySelector("#paper-rename-dialog").requestClose()');await c.wait('!document.querySelector("#paper-rename-dialog")');const index=await c.evaluate('[...document.querySelectorAll("#texture-dialog .texture-tile")].findIndex(tile=>tile.querySelector(".texture-caption").textContent==="Linen scan")');await auditPopup(c,`#texture-dialog .texture-tile[data-texture-tile="${index}"]`,true);},closeaction:async c=>{await c.evaluate('document.querySelector("#texture-dialog .panel-context-menu:popover-open").hidePopover();document.querySelector("#texture-dialog").requestClose()');const id=await c.evaluate('layerApp.state().layer_properties.controls.find(control=>control.paper).paper.tiles.find(tile=>tile.label==="Linen scan").id');await c.send({type:'effect',action:{op:'delete_paper',id}});await auditSurfaceClose(c);}},

  ...['picker','selection','quickmask','selectionlayer','layermask','guide'].map(kind=>({batch:'m0',id:`canvas-bar-${kind}`,rootselector:'.canvas-action-bar:not(.suppressed)',openaction:c=>auditCanvasBar(c,kind),closeaction:auditCanvasClose})),
  ...['more','copy','adjust','mask','refine'].map(menu=>({batch:'m0',id:`canvas-bar-menu-${menu}`,rootselector:'.panel-context-menu:popover-open',openaction:async c=>{await auditCanvasBar(c,'selection');await auditBarMenu(c,menu);},closeaction:auditCanvasClose})),

  ...[['curves-editor','curves'],['gradient-editor','gradient_fill']].map(([id,effect])=>({batch:'m0',id,collector:'drawer',rootselector:'.content-drawer',openaction:c=>auditEffect(c,effect),variants:id==='curves-editor'?[{id:'properties-filter',openaction:async()=>{}}]:[],closeaction:auditLayerClose})),
  ...[['properties-actions-menu','[data-property-action-group]','.toolbar-choice-menu:popover-open'],['properties-add-filter-menu','#properties-add-filter','.panel-context-menu:popover-open']].map(([id,selector,rootselector])=>({batch:'m0',id,rootselector,openaction:async c=>{if(id==='properties-actions-menu')await auditEffect(c,'levels');else await auditPanel(c,'properties');await c.click(selector);},variants:id==='properties-actions-menu'?[{id:'property-picker',openaction:async()=>{}}]:[],closeaction:id==='properties-actions-menu'?auditLayerClose:auditSurfaceClose})),
  {batch:'m0',id:'profile-library-row-menu',collector:'menu',rootselector:'.panel-context-menu:popover-open',openaction:async c=>{
    await c.evaluate(`(async()=>{if((await layerApp.app.profile_library('list')).length)throw Error('The owned profile library starts empty');const response=await fetch('/pkg/proof-p3.icc');if(!response.ok)throw Error('Missing existing P3 ICC fixture');await layerApp.app.profile_library('import',undefined,new Uint8Array(await response.arrayBuffer()));const entries=await layerApp.app.profile_library('list');if(entries.length!==1)throw Error('The owned P3 profile is the only library row');window.phoneAuditProfileId=entries[0].id;})()`);
    await dialogAuditRows.find(row=>row.id==='profile-library').openaction(c);
    await c.wait('!!document.querySelector(".profile-library .profile-library-more:not(:disabled)")');await auditPopup(c,'.profile-library .profile-library-more');
  },closeaction:async c=>{await auditSurfaceClose(c);await c.evaluate(`(async()=>{await layerApp.app.profile_library('remove',phoneAuditProfileId);delete window.phoneAuditProfileId;})()`);}},

  ...['vector','selection','fill','mask','image','group'].flatMap(kind=>[
    {batch:'m0',id:`layers-${kind}`,collector:'drawer',rootselector:'.content-drawer',openaction:c=>auditLayerKind(c,kind),closeaction:auditLayerClose},
    {batch:'m0',id:`properties-${kind}`,collector:'drawer',rootselector:'.content-drawer',openaction:async c=>{await auditLayerKind(c,kind);await c.click('.content-drawer .layer-properties-disclosure');await c.wait('getComputedStyle(document.querySelector(".content-drawer .layer-properties-pages > .layers-panel")).visibility==="hidden"');},closeaction:auditLayerClose},
  ]),
  {batch:'m0',id:'layers-inline-rename',collector:'drawer',rootselector:'.content-drawer',openaction:async c=>{await auditPanel(c,'layers');await c.evaluate('layerApp.dispatch({type:"layer",action:{op:"begin_rename",id:layerApp.state().layer_tools.editing_layer.id}});null');await c.wait('layerApp.state().layer_tools.rename_layer===layerApp.state().layer_tools.editing_layer.id&&!!document.querySelector(".content-drawer .layer-name-entry")');},closeaction:async c=>{await c.send({type:'layer',action:{op:'cancel_rename'}});await auditSurfaceClose(c);}},
  {batch:'m0',id:'layers-opacity-popover',rootselector:'.layer-opacity-popup:popover-open',openaction:async c=>{await auditPanel(c,'layers');const point=await c.evaluate('(()=>{const r=document.querySelector(".layer-row[data-layer]:not(.layer-row-unit) .layer-name").getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()');await c.call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:701,...point,x:point.x-15},{id:702,x:point.x+15,y:point.y}]});await c.call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await c.settle();},closeaction:auditSurfaceClose},

  ...[
    ['layers-row-menu','.layer-row[data-layer]:not(.layer-row-unit)',true],
    ['layers-blend-menu','.layer-blend',false],
    ['layers-add-filter-menu','#layer-add-filter',false],
  ].map(([id,selector,context])=>auditMenuRow(id,c=>auditPanel(c,'layers'),selector,context)),
  ...[
    ['color-slot-menu','[data-color-slot="foreground"]',true],['color-wheel-menu','.color-wheel',true],
    ['palette-library-menu','.palette-library-add',false],['palette-row-menu','.palette-choice',true],
    ['palette-swatch-menu','.palette-swatches .palette-tile',true],
  ].map(([id,selector,context])=>auditMenuRow(id,async c=>{await auditPanel(c,'color');if(id==='palette-library-menu'||id==='palette-row-menu')await c.click('.palette-selector');},selector,context)),
  auditMenuRow('selection-actions-menu',async c=>{await c.send({type:'invoke',command:'select'});await auditPanel(c,'tool_settings');},'.selection-menu-button',false,'.panel-context-menu:popover-open',auditToolClose),
  auditMenuRow('tool-action-dropdown',async c=>{await c.send({type:'invoke',command:'crop'});await auditPanel(c,'tool_settings');},'[data-tool-action-group]',false,'.panel-context-menu:popover-open',auditToolClose),
  auditMenuRow('color-model-popover',c=>c.send({type:'invoke',command:'edit_color'}),'[data-color-format="0"]',false,'.color-format-menu:not([hidden])'),
  auditMenuRow('new-drawing-resolution-menu',c=>c.send({type:'invoke',command:'new_document'}),'#new-drawing-resolution-choices'),
  auditMenuRow('keymap-menu',c=>c.send(shortcuts),'#keymap-menu > summary',false,'#keymap-menu[open]'),
  auditMenuRow('preferences-reset-menu',c=>c.send({type:'open_settings',page:'appearance'}),'[data-preference="transparency"]',true,'#preference-context-menu:popover-open'),
  auditMenuRow('workspace-row-menu',async c=>{await managerPage(c,'workspaces');},'.workspace-grip'),
  {batch:'m0',id:'command-search-parameter',rootselector:'#command-bar[open]',openaction:async({send,wait})=>{await send({type:'invoke',command:'search_commands'});await send({type:'command_search',action:{type:'query',text:'brush size'}});await send({type:'command_search',action:{type:'commit',text:'brush size'}});await wait('layerApp.state().command_search?.parameter?.id==="tool_setting.size"');},closeaction:auditSurfaceClose},
  ...['query','empty'].flatMap(kind=>[
    {batch:'m0',id:`preferences-${kind}`,rootselector:'#settings[open]',openaction:async({send})=>{await send({type:'invoke',command:'settings'});await send(preference({type:'toggle_search',open:true}));await send(preference({type:'search',query:kind==='query'?'color':'zzznomatchzzz'}));},closeaction:auditSurfaceClose},
    {batch:'m0',id:`shortcuts-${kind}`,rootselector:'#settings[open]',openaction:async({send,click,call,settle})=>{await send(shortcuts);await click('#shortcuts-search');await call('Input.insertText',{text:kind==='query'?'Undo':'zzznomatchzzz'});await settle();},closeaction:auditSurfaceClose},
    {batch:'m0',id:`command-search-${kind}`,rootselector:'#command-bar[open]',openaction:async({send})=>{await send({type:'invoke',command:'search_commands'});await send({type:'command_search',action:{type:'query',text:kind==='query'?'Undo':'zzznomatchzzz'}});},closeaction:auditSurfaceClose},
  ]),

  {batch:'w6a',id:'preferences-root',openaction:{type:'invoke',command:'settings'},rootselector:'#settings[open]',baseline:true},
  ...['appearance','canvas','color','input','shortcuts','about'].map(page=>({batch:'w6a',id:page==='shortcuts'?'preferences-shortcuts-direct':`preferences-${page}`,requestedpage:page,openaction:page==='shortcuts'?shortcuts:{type:'open_settings',page},rootselector:'#settings[open]',baseline:page==='appearance'})),
  {batch:'w6a',id:'preferences-shortcuts-category',openaction:[shortcuts,preference({type:'shortcut_category',id:'Modifier keys'})],rootselector:'#settings[open]'},
  {batch:'w6a',id:'shortcut-editor',openaction:[shortcuts,preference({type:'edit_shortcut',id:'command.Redo'})],rootselector:'#shortcut-editor[open]',variants:[
    {id:'shortcut-recording',openaction:preference({type:'begin_shortcut',id:'command.Hand'})},
    {id:'shortcut-conflict',openaction:async({call,settle})=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'e',code:'KeyE',windowsVirtualKeyCode:69});await settle();}}]},
  {batch:'w6a',id:'modifier-key',openaction:[shortcuts,preference({type:'add_modifier_key'})],rootselector:'#modifier-key[open]'},
  {batch:'w6a',id:'modifier-per-tool',openaction:async({send,evaluate})=>{
    await send(shortcuts);const key=await evaluate('layerApp.app.preferences().shortcut_page.modifiers[0].key');
    await send(preference({type:'edit_modifier_key',key}));await send(preference({type:'modifier_key_per_tool',key,per_tool:true}));
  },rootselector:'#settings[open]'},
  {batch:'w6a',id:'pen-button-per-tool',openaction:[{type:'open_settings',page:'input'},preference({type:'edit_pen_button',trigger:'pen.button.primary'}),preference({type:'pen_button_per_tool',trigger:'pen.button.primary',per_tool:true})],rootselector:'#settings[open]'},
  {batch:'w6a',id:'keymap-details',openaction:[shortcuts,preference({type:'keymap_details',open:true})],rootselector:'#keymap-details[open]'},
  {batch:'w6a',id:'choose-action',openaction:[{type:'open_settings',page:'input'},preference({type:'open_action_picker',trigger:'touch.tap.2'})],rootselector:'#action-picker[open]'},
  {batch:'w6a',id:'keymap-import',openaction:[shortcuts,preference({type:'import_keymap',text:JSON.stringify({format:'capycanvas-keymap',version:1,keymap:{id:'photoshop',revision:1}})})],rootselector:'#keymap-import[open]',closeaction:preference({type:'cancel_keymap_import'})},
  {batch:'w6b',id:'new-drawing',openaction:{type:'invoke',command:'new_document'},rootselector:'#new-drawing-dialog[open]',baseline:true,drawingstate:{kind:null,save:false},closeaction:async({evaluate})=>{await evaluate('document.querySelector("#new-drawing-cancel").click()');},variants:[
    {id:'new-paper',drawingstate:{kind:'paper',save:false},openaction:async({click})=>{await click('#new-drawing-texture');}},
    {id:'new-drawing-footer-presets',drawingstate:{kind:'presets',save:false},openaction:async({click})=>{await click('#new-drawing-paper-cancel');await click('#new-drawing-presets-button');}},
    {id:'new-drawing-footer-save-preset',drawingstate:{kind:'presets',save:true},openaction:async({click})=>{await click('#new-drawing-save-as');}},
    {id:'new-drawing-footer-paper-color',drawingstate:{kind:'paper_color',save:false},openaction:async({click})=>{await click('#new-drawing-save-cancel');await click('#new-drawing-browser-cancel');await click('#new-drawing-paper-color');}},
  ]},
  {batch:'w6b',id:'color',openaction:async({send,evaluate,click,wait})=>{
    const tile=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone&&layerApp.state().workspace.layout.phone.find(tile=>tile.control.kind==="color")?.id');
    if(tile!=null){await click(`#phone-tool-${tile}`);if(!await evaluate('!!layerApp.state().customization.drawer'))await click(`#phone-tool-${tile}`);}
    else {await send({type:'customize',action:{type:'set_panel_visible',panel:'color',visible:true}});
    const group=await evaluate('layerApp.app.layout(innerWidth,innerHeight).groups.find(group=>group.panels.includes("color"))');
    if(group&&group.active!=='color')await send({type:'select_panel_tab',group:group.id,panel:'color'});}
    await wait(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].some(button=>button.getBoundingClientRect().width>0)`);
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(button=>button.getBoundingClientRect().width>0).click()`);
  },rootselector:'.color-dialog[open]',baseline:true,closeaction:async({evaluate,wait,send})=>{await evaluate('document.querySelector(".color-dialog[open]").requestClose()');await wait('!document.querySelector(".color-dialog[open]")');await send({type:'customize',action:{type:'close_expanded'}});},variants:[{id:'all-swatches',openaction:async({evaluate,settle})=>{await evaluate('document.querySelector(".color-swatches").click()');await settle();}}]},
  {batch:'w6b',id:'paper',rootselector:'#texture-dialog[open]',openaction:async({evaluate,wait,settle,click,send})=>{
    await evaluate(`(()=>{for(const layer of layerApp.state().layers){layerApp.dispatch({type:'select_layer',id:layer.id});if(layerApp.state().layer_properties.controls.some(control=>control.paper))break;}})()`);
    await settle();
    const tile=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone&&layerApp.state().workspace.layout.phone.find(tile=>tile.control.panel==="layers")?.id');
    if(tile!=null){await click(`#phone-tool-${tile}`);if(!await evaluate('!!layerApp.state().customization.drawer'))await click(`#phone-tool-${tile}`);await send({type:'customize',action:{type:'toggle_layer_drawer_properties'}});}
    await wait('!!document.querySelector("#property-texture")');await evaluate('document.querySelector("#property-texture").click()');
  },closeaction:async({evaluate,wait,send})=>{await evaluate('document.querySelector("#texture-dialog[open]").requestClose()');await wait('!document.querySelector("#texture-dialog[open]")');await send({type:'customize',action:{type:'close_expanded'}});}},
  {batch:'w6b',id:'export',openaction:async({send,wait})=>{
    await send({type:'invoke',command:'export_document'});
    await wait('document.querySelectorAll("#export-options .native-comparison canvas").length===2');
  },rootselector:'#export-options[open]',variants:
    ['size','color','presets'].map(id=>({id:`export-${id}`,openaction:async({evaluate,settle})=>{
      if(id!=='size'){await evaluate('document.querySelector("#export-options [data-dialog-part=start]").click()');await settle();}
      await evaluate(`document.querySelector('[data-export-page="${id}"]').click()`);await settle();
    }})),closeaction:async({evaluate,settle})=>{await evaluate('document.querySelector("#export-options [data-dialog-part=start]").click()');await settle();}},
  ...[['canvas-size','canvas_size','#canvas-size-dialog[open]'],['image-size','image_size','#image-size-dialog[open]'],['properties','document_properties','dialog.document-dialog[open]']].map(([id,command,rootselector])=>({batch:'w6b',id,openaction:{type:'invoke',command},rootselector})),
  {batch:'w6c',id:'rename-drawing-preset',rootselector:'#new-drawing-dialog[open]',drawingstate:{kind:'presets',save:true},openaction:async context=>{
    const {send,click,evaluate,call}=context;await send({type:'invoke',command:'new_document'});await click('#new-drawing-presets-button');await click('#new-drawing-save-as');
    await drawingState(evaluate,{kind:'presets',save:true});await evaluate('document.querySelector("#new-drawing-save-name").select()');await call('Input.insertText',{text:'Canvas study'});await click('#new-drawing-save-confirm');
    await drawingState(evaluate,{kind:'presets',save:false});await drawingPresetMenu(context,'rename');
    assert.equal(await evaluate('layerApp.state().new_drawing.drawer.save?.name'),'Canvas study');
  },closeaction:async context=>{await context.click('#new-drawing-save-cancel');await drawingPresetMenu(context,'delete');await context.evaluate('document.querySelector("#new-drawing-cancel").click()');}},
  {batch:'w6c',id:'rename-paper',rootselector:'#paper-rename-dialog[open]',openaction:async context=>{
    const {evaluate,click,wait}=context;await dialogAuditRows.find(row=>row.id==='paper').openaction(context);
    await evaluate(`(async()=>{const canvas=new OffscreenCanvas(64,48),context=canvas.getContext('2d');for(let y=0;y<48;y++)for(let x=0;x<64;x++){const v=(x*7+y*3)%256;context.fillStyle='rgb('+v+','+v+','+v+')';context.fillRect(x,y,1,1);}const file=new File([await canvas.convertToBlob({type:'image/png'})],'Linen scan.png',{type:'image/png'}),click=HTMLInputElement.prototype.click;HTMLInputElement.prototype.click=function(){if(this.type!=='file'||this.accept!=='image/*')return click.call(this);HTMLInputElement.prototype.click=click;const transfer=new DataTransfer();transfer.items.add(file);this.files=transfer.files;this.dispatchEvent(new Event('change'));};})()`);
    await click('#texture-import');await wait('!layerApp.state().requests.some(request=>request.kind.type==="paper")&&layerApp.state().layer_properties.controls.some(control=>control.paper?.tiles.some(tile=>tile.label==="Linen scan"))');
    await click('#property-texture');await wait('!!document.querySelector("#texture-dialog[open]")');
    await evaluate(`(()=>{const tile=[...document.querySelectorAll('#texture-dialog .texture-tile')].find(tile=>tile.querySelector('.texture-caption').textContent==='Linen scan');tile.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true}));document.querySelector('#texture-dialog [role=menu] button').click();})()`);
  },closeaction:async({evaluate,wait,send})=>{await evaluate('document.querySelector("#paper-rename-dialog").requestClose()');await wait('!document.querySelector("#paper-rename-dialog")');await evaluate('document.querySelector("#texture-dialog").requestClose()');const id=await evaluate('layerApp.state().layer_properties.controls.find(control=>control.paper).paper.tiles.find(tile=>tile.label==="Linen scan").id');await send({type:'effect',action:{op:'delete_paper',id}});await send({type:'customize',action:{type:'close_expanded'}});}},
  {batch:'w6c',id:'save-export-preset',rootselector:'#export-preset-name-dialog[open]',openaction:async({send,evaluate,wait})=>{
    await send({type:'invoke',command:'export_document'});await wait('!!document.querySelector("#export-options[open]")');
    await wait('document.querySelectorAll("#export-options .native-comparison canvas").length===2');
    await evaluate(`(()=>{const root=document.querySelector('#export-options');root.querySelector('[data-export-page="presets"]').click();[...root.querySelectorAll('button')].find(button=>button.textContent===layerApp.app.export_copy().save_new_preset).click();})()`);
  },closeaction:async({evaluate,wait})=>{await evaluate('document.querySelector("#export-preset-name-dialog").requestClose()');await wait('!document.querySelector("#export-preset-name-dialog")');await evaluate('document.querySelector("#export-options [data-dialog-part=start]").click()');}},
  ...[['new-palette','new_palette'],['rename-palette','rename_palette'],['delete-palette','remove_palette']].map(([id,command])=>({batch:'w6c',id,rootselector:'dialog.palette-form[open]',openaction:async({send,evaluate,wait})=>{
    if(command!=='new_palette'&&!await evaluate('layerApp.state().color_library.palettes.some(palette=>palette.name==="Canvas study")'))await send({type:'color',action:{op:'library',action:{op:'create_palette',name:'Canvas study'}}});
    await send({type:'phone',action:{kind:'panel',panel:'palettes'}});await wait('!!document.querySelector(".content-drawer .palettes-panel .palette-selector")');
    await evaluate(`(()=>{const root=document.querySelector('.content-drawer .palettes-panel');root.querySelector('.palette-selector').click();const node=${JSON.stringify(command)}==='new_palette'?root.querySelector('.palette-library-add'):[...root.querySelectorAll('.palette-choice')].find(node=>node.dataset.name==='Canvas study');node.menuCommand({command:${JSON.stringify(command)},id:layerApp.state().color_library.palettes.find(palette=>palette.name==='Canvas study')?.id});})()`);
  },closeaction:async({evaluate,wait,send})=>{await evaluate('document.querySelector("dialog.palette-form[open]").requestClose()');await wait('!document.querySelector("dialog.palette-form[open]")');await send({type:'customize',action:{type:'close_expanded'}});}})),
  {batch:'w6c',id:'profile-library',rootselector:'.profile-library[open]',openaction:async({send,evaluate,wait})=>{
    await send({type:'open_settings',page:'color'});await wait('[...document.querySelectorAll("#settings button")].some(button=>button.textContent===layerApp.app.profile_copy().manage)');
    await evaluate('[...document.querySelectorAll("#settings button")].find(button=>button.textContent===layerApp.app.profile_copy().manage).click()');
  },closeaction:async({evaluate,wait,send})=>{await evaluate('document.querySelector(".profile-library[open]").requestClose()');await wait('!document.querySelector(".profile-library[open]")');await send({type:'close_settings'});}},
  ...[['workspaces','workspaces'],['layout-history','history'],['this-workspace','this_workspace'],['saved-toolbars','toolbar_library']].map(([id,page])=>({batch:'w6c',id,rootselector:'.workspace-manager[open]',openaction:context=>managerPage(context,page),closeaction:managerClose})),
  ...[['this-workspace-details','this_workspace'],['saved-toolbar-details','toolbar_library']].map(([id,page])=>({batch:'w6c',id,rootselector:'.workspace-manager[open]',openaction:context=>managerPage(context,page,true),closeaction:managerClose})),
  ...[['new-workspace','new'],['new-toolbar','new_toolbar']].map(([id,type])=>({batch:'w6c',id,rootselector:'.workspace-form[open]',openaction:async context=>{
    await managerPage(context,type==='new'?'workspaces':'this_workspace');await managerInput(context,{type:'form',action:type==='new'?{type}:{type,value:null}});
  },closeaction:managerClose})),
  ...[['save-toolbar','this_workspace','save_toolbar'],['replace-toolbar','this_workspace','replace_toolbar'],['rename-saved-toolbar','toolbar_library','rename'],['delete-saved-toolbar','toolbar_library','delete'],['update-saved-toolbar','toolbar_library','update_toolbar']].map(([id,page,type])=>({batch:'w6c',id,rootselector:'.workspace-form[open]',openaction:context=>managerOffered(context,page,type),closeaction:managerClose})),
  ...[['rename-toolbar','rename_toolbar'],['duplicate-toolbar','duplicate_toolbar'],['delete-toolbar','delete_toolbar']].map(([id,type])=>({batch:'w6c',id,rootselector:'#toolbar-prompt[open]',openaction:context=>managerOffered(context,'this_workspace',type),closeaction:async context=>{await context.send({type:'customize',action:{type:'cancel_toolbar'}});await managerClose(context);}})),
  ...[['reset-workspace','reset_layout'],['reset-brushes','reset_brushes']].map(([id,type])=>({batch:'w6c',id,rootselector:'.workspace-form[open]',openaction:async context=>{await context.send({type:'workspace_manager',command:{type}});},closeaction:managerClose})),
  ...[['rename-workspace','rename'],['delete-workspace','delete']].map(([id,type])=>({batch:'w6c',id,...(type==='rename'?{aliasof:'workspaces'}:{}),rootselector:type==='rename'?'.workspace-manager[open]':'.workspace-form[open]',openaction:async context=>{
    const id=await managerWorkspace(context);const action=await context.evaluate(`JSON.parse(layerApp.app.workspace_view()).rows.find(row=>row.id===${JSON.stringify(id)}).actions.find(button=>button.enabled&&button.action.type===${JSON.stringify(type)})?.action`);
    assert.ok(action,`Authored workspace offers ${type}`);await managerInput(context,{type:'form',action});
  },closeaction:managerClose})),

  {batch:'w6c',id:'save-as-new-workspace',rootselector:'.workspace-form[open]',openaction:async context=>{
    await managerInput(context,{type:'form',action:{type:'save_as_new'}});
  },closeaction:managerClose},
  {batch:'w6c',id:'delete-inactive-workspace',rootselector:'.workspace-form[open]',openaction:async context=>{
    const id=await managerWorkspace(context);await managerClose(context);await managerInput(context,{type:'switch',id:'builtin:workspace:painter'});await managerPage(context,'workspaces');
    const action=await context.evaluate(`JSON.parse(layerApp.app.workspace_view()).rows.find(row=>row.id===${JSON.stringify(id)}).actions.find(button=>button.enabled&&button.action.type==='delete')?.action`);
    assert.ok(action,'Inactive workspace offers Delete');await managerInput(context,{type:'form',action});
  },closeaction:managerClose},
  {batch:'w6c',id:'legacy-toolbar-manager',aliasof:'this-workspace',blocked:'Managed workspace menus and commands route Manage Toolbars to This Workspace; the retained raw customization action has no separate managed user route.'},
  ...[['add-tools',false],['insert-tools',true]].map(([id,phone])=>({batch:'w6c',id,rootselector:'#tool-picker[open]',openaction:async({send,evaluate})=>{
    if(phone){const before=await evaluate('layerApp.state().workspace.layout.phone[0].id');await send({type:'phone',action:{kind:'insert_tools',before}});}
    else await send({type:'customize',action:{type:'insert_tools',panel:'toolbar',before:null}});
  },closeaction:{type:'customize',action:{type:'cancel_tools'}}})),
  {batch:'w6c',id:'unsaved-changes',rootselector:'dialog.document-dialog[open]',openaction:async({send,evaluate})=>{
    await send({type:'invoke',command:'add_layer'});await evaluate('layerApp.documents.close(layerApp.app.document_tabs(0).selected);null');
  }},
  {batch:'w6c',id:'cannot-open-file',rootselector:'#file-launch-error[open]',openaction:async({evaluate,wait})=>{
    await evaluate(`window.phoneAuditOpenFailure=null;void layerApp.documents.openFiles([new File(['invalid'],'broken.png')]).then(()=>phoneAuditOpenFailure={resolved:true},error=>phoneAuditOpenFailure={message:error?.message??String(error)});`);
    await wait('!!document.querySelector("#file-launch-error[open]")');
    assert.deepEqual(await evaluate('({error:layerApp.state().host_error??null,notice:document.querySelector("#status").textContent.trim()})'),{error:null,notice:''},'The shared file-failure alert owns the failed open');
  },closeaction:async({evaluate,wait})=>{
    await evaluate(`[...document.querySelectorAll('#file-launch-error button')].find(button=>button.textContent===layerApp.app.bootstrap_view().ok).click()`);
    await wait('phoneAuditOpenFailure!==null&&!layerApp.documents.busy()');
    assert.equal(await evaluate('phoneAuditOpenFailure.resolved'),true,'Acknowledgement completes the handled file-open workflow');
    assert.deepEqual(await evaluate('({error:layerApp.state().host_error??null,notice:document.querySelector("#status").textContent.trim()})'),{error:null,notice:''},'The decoder diagnostic does not escape after acknowledgement');
    await evaluate('delete window.phoneAuditOpenFailure');
  }},

  {batch:'w6c',id:'workspace-save-status',rootselector:'.workspace-status:not([hidden])',openaction:auditSaveFailureOpen,closeaction:auditSaveFailureClose,finalize:auditBoundaryRestore},
  {batch:'w6c',id:'failed-drawing-recovery',blocked:'Source-derived capture gap: prior-window OPFS ownership prevents live checkpoint mutation; the valid copied-manifest fixture reaches the real error but healthy/startup background depends on manifest restoration order. Both Web cases exhausted their bounded attempts; no qualified capture.'},
  {batch:'w6c',id:'stroke-recording-failure',rootselector:'dialog.document-dialog[open]',openaction:auditRecordingFailureOpen,closeaction:auditRecordingFailureClose,finalize:auditRecordingFailureRestore},
  {batch:'w6c',id:'recover-interrupted-workspaces',rootselector:'.workspace-form[open]',openaction:auditInterruptedWorkspaceOpen,closeaction:auditInterruptedWorkspaceClose,finalize:auditBoundaryRestore},
  ...[
    ['restore-workspaces','Requires the existing released-storage/restart setup in saved-data-ux.'],
    ['workspace-not-saved','Web uses the browser-owned beforeunload prompt; no application close-recovery consumer exists. W7 storage-alert is this same native state, not another Web route.'],
    ['preserved-package-preview','Requires the existing package fixture preparation in package-view.'],
  ].map(([id,blocked])=>({batch:'w6c',id,blocked})),
  {batch:'w6d',id:'selection-refine',rootselector:'#selection-refine-panel',openaction:async({send,evaluate,settle,wait})=>{
    await send({type:'invoke',command:'select_all'});await wait('layerApp.state().layer_tools.has_selection');
    await send({type:'invoke',command:'feather_selection'});await wait('layerApp.state().layer_tools.selection_resize?.kind==="feather"');
    const {readyCanvas}=await import('./canvas-pixels-fixture.mjs');await readyCanvas({evaluate,settle});
  },closeaction:async({send})=>{await send({type:'selection',action:{op:'cancel_resize'}});await send({type:'invoke',command:'deselect'});}},
  {batch:'w6d',id:'frequency-separation',rootselector:'#frequency-separation-panel',openaction:async({send,evaluate,settle,wait})=>{
    await wait('layerApp.state().commands.some(command=>command.id==="frequency_separation"&&command.enabled)');await send({type:'invoke',command:'frequency_separation'});
    await wait('!!layerApp.state().layer_tools.frequency_separation');const {readyCanvas}=await import('./canvas-pixels-fixture.mjs');await readyCanvas({evaluate,settle});
  },closeaction:{type:'frequency_separation',action:{op:'cancel'}}},
  ...[['pressure-calibration','pressure'],['tilt-calibration','tilt']].map(([id,kind])=>({batch:'w6d',id,rootselector:`#pen-${kind}-dialog`,openaction:async({send,click,wait})=>{
    await send({type:'open_settings',page:'input'});await wait('!!document.querySelector("#settings[open]")');await click(`#setting-pen-${kind}`);
    await wait(`!!layerApp.state().${kind}_calibration&&!document.querySelector('#settings[open]')`);
  },closeaction:async({click})=>{await click(`#pen-${kind}-cancel`);}})),
  {batch:'w6d',id:'soft-proof',rootselector:'.content-drawer .proof-panel',collector:'drawer',openaction:async({send,evaluate,wait})=>{
    await send({type:'invoke',command:'soft_proof_setup'});await wait('!!document.querySelector(".content-drawer .proof-panel .proof-modes")');
    await evaluate('[...document.querySelectorAll(".content-drawer .proof-modes button")].find(button=>button.value==="print").click()');await wait('layerApp.app.proof_form().mode==="print"');
  },closeaction:{type:'customize',action:{type:'close_expanded'}}},
  {batch:'w6d',id:'sdr-rendition',rootselector:'.content-drawer .proof-panel',collector:'drawer',document:{color:{space:'Srgb',depth:'F32'},count:2},openaction:async({send,evaluate,wait})=>{
    await evaluate('window.phoneSdrOriginal=String(layerApp.app.document_tabs(0).selected)');await send({type:'invoke',command:'new_document'});
    const {createDrawing}=await import('./new-drawing-fixture.mjs');await evaluate(createDrawing({width:1200,height:1600,space:'Srgb',depth:'F32',paper:'white',relief:0}));
    await wait('layerApp.app.document_color().depth==="F32"&&layerApp.app.brush_ready()&&!layerApp.documents.busy()');
    await send({type:'invoke',command:'sdr_rendition'});await wait('layerApp.app.proof_form().mode==="sdr"&&!!document.querySelector(".content-drawer .proof-panel")');
  },closeaction:async({send,evaluate,wait})=>{await send({type:'customize',action:{type:'close_expanded'}});await evaluate('layerApp.documents.close(layerApp.app.document_tabs(0).selected)');await wait('String(layerApp.app.document_tabs(0).selected)===phoneSdrOriginal&&!layerApp.documents.busy()');await evaluate('delete window.phoneSdrOriginal');}},
  {batch:'w6d',id:'command-search',rootselector:'#command-bar[open]',openaction:async({send,evaluate})=>{
    await send({type:'invoke',command:'add_layer'});if(await evaluate('layerApp.state().commands.some(command=>command.id==="deselect"&&command.enabled)'))await send({type:'invoke',command:'deselect'});await send({type:'invoke',command:'search_commands'});
  },closeaction:async({evaluate})=>{await evaluate('document.querySelector("#command-bar").requestClose()');}},
  {batch:'w6d',id:'document-opening-progress',rootselector:'.file-progress[open]',openaction:auditOpeningProgressOpen,closeaction:auditOpeningProgressClose,finalize:auditBoundaryRestore},
  {batch:'w6d',id:'image-import-progress',rootselector:'.file-progress[open]',openaction:context=>auditOpeningProgressOpen(context,'import_image'),closeaction:auditOpeningProgressClose,finalize:auditBoundaryRestore},
  {batch:'w6d',id:'export-progress',rootselector:'.file-progress[open]',openaction:auditExportProgressOpen,closeaction:auditExportProgressClose,finalize:auditExportProgressRestore},
  {batch:'w6d',id:'document-color-progress',rootselector:'dialog.document-dialog[open]',openaction:auditColorProgressOpen,closeaction:auditColorProgressClose,finalize:auditColorProgressRestore},
  {batch:'w6d',id:'clipboard-progress',rootselector:'.file-progress[open]',document:{extent:[2048,1025],color:{space:'Srgb',depth:'U8'},count:2},openaction:auditClipboardProgressOpen,closeaction:auditClipboardProgressClose,finalize:auditClipboardProgressRestore},
  ...[['assign-profile','assign_profile'],['convert-color-space','convert_color_space'],['change-bit-depth','change_bit_depth'],['repair-source-profile','repair_source_profile'],['rasterize-source','rasterize_source']].map(([id,command])=>({
    batch:'w7-photo',id,rootselector:'dialog.document-dialog:not(.file-progress)[open]',document:{extent:[513,257],color:{space:'ProPhoto',depth:'U16'},count:'photo',...(id!=='rasterize-source'?{draft:{state:id}}:{})},
    openaction:async context=>{await photoAuditOpen(context);await context.send({type:'invoke',command});
      const draft=await context.evaluate(`(${photoAuditDraft.toString()})(layerApp.app,${JSON.stringify(id)})`);
      if(draft){const selector=`dialog.document-dialog[open] .dropdown[aria-label=${JSON.stringify(draft.label)}]`;await context.wait(`!!document.querySelector(${JSON.stringify(selector)})`);await context.evaluate(dropdownChoice(selector,draft.value));}
      await context.wait('!!document.querySelector("dialog.document-dialog:not(.file-progress)[open] .color-comparison canvas")&&!document.querySelector("dialog.file-progress[open]")');},
    closeaction:photoAuditClose,
  })),
  {batch:'w7-photo',id:'untagged-source-profile',rootselector:'dialog[open]:has(.dropdown[aria-label="Interpret as"])',openaction:untaggedAuditOpen,closeaction:untaggedAuditClose},
];

export function dialogAuditSpecifications(rootselector,root=document){
  const node=root.querySelector(rootselector),specs=[{id:'page',role:'page',selector:rootselector}];
  if((node?.tagName==='DIALOG'||node?.classList?.contains('dialog-page'))&&node.dataset?.dialogFrame!=='alert')specs.push({id:'page.header',role:'page-header',selector:`${rootselector} [data-dialog-part="header"]`});
  for(const part of ['start','end','panes'])if(node?.querySelector(`[data-dialog-part="${part}"]`))specs.push({id:`page.${part}`,role:part==='panes'?'panes':'page-action',selector:`${rootselector} [data-dialog-part="${part}"]`});
  for(const row of node?.querySelectorAll?.('.preference-row')??[]){
    const input=row.querySelector('label[for]')?.htmlFor;if(!input?.startsWith('setting-'))continue;
    const selector=`${rootselector} .preference-row:has(label[for=${JSON.stringify(input)}])`;
    for(const [part,suffix]of [['value','.preference-choice > summary > span'],['arrow','.preference-choice > summary > svg:last-child']])if(row.querySelector(suffix))specs.push({id:`preference.${input.slice(8)}.${part}`,role:`preference-${part}`,selector:`${selector} ${suffix}`});
  }
  return specs;
}

async function assertSettingsPresentation(evaluate,state){
  if(!state.startsWith('preferences-')&&!['shortcut-editor','shortcut-recording','shortcut-conflict','modifier-key','keymap-details'].includes(state))return;
  const actual=await evaluate(`(()=>{
    const shown=node=>!!node?.getClientRects().length&&getComputedStyle(node).visibility!=='hidden';
    const visibleText=node=>{const range=document.createRange();range.selectNodeContents(node);return [...range.getClientRects()].map(rect=>{let box={left:rect.left,right:rect.right,top:rect.top,bottom:rect.bottom};for(let parent=node;parent;parent=parent.parentElement){const style=getComputedStyle(parent);if(/hidden|clip|auto|scroll/.test(style.overflowX+' '+style.overflowY)){const clip=parent.getBoundingClientRect();box={left:Math.max(box.left,clip.left),right:Math.min(box.right,clip.right),top:Math.max(box.top,clip.top),bottom:Math.min(box.bottom,clip.bottom)};}}return box;}).filter(box=>box.right>box.left&&box.bottom>box.top);};
    const overlaps=[];for(const row of document.querySelectorAll('dialog[open] .preference-row')){const text=row.querySelector(':scope > .preference-text');if(!shown(text))continue;for(const suffix of row.querySelectorAll(':scope > .preference-choice > summary > span,:scope > .settings-info,:scope > .settings-link,:scope.recording > button')){if(!shown(suffix))continue;for(const a of visibleText(text))for(const b of visibleText(suffix))if(Math.min(a.right,b.right)-Math.max(a.left,b.left)>1&&Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)>1)overlaps.push({title:text.textContent,value:suffix.textContent});}}
    const lineOverlaps=[];for(const node of [...document.querySelectorAll('#settings[open] .preference-text label,#settings[open] .preference-text p,dialog[open] .recording .preference-text')].filter(shown)){const walker=document.createTreeWalker(node,NodeFilter.SHOW_TEXT),rects=[];while(walker.nextNode()){const range=document.createRange();range.selectNodeContents(walker.currentNode);rects.push(...[...range.getClientRects()].filter(rect=>rect.width>0&&rect.height>0));}const lines=[];for(const rect of rects.sort((a,b)=>a.top-b.top)){const previous=lines.at(-1);if(previous&&Math.abs(previous.top-rect.top)<.5)previous.bottom=Math.max(previous.bottom,rect.bottom);else lines.push({top:rect.top,bottom:rect.bottom});}for(let index=1;index<lines.length;index++)if(lines[index].top<lines[index-1].bottom-.5)lineOverlaps.push({text:node.textContent,lines});}
    const active=[...document.querySelectorAll('dialog[open]')].at(-1),navigation=document.querySelector('.preferences-navigation');
    const swatches=[...document.querySelectorAll('.swatch-circles .swatch')].filter(shown);
    return {overlaps,lineOverlaps,single:!!document.querySelector('.preferences-layout.single-pane'),navigation:shown(navigation),
      chevrons:[...(navigation?.querySelectorAll('button')??[])].map(node=>shown(node.querySelector('.row-chevron'))),
      card:navigation?getComputedStyle(navigation).backgroundColor:null,
      library:!!document.querySelector('#color-profile-library')?.closest('.preference-group'),
      swatches:swatches.map(node=>({corner:getComputedStyle(node).cornerShape,radius:getComputedStyle(node).borderRadius})),
      actions:[...document.querySelectorAll('.preference-action')].filter(shown).map(node=>getComputedStyle(node).backgroundColor),
      choices:[...document.querySelectorAll('#settings[open] .preference-row > .preference-choice > summary:has(> span):has(> svg:last-child)')].filter(shown).map(node=>{const row=node.closest('.preference-row').getBoundingClientRect(),value=node.querySelector('span').getBoundingClientRect(),arrow=node.querySelector('svg:last-child').getBoundingClientRect();return{row:[row.left,row.right],value:[value.left,value.right],arrow:[arrow.left,arrow.right]};}),
      empty:[...(active?.querySelectorAll('.preference-stack > p')??[])].filter(node=>!node.textContent.trim()&&shown(node)).length,
      warning:document.querySelector('#shortcut-recording svg')?.dataset.asset,
      title:document.querySelector('#modifier-key h2')?.textContent,capture:layerApp.app.preferences().capture?.label,
      keymap:[...document.querySelectorAll('#keymap-details .preference-content > .settings-group')].map(node=>{const card=node.getBoundingClientRect(),body=node.closest('.preference-viewport').getBoundingClientRect();return{x:card.x,width:card.width,bottom:card.bottom,bodyBottom:body.bottom};})};})()`);
  assert.equal(actual.empty,0,`${state}: empty shortcut paragraphs stay hidden`);
  assert.deepEqual(actual.overlaps,[],`${state}: visible title, value, link and recording actions do not overlap`);
  assert.deepEqual(actual.lineOverlaps,[],`${state}: wrapped text lines do not overlap`);
  for(const choice of actual.choices)assert.ok(choice.value[0]>=choice.row[0]-.5&&choice.value[1]<=choice.row[1]+.5&&choice.arrow[0]>=choice.row[0]-.5&&choice.arrow[1]<=choice.row[1]+.5,`Settings choice value/arrow stay inside their row: ${JSON.stringify(choice)}`);
  if(state==='preferences-root'&&actual.single){assert.ok(actual.navigation&&actual.chevrons.length&&actual.chevrons.every(Boolean),'Settings root card has visible chevrons');assert.notEqual(actual.card,'rgba(0, 0, 0, 0)');}
  if(state==='preferences-color')assert.ok(actual.library,'Color Profile Library uses a card row');
  assert.ok(actual.actions.every(fill=>fill!=='rgba(0, 0, 0, 0)'&&fill!=='transparent'),'Settings action buttons retain their native fill');
  for(const shape of actual.swatches)assert.match(shape.corner??'',/squircle|superellipse/,`Settings swatch uses native square contour: ${JSON.stringify(shape)}`);
  if(state==='shortcut-conflict')assert.match(actual.warning??'',/warning/,'Shortcut conflict uses a warning triangle');
  if(state==='modifier-key')assert.equal(actual.title,actual.capture,'Modifier title follows shared recording label');
  if(state==='keymap-details')assert.ok(actual.keymap.length&&actual.keymap.every(bounds=>bounds.width>0&&Math.abs(bounds.bottom-bounds.bodyBottom)<=1),'Keymap card fills the remaining body height');
}

export function selectDialogAuditRows(dialogs,requested){
  const rows=dialogAuditRows.filter(row=>dialogs?.batch?dialogs.batch==='all'||row.batch===dialogs.batch:dialogs?.mode==='desktop'?row.baseline:row.batch==='w6a'&&row.id.startsWith('preferences-'));
  if(requested==null)return rows;
  assert.ok(dialogs?.batch,'LAYER_PHONE_DIALOG_ROWS requires a selected dialog batch');
  const ids=requested.split(',').map(id=>id.trim());
  assert.ok(ids.every(Boolean),'LAYER_PHONE_DIALOG_ROWS requires nonempty row IDs');
  for(const id of ids)assert.ok(rows.some(row=>row.id===id),`Unknown dialog row ${id} in batch ${dialogs.batch}`);
  return rows.filter(row=>ids.includes(row.id));
}

export async function checkPhoneAudit({call,evaluate,reload,settle,canvasPixels,mode='audit',batch=null}) {
  const audit=auditModes[mode];assert.ok(audit,`Unknown phone audit mode: ${mode}`);
  const gtkAllocations=process.env.LAYER_PHONE_GTK_ALLOCATIONS;
  if(mode==='customize')assert.ok(gtkAllocations,'LAYER_PHONE_GTK_ALLOCATIONS must point to the matching GTK phone customization captures');
  const dialogs=audit.dialogMode?{mode:audit.dialogMode,batch:batch??(mode==='dialogs-phone'?'w6a':null)}:null;
  const {createDrawing}=await import('./new-drawing-fixture.mjs');
  const directory=process.env.LAYER_TEST_ARTIFACTS||'/mnt/localssd/capy9-phone-impl/web-phone-audit';
  await mkdir(directory,{recursive:true});
  const root=fileURLToPath(new URL('../..',import.meta.url));
  const source={};
  const requestedRows=process.env.LAYER_PHONE_DIALOG_ROWS,rows=dialogs?selectDialogAuditRows(dialogs,requestedRows):[];
  const report={schema_version:1,host:'web',source,dialog_batch:dialogs?.batch??null,requested_dialog_rows:requestedRows?.split(',').map(id=>id.trim())??null,cases:[],unreachable:[]};
  const original=await evaluate(`({settings:layerApp.state().settings,text:document.documentElement.style.getPropertyValue('--ui-text-size'),workspace:JSON.parse(layerApp.app.workspace_view()).id,documents:layerApp.app.document_tabs(0).tabs.map(tab=>({id:String(tab.id),modified:tab.modified}))})`);
  assert.equal(original.documents.length,1,'Audit starts with one isolated startup drawing');
  assert.equal(original.documents[0].modified,false,'Audit may only close an unmodified startup drawing');
  const reducedMotion=await evaluate('matchMedia("(prefers-reduced-motion: reduce)").matches?"reduce":"no-preference"');
  report.prefers_reduced_motion=dialogs?'reduce':reducedMotion;
  if(dialogs)await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
  try {
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else requestAnimationFrame(check);}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)});null`);await settle();};
  const close=async()=>{
    if(await evaluate('layerApp.state().phone.menu'))await send({type:'phone',action:{kind:'capy'}});
    if(await evaluate('!!document.querySelector("dialog[open]")')){
      await evaluate('document.querySelector("dialog[open]")?.requestClose()');
      await settle();
    }
    await wait('!document.querySelector("dialog[open]")');
    await wait('!layerApp.state().requests.length');
  };
  const collect=specifications=>evaluate(`(${measurePhoneAuditElements.toString()})(${JSON.stringify(specifications)})`);
  const capture=async (test,state,specifications,availability,requested_page=null,expected_documents=1,expected_color={space:'Srgb',depth:'U8'},expected_extent=[1200,1600],expected_draft=null)=>{
    const actual=await collect(specifications);
    const context=actual.elements.filter(element=>element.role==='context');
    actual.elements=actual.elements.filter(element=>element.role!=='context');
    const metadata=await evaluate(`(()=>{const s=layerApp.state(),font=getComputedStyle(document.documentElement),preferences=layerApp.app.preferences(),visible=selector=>{const n=document.querySelector(selector);return !!n?.getClientRects().length&&getComputedStyle(n).display!=='none';};return {viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,theme:s.theme,font_px:parseFloat(font.fontSize),font:font.fontFamily,phone_active:!!layerApp.app.layout(innerWidth,innerHeight).phone,workspace:JSON.parse(layerApp.app.workspace_view()).id,url:location.href,preferences:preferences?{page:preferences.page,page_requested:preferences.page_requested??null,sidebar_visible:visible('.preferences-sidebar'),content_visible:visible('.preferences-content')}:null}})()`);
    assert.deepEqual(metadata.viewport,test.viewport,'Capture logical viewport');assert.equal(metadata.dpr,1,'Capture DPR');
    assert.equal(metadata.theme,test.theme,'Capture theme');
    metadata.phone_tiles=await evaluate('layerApp.state().workspace.layout.phone??[]');
    metadata.alert=await evaluate(`(()=>{const root=[...document.querySelectorAll('dialog[data-dialog-frame="alert"][open]')].at(-1);if(!root)return null;const style=getComputedStyle(root),footer=root.querySelector('footer'),bounds=node=>{const r=node.getBoundingClientRect();return[r.x,r.y,r.width,r.height];};return{bounds:bounds(root),short:root.classList.contains('short'),stacked:footer?.classList.contains('alert-responses-stacked')??false,font:style.font,padding:[style.paddingTop,style.paddingRight,style.paddingBottom,style.paddingLeft],backdrop:getComputedStyle(root,'::backdrop').backgroundColor,chips:[...document.querySelectorAll('#phone-frame .phone-chip')].map(node=>({id:node.id,bounds:bounds(node),fill:getComputedStyle(node).backgroundColor,role:getComputedStyle(node).getPropertyValue('--glass-chip')})),responses:[...(footer?.querySelectorAll('button')??[])].filter(node=>node.getClientRects().length&&!node.hidden).map(node=>{const s=getComputedStyle(node);return{text:node.textContent,bounds:bounds(node),font:s.font,color:s.color,fill:s.backgroundColor,image:s.backgroundImage};})};})()`);
    if(state==='command-search')metadata.command_search=await evaluate('({query:layerApp.state().command_search?.query??"",result_ids:(layerApp.state().command_search?.results??[]).map(result=>result.id)})');
    if(state==='soft-proof'||state==='sdr-rendition')metadata.proof_body_rows=await evaluate(`(()=>{const body=document.querySelector('.content-drawer .proof-panel');return [...body.querySelectorAll('.proof-modes button,.proof-print .document-size,.proof-sdr .document-size,.proof-tone-pad')].filter(node=>node.getClientRects().length).map(node=>{const r=node.getBoundingClientRect();return{label:node.textContent.trim(),bounds:[r.x,r.y,r.width,r.height]};});})()`);
    if(mode==='frame')metadata.presented_tools=await evaluate('layerApp.app.phone_tools()');
    if(mode==='menus')Object.assign(metadata,await evaluate('({menu_route:layerApp.state().phone.menu_route,swapped:layerApp.state().phone.swapped,hidden:layerApp.state().phone.hidden})'));
    if(mode==='input')metadata.input=await evaluate('({coarse:matchMedia("(pointer: coarse)").matches,screen:[screen.width,screen.height],mode:layerApp.state().settings.finger_drawing,pen_seen:layerApp.state().settings.pen_seen,picker:layerApp.state().color_picker})');
    const document=await evaluate(`(()=>{const s=layerApp.state(),tab=s.tabs.find(t=>t.active);return {extent:tab?[tab.width,tab.height]:null,color:layerApp.app.document_color(),count:layerApp.app.document_tabs(0).tabs.length,busy:s.document_file.busy,host_busy:layerApp.documents.busy()};})()`);
    assert.equal(document.count,expected_documents,'Captured drawing count');
    assert.deepEqual(document.extent,expected_extent,'Captured active drawing extent');
    assert.deepEqual(document.color,expected_color,'Captured active drawing color');
    metadata.document=document;
    if(expected_draft){const draft=expected_draft.state?await evaluate(`(${photoAuditDraft.toString()})(layerApp.app,${JSON.stringify(expected_draft.state)})`):expected_draft;const selector=`dialog.document-dialog[open] .dropdown[aria-label=${JSON.stringify(draft.label)}]`;const value=await evaluate(`document.querySelector(${JSON.stringify(selector)})?.value??null`);assert.equal(value,draft.value,'Captured photo draft choice');metadata.draft_choice={label:draft.label,value};}
    if(['assign-profile','convert-color-space','change-bit-depth','repair-source-profile','rasterize-source','untagged-source-profile','profile-library','soft-proof'].includes(state)){metadata.color_presentation=await evaluate(`(${colorPresentationAudit.toString()})(${JSON.stringify(state)})`);assert.deepEqual(metadata.color_presentation.issues,[],`${state}: native Color/Profile presentation`);}
    if(['profile-picker-menu','profile-library-row-menu'].includes(state)){
      const selector=state==='profile-picker-menu'?'.proof-print .dropdown':'.profile-library-more';
      metadata.profile_menu=await evaluate(`(()=>{const model=document.querySelector(${JSON.stringify(selector)}).menuModel(),root=document.querySelector('.panel-context-menu:popover-open');return{sections:model.sections,rows:[...root.querySelectorAll('button')].map(node=>{const r=node.getBoundingClientRect();return{label:node.textContent.trim(),disabled:node.disabled,bounds:[r.x,r.y,r.width,r.height]};})};})()`);
      assert.ok(metadata.profile_menu.rows.length&&metadata.profile_menu.sections.every(section=>section.length),'The native profile menu has visible rows and no empty sections');
    }
    metadata.camera=await evaluate(`JSON.parse(JSON.stringify(layerApp.state().camera,(_,value)=>typeof value==='bigint'?String(value):value))`);
    if(mode==='drawers')metadata.drawer=await evaluate(`(()=>{const s=layerApp.state(),d=s.customization.drawer;if(!d)return null;return JSON.parse(JSON.stringify({model:d,measure:layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}),tools:d.columns.flat().filter(p=>['brushes','brush_sets','sculpt_sets','tools'].includes(p)).map(panel=>({panel,view:layerApp.app.tool_panel(panel,d.anchor)}))},(_,value)=>typeof value==='bigint'?String(value):value));})()`);
    assert.ok(Math.abs(metadata.font_px-test.text_pt*4/3)<.05,`Measured ${metadata.font_px}px does not match ${test.text_pt}pt`);
    const issues=[];
    assert.deepEqual(await evaluate(`(${dialogScrollerIssues.toString()})()`),[],`${state}: dialog/drawer scrolling keeps overlay indicators and fixed chrome`);
    if(requested_page){
      assert.ok(metadata.preferences,'Requested Settings page must have a live model');
      if(requested_page==='list'){
        if(metadata.phone_active&&(!metadata.preferences.sidebar_visible||metadata.preferences.content_visible))issues.push('Plain Settings did not land on the phone page list');
      } else {
        assert.equal(metadata.preferences.page,requested_page,'Requested Settings model page');
        assert.equal(metadata.preferences.content_visible,true,'Requested Settings content must be rendered');
      }
    }
    const stem=`phone-${test.viewport.join('x')}-${test.theme}-${test.text_pt}pt-${state}`;
    const record={schema_version:1,host:'web',state,viewport:metadata.viewport,theme:metadata.theme,dpr:metadata.dpr,text_scale:test.text_scale,preset:metadata.workspace,phone_active:metadata.phone_active,
      status:!metadata.phone_active&&!dialogs?'gated':actual.missing.length||issues.length?'missing':'captured',availability,requested_page,issues,
      source:{...source,...metadata,text_pt:test.text_pt,context},...actual,expected_differences:[]};
    const png=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/${stem}.png`,Buffer.from(png.data,'base64'));
    await writeFile(`${directory}/${stem}.allocations.json`,JSON.stringify(record,null,2)+'\n');
    report.cases.push({state,viewport:test.viewport,theme:test.theme,text_scale:test.text_scale,status:record.status,json:`${stem}.allocations.json`,png:`${stem}.png`});
    await writeFile(`${directory}/index.json`,JSON.stringify(report,null,2)+'\n');
    return record;
  };
  await send({type:'invoke',command:'new_document'});
  const setup=await evaluate(createDrawing({width:1200,height:1600,resolution:300,space:'Srgb',depth:'U8',paper:'white',relief:0,name:'Phone drawing'}));
  assert.equal(setup.resolution,300,'New drawing recipe resolution');
  assert.deepEqual(setup.color,{space:'Srgb',depth:'U8'},'New drawing recipe color');
  assert.deepEqual(setup.paper.color.rgba,[1,1,1,1],'New drawing recipe white paper');
  assert.equal(setup.paper.transparent??false,false,'New drawing recipe opaque paper');
  assert.equal(setup.paper.relief,0,'New drawing recipe relief');
  source.document_recipe=setup;
  await wait(`!layerApp.state().document_file.busy&&!layerApp.documents.busy()&&layerApp.app.brush_ready()&&((tab)=>tab?.width===1200&&tab.height===1600)(layerApp.state().tabs.find(t=>t.active))`);
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'Srgb',depth:'U8'},'Created drawing color');
  await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
  assert.deepEqual(await evaluate('layerApp.state().settings.new_document'),original.settings.new_document,'New drawing future defaults preserved');
  const remaining=await evaluate('layerApp.app.document_tabs(0).tabs.map(tab=>({id:String(tab.id),modified:tab.modified}))');
  assert.ok(remaining.length===1||remaining.length===2,'New drawing opens at most one additional tab');
  if(remaining.length===2){
    const startup=remaining.find(tab=>tab.id===original.documents[0].id);
    assert.ok(startup,'The only old tab must be the isolated startup drawing');
    assert.equal(startup.modified,false,'Startup drawing stayed unmodified');
    await evaluate(`layerApp.documents.close(BigInt(${JSON.stringify(startup.id)}))`);
  }
  await wait('layerApp.app.document_tabs(0).tabs.length===1&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await send({type:'workspace_manager',command:{type:'switch',id:'builtin:workspace:painter'}});
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:painter'&&!JSON.parse(layerApp.app.workspace_view()).busy`);
  await send({type:'invoke',command:'sculpt'});
  await send({type:'select_tool_group',group:'blend'});
  assert.deepEqual(await evaluate('layerApp.state().tool_panels.sculpt_sets.groups.filter(group=>group.selected).map(group=>group.action.group)'),['blend'],'The actual Sculpt group is Blend before restoring Pencil');
  await send({type:'select_brush',id:2});
  await wait('layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.state().layers.map(layer=>layer.label)'),['Current ink','Paper'],'Audit starts with the native blank layer list');
  assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false,'Audit starts without a selection');
  assert.deepEqual(await evaluate('({preset:layerApp.state().brush.preset,notice:layerApp.state().notice??null,error:layerApp.state().host_error??null,status:document.querySelector("#status").textContent.trim()})'),{preset:2,notice:null,error:null,status:''},'The initial native-matched fixture has Pencil and no notice or host error');
  const click=async selector=>{
    await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n||n.disabled)throw Error('Unavailable '+${JSON.stringify(selector)});n.scrollIntoView({block:'nearest',inline:'nearest'});})()`);
    await settle();
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n||n.disabled)throw Error('Unavailable '+${JSON.stringify(selector)});const b=n.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const closeDrawer=async()=>{
    await send({type:'customize',action:{type:'close_expanded'}});
    if(await evaluate('layerApp.state().commands.find(c=>c.id==="cancel_transform")?.enabled'))await send({type:'invoke',command:'cancel_transform'});
    await wait('!layerApp.state().customization.drawer&&!document.querySelector(".content-drawer")');
  };
  const drawerReady=()=>wait(`(()=>{const d=layerApp.state().customization.drawer,r=document.querySelector('.content-drawer[data-drawer="tool"]');if(!d||!r)return false;const p=layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}).measure;return p&&r.getBoundingClientRect().width>=p.placement.bounds.width-.5;})()`);
  const saveRecording=async name=>{
    const data=await evaluate(`(${takeStrokeRecording})(layerApp.app)`),path=`${directory}/${name}.capystrokes`;
    await writeFile(path,Buffer.concat([Buffer.from('CAPYPEN4'),gzipSync(Buffer.from(data))]));
    const records=execFileSync('cargo',['run','--locked','-q','-p','layer-engine','--features','prediction-bench','--example','stroke-recording','--','dump',path],{cwd:root,encoding:'utf8'}).trim().split('\n').map(line=>JSON.parse(line));
    await writeFile(`${path}.json`,JSON.stringify(records,null,2)+'\n');return records;
  };
  const requestedCases=process.env.LAYER_PHONE_AUDIT_CASES?.split(',').map(value=>value.trim());
  const availableCases=audit.matrix;
  for(const key of requestedCases??[])assert.ok(availableCases.some(test=>`${test.viewport.join('x')}-${test.theme}`===key),`Unknown phone audit case: ${key}`);
  const selectedCases=requestedCases??audit.defaultCases??null;
  const matrix=availableCases.filter(test=>!selectedCases||selectedCases.includes(`${test.viewport.join('x')}-${test.theme}`));
  assert.ok(matrix.length,'The requested audit cases exist');
  for(const test of matrix){
    await close();
    await wait('layerApp.state().workspace_ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
    await call('Emulation.setDeviceMetricsOverride',{width:test.viewport[0],height:test.viewport[1],deviceScaleFactor:1,mobile:!!audit.mobile,...(audit.mobile?{screenWidth:412,screenHeight:891}:{})});
    await send({type:'set_theme',theme:test.theme});
    await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(`${test.text_pt}pt`)});null`);
    await settle();
    await send({type:'invoke',command:'fit_canvas'});
    const frame=dialogs?.mode==='desktop'?[]:phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]'));
    const active=await evaluate('!!layerApp.app.layout(innerWidth,innerHeight).phone');
    if(!dialogs)await capture(test,'rest',[...frame,
      {id:'current.header',role:'context',selector:'#header'},
      {id:'current.canvas',role:'context',selector:'#canvas'},
      {id:'current.capy',role:'context',selector:'#zen-capy'}],active?'phone':'desktop fallback: Web phone presentation is not enabled');
    if(mode==='input'){
      assert.equal(await evaluate('matchMedia("(pointer: coarse)").matches'),true,'The primary browser pointer is coarse');
      await send({type:'restore_settings',settings:{...original.settings,theme:test.theme,finger_drawing:'auto',pen_seen:false}});
      await send({type:'invoke',command:'drawing_brush'});await send({type:'set_brush_size',value:40});await send({type:'set_color',rgba:[0,0,0,1]});
      const {readyCanvas}=await import('./canvas-pixels-fixture.mjs');
      const ready=()=>readyCanvas({evaluate,settle});await ready();
      const area=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone.work_area');
      const start={x:area.x+area.width*.4,y:area.y+area.height*.5},end={x:area.x+area.width*.6,y:start.y};
      const roi={x:Math.floor(start.x-10),y:Math.floor(start.y-10),width:Math.ceil(end.x-start.x+20),height:20,scale:1};
      const pixels=async()=>{await ready();return canvasPixels(roi);};
      const touch=(type,touchPoints=[])=>call('Input.dispatchTouchEvent',{type,touchPoints});
      const mouse=(type,point,buttons=0)=>call('Input.dispatchMouseEvent',{type,...point,button:type==='mouseMoved'?'none':'left',buttons,clickCount:1,pointerType:'pen',force:buttons?1:0});
      const clear=async()=>{await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:2,y:2,button:'none',buttons:0});await settle();};
      const stroke=async()=>{await touch('touchStart',[{id:100,...start}]);for(let i=1;i<=20;i++)await touch('touchMove',[{id:100,x:start.x+(end.x-start.x)*i/20,y:start.y}]);await touch('touchEnd');await wait('layerApp.app.document_park_ready()');};
      const undo=async()=>{await send({type:'invoke',command:'undo'});await wait('layerApp.app.document_park_ready()');};
      const snapshot=async name=>capture(test,name,phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),'native browser contacts and the shared Finger/picker engine');
      const tap=async count=>{await touch('touchStart',Array.from({length:count},(_,i)=>({id:200+i,x:area.x+area.width*.5+(i-(count-1)/2)*25,y:start.y})));await touch('touchEnd');await settle();};
      const mode=value=>send({type:'preferences',action:{type:'edit',id:'finger_drawing',value}});
      await mouse('mouseMoved',start);assert.equal(await evaluate('layerApp.state().settings.pen_seen'),false,'Hover does not admit a pen');
      await evaluate('layerApp.dispatch({type:"layer",action:{op:"lock",id:layerApp.state().layer_tools.editing_layer.id,value:true}});null');
      await mouse('mousePressed',start,1);await mouse('mouseReleased',start);await settle();
      assert.equal(await evaluate('layerApp.state().settings.pen_seen'),false,'A refused locked-layer contact does not admit a pen');
      await undo();await clear();const blank=await pixels();
      await stroke();const auto=await pixels();assert.ok(auto.white<blank.white-20,'Coarse phone Auto paints real pixels');
      await snapshot('input-auto-stroke');await undo();assert.deepEqual(await pixels(),blank);
      await click('#zen-capy');await wait('layerApp.state().phone.menu');await click('#phone-finger');
      assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),'off');assert.equal(await evaluate('document.querySelector("#phone-finger").getAttribute("aria-pressed")'),'false');
      await close();await stroke();await send({type:'invoke',command:'fit_canvas'});assert.deepEqual(await pixels(),blank,'Off contact leaves the artwork unchanged');
      await snapshot('input-off');await click('#zen-capy');await click('#phone-finger');
      assert.equal(await evaluate('layerApp.state().settings.finger_drawing'),'on');await close();
      await stroke();assert.ok((await pixels()).white<blank.white-20,'Capy On paints');await snapshot('touch-stroke');
      await tap(2);await wait('layerApp.app.document_park_ready()');assert.deepEqual(await pixels(),blank,'Two fingers undo the prior stroke');
      await tap(3);await wait('layerApp.app.document_park_ready()');assert.ok((await pixels()).white<blank.white-20,'Three fingers redo it');
      await tap(2);await wait('layerApp.app.document_park_ready()');assert.deepEqual(await pixels(),blank);
      await evaluate('layerApp.app.start_stroke_recording();null');await touch('touchStart',[{id:100,...start}]);
      await wait('layerApp.state().color_picker.preview!=null&&layerApp.state().color_picker.loupe_point!=null');
      await ready();
      const picker=await evaluate('layerApp.state().color_picker');
      assert.ok(Math.abs(picker.sample_point[0]-start.x)<=1&&Math.abs(picker.sample_point[1]-start.y)<=1);
      assert.ok(Math.abs(picker.sample_point[1]-picker.loupe_point[1]-44)<=1,'The shared browser fallback lifts the actual picker by 44 logical pixels');
      for(const angle of [30,60,90,120,150]){
        const radians=angle*Math.PI/180,p={x:Math.round(picker.loupe_point[0]+39*Math.cos(radians)),y:Math.round(picker.loupe_point[1]+39*Math.sin(radians)),width:1,height:1,scale:1};
        assert.equal((await canvasPixels(p)).white,0,'The original black ring is rendered at its shared radius');
      }
      await snapshot('hold-active');await touch('touchEnd');await wait('layerApp.state().layer_tools.tool==="paint"&&layerApp.app.document_park_ready()');
      const held=await saveRecording(`finger-hold-${test.viewport.join('x')}-${test.theme}`),heldEvents=held.flatMap(r=>r.Raw?[r.Raw.event]:[]);
      assert.equal(heldEvents.filter(e=>e.phase==='Cancel').length,1,'The held picker cancels paint once');
      assert.deepEqual(held.flatMap(r=>r.End?[r.End.cancelled]:[]),[true]);
      assert.ok(await evaluate('layerApp.state().colors.foreground.rgba.slice(0,3).every(v=>v>.95)'),'Lifting accepts the sampled white');
      await send({type:'set_color',rgba:[0,0,0,1]});assert.deepEqual(await pixels(),blank,'Hold withdraws its provisional paint');
      const colors=await evaluate('layerApp.state().colors');await touch('touchStart',[{id:100,...start}]);await wait('layerApp.state().color_picker.preview!=null');
      await touch('touchCancel');await wait('layerApp.state().layer_tools.tool==="paint"&&layerApp.app.document_park_ready()');
      assert.deepEqual(await evaluate('layerApp.state().colors'),colors,'Native cancellation restores the picker colors');
      const pair=[{id:100,x:area.x+area.width*.5-30,y:start.y},{id:101,x:area.x+area.width*.5+30,y:start.y}];
      const beforePan=await evaluate('layerApp.state().camera.translation');await touch('touchStart',pair);
      for(let i=1;i<=3;i++)await touch('touchMove',pair.map(p=>({...p,x:p.x+i*14,y:p.y+i*9})));
      await touch('touchEnd');await settle();assert.notDeepEqual(await evaluate('layerApp.state().camera.translation'),beforePan,'Two contacts navigate after withdrawing the finger');
      await send({type:'invoke',command:'fit_canvas'});assert.deepEqual(await pixels(),blank);await snapshot('input-navigation');
      const durable=await evaluate('JSON.stringify(layerApp.app.workspace_persistence())');
      await tap(4);await wait('layerApp.state().phone.hidden');await snapshot('input-four-finger-zen');
      assert.equal(await evaluate('JSON.stringify(layerApp.app.workspace_persistence())'),durable,'Four-finger Zen preserves the saved workspace');
      await click('#zen-capy');await wait('!layerApp.state().phone.hidden');
      await touch('touchStart',[{id:100,...start}]);await touch('touchMove',[{id:100,x:start.x+12,y:start.y}]);
      await mouse('mousePressed',start,1);await touch('touchStart',[{id:100,x:start.x+12,y:start.y},{id:101,...end}]);
      for(let i=1;i<=8;i++)await mouse('mouseMoved',{x:start.x+(end.x-start.x)*i/8,y:start.y},1);
      await mouse('mouseReleased',end);await wait('layerApp.state().settings.pen_seen&&layerApp.app.document_park_ready()');
      await clear();assert.ok((await pixels()).white<blank.white-20,'The real pen wins the provisional finger');await snapshot('input-pen-wins');await undo();
      await touch('touchMove',[{id:100,...end},{id:101,...start}]);await touch('touchEnd');await clear();
      assert.deepEqual(await pixels(),blank,'Rejected fingers cannot resume after pen lift');
      await stroke();assert.ok((await pixels()).white<blank.white-20,'A fresh contact still paints in On after pen admission');await undo();
      await mode(0);await stroke();await send({type:'invoke',command:'fit_canvas'});assert.deepEqual(await pixels(),blank,'Auto stops painting after real pen admission');
      await snapshot('input-auto-after-pen');
      await send({type:'restore_settings',settings:original.settings});
      continue;
    }
    if(mode==='customize'){
      const durable=()=>evaluate('layerApp.app.workspace_persistence()');
      const history=()=>evaluate('JSON.parse(layerApp.app.workspace_capture()).history');
      const before=await durable(),oldHistory=await history();
      const tiles=()=>evaluate('layerApp.state().workspace.layout.phone');
      const point=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
      const touch=(type,position)=>call('Input.dispatchTouchEvent',{type,touchPoints:position?[{id:41,...position}]:[]});
      const hold=async position=>{
        const start=await evaluate('performance.now()');await touch('touchStart',position);
        await wait(`performance.now()-${start}>=900&&document.querySelector("#phone-frame").dataset.contact==="held"`);
        assert.equal(await evaluate('!!document.querySelector(".panel-context-menu:popover-open")'),false,'Holding lifts without opening a menu before release');
      };
      const release=async()=>{await touch('touchEnd');await settle();};
      const menu=async(tile)=>{
        const position=await point(`#phone-tool-${tile}`);await hold(position);await release();
        await wait('document.querySelector(".panel-context-menu")?.matches(":popover-open")');
      };
      const heldPointer=async(position,pointerType)=>{
        const start=await evaluate('performance.now()');
        await call('Input.dispatchMouseEvent',{type:'mousePressed',...position,button:'left',buttons:1,clickCount:1,pointerType});
        await wait(`performance.now()-${start}>=900&&document.querySelector("#phone-frame").dataset.contact==="held"`);
        assert.equal(await evaluate('!!document.querySelector(".panel-context-menu:popover-open")'),false,`${pointerType} hold arms before release`);
        await call('Input.dispatchMouseEvent',{type:'mouseReleased',...position,button:'left',buttons:0,clickCount:1,pointerType});await settle();
        if(pointerType==='pen')await wait('!!document.querySelector(".panel-context-menu:popover-open")');
        assert.equal(await evaluate('!!document.querySelector(".panel-context-menu:popover-open")'),pointerType==='pen',`${pointerType} release menu`);
      };
      const menuAction=async label=>{
        await wait(`!![...document.querySelectorAll('.panel-context-menu:popover-open button')].find(n=>n.menuItem?.label===${JSON.stringify(label)})`);
        await evaluate(`(()=>{const row=[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(n=>n.menuItem?.label===${JSON.stringify(label)});row.dataset.phoneAuditAction='';})()`);
        await click('.panel-context-menu:popover-open [data-phone-audit-action]');
      };
      const captureCustom=async name=>{
        const specs=phoneAuditFrameSpecifications(await tiles());
        if(await evaluate('!!document.querySelector(".workspace-form[open]")'))specs.push(...await evaluate(`(${dialogAuditSpecifications.toString()})('.workspace-form[open]')`));
        if(await evaluate('!!document.querySelector("#tool-picker[open]")')){
          specs.push(
            ...(await evaluate(`(${dialogAuditSpecifications.toString()})('#tool-picker[open]')`)),
            ...[['body','body','.tool-picker-body'],['catalog','catalog','.tool-picker-catalog'],['search','search','.tool-search'],['choices','scroll','.tool-picker-catalog > .panel-frame']].map(([id,role,selector])=>({id:`picker.${id}`,role,selector:`#tool-picker ${selector}`})));
          if(await evaluate('(()=>{const row=document.querySelector(".tool-choice:first-child").getBoundingClientRect(),clip=document.querySelector(".tool-picker-catalog > .panel-frame").getBoundingClientRect();return row.bottom>clip.top&&row.top<clip.bottom;})()'))specs.push(
            ...[['row.0','row','.tool-choice:first-child'],['row.0.title','label','.tool-choice:first-child > span > span'],['row.0.subtitle','label','.tool-choice:first-child > span > small']].map(([id,role,selector])=>({id:`picker.${id}`,role,selector:`#tool-picker ${selector}`})));
        }
        return capture(test,name,specs,'shared phone customization');
      };
      const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();};
      const first=(await tiles())[0].id;
      const tilePoint=await point(`#phone-tool-${first}`);
      await heldPointer(tilePoint,'mouse');
      await heldPointer(tilePoint,'pen');await escape();
      await menu(first);await captureCustom('customize-tile-menu');await menuAction('Remove Tool');
      const removed=await durable();assert.equal(removed.layout.phone.some(t=>t.id===first),false);
      assert.equal((await history()).undo.length,oldHistory.undo.length+1);
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await durable(),before);
      await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await durable(),removed);
      const anchor=(await tiles())[0].id;
      await menu(anchor);await menuAction('Insert Tools…');
      await wait('!!document.querySelector("#tool-picker.dialog-page[open]")');
      const bounds=await evaluate('(()=>{const n=document.querySelector("#tool-picker"),r=n.getBoundingClientRect(),p=layerApp.app.dialog_presentation("editor",innerWidth,innerHeight,{x:0,y:0,width:innerWidth,height:innerHeight});return{actual:[r.x,r.y,r.width,r.height],expected:Object.values(p.bounds),modal:n.matches(":modal"),header:getComputedStyle(n.firstElementChild).display};})()');
      assert.equal(bounds.modal,true);assert.equal(bounds.header,'grid');
      assert.ok(bounds.actual.every((value,i)=>Math.abs(value-bounds.expected[i])<=1),'The picker consumes shared page bounds');
      await evaluate('document.querySelector("#tool-search").value="brush";document.querySelector("#tool-search").dispatchEvent(new Event("input",{bubbles:true}));document.querySelector("#tool-search").focus();null');
      await escape();assert.equal(await evaluate('layerApp.app.tool_picker().query'),'');
      assert.equal(await evaluate('document.querySelector("#tool-picker").open'),true,'First Escape clears focused search');
      const selected=await evaluate('layerApp.app.tool_picker().choices.filter(c=>c.control.kind!=="divider").slice(0,2).map(c=>c.control)');
      for(const control of selected)await send({type:'customize',action:{type:'picker_select',control,selected:true}});
      const wrapped=await evaluate('[...document.querySelectorAll(".tool-choice > span > :is(span,small)")].filter(n=>{const range=document.createRange();range.selectNodeContents(n);const rects=[...range.getClientRects()];return rects.some(r=>Math.abs(r.top-rects[0].top)>1);}).map(n=>n.textContent)');
      assert.deepEqual(wrapped,[],'Picker titles and descriptions stay on one line');
      await captureCustom('customize-insert-two');
      const name=`phone-${test.viewport.join('x')}-${test.theme}-`;
      const references=globSync('**/*customize-insert-two.allocations.json',{cwd:gtkAllocations}).filter(path=>path.split('/').at(-1).startsWith(name));
      assert.equal(references.length,1,`Expected one matching GTK picker allocation file for ${name} in ${gtkAllocations}; found ${references.length}`);
      await verifyPickerAllocations(join(gtkAllocations,references[0]),join(directory,`phone-${test.viewport.join('x')}-${test.theme}-${test.text_pt}pt-customize-insert-two.allocations.json`));
      const prior=(await history()).undo.length;await click('#confirm-tools');
      assert.deepEqual((await tiles()).slice(0,2).map(t=>t.control),selected);
      assert.equal((await tiles())[2].id,anchor);assert.equal((await history()).undo.length,prior+1);
      await menu(anchor);await menuAction('Insert Tools…');
      const last=await evaluate('layerApp.app.tool_picker().choices.find(c=>c.control.kind!=="divider").control');
      await send({type:'customize',action:{type:'picker_select',control:last,selected:true}});await click('#confirm-tools');
      const full=await durable();assert.equal(full.layout.phone.filter(t=>t.control.kind!=='divider').length,10);
      await menu(anchor);await menuAction('Insert Tools…');
      assert.equal(await evaluate('[...document.querySelectorAll(".tool-choice")].every((row,i)=>{const c=layerApp.app.tool_picker().choices[i],n=row.querySelector("input"),children=[...row.children].filter(child=>child!==n);return c.control.kind==="divider"?!n.disabled:n.disabled&&row.getAttribute("aria-disabled")==="true"&&Number(getComputedStyle(row).opacity)===1&&children.every(child=>Number(getComputedStyle(child).opacity)<1)&&getComputedStyle(n).width==="20px";})'),true,'Capacity dims content and disables every non-divider row');
      await captureCustom('customize-capacity');
      const disabled=await evaluate('layerApp.app.tool_picker().choices.findIndex(c=>c.control.kind!=="divider")');
      await click(`.tool-choice:nth-child(${disabled+1})`);assert.equal(await evaluate('Number(layerApp.app.tool_picker().selected_count)'),0,'A disabled row cannot select a tool');
      const divider=await evaluate('layerApp.app.tool_picker().choices.findIndex(c=>c.control.kind==="divider")');
      await click(`.tool-choice:nth-child(${divider+1})`);
      assert.equal(await evaluate('layerApp.app.tool_picker().can_confirm'),true);
      await captureCustom('customize-capacity-divider');
      const unchanged=await history();await escape();assert.equal(await evaluate('layerApp.app.tool_picker()??null'),null);assert.deepEqual(await history(),unchanged);
      const sequence=await tiles(),from=await point(`#phone-tool-${sequence[0].id}`),to=await point(`#phone-tool-${sequence[2].id}`);
      to[await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone.edge==="top"?"x":"y"')]+=1;
      const moveHistory=await history();await hold(from);await touch('touchMove',to);
      await wait('document.querySelector("#phone-frame").dataset.contact==="dragging"');
      const paint=await evaluate(`(()=>{const n=document.querySelector('#phone-tool-${sequence[0].id}').parentElement,s=getComputedStyle(n);return{lifted:n.classList.contains('header-drag-preview'),shadow:s.boxShadow,background:s.backgroundColor};})()`);
      assert.equal(paint.lifted,true,JSON.stringify(paint));assert.ok(paint.shadow.includes('inset')&&paint.shadow.includes('2px'),JSON.stringify(paint));assert.notEqual(paint.background,'rgba(0, 0, 0, 0)');
      assert.deepEqual(await history(),moveHistory,'Reorder preview never enters shared history');
      await captureCustom('customize-reorder-held');await release();
      const expected=[sequence[1],sequence[2],sequence[0],...sequence.slice(3)];
      assert.deepEqual(await tiles(),expected);assert.equal((await history()).undo.length,moveHistory.undo.length+1);
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await tiles(),sequence);
      const outside={x:test.viewport[0]/2,y:test.viewport[1]/2},cancelled=await history();
      await hold(from);await touch('touchMove',outside);await release();assert.deepEqual(await history(),cancelled);
      await captureCustom('customize-offbar-cancelled');
      await hold(from);await escape();await release();assert.deepEqual(await history(),cancelled,'Escape cancels a held contact');
      const empty=await durable();empty.layout.phone=[];await send({type:'restore_workspace',workspace:empty});
      const blank=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone,occupied=[...p.tool_hits,...p.tools,...p.controls,p.capy_hit,p.settings_hit,p.size,p.strength];for(const bar of p.bars)for(let y=Math.ceil(bar.y);y<Math.floor(bar.y+bar.height);y++)for(let x=Math.ceil(bar.x);x<Math.floor(bar.x+bar.width);x++){const point={x:x+.5,y:y+.5};if(!occupied.some(b=>b.width>0&&b.height>0&&point.x>=b.x&&point.x<b.x+b.width&&point.y>=b.y&&point.y<b.y+b.height))return point;}throw Error('The shared phone bars have no free background contact');})()`);
      await hold(blank);await release();await wait('document.querySelector(".panel-context-menu")?.matches(":popover-open")');
      assert.deepEqual(await evaluate('[...document.querySelectorAll(".panel-context-menu:popover-open button")].map(n=>n.menuItem.label)'),['Add Tools…']);
      await captureCustom('customize-empty-toolbar');await menuAction('Add Tools…');await escape();
      assert.deepEqual((await durable()).layout.phone,[],'Cancelling Add Tools preserves an explicit empty toolbar');
      await send({type:'invoke',command:'reset_layout'});
      await wait('!!document.querySelector(".workspace-form[open]")');
      assert.equal(await evaluate('document.querySelector(".workspace-form").classList.contains("dialog-page")'),false,'Phone Restore stays a compact alert');
      await captureCustom('customize-phone-restore-confirm');
      await click('.workspace-form footer .suggested-action');
      await wait('!JSON.parse(layerApp.app.workspace_view()).busy&&!document.querySelector(".workspace-form[open]")');
      const restored=await durable();assert.notDeepEqual(restored.layout.phone,full.layout.phone);
      const desktop=layout=>{const {phone,next_tile_id,...rest}=layout;return rest;};
      assert.deepEqual(desktop(restored.layout),desktop(full.layout),'Phone Restore preserves desktop topology');
      await captureCustom('customize-phone-restored');
      await send({type:'restore_workspace',workspace:before});
      continue;
    }
    if(mode==='menus'){
      await send({type:'preferences',action:{type:'edit',id:'finger_drawing',value:2}});
      const route=()=>evaluate('JSON.stringify(layerApp.state().phone.menu_route)');
      const rootRoute=await route();
      const open=async()=>{if(!await evaluate('layerApp.state().phone.menu'))await click('#zen-capy');await wait('layerApp.state().phone.menu&&document.querySelector("#phone-menu-scroll")?.clientHeight>0');};
      await open();
      const bounds=()=>evaluate('document.querySelector(".menu-drawer").getBoundingClientRect().toJSON()');
      let initial=await bounds();
      const captureMenu=async (name,expectedDocuments=1,expectedExtent=[1200,1600])=>{
        const point=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone;return p.edge==='top'?{x:layerApp.state().phone.swapped?p.capy.x-4:p.capy.x+p.tile+4,y:p.capy.y+p.tile*.5}:{x:p.capy.x+p.tile*.5,y:p.capy.y+p.tile+4};})()`);
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,button:'none',buttons:0});await settle();
        const specs=[...frame,{id:'menu',role:'menu',selector:'#phone-menu'},
          {id:'menu.list',role:'menu-list',selector:'#phone-menu-scroll'},
          {id:'menu.separator',role:'separator',selector:'.menu-separator'},
          {id:'menu.page',role:'menu-page',selector:'.menu-page:not([hidden]):not([inert])'},
          {id:'menu.back',role:'back',selector:'.menu-page:not([hidden]):not([inert]) .submenu-back',optional:true},
          {id:'menu.bridge',role:'bridge',selector:'.drawer-bridge'},
          ...['preferences','search','finger','zen','swap'].map(name=>({id:`menu.tile.${name}`,role:'menu-tile',selector:`#phone-${name}`}))];
        const rows=await evaluate(`(()=>{const rows=[...document.querySelectorAll('.menu-page:not([hidden]):not([inert]) .menu-row')];return rows.map((row,i)=>{row.dataset.auditRow=i;return{index:i,parts:[['label','.menu-label'],['check','.menu-check'],['icon','.menu-icon:not([hidden])'],['arrow','.submenu-arrow,.menu-back-arrow'],['detail','.shortcut-hint:not([hidden])']].filter(([,selector])=>row.querySelector(selector)).map(([part])=>part),close:row.parentElement.classList.contains('menu-drawing-row')};});})()`);
        for(const {index,parts,close} of rows){
          const selector=`.menu-page:not([hidden]):not([inert]) .menu-row[data-audit-row="${index}"]`;
          specs.push({id:`menu.row.${index}`,role:'menu-row',selector});
          const selectors={label:'.menu-label',check:'.menu-check',icon:'.menu-icon:not([hidden])',arrow:'.submenu-arrow,.menu-back-arrow',detail:'.shortcut-hint:not([hidden])'};
          for(const part of parts){
            const suffix=part==='arrow'?` > :is(.submenu-arrow,.menu-back-arrow)`: ` ${selectors[part]}`;
            specs.push({id:`menu.row.${index}.${part}`,role:`menu-${part}`,selector:selector+suffix});
          }
          if(close)specs.push({id:`menu.row.${index}.close`,role:'menu-close',selector:selector+' + .menu-close'});
        }
        const record=await capture(test,name,specs,'typed shared Capy menu through the common drawer owner',null,expectedDocuments,{space:'Srgb',depth:'U8'},expectedExtent);
        assert.deepEqual(record.missing,[]);assert.ok(record.elements.find(e=>e.id==='menu.bridge').visible);
        const row=record.elements.find(e=>e.id==='menu.row.0'),tiles=record.elements.filter(e=>e.role==='menu-tile');
        for(const tile of tiles)assert.ok(Math.abs(tile.bounds[2]-row.bounds[3])<=1&&Math.abs(tile.bounds[3]-row.bounds[3])<=1,'The strip tiles match the native menu row height');
        const actual=await bounds();for(const axis of ['x','y','width','height'])assert.ok(Math.abs(actual[axis]-initial[axis])<=1,`${name}: root outer ${axis} remains fixed`);
        return record;
      };
      const projections=await evaluate(`layerApp.app.phone_menu().pages.map(p=>({id:p.id,parent:p.parent,title:p.title,sections:p.sections.map(s=>s.map(r=>({label:r.label,target:r.target,enabled:r.enabled,selected:r.selected})))}))`);
      async function visit(page,swapped){
        assert.equal(await route(),JSON.stringify(page.id));
        const name=page.id.page?.kind==='application'?`application-${page.id.page.menu}`:page.id.page?.kind??'main';
        await captureMenu(`menu-${name}${page.id.path.length?'-'+page.id.path.flat().join('-'):''}${swapped?'-swapped':''}`);
        for(const [s,section]of page.sections.entries())for(const [i,row]of section.entries())if(row.target){
          await click(`.menu-page:not([hidden]):not([inert]) [data-position="[${s},${i}]"]`);
          await wait(`JSON.stringify(layerApp.state().phone.menu_route)===${JSON.stringify(JSON.stringify(row.target))}`);
          await visit(projections.find(p=>JSON.stringify(p.id)===JSON.stringify(row.target)),swapped);
          await click('.menu-page:not([hidden]):not([inert]) .submenu-back');
          await wait(`JSON.stringify(layerApp.state().phone.menu_route)===${JSON.stringify(JSON.stringify(page.id))}`);
        }
      }
      const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
      for(const swapped of [false,true]){
        if(swapped)await click('#phone-swap');
        await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
        initial=await bounds();await visit(projections.find(p=>!p.parent),swapped);
        assert.equal(await route(),rootRoute);
        for(const material of [0,2]){
          await send({type:'preferences',action:{type:'edit',id:'transparency',value:material}});
          initial=await bounds();await captureMenu(`menu-root-${swapped?'swapped':'normal'}-${material?'glass':'opaque'}`);
        }
      }
      await click('#phone-swap');
      await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});initial=await bounds();
      const fileRoute=projections.find(p=>p.id.page?.menu==='file'&&!p.id.path.length).id;
      const filePosition=projections.find(p=>!p.parent).sections.flatMap((section,s)=>section.map((row,i)=>row.target?.page?.menu==='file'?[s,i]:null)).find(Boolean);
      const fileSelector=`.menu-page:not([hidden]):not([inert]) [data-position="${JSON.stringify(filePosition)}"]`;
      const key=async(name,code)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,code:name,windowsVirtualKeyCode:code});await settle();};
      const gesture=async(delta,complete)=>{
        await click(fileSelector);
        await wait(`JSON.stringify(layerApp.state().phone.menu_route)===${JSON.stringify(JSON.stringify(fileRoute))}`);
        const point=await evaluate(`(()=>{const n=document.querySelector('.menu-page:not([hidden]):not([inert]) .submenu-back'),r=n.getBoundingClientRect();return{x:r.x+24,y:r.y+r.height/2,width:document.querySelector('#phone-menu-scroll').clientWidth};})()`);
        const target={x:point.x+point.width*delta[0],y:point.y+delta[1]};
        await call('Input.dispatchMouseEvent',{type:'mousePressed',x:point.x,y:point.y,button:'left',buttons:1,clickCount:1});
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...target,button:'left',buttons:1});
        assert.equal(await evaluate('!!document.querySelector("#phone-menu [data-pressed]")'),false,'A claimed drag cancels the row highlight');
        if(delta[0]>0)assert.ok(await evaluate(`document.querySelector('.menu-page:not([hidden]):not([inert])').style.transform!==''`),'Back follows the contact before release');
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...target,button:'left',buttons:1});
        await call('Input.dispatchMouseEvent',{type:'mouseReleased',...target,button:'left',buttons:0,clickCount:1});
        await wait('!document.querySelector("#phone-menu-pages").inert');
        assert.equal(await route(),complete?rootRoute:JSON.stringify(fileRoute),'A drag neither activates its starting row nor takes an unintended Back');
        if(!complete)await key('Escape',27);
        assert.equal(await route(),rootRoute);
      };
      await gesture([.12,0],false);
      await gesture([.45,0],true);
      await gesture([0,24],false);
      await evaluate(`document.querySelector(${JSON.stringify(fileSelector)}).focus()`);
      await key('ArrowRight',39);await wait(`JSON.stringify(layerApp.state().phone.menu_route)===${JSON.stringify(JSON.stringify(fileRoute))}`);
      await key('ArrowLeft',37);assert.equal(await route(),rootRoute);
      const longest=projections.filter(page=>JSON.stringify(page.parent)===rootRoute).sort((a,b)=>b.sections.flat().length-a.sections.flat().length)[0];
      const longestPosition=projections.find(p=>!p.parent).sections.flatMap((section,s)=>section.map((row,i)=>JSON.stringify(row.target)===JSON.stringify(longest.id)?[s,i]:null)).find(Boolean);
      await click(`.menu-page:not([hidden]):not([inert]) [data-position="${JSON.stringify(longestPosition)}"]`);
      const scroll=await evaluate(`(()=>{const n=document.querySelector('#phone-menu-scroll'),r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height*.75,height:r.height,overflow:n.scrollHeight-n.clientHeight,value:n.scrollTop};})()`);
      assert.ok(scroll.overflow-scroll.value>1,'The longest submenu provides a real scrolling workload from its initial offset');
      const touch=(type,touchPoints=[])=>call('Input.dispatchTouchEvent',{type,touchPoints});
      await touch('touchStart',[{id:100,x:scroll.x,y:scroll.y}]);
      for(let i=1;i<=4;i++)await touch('touchMove',[{id:100,x:scroll.x,y:scroll.y-Math.min(100,scroll.height*.4)*i/4}]);
      await wait(`document.querySelector("#phone-menu-scroll").scrollTop>${scroll.value+1}`);
      assert.equal(await evaluate('!!document.querySelector("#phone-menu [data-pressed]")'),false,'Native touch scrolling cancels the pressed row');
      await touch('touchEnd');await settle();
      assert.equal(await route(),JSON.stringify(longest.id),'Touch scrolling never activates a row');
      assert.equal(await evaluate('!!document.querySelector("dialog[open]")'),false);
      await key('Escape',27);assert.equal(await route(),rootRoute);
      for(const name of ['finger','swap']){
        const before=await evaluate(`document.querySelector('#phone-${name}').getAttribute('aria-pressed')`);
        await click(`#phone-${name}`);assert.equal(await evaluate('layerApp.state().phone.menu'),true);
        assert.notEqual(await evaluate(`document.querySelector('#phone-${name}').getAttribute('aria-pressed')`),before);
        initial=await bounds();await captureMenu(`menu-${name}-checked`);
        await click(`#phone-${name}`);initial=await bounds();
      }
      await click('#phone-search');await wait('!layerApp.state().phone.menu&&document.querySelector("#command-bar").open');
      await evaluate(`(()=>{const input=document.querySelector('#command-search');input.value='pencil';input.dispatchEvent(new Event('input',{bubbles:true}));})()`);
      await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
      assert.equal(await evaluate('document.querySelector("#command-bar").open&&document.querySelector("#command-search").value===""'),true,'First Escape clears focused search');
      await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
      await wait('!document.querySelector("#command-bar").open');
      await open();await click('#phone-preferences');
      await wait('!layerApp.state().phone.menu&&!!layerApp.app.preferences()&&!!document.querySelector("dialog[open]")');
      await close();
      await open();await click('#phone-zen');
      await wait('!layerApp.state().phone.menu&&layerApp.state().phone.hidden');
      await key('Escape',27);await wait('!layerApp.state().phone.hidden');
      await open();await click('#phone-tool-settings');
      await wait('!layerApp.state().phone.menu&&!!layerApp.state().customization.drawer');
      await closeDrawer();
      await send({type:'invoke',command:'drawings'});await wait('layerApp.state().phone.menu&&layerApp.state().phone.menu_route.page?.kind==="drawings"');
      assert.equal(await evaluate('!!document.querySelector(".drawing-list[open]")'),false,'Drawings uses the same phone menu page');
      await close();
      const first=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
      await send({type:'invoke',command:'add_layer'});await wait('layerApp.app.document_tabs(0).tabs[0].modified&&layerApp.app.document_park_ready()');
      await open();await click('#phone-drawing');
      await click('.menu-page:not([hidden]):not([inert]) [data-position="[1,0]"]');
      await wait('!layerApp.state().phone.menu&&!!document.querySelector("dialog[open]")');
      await evaluate(createDrawing({width:128,height:96,resolution:300,space:'Srgb',depth:'U8',paper:'white',relief:0,name:'Menu drawing'}));
      await wait('layerApp.app.document_tabs(0).tabs.length===2&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
      const second=await evaluate('Number(layerApp.app.document_tabs(0).selected)');assert.notEqual(first,second);
      await open();initial=await bounds();await captureMenu('menu-root-two',2,[128,96]);
      await click('#phone-drawing');await captureMenu('menu-drawings-two',2,[128,96]);
      await click(`#phone-drawing-${first}`);
      await wait(`!layerApp.state().phone.menu&&Number(layerApp.app.document_tabs(0).selected)===${first}&&!layerApp.documents.busy()`);
      await send({type:'invoke',command:'drawings'});initial=await bounds();await captureMenu('menu-drawings-current',2);
      await click(`#phone-drawing-${first} + .menu-close`);
      await wait('!layerApp.state().phone.menu&&!!document.querySelector("dialog[open]")');
      const discard=await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label');
      const discardSelector=await evaluate(`(()=>{const buttons=[...document.querySelectorAll('dialog[open] footer button')],button=buttons.find(n=>n.textContent===${JSON.stringify(discard)});if(!button)throw Error('Missing discard response');button.dataset.auditDiscard='true';return '[data-audit-discard]';})()`);
      await click(discardSelector);
      await wait(`layerApp.app.document_tabs(0).tabs.length===1&&Number(layerApp.app.document_tabs(0).selected)===${second}&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy`);
      await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
      await close();continue;
    }
    if(mode==='drawers'){
      const fixture={send,evaluate,wait,call,settle,click};
      const selectedStates=selectPhoneDrawerStates(process.env.LAYER_PHONE_DRAWER_STATES);
      const wanted=name=>!selectedStates||selectedStates.has(name);
      const capturedStates=new Set();
      const assertRequested=()=>assert.deepEqual([...selectedStates].filter(name=>!capturedStates.has(name)),[],`Requested drawer states were not captured`);
      const captureDrawer=async name=>{
        try {await drawerReady();}
        catch(error){
          const stem=`${directory}/phone-${test.viewport.join('x')}-${test.theme}-${name}-failure`;
          const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${stem}.png`,Buffer.from(png.data,'base64'));
          await writeDrawerAudit(evaluate,`${stem}.allocations.json`,name);
          throw Error(`${name}: drawer did not settle`,{cause:error});
        }
        await settle();
        await auditLayerPreviews({evaluate,wait});
        const actual=await evaluate(`(${phoneAuditDrawerSpecifications.toString()})()`);
        const record=await capture(test,name,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),...actual],'existing shared drawer owner');
        assert.equal(record.status,'captured');assert.deepEqual(record.missing,[]);
        const drawer=record.elements.find(e=>e.id==='drawer');
        assert.ok(drawer?.visible&&record.elements.some(e=>e.id==='drawer.bridge'&&e.visible),'The opener retains a visible drawer and bridge');
        assert.equal(record.elements.filter(e=>e.role==='drawer-column').length,(await evaluate('layerApp.state().customization.drawer')).columns.length);
        const naturals=await evaluate(`(()=>{const minimums=layerApp.app.drawer_measure({viewport:[innerWidth,innerHeight],column:null,tiles:[]}).minimums;return [...document.querySelectorAll('.content-drawer[data-drawer="tool"] > .drawer-contents > .drawer-column')].flatMap((column,i)=>[...column.children].map((panel,j)=>{const s=getComputedStyle(panel);return{minimum:minimums[i][j],height:panel.getBoundingClientRect().height+parseFloat(s.marginTop)+parseFloat(s.marginBottom)};}));})()`);
        for(const {minimum,height}of naturals)if(minimum)assert.ok(Math.abs(height-minimum)<=1,`A scroll body keeps its requested natural extent: ${height} versus ${minimum}`);
        capturedStates.add(name);
        return record;
      };
      for(const [preset,tag]of [['painter','sketch'],['illustrator','paint'],['photographer','photo']]){
        await closeDrawer();await send({type:'workspace_manager',command:{type:'switch',id:`builtin:workspace:${preset}`}});
        await wait(`JSON.parse(layerApp.app.workspace_view()).id===${JSON.stringify(`builtin:workspace:${preset}`)}&&!JSON.parse(layerApp.app.workspace_view()).busy`);
        const tiles=await evaluate('layerApp.state().workspace.layout.phone');
        for(const tile of tiles.filter(tile=>tile.control.kind!=='divider')){
          const name=tile.control.command??tile.control.slot??tile.control.panel??tile.control.kind;
          const state=`inventory-${tag}-${name.replaceAll('_','')}`;
          if(selectedStates&&![state,`${state}-bottom`,...(tile.control.panel==='layers'?[`inventory-${tag}-layers-properties`]:[])].some(wanted))continue;
          const transforming=name==='scale_rotate';
          if(transforming)await auditOwnedImage(fixture);
          if(preset==='painter'&&(tile.control.panel==='layers'||tile.control.kind==='color'))await send({type:'select_brush',id:2});
          const selector=`#phone-tool-${tile.id}`;
          await openPhoneDrawerTile(click,()=>evaluate('!!layerApp.state().customization.drawer'),selector);
          if(wanted(state))await captureDrawer(state);
          const color=await evaluate(`(()=>{const n=document.querySelector('.content-drawer .color-wheel'),c=n?.closest('.drawer-column');if(!n||!c)return null;return{wheel:n.getBoundingClientRect().toJSON(),column:c.getBoundingClientRect().toJSON()}})()`);
          if(color)assert.ok(color.wheel.left>=color.column.left-1&&color.wheel.right<=color.column.right+1&&color.wheel.top>=color.column.top-1&&color.wheel.bottom<=color.column.bottom+1,'The Color wheel stays inside its live column');
          if(color){
            const scroll=await evaluate(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents'),r=n.getBoundingClientRect();return{shared:n.classList.contains('shared-scroll'),extent:n.scrollHeight,height:n.clientHeight,x:r.x+2,y:r.y+r.height/2};})()`);
            assert.equal(scroll.shared,true,'Color uses the shared whole-drawer scroll owner');
            if(scroll.extent>scroll.height+1){
              await call('Input.dispatchMouseEvent',{type:'mouseWheel',x:scroll.x,y:scroll.y,deltaX:0,deltaY:scroll.extent});
              await wait(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents');return n.scrollTop+n.clientHeight>=n.scrollHeight-1;})()`);
              if(wanted(`${state}-bottom`))await captureDrawer(`${state}-bottom`);
              assert.equal(await evaluate(`(()=>{const n=document.querySelector('.content-drawer .drawer-contents').getBoundingClientRect(),f=document.querySelector('.content-drawer .palette-footer').getBoundingClientRect();return f.top>=n.top-1&&f.bottom<=n.bottom+1;})()`),true,'The Color footer is reachable at the whole drawer bottom');
            }
          }
          const rows=await evaluate('document.querySelectorAll(".content-drawer .tool-subtools .tool-choice-button").length');
          if(rows){
            await evaluate(`(()=>{const n=document.querySelector('.content-drawer .tool-subtools .tool-choice-button:last-child').closest('.drawer-column');n.scrollTop=n.scrollHeight;})()`);await settle();
            if(wanted(`${state}-bottom`))await captureDrawer(`${state}-bottom`);
          }
          if(name==='sculpt'){
            const previous=await evaluate(`document.querySelector('.content-drawer .tool-groups button[aria-pressed="true"]').getAttribute('aria-label')`);
            const edge=await evaluate(`(()=>{for(const n of document.querySelectorAll('.content-drawer .tool-groups button[aria-pressed="false"]:not(:disabled)')){n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect(),x=r.right-2,y=r.y+r.height/2;if(n.contains(document.elementFromPoint(x,y)))return{x,y,label:n.getAttribute('aria-label')};}throw Error('No unobscured unselected group edge');})()`);
            for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:edge.x,y:edge.y,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
            await wait(`[...document.querySelectorAll('.content-drawer .tool-groups button')].some(n=>n.getAttribute('aria-label')===${JSON.stringify(edge.label)}&&n.getAttribute('aria-pressed')==='true')`);
            await click(`.content-drawer .tool-groups button[aria-label=${JSON.stringify(previous)}]`);
            await wait(`[...document.querySelectorAll('.content-drawer .tool-groups button')].some(n=>n.getAttribute('aria-label')===${JSON.stringify(previous)}&&n.getAttribute('aria-pressed')==='true')`);
          }
          if(tile.control.panel==='layers'&&wanted(`inventory-${tag}-layers-properties`)){
            await click('.content-drawer .layer-properties-disclosure');
            await wait(`getComputedStyle(document.querySelector('.content-drawer .layer-properties-pages > .layers-panel')).visibility==='hidden'`);
            await captureDrawer(`inventory-${tag}-layers-properties`);
            await click('.content-drawer .layer-properties-disclosure');
            await wait(`getComputedStyle(document.querySelector('.content-drawer .layer-properties-pages > .properties-panel')).visibility==='hidden'`);
          }
          await closeDrawer();
          if(transforming)await auditLayerClose(fixture);
        }
        if(wanted(`inventory-${tag}-settings`)){
          await click('#phone-tool-settings');await captureDrawer(`inventory-${tag}-settings`);
          assert.deepEqual((await evaluate('layerApp.state().customization.drawer')).columns,[['tool_settings']]);
        }
      }
      await closeDrawer();
      for(const panel of ['navigator','histogram','waveform','stats','sizes','brushes']){
        if(!wanted(`inventory-window-${panel}`))continue;
        await send({type:'phone',action:{kind:'panel',panel}});await captureDrawer(`inventory-window-${panel}`);await closeDrawer();
      }
      if(!selectedStates||[...selectedStates].some(state=>state.startsWith('inventory-settings-')))await send({type:'select_brush',id:2});
      for(const command of await evaluate('layerApp.app.catalog().tool_commands')){
        if(!wanted(`inventory-settings-${command}`))continue;
        let offered=await evaluate(`layerApp.state().commands.find(item=>item.id===${JSON.stringify(command)})`);
        const transforming=command==='scale_rotate';
        if(transforming){await auditOwnedImage(fixture);offered=await evaluate(`layerApp.state().commands.find(item=>item.id===${JSON.stringify(command)})`);}
        if(!offered?.enabled){report.unreachable.push({state:`inventory-settings-${command}`,viewport:test.viewport,theme:test.theme,status:'unavailable',reason:offered?.disabled_reason??'The shared command is not offered in this fixture',png:null});if(transforming)await auditLayerClose(fixture);continue;}
        await closeDrawer();await send({type:'invoke',command});
        if(transforming)assert.equal(await evaluate('layerApp.state().commands.find(item=>item.id==="scale_rotate").selected'),true,'The owned image selects the actual Scale/Rotate tool');
        await send({type:'phone',action:{kind:'panel',panel:'tool_settings'}});
        await captureDrawer(`inventory-settings-${command}`);await closeDrawer();
        if(await evaluate('layerApp.state().commands.find(c=>c.id==="cancel_transform")?.enabled'))await send({type:'invoke',command:'cancel_transform'});
        if(transforming)await auditLayerClose(fixture);
      }
      const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
      for(const swapped of [false,true]){
        if(await evaluate('layerApp.state().phone.swapped')!==swapped)await send({type:'phone',action:{kind:'swap_sides'}});
        for(const material of [0,2]){
          const variant=`${swapped?'swapped':'normal'}-${material?'glass':'opaque'}`;
          if(!wanted(`drawer-capy-layers-${variant}`)&&!wanted(`drawer-settings-${variant}`))continue;
          await send({type:'preferences',action:{type:'edit',id:'transparency',value:material}});
          if(wanted(`drawer-capy-layers-${variant}`)){
            await send({type:'phone',action:{kind:'panel',panel:'layers'}});
            await captureDrawer(`drawer-capy-layers-${variant}`);
            const anchor=await evaluate('layerApp.state().customization.drawer.anchor');
            assert.equal(anchor.kind,'phone');assert.equal(anchor.anchor.kind,'capy');
            await closeDrawer();
          }
          if(wanted(`drawer-settings-${variant}`)){
            await click('#phone-tool-settings');await captureDrawer(`drawer-settings-${variant}`);await closeDrawer();
          }
        }
      }
      await send({type:'phone',action:{kind:'swap_sides'}});
      await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
      if(!wanted('layers-live-capped')){await closeDrawer();if(selectedStates)assertRequested();continue;}
      await send({type:'phone',action:{kind:'panel',panel:'layers'}});
      await drawerReady();await settle();
      const count=await evaluate('layerApp.state().layers.length');
      const originalHeight=await evaluate('document.querySelector(".content-drawer").getBoundingClientRect().height');
      for(let i=0;i<24;i++){
        await click('.content-drawer .layer-footer [data-footer-button="new_layer"]');
        await wait(`layerApp.state().layers.length===${count+i+1}`);
      }
      await wait(`(()=>{const rows=document.querySelector('.content-drawer .layer-rows'),id=layerApp.state().layer_tools.editing_layer.id,row=rows.querySelector('[data-layer="'+id+'"]');if(!row)return false;const b=rows.getBoundingClientRect(),r=row.getBoundingClientRect();return rows.scrollHeight>rows.clientHeight&&r.top>=b.top-1&&r.bottom<=b.bottom+1;})()`);
      const grown=await captureDrawer('layers-live-capped');
      const grownHeight=grown.elements.find(e=>e.id==='drawer').bounds[3];
      const budget=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone;return p.edge==='top'?p.work_area.height:p.bars[0].height;})()`);
      assert.ok(Math.abs(grownHeight-budget)<=1,'Many layers reach the shared live phone budget');
      assert.ok(originalHeight>=budget-1||grownHeight>originalHeight,'Layers grows when its initial content is below the cap');
      for(let i=0;i<24;i++){
        await send({type:'invoke',command:'undo'});
        await wait(`layerApp.state().layers.length===${count+23-i}`);
      }
      await closeDrawer();
      if(selectedStates)assertRequested();
      continue;
    }
    if(mode==='frame'){
      assert.equal(active,true,'Web phone capability is presented');
      const check=async(name)=>{
        const p=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone');
        const point=await evaluate(`(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone;if(layerApp.state().phone.hidden){const r=document.querySelector('#zen-capy').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};}return p.edge==='top'?{x:layerApp.state().phone.swapped?p.capy.x-4:p.capy.x+p.tile+4,y:p.capy.y+p.tile*.5}:{x:p.capy.x+p.tile*.5,y:p.capy.y+p.tile+4};})()`);
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,button:'none',buttons:0});await settle();
        const record=await capture(test,name,phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),'shared phone frame');
        assert.deepEqual(record.missing,[],`${name}: every frame element is present`);
        const expected=new Map([
          ['bar.tools',p.bars[0]],['bar.capy',p.bars[1]],['chip.tools',p.chips[0]],['chip.capy',p.chips[1]],
          ['capy',p.capy],['tile.settings',p.settings],['slider.size',p.size],['slider.strength',p.strength],
          ...record.source.phone_tiles.filter(t=>t.control.kind!=='divider').map(t=>[`tile.${t.id}`,p.tools[record.source.phone_tiles.indexOf(t)]]),
        ]);
        for(const [id,bounds]of expected){
          const actual=record.elements.find(e=>e.id===id);assert.ok(actual,id);
          const visible=bounds.width>0&&bounds.height>0;assert.equal(actual.visible,visible,`${name}/${id}: visibility`);
          if(visible)[bounds.x,bounds.y,bounds.width,bounds.height].forEach((v,i)=>assert.ok(Math.abs(actual.bounds[i]-v)<=1,`${name}/${id}: ${actual.bounds} vs ${JSON.stringify(bounds)}`));
        }
        for(const name of ['size','strength']){
          const slider=record.elements.find(e=>e.id===`slider.${name}`),track=record.elements.find(e=>e.id===`slider.${name}.track`);
          if(!slider.visible)continue;
          const [x,y,w,h]=slider.bounds,[tx,ty,tw,th]=track.bounds;
          if(p.edge==='top')assert.ok(Math.abs(tx-x)<=1&&Math.abs(tw-w)<=1&&Math.abs(ty+th/2-y-h/2)<=1,'The horizontal trough spans and centers on its slider allocation');
          else assert.ok(Math.abs(ty-y)<=1&&Math.abs(th-h)<=1&&Math.abs(tx+tw/2-x-w/2)<=1,'The vertical trough spans and centers on its slider allocation');
        }
        assert.equal(await evaluate('document.querySelector("#header").getClientRects().length'),0,'Phone hides the desktop header');
        const selected=await evaluate('[...document.querySelectorAll("#phone-frame .phone-tool[aria-pressed=true]")].filter(n=>n.checkVisibility()).map(n=>getComputedStyle(n).backgroundColor)');
        assert.ok(selected.every(color=>color!=='rgba(0, 0, 0, 0)'),'Selected phone tools have a visible selection surface');
        assert.deepEqual(await evaluate('[layerApp.state().phone.native,layerApp.state().phone.native_counts,layerApp.state().phone.movable]'),[[0,0],[0,0],false]);
      };
      await check('frame-rest');
      await send({type:'set_brush_size',value:40});
      await send({type:'set_color',rgba:[0,0,0,1]});
      assert.deepEqual(await evaluate('layerApp.state().colors.foreground.rgba'),[0,0,0,1]);
      await send({type:'phone',action:{kind:'swap_sides'}});await check('frame-swapped');
      await send({type:'phone',action:{kind:'swap_sides'}});
      await send({type:'invoke',command:'zen_mode'});await check('frame-hidden');
      await evaluate('document.querySelector("#zen-capy").click()');await settle();
      assert.equal(await evaluate('layerApp.state().phone.hidden'),false,'The same Capy reveals the bars');
      await check('frame-revealed');
      const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
      for(const value of [0,3]){
        await send({type:'preferences',action:{type:'edit',id:'transparency',value}});
        await check(value===0?'frame-opaque':'frame-glass');
      }
      await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});
      await evaluate('window.phoneFrameWorkspace=layerApp.state().workspace;null');
      await send({type:'phone',action:{kind:'insert_tools',before:null}});
      for(const command of ['drawing_brush','eraser'])await send({type:'customize',action:{type:'picker_select',control:{kind:'command',command},selected:true}});
      await send({type:'customize',action:{type:'confirm_tools'}});
      assert.equal(await evaluate('layerApp.state().workspace.layout.phone.length'),10);
      await check('frame-ten-tools');
      await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:{...phoneFrameWorkspace,layout:{...phoneFrameWorkspace.layout,phone:[]}}});null');await settle();
      await check('frame-empty');
      await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:phoneFrameWorkspace});delete window.phoneFrameWorkspace;null');await settle();
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:19,right:7,bottom:13,left:5}});
      await wait('JSON.stringify(layerApp.state().phone.insets)===JSON.stringify([5,19,7,13])');
      await check('frame-safe-insets');
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:13,right:5,bottom:19,left:7}});
      await wait('JSON.stringify(layerApp.state().phone.insets)===JSON.stringify([7,13,5,19])');
      await check('frame-safe-redistributed');
      await call('Emulation.setSafeAreaInsetsOverride',{insets:{top:0,right:0,bottom:0,left:0}});
      await wait('layerApp.state().phone.insets.every(v=>v===0)');
      for(const name of ['size','strength']){
        const selector=`#component-slider-${name} input.number-slider`;
        const b=await evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
        const p=await evaluate('layerApp.app.layout(innerWidth,innerHeight).phone');
        const vertical=p.edge!=='top',start={x:b.x+(vertical?2:b.width/2),y:b.y+(vertical?b.height/2:2)};
        const allocation=p[name];
        assert.ok(Math.abs((vertical?b.width:b.height)-(vertical?allocation.width:allocation.height))<=1,'Slider input covers the full bar thickness');
        assert.equal(await evaluate(`document.elementFromPoint(${start.x},${start.y})?.matches('input.number-slider')`),true,'The bar-edge contact reaches the slider');
        const mouse=(type,point)=>call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
        await mouse('mousePressed',start);await mouse('mouseReleased',start);await settle();
        await wait('!!document.querySelector(".toolbar-brush-preview:popover-open")');
        const preview={id:`slider.${name}.preview`,role:'preview',selector:'.toolbar-brush-preview:popover-open'};
        await capture(test,`slider-${name}-tap`,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),preview],'existing shared slider preview');
        const strip=await evaluate(`document.querySelector(${JSON.stringify(`#component-slider-${name}`)}).getBoundingClientRect().toJSON()`);
        const end=vertical?{x:strip.right+2*p.tile,y:test.viewport[1]*.65}:{x:test.viewport[0]*.65,y:strip.y-2*p.tile};
        const from={x:b.x+b.width/2,y:b.y+b.height/2};
        const along=vertical?{x:from.x,y:from.y-12}:{x:from.x+12,y:from.y};
        await mouse('mousePressed',from);await mouse('mouseMoved',along);await settle();
        await wait('!!document.querySelector(".toolbar-brush-preview:popover-open")');
        await capture(test,`slider-${name}-track`,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),preview],'held native slider track contact');
        await mouse('mouseMoved',end);await settle();
        const actual=await evaluate(`layerApp.app.toolbar_component({kind:${JSON.stringify(name==='size'?'brush_size_slider':'brush_strength_slider')}}).numeric`);
        const expected=await evaluate(`layerApp.app.number_input({control:layerApp.app.toolbar_component({kind:${JSON.stringify(name==='size'?'brush_size_slider':'brush_strength_slider')}}).numeric.numeric,value:${actual.value},operation:{type:'position',position:${vertical ? .35 : .65}}}).value`);
        assert.ok(Math.abs(actual.value-expected)<=Math.max(.001,Math.abs(expected)*.001),`${name}: far mapping uses the window extent, ${actual.value} vs ${expected}`);
        await capture(test,`slider-${name}-away`,[...phoneAuditFrameSpecifications(await evaluate('layerApp.state().workspace.layout.phone??[]')),preview],'shared window-extent slider mapping');
        await mouse('mouseReleased',end);await settle();
        assert.equal(await evaluate('!!document.querySelector(".toolbar-brush-preview:popover-open")'),false,'A drag lift closes its preview');
      }
      continue;
    }
    const open=async action=>{if(typeof action==='function')await action({send,evaluate,wait,call,reload,settle,click});else for(const step of Array.isArray(action)?action:[action])await send(step);};
    if(dialogs)for(const row of rows){
      if(row.blocked){report.unreachable.push({state:row.id,viewport:test.viewport,theme:test.theme,dpr:1,text_scale:test.text_scale,preset:'builtin:workspace:painter',phone_active:active,status:'unavailable',missing:[],reason:row.blocked,png:null});continue;}
      try{
      await open(row.openaction);await wait(`!!document.querySelector(${JSON.stringify(row.rootselector)})`);
      if(await evaluate(`document.querySelector(${JSON.stringify(row.rootselector)})?.tagName==='DIALOG'&&!document.querySelector(':popover-open')`))await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:0,y:0,button:'none',buttons:0});
      await settle();
      if(row.id==='preferences-root'&&test.viewport[0]===320&&test.viewport[1]===320){
        const about=await evaluate(`(()=>{const node=document.querySelector('[data-settings-page="about"]'),body=node.closest('.preferences-sidebar-body');node.scrollIntoView({block:'nearest'});const r=node.getBoundingClientRect(),b=body.getBoundingClientRect();return{top:r.top,bottom:r.bottom,bodyTop:b.top,bodyBottom:b.bottom,scroll:body.scrollTop};})()`);
        assert.ok(about.scroll>0&&about.top>=about.bodyTop-.5&&about.bottom<=about.bodyBottom+.5,'320-square Settings root scrolls About into its body');
        await click('[data-settings-page="about"]');await wait('layerApp.app.preferences().page==="about"&&layerApp.app.preferences().page_requested');
        await evaluate('layerApp.dispatch({type:"preferences",action:layerApp.app.preferences().navigation.back});null');await wait('!layerApp.app.preferences().page_requested');
        await evaluate('document.querySelector(".preferences-sidebar-body").scrollTop=0');await settle();
      }
      if(row.id==='preferences-color'&&test.viewport[0]===412&&test.theme==='light'){
        await click('label[for="color-profile-library"]');await wait('!!document.querySelector(".profile-library[open]")');
        await wait(`!!document.querySelector('.profile-library[open] footer button:not(:disabled)')`);
        await click('.profile-library[open] [data-dialog-part="end"]:not(:disabled)');
        await wait('!document.querySelector(".profile-library[open]")&&layerApp.app.preferences().page==="color"');
      }
      if(row.id==='choose-action'){
        const before=await evaluate(`(()=>{const search=document.querySelector('#action-picker-search'),viewport=document.querySelector('#action-picker .preference-viewport');const y=search.getBoundingClientRect().y;viewport.scrollTop=viewport.scrollHeight;return{y,overflow:viewport.scrollHeight-viewport.clientHeight};})()`);
        await settle();
        const after=await evaluate(`(()=>{const search=document.querySelector('#action-picker-search'),viewport=document.querySelector('#action-picker .preference-viewport');const result={y:search.getBoundingClientRect().y,scroll:viewport.scrollTop};viewport.scrollTop=0;return result;})()`);
        assert.ok(before.overflow>0&&after.scroll>0,'Choose Action has scrolling results');
        assert.equal(after.y,before.y,'Scrolling actions keeps Search in its native top bar');
        await settle();
      }
      if(row.id==='preferences-shortcuts-direct'&&active){
        await click('#shortcuts-search');await call('Input.insertText',{text:'Escape'});await settle();
        assert.equal(await evaluate('layerApp.app.preferences().shortcut_query'),'Escape');
        const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();};
        await escape();assert.deepEqual(await evaluate('({query:layerApp.app.preferences().shortcut_query,page:layerApp.app.preferences().page,requested:layerApp.app.preferences().page_requested,open:document.querySelector("#settings").open})'),{query:'',page:'shortcuts',requested:true,open:true},'Escape clears Shortcuts search before navigating');
        await escape();assert.equal(await evaluate('document.querySelector("#settings").open&&!layerApp.app.preferences().page_requested'),true,'The next Escape returns to the Settings landing');
        await send(shortcuts);await settle();
      }
      if(row.requestedpage)assert.equal(await evaluate('layerApp.app.preferences().page'),row.requestedpage,'Requested Settings page');
      const shot=async route=>{await assertSettingsPresentation(evaluate,route.id);if(route.drawingstate)await drawingState(evaluate,route.drawingstate);if(route.id==='new-drawing-footer-paper-color'){const {assertColorEditorCells}=await import('./color-editor.test.mjs');await assertColorEditorCells(evaluate,'#new-drawing-color-drawer');}const parts=await evaluate(row.collector==='menu'?JSON.stringify([{id:'page',role:'page',selector:row.rootselector}]):row.collector==='drawer'?`(${phoneAuditDrawerSpecifications.toString()})()`:`(${dialogAuditSpecifications.toString()})(${JSON.stringify(row.rootselector)})`);const specifications=dialogPhoneSpecifications(frame,parts,active);return capture(test,route.id,specifications,dialogs?'existing dialog presentation':'existing Web editor; phone page/header parity not established',null,route.document?.count==='photo'?await evaluate('phoneAuditPhotoCount'):route.document?.count??1,route.document?.color??{space:'Srgb',depth:'U8'},route.document?.extent,route.document?.draft);};
      if(row.id==='color'){
        const {assertColorEditorCells}=await import('./color-editor.test.mjs');
        await assertColorEditorCells(evaluate);
      }
      if(row.id==='sdr-rendition'){const {assertSdrDialLayout}=await import('./hdr.test.mjs');await assertSdrDialLayout(evaluate);}
      await shot(row);
      for(const variant of row.variants??[]){await open(variant.openaction);await shot(variant);}
      if(row.closeaction)await open(row.closeaction);
      if(row.batch==='w6a')await send({type:'close_settings'});
      await close();
      }finally{if(row.finalize)await open(row.finalize);}
    }
    await writeFile(`${directory}/index.json`,JSON.stringify(report,null,2)+'\n');
  }
  if(mode==='menus'){
    await close();
    await call('Emulation.setDeviceMetricsOverride',{width:1600,height:1000,deviceScaleFactor:1,mobile:false});
    await evaluate("document.documentElement.style.setProperty('--ui-text-size','11pt');null");
    await settle();
    for(const theme of ['light','dark'])for(const menu of ['primary','file','edit','layer','select','view','window']){
      await send({type:'set_theme',theme});
      const selector='#header [data-kind="menu"] details';
      await click(`${selector} > summary`);
      await wait(`document.querySelector(${JSON.stringify(selector)})?.open`);
      if(menu!=='primary'){
        const index=await evaluate(`[...document.querySelectorAll('${selector} .popover > button')].findIndex(node=>node.menuItem?.page?.kind==='application'&&node.menuItem.page.menu===${JSON.stringify(menu)})`);
        assert.ok(index>=0,`Desktop menu route ${menu} exists`);
        await click(`${selector} .popover > button:nth-of-type(${index+1})`);
        await wait(`!!document.querySelector('${selector} .submenu-back')`);
      }
      const rows=await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(selector)}).querySelector('.popover'),box=n=>{const r=n.getBoundingClientRect();return[r.x,r.y,r.width,r.height];};return{menu:box(root),rows:[...root.querySelectorAll(':scope > button')].map(n=>({label:n.querySelector('.menu-label')?.textContent,hint:n.querySelector('.shortcut-hint')?.textContent,bounds:box(n)}))};})()`);
      const name=`desktop-reference-${menu}-${theme}`;
      await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      await writeFile(`${directory}/${name}.json`,JSON.stringify({host:'web',theme,viewport:[1600,1000],text_pt:11,...rows},null,2)+'\n');
      await click(`${selector} > summary`);
    }
  }
  if(mode==='frame'){
    const settings=await evaluate('layerApp.state().settings');
    await send({type:'restore_settings',settings:{...settings,finger_drawing:'on'}});
    await send({type:'invoke',command:'brush'});
    await wait('layerApp.app.brush_ready()');
    const origin=await evaluate('(()=>{const p=layerApp.app.layout(innerWidth,innerHeight).phone.work_area;return{x:p.x+p.width*.4,y:p.y+p.height*.5};})()');
    const touch=(type,touchPoints=[])=>call('Input.dispatchTouchEvent',{type,touchPoints});
    for(const cancelled of [false,true]){
      const history=await evaluate('layerApp.state().commands.filter(c=>c.id==="undo"||c.id==="redo").map(c=>[c.id,c.enabled])');
      assert.equal(await evaluate(`document.elementFromPoint(${origin.x},${origin.y})===layerApp.canvas`),true,'A recorded contact reaches the canvas rather than a completion prompt');
      await evaluate('layerApp.app.start_stroke_recording();null');
      await touch('touchStart',[{id:100,...origin}]);
      for(let i=1;i<=4;i++)await touch('touchMove',[{id:100,x:origin.x+5*i,y:origin.y}]);
      await wait('Number(layerApp.app.stroke_recording_status().raw_events)>=3');
      if(cancelled)await touch('touchStart',[{id:100,x:origin.x+20,y:origin.y},{id:101,x:origin.x+60,y:origin.y}]);
      await touch('touchEnd');
      await wait('layerApp.app.document_park_ready()');
      const records=await saveRecording(`finger-${cancelled?'withdrawn':'paint'}`);
      const events=records.flatMap(record=>record.Raw?[record.Raw.event]:[]);
      assert.ok(events.length>=3,'Native browser touch passes the packed Wasm paint ABI');
      assert.ok(events.every(event=>event.tool==='Finger'&&event.pressure===1),'Every decoded browser paint sample is Finger at full pressure');
      assert.equal(events.filter(event=>event.phase==='Cancel').length,cancelled?1:0,'Withdrawal cancels exactly once');
      const ends=records.flatMap(record=>record.End?[record.End]:[]);
      assert.equal(ends.length,1,'One engine stroke ends per native contact');
      assert.equal(ends[0].cancelled,cancelled,'Withdrawal rolls back the provisional stroke');
      assert.equal(ends[0].interrupted,false,'The recording includes the engine stroke end before it stops');
      if(!cancelled)assert.equal(events.at(-1).phase,'Up');
      if(!cancelled){
        assert.equal(await evaluate('layerApp.state().commands.find(c=>c.id==="undo").enabled'),true,'The completed stroke is undoable');
        await send({type:'invoke',command:'undo'});
        await wait('layerApp.app.document_park_ready()');
        assert.equal(await evaluate('layerApp.state().commands.find(c=>c.id==="redo").enabled'),true,'Undo completes before the next recorded contact');
      }
      else assert.deepEqual(await evaluate('layerApp.state().commands.filter(c=>c.id==="undo"||c.id==="redo").map(c=>[c.id,c.enabled])'),history,'Withdrawal preserves artwork history');
    }
    await send({type:'restore_settings',settings});
    const durable=await evaluate('JSON.stringify(layerApp.app.workspace_persistence())');
    await evaluate('window.phoneFrameCapy=document.querySelector("#zen-capy");phoneFrameCapy.focus();null');
    await call('Emulation.setDeviceMetricsOverride',{width:600,height:700,deviceScaleFactor:1,mobile:false});await settle();
    await wait('!layerApp.app.layout(innerWidth,innerHeight).phone&&document.querySelector("#header").getClientRects().length>0&&!document.querySelector("#phone-frame").getClientRects().length');
    assert.equal(await evaluate('document.activeElement===phoneFrameCapy'),true,'Returning to desktop preserves Capy focus');
    assert.equal(await evaluate('JSON.stringify(layerApp.app.workspace_persistence())'),durable,'Leaving phone mode preserves the authored workspace');
    await call('Emulation.setDeviceMetricsOverride',{width:412,height:891,deviceScaleFactor:1,mobile:false});await settle();
    await wait('!!layerApp.app.layout(innerWidth,innerHeight).phone&&document.querySelector("#phone-frame").getClientRects().length>0');
    assert.equal(await evaluate('document.querySelector("#zen-capy")===phoneFrameCapy&&document.activeElement===phoneFrameCapy'),true,'The same focused Capy returns to phone mode');
    assert.equal(await evaluate('JSON.stringify(layerApp.app.workspace_persistence())'),durable,'Entering phone mode preserves the authored workspace');
    await evaluate('delete window.phoneFrameCapy;null');
  }
  await close();
  await send({type:'workspace_manager',command:{type:'switch',id:original.workspace}});
  await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(original.text)});null`);
  await send({type:'restore_saved_settings',saved:JSON.stringify(original.settings)});
  await call('Emulation.clearDeviceMetricsOverride');
  console.log(`PHONE ${mode.toUpperCase()}: ${report.cases.length} captures; ${report.unreachable.length} unreachable states; ${report.requested_dialog_rows?`selected rows ${report.requested_dialog_rows.join(',')}; `:''}${directory}/index.json ${mode==='frame'?'(shared allocation checks passed; cross-host pixels pending)':'(not a parity pass)'}`);
  } finally {if(dialogs)await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:reducedMotion}]});}
}
