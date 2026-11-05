import {createDialogHost} from './dialog-host.js';
import {createPageKit} from './page-kit.js';
import { liveCopy, bindCopy, refreshBindings } from './localization.js';
const profileLibraries=new Set();
export function localizeProfileLibraries(){for(const render of profileLibraries)render();}
export async function chooseExport({app,dialog,element,button,icon,dropdown,numberField,gpuOperation,id}) {
  let view=app.begin_export_dialog(id,await app.load_export_dialog()),closed=false,updating=false,profileRevision=-1,previewRevision=-1,pending=null,running=null,control,completed,failure=null,picker;
  const kit=createPageKit({element,icon}),comparisonCopy=liveCopy(app,'comparison_copy');
  const send=action=>{if(!closed&&!updating){view=app.export_dialog_action(id,action);render();}};
  let render=()=>{};
  const floating=()=>{const v=window.visualViewport;return app.dialog_presentation(view.dialog.role,innerWidth,innerHeight,{x:v?.offsetLeft??0,y:v?.offsetTop??0,width:v?.width??innerWidth,height:v?.height??innerHeight}).kind==='floating';};
  async function drain() {
    while(pending!=null&&!closed){
      const revision=pending;pending=null;control=app.capture_control();
      try {
        const output=await gpuOperation(()=>app.export_image(id,null,control,true));
        if(!closed&&!control.cancelled()&&revision===view.preview_revision){completed=output;failure=null;send({type:'preview',value:{ready:true,transparent:output.transparent,range_exceeded:output.range_exceeded}});}
      }catch(error){if(!closed&&!control.cancelled()&&revision===view.preview_revision){failure=error;render();}}
      finally{control.free();control=null;}
    }
  }
  function schedule() {
    if(previewRevision===view.preview_revision)return;
    previewRevision=view.preview_revision;completed=null;pending=previewRevision;control?.cancel();
    if(!running)running=drain().finally(()=>{running=null;});
  }
  try {return await dialog(()=>view.copy.title,(form,finish,{header,caption,layout,dismiss})=>{
    form.parentNode.id='export-options';
    const panes=new Map(['root','size','color','presets'].map(page=>[page,element('section','document-page')]));
    const group=page=>{const value=kit.group();panes.get(page).append(value.section);return value;};
    const delivery=group('root'),sizeGroup=group('size'),colorGroup=group('color'),presetsGroup=group('presets');
    const numbers=[];
    const combo=(parent,key,title,action,subtitle=true)=>{
      const node=dropdown(()=>view.copy[title]??view.metadata.label,value=>send(action(value)));node.id=`export-${key}`;
      const row=kit.comboRow({title:()=>view.copy[title]??view.metadata.label,control:node,subtitle});parent.list.append(row);return node;
    };
    const toggle=(parent,key,title,action,description)=>{
      const input=element('input','settings-switch');input.type='checkbox';input.setAttribute('role','switch');input.id=`export-${key}`;
      bindCopy(input,()=>view.copy[title]??view.metadata.remove_location,'ariaLabel');input.onchange=()=>send(action(input.checked));
      const row=kit.row({title:()=>view.copy[title]??view.metadata.remove_location,description:description?()=>view.copy[description]:null,suffix:input}).line;parent.list.append(row);return {input,row};
    };
    const numeric=(parent,key,title,spec,action)=>{
      const node=kit.numberRow(numberField(view.numeric[spec],()=>view.copy[title],value=>send(action(value)),false,false,false));node.id=`export-${key}`;parent.list.append(node);numbers.push(node);return node;
    };
    const edit=(type,value)=>({type:'edit',action:{type,value}});
    const output=combo(delivery,'output','output',value=>({type:'output',value}));
    output.addEventListener('pointerdown',()=>send({type:'output_contact'}));output.addEventListener('keydown',()=>send({type:'output_contact'}));
    const format=combo(delivery,'format','format',value=>edit('format',value));
    const flatten=toggle(delivery,'flatten','flatten',enabled=>({type:'flatten',enabled}));
    const quality=numeric(delivery,'jpeg-quality','quality','quality',value=>edit('quality',value));
    const metadata=combo(delivery,'metadata','metadata',value=>({type:'metadata_keep',value}));
    const location=toggle(delivery,'remove-location','remove_location',enabled=>({type:'remove_location',enabled}));
    const metadataNote=element('p','settings-description');panes.get('root').append(metadataNote);
    const comparison=element('div','color-comparison native-comparison'),status=element('p','settings-description');let painted,paintedSdr;
    for(const key of ['width','height','gap'])comparison.style.setProperty(`--comparison-${key}`,`${comparisonCopy[key]}px`);
    panes.get('root').append(comparison,status);
    const rendition=element('div','export-rendition');
    for(const {value:sdr}of view.renditions){const node=button(()=>view.renditions.find(choice=>choice.value===sdr).label,()=>send({type:'rendition',sdr}));node.dataset.rendition=String(sdr);rendition.append(node);}panes.get('root').append(rendition);
    const compression=element('p','settings-description',()=>view.copy.compression_note);panes.get('root').append(compression);
    const links=group('root'),linkLabels=[];
    const navigation=(page,title,subtitle)=>{
      const node=button('',()=>send({type:'navigate',page}),'preference-navigation');node.dataset.exportPage=page.toLowerCase();
      const row=kit.row({title,description:subtitle,suffix:icon('go-next'),line:node});links.list.append(row.line);linkLabels.push(()=>{bindCopy(row.label,title);bindCopy(row.text.lastChild,subtitle);});return row;
    };
    navigation('size',()=>view.copy.size,()=>view.size_subtitle);
    const colorLink=navigation('color',()=>view.color_title,()=>view.color_subtitle);
    navigation('presets',()=>view.copy.preset,()=>view.preset_names[view.preset]);
    const clipping=group('root'),clip=toggle(clipping,'hdr-clip','clip_hdr',enabled=>edit('clip_hdr_range',enabled),'clip_help');
    const size=combo(sizeGroup,'size','size',value=>({type:'fit',enabled:value==='Fit'}));
    const width=numeric(sizeGroup,'width','maximum_width','dimension',value=>({type:'bound',axis:0,value}));
    const height=numeric(sizeGroup,'height','maximum_height','dimension',value=>({type:'bound',axis:1,value}));
    const enlarge=toggle(sizeGroup,'enlarge','enlarge',enabled=>({type:'enlarge',enabled}));
    const resolution=combo(sizeGroup,'resolution','resolution',value=>edit('resolution',view.resolutions[Number(value)].value));
    const ppi=numeric(sizeGroup,'ppi','ppi','ppi',value=>({type:'ppi',value}));
    const sizeNote=element('p','settings-description');panes.get('size').append(sizeNote);
    picker=createProfilePicker({app,element,button,icon,dropdown,label:()=>view.copy.profile,purpose:{kind:'output'},initial:app.export_dialog_profile(id,Number(view.fields.profile)),originals:()=>view.original_profiles,currentLabel:()=>view.original_profiles?.find(choice=>choice.value===view.fields.profile)?.label,originalProfile:index=>app.export_dialog_profile(id,index),
      change:state=>send({type:'profile',result:state.pending?{Err:'ProfileMissing'}:state.error?{Err:state.error.color_feature_error??{Diagnostic:String(state.error)}}:{Ok:picker.profileValue()}})});
    picker.classList.add('export-profile-picker');const profileRow=kit.comboRow({title:()=>view.copy.profile,control:picker});colorGroup.list.append(profileRow);
    const depth=combo(colorGroup,'depth','depth',value=>edit('depth',value));
    const background=combo(colorGroup,'background','background',value=>edit('background',value));
    const print=button('',async()=>{const value=app.export_dialog_print_profile(id);if(!value)return;const [profile,name,intent]=value;send({type:'profile',result:{Err:'ProfileMissing'}});try{const admitted=await app.validate_profile_picker({profile,name},{kind:'output'},null);if(!closed){send({type:'intent',value:intent});send({type:'profile',result:{Ok:admitted}});}}catch(error){send({type:'profile',result:{Err:error.color_feature_error??{Diagnostic:String(error)}}});}},'preference-navigation');
    kit.row({title:()=>view.copy.use_print_profile,description:()=>view.print_profile_name??'',suffix:icon('go-next'),line:print});colorGroup.list.append(print);
    const dither=toggle(colorGroup,'dither','reduce_banding',enabled=>({type:'dither',enabled}),'dither_gradients');
    const jpegNote=element('p','settings-description',()=>view.copy.jpeg_note),precision=element('p','settings-description');panes.get('color').append(jpegNote,precision);
    const advanced=element('details','preference-group preference-expander'),advancedTitle=element('summary','preference-row',()=>view.copy.conversion),advancedGroup=kit.group();advancedTitle.append(icon('chevron-down'));advanced.append(advancedTitle,advancedGroup.section);panes.get('color').append(advanced);
    const intent=combo(advancedGroup,'intent','intent',value=>({type:'intent',value}),false);
    const preset=combo(presetsGroup,'preset','preset',value=>({type:'preset',index:Number(value)}));
    const presetActions=group('presets'),presetStatus=element('p','settings-description');panes.get('presets').append(presetStatus);
    async function storePreset(operation){
      let write,error=null;
      try{write=app.begin_export_preset_write(id,operation);await write.store();}catch(value){error=value;}
      if(!closed){if(write){view=app.finish_export_preset_write(id,write,error);write=null;render();}else{failure=error;send({type:'preset_cancel'});}}
      write?.free();
    }
    async function presetAction(operation){
      if(!commitNumbers())return;send({type:'begin_preset',operation});if(!view.preset_busy)return;
      if(operation!=='save'){await storePreset(operation);return;}
      let input,save;
      const accepted=await dialog(()=>view.copy.save_preset_title,(body,complete,{caption})=>{
        caption.tabIndex=-1;caption.autofocus=!floating();body.parentNode.id='export-preset-name-dialog';const own=kit.group(),field=kit.entryRow({title:()=>view.copy.preset_name,id:'export-preset-name',value:view.name});input=field.input;input.autofocus=floating();own.list.append(field.line);
        input.oninput=()=>{send({type:'name',value:input.value});save.disabled=!view.name_valid;};
        const cancel=button(()=>view.copy.common.cancel,()=>complete(false));save=button(()=>view.copy.common.save,()=>{if(view.name_valid)complete(true);},'suggested-action');save.disabled=!view.name_valid;
        const footer=kit.footer({actions:[cancel,save]});body.append(own.section,footer);body.onsubmit=e=>{e.preventDefault();save.click();};return {cancel,primary:save,footer};
      },view.copy.preset_dialog);
      if(!closed){if(accepted)await storePreset(operation);else send({type:'preset_cancel'});}
    }
    const presetButtons=['save','update','remove','reset'].map((operation,index)=>{const node=button(()=>view.preset_labels[index],()=>presetAction(operation),'preference-button-row');node.id=`export-preset-${operation.toLowerCase()}`;if(operation==='remove')node.classList.add('destructive-action');presetActions.list.append(node);return node;});
    const error=element('p','error-message');
    const cancel=button(()=>view.copy.common.cancel,dismiss),choose=button(()=>view.copy.choose_file,()=>{if(commitNumbers()&&view.can_export)finish(app.export_dialog_delivery(id));},'suggested-action');
    choose.id='export-confirm';cancel.id='export-cancel';const close=button('',()=>{control?.cancel();finish(null);},'export-window-close');close.append(icon('window-close'));bindCopy(close,()=>view.copy.common.close,'ariaLabel');close.hidden=true;header.append(cancel,choose,close);form.append(...panes.values(),error);
    function commitNumbers(){return numbers.filter(node=>!node.hidden).every(node=>node.commit());}
    function choices(node,values,value){node.update({choices:values.map(choice=>({value:choice.value,label:choice.label})),value});}
    function show(node,visible){node.hidden=!visible;}
    render=()=>{
      if(closed)return;schedule();updating=true;
      const fields=view.fields,copy=view.copy;
      choices(output,view.outputs,view.output);show(output,view.show_output);choices(format,view.formats,fields.format);show(format,view.show_conversion);
      choices(depth,view.depths,fields.depth);show(depth,view.show_depth);depth.update({disabled:!view.depth_enabled});choices(background,view.backgrounds,fields.background);show(background,view.show_background);
      choices(intent,view.intents,fields.encoding.conversion.intent);
      size.update({choices:[{value:'Original',label:copy.original_size},{value:'Fit',label:copy.fit_bounds}],value:view.show_size?'Fit':'Original'});
      resolution.update({choices:view.resolutions.map((choice,value)=>({value,label:choice.label})),value:view.resolutions.findIndex(choice=>JSON.stringify(choice.value)===JSON.stringify(fields.resolution))});
      for(const [node,value,visible]of [[width,view.bounds[0],view.show_size],[height,view.bounds[1],view.show_size],[ppi,view.ppi,view.show_ppi],[quality,fields.jpeg_quality,view.show_quality]]){node.update(value);show(node,visible);}
      for(const [value,checked,visible]of [[flatten,view.flatten,view.show_flatten],[enlarge,view.enlarge,view.show_size],[clip,view.clip,view.show_clip],[dither,view.dither,view.show_dither],[location,fields.metadata.remove_location,view.show_location]]){value.input.checked=checked;show(value.row,visible);}
      metadata.update({choices:view.metadata.choices,value:fields.metadata.keep});show(metadata,view.show_metadata);metadataNote.textContent=view.metadata.note??'';show(metadataNote,view.show_metadata_note);
      show(clipping.section,view.show_clip);show(colorLink.line,view.show_color);show(profileRow,view.show_conversion);show(advanced,view.show_conversion);show(print,view.show_print_profile);print.disabled=!view.profile_ready;
      if(profileRevision!==view.profile_revision){profileRevision=view.profile_revision;picker.updateProfile(app.export_dialog_profile(id,Number(fields.profile)));}
      show(jpegNote,view.jpeg_note);precision.textContent=view.precision_note??'';show(precision,!!view.precision_note);sizeNote.textContent=view.size_summary;
      preset.update({choices:view.preset_names.map((label,value)=>({value,label,enabled:view.preset_enabled[value]})),value:view.preset});
      for(const [index,enabled]of [view.preset_save,view.preset_update,view.preset_remove,view.preset_reset].entries()){bindCopy(presetButtons[index],()=>view.preset_labels[index]);show(presetButtons[index],enabled);presetButtons[index].disabled=!enabled||view.preset_busy;}
      for(const update of linkLabels)update();
      presetStatus.textContent=view.preset_status??'';show(presetStatus,!!view.preset_status);presetStatus.classList.toggle('error-message',view.preset_error);
      if(painted!==completed||paintedSdr!==view.preview_sdr){painted=completed;paintedSdr=view.preview_sdr;comparison.replaceChildren();if(completed)for(const [index,image]of [completed.previews[0],view.preview_sdr&&completed.sdr_preview?completed.sdr_preview:completed.previews[1]].entries()){
        const figure=element('figure'),canvas=element('canvas'),caption=element('figcaption','',view.comparison_labels[index]);[canvas.width,canvas.height]=image.extent;
        canvas.getContext('2d',{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);canvas.setAttribute('aria-label',view.comparison_labels[index]);figure.append(caption,canvas);comparison.append(figure);
      }}
      for(const [index,figure]of [...comparison.children].entries()){figure.firstChild.textContent=view.comparison_labels[index];figure.lastChild.setAttribute('aria-label',view.comparison_labels[index]);}
      show(rendition,view.show_rendition);for(const node of rendition.children)node.setAttribute('aria-pressed',String(node.dataset.rendition===String(view.preview_sdr)));
      show(compression,view.compression_note);status.textContent=completed?.status??copy.preparing_comparison;show(status,!!status.textContent);
      error.textContent=failure?app.color_feature_error_copy(failure):view.error??'';show(error,!!error.textContent);choose.disabled=!view.can_export;
      const page=view.page==='save_preset'?'presets':view.page;for(const [key,node]of panes)show(node,key===page);
      caption.textContent=({size:copy.image_size,color:copy.color_transparency,presets:copy.presets}[page]??copy.title);layout();updating=false;
    };
    form.localize=()=>{view=app.export_dialog_view(id);refreshBindings();if(completed)completed.status=app.export_dialog_preview_copy(id,completed.clipped_channels>0);render();picker.refreshProfileCaption();};form.onsubmit=e=>e.preventDefault();render();
    return {cancel,primary:()=>{const subpage=floating()&&view.page!=='root';show(close,subpage);if(subpage)close.dataset.dialogPart='end';else delete close.dataset.dialogPart;return subpage?null:choose;},headerStyle:'flat',subpage:()=>view.page!=='root',onBack:()=>send({type:'back'}),onCancel:()=>{control?.cancel();finish(null);}};
  },view.dialog);
  }finally{closed=true;pending=null;control?.cancel();await running;picker?.disposeProfilePicker();}
}

export function importProfile(app,element) {
  const copy=liveCopy(app,"profile_copy");
  return new Promise((resolve,reject)=>{
    const input=element("input");input.type="file";input.accept=".icc,.icm";input.hidden=true;document.body.append(input);
    input.oncancel=()=>{input.remove();resolve(null);};
    input.onchange=async()=>{try{const file=input.files[0];if(!file){resolve(null);return;}if(file.size>copy.read_bytes)throw {color_feature_error:"ProfileReadLimit"};resolve(await app.profile_library("import",undefined,new Uint8Array(await file.arrayBuffer())));}catch(e){reject(e);}finally{input.remove();}};
    input.click();
  });
}

export function createProfilePicker({app,element,button,icon,dropdown,label,purpose,initial,documentProfile:initialDocument,documentLabel,emptyLabel,context,originals=()=>[],originalProfile,currentLabel,change,onError}) {
  const copy=liveCopy(app,'profile_copy'),standard=app.profile_picker_standard(purpose);
  let saved=[],current=initial??null,documentProfile=initialDocument??null,inventory=null,menuFailure=null,disposed=false,busy=false,failure=null,selection=0,revision=0;
  const node=dropdown(label,async value=>{
    if(busy||disposed)return;
    const ticket=++selection;busy=true;failure=null;sync();change?.({pending:true,error:null});
    try {
      let profile=value==='document'?documentProfile:value==='current'?current:value.startsWith('original:')?originalProfile(Number(value.slice(9))):value.startsWith('standard:')?standard[Number(value.slice(9))]:value==='add'?await importProfile(app,element):value==='manage'?await chooseProfileLibrary({app,element,button,icon,dropdown,manage:true}):await app.profile_library('get',value.slice(6));
      if(disposed||ticket!==selection)return;
      if(profile)profile=await app.validate_profile_picker(profile,purpose,context??null);
      if(disposed||ticket!==selection)return;
      if(profile||value==='document'||value==='current')current=profile;
      await refresh();
    }catch(error){if(ticket===selection){failure=error;if(!disposed)onError?.(error);}}
    finally{busy=false;if(!disposed){sync();if(ticket===selection)change?.({pending:false,error:failure});}}
  });
  const name=profile=>profile===current&&currentLabel?.()?currentLabel():profile?.entry_copy_source?app.profile_entry_copy(profile.entry_copy_source).name:profile?app.profile_name_copy(profile.name??standard.find(value=>value.profile.Builtin===profile.profile?.Builtin)?.name??''):documentLabel?.();
  function sync() {
    const sections=[];
    if(!inventory)sections.push([{label:()=>copy.loading,enabled:false}]);
    if(originals()?.length)sections.push([{is_heading:true,label:()=>copy.document},...originals().map(choice=>({value:`original:${choice.value}`,label:()=>choice.label}))]);
    if(inventory?.document)sections.push([{is_heading:true,label:()=>copy.document},{value:'document',label:()=>name(documentProfile)}]);
    if(inventory?.current)sections.push([{is_heading:true,label:()=>copy.current},{value:'current',label:()=>name(current)}]);
    if(inventory?.saved.length)sections.push([{is_heading:true,label:()=>copy.saved},...inventory.saved.map(index=>saved[Number(index)]).map(entry=>({value:`saved:${entry.id}`,label:()=>app.profile_entry_copy(entry.entry_copy_source).name}))]);
    if(menuFailure)sections.push([{label:()=>app.color_feature_error_copy(menuFailure,true),enabled:false}]);
    if(inventory?.standard)sections.push([{is_heading:true,label:()=>copy.standard},...standard.map((profile,index)=>({value:`standard:${index}`,label:()=>name(profile)}))]);
    sections.push([{value:'add',label:()=>copy.add_profile_dialog},{value:'manage',label:()=>copy.manage}]);
    const index=current?.profile?.Builtin==null?-1:standard.findIndex(profile=>profile.profile.Builtin===current.profile.Builtin);
    node.update({value:inventory?.document&&current===documentProfile?'document':inventory?.current?'current':index>=0?`standard:${index}`:'',text:()=>busy?copy.reading:name(current)??emptyLabel?.()??copy.choose,disabled:busy,
      menu:()=>({sections:sections.map(section=>section.map(choice=>({...choice,label:choice.label(),command:choice.value,enabled:choice.enabled!==false})))})});
  }
  async function refresh(){
    const ticket=++revision,profile=current,document=documentProfile;let entries=[],error=null;
    try{entries=await app.profile_library('list');}catch(reason){error=reason;}
    if(disposed||ticket!==revision)return;
    const view=await app.profile_picker_inventory(profile,document,purpose,context??null,entries.map(entry=>({id:entry.id,visible:entry.visible!==false,channels:entry.channels??null,valid:!entry.issue})));
    if(!disposed&&ticket===revision){saved=entries;inventory=view;menuFailure=error;sync();}
  }
  node.updateProfile=(profile,document=false)=>{selection++;current=profile;documentProfile=document?profile:null;failure=null;sync();node.refreshProfiles();};
  node.profileValue=()=>busy||failure?null:current;
  node.refreshProfileCaption=sync;
  node.refreshProfiles=()=>refresh().catch(error=>{if(!disposed)onError?.(error);});
  node.disposeProfilePicker=()=>{disposed=true;};
  sync();node.refreshProfiles();return node;
}

export async function chooseSourceProfile({app,dialog,element,button,icon,dropdown,interpretation}) {
  const kit=createPageKit({element,icon}),copy=liveCopy(app,'profile_copy');let picker;
  try{return await dialog(()=>copy.interpret_title,(form,finish)=>{
    form.append(element('p','alert-description',()=>copy.interpret_help));
    let failure=null;const error=element('p','preferences-error'),fail=value=>{failure=value;error.hidden=value==null;bindCopy(error,()=>failure==null?'':app.color_feature_error_copy(failure,true));};error.hidden=true;
    picker=createProfilePicker({app,element,button,icon,dropdown,label:()=>copy.interpret_as,purpose:{kind:'source',channels:interpretation.channels},context:{kind:'interpretation',source:interpretation},initial:{profile:interpretation.profile,channels:null},change:state=>{use.disabled=state.pending||!!state.error;fail(state.error);},onError:fail});
    const group=kit.group();group.list.append(kit.comboRow({title:()=>copy.interpret_as,control:picker}));form.append(group.section,error);
    const cancel=button(()=>copy.common.cancel,()=>finish(null)),use=button(()=>copy.use_profile,()=>{const profile=picker.profileValue();if(profile)finish(profile.profile);},'suggested-action'),footer=kit.footer({actions:[cancel,use]});form.append(footer);return {cancel,primary:use,footer};
  },copy.dialog);}finally{picker?.disposeProfilePicker();}
}

export function chooseProfileLibrary({app,element,button,icon,dropdown,manage=false}) {
  const kit=createPageKit({element,icon});
  const copy=liveCopy(app,"profile_copy");
  return new Promise(resolve=>{
    const root=element("dialog","document-dialog profile-library"),form=element("form"),group=kit.group(),list=group.list,error=element("p","error-message"),search=kit.entry({label:()=>copy.search});
    search.type="search";search.id="profile-library-search";bindCopy(search,()=>copy.search,"placeholder");bindCopy(search,()=>copy.search,"ariaLabel");
    error.hidden=true;
    root.style.setProperty("--profile-library-width",`${copy.library_size[0]}px`);root.style.setProperty("--profile-library-height",`${copy.library_size[1]}px`);
    form.method="dialog";let result=null,closed=false,failure=null,busy=false,entries=[];
    const fail=value=>{failure=value;error.hidden=value==null;bindCopy(error,()=>failure==null?"":app.color_feature_error_copy(failure,true));};
    const finish=value=>{if(busy&&value==null)return;result=value;shell.hide();};
    root.addEventListener("close",()=>{closed=true;shell.dispose();body.dispose();profileLibraries.delete(render);root.remove();resolve(result);},{once:true});
    const header=element("header","dialog-header");header.append(element("h2","",()=>copy.manage_title));
    const searchRow=kit.search(search),body=kit.body(group.section);form.append(element("p","settings-description",()=>copy.menu_help),searchRow,body.root,error);
    const done=button(()=>copy.common.done,()=>finish(null));
    const render=()=>{
      searchRow.hidden=entries.length<=7&&!search.value;search.disabled=busy;
      list.replaceChildren();
      for(const entry of entries.filter(entry=>app.profile_library_matches(entry.entry_copy_source,search.value))){
        const actions=element("div","preference-row-actions");
        let tag,copyView;const entryCopy=()=>{const current=app.language_tag();if(tag!==current){copyView=app.profile_entry_copy(entry.entry_copy_source);tag=current;}return copyView;};
        const row=kit.row({title:()=>entryCopy().name,description:()=>entryCopy().issue??entryCopy().details,suffix:actions});
        if(!manage&&!entry.issue)actions.append(button(()=>copy.use_profile,()=>run(async()=>{const profile=await app.profile_library('get',entry.id);if(!closed)finish(profile);})));
        if(entry.visible!==false){const pin=icon('pin');pin.classList.add('profile-library-pin');bindCopy(pin,()=>copy.shown,'ariaLabel');actions.append(pin);}
        const more=dropdown(()=>app.profile_options_copy(entryCopy().name),value=>run(async()=>{await app.profile_library(value==='remove'?'remove':entry.visible===false?'show':'hide',entry.id);await refresh();}));
        more.classList.add('profile-library-more');more.replaceChildren(icon('more'));
        more.update({value:entry.visible!==false?'show':'',menu:()=>({sections:[[{label:copy.show,command:'show',selected:entry.visible!==false,enabled:!busy}],[{label:copy.remove,command:'remove',enabled:!busy}]]})});
        more.addEventListener('click',event=>event.stopPropagation());actions.append(more);list.append(row.line);
      }
      for(const node of form.querySelectorAll('button'))node.disabled=busy;
    };
    const refresh=async()=>{entries=await app.profile_library("list");if(!closed)render();};
    const run=async action=>{
      if(busy||closed)return;busy=true;render();fail(null);
      try{await action();}catch(e){if(!closed)fail(e);}finally{busy=false;if(!closed)render();}
    };
    search.addEventListener("input",render);profileLibraries.add(render);
    const add=button('',()=>run(async()=>{const profile=await importProfile(app,element);if(profile&&!manage&&!closed)finish(profile);else await refresh();}));
    add.append(icon('plus'));add.className='profile-library-import';bindCopy(add,()=>copy.add_profile,'ariaLabel');bindCopy(add,()=>copy.add_profile,'title');
    const slot=element('div','dialog-header-slot');slot.dataset.dialogSlot='end';slot.append(add);header.append(slot);form.append(kit.footer({actions:[done]}));
    form.onsubmit=e=>e.preventDefault();root.append(header,form);document.body.append(root);
    const shell=createDialogHost({app,dialog:root,header,cancel:done,primary:done,retainPrimary:true,endSlot:slot,icon,copy:copy.common,onCancel:()=>finish(null),search,clearSearch:()=>{search.value="";render();}});shell.present(copy.library_dialog);run(refresh);
  });
}
