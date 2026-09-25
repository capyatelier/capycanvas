import {createDrawingTabs} from './drawing-tabs.js';
import {createDocumentRecovery} from './document-recovery.js';
import {chooseDocumentColor} from './document-color.js';
import {createProof} from './proof.js';
import {createHistogram} from './histogram.js';
import {chooseExport,chooseSourceProfile} from './export-controls.js';

const exportFormats={
  Exr:["exr","image/x-exr","OpenEXR image"],
  PngHdr:["png","image/png","HDR PQ PNG"],PngHdrMapped:["png","image/png","HDR PQ PNG"],
  JpegHdr:["jpg","image/jpeg","HDR gain-map JPEG"],JpegHdrMapped:["jpg","image/jpeg","HDR gain-map JPEG"],
  AvifHdr:["avif","image/avif","HDR gain-map AVIF"],AvifHdrMapped:["avif","image/avif","HDR gain-map AVIF"],
  Png:["png","image/png","PNG image"],Tiff:["tif","image/tiff","TIFF image"],Jpeg:["jpg","image/jpeg","JPEG image"],
};
import {createImageImport} from './image-import.js';
// Browser file transport; document checkpoints, stale-edit guards and unsaved
// decisions stay in UiSession. File handles never enter a project or localStorage.
export function createDocuments({app,state,canvas,dispatch,applyChange,wake,element,button,icon,message,gpuOperation,rasterWorker,resumeCanvas,contentChanged}) {
  const active=new Set(),handles=new Map(),histogram=createHistogram({app,element,button});
  let nextHandle=0,closing=false,changing=false,batching=false;
  const images=createImageImport({app,canvas,dispatch,applyChange,wake,element,button,icon,message,gpuOperation,
    interpret:()=>chooseSourceProfile({app,dialog,element,button})});
  const pruneHandles=()=>{const live=new Set(app.document_tabs(0).tabs.map(t=>t.uri));for(const key of handles.keys())if(!live.has(key))handles.delete(key);};
  const location=(name,handle)=>{const uri=`browser:${++nextHandle}`;if(handle)handles.set(uri,handle);return{uri,name};};
  const dialog=(title,build)=>new Promise(resolve=>{
    const root=element("dialog","document-dialog"),form=element("form");
    form.method="dialog";form.append(element("h2","",title));root.append(form);document.body.append(root);
    let result=null;root.addEventListener("close",()=>{root.remove();resolve(result);},{once:true});
    const finish=value=>{result=value;root.close();};
    build(form,finish);root.showModal();
  });
  const cancel=(footer,finish)=>footer.append(button("Cancel",()=>finish(null)));
  const proof=createProof({app,dialog,element,button,icon,applyChange,wake,dispatch,contentChanged});
  async function newDocument() {
    const spec=app.editor_models(innerWidth,innerHeight).document_options,model=spec.creation;
    return dialog(spec.new_title,(form,finish)=>{
      const field=(title,node)=>{const label=element("label","document-size",title);node.setAttribute("aria-label",title);label.append(node);form.append(label);return node;};
      const select=(title,choices,value)=>{const node=element("select");for(const [id,label] of choices){const option=element("option","",label);option.value=id;node.append(option);}node.value=value;return field(title,node);};
      const preset=select("Preset",[["custom","Custom"],...model.presets.map((p,i)=>[String(i),p.name])],"custom");
      const fields=model.options.extent.map((value,i)=>{
        const input=element("input");Object.assign(input,{type:"number",min:1,max:spec.max_dimension,step:1,value,required:true});
        return field(i?spec.height_label:spec.width_label,input);
      });
      const space=select("Color space",model.spaces,model.options.color.space);
      const depth=select("Bit depth",[["U8","8-bit SDR"],["U16","16-bit SDR"],["F16","16-bit float HDR"],["F32","32-bit float HDR"]],model.options.color.depth);
      const background=select("Background",[["White","White"],["Transparent","Transparent"]],model.options.background);
      const read=()=>({extent:fields.map(i=>Number(i.value)),color:{space:space.value,depth:depth.value},background:background.value});
      preset.onchange=()=>{const p=model.presets[Number(preset.value)];if(!p)return;fields.forEach((f,i)=>f.value=p.options.extent[i]);space.value=p.options.color.space;depth.value=p.options.color.depth;background.value=p.options.background;};
      const name=field("Save as preset",element("input"));name.maxLength=64;name.placeholder="Optional name";
      const remember=element("input");remember.type="checkbox";field("Use as defaults",remember);
      const error=element("p","error-message");form.append(error);
      const footer=element("footer");cancel(footer,finish);
      const create=button("Create",()=>{if(!form.reportValidity())return;const options=read();try{
        applyChange(app.dispatch({type:"new_document_preferences",action:{type:"remember",options,name:name.value,defaults:remember.checked}}));
        finish(options);
      }catch(e){error.textContent=String(e);}},"suggested-action");
      footer.append(create);form.append(footer);form.onsubmit=e=>{e.preventDefault();create.click();};
    });
  }
  async function clipboardImage() {
    if(!navigator.clipboard?.read)throw new Error("Image paste is unavailable in this browser. Use Import Image as Layer.");
    const items=await navigator.clipboard.read();
    const formats=app.photo_formats(),preferred=formats.flatMap(f=>f.mime_types.flatMap(m=>[`web ${m}`,m]));
    const files=[];
    for(const item of items) {
      const type=preferred.find(type=>item.types.includes(type));if(!type)throw new Error(`Copy a supported image (${formats.map(f=>f.name).join(', ')}) to paste.`);
      const blob=await item.getType(type),mime=type.replace(/^web /,"");
      if(blob.size>512*1024*1024)throw new Error("Clipboard image exceeds 512 MiB");
      const extension=formats.find(f=>f.mime_types.includes(mime)).extensions[0];
      files.push(new File([blob],`Pasted image.${extension}`,{type:mime}));
    }
    if(!files.length)throw new Error("Copy an image to paste, or import the original file.");
    return files;
  }
  function chooseFile(placing=false) {
    const formats=app.photo_formats(),accept=Object.fromEntries(formats.flatMap(f=>f.mime_types.map(m=>[m,f.extensions.map(e=>'.'+e)])));
    if(!placing)accept['application/octet-stream']=['.capy'];
    if(window.showOpenFilePicker) return window.showOpenFilePicker({multiple:true,types:[{description:placing?"Images":"Drawing or photo",accept}]})
      .then(async handles=>Promise.all(handles.map(async handle=>({file:await handle.getFile(),handle}))));
    return new Promise(resolve=>{
      const input=element("input");input.type="file";input.multiple=true;input.accept=Object.values(accept).flat().join(',');input.hidden=true;document.body.append(input);
      const done=value=>{input.remove();resolve(value);};
      input.onchange=()=>done([...input.files].map(file=>({file})));input.oncancel=()=>done(null);input.click();
    });
  }
  async function download(bytes,name,mime) {
    const url=URL.createObjectURL(new Blob([bytes],{type:mime}));
    try {
      return await dialog("Download file",(form,finish)=>{
        form.append(element("p","",`Download “${name}”, then confirm it was saved.`));
        const footer=element("footer");cancel(footer,finish);
        const done=button("File saved",()=>finish(true),"suggested-action");done.disabled=true;
        const start=button("Download",()=>{const a=element("a");a.href=url;a.download=name;a.click();done.disabled=false;});
        footer.append(start,done);form.append(footer);
      });
    } finally {URL.revokeObjectURL(url);}
  }
  async function destination(request,recipe) {
    const old=request.location && handles.get(request.location.uri);
    if(old)return{location:request.location,handle:old};
    if(window.showSaveFilePicker) {
      const [extension,mime,description]=recipe?exportFormats[recipe.format]:["capy","application/octet-stream","Capy Canvas drawing"];
      const name=recipe?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name;
      const handle=await window.showSaveFilePicker({suggestedName:name,types:[{description,accept:{[mime]:["."+extension]}}]});
      return{location:location(handle.name,handle),handle};
    }
    const extension=recipe?exportFormats[recipe.format][0]:null;
    return {location:location(extension?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name)};
  }
  async function handle(request) {
    if(active.has(request.id))return;active.add(request.id);
    const ownerId=app.document_tabs(0).selected;
    let candidate;
    try {
      if(request.kind.type==='drawings'){dispatch({type:'complete_request',id:request.id});tabs.showSelector();return;}
      if(["soft_proof_setup","sdr_rendition"].includes(request.kind.type)){await proof.run(request.id,request.kind.type==="sdr_rendition");if(app.state().requests.some(r=>r.id===request.id))dispatch({type:"complete_request",id:request.id});return;}
      if(request.kind.type==="histogram"){histogram.open();dispatch({type:"complete_request",id:request.id});return;}
      if(request.kind.type!=="document")throw new Error(`Unsupported host request: ${request.kind.type}`);
      const r=request.kind.request,id=request.id;
      if(r.type==="confirm_close") {
        const spec=app.editor_models(innerWidth,innerHeight).document_options;
        const decision=await dialog(r.title,(form,finish)=>{
          form.append(element("p","",spec.unsaved_description));const footer=element("footer");cancel(footer,finish);
          footer.append(button(spec.discard_label,()=>finish("discard")),button("Save",()=>finish("save"),"suggested-action"));form.append(footer);
        });
        applyChange(app.respond_document(id,decision??"cancel"));return;
      }
      if(r.type==="properties") {
        const rows=await app.document_properties();
        await dialog("Document Properties",(form,finish)=>{for(const [name,value]of rows){form.append(element("h3","",name),element("p","source-details",value));}form.append(button("Done",()=>finish(true)));});
        applyChange(app.finish_document(id,true));
      } else if(["change_color","color_history","repair_source_profile","rasterize_source"].includes(r.type)) {
        candidate=await chooseDocumentColor({app,dialog,element,button,gpuOperation,request:r,id});
        if(candidate?.is_copy?.()) {
          const master=app.state().document_file.location;
          const target=await destination({name:(master?.name??"Drawing.capy").replace(/\.[^.]+$/,"")+" converted.capy"});
          if(!target.location.name.toLowerCase().endsWith(".capy"))throw new Error("Use a .capy filename for the converted drawing.");
          const original=handles.get(master?.uri);
          if(original&&target.handle&&await original.isSameEntry?.(target.handle))throw new Error("Choose a different file to keep the editable drawing.");
          const progress=element("aside","file-progress");progress.setAttribute("role","status");
          let cancelled=false;
          progress.append(element("span","","Preparing converted copy…"),button("Cancel",()=>{cancelled=true;candidate.cancel();}));document.body.append(progress);
          try {
            const bytes=await app.save_color_copy(candidate);
            if(cancelled)throw new DOMException("Converted copy cancelled","AbortError");
            progress.firstChild.textContent="Writing converted copy…";progress.querySelector("button").disabled=true;
            let success;
            if(target.handle){const stream=await target.handle.createWritable();try{await stream.write(bytes);await stream.close();success=true;}catch(error){try{await stream.abort();}catch{}throw error;}}
            else success=!!await download(bytes,target.location.name,"application/octet-stream");
            applyChange(app.finish_document(id,success));
          } catch(error){if(cancelled)throw new DOMException("Converted copy cancelled","AbortError");throw error;}
          finally{progress.remove();}
        } else if(candidate){const prepared=candidate;candidate=null;applyChange(["repair_source_profile","rasterize_source"].includes(r.type)?app.adopt_source(prepared):app.adopt_color(prepared));wake();}
        else applyChange(app.finish_document(id,false));
      } else if(["place","paste"].includes(r.type)) {
        await images.run(id,async()=>r.type==="paste"?clipboardImage():(await chooseFile(true))?.map(c=>c.file));
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
            if(app.state().document_file.epoch===epoch)dispatch({type:"invoke",command:"export_document"});
          } catch(error) { message(String(error)); }
          return;
        }
        const choice=r.type==="export"?await chooseExport({app,dialog,element,button,gpuOperation,id}):null;
        const recipe=choice?.recipe??null;
        if(r.type==="export"&&!recipe){applyChange(app.finish_document(id,false));return;}
        const target=await destination(r,recipe);
        if(recipe){
          const extension=exportFormats[recipe.format][0];
          const extensions=extension==='jpg'?['jpg','jpeg']:extension==='tif'?['tif','tiff']:[extension];
          if(!extensions.includes(target.location.name.split('.').at(-1).toLowerCase()))throw new Error(`Use a .${extensions[0]} filename for this image format.`);
          const master=handles.get(app.state().document_file.location?.uri);
          if(master&&target.handle&&await master.isSameEntry?.(target.handle))throw new Error("Choose a different file to keep the editable drawing.");
        }
        let output,control,progress;
        try {
          if(recipe){
            control=app.capture_control();progress=element("aside","file-progress");progress.setAttribute("role","status");
            progress.append(element("span","","Preparing image…"),button("Cancel",()=>{control.cancel();progress.firstChild.textContent="Cancelling…";}));document.body.append(progress);
          }
          const bytes=r.type==="save"?await app.save_project(id,target.location):(output=await gpuOperation(()=>app.export_image(id,recipe,control))).blob;
          if(progress){progress.firstChild.textContent="Writing image…";progress.querySelector('button').disabled=true;}

          let success;
          if(target.handle) {
            const stream=await target.handle.createWritable();
            try {await stream.write(bytes);await stream.close();success=true;}
            catch(error){try{await stream.abort();}catch{}throw error;}
          } else success=!!await download(bytes,target.location.name,recipe?exportFormats[recipe.format][1]:"application/octet-stream");
          applyChange(app.finish_document(id,success));
          if(success&&recipe)try{await app.export_presets({type:"remember",index:choice.destination<4?choice.destination:3,recipe});}catch(error){message(`Image saved; export preferences were not saved: ${error}`);}
          if(success && r.type==="save" && !app.state().document_file.modified) {
            await recovery.retire(ownerId);
          }
        } catch(error) {
          const wasCancelled=control?.cancelled();control?.cancel();
          if(wasCancelled)throw new DOMException("Image export cancelled","AbortError");throw error;
        } finally {
          progress?.remove();control?.free();
          if(output)try{await rasterWorker({operation:"output-close",metadata:output.token,buffers:[]});}catch(error){message(`Temporary output cleanup failed: ${error}`);}
        }
      } else throw new Error(`Unknown document operation: ${r.type}`);
    } catch(error) {
      if(request.kind.type==="document" && app.state().requests.some(r=>r.id===request.id)) {
        const cancelled=error?.name==="AbortError";applyChange(app.finish_document(request.id,false,cancelled?undefined:String(error)));
      } else if(app.state().requests.some(r=>r.id===request.id))dispatch({type:"complete_request",id:request.id,error:String(error)});
      else if(error?.name!=="AbortError")message(String(error));
    } finally {candidate?.free();active.delete(request.id);pruneHandles();}
  }
  async function readyToPark(){
    const deadline=performance.now()+30000;
    while(!app.document_park_ready()){
      if(performance.now()>deadline)throw Error('Finish the current operation before switching drawings.');
      wake();await new Promise(resolve=>setTimeout(resolve,8));
    }
  }
  let spilling=null;
  function trim(){
    if(spilling)return spilling;
    const run=async()=>{
      try{while(await app.spill_document_tiles()){}app.document_storage_result();}
      catch(error){app.document_storage_result(String(error));message(`${error}\nThe drawing is retained in memory. Free disk space or close some tabs.`);}
    };
    spilling=run().finally(()=>spilling=null);return spilling;
  }
  async function transition(action,capture=true){
    if(changing)throw Error('A drawing change is already in progress.');
    changing=true;tabs.cancel();
    const blocker=element('div','document-transition');blocker.setAttribute('role','status');blocker.append(element('span','','Switching drawing…'));document.body.append(blocker);
    try{
      await proof.pause();await histogram.retire();await readyToPark();
      if(capture)await recovery.capture().catch(error=>message(`Recovery unavailable: ${error}`));
      await action();
      await trim();
      await recovery.ensure();
      await resumeCanvas();
    }finally{changing=false;blocker.remove();proof.resume();tabs.refresh(true);pruneHandles();wake();}
  }
  async function select(id){
    if(id==null||String(id)===String(app.document_tabs(0).selected))return;
    if(active.size||batching||closing||document.querySelector('dialog[open]'))throw Error('Finish the current dialog before switching drawings.');
    await transition(()=>applyChange(app.select_document(BigInt(id))));
  }
  async function close(id){
    await select(id);
    if(changing||batching||active.size)throw Error('Finish the current operation before closing the drawing.');
    const deadline=performance.now()+30000;
    while(!app.document_close_available()){
      if(performance.now()>deadline)throw Error('Finish the current operation before closing the drawing.');
      wake();await new Promise(resolve=>setTimeout(resolve,8));
    }
    if(String(id)!==String(app.document_tabs(0).selected))throw Error('The selected drawing changed before closing.');
    dispatch({type:'invoke',command:'close_document'});
  }
  async function openDrawing({request=null,file=null,options,recovered=false,bytes}){
    await trim();
    if(request===null&&!recovered){
      const change=app.dispatch({type:'invoke',command:file?'open_document':'new_document'});
      const pending=app.state().requests.find(r=>r.kind.type==='document'&&['new','open'].includes(r.kind.request.type));
      if(!pending)throw Error('Finish the current operation before opening a drawing.');
      request=pending.id;active.add(request);
      applyChange(change);
    }
    const id=request??0,fileState=app.state().document_file,target=file?location(file.file.name,file.handle):null;
    const progress=element('aside','file-progress');progress.setAttribute('role','status');let cancelled=false,candidate;
    progress.append(element('span','','Preparing drawing…'),button('Cancel',()=>{cancelled=true;rasterWorker({operation:'cancel-read',metadata:'',buffers:[]}).catch(()=>{});}));document.body.append(progress);
    try{
      if(file)bytes=new Uint8Array(await file.file.arrayBuffer());
      candidate=await gpuOperation(()=>app.prepare_document(id,bytes,0,0,fileState.epoch,fileState.revision,recovered,target?.name,options,()=>chooseSourceProfile({app,dialog,element,button}),()=>cancelled));
      if(cancelled)throw new DOMException('Opening cancelled','AbortError');
      if(request!==null)applyChange(app.finish_document(request,true));
      await transition(()=>{const prepared=candidate;candidate=null;applyChange(app.adopt_document(prepared,target));});
    }catch(error){
      if(request!==null&&app.state().requests.some(r=>r.id===request))applyChange(app.finish_document(request,false,error?.name==='AbortError'?undefined:String(error)));
      throw error;
    }finally{candidate?.free();progress.remove();if(request!==null)active.delete(request);}
  }
  async function openBatch(files,request=null){
    if(batching)throw Error('Another file batch is still opening.');
    batching=true;
    try{for(const [index,file] of files.entries()){
      try{await openDrawing({request:index===0?request:null,file});}
      catch(error){if(error?.name==='AbortError')break;if(files.length===1)throw error;message(`Could not open ${file.file.name}: ${error}`);}
    }}finally{batching=false;tabs.refresh(true);}
  }
  async function openFiles(files){
    if(active.size||batching||changing||closing||document.querySelector('dialog[open]'))throw Error('Finish the current operation before opening drawings.');
    await openBatch(files.map(value=>value.file?value:{file:value}));
  }
  const recovery=createDocumentRecovery({app,call:rasterWorker,dialog,element,button,message,restore:bytes=>openDrawing({recovered:true,bytes}),settled:trim,
    canOffer:()=>!active.size&&!batching&&!changing&&!closing&&!document.querySelector('dialog[open]')&&app.document_park_ready()});
  const tabs=createDrawingTabs({app,element,button,icon,applyChange,select,close,openFiles,message,busy:()=>changing||batching||closing});
  return {title:tabs.root,key:tabs.key,select,close,openFiles,busy:()=>changing||batching,showSelector:tabs.showSelector,
    mountProof:proof.mount,handle,autosave:recovery.autosave,startRecovery:recovery.start,refresh(){
    proof.sync();tabs.refresh();
    const published=state();images.refresh(published);
    if(closing||changing||!published.document_file.close_ready)return;
    closing=true;
    const id=app.document_tabs(0).selected;
    transition(async()=>{await recovery.retire(id,true);applyChange(app.close_document_tab());},false)
      .catch(error=>{app.reset_document_close();message(String(error));})
      .finally(()=>{closing=false;tabs.refresh(true);pruneHandles();});
  }};
}
