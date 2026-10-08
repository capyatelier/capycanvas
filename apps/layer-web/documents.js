import { liveCopy, bindCopy } from './localization.js';
import {createDrawingTabs} from './drawing-tabs.js';
import {createDocumentRecovery} from './document-recovery.js';
import {chooseDocumentColor} from './document-color.js';
import {createProof} from './proof.js';
import {chooseExport,chooseSourceProfile} from './export-controls.js';

export const exportFormats={
  Exr:["exr","image/x-exr"],
  PngHdr:["png","image/png"],PngHdrMapped:["png","image/png"],
  JpegHdr:["jpg","image/jpeg"],JpegHdrMapped:["jpg","image/jpeg"],
  AvifHdr:["avif","image/avif"],AvifHdrMapped:["avif","image/avif"],
  Png:["png","image/png"],Tiff:["tif","image/tiff"],Jpeg:["jpg","image/jpeg"],
  Webp:["webp","image/webp"],
};
import {createImageImport} from './image-import.js';
// Browser file transport; document checkpoints, stale-edit guards and unsaved
// decisions stay in UiSession. File handles never enter a project or localStorage.
export function createDocuments({app,bootstrap,delivery,state,canvas,dispatch,applyChange,wake,element,button,icon,numberField,message,gpuOperation,rasterWorker,resumeCanvas,contentChanged}) {
  const common=bootstrap.common,exportCopy=liveCopy(app,"export_copy");
  const formatLabels=()=>({Exr:exportCopy.format_exr,PngHdr:exportCopy.format_pq,PngHdrMapped:exportCopy.format_pq_clipped,
    JpegHdr:exportCopy.format_jpeg_hdr,JpegHdrMapped:exportCopy.format_jpeg_hdr,AvifHdr:exportCopy.format_avif_hdr,AvifHdrMapped:exportCopy.format_avif_hdr,
    Png:exportCopy.format_png,Tiff:exportCopy.format_tiff,Jpeg:exportCopy.format_jpeg,Webp:exportCopy.format_webp});
  const deliveryMessage=(type,values)=>app.document_delivery_message({type,...values});
  const deliveryFailure=(type,values)=>({document_host_error:{type:"delivery",reason:{type,...values}}});
  const transportFailure=reason=>({document_host_error:{type:"transport",reason}});
  const active=new Set(),handles=new Map();
  let closing=false,changing=false,batching=false;
  let nativeKey=null,nativePending=null,keyboardPaste=false;
  const nativeDeliveries=new Map();
  const editing=target=>!!target?.closest?.('input,select,textarea,[contenteditable]:not([contenteditable=false]),dialog[open]');
  function finishNativePaste(files=[]){
    const pending=nativePending;nativePending=null;
    if(pending){clearTimeout(pending.timer);pending.resolve(files);}
  }
  const cancelNativePaste=()=>finishNativePaste();
  function nativeDelivery(id){
    if(nativeDeliveries.has(id))return nativeDeliveries.get(id);
    const promise=new Promise(resolve=>{nativePending={id,resolve,timer:setTimeout(cancelNativePaste,1000)};});
    nativeDeliveries.set(id,promise);return promise;
  }
  function allowNativePaste(){
    if(!nativeKey)return false;
    const request=app.state().requests.find(request=>!nativeKey.has(request.id)&&request.kind.type==='document'&&request.kind.request.type==='paste');
    if(!request)return false;
    nativeDelivery(request.id);return true;
  }
  window.addEventListener('blur',cancelNativePaste);
  window.addEventListener('pagehide',cancelNativePaste);
  document.addEventListener('paste',event=>{
    if(event.defaultPrevented||editing(event.target)){finishNativePaste();return;}
    const files=[...event.clipboardData?.files??[]];
    if(nativePending){
      const current=app.state().requests.some(request=>request.id===nativePending.id);
      event.preventDefault();finishNativePaste(current?files:[]);return;
    }
    if(keyboardPaste||active.size||changing||closing||batching||!files.length)return;
    const before=new Set(app.state().requests.map(request=>request.id));
    const change=app.native_paste_input();
    const request=app.state().requests.find(request=>!before.has(request.id)&&request.kind.type==='document'&&request.kind.request.type==='paste');
    if(request){nativeDeliveries.set(request.id,Promise.resolve(files));event.preventDefault();applyChange(change);}
  });
  const images=createImageImport({app,canvas,dispatch,applyChange,wake,element,button,icon,message,gpuOperation,
    openClipboard,interpret:()=>chooseSourceProfile({app,dialog,element,button})});
  const pruneHandles=()=>{const live=new Set(app.document_tabs(0).tabs.flatMap(t=>[t.uri,t.export_uri]));for(const key of handles.keys())if(!live.has(key))handles.delete(key);};
  const location=(name,handle)=>{const uri=`browser:${crypto.randomUUID()}`;if(handle)handles.set(uri,handle);return{uri,name};};
  const openDialogs=new Set();
  const dialog=(title,build)=>new Promise(resolve=>{
    const root=element("dialog","document-dialog"),form=element("form");
    form.method="dialog";form.append(element("h2","",title));openDialogs.add(form);root.append(form);document.body.append(root);
    let result=null;root.addEventListener("close",()=>{openDialogs.delete(form);root.remove();resolve(result);},{once:true});
    const finish=value=>{result=value;root.close();};
    build(form,finish);root.showModal();
  });
  const cancel=(footer,finish)=>footer.append(button(()=>common.cancel,()=>finish(null)));
  const proof=createProof({app,dialog,element,button,icon,applyChange,wake,dispatch,contentChanged});
  async function newDocument() {
    const spec=app.editor_models(innerWidth,innerHeight).document_options;
    let model=spec.creation;
    return dialog(()=>model.text.new_title,(form,finish)=>{
      const field=(id,title,node)=>{const label=element("label","document-size",title);node.dataset.documentField=id;if(typeof title==="function")bindCopy(node,title,"ariaLabel");else node.setAttribute("aria-label",title);label.append(node);form.append(label);return node;};
      const select=(id,title,choices,value)=>{const node=element("select");for(const [key,label] of choices){const option=element("option","",label);option.value=key;node.append(option);}node.value=value;return field(id,title,node);};
      const presetKey=id=>id?JSON.stringify(id):"custom";
      const preset=select("preset",()=>model.text.preset,[["custom",model.text.custom],...model.presets.map(p=>[presetKey(p.id),p.name])],presetKey(model.selected));
      const selectedPreset=()=>model.presets.find(p=>presetKey(p.id)===preset.value);
      const remove=button(()=>model.text.remove_preset,()=>{const action=selectedPreset()?.remove;if(!action)return;
        applyChange(app.dispatch({type:"new_document_preferences",action}));
        model=app.editor_models(innerWidth,innerHeight).document_options.creation;
        preset.replaceChildren(...[["custom",model.text.custom],...model.presets.map(p=>[presetKey(p.id),p.name])].map(([key,label])=>{const option=element("option","",label);option.value=key;return option;}));
        preset.value="custom";remove.disabled=true;
      });remove.dataset.documentAction="remove-preset";remove.disabled=!selectedPreset()?.remove;form.append(remove);
      const dimensions=[...model.options.extent];
      const fields=dimensions.map((value,i)=>{
        const number=numberField(spec.numeric,()=>i?model.text.height:model.text.width,next=>{dimensions[i]=next;edited();describe();});
        number.classList.add("document-size");number.entry.dataset.documentField=i?"height":"width";number.entry.required=true;
        number.update(value);form.append(number);return number;
      });
      const space=select("space",()=>model.text.space,model.spaces,model.options.color.space);
      const depth=select("depth",()=>model.text.depth,model.depths,model.options.color.depth);
      const blending=select("blending",()=>model.blending.label,model.blending.choices.map(c=>[c.id,c.label]),model.options.blend_space);
      const blendingNote=element("p","document-note");form.append(blendingNote);
      const summary=element("p","document-color-summary"),note=element("p","document-color-note");form.append(summary,note);
      const background=select("background",()=>model.text.background,model.backgrounds,model.options.background);
      let chosen=model.options.blend_space;
      const read=()=>({extent:[...dimensions],color:{space:space.value,depth:depth.value},background:background.value,blend_space:chosen});
      const describe=()=>{const appearance=app.new_document_appearance(read());
        blending.disabled=!appearance.blending_editable;blending.value=appearance.blending;
        blendingNote.textContent=appearance.blending_help;summary.textContent=appearance.summary;
        note.textContent=appearance.note??"";note.hidden=appearance.note==null;
      };
      const edited=()=>{preset.value="custom";remove.disabled=true;};
      for(const number of fields)number.entry.addEventListener("input",edited);
      for(const input of [space,depth,background])input.addEventListener("change",()=>{edited();describe();});
      blending.onchange=()=>{chosen=blending.value;edited();describe();};
      preset.onchange=()=>{const p=selectedPreset();remove.disabled=!p?.remove;if(!p)return;
        fields.forEach((number,i)=>{dimensions[i]=p.options.extent[i];number.cancelEditing();number.update(dimensions[i]);});space.value=p.options.color.space;depth.value=p.options.color.depth;background.value=p.options.background;chosen=p.options.blend_space;describe();
      };
      describe();
      const name=field("preset-name",()=>model.text.preset_name,element("input"));name.maxLength=64;
      const remember=element("input");remember.type="checkbox";field("remember",()=>model.text.remember,remember);
      form.localize=()=>{
        model=app.editor_models(innerWidth,innerHeight).document_options.creation;
        for(const [node,choices] of [[preset,[["custom",model.text.custom],...model.presets.map(p=>[presetKey(p.id),p.name])]],[space,model.spaces],[depth,model.depths],[background,model.backgrounds],[blending,model.blending.choices.map(c=>[c.id,c.label])]]) {
          for(const [key,label] of choices){const option=[...node.options].find(o=>o.value===key);if(option)option.textContent=label;}
        }
        describe();
      };
      const error=element("p","error-message");form.append(error);
      const footer=element("footer"),cancelNew=button(()=>model.text.cancel,()=>finish(null));cancelNew.dataset.documentAction="cancel";footer.append(cancelNew);
      const create=button(()=>model.text.create,()=>{if(!fields.every(number=>number.commit())||!form.reportValidity())return;const options=read();try{
        applyChange(app.dispatch({type:"new_document_preferences",action:{type:"remember",options,name:name.value,defaults:remember.checked}}));
        finish(options);
      }catch(e){error.textContent=String(e);}},"suggested-action");create.dataset.documentAction="create";
      footer.append(create);form.append(footer);form.onsubmit=e=>{e.preventDefault();create.click();};
    });
  }
  const clipMime="web application/x-capycanvas-clip",customClip=!!globalThis.ClipboardItem?.supports?.(clipMime);
  const clipboardRead=()=>{
    if(!navigator.clipboard?.read)throw deliveryFailure("clipboard_unavailable");
    return navigator.clipboard.read();
  };
  // Called synchronously from the key or click task: browsers accept a
  // clipboard write only there, so its ClipboardItem waits on the capture.
  async function copyClip(id) {
    const task=app.capture_clip(id),nonce=crypto.randomUUID();
    let deliver,fail;
    const png=new Promise((resolve,reject)=>{deliver=resolve;fail=reject;});
    png.catch(()=>{});
    const items={"image/png":png};
    if(customClip)items[clipMime]=png.then(()=>new Blob([nonce],{type:clipMime.slice(4)}));
    let written=null;
    try{written=navigator.clipboard?.write&&globalThis.ClipboardItem?navigator.clipboard.write([new ClipboardItem(items)]):null;}
    catch(error){written=Promise.reject(error);}
    const writeError=written?written.then(()=>null,error=>error):Promise.resolve(new Error("this browser has no clipboard writer"));
    let control,progress,clip,cancelling=false;
    const retire=()=>control?.cancel();
    window.addEventListener("pagehide",retire);
    const caption=()=>cancelling?delivery.cancelling:app.document_request_title(id)??"";
    try {
      control=app.capture_control();
      if(task.large()){
        progress=element("aside","file-progress");progress.setAttribute("role","status");
        const label=element("span","",caption);progress.append(label,button(()=>common.cancel,()=>{control.cancel();cancelling=true;bindCopy(label,caption);}));
        document.body.append(progress);
      }
      clip=await gpuOperation(()=>task.run(control,nonce));
      if(control.cancelled())throw new DOMException("Copy cancelled","AbortError");
      deliver(new Blob([clip.png()],{type:"image/png"}));
      const failure=await writeError;
      if(control.cancelled())throw new DOMException("Copy cancelled","AbortError");
      if(failure)throw deliveryFailure("clipboard_unavailable");
      app.adopt_clip(clip);clip=null;
      applyChange(app.finish_document(id,true));
    } catch(error) {
      fail(error);
      if(control?.cancelled())throw new DOMException("Copy cancelled","AbortError");
      throw error;
    } finally {window.removeEventListener("pagehide",retire);clip?.free();progress?.remove();control?.free();}
  }
  // This window's copy when the system clipboard still holds it.
  async function ownedClipboard(){
    const nonce=app.clip_nonce();
    if(!nonce)return {own:false};
    if(!customClip)return {own:false};
    const items=await clipboardRead();
    for(const item of items)if(item.types.includes(clipMime)&&await(await item.getType(clipMime)).text()===nonce)return {own:true};
    return {own:false,items};
  }
  async function clipboardImage(read) {
    const items=read??await clipboardRead();
    const formats=app.photo_formats(),preferred=formats.flatMap(f=>f.mime_types.flatMap(m=>[`web ${m}`,m]));
    const files=[];
    for(const item of items) {
      const type=preferred.find(type=>item.types.includes(type));if(!type)continue;
      const blob=await item.getType(type),mime=type.replace(/^web /,"");
      if(blob.size>512*1024*1024)throw deliveryFailure("clipboard_too_large");
      const extension=formats.find(f=>f.mime_types.includes(mime)).extensions[0];
      files.push(new File([blob],deliveryMessage("pasted_image",{extension}),{type:mime}));
    }
    if(!items.length)throw deliveryFailure("clipboard_empty");
    if(!files.length)throw deliveryFailure("clipboard_formats",{formats:formats.map(f=>f.name).join(", ")});
    return files;
  }
  function chooseFile(placing=false,filter=null) {
    const formats=app.photo_formats(),accept=filter?{'application/octet-stream':['.'+filter[1]]}:Object.fromEntries(formats.flatMap(f=>f.mime_types.map(m=>[m,f.extensions.map(e=>'.'+e)])));
    if(!placing&&!filter)accept['application/octet-stream']=['.capy'];
    if(window.showOpenFilePicker) return window.showOpenFilePicker({multiple:!filter,...(filter?{id:filter[1],startIn:"downloads"}:{}),types:[{description:filter?.[0]??(placing?delivery.images:delivery.drawing_or_photo),accept}]})
      .then(async handles=>Promise.all(handles.map(async handle=>({file:await handle.getFile(),handle}))));
    return new Promise(resolve=>{
      const input=element("input");input.type="file";input.multiple=!filter;input.accept=Object.values(accept).flat().join(',');input.hidden=true;document.body.append(input);
      const done=value=>{input.remove();resolve(value);};
      input.onchange=()=>done([...input.files].map(file=>({file})));input.oncancel=()=>done(null);input.click();
    });
  }
  async function download(bytes,name,mime) {
    const url=URL.createObjectURL(new Blob([bytes],{type:mime}));
    try {
      return await dialog(()=>delivery.download_file,(form,finish)=>{
        form.append(element("p","",()=>deliveryMessage("download_confirm",{name})));
        const footer=element("footer");cancel(footer,finish);
        const done=button(()=>delivery.file_saved,()=>finish(true),"suggested-action");done.disabled=true;
        const start=button(()=>delivery.download,()=>{const a=element("a");a.href=url;a.download=name;a.click();done.disabled=false;});
        footer.append(start,done);form.append(footer);
      });
    } finally {URL.revokeObjectURL(url);}
  }
  async function destination(request,recipe) {
    const old=request.location && handles.get(request.location.uri);
    if(old&&request.type==='save'&&(!old.queryPermission||await old.queryPermission({mode:'readwrite'})==='granted')) {
      let observed=null;
      try{if(old.getFile)observed=await rasterWorker({operation:'fingerprint',file:await old.getFile(),buffers:[]});}catch{}
      if(app.session_destination_matches(observed))return{location:request.location,handle:old,reused:true};
      message(()=>delivery.destination_changed);
    } else if(old&&request.type!=='save') {
      if(!request.repeat)return{location:request.location,handle:old};
      try {
        let permission=await old.queryPermission?.({mode:'readwrite'})??'granted';
        if(permission==='prompt')permission=await old.requestPermission({mode:'readwrite'});
        if(permission==='granted'){await old.getFile();return{location:request.location,handle:old};}
      } catch(error) {if(error?.name==='AbortError')throw error;}

    }
    if(window.showSaveFilePicker) {
      const [extension,mime]=recipe?exportFormats[recipe.format]:["capy","application/vnd.capycanvas"];
      const description=recipe?formatLabels()[recipe.format]:delivery.drawing_type;
      const name=recipe?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name;
      const handle=await window.showSaveFilePicker({suggestedName:name,types:[{description,accept:{[mime]:["."+extension]}}]});
      return{location:location(handle.name,handle),handle};
    }
    const extension=recipe?exportFormats[recipe.format][0]:null;
    return {location:location(extension?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name)};
  }
  async function handle(request) {
    if(active.has(request.id))return;active.add(request.id);
    let candidate;
    try {
      if(request.kind.type==='drawings'){dispatch({type:'complete_request',id:request.id});tabs.showSelector();return;}
      if(["soft_proof_setup","sdr_rendition"].includes(request.kind.type)){await proof.run(request.id,request.kind.type==="sdr_rendition");if(app.state().requests.some(r=>r.id===request.id))dispatch({type:"complete_request",id:request.id});return;}
      if(request.kind.type!=="document")throw new Error(`Unsupported host request: ${request.kind.type}`);
      const r=request.kind.request,id=request.id;
      if(r.type==="confirm_close") {
        const current=()=>app.state().requests.find(request=>request.id===id)?.kind.request;
        const spec=()=>app.editor_models(innerWidth,innerHeight).document_options;
        const decision=await dialog(()=>current()?.title??r.title,(form,finish)=>{
          form.append(element("p","",()=>spec().unsaved_description));const footer=element("footer");cancel(footer,finish);
          footer.append(button(()=>spec().discard_label,()=>finish("discard")),button(()=>common.save,()=>finish("save"),"suggested-action"));form.append(footer);
        });
        applyChange(app.respond_document(id,decision??"cancel"));return;
      }
      if(r.type==="properties") {
        const info=await app.document_properties();
        if(!app.state().requests.some(request=>request.id===id))return;
        let view=app.document_properties_copy(info);
        await dialog(()=>view.title,(form,finish)=>{
          for(const section of ['rows','sources'])for(const [index]of view[section].entries())form.append(element("h3","",()=>view[section][index][0]),element("p","source-details",()=>view[section][index][1]));
          form.localize=()=>{view=app.document_properties_copy(info);};
          form.append(button(()=>view.done,()=>finish(true)));
        });
        applyChange(app.finish_document(id,true));
      } else if(["change_color","color_history","repair_source_profile","rasterize_source"].includes(r.type)) {
        candidate=await chooseDocumentColor({app,dialog,element,button,gpuOperation,request:r,id});
        if(candidate?.is_copy?.()) {
          const master=app.state().document_file.location;
          const target=await destination({name:deliveryMessage("converted_name",{name:(master?.name??app.document_tabs(0).tabs.find(t=>String(t.id)===String(app.document_tabs(0).selected)).title).replace(/\.[^.]+$/,"")})});
          if(!target.location.name.toLowerCase().endsWith(".capy"))throw deliveryFailure("converted_filename_invalid");
          const original=handles.get(master?.uri);
          if(original&&target.handle&&await original.isSameEntry?.(target.handle))throw deliveryFailure("choose_different");
          const progress=element("aside","file-progress");progress.setAttribute("role","status");
          let cancelled=false,phase="preparing_converted_copy";const caption=()=>delivery[phase],label=element("span","",caption);
          progress.append(label,button(()=>common.cancel,()=>{cancelled=true;phase="cancelling";bindCopy(label,caption);candidate.cancel();}));document.body.append(progress);
          let output;
          try {
            output=await app.save_color_copy(candidate);const bytes=output.blob;
            if(cancelled)throw new DOMException("Converted copy cancelled","AbortError");
            phase="writing_converted_copy";bindCopy(label,caption);progress.querySelector("button").disabled=true;
            let success;
            if(target.handle){const stream=await target.handle.createWritable();try{await stream.write(bytes);await stream.close();success=true;}catch(error){try{await stream.abort();}catch{}throw error;}}
            else success=!!await download(bytes,target.location.name,"application/octet-stream");
            applyChange(app.finish_document(id,success));
          } catch(error){if(cancelled)throw new DOMException("Converted copy cancelled","AbortError");throw error;}
          finally{progress.remove();if(output)await rasterWorker({operation:"output-close",metadata:output.token,buffers:[]}).catch(error=>message(String(error)));}
        } else if(candidate){const prepared=candidate;candidate=null;applyChange(["repair_source_profile","rasterize_source"].includes(r.type)?app.adopt_source(prepared):app.adopt_color(prepared));wake();}
        else applyChange(app.finish_document(id,false));
      } else if(r.type==="import_lookup") {
        const file=(await chooseFile(false,app.document_file_filter(id)))?.[0]?.file;
        if(!file){applyChange(app.finish_document(id,false));return;}
        const bytes=new Uint8Array(await file.slice(0,app.lookup_text_limit()+1).arrayBuffer());
        candidate=await app.prepare_lookup(id,bytes,file.name);
        const prepared=candidate;candidate=null;applyChange(app.adopt_lookup(prepared));wake();
      } else if(r.type==="copy") {
        await copyClip(id);
      } else if(r.type==="paste") {
        if(nativeKey&&!nativeKey.has(id))nativeDelivery(id);
        const files=nativeDeliveries.has(id)?await nativeDeliveries.get(id):null;
        nativeDeliveries.delete(id);
        if(files){
          if(!app.state().requests.some(request=>request.id===id))return;
          if(!files.length){applyChange(app.finish_document(id,false));return;}
          await images.run(id,()=>files);return;
        }
        const clipboard=await ownedClipboard();
        if(clipboard.own&&r.mode==="new_image")await openClipboard(id);
        else if(clipboard.own){applyChange(app.paste_clip(id));wake();}
        else await images.run(id,()=>clipboardImage(clipboard.items));
      } else if(r.type==="place") {
        await images.run(id,async()=>(await chooseFile(true))?.map(c=>c.file));
      } else if(["new","open"].includes(r.type)) {
        if(r.type==='new'){
          const options=await newDocument();if(!options){applyChange(app.finish_document(id,false));return;}
          await openDrawing({request:id,options});
        }else{
          const chosen=await chooseFile();if(!chosen?.length){applyChange(app.finish_document(id,false));return;}
          await openBatch(chosen,id);
        }
      } else if(r.type==="save"||r.type==="export") {
        if(r.type==="export"&&proof.hasPending()) {
          // Export has only requested its options; no capture or file write has
          // started. Release that request so the pending document edit can
          // commit, then request options against the resulting revision.
          const epoch=app.state().document_file.epoch;
          applyChange(app.finish_document(id,false));
          try {
            await proof.finishPending();
            if(app.state().document_file.epoch===epoch)dispatch({type:"invoke",command:r.repeat?"export_again":"export_document"});
          } catch(error) { message(error); }
          return;
        }
        const choice=r.repeat?{recipe:r.repeat.recipe}:r.type==="export"?await chooseExport({app,dialog,element,button,numberField,gpuOperation,id}):null;
        const recipe=choice?.recipe??null;
        if(r.type==="export"&&!recipe){applyChange(app.finish_document(id,false));return;}
        const target=await destination(r.repeat?{...r,location:r.repeat.location}:r,recipe);
        if(recipe){
          const extension=exportFormats[recipe.format][0];
          const extensions=extension==='jpg'?['jpg','jpeg']:extension==='tif'?['tif','tiff']:[extension];
          if(!extensions.includes(target.location.name.split('.').at(-1).toLowerCase()))throw deliveryFailure("export_extension",{extension:extensions[0]});
          const master=handles.get(app.state().document_file.location?.uri);
          if(master&&target.handle&&await master.isSameEntry?.(target.handle))throw deliveryFailure("choose_different");
        }
        let output,control,progress,progressLabel,progressPhase="preparing";
        const progressCaption=()=>progressPhase==="cancelling"?delivery.cancelling:exportCopy[progressPhase];
        try {
          if(recipe){
            control=app.capture_control();progress=element("aside","file-progress");progress.setAttribute("role","status");
            progressLabel=element("span","",progressCaption);progress.append(progressLabel,button(()=>common.cancel,()=>{control.cancel();progressPhase="cancelling";bindCopy(progressLabel,progressCaption);}));document.body.append(progress);
          }
          if(recipe)applyChange(app.dispatch({type:"prepare_export",id,owner:r.owner,recipe,location:target.location}));
          const bytes=(output=r.type==="save"?await app.save_project(id,target.location):await gpuOperation(()=>app.export_image(id,recipe,control))).blob;
          if(progress){progressPhase="writing_image";bindCopy(progressLabel,progressCaption);progress.querySelector('button').disabled=true;}

          let success;
          if(target.handle) {
            if(target.reused) {
              const observed=target.handle.getFile?await rasterWorker({operation:'fingerprint',file:await target.handle.getFile(),buffers:[]}):null;
              if(!app.session_destination_matches(observed))throw deliveryFailure('destination_changed');
            }
            const stream=await target.handle.createWritable();
            try {await stream.write(bytes);await stream.close();success=true;}
            catch(error){try{await stream.abort();}catch{}throw error;}
          } else success=!!await download(bytes,target.location.name,recipe?exportFormats[recipe.format][1]:"application/octet-stream");
          applyChange(app.finish_document(id,success));
          if(success&&r.type==='save'&&output.fingerprint&&app.state().document_file.location?.uri===target.location.uri)
            app.session_record_destination(target.location,output.fingerprint);
          if(success&&recipe&&!r.repeat)try{await app.export_presets({type:'remember',index:choice.destination<4?choice.destination:3,recipe});}catch(error){message(error?.color_feature_error!==undefined?{document_host_error:{type:'export_preferences',reason:error.color_feature_error}}:deliveryFailure('export_preferences',{detail:String(error)}));}

          if(success && r.type==="save" && !app.state().document_file.modified) {
            await recovery.capture();
          }
        } catch(error) {
          const wasCancelled=control?.cancelled();control?.cancel();
          if(wasCancelled)throw new DOMException("Image export cancelled","AbortError");throw error;
        } finally {
          progress?.remove();control?.free();
          if(output)try{await rasterWorker({operation:"output-close",metadata:output.token,buffers:[]});}catch(error){message(deliveryFailure("output_cleanup",{detail:String(error)}));}
        }
      } else throw new Error(`Unknown document operation: ${r.type}`);
    } catch(error) {
      if(request.kind.type==="document" && app.state().requests.some(r=>r.id===request.id)) {
        const cancelled=error?.name==="AbortError";applyChange(app.finish_document(request.id,false,cancelled?undefined:error));
      } else if(app.state().requests.some(r=>r.id===request.id))applyChange(app.finish_host_request(request.id,error));
      else if(error?.name!=="AbortError")message(error);
    } finally {candidate?.free();active.delete(request.id);pruneHandles();}
  }
  async function openClipboard(id,images,control){
    const ownControl=!control;
    control??=app.capture_control();
    const retire=()=>control.cancel();
    window.addEventListener('pagehide',retire);
    let candidate,progress;
    try {
      if(ownControl){
        progress=element('aside','file-progress');progress.setAttribute('role','status');
        progress.append(element('span','',()=>bootstrap.preparing_document),button(()=>common.cancel,retire));document.body.append(progress);
      }
      candidate=await gpuOperation(()=>app.prepare_clipboard_document(id,images,()=>control.cancelled()));
      if(control.cancelled())throw new DOMException("Paste cancelled","AbortError");
      applyChange(app.finish_document(id,true));
      await transition(()=>{
        if(control.cancelled())throw new DOMException("Paste cancelled","AbortError");
        const prepared=candidate;candidate=null;applyChange(app.adopt_document(prepared,null));
      });
    } finally {window.removeEventListener('pagehide',retire);candidate?.free();progress?.remove();if(ownControl)control.free();}
  }
  async function readyToPark(){
    const deadline=performance.now()+30000;
    while(!app.document_park_ready()){
      if(performance.now()>deadline)throw transportFailure("switch_operation");
      wake();await new Promise(resolve=>setTimeout(resolve,8));
    }
  }
  let spilling=null;
  function trim(){
    if(spilling)return spilling;
    const run=async()=>{
      try{while(await app.spill_document_tiles()){}app.document_storage_result();}
      catch(error){const detail=String(error);app.document_storage_result(detail);message(()=>app.document_storage_retained(detail));}
    };
    spilling=run().finally(()=>spilling=null);return spilling;
  }
  async function transition(action){
    if(changing)throw transportFailure("change_in_progress");
    changing=true;tabs.cancel();
    const blocker=element('div','document-transition');blocker.setAttribute('role','status');blocker.append(element('span','',()=>delivery.switching_drawing));document.body.append(blocker);
    try{
      await proof.pause();await readyToPark();
      await action();
      await trim();
      await recovery.ensure();
      await resumeCanvas();
    }finally{changing=false;blocker.remove();proof.resume();tabs.refresh(true);pruneHandles();wake();}
  }
  async function select(id){
    if(id==null||String(id)===String(app.document_tabs(0).selected))return;
    if(active.size||batching||closing||document.querySelector('dialog[open]'))throw transportFailure("switch_dialog");
    await transition(()=>applyChange(app.select_document(BigInt(id))));
  }
  async function close(id){
    await select(id);
    if(changing||batching||active.size)throw transportFailure("close_operation");
    const deadline=performance.now()+30000;
    while(!app.document_close_available()){
      if(performance.now()>deadline)throw transportFailure("close_operation");
      wake();await new Promise(resolve=>setTimeout(resolve,8));
    }
    if(String(id)!==String(app.document_tabs(0).selected))throw transportFailure("selected_changed");
    dispatch({type:'invoke',command:'close_document'});
  }
  async function openDrawing({request=null,file=null,options,bytes}){
    await trim();
    if(request===null){
      const change=app.dispatch({type:'invoke',command:file?'open_document':'new_document'});
      const pending=app.state().requests.find(r=>r.kind.type==='document'&&['new','open'].includes(r.kind.request.type));
      if(!pending)throw transportFailure("open_operation");
      request=pending.id;active.add(request);
      applyChange(change);
    }
    const id=request??0,fileState=app.state().document_file,target=file?location(file.file.name,file.handle):null;
    const progress=element('aside','file-progress');progress.setAttribute('role','status');let cancelled=false,candidate;
    progress.append(element('span','',()=>bootstrap.preparing_document),button(()=>common.cancel,()=>{cancelled=true;rasterWorker({operation:'cancel-read',metadata:'',buffers:[]}).catch(()=>{});}));document.body.append(progress);
    try{
      const original=file?.file??(bytes instanceof Blob?bytes:new Blob([bytes]));
      if(file)bytes=new Uint8Array(await file.file.arrayBuffer());
      else if(bytes instanceof Blob)bytes=new Uint8Array(await bytes.arrayBuffer());
      candidate=await gpuOperation(()=>app.prepare_document(id,bytes,0,0,fileState.epoch,fileState.revision,target?.name,options,()=>chooseSourceProfile({app,dialog,element,button}),()=>cancelled));
      if(cancelled)throw new DOMException('Opening cancelled','AbortError');
      if(app.package_view(candidate)){
        progress.remove();
        await showPackage(candidate,original,target?.name??"drawing.capy",file?.handle);
        if(request!==null)applyChange(app.finish_document(request,false));
        return false;
      }
      if(request!==null)applyChange(app.finish_document(request,true));
      await transition(()=>{const prepared=candidate;candidate=null;applyChange(app.adopt_document(prepared,target));});
      if(target&&app.state().document_file.location?.uri===target.uri) {
        const fingerprint=await rasterWorker({operation:'fingerprint',file:original,buffers:[]});
        if(app.state().document_file.location?.uri===target.uri)app.session_record_destination(target,fingerprint);
      }
      return true;
    }catch(error){
      if(request!==null&&app.state().requests.some(r=>r.id===request))applyChange(app.finish_document(request,false,error?.name==='AbortError'?undefined:error));
      throw error;
    }finally{candidate?.free();progress.remove();if(request!==null)active.delete(request);}
  }
  async function showPackage(candidate,original,name,originalHandle) {
    const model=app.package_view(candidate),preview=app.package_preview(candidate);
    const url=preview?URL.createObjectURL(new Blob([preview],{type:"image/png"})):null;
    try {
      await dialog(()=>model.status,(form,finish)=>{
        form.append(element("p","",model.reason));
        for(const output of model.outputs)form.append(element("p","",output.name));
        if(url){const image=element("img");image.src=url;image.style.maxWidth="100%";image.style.maxHeight="60vh";image.alt=model.status;form.append(image);}
        const footer=element("footer");
        const savePart=async(bytes,filename,mime,previewOnly)=>{
          try {
            if(window.showSaveFilePicker){
              const handle=await window.showSaveFilePicker({suggestedName:filename});
              if(previewOnly&&originalHandle&&await originalHandle.isSameEntry(handle))throw new Error(model.destination_error);
              const stream=await handle.createWritable();
              try{await stream.write(bytes);await stream.close();}catch(error){await stream.abort();throw error;}
            }else await download(bytes,filename,mime);
          }catch(error){if(error?.name!=="AbortError")message(String(error));}
        };
        if(model.capabilities.copy_original)footer.append(button(()=>model.copy_original,()=>savePart(original,name,"application/vnd.capycanvas",false)));
        if(model.capabilities.export&&preview)footer.append(button(()=>model.export_preview,()=>savePart(new Blob([preview],{type:"image/png"}),name.replace(/\.[^.]+$/,"")+".png","image/png",true)));
        footer.append(button(()=>model.close,()=>finish(null)));form.append(footer);
      });
    } finally {if(url)URL.revokeObjectURL(url);}
  }
  async function openBatch(files,request=null){
    if(batching)throw transportFailure("batch_opening");
    batching=true;
    try{for(const [index,file] of files.entries()){
      try{await openDrawing({request:index===0?request:null,file});}
      catch(error){if(error?.name==='AbortError')break;if(files.length===1)throw error;console.error(error);const name=file.file.name;message(()=>app.file_open_failure(name));}
    }}finally{batching=false;tabs.refresh(true);}
  }
  async function openFiles(files){
    if(active.size||batching||changing||closing||document.querySelector('dialog[open]'))throw transportFailure("open_drawings_operation");
    await openBatch(files.map(value=>value.file?value:{file:value}));
  }
  async function restoreSession(checkpoint,buffers,recovered,initial,storedHandles,activate,identity) {
    const observe=async location=>{
      const handle=storedHandles.find(saved=>saved.uri===location?.uri)?.handle;
      if(!handle)return null;
      try {
        if(await handle.queryPermission({mode:'read'})!=='granted')return null;
        return await rasterWorker({operation:'fingerprint',file:await handle.getFile(),buffers:[]});
      } catch {return null;}
    };
    let candidate=await app.prepare_session_restart(checkpoint,buffers,recovered,observe);
    try {
      const replace=activate&&app.session_can_replace_startup(initial);
      let adopted;
      const publish=()=>{const prepared=candidate;candidate=null;adopted=app.adopt_session_restart(prepared,initial,BigInt(identity),activate);applyChange(adopted.change);};
      if(replace)await transition(publish);else publish();
      for(const {uri,handle} of storedHandles){if(handle)handles.set(uri,handle);}
      return adopted.id;
    } finally {candidate?.free();}
  }
  const recovery=createDocumentRecovery({app,call:rasterWorker,message,restore:restoreSession,settled:trim,
    reserve:ids=>app.reserve_session_ids(ids.map(BigInt)),failed:()=>dialog(()=>bootstrap.recovery.attention,(form,finish)=>{form.append(element('p','',()=>bootstrap.recovery.explanation));const footer=element('footer');footer.append(button(()=>bootstrap.recovery.later,()=>finish('later')),button(()=>bootstrap.recovery.discard,()=>finish('discard')),button(()=>bootstrap.recovery.retry,()=>finish('retry'),'suggested-action'));form.append(footer);}),
    handles:id=>{const state=app.session_stamp_for(BigInt(id)).state;return [...new Set([state.location?.uri,state.last_export?.location.uri])].flatMap(uri=>{const handle=handles.get(uri);return typeof FileSystemFileHandle!=='undefined'&&handle instanceof FileSystemFileHandle?[{uri,handle}]:[];});},order:order=>applyChange(app.restore_session_order(order)),
    canOffer:()=>app.gpu_ready()&&!document.hidden&&!active.size&&!batching&&!changing&&!closing&&!document.querySelector('dialog[open]')&&app.document_park_ready()});
  const tabs=createDrawingTabs({app,element,button,icon,applyChange,select,close,openFiles,message,busy:()=>changing||batching||closing});
  return {title:tabs.root,key(event,send){
    if((event.ctrlKey||event.metaKey)&&!event.altKey&&event.key.toLowerCase()==='v'){
      keyboardPaste=true;setTimeout(()=>{keyboardPaste=false;},0);
      if(!navigator.clipboard?.read){
        nativeKey=!event.repeat&&!event.defaultPrevented&&!editing(event.target)?new Set(app.state().requests.map(request=>request.id)):null;
        try {send();} finally {nativeKey=null;}
        return true;
      }
    }
    return tabs.key(event);
  },allowNativePaste,select,close,openFiles,busy:()=>changing||batching||closing||active.size>0,showSelector:tabs.showSelector,
    mountProof:proof.mount,localize(){for(const form of openDialogs)form.localize?.();proof.sync();tabs.refresh(true);},handle,autosave:recovery.autosave,startRecovery:recovery.start,refresh(){
    proof.sync();tabs.refresh();
    const requests=new Set(app.state().requests.map(request=>request.id));
    if(nativePending&&!requests.has(nativePending.id))finishNativePaste();
    for(const id of nativeDeliveries.keys())if(!requests.has(id))nativeDeliveries.delete(id);
    const published=state();
    if(closing||changing||!published.document_file.close_ready)return;
    closing=true;
    const id=app.document_tabs(0).selected;
    transition(async()=>{let prepared=app.prepare_document_close();try{await recovery.retire(id,true);const close=prepared;prepared=null;applyChange(app.commit_document_close(close));}finally{prepared?.free();}})
      .catch(error=>{applyChange(app.cancel_document_close());message(error);})
      .finally(()=>{closing=false;tabs.refresh(true);pruneHandles();});
  }};
}
