// The shared recipe describes a delivery copy, independent of the master.
export const SDR_FORMATS=["Png","Tiff","Jpeg","Webp"];
export async function chooseExport({app,dialog,element,button,numberField,gpuOperation,id}) {
  let control,running,closed=false;
  const model=app.export_form(),copy=model.copy,profilesCopy=app.profile_copy();
  let library=await app.export_presets({type:"get",index:0});
  try { const result=await dialog(copy.title,(form,finish)=>{
    let recipe=library.recipe,currentPreset=0;
    const field=(label,node)=>{const root=element("label","document-size",label);node.setAttribute("aria-label",label);root.append(node);form.append(root);return node;};
    const select=(label,choices)=>{const node=element("select");for(const[id,name]of choices){const option=element("option","",name);option.value=id;node.append(option);}return field(label,node);};
    const number=(label,value,control)=>{
      const node=numberField(control,label,next=>{node.numericValue=next;});node.numericValue=value;node.update(value);return field(label,node);
    };
    const setNumber=(node,value)=>{node.cancelEditing();node.numericValue=value;node.update(value);};
    form.append(element("p","",copy.help));
    const destination=select(copy.destination,library.names.map((name,i)=>[i,name]));
    const range=select(copy.range,["F16","F32"].includes(app.document_color().depth)?[["sdr",copy.sdr_rendition],["jpeg",copy.jpeg_gainmap],["avif",copy.avif_gainmap],["hdr",copy.format_pq],["exr",copy.format_exr]]:[["sdr","SDR"]]);
    const clip=field(copy.clip_hdr,element("input"));clip.type="checkbox";
    const format=select(copy.format,SDR_FORMATS.map(id=>[id,({Png:copy.format_png,Tiff:copy.format_tiff,Jpeg:copy.format_jpeg,Webp:copy.format_webp})[id]]));
    const profile=select(copy.profile,model.profiles.map((p,i)=>[i,p.name]));
    const depth=select(copy.depth,[["U8",copy.depth_8],["U16",copy.depth_16],["F32",copy.depth_float32]]);
    const background=select(copy.transparency,[["Preserve",copy.preserve],["White",copy.white_background],["Black",copy.black_background]]);
    const intent=select(copy.intent,[["RelativeColorimetric",copy.relative],["Perceptual",copy.perceptual],["Saturation",copy.saturation],["AbsoluteColorimetric",copy.absolute]]);
    const dither=select(copy.dither,[["None",copy.dither_none],["Stochastic8",copy.dither_stochastic]]);
    const quality=number(copy.quality,90,model.numeric.quality);
    const size=select(copy.pixel_size,[["Original",copy.original_size],["Fit",copy.fit_bounds]]);
    const width=number(copy.maximum_width,2048,model.numeric.dimension),height=number(copy.maximum_height,2048,model.numeric.dimension);
    const resolution=select(copy.resolution,[["Master",copy.keep_resolution],["Ppi",copy.ppi],["Omit",copy.omit]]),ppi=number(copy.ppi,300,model.numeric.ppi);
    let metadataView=app.export_draft(recipe,{type:"refresh"}).metadata;
    const metadata=select(metadataView.label,metadataView.choices.map(c=>[c.value,c.label]));
    const removeLocation=field(metadataView.remove_location,element("input"));removeLocation.type="checkbox";
    const metadataNote=element("p","export-metadata-note");form.append(metadataNote);
    const readMetadata=()=>({keep:metadata.value,remove_location:removeLocation.checked});
    const rangeFormat=()=>range.value==="exr"?"Exr":range.value==="sdr"?format.value:({hdr:"PngHdr",jpeg:"JpegHdr",avif:"AvifHdr"}[range.value]+(clip.checked?"Mapped":""));
    const visible=()=>{const hdr=range.value!=="sdr";clip.closest("label").hidden=!["hdr","jpeg","avif"].includes(range.value);for(const n of[format,profile,depth,intent,dither])n.closest("label").hidden=hdr;background.closest("label").hidden=hdr&&range.value!=="jpeg";quality.closest("label").hidden=!["jpeg","avif"].includes(range.value)&&!(range.value==="sdr"&&format.value==="Jpeg");for(const f of[width,height])f.closest("label").hidden=size.value!=="Fit";ppi.closest("label").hidden=resolution.value!=="Ppi";
      metadata.closest("label").hidden=!model.metadata||!metadataView.available;removeLocation.closest("label").hidden=!model.metadata||!metadataView.location;
      metadataNote.textContent=metadataView.note??"";metadataNote.hidden=!model.metadata||!metadataView.note;};
    const load=()=>{
      if(!model.profiles.some(p=>p.name===recipe.profile.name&&JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile))){model.profiles.push(recipe.profile);const option=element("option","",recipe.profile.name);option.value=model.profiles.length-1;profile.append(option);}
      range.value=recipe.format==="Exr"?"exr":recipe.format.startsWith("PngHdr")?"hdr":recipe.format.startsWith("JpegHdr")?"jpeg":recipe.format.startsWith("AvifHdr")?"avif":"sdr";clip.checked=recipe.format.endsWith("Mapped");format.value=range.value!=="sdr"?"Png":recipe.format;profile.value=String(Math.max(0,model.profiles.findIndex(p=>p.name===recipe.profile.name&&JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile))));depth.value=recipe.depth;background.value=recipe.background;
      intent.value=recipe.encoding.conversion.intent;dither.value=recipe.encoding.dither;setNumber(quality,recipe.jpeg_quality);size.value=recipe.size.Fit?"Fit":"Original";
      if(recipe.size.Fit){setNumber(width,recipe.size.Fit.bounds[0]);setNumber(height,recipe.size.Fit.bounds[1]);};resolution.value=recipe.resolution.Ppi?"Ppi":recipe.resolution;if(recipe.resolution.Ppi)setNumber(ppi,recipe.resolution.Ppi);
      metadata.value=recipe.metadata.keep;removeLocation.checked=recipe.metadata.remove_location;visible();
    };
    destination.onchange=()=>preference({type:"get",index:Number(destination.value)});
    const updateDraft=action=>{
      const draft=app.export_draft(readRecipe(),action);recipe=draft.recipe;metadataView=draft.metadata;
      for(const[node,allowed]of[[format,draft.formats],[depth,draft.depths],[background,draft.backgrounds],[dither,draft.dithers]]) {
        for(const option of node.options)option.disabled=!allowed.includes(option.value);
      }
      load();
    };
    range.onchange=()=>updateDraft({type:"format",value:rangeFormat()});
    clip.onchange=()=>updateDraft({type:"format",value:rangeFormat()});
    format.onchange=()=>updateDraft({type:"format",value:format.value});
    profile.onchange=()=>updateDraft({type:"profile",value:model.profiles[Number(profile.value)]});
    depth.onchange=()=>updateDraft({type:"depth",value:depth.value});
    background.onchange=()=>updateDraft({type:"background",value:background.value});
    metadata.onchange=removeLocation.onchange=()=>updateDraft({type:"metadata",value:readMetadata()});
    size.onchange=resolution.onchange=visible;load();
    const error=element("p","error-message");form.append(error);
    form.append(button(profilesCopy.import,async()=>{
      try {const imported=await importProfile(app,element);if(!imported)return;model.profiles.push(imported);const option=element("option","",imported.name);option.value=model.profiles.length-1;profile.append(option);profile.value=option.value;updateDraft({type:"profile",value:imported});error.textContent="";}
      catch(e){error.textContent=String(e);}
    }));
    form.append(button(profilesCopy.saved_dialog,async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;model.profiles.push(imported);const option=element("option","",imported.name);option.value=model.profiles.length-1;profile.append(option);profile.value=option.value;updateDraft({type:"profile",value:imported});invalidate();}catch(e){error.textContent=String(e);}}));
    const readRecipe=()=>({format:rangeFormat(),profile:model.profiles[Number(profile.value)],depth:depth.value,background:background.value,
      encoding:{conversion:{intent:intent.value,black_point_compensation:false},dither:dither.value},jpeg_quality:quality.numericValue,
      size:size.value==="Original"?"Original":{Fit:{bounds:[width.numericValue,height.numericValue],enlarge:recipe.size.Fit?.enlarge??false}},
      resolution:resolution.value==="Ppi"?{Ppi:ppi.numericValue}:resolution.value,metadata:readMetadata()});
    const selected=()=>app.export_validate(app.export_draft(readRecipe(),{type:"refresh"}).recipe);
    updateDraft({type:"refresh"});
    const presetName=field(copy.preset_name,element("input"));presetName.maxLength=80;
    const presetButtons=element("div","document-size");
    const presetAction=type=>{if(!commitNumbers()||!form.reportValidity())return;try{preference(type==="save"?{type,name:presetName.value,recipe:selected()}:{type,index:Number(destination.value),recipe:selected()});}catch(e){error.textContent=String(e);}};
    const savePreset=button(copy.save_preset,()=>presetAction("save"));
    const updatePreset=button(copy.update_preset,()=>presetAction("update"));
    const removePreset=button(copy.delete_preset,()=>preference({type:"remove",index:Number(destination.value)}));
    const resetPreset=button(copy.reset_destination,()=>preference({type:"reset",index:Number(destination.value)}));
    const presetAvailability=()=>{updatePreset.disabled=removePreset.disabled=Number(destination.value)<4;resetPreset.disabled=Number(destination.value)>=4;};
    presetButtons.append(savePreset,updatePreset,removePreset,resetPreset);form.append(presetButtons);presetAvailability();
    let preferenceBusy=false;
    async function preference(action){
      if(preferenceBusy||running)return;preferenceBusy=true;const inputs=[...form.querySelectorAll('input,select,button')];inputs.forEach(n=>n.disabled=true);
      try{library=await app.export_presets(action);destination.replaceChildren();library.names.forEach((name,i)=>{const option=element("option","",name);option.value=i;destination.append(option);});currentPreset=library.index??0;destination.value=String(currentPreset);if(library.recipe){recipe=library.recipe;load();updateDraft({type:"refresh"});}invalidate();error.textContent="";}
      catch(e){destination.value=String(currentPreset);error.textContent=String(e);}finally{preferenceBusy=false;inputs.forEach(n=>n.disabled=false);presetAvailability();}
    }
    const comparison=element("div","color-comparison"),status=element("p"),footer=element("footer");
    const previewMode=select(copy.preview_rendition,[["hdr",copy.hdr_preview],["sdr",copy.sdr_base]]);previewMode.closest("label").hidden=true;
    let rangeBlocked=false,completed;
    const invalidate=()=>{completed=null;comparison.replaceChildren();previewMode.closest("label").hidden=true;status.textContent="";rangeBlocked=false;choose.disabled=false;};
    const invalidateInput=e=>{if(e.target!==previewMode)invalidate();};
    form.addEventListener("input",invalidateInput,true);form.addEventListener("change",invalidateInput,true);
    const drawComparison=()=>{
      const output=completed;comparison.replaceChildren();
      const images=[output.previews[0],output.sdr_preview&&previewMode.value==="sdr"?output.sdr_preview:output.previews[1]];
      images.forEach((image,index)=>{const figure=element("figure"),canvas=element("canvas");[canvas.width,canvas.height]=image.extent;
        // These bounded previews already arrive as CPU pixels. Keep their 2D
        // storage on CPU, including while the scrollable comparison is offscreen.
        canvas.getContext("2d",{willReadFrequently:true}).putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);canvas.setAttribute("aria-label",index?copy.output_preview:copy.artwork_preview);
        const caption=index?(output.sdr_preview?(previewMode.value==="sdr"?copy.sdr_base:copy.hdr_preview):copy.output):copy.artwork;
        figure.append(canvas,element("figcaption","",caption));comparison.append(figure);});
    };
    previewMode.onchange=()=>{if(completed)drawComparison();};
    const cancel=button(copy.common.cancel,()=>{control?.cancel();finish(null);});
    const commitNumbers=()=>[quality,width,height,ppi].filter(node=>!node.closest("label").hidden).every(node=>node.commit());
    const choose=button(copy.choose_file,()=>{if(!commitNumbers()||!form.reportValidity())return;try{finish({recipe:selected(),destination:Number(destination.value)});}catch(e){error.textContent=String(e);}},"suggested-action");
    const preview=button(copy.preview,()=>{
      if(running||!commitNumbers()||!form.reportValidity())return;
      let recipe;try{recipe=selected();}catch(e){error.textContent=String(e);return;}
      invalidate();control?.free();control=app.capture_control();status.textContent=copy.preparing_comparison;error.textContent="";
      const inputs=[...form.querySelectorAll('input,select,button')].filter(node=>node!==cancel);inputs.forEach(node=>node.disabled=true);
      running=(async()=>{
        try{
          const output=await gpuOperation(()=>app.export_image(id,recipe,control,true));
          if(closed||control.cancelled())return;
          completed=output;previewMode.closest("label").hidden=!output.sdr_preview;drawComparison();
          rangeBlocked=["PngHdr","JpegHdr","AvifHdr"].includes(recipe.format)&&output.clipped_channels>0;
          status.textContent=app.export_preview_status(recipe,Boolean(output.sdr_preview),output.clipped_channels>0);
        }catch(e){if(!closed&&!control.cancelled())error.textContent=String(e);}
        finally{running=null;if(!closed){inputs.forEach(node=>node.disabled=false);choose.disabled=rangeBlocked;presetAvailability();}}
      })();
    });
    footer.append(cancel,preview,choose);form.append(comparison,status,footer);form.onsubmit=e=>e.preventDefault();
  });
  closed=true;control?.cancel();await running;return result;
  }finally{control?.free();}

}

export function importProfile(app,element) {
  const copy=app.profile_copy();
  return new Promise((resolve,reject)=>{
    const input=element("input");input.type="file";input.accept=".icc,.icm";input.hidden=true;document.body.append(input);
    input.oncancel=()=>{input.remove();resolve(null);};
    input.onchange=async()=>{try{const file=input.files[0];if(!file){resolve(null);return;}if(file.size>copy.read_bytes)throw new Error(copy.read_limit);resolve(await app.profile_library("import",undefined,new Uint8Array(await file.arrayBuffer())));}catch(e){reject(e);}finally{input.remove();}};
    input.click();
  });
}

export function chooseSourceProfile({app,dialog,element,button}) {
  const copy=app.profile_copy();
  return dialog(copy.interpret_title,(form,finish)=>{
    form.append(element("p","",copy.interpret_help));
    const profiles=["Srgb","DisplayP3","AdobeRgb","ProPhoto"].map(space=>({Builtin:space}));
    const select=element("select");select.setAttribute("aria-label",copy.interpret_as);
    ["sRGB","Display P3","Adobe RGB (1998)","ProPhoto RGB"].forEach((name,i)=>{const option=element("option","",name);option.value=i;select.append(option);});form.append(select);
    const error=element("p","error-message");form.append(error);
    form.append(button(copy.import,async()=>{try{const imported=await importProfile(app,element);if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;select.append(option);select.value=option.value;error.textContent="";}catch(e){error.textContent=String(e);}}));
    form.append(button(copy.saved_dialog,async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;select.append(option);select.value=option.value;}catch(e){error.textContent=String(e);}}));
    const footer=element("footer");footer.append(button(copy.common.cancel,()=>finish(null)),button(copy.use_profile,()=>finish(profiles[Number(select.value)]),"suggested-action"));form.append(footer);
  });
}

export function chooseProfileLibrary({app,element,button,manage=false}) {
  const copy=app.profile_copy();
  return new Promise(resolve=>{
    const root=element("dialog","document-dialog profile-library"),form=element("form"),list=element("div"),error=element("p","error-message");
    form.method="dialog";let result=null,closed=false;
    const finish=value=>{result=value;root.close();};
    root.addEventListener("close",()=>{closed=true;root.remove();resolve(result);},{once:true});
    form.append(element("h2","",copy.library_title),element("p","",copy.library_help),list,error);
    const done=button(copy.common.done,()=>finish(null));
    const run=async action=>{
      const inputs=[...form.querySelectorAll('button')];inputs.forEach(b=>b.disabled=true);error.textContent="";
      try{await action();}catch(e){if(!closed)error.textContent=String(e);}finally{if(!closed)inputs.forEach(b=>b.disabled=false);}
    };
    const refresh=async()=>{
      const entries=await app.profile_library("list");if(closed)return;
      list.replaceChildren();if(!entries.length)list.append(element("p","",copy.empty));
      for(const entry of entries){
        const row=element("section","profile-entry"),actions=element("div","document-size");
        row.append(element("h3","",entry.name),element("p","",entry.issue??entry.details));
        if(!manage&&!entry.issue)actions.append(button(copy.use_profile,()=>run(async()=>{const profile=await app.profile_library("get",entry.id);if(!closed)finish(profile);})));
        actions.append(button(entry.visible===false?copy.show:copy.hide,()=>run(async()=>{await app.profile_library(entry.visible===false?"show":"hide",entry.id);await refresh();})));
        actions.append(button(copy.remove,()=>run(async()=>{await app.profile_library("remove",entry.id);await refresh();})));row.append(actions);list.append(row);
      }
    };
    form.append(button(copy.import,()=>run(async()=>{const profile=await importProfile(app,element);if(profile&&!manage&&!closed)finish(profile);else await refresh();})),done);
    form.onsubmit=e=>e.preventDefault();root.append(form);document.body.append(root);root.showModal();run(refresh);
  });
}
