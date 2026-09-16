// The shared recipe describes a delivery copy, independent of the master.
export async function chooseExport({app,dialog,element,button,gpuOperation,id}) {
  let control,running,closed=false;
  const model=app.export_form();
  try { const result=await dialog("Export image",(form,finish)=>{
    let recipe=structuredClone(model.recipes[0][1]);
    const field=(label,node)=>{const root=element("label","document-size",label);node.setAttribute("aria-label",label);root.append(node);form.append(root);return node;};
    const select=(label,choices)=>{const node=element("select");for(const[id,name]of choices){const option=element("option","",name);option.value=id;node.append(option);}return field(label,node);};
    const number=(label,value,min,max)=>{const node=element("input");Object.assign(node,{type:"number",value,min,max,step:1});return field(label,node);};
    form.append(element("p","","Export a profiled copy. The editable drawing stays unchanged."));
    const destination=select("Destination",model.recipes.map(([name],i)=>[i,name]));
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
      format.value=recipe.format;profile.value=String(Math.max(0,model.profiles.findIndex(p=>JSON.stringify(p.profile)===JSON.stringify(recipe.profile.profile))));depth.value=recipe.depth;background.value=recipe.background;
      intent.value=recipe.encoding.conversion.intent;dither.value=recipe.encoding.dither;quality.value=recipe.jpeg_quality;size.value="Original";resolution.value="Master";visible();
    };
    destination.onchange=()=>{recipe=structuredClone(model.recipes[Number(destination.value)][1]);load();};
    format.onchange=()=>{if(format.value==="Jpeg"){depth.value="U8";if(background.value==="Preserve")background.value="White";}visible();};
    size.onchange=resolution.onchange=visible;load();
    const error=element("p","error-message");form.append(error);
    form.append(button("Import ICC Profile…",async()=>{
      try {const imported=await importProfile(app,element);if(!imported)return;model.profiles.push(imported);const option=element("option","",imported.name);option.value=model.profiles.length-1;profile.append(option);profile.value=option.value;error.textContent="";}
      catch(e){error.textContent=String(e);}
    }));
    const selected=()=>app.export_validate({format:format.value,profile:model.profiles[Number(profile.value)],depth:depth.value,background:background.value,
      encoding:{conversion:{intent:intent.value,black_point_compensation:false},dither:dither.value},jpeg_quality:Number(quality.value),
      size:size.value==="Original"?"Original":{Fit:{bounds:[Number(width.value),Number(height.value)],enlarge:false}},
      resolution:resolution.value==="Ppi"?{Ppi:Number(ppi.value)}:resolution.value});
    const comparison=element("div","color-comparison"),status=element("p"),footer=element("footer");
    const invalidate=()=>{comparison.replaceChildren();status.textContent="";};
    form.addEventListener("input",invalidate,true);form.addEventListener("change",invalidate,true);
    const cancel=button("Cancel",()=>{control?.cancel();finish(null);});
    const choose=button("Choose File…",()=>{if(!form.reportValidity())return;try{finish(selected());}catch(e){error.textContent=String(e);}},"suggested-action");
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
        finally{running=null;if(!closed)inputs.forEach(node=>node.disabled=false);}
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
    input.onchange=async()=>{try{const file=input.files[0];if(!file){resolve(null);return;}if(file.size>16*1024*1024)throw new Error("ICC profile exceeds 16 MiB");resolve(app.inspect_profile(new Uint8Array(await file.arrayBuffer())));}catch(e){reject(e);}finally{input.remove();}};
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
    const footer=element("footer");footer.append(button("Cancel",()=>finish(null)),button("Use Profile",()=>finish(profiles[Number(select.value)]),"suggested-action"));form.append(footer);
  });
}
