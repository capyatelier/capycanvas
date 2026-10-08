import {liveCopy,bindCopy} from './localization.js';
// Browser transport and hit targets only. Rust captures camera coordinates,
// validates layer destinations, prepares the entire batch and owns its history.
export function createImageImport({app,canvas,dispatch,applyChange,wake,element,button,message,gpuOperation,interpret,openClipboard}) {
  const bootstrap=liveCopy(app,"bootstrap_view"),delivery=liveCopy(app,"document_delivery_copy");
  let incoming=null,control=null;
  const command=id=>app.state().commands.find(c=>c.id===id);
  const clear=()=>document.querySelectorAll('.external-image-drop').forEach(row=>row.classList.remove('external-image-drop','layer-drop-before','layer-drop-after','layer-drop-into'));
  const isFiles=e=>e.dataTransfer?.types.includes('Files');
  function hit(e) {
    if(!command('import_image')?.enabled)return null;
    const row=e.target.closest?.('.layer-row');
    if(row) {
      const bounds=row.getBoundingClientRect(),target=BigInt(row.dataset.layer??row.dataset.objectLayer);
      const position=app.image_layer_drop(target,row.dataset.layer?Math.max(0,Math.min(1,(e.clientY-bounds.top)/bounds.height)):.5);
      return position?{row,destination:{target,position},screen:null}:null;
    }
    if(e.target!==canvas)return null;
    const r=canvas.getBoundingClientRect();
    return {destination:null,screen:{x:(e.clientX-r.x)*canvas.width/r.width,y:(e.clientY-r.y)*canvas.height/r.height}};
  }
  document.addEventListener('dragover',e=>{
    if(!isFiles(e))return;
    e.preventDefault();clear();const target=hit(e);e.dataTransfer.dropEffect=target?'copy':'none';
    if(target?.row)target.row.classList.add('external-image-drop',`layer-drop-${{above:'before',below:'after',into:'into'}[target.destination.position]}`);
  });
  document.addEventListener('dragleave',e=>{if(!e.relatedTarget||!e.target.contains?.(e.relatedTarget))clear();});
  document.addEventListener('drop',e=>{
    if(!isFiles(e))return;
    e.preventDefault();clear();const target=hit(e),files=[...e.dataTransfer.files];
    if(!target||!files.length)return;
    incoming={...target,files};
    dispatch({type:'invoke',command:'import_image'});
    // The document request is serviced synchronously by dispatch. Never retain
    // an unclaimed drop for a later picker request.
    incoming=null;
  });
  window.addEventListener('pagehide',()=>control?.cancel());
  return {
    async run(id,choose) {
      const drop=incoming;incoming=null;
      const request=app.capture_image_import(id,drop?.screen??null,drop?.destination??null);
      let prepared,progress;
      try {
        const files=drop?.files??await choose();
        if(!files?.length){applyChange(app.finish_document(id,false));return;}
        control=app.capture_control();
        progress=element('aside','file-progress');progress.setAttribute('role','status');
        let cancelling=false;const caption=()=>cancelling?delivery.cancelling:bootstrap.preparing_document,label=element('span','',caption);
        progress.append(label,button(()=>bootstrap.common.cancel,()=>{control.cancel();cancelling=true;bindCopy(label,caption);}));document.body.append(progress);
        prepared=await gpuOperation(()=>app.prepare_images(request,files,interpret,control));
        const batch=prepared;prepared=null;
        if(app.state().requests.find(r=>r.id===id)?.kind.request.mode==='new_image')await openClipboard(id,batch,control);
        else applyChange(app.adopt_images(batch));
        wake();message('');
      } catch(error) {
        if(control?.cancelled())throw new DOMException('Image import cancelled','AbortError');
        throw error;
      } finally {prepared?.free();request.free();control?.free();control=null;progress?.remove();}
    }
  };
}
