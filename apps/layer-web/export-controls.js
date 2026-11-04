import {createDialogHost} from './dialog-host.js';
import { liveCopy, bindCopy } from './localization.js';
// The shared recipe describes a delivery copy, independent of the master.
const profileLibraries=new Set();
export function localizeProfileLibraries(){for(const render of profileLibraries)render();}
export const SDR_FORMATS=["Png","Tiff","Jpeg","Webp"];
export async function chooseExport({app,dialog,element,button,icon,dropdown,numberField,gpuOperation,id}) {
  let control,running,closed=false;
  const model=app.export_form(),copy=liveCopy(app,"export_copy"),profilesCopy=liveCopy(app,"profile_copy");
  let page="main";
  let library=await app.export_presets({type:"get",index:0});
  const localizeLibrary=()=>{library.names=app.localize_export_presets(library.names);};localizeLibrary();
  try { const result=await dialog(()=>({size:copy.image_size,color:copy.color_transparency,presets:copy.presets}[page]??copy.title),(form,finish,{caption,layout,dismiss})=>{
    form.parentNode.id="export-options";
    let recipe=library.recipe,currentPreset=0;const choiceSets=new Map();
    const field=(label,node)=>{const root=element("label","document-size",label);if(typeof label==="function")bindCopy(node,label,"ariaLabel");else node.setAttribute("aria-label",label);root.append(node);form.append(root);return node;};
    const select=(label,choices,change)=>{const node=dropdown(label,value=>{if(node!==previewMode)invalidate();change?.(value);}),items=choices.map(([value,label])=>({value,label}));choiceSets.set(node,items);node.update({choices:items,value:items[0]?.value});return field(label,node);};
    const number=(label,value,control)=>{
      const node=numberField(control,label,next=>{node.numericValue=next;});node.numericValue=value;node.update(value);return field(label,node);
    };
    const setNumber=(node,value)=>{node.cancelEditing();node.numericValue=value;node.update(value);};
    form.append(element("p","",()=>copy.help));
    const destination=select(()=>copy.destination,library.names.map((name,i)=>[i,()=>library.names[i]]),value=>preference({type:"get",index:Number(value)}));
    const range=select(()=>copy.range,["F16","F32"].includes(app.document_color().depth)?[["sdr",()=>copy.sdr_rendition],["jpeg",()=>copy.jpeg_gainmap],["avif",()=>copy.avif_gainmap],["hdr",()=>copy.format_pq],["exr",()=>copy.format_exr]]:[["sdr","SDR"]],()=>updateDraft({type:"format",value:rangeFormat()}));
    const clip=field(()=>copy.clip_hdr,element("input","panel-check"));clip.type="checkbox";
    const format=select(()=>copy.format,SDR_FORMATS.map(id=>[id,()=>({Png:copy.format_png,Tiff:copy.format_tiff,Jpeg:copy.format_jpeg,Webp:copy.format_webp})[id]]),value=>updateDraft({type:"format",value}));
    const profileChoices=()=>model.profiles.map((p,value)=>({value,label:()=>model.profile_captions?.[value]?app.export_profile_caption_copy(model.profile_captions[value]):app.profile_name_copy(p.name)}));
    const profile=select(()=>copy.profile,[],value=>updateDraft({type:"profile",value:model.profiles[Number(value)]}));
    const depth=select(()=>copy.depth,[["U8",()=>copy.depth_8],["U16",()=>copy.depth_16],["F32",()=>copy.depth_float32]],value=>updateDraft({type:"depth",value}));
    const background=select(()=>copy.transparency,[["Preserve",()=>copy.preserve],["White",()=>copy.white_background],["Black",()=>copy.black_background]],value=>updateDraft({type:"background",value}));
    const intent=select(()=>copy.intent,[["RelativeColorimetric",()=>copy.relative],["Perceptual",()=>copy.perceptual],["Saturation",()=>copy.saturation],["AbsoluteColorimetric",()=>copy.absolute]]);
    const dither=select(()=>copy.dither,[["None",()=>copy.dither_none],["Stochastic8",()=>copy.dither_stochastic]]);
    const quality=number(()=>copy.quality,90,model.numeric.quality);
    const size=select(()=>copy.pixel_size,[["Original",()=>copy.original_size],["Fit",()=>copy.fit_bounds]],()=>visible());
    const width=number(()=>copy.maximum_width,2048,model.numeric.dimension),height=number(()=>copy.maximum_height,2048,model.numeric.dimension);
    const enlarge=field(()=>copy.enlarge,element("input","panel-check"));enlarge.type="checkbox";
    const resolution=select(()=>copy.resolution,[["Master",()=>copy.keep_resolution],["Ppi",()=>copy.ppi],["Omit",()=>copy.omit]],()=>visible()),ppi=number(()=>copy.ppi,300,model.numeric.ppi);
    let metadataView=app.export_draft(recipe,{type:"refresh"}).metadata;
    const metadata=select(()=>metadataView.label,metadataView.choices.map(c=>[c.value,()=>metadataView.choices.find(choice=>choice.value===c.value).label]),()=>updateDraft({type:"metadata",value:readMetadata()}));
    const removeLocation=field(()=>metadataView.remove_location,element("input","panel-check"));removeLocation.type="checkbox";
    const metadataNote=element("p","export-metadata-note");form.append(metadataNote);
    const readMetadata=()=>({keep:metadata.value,remove_location:removeLocation.checked});
    const rangeFormat=()=>range.value==="exr"?"Exr":range.value==="sdr"?format.value:({hdr:"PngHdr",jpeg:"JpegHdr",avif:"AvifHdr"}[range.value]+(clip.checked?"Mapped":""));
    const visible=()=>{const hdr=range.value!=="sdr";clip.closest("label").hidden=!["hdr","jpeg","avif"].includes(range.value);for(const n of[format,profile,depth,intent,dither])n.closest("label").hidden=hdr;background.closest("label").hidden=hdr&&range.value!=="jpeg";quality.closest("label").hidden=!["jpeg","avif"].includes(range.value)&&!(range.value==="sdr"&&format.value==="Jpeg");for(const f of[width,height,enlarge])f.closest("label").hidden=size.value!=="Fit";ppi.closest("label").hidden=resolution.value!=="Ppi";
      metadata.closest("label").hidden=!model.metadata||!metadataView.available;removeLocation.closest("label").hidden=!model.metadata||!metadataView.location;
      bindCopy(metadataNote,()=>metadataView.note??"");metadataNote.hidden=!model.metadata||!metadataView.note;};
    const load=()=>{
      if(!model.profiles.some(p=>p.name===recipe.profile.name&&JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile)))model.profiles.push(recipe.profile);
      range.update({value:recipe.format==="Exr"?"exr":recipe.format.startsWith("PngHdr")?"hdr":recipe.format.startsWith("JpegHdr")?"jpeg":recipe.format.startsWith("AvifHdr")?"avif":"sdr"});clip.checked=recipe.format.endsWith("Mapped");format.update({value:range.value!=="sdr"?"Png":recipe.format});profile.update({choices:profileChoices(),value:Math.max(0,model.profiles.findIndex(p=>p.name===recipe.profile.name&&JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile)))});depth.update({value:recipe.depth});background.update({value:recipe.background});
      intent.update({value:recipe.encoding.conversion.intent});dither.update({value:recipe.encoding.dither});setNumber(quality,recipe.jpeg_quality);size.update({value:recipe.size.Fit?"Fit":"Original"});
      enlarge.checked=recipe.size.Fit?.enlarge??false;if(recipe.size.Fit){setNumber(width,recipe.size.Fit.bounds[0]);setNumber(height,recipe.size.Fit.bounds[1]);};resolution.update({value:recipe.resolution.Ppi?"Ppi":recipe.resolution});if(recipe.resolution.Ppi)setNumber(ppi,recipe.resolution.Ppi);
      metadata.update({value:recipe.metadata.keep});removeLocation.checked=recipe.metadata.remove_location;visible();
    };
    const updateDraft=action=>{
      const draft=app.export_draft(readRecipe(),action);recipe=draft.recipe;metadataView=draft.metadata;
      for(const[node,allowed]of[[format,draft.formats],[depth,draft.depths],[background,draft.backgrounds],[dither,draft.dithers]]) {
        node.update({choices:choiceSets.get(node).map(choice=>({...choice,enabled:allowed.includes(String(choice.value))}))});
      }
      load();
    };
    clip.onchange=()=>updateDraft({type:"format",value:rangeFormat()});
    removeLocation.onchange=()=>updateDraft({type:"metadata",value:readMetadata()});load();
    let failure=null;const error=element("p","error-message"),fail=(value,profile)=>{failure=value;bindCopy(error,()=>failure==null?"":app.color_feature_error_copy(failure,profile));};form.append(error);
    form.append(button(()=>profilesCopy.import,async()=>{
      try {const imported=await importProfile(app,element);if(!imported)return;model.profiles.push(imported);profile.update({choices:profileChoices(),value:model.profiles.length-1});updateDraft({type:"profile",value:imported});fail(null);}
      catch(e){fail(e,true);}
    }));
    form.append(button(()=>profilesCopy.saved_dialog,async()=>{try{const imported=await chooseProfileLibrary({app,element,button,icon});if(!imported)return;model.profiles.push(imported);profile.update({choices:profileChoices(),value:model.profiles.length-1});updateDraft({type:"profile",value:imported});invalidate();}catch(e){fail(e,true);}}));
    const readRecipe=()=>({format:rangeFormat(),profile:model.profiles[Number(profile.value)],depth:depth.value,background:background.value,
      encoding:{conversion:{intent:intent.value,black_point_compensation:false},dither:dither.value},jpeg_quality:quality.numericValue,
      size:size.value==="Original"?"Original":{Fit:{bounds:[width.numericValue,height.numericValue],enlarge:enlarge.checked}},
      resolution:resolution.value==="Ppi"?{Ppi:ppi.numericValue}:resolution.value,metadata:readMetadata()});
    const selected=()=>app.export_validate(app.export_draft(readRecipe(),{type:"refresh"}).recipe);
    updateDraft({type:"refresh"});
    const presetName=element("input");presetName.maxLength=80;
    const presetButtons=element("div","document-size");
    const presetAction=type=>{if(!commitNumbers()||!form.reportValidity())return;try{preference(type==="save"?{type,name:presetName.value,recipe:selected()}:{type,index:Number(destination.value),recipe:selected()});}catch(e){fail(e);}};
    const savePreset=button(()=>copy.save_new_preset,async()=>{
      presetName.value='';
      const name=await dialog(()=>copy.save_preset_title,(form,finish)=>{
        form.parentNode.id='export-preset-name-dialog';presetName.id='export-preset-name';
        const label=element('label','document-size',()=>copy.preset_name);label.append(presetName);
        const cancel=button(()=>copy.common.cancel,()=>finish(null)),save=button(()=>copy.common.save,()=>finish(presetName.value),'suggested-action');
        const footer=element('footer');footer.append(cancel,save);form.append(label,footer);
        const validate=()=>{save.disabled=!app.export_preset_name_valid(presetName.value);};presetName.oninput=validate;validate();
        form.onsubmit=event=>{event.preventDefault();if(!save.disabled)save.click();};
        return {cancel,primary:save};
      },library.dialog);
      if(name!=null){presetName.value=name;presetAction('save');}
    });
    const updatePreset=button(()=>copy.update_preset,()=>presetAction("update"));
    const removePreset=button(()=>copy.delete_preset,()=>preference({type:"remove",index:Number(destination.value)}));
    const resetPreset=button(()=>copy.reset_destination,()=>preference({type:"reset",index:Number(destination.value)}));
    const presetAvailability=()=>{updatePreset.disabled=removePreset.disabled=Number(destination.value)<4;resetPreset.disabled=Number(destination.value)>=4;};
    presetButtons.append(savePreset,updatePreset,removePreset,resetPreset);form.append(presetButtons);presetAvailability();
    let preferenceBusy=false;
    async function preference(action){
      if(preferenceBusy||running)return;preferenceBusy=true;const inputs=[...form.querySelectorAll('input,select,button')];inputs.forEach(n=>n.disabled=true);
      try{library=await app.export_presets(action);localizeLibrary();currentPreset=library.index??0;destination.update({choices:library.names.map((name,value)=>({value,label:()=>library.names[value]})),value:currentPreset});if(library.recipe){recipe=library.recipe;load();updateDraft({type:"refresh"});}invalidate();fail(null);}
      catch(e){destination.update({value:currentPreset});fail(e,false);}finally{preferenceBusy=false;inputs.forEach(n=>n.disabled=false);presetAvailability();}
    }
    const comparison=element("div","color-comparison"),status=element("p"),footer=element("footer");
    const previewMode=select(()=>copy.preview_rendition,[["hdr",()=>copy.hdr_preview],["sdr",()=>copy.sdr_base]],()=>{if(completed)drawComparison();});previewMode.closest("label").hidden=true;
    let rangeBlocked=false,completed,completedRecipe,phase="idle";
    const presentStatus=()=>bindCopy(status,()=>phase==="preparing"?copy.preparing_comparison:completed?app.export_preview_copy(completedRecipe.format,Boolean(completed.sdr_preview),completed.clipped_channels>0):"");
    const invalidate=()=>{completed=null;comparison.replaceChildren();previewMode.closest("label").hidden=true;phase="idle";presentStatus();rangeBlocked=false;choose.disabled=false;};
    const invalidateInput=e=>{if(e.target!==previewMode)invalidate();};
    form.addEventListener("input",invalidateInput,true);form.addEventListener("change",invalidateInput,true);
    const drawComparison=()=>{
      const output=completed;comparison.replaceChildren();
      const images=[output.previews[0],output.sdr_preview&&previewMode.value==="sdr"?output.sdr_preview:output.previews[1]];
      images.forEach((image,index)=>{const figure=element("figure"),canvas=element("canvas");[canvas.width,canvas.height]=image.extent;
        // These bounded previews already arrive as CPU pixels. Keep their 2D
        // storage on CPU, including while the scrollable comparison is offscreen.
        canvas.getContext("2d",{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);bindCopy(canvas,()=>index?copy.output_preview:copy.artwork_preview,"ariaLabel");
        const caption=()=>index?(output.sdr_preview?(previewMode.value==="sdr"?copy.sdr_base:copy.hdr_preview):copy.output):copy.artwork;
        figure.append(canvas,element("figcaption","",caption));comparison.append(figure);});
    };
    const cancel=button(()=>copy.common.cancel,dismiss);
    const commitNumbers=()=>[quality,width,height,ppi].filter(node=>!node.closest("label").hidden).every(node=>node.commit());
    const choose=button(()=>copy.choose_file,()=>{if(!commitNumbers()||!form.reportValidity())return;try{finish({recipe:selected(),destination:Number(destination.value)});}catch(e){fail(e);}},"suggested-action");
    const preview=button(()=>copy.preview,()=>{
      if(running||!commitNumbers()||!form.reportValidity())return;
      let recipe;try{recipe=selected();}catch(e){fail(e);return;}
      invalidate();control?.free();control=app.capture_control();phase="preparing";presentStatus();fail(null);
      const inputs=[...form.querySelectorAll('input,select,button')].filter(node=>node!==cancel);inputs.forEach(node=>node.disabled=true);
      running=(async()=>{
        try{
          const output=await gpuOperation(()=>app.export_image(id,recipe,control,true));
          if(closed||control.cancelled())return;
          completed=output;completedRecipe=recipe;phase="complete";previewMode.closest("label").hidden=!output.sdr_preview;drawComparison();
          rangeBlocked=["PngHdr","JpegHdr","AvifHdr"].includes(recipe.format)&&output.clipped_channels>0;
          presentStatus();
        }catch(e){if(!closed&&!control.cancelled())fail(e);}
        finally{running=null;if(!closed){if(!completed){phase="idle";presentStatus();}inputs.forEach(node=>node.disabled=false);choose.disabled=rangeBlocked;presetAvailability();}}
      })();
    });
    form.localize=()=>{localizeLibrary();metadataView=app.export_metadata_copy(recipe.format,recipe.metadata.keep);};
    footer.append(cancel,preview,choose);form.append(comparison,status,footer);form.onsubmit=e=>e.preventDefault();
    const panes=new Map(['main','size','color','presets'].map(id=>[id,element('section','document-page')]));
    for(const node of [...form.children])if(node!==footer)panes.get('main').append(node);
    for(const node of [size,width,height,enlarge,resolution,ppi])panes.get('size').append(node.closest('label'));
    for(const node of [profile,depth,background,intent,dither,quality])panes.get('color').append(node.closest('label'));
    panes.get('presets').append(destination.closest('label'));
    panes.get('presets').append(presetButtons);
    const show=id=>{page=id;for(const [key,node]of panes)node.hidden=key!==id;bindCopy(caption,()=>({size:copy.image_size,color:copy.color_transparency,presets:copy.presets}[page]??copy.title));layout();};
    const links=element('div','document-navigation');
    for(const [id,label]of [['size',()=>copy.size],['color',()=>copy.color_transparency],['presets',()=>copy.preset]]){const link=button(label,()=>show(id));link.dataset.exportPage=id;links.append(link);}
    panes.get('main').append(links);form.replaceChildren(...panes.values(),footer);show('main');
    return {cancel,primary:choose,subpage:()=>page!=='main',onBack:()=>show('main'),onCancel:()=>{control?.cancel();finish(null);}};
  },model.dialog);
  closed=true;control?.cancel();await running;return result;
  }finally{control?.free();}

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

export function chooseSourceProfile({app,dialog,element,button,icon,dropdown}) {
  const copy=liveCopy(app,"profile_copy");
  return dialog(()=>copy.interpret_title,(form,finish)=>{
    form.append(element("p","",()=>copy.interpret_help));
    const profiles=["Srgb","DisplayP3","AdobeRgb","ProPhoto"].map(space=>({Builtin:space}));
    const select=dropdown(()=>copy.interpret_as),choices=["sRGB","Display P3","Adobe RGB (1998)","ProPhoto RGB"].map((label,value)=>({value,label}));select.update({choices,value:0});form.append(select);
    const append=imported=>{profiles.push(imported.profile);choices.push({value:profiles.length-1,label:()=>app.profile_name_copy(imported.name)});select.update({choices,value:profiles.length-1});};
    let failure=null;const error=element("p","error-message"),fail=(value,profile)=>{failure=value;bindCopy(error,()=>failure==null?"":app.color_feature_error_copy(failure,profile));};form.append(error);
    form.append(button(()=>copy.import,async()=>{try{const imported=await importProfile(app,element);if(!imported)return;append(imported);fail(null);}catch(e){fail(e,true);}}));
    form.append(button(()=>copy.saved_dialog,async()=>{try{const imported=await chooseProfileLibrary({app,element,button,icon});if(!imported)return;append(imported);}catch(e){fail(e,true);}}));
    const footer=element("footer"),cancel=button(()=>copy.common.cancel,()=>finish(null)),use=button(()=>copy.use_profile,()=>finish(profiles[Number(select.value)]),"suggested-action");footer.append(cancel,use);form.append(footer);return {cancel,primary:use};
  },copy.dialog);
}

export function chooseProfileLibrary({app,element,button,icon,manage=false}) {
  const copy=liveCopy(app,"profile_copy");
  return new Promise(resolve=>{
    const root=element("dialog","document-dialog profile-library"),form=element("form"),list=element("div"),error=element("p","error-message"),search=element("input");
    search.type="search";search.id="profile-library-search";bindCopy(search,()=>copy.search,"placeholder");bindCopy(search,()=>copy.search,"ariaLabel");
    form.method="dialog";let result=null,closed=false,failure=null,busy=false,entries=[];
    const fail=value=>{failure=value;bindCopy(error,()=>failure==null?"":app.color_feature_error_copy(failure,true));};
    const finish=value=>{if(busy&&value==null)return;result=value;shell.hide();};
    root.addEventListener("close",()=>{closed=true;shell.dispose();profileLibraries.delete(render);root.remove();resolve(result);},{once:true});
    const header=element("header","dialog-header");header.append(element("h2","",()=>copy.library_title));
    form.append(element("p","",()=>copy.library_help),search,list,error);
    const done=button(()=>copy.common.done,()=>finish(null));
    const render=()=>{
      search.hidden=entries.length<=7&&!search.value;search.disabled=busy;
      list.replaceChildren();if(!entries.length)list.append(element("p","",()=>copy.empty));
      for(const entry of entries.filter(entry=>app.profile_library_matches(entry.entry_copy_source,search.value))){
        const row=element("section","profile-entry"),actions=element("div","document-size");
        let tag,copyView;const entryCopy=()=>{const current=app.language_tag();if(tag!==current){copyView=app.profile_entry_copy(entry.entry_copy_source);tag=current;}return copyView;};
        row.append(element("h3","",()=>entryCopy().name),element("p","",()=>entryCopy().issue??entryCopy().details));
        if(!manage&&!entry.issue)actions.append(button(()=>copy.use_profile,()=>run(async()=>{const profile=await app.profile_library("get",entry.id);if(!closed)finish(profile);})));
        actions.append(button(()=>entry.visible===false?copy.show:copy.hide,()=>run(async()=>{await app.profile_library(entry.visible===false?"show":"hide",entry.id);await refresh();})));
        actions.append(button(()=>copy.remove,()=>run(async()=>{await app.profile_library("remove",entry.id);await refresh();})));row.append(actions);list.append(row);
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
    const slot=element('div','dialog-header-slot');slot.dataset.dialogSlot='end';slot.append(add);header.append(slot);form.append(done);
    form.onsubmit=e=>e.preventDefault();root.append(header,form);document.body.append(root);
    const shell=createDialogHost({app,dialog:root,header,cancel:done,primary:done,endSlot:slot,icon,copy:copy.common,onCancel:()=>finish(null),search,clearSearch:()=>{search.value="";render();}});shell.present(copy.library_dialog);run(refresh);
  });
}
