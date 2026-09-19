import {chooseDocumentColor} from './document-color.js';
import {createProof} from './proof.js';
import {createHistogram} from './histogram.js';
import {chooseExport,chooseSourceProfile} from './export-controls.js';
import {createImageImport} from './image-import.js';
// Browser file transport; document checkpoints, stale-edit guards and unsaved
// decisions stay in UiSession. File handles never enter a project or localStorage.
export function createDocuments({app,state,canvas,dispatch,applyChange,wake,element,button,icon,message,gpuOperation,rasterWorker}) {
  const active=new Set(),handles=new Map(),histogram=createHistogram({app,element,button});
  let nextHandle=0,closing=false;
  const images=createImageImport({app,canvas,dispatch,applyChange,wake,element,button,icon,message,gpuOperation,
    interpret:()=>chooseSourceProfile({app,dialog,element,button})});
  const pruneHandles=()=>{const current=app.state().document_file.location?.uri;for(const key of handles.keys())if(key!==current)handles.delete(key);};
  const location=(name,handle)=>{const uri=`browser:${++nextHandle}`;if(handle)handles.set(uri,handle);return{uri,name};};
  const dialog=(title,build)=>new Promise(resolve=>{
    const root=element("dialog","document-dialog"),form=element("form");
    form.method="dialog";form.append(element("h2","",title));root.append(form);document.body.append(root);
    let result=null;root.addEventListener("close",()=>{root.remove();resolve(result);},{once:true});
    const finish=value=>{result=value;root.close();};
    build(form,finish);root.showModal();
  });
  const cancel=(footer,finish)=>footer.append(button("Cancel",()=>finish(null)));
  const proof=createProof({app,dialog,element,button,icon,applyChange,wake});
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
    if(window.showOpenFilePicker) return window.showOpenFilePicker({multiple:placing,types:[{description:placing?"Images":"Drawing or photo",accept}]})
      .then(async handles=>Promise.all(handles.map(async handle=>({file:await handle.getFile(),handle}))));
    return new Promise(resolve=>{
      const input=element("input");input.type="file";input.multiple=placing;input.accept=Object.values(accept).flat().join(',');input.hidden=true;document.body.append(input);
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
      const formats={Exr:["exr","image/x-exr","OpenEXR image"],PngHdr:["png","image/png","HDR PQ PNG"],PngHdrMapped:["png","image/png","HDR PQ PNG"],Png:["png","image/png","PNG image"],Tiff:["tif","image/tiff","TIFF image"],Jpeg:["jpg","image/jpeg","JPEG image"]};
      const [extension,mime,description]=recipe?formats[recipe.format]:["capy","application/octet-stream","Capy Canvas drawing"];
      const name=recipe?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name;
      const handle=await window.showSaveFilePicker({suggestedName:name,types:[{description,accept:{[mime]:["."+extension]}}]});
      return{location:location(handle.name,handle),handle};
    }
    const extension=recipe?{Exr:"exr",PngHdr:"png",PngHdrMapped:"png",Png:"png",Tiff:"tif",Jpeg:"jpg"}[recipe.format]:null;
    return {location:location(extension?request.name.replace(/\.[^.]+$/,"")+"."+extension:request.name)};
  }
  async function handle(request) {
    if(active.has(request.id))return;active.add(request.id);
    let candidate;
    try {
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
        const fileState=app.state().document_file;let bytes,inputFile,extent=[0,0],target=null,options;
        if(r.type==="new") {options=await newDocument();if(!options){applyChange(app.finish_document(id,false));return;}}
        else {const chosen=(await chooseFile())?.[0];if(!chosen){applyChange(app.finish_document(id,false));return;}
          inputFile=chosen.file;target=location(chosen.file.name,chosen.handle);}
        const progress=element("aside","file-progress");progress.setAttribute("role","status");let cancelled=false;
        progress.append(element("span","","Preparing drawing…"),button("Cancel",()=>{cancelled=true;progress.firstChild.textContent="Cancelling…";rasterWorker({operation:"cancel-read",metadata:"",buffers:[]}).catch(()=>{});}));document.body.append(progress);
        try{
          if(inputFile)bytes=new Uint8Array(await inputFile.arrayBuffer());
          candidate=await gpuOperation(()=>app.prepare_document(id,bytes,...extent,fileState.epoch,fileState.revision,false,target?.name,options,()=>chooseSourceProfile({app,dialog,element,button}),()=>cancelled));
          if(cancelled)throw new DOMException("Opening cancelled","AbortError");
          applyChange(app.adopt_document(candidate,target));candidate=null;wake();
          await retireRecovery();
        }finally{progress.remove();}
      } else if(r.type==="save"||r.type==="export") {
        const choice=r.type==="export"?await chooseExport({app,dialog,element,button,gpuOperation,id}):null;
        const recipe=choice?.recipe??null;
        if(r.type==="export"&&!recipe){applyChange(app.finish_document(id,false));return;}
        const target=await destination(r,recipe);
        if(recipe){
          const extensions={Exr:['exr'],PngHdr:['png'],PngHdrMapped:['png'],Png:['png'],Tiff:['tif','tiff'],Jpeg:['jpg','jpeg']}[recipe.format];
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
          } else success=!!await download(bytes,target.location.name,recipe?{Exr:"image/x-exr",PngHdr:"image/png",PngHdrMapped:"image/png",Png:"image/png",Tiff:"image/tiff",Jpeg:"image/jpeg"}[recipe.format]:"application/octet-stream");
          applyChange(app.finish_document(id,success));
          if(success&&recipe)try{await app.export_presets({type:"remember",index:choice.destination<4?choice.destination:3,recipe});}catch(error){message(`Image saved; export preferences were not saved: ${error}`);}
          if(success && r.type==="save" && !app.state().document_file.modified) {
            await retireRecovery();
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
      if(request.kind.type==="document") {
        const cancelled=error?.name==="AbortError";applyChange(app.finish_document(request.id,false,cancelled?undefined:String(error)));
      } else dispatch({type:"complete_request",id:request.id,error:String(error)});
    } finally {candidate?.free();active.delete(request.id);pruneHandles();}
  }
  // Each live tab owns its recovery record through a Web Lock. Abandoned
  // records can be offered in another tab without racing a live drawing.
  const recoveryKey=crypto.randomUUID(),lockName=key=>`capy-raster:${key}`;
  let recoveryBusy=false,recoveryStarted=false,recoveryPolicy="",recoveryJobs=Promise.resolve();
  const heldOrigins=new Set();
  const recoveryCall=(operation,key="")=>rasterWorker({operation:`recover-${operation}`,metadata:key,buffers:[]});
  const recoveryEvent=event=>{
    const result=app.recovery_update(recoveryPolicy,event);recoveryPolicy=result.state;return result.update;
  };
  const observeRecovery=()=>recoveryEvent({type:"observe",document:app.recovery_document(),owned:recoveryStarted});
  function executeRecovery(first) {
    if(!first)return recoveryJobs;
    const run=async()=>{
      let work=first;
      while(work){
        let success=false;
        try{
          switch(work.kind.type){
            case "capture":await app.save_recovery(recoveryKey);success=true;break;
            case "retire":await recoveryCall("delete",recoveryKey);success=true;break;
            case "retire_origin":{
              const key=work.kind.key;
              const remove=async()=>{await recoveryCall("delete",key);success=true;};
              if(heldOrigins.has(key))await remove();
              else await navigator.locks.request(lockName(key),{ifAvailable:true},async lock=>{if(lock)await remove();});
              break;
            }
            case "restore":{
              let candidate;
              try{
                const state=app.state().document_file,bytes=await recoveryCall("get",work.kind.key);
                candidate=await gpuOperation(()=>app.prepare_document(0,bytes,0,0,state.epoch,state.revision,true));
                applyChange(app.adopt_document(candidate,null));candidate=null;wake();
                observeRecovery();success=true;
              }finally{candidate?.free();}
              break;
            }
          }
        }catch(error){message(`Recovery operation failed: ${error}`);}
        work=recoveryEvent({type:"complete",token:work.token,success}).work;
      }
    };
    recoveryJobs=recoveryJobs.then(run,run);return recoveryJobs;
  }
  async function retireRecovery() {
    await executeRecovery(recoveryEvent({type:"retire",discard_origin:true}).work);
  }
  async function autosave() {
    if(!recoveryStarted)return;
    await executeRecovery(observeRecovery().work);
  }
  let recoveryInitialization;
  function startRecovery() { return recoveryInitialization ||= initializeRecovery(); }
  async function initializeRecovery() {
    if(!navigator.locks)throw new Error("Recovery storage requires Web Locks");
    await new Promise(resolve=>navigator.locks.request(lockName(recoveryKey),()=>{resolve();return new Promise(()=>{});}));
    recoveryStarted=true;
    await executeRecovery(observeRecovery().work);
    for(const key of await recoveryCall("list")) {
      await navigator.locks.request(lockName(key),{ifAvailable:true},async lock=>{
        if(!lock||key===recoveryKey)return;
        heldOrigins.add(key);
        try {
          const offered=recoveryEvent({type:"offer",key,owned:true});
          if(offered.offer!==key)return;
          const decision=await dialog("Recover drawing?",(form,finish)=>{
            form.append(element("p","","An unsaved drawing from a closed tab is available."));
            const footer=element("footer");
            footer.append(button("Keep for Later",()=>finish(null)),button("Discard",()=>finish("discard")),button("Recover",()=>finish("recover"),"suggested-action"));form.append(footer);
          });
          const event=decision==="recover"?{type:"restore"}:{type:"dismiss",discard:decision==="discard"};
          await executeRecovery(recoveryEvent(event).work);
        } finally {heldOrigins.delete(key);}
      });
      if(app.state().document_file.modified)break;
    }
  }
  setInterval(()=>{
    if(!recoveryStarted && !recoveryBusy && app.gpu_ready() && app.brush_ready()) {
      recoveryBusy=true;startRecovery().catch(error=>message(`Recovery unavailable: ${error}`)).finally(()=>{recoveryBusy=false;});
    } else autosave();
  },15000);
  document.addEventListener("visibilitychange",()=>{if(document.hidden)autosave();});
  // Exposed on the existing host controller for deterministic lifecycle tests.
  window.addEventListener("beforeunload",e=>{if(app.state().document_file.modified){e.preventDefault();e.returnValue="";}});
  return {handle,autosave,startRecovery,refresh(){
    proof.sync();
    const published=state();
    images.refresh(published);
    if(closing || !published.document_file.close_ready)return;
    closing=true;
    const current=app.state().document_file,extent=app.editor_models(innerWidth,innerHeight).document_options.extent;
    // Close leaves an empty untitled workspace after the shared unsaved decision.
    gpuOperation(()=>app.prepare_document(0,undefined,...extent,current.epoch,current.revision))
      .then(async candidate=>{applyChange(app.adopt_document(candidate,null));wake();await retireRecovery();})
      .catch(error=>{app.reset_document_close();message(error);})
      .finally(()=>{closing=false;pruneHandles();});
  }};
}
