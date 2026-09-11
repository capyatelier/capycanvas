// Browser file transport; document checkpoints, stale-edit guards and unsaved
// decisions stay in UiSession. File handles never enter a project or localStorage.
export function createDocuments({app,dispatch,applyChange,wake,element,button,message,gpuOperation}) {
  const active=new Set(),handles=new Map();
  let nextHandle=0,closing=false;
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
  async function newDocument() {
    const spec=app.editor_models(innerWidth,innerHeight).document_options;
    return dialog(spec.new_title,(form,finish)=>{
      const fields=spec.extent.map((value,i)=>{
        const label=element("label","document-size",i?spec.height_label:spec.width_label),input=element("input");
        Object.assign(input,{type:"number",min:1,max:spec.max_dimension,step:1,value,required:true});
        input.setAttribute("aria-label",label.textContent);label.append(input);form.append(label);return input;
      });
      const footer=element("footer");cancel(footer,finish);const create=button("Create",()=>{if(form.reportValidity())finish(fields.map(i=>Number(i.value)));},"suggested-action");
      footer.append(create);form.append(footer);
      form.onsubmit=e=>{e.preventDefault();create.click();};
    });
  }
  function chooseFile() {
    if(window.showOpenFilePicker) return window.showOpenFilePicker({multiple:false,types:[{description:"Capy Canvas drawing",accept:{"application/octet-stream":[".capy"]}}]})
      .then(async([handle])=>({file:await handle.getFile(),handle}));
    return new Promise(resolve=>{
      const input=element("input");input.type="file";input.accept=".capy";input.hidden=true;document.body.append(input);
      const done=value=>{input.remove();resolve(value);};
      input.onchange=()=>done(input.files[0]?{file:input.files[0]}:null);input.oncancel=()=>done(null);input.click();
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
  async function destination(request) {
    const old=request.location && handles.get(request.location.uri);
    if(old)return{location:request.location,handle:old};
    if(window.showSaveFilePicker) {
      const png=request.type==="export",handle=await window.showSaveFilePicker({
        suggestedName:request.name,types:[{description:png?"PNG image":"Capy Canvas drawing",accept:{[png?"image/png":"application/octet-stream"]:[png?".png":".capy"]}}]});
      return{location:location(handle.name,handle),handle};
    }
    return {location:location(request.name)};
  }
  async function handle(request) {
    if(active.has(request.id))return;active.add(request.id);
    let candidate;
    try {
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
      if(r.type==="new"||r.type==="open") {
        const fileState=app.state().document_file;let bytes,extent=[0,0],target=null;
        if(r.type==="new") {extent=await newDocument();if(!extent){applyChange(app.finish_document(id,false));return;}}
        else {const chosen=await chooseFile();if(!chosen){applyChange(app.finish_document(id,false));return;}
          bytes=new Uint8Array(await chosen.file.arrayBuffer());target=location(chosen.file.name,chosen.handle);}
        message("Preparing drawing…");
        candidate=await gpuOperation(()=>app.prepare_document(id,bytes,...extent,fileState.epoch,fileState.revision));
        applyChange(app.adopt_document(candidate,target));candidate=null;message("");wake();
      } else if(r.type==="save"||r.type==="export") {
        // Invoke the picker before awaiting work to retain browser user activation.
        const target=await destination(r);
        if(r.type==="export"){
          const started=performance.now();wake();
          await new Promise(requestAnimationFrame);
          while(!app.export_ready()){
            if(performance.now()-started>60000)throw new Error("Export shader preparation timed out");
            wake();await new Promise(resolve=>setTimeout(resolve,16));
          }
        }
        const bytes=r.type==="save"?app.save_project(id,target.location):await gpuOperation(()=>app.export_png(id));
        let success;
        if(target.handle) {
          const stream=await target.handle.createWritable();
          try {await stream.write(bytes);await stream.close();success=true;}
          catch(error){try{await stream.abort();}catch{}throw error;}
        } else success=!!await download(bytes,target.location.name,r.type==="export"?"image/png":"application/octet-stream");
        applyChange(app.finish_document(id,success));
      } else throw new Error(`Unknown document operation: ${r.type}`);
    } catch(error) {
      if(request.kind.type==="document") {
        const cancelled=error?.name==="AbortError";applyChange(app.finish_document(request.id,false,cancelled?undefined:String(error)));
      } else dispatch({type:"complete_request",id:request.id,error:String(error)});
    } finally {candidate?.free();active.delete(request.id);pruneHandles();}
  }
  window.addEventListener("beforeunload",e=>{if(app.state().document_file.modified){e.preventDefault();e.returnValue="";}});
  return {handle,refresh(){
    if(closing || !app.state().document_file.close_ready)return;
    closing=true;
    const current=app.state().document_file,extent=app.editor_models(innerWidth,innerHeight).document_options.extent;
    // Close leaves an empty untitled workspace after the shared unsaved decision.
    gpuOperation(()=>app.prepare_document(0,undefined,...extent,current.epoch,current.revision))
      .then(candidate=>{applyChange(app.adopt_document(candidate,null));wake();})
      .catch(error=>{app.reset_document_close();message(error);})
      .finally(()=>{closing=false;pruneHandles();});
  }};
}
