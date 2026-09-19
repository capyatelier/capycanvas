import {importProfile,chooseProfileLibrary} from './export-controls.js';
// A comparison owns one immutable candidate. Cancel drains its work before the
// document request is released; Apply publishes that exact prepared result.
export async function chooseDocumentColor({app,dialog,element,button,gpuOperation,request,id}) {
  const history=request.type==="color_history",source=["repair_source_profile","rasterize_source"].includes(request.type),rasterize=request.type==="rasterize_source";
  const operation=source?(rasterize?"rasterize":"repair"):request.operation,current=app.document_color();
  const profiles=["Srgb","DisplayP3","AdobeRgb","ProPhoto"].map(Builtin=>({Builtin}));
  let candidate,control,running,closed=false,accepted=false;
  try {
    await dialog(history?(request.redo?"Redo Color Change":"Undo Color Change"):{assign:"Assign Profile",convert:"Convert Color Space",depth:"Change Bit Depth",repair:"Repair Source Profile",rasterize:"Rasterize Retained Source"}[operation],(form,finish)=>{
      const inputs=[];
      const select=(title,choices,value)=>{
        const label=element("label","document-size",title),node=element("select");node.setAttribute("aria-label",title);
        for(const [id,name] of choices){const option=element("option","",name);option.value=id;node.append(option);}node.value=value;
        label.append(node);form.append(label);inputs.push(node);return node;
      };
      if(!history&&!source)form.append(element("p","",operation==="assign"?"Keep document RGB numbers and reinterpret their color. Retained original photos keep their source profile.":operation==="depth"?"Change stored precision. Effects and the blending domain remain the same.":"Convert editable layers. Compare the complete composition before applying; original photo samples stay retained."));
      if(source)form.append(element("p","",rasterize?"Convert the original to the document color space and bit depth at its full size. Existing paint, position, masks and adjustments stay intact. Undo restores the original profile and precision.":"Change how original image numbers are interpreted, keeping their samples and depth. A layer with pixel edits receives a separate corrected original at the same position; existing edits remain intact."));
      const profile=source&&!rasterize?select("Correct source profile",[["0","sRGB"],["1","Display P3"],["2","Adobe RGB (1998)"],["3","ProPhoto RGB"]],"0"):null;
      const space=!history&&!source&&operation!=="depth"?select("Color space",[["Srgb","sRGB"],["DisplayP3","Display P3"],["AdobeRgb","Adobe RGB"],["ProPhoto","ProPhoto RGB"]],current.space):null;
      const depth=operation==="depth"?select("Bit depth",[["U8","8-bit SDR"],["U16","16-bit SDR"],["F16","16-bit float HDR"]],current.depth):null;
      const dither=operation==="depth"?select("Dither",[["None","None"],["Stochastic8","Stochastic (8-bit)"]],"None"):null;
      const result=operation==="convert"?select("Result",[["layers","Editable layers"],["copy","Save flattened copy"]],"layers"):null;
      const intent=operation==="convert"?select("Rendering intent",[["RelativeColorimetric","Relative colorimetric"],["Perceptual","Perceptual"],["Saturation","Saturation"],["AbsoluteColorimetric","Absolute colorimetric"]],"RelativeColorimetric"):null;
      const status=element("p"),comparison=element("div","color-comparison"),footer=element("footer");
      const apply=button(source?(rasterize?"Rasterize":"Apply Profile"):"Apply",()=>{accepted=true;finish(true);},"suggested-action");apply.disabled=true;
      const cancel=button("Cancel",()=>{control?.cancel();finish(null);});
      const invalidate=()=>{candidate?.free();candidate=null;apply.disabled=true;comparison.replaceChildren();status.textContent="Preview the complete result before applying.";};
      if(profile){const load=button("Import ICC Profile…",async()=>{try{const imported=await importProfile(app,element);if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;profile.append(option);profile.value=option.value;invalidate();}catch(error){status.textContent=String(error);}});const saved=button("Saved Profiles…",async()=>{try{const imported=await chooseProfileLibrary({app,element,button});if(!imported)return;profiles.push(imported.profile);const option=element("option","",imported.name);option.value=profiles.length-1;profile.append(option);profile.value=option.value;invalidate();}catch(error){status.textContent=String(error);}});inputs.push(load,saved);form.append(load,saved);}
      inputs.forEach(node=>node.onchange=invalidate);
      const prepare=()=>{
        if(running)return;invalidate();control?.free();control=app.capture_control();
        inputs.forEach(node=>node.disabled=true);preview.disabled=true;status.textContent="Preparing complete color result…";
        const choice=source?(rasterize?null:profiles[Number(profile.value)]):history?null:operation==="assign"?{Assign:space.value}:operation==="depth"?{Depth:{depth:depth.value,dither:depth.value==="U8"?dither.value:"None"}}:{Convert:{space:space.value,options:{intent:intent.value,black_point_compensation:false}}};
        running=(async()=>{
          try {
            const next=await gpuOperation(async()=>{
              if(!source)return app.prepare_color(id,choice,control,result?.value==="copy");
              const prepared=await app.prepare_source(id,choice,control);
              return app.prepare_source_comparison(prepared);
            });
            if(closed||control.cancelled()){next.free();return;}
            candidate=next;
            if(history){accepted=true;finish(true);return;}
            candidate.previews().forEach((image,index)=>{
              const figure=element("figure"),canvas=element("canvas"),caption=element("figcaption","",index?"After":"Before");
              [canvas.width,canvas.height]=image.extent;
              canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(image.pixels),...image.extent),0,0);
              canvas.setAttribute("aria-label",index?"Prepared composition":"Original composition");figure.append(canvas,caption);comparison.append(figure);
            });
            status.textContent=candidate.clipped_channels()>0?"Some colors exceed the destination gamut. Compare the result before applying.":"Complete composition · sRGB display preview";
            if(source){status.textContent=`Current source profile: ${candidate.source_profile()}. `+status.textContent;if(candidate.adds_layer())status.textContent+=" Apply adds a corrected original as a new layer; the existing layer keeps its edits, masks and adjustments.";apply.textContent=rasterize?"Rasterize":candidate.adds_layer()?"Add Corrected Source":"Apply Profile";}
            if(!source)apply.textContent=candidate.is_copy()?"Save Copy…":"Apply";
            apply.disabled=false;
          }catch(error){const wasCancelled=control.cancelled();control.cancel();if(!closed&&!wasCancelled)status.textContent=String(error);}
          finally{running=null;if(!closed){inputs.forEach(node=>node.disabled=false);preview.disabled=false;}}
        })();
      };
      const preview=button("Preview Complete Result",prepare);
      footer.append(cancel);if(!history)footer.append(preview,apply);
      form.append(comparison,status,footer);form.onsubmit=e=>e.preventDefault();
      if(history)queueMicrotask(prepare);
    });
    closed=true;if(!accepted)control?.cancel();await running;
    if(!accepted){candidate?.free();candidate=null;}
    return candidate;
  } finally {control?.free();}
}
