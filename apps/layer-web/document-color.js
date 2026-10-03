import { liveCopy, bindCopy } from './localization.js';
import {importProfile,chooseProfileLibrary} from './export-controls.js';
// A comparison owns one immutable candidate. Cancel drains its work before the
// document request is released; Apply publishes that exact prepared result.
export async function chooseDocumentColor({app,dialog,element,button,gpuOperation,request,id}) {
  const copy=liveCopy(app,"document_color_copy"),exportCopy=liveCopy(app,"export_copy"),profilesCopy=liveCopy(app,"profile_copy");
  const history=request.type==="color_history",source=["repair_source_profile","rasterize_source"].includes(request.type),rasterize=request.type==="rasterize_source";
  const operation=source?(rasterize?"rasterize":"repair"):request.operation,current=app.document_color();
  const profiles=["Srgb","DisplayP3","AdobeRgb","ProPhoto"].map(Builtin=>({Builtin}));
  let candidate,control,running,closed=false,accepted=false;
  try {
    await dialog(()=>history?(request.redo?copy.redo_title:copy.undo_title):{assign:copy.assign_title,convert:copy.convert_title,depth:copy.depth_title,repair:copy.repair_title,rasterize:copy.rasterize_title}[operation],(form,finish)=>{
      const inputs=[];
      const select=(title,choices,value)=>{
        const label=element("label","document-size",title),node=element("select");if(typeof title==="function")bindCopy(node,title,"ariaLabel");else node.setAttribute("aria-label",title);
        for(const [id,name] of choices){const option=element("option","",name);option.value=id;node.append(option);}node.value=value;
        label.append(node);form.append(label);inputs.push(node);return node;
      };
      if(!history&&!source)form.append(element("p","",()=>operation==="assign"?copy.assign_help:operation==="depth"?copy.depth_help:copy.convert_help));
      if(source)form.append(element("p","",()=>rasterize?copy.rasterize_help:copy.repair_help));
      const profile=source&&!rasterize?select(()=>copy.correct_profile,[["0","sRGB"],["1","Display P3"],["2","Adobe RGB (1998)"],["3","ProPhoto RGB"]],"0"):null;
      const space=!history&&!source&&operation!=="depth"?select(()=>copy.space,[["Srgb","sRGB"],["DisplayP3","Display P3"],["AdobeRgb","Adobe RGB"],["ProPhoto","ProPhoto RGB"]],current.space):null;
      const depth=operation==="depth"?select(()=>copy.depth,[["U8",()=>copy.depth_8],["U16",()=>copy.depth_16],["F16",()=>copy.depth_float16],["F32",()=>copy.depth_float32]],current.depth):null;
      const dither=operation==="depth"?select(()=>copy.dither,[["None",()=>exportCopy.metadata_none],["Stochastic8",()=>copy.dither_stochastic]],"None"):null;
      const result=operation==="convert"?select(()=>copy.result,[["layers",()=>copy.editable_layers],["copy",()=>copy.flattened_copy]],"layers"):null;
      const intent=operation==="convert"?select(()=>exportCopy.intent,[["RelativeColorimetric",()=>exportCopy.relative],["Perceptual",()=>exportCopy.perceptual],["Saturation",()=>exportCopy.saturation],["AbsoluteColorimetric",()=>exportCopy.absolute]],"RelativeColorimetric"):null;
      let phase="empty",failure=null,presentation=null;
      const status=element("p"),comparison=element("div","color-comparison"),footer=element("footer");
      const applyLabel=()=>source?(rasterize?copy.rasterize:presentation?.addsLayer?copy.add_source:copy.apply_profile):presentation?.isCopy?copy.save_copy:copy.common.apply;
      const apply=button(applyLabel,()=>{accepted=true;finish(true);},"suggested-action");apply.disabled=true;
      const cancel=button(()=>copy.common.cancel,()=>{control?.cancel();finish(null);});
      const statusLabel=()=>{if(failure)return app.color_feature_error_copy(failure,true);if(phase==="preparing")return copy.preparing;if(phase==="empty")return "";if(!presentation)return copy.preview_help;const preview=presentation.clipped?copy.outside_gamut:copy.composition_preview;return source?app.color_source_preview(app.source_profile_name_copy(presentation.sourceProfile),preview,presentation.addsLayer):preview;};
      const present=()=>{bindCopy(status,statusLabel);bindCopy(apply,applyLabel);};
      const fail=error=>{failure=error;present();};
      const invalidate=()=>{candidate?.free();candidate=null;presentation=null;failure=null;phase="draft";apply.disabled=true;comparison.replaceChildren();present();};
      if(profile){const load=button(()=>profilesCopy.import,async()=>{try{const imported=await importProfile(app,element);if(!imported)return;profiles.push(imported.profile);const option=element("option","",()=>app.profile_name_copy(imported.name));option.value=profiles.length-1;profile.append(option);profile.value=option.value;invalidate();}catch(error){fail(error);}});const saved=button(()=>profilesCopy.saved_dialog,async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;profiles.push(imported.profile);const option=element("option","",()=>app.profile_name_copy(imported.name));option.value=profiles.length-1;profile.append(option);profile.value=option.value;invalidate();}catch(error){fail(error);}});inputs.push(load,saved);form.append(load,saved);}
      inputs.forEach(node=>node.onchange=invalidate);
      const prepare=()=>{
        if(running)return;invalidate();control?.free();control=app.capture_control();
        inputs.forEach(node=>node.disabled=true);preview.disabled=true;phase="preparing";present();
        const choice=source?(rasterize?null:profiles[Number(profile.value)]):history?null:operation==="assign"?{Assign:space.value}:operation==="depth"?{Depth:{depth:depth.value,dither:depth.value==="U8"?dither.value:"None"}}:{Convert:{space:space.value,options:{intent:intent.value,black_point_compensation:false}}};
        running=(async()=>{
          try {
            const next=await gpuOperation(async()=>{
              if(!source)return app.prepare_color(id,choice,control,result?.value==="copy");
              const prepared=await app.prepare_source(id,choice,control);
              return app.prepare_source_comparison(prepared);
            });
            if(closed||control.cancelled()){next.free();return;}
            candidate=next;presentation={clipped:candidate.clipped_channels()>0,isCopy:!source&&candidate.is_copy(),addsLayer:source&&candidate.adds_layer(),sourceProfile:source?candidate.source_profile():null};phase="complete";
            if(history){accepted=true;finish(true);return;}
            candidate.previews().forEach((image,index)=>{
              const figure=element("figure"),canvas=element("canvas"),caption=element("figcaption","",()=>index?copy.after:copy.before);
              [canvas.width,canvas.height]=image.extent;
              canvas.getContext("2d",{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);
              bindCopy(canvas,()=>index?copy.prepared_composition:copy.original_composition,"ariaLabel");figure.append(canvas,caption);comparison.append(figure);
            });
            present();
            apply.disabled=false;
          }catch(error){const wasCancelled=control.cancelled();control.cancel();if(!closed&&!wasCancelled)fail(error);}
          finally{running=null;if(!closed){if(phase==="preparing"){phase="draft";present();}inputs.forEach(node=>node.disabled=false);preview.disabled=false;}}
        })();
      };
      const preview=button(()=>copy.preview,prepare);
      footer.append(cancel);if(!history)footer.append(preview,apply);
      form.localize=present;form.append(comparison,status,footer);form.onsubmit=e=>e.preventDefault();
      if(history)queueMicrotask(prepare);
    });
    closed=true;if(!accepted)control?.cancel();await running;
    if(!accepted){candidate?.free();candidate=null;}
    return candidate;
  } finally {control?.free();}
}
