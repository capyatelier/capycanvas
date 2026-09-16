// A comparison owns one immutable candidate. Cancel drains its work before the
// document request is released; Apply publishes that exact prepared result.
export async function chooseDocumentColor({app,dialog,element,button,gpuOperation,request,id}) {
  const history=request.type==="color_history",operation=request.operation,current=app.document_color();
  let candidate,control,running,closed=false,accepted=false;
  try {
    await dialog(history?(request.redo?"Redo Color Change":"Undo Color Change"):{assign:"Assign Profile",convert:"Convert Color Space",depth:"Change Bit Depth"}[operation],(form,finish)=>{
      const inputs=[];
      const select=(title,choices,value)=>{
        const label=element("label","document-size",title),node=element("select");node.setAttribute("aria-label",title);
        for(const [id,name] of choices){const option=element("option","",name);option.value=id;node.append(option);}node.value=value;
        label.append(node);form.append(label);inputs.push(node);return node;
      };
      if(!history)form.append(element("p","",operation==="assign"?"Keep document RGB numbers and reinterpret their color. Retained original photos keep their source profile.":operation==="depth"?"Change stored precision. Effects and the blending domain remain the same.":"Convert editable layers. Compare the complete composition before applying; original photo samples stay retained."));
      const space=!history&&operation!=="depth"?select("Color space",[["Srgb","sRGB"],["DisplayP3","Display P3"],["AdobeRgb","Adobe RGB"],["ProPhoto","ProPhoto RGB"]],current.space):null;
      const depth=operation==="depth"?select("Bit depth",[["U8","8-bit SDR"],["U16","16-bit SDR"]],current.depth):null;
      const dither=operation==="depth"?select("Dither",[["None","None"],["Stochastic8","Stochastic (8-bit)"]],"None"):null;
      const intent=operation==="convert"?select("Rendering intent",[["RelativeColorimetric","Relative colorimetric"],["Perceptual","Perceptual"],["Saturation","Saturation"],["AbsoluteColorimetric","Absolute colorimetric"]],"RelativeColorimetric"):null;
      const status=element("p"),comparison=element("div","color-comparison"),footer=element("footer");
      const apply=button("Apply",()=>{accepted=true;finish(true);},"suggested-action");apply.disabled=true;
      const cancel=button("Cancel",()=>{control?.cancel();finish(null);});
      const invalidate=()=>{candidate?.free();candidate=null;apply.disabled=true;comparison.replaceChildren();status.textContent="Preview the complete result before applying.";};
      inputs.forEach(node=>node.onchange=invalidate);
      const prepare=()=>{
        if(running)return;invalidate();control?.free();control=app.capture_control();
        inputs.forEach(node=>node.disabled=true);preview.disabled=true;status.textContent="Preparing complete color result…";
        const choice=history?null:operation==="assign"?{Assign:space.value}:operation==="depth"?{Depth:{depth:depth.value,dither:depth.value==="U8"?dither.value:"None"}}:{Convert:{space:space.value,options:{intent:intent.value,black_point_compensation:false}}};
        running=(async()=>{
          try {
            const next=await gpuOperation(()=>app.prepare_color(id,choice,control));
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
