// The shared recipe describes a delivery copy, independent of the master.
export async function chooseExport({app,dialog,element,button,gpuOperation,id}) {
  let control,running,closed=false;
  const model=app.export_form();
  let library=await app.export_presets({type:"get",index:0});
  try { const result=await dialog("Export image",(form,finish)=>{
    let recipe=library.recipe,currentPreset=0;
    const field=(label,node)=>{const root=element("label","document-size",label);node.setAttribute("aria-label",label);root.append(node);form.append(root);return node;};
    const select=(label,choices)=>{const node=element("select");for(const[id,name]of choices){const option=element("option","",name);option.value=id;node.append(option);}return field(label,node);};
    const number=(label,value,min,max)=>{const node=element("input");Object.assign(node,{type:"number",value,min,max,step:1});return field(label,node);};
    form.append(element("p","","Export a profiled copy. The editable drawing stays unchanged."));
    const destination=select("Destination",library.names.map((name,i)=>[i,name]));
    const format=select("Format",[["Png","PNG"],["Tiff","TIFF"],["Jpeg","JPEG"]]);
    const profile=select("Output profile",model.profiles.map((p,i)=>[i,p.name]));
    const depth=select("Bit depth",[["U8","8-bit"],["U16","16-bit"]]);
    const background=select("Transparency",[["Preserve","Preserve"],["White","White background"],["Black","Black background"]]);
    const intent=select("Rendering intent",[["RelativeColorimetric","Relative colorimetric"],["Perceptual","Perceptual"],["Saturation","Saturation"],["AbsoluteColorimetric","Absolute colorimetric"]]);
    const dither=select("Dither",[["None","None"],["Stochastic8","Stochastic (8-bit output)"]]);
    const quality=number("JPEG quality",90,1,100);
    const size=select("Pixel size",[["Original","Original"],["Fit","Fit within bounds"]]);
    const width=number("Maximum width",2048,1,32768),height=number("Maximum height",2048,1,32768);
    const resolution=select("Resolution metadata",[["Master","Keep original"],["Ppi","Pixels per inch"],["Omit","Omit"]]),ppi=number("Pixels per inch",300,1,65535);
    const visible=()=>{quality.closest("label").hidden=format.value!=="Jpeg";for(const f of[width,height])f.closest("label").hidden=size.value!=="Fit";ppi.closest("label").hidden=resolution.value!=="Ppi";};
    const load=()=>{
      if(!model.profiles.some(p=>JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile))){model.profiles.push(recipe.profile);const option=element("option","",recipe.profile.name);option.value=model.profiles.length-1;profile.append(option);}
      format.value=recipe.format;profile.value=String(Math.max(0,model.profiles.findIndex(p=>JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile))));depth.value=recipe.depth;background.value=recipe.background;
      intent.value=recipe.encoding.conversion.intent;dither.value=recipe.encoding.dither;quality.value=recipe.jpeg_quality;size.value=recipe.size.Fit?"Fit":"Original";
      if(recipe.size.Fit)[width.value,height.value]=recipe.size.Fit.bounds;resolution.value=recipe.resolution.Ppi?"Ppi":recipe.resolution;if(recipe.resolution.Ppi)ppi.value=recipe.resolution.Ppi;visible();
    };
    destination.onchange=()=>preference({type:"get",index:Number(destination.value)});
    format.onchange=()=>{if(format.value==="Jpeg"){depth.value="U8";if(background.value==="Preserve")background.value="White";}visible();};
    size.onchange=resolution.onchange=visible;load();
    const error=element("p","error-message");form.append(error);
    form.append(button("Import ICC Profile…",async()=>{
      try {const imported=await importProfile(app,element);if(!imported)return;model.profiles.push(imported);const option=element("option","",imported.name);option.value=model.profiles.length-1;profile.append(option);profile.value=option.value;error.textContent="";}
      catch(e){error.textContent=String(e);}
    }));
    form.append(button("Saved Profiles…",async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;model.profiles.push(imported);const option=element("option","",imported.name);option.value=model.profiles.length-1;profile.append(option);profile.value=option.value;invalidate();}catch(e){error.textContent=String(e);}}));
    const selected=()=>app.export_validate({format:format.value,profile:model.profiles[Number(profile.value)],depth:depth.value,background:background.value,
      encoding:{conversion:{intent:intent.value,black_point_compensation:false},dither:dither.value},jpeg_quality:Number(quality.value),
      size:size.value==="Original"?"Original":{Fit:{bounds:[Number(width.value),Number(height.value)],enlarge:recipe.size.Fit?.enlarge??false}},
      resolution:resolution.value==="Ppi"?{Ppi:Number(ppi.value)}:resolution.value});
    const presetName=field("Preset name",element("input"));presetName.maxLength=80;
    const presetButtons=element("div","document-size");
    const presetAction=type=>{try{preference(type==="save"?{type,name:presetName.value,recipe:selected()}:{type,index:Number(destination.value),recipe:selected()});}catch(e){error.textContent=String(e);}};
    const savePreset=button("Save Preset",()=>presetAction("save"));
    const updatePreset=button("Update Preset",()=>presetAction("update"));
    const removePreset=button("Delete Preset",()=>preference({type:"remove",index:Number(destination.value)}));
    const resetPreset=button("Reset Destination",()=>preference({type:"reset",index:Number(destination.value)}));
    const presetAvailability=()=>{updatePreset.disabled=removePreset.disabled=Number(destination.value)<4;resetPreset.disabled=Number(destination.value)>=4;};
    presetButtons.append(savePreset,updatePreset,removePreset,resetPreset);form.append(presetButtons);presetAvailability();
    let preferenceBusy=false;
    async function preference(action){
      if(preferenceBusy||running)return;preferenceBusy=true;const inputs=[...form.querySelectorAll('input,select,button')];inputs.forEach(n=>n.disabled=true);
      try{library=await app.export_presets(action);destination.replaceChildren();library.names.forEach((name,i)=>{const option=element("option","",name);option.value=i;destination.append(option);});currentPreset=library.index??0;destination.value=String(currentPreset);if(library.recipe){recipe=library.recipe;load();}invalidate();error.textContent="";}
      catch(e){destination.value=String(currentPreset);error.textContent=String(e);}finally{preferenceBusy=false;inputs.forEach(n=>n.disabled=false);presetAvailability();}
    }
    const comparison=element("div","color-comparison"),status=element("p"),footer=element("footer");
    const invalidate=()=>{comparison.replaceChildren();status.textContent="";};
    form.addEventListener("input",invalidate,true);form.addEventListener("change",invalidate,true);
    const cancel=button("Cancel",()=>{control?.cancel();finish(null);});
    const choose=button("Choose File…",()=>{if(!form.reportValidity())return;try{finish({recipe:selected(),destination:Number(destination.value)});}catch(e){error.textContent=String(e);}},"suggested-action");
    const preview=button("Preview Output",()=>{
      if(running||!form.reportValidity())return;
      let recipe;try{recipe=selected();}catch(e){error.textContent=String(e);return;}
      invalidate();control?.free();control=app.capture_control();status.textContent="Preparing complete output comparison…";error.textContent="";
      const inputs=[...form.querySelectorAll('input,select,button')].filter(node=>node!==cancel);inputs.forEach(node=>node.disabled=true);
      running=(async()=>{
        try{
          const output=await gpuOperation(()=>app.export_image(id,recipe,control,true));
          if(closed||control.cancelled())return;
          output.previews.forEach((image,index)=>{const figure=element("figure"),canvas=element("canvas");[canvas.width,canvas.height]=image.extent;
            canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);canvas.setAttribute("aria-label",index?"Output preview":"Artwork preview");
            figure.append(canvas,element("figcaption","",index?"Output":"Artwork"));comparison.append(figure);});
          status.textContent="sRGB display preview · includes output size, profile, depth, transparency and dither; excludes JPEG compression artifacts."+(output.clipped_channels>0?" Some colors exceed the output gamut and will be clipped.":"");
        }catch(e){if(!closed&&!control.cancelled())error.textContent=String(e);}
        finally{running=null;if(!closed){inputs.forEach(node=>node.disabled=false);presetAvailability();}}
      })();
    });
    footer.append(cancel,preview,choose);form.append(comparison,status,footer);form.onsubmit=e=>e.preventDefault();
  });
  closed=true;control?.cancel();await running;return result;
  }finally{control?.free();}

}

export function importProfile(app,element) {
  return new Promise((resolve,reject)=>{
    const input=element("input");input.type="file";input.accept=".icc,.icm";input.hidden=true;document.body.append(input);
    input.oncancel=()=>{input.remove();resolve(null);};
    input.onchange=async()=>{try{const file=input.files[0];if(!file){resolve(null);return;}if(file.size>16*1024*1024)throw new Error("ICC profile exceeds 16 MiB");resolve(await app.profile_library("import",undefined,new Uint8Array(await file.arrayBuffer())));}catch(e){reject(e);}finally{input.remove();}};
    input.click();
  });
}

export function chooseSourceProfile({app,dialog,element,button}) {
  return dialog("Choose image interpretation",(form,finish)=>{
    form.append(element("p","","This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained."));
    const profiles=["Srgb","DisplayP3","AdobeRgb","ProPhoto"].map(space=>({Builtin:space}));
    const select=element("select");select.setAttribute("aria-label","Interpret as");
    ["sRGB","Display P3","Adobe RGB (1998)","ProPhoto RGB"].forEach((name,i)=>{const option=element("option","",name);option.value=i;select.append(option);});form.append(select);
    const error=element("p","error-message");form.append(error);
    form.append(button("Import ICC Profile…",async()=>{try{const imported=await importProfile(app,element);if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;select.append(option);select.value=option.value;error.textContent="";}catch(e){error.textContent=String(e);}}));
    form.append(button("Saved Profiles…",async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;select.append(option);select.value=option.value;}catch(e){error.textContent=String(e);}}));
    const footer=element("footer");footer.append(button("Cancel",()=>finish(null)),button("Use Profile",()=>finish(profiles[Number(select.value)]),"suggested-action"));form.append(footer);
  });
}

export function chooseProfileLibrary({app,element,button,manage=false}) {
  return new Promise(resolve=>{
    const root=element("dialog","document-dialog profile-library"),form=element("form"),list=element("div"),error=element("p","error-message");
    form.method="dialog";let result=null,closed=false;
    const finish=value=>{result=value;root.close();};
    root.addEventListener("close",()=>{closed=true;root.remove();resolve(result);},{once:true});
    form.append(element("h2","","Color Profile Library"),element("p","","Imported profiles are stored as exact copies. Removing an entry leaves original files and profiles embedded in drawings or export presets intact."),list,error);
    const done=button("Done",()=>finish(null));
    const run=async action=>{
      const inputs=[...form.querySelectorAll('button')];inputs.forEach(b=>b.disabled=true);error.textContent="";
      try{await action();}catch(e){if(!closed)error.textContent=String(e);}finally{if(!closed)inputs.forEach(b=>b.disabled=false);}
    };
    const refresh=async()=>{
      const entries=await app.profile_library("list");if(closed)return;
      list.replaceChildren();if(!entries.length)list.append(element("p","","No imported profiles"));
      for(const entry of entries){
        const row=element("section","profile-entry"),actions=element("div","document-size");
        row.append(element("h3","",entry.name),element("p","",entry.issue??`${entry.channels} · ${entry.bytes} bytes · ${entry.id.slice(0,12)}`));
        if(!manage&&!entry.issue)actions.append(button("Use Profile",()=>run(async()=>{const profile=await app.profile_library("get",entry.id);if(!closed)finish(profile);})));
        actions.append(button("Remove",()=>run(async()=>{await app.profile_library("remove",entry.id);await refresh();})));row.append(actions);list.append(row);
      }
    };
    form.append(button("Import ICC Profile…",()=>run(async()=>{const profile=await importProfile(app,element);if(profile&&!manage&&!closed)finish(profile);else await refresh();})),done);
    form.onsubmit=e=>e.preventDefault();root.append(form);document.body.append(root);root.showModal();run(refresh);
  });
}
