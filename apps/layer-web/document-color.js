import { liveCopy, bindCopy } from './localization.js';
import {createProfilePicker} from './export-controls.js';
import {createPageKit} from './page-kit.js';
// A comparison owns one immutable candidate. Cancel drains its work before the
// document request is released; Apply publishes that exact prepared result.
export async function chooseDocumentColor({app,dialog,element,button,icon,dropdown,gpuOperation,request,id}) {
  const copy=liveCopy(app,"document_color_copy"),exportCopy=liveCopy(app,"export_copy"),comparisonCopy=liveCopy(app,"comparison_copy"),kit=createPageKit({element,icon});
  const history=request.type==="color_history",source=["repair_source_profile","rasterize_source"].includes(request.type),rasterize=request.type==="rasterize_source";
  const operation=source?(rasterize?"rasterize":"repair"):request.operation,current=app.document_color();
  const sourceContext=source?app.source_dialog_context(id):null;
  let sourceName=sourceContext?.name??null,profile;
  let candidate,control,running,closed=false,accepted=false,pending=false;
  try {
    await dialog(()=>history?copy.preparing_title:{assign:copy.assign_title,convert:copy.convert_title,depth:copy.depth_title,repair:copy.repair_title,rasterize:copy.rasterize_title}[operation],(form,finish)=>{
      form.classList.add('document-color-content');
      const group=kit.group();
      const select=(title,choices,value,subtitle=false)=>{
        const node=dropdown(title,()=>schedule());node.update({choices:choices.map(([value,label])=>({value,label})),value});
        group.list.append(kit.comboRow({title,control:node,subtitle}));return node;
      };
      if(!history)form.append(element('p','document-color-help',()=>rasterize?app.rasterize_details():source?sourceContext.baked?copy.source_baked_help:copy.source_native_help:operation==='assign'?copy.assign_native_help:operation==='depth'?copy.depth_native_help:copy.convert_native_help));
      const original=source&&!rasterize?kit.row({title:()=>copy.current_source,description:()=>app.source_description(sourceName,sourceContext.assumed)}):null;
      if(original){original.line.classList.add('source-current');group.list.append(original.line);}
      const spaces=history||source||operation==='depth'?[]:app.profile_picker_standard({kind:'output'}).map(profile=>[profile.profile.Builtin,()=>app.profile_name_copy(profile.name)]);
      const space=!history&&!source&&operation!=='depth'?select(()=>exportCopy.profile,spaces,current.space):null;
      const depth=operation==='depth'?select(()=>exportCopy.depth,[['U8',()=>copy.depth_8],['U16',()=>copy.depth_16],['F16',()=>copy.depth_float16],['F32',()=>copy.depth_float32]],app.document_color_suggested_depth(current.depth)):null;
      const dither=operation==='depth'?element('input','settings-switch'):null;
      if(dither){dither.type='checkbox';dither.setAttribute('role','switch');bindCopy(dither,()=>exportCopy.reduce_banding,'ariaLabel');dither.onchange=()=>schedule();group.list.append(kit.row({title:()=>exportCopy.reduce_banding,description:()=>exportCopy.dither_gradients,suffix:dither}).line);}
      const result=operation==='convert'?select(()=>copy.result,[['layers',()=>copy.convert_layers],['copy',()=>copy.create_flattened]],'layers',true):null;
      const intent=operation==='convert'?select(()=>exportCopy.intent,[['RelativeColorimetric',()=>exportCopy.relative],['Perceptual',()=>exportCopy.perceptual],['Saturation',()=>exportCopy.saturation],['AbsoluteColorimetric',()=>exportCopy.absolute]],'RelativeColorimetric',true):null;
      if(!history&&!rasterize)form.append(group.section);
      let phase="empty",failure=null,presentation=null;
      const detail=element("p","document-color-detail"),status=element("p",history?"alert-description":""),comparison=element("div","color-comparison native-comparison"),footer=kit.footer();
      comparison.style.setProperty("--comparison-width",`${comparisonCopy.width}px`);comparison.style.setProperty("--comparison-gap",`${comparisonCopy.gap}px`);comparison.style.setProperty("--comparison-height",`${comparisonCopy.height}px`);
      const applyLabel=()=>source?(rasterize?copy.rasterize:sourceContext.baked?copy.add_source:copy.apply_profile):result?.value==='copy'?copy.create_copy:copy.common.apply;
      const apply=button(applyLabel,()=>{if(closed||!candidate||running||pending)return;accepted=true;closed=true;finish(true);},"suggested-action");apply.disabled=true;
      const cancel=button(()=>copy.common.cancel,()=>{if(closed)return;closed=true;pending=false;control?.cancel();finish(null);});
      const statusLabel=()=>failure?app.color_feature_error_copy(failure,true):phase==='preparing'?history?copy.preparing_help:rasterize?copy.rasterizing:copy.preparing_comparison:phase==='unchanged'?copy.choose_different:'';
      const detailLabel=()=>history?'':rasterize?presentation?presentation.clipped?copy.source_clipped:copy.rasterize_compare:copy.rasterize_comparison:source?'':presentation?presentation.clipped?copy.clipped_comparison:presentation.isCopy?copy.layered_open:copy.compare_before_apply:copy.choose_destination;
      const present=()=>{bindCopy(status,statusLabel);status.hidden=!statusLabel();bindCopy(detail,detailLabel);detail.hidden=!detailLabel();bindCopy(apply,applyLabel);};
      const fail=error=>{failure=error;present();};
      const invalidate=()=>{candidate?.free();candidate=null;presentation=null;failure=null;phase='draft';apply.disabled=true;comparison.replaceChildren();present();};
      if(original){profile=createProfilePicker({app,element,button,icon,dropdown,label:()=>copy.correct_profile,purpose:sourceContext.purpose,context:{kind:'request',id},documentLabel:()=>app.source_profile_name_copy(sourceName),change:state=>{if(state.pending||state.error){invalidate();pending=false;control?.cancel();if(state.error)fail(state.error);}else schedule();},onError:fail});group.list.append(kit.comboRow({title:()=>copy.correct_profile,control:profile}));}
      const schedule=()=>{if(closed)return;if(dither)dither.disabled=!app.document_color_dither(depth.value,dither.checked).available;invalidate();pending=true;control?.cancel();if(!running)queueMicrotask(prepare);};
      const prepare=()=>{
        if(running||closed||!pending)return;pending=false;invalidate();control?.free();control=app.capture_control();
        phase="preparing";present();
        const flattened=result?.value==="copy";
        const choice=source?(rasterize?null:profile.profileValue()?.profile??null):history?null:operation==="assign"?{Assign:space.value}:operation==="depth"?{Depth:{depth:depth.value,dither:app.document_color_dither(depth.value,dither.checked).value}}:{Convert:{space:space.value,options:{intent:intent.value,black_point_compensation:false}}};
        if(!history&&!source&&!app.color_comparison_needed(choice,flattened)){phase='unchanged';present();return;}
        running=(async()=>{
          try {
            const next=await gpuOperation(async()=>{
              if(!source)return app.prepare_color(id,choice,control,flattened);
              const prepared=await app.prepare_source(id,choice,control);
              return app.prepare_source_comparison(prepared);
            });
            if(closed||pending||control.cancelled()){next.free();return;}
            candidate=next;presentation={clipped:candidate.clipped_channels()>0,isCopy:!source&&candidate.is_copy()};if(original){sourceName=candidate.source_profile();bindCopy(original.text.querySelector('p'),()=>app.source_description(sourceName,sourceContext.assumed));profile.refreshProfileCaption();}phase="complete";
            if(history){accepted=true;closed=true;finish(true);return;}
            candidate.previews().forEach((image,index)=>{
              const figure=element("figure"),canvas=element("canvas"),caption=element("figcaption","",()=>index?comparisonCopy.copy.after:comparisonCopy.copy.before);
              [canvas.width,canvas.height]=image.extent;
              canvas.getContext("2d",{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);
              bindCopy(canvas,()=>index?comparisonCopy.copy.after:comparisonCopy.copy.before,"ariaLabel");figure.append(caption,canvas);comparison.append(figure);
            });
            present();
            apply.disabled=false;
          }catch(error){const wasCancelled=control.cancelled();control.cancel();if(!closed&&!pending&&!wasCancelled)fail(error);}
          finally{running=null;if(!closed){if(phase==="preparing"){phase="draft";present();}if(pending)queueMicrotask(prepare);}}
        })();
      };
      footer.append(cancel);if(!history)footer.append(apply);
      form.localize=present;if(!history)form.append(detail,comparison);form.append(status,footer);form.onsubmit=e=>e.preventDefault();
      if(dither)dither.disabled=!app.document_color_dither(depth.value,dither.checked).available;pending=true;queueMicrotask(prepare);return {cancel,primary:history?null:apply};
    },history?copy.progress_dialog:copy.dialog);
    closed=true;pending=false;profile?.disposeProfilePicker();if(!accepted)control?.cancel();await running;
    return accepted?candidate:null;
  } finally {
    closed=true;pending=false;profile?.disposeProfilePicker();if(!accepted)control?.cancel();
    try {await running;}finally{if(!accepted)candidate?.free();control?.free();}
  }
}
