// One cancellable, full-resolution inspection per open window. Navigation and
// editing remain available; a stale result is labeled until its successor ends.
export function createHistogram({app,element,button}) {
  let root,control,running=false,result,wanted,failed,changed=0,timer;
  const key=()=>{const f=app.state().document_file;return`${f.epoch}:${f.revision}`;};
  return {open(){
    if(root){root.focus();return;}
    root=element("dialog","histogram-dialog");root.setAttribute("aria-label","Histogram");
    const title=element("h2","","Histogram"),close=button("Close",()=>root.close()),status=element("p"),description=element("p"),range=element("p");
    const select=element("select");select.setAttribute("aria-label","Histogram channel");
    ["RGB","Red","Green","Blue","Luminance"].forEach((name,i)=>{const option=element("option","",name);option.value=i;select.append(option);});
    const logarithmic=element("input");logarithmic.type="checkbox";
    const logLabel=element("label","","Log scale");logLabel.prepend(logarithmic);
    const automatic=element("input");automatic.type="checkbox";automatic.checked=true;
    const autoLabel=element("label","","Auto update");autoLabel.prepend(automatic);
    const canvas=element("canvas");canvas.width=512;canvas.height=180;canvas.setAttribute("aria-label","Document histogram");
    const draw=()=>{
      if(!result)return;const h=result.histogram,channel=Number(select.value),indices=channel?[channel-1]:[0,1,2];
      const [start,end]=result.axis.bins.map(Number),bins=Array.from({length:end-start},(_,i)=>i+start);
      const scale=n=>logarithmic.checked?Math.log1p(Number(n)):Number(n);
      const maximum=Math.max(1,...indices.flatMap(i=>bins.map(x=>scale(h.channels[i].bins[x]))));
      const context=canvas.getContext("2d");context.clearRect(0,0,512,180);
      for(const i of indices){context.beginPath();context.moveTo(0,180);bins.forEach((x,j)=>context.lineTo(j/(bins.length-1)*512,180-scale(h.channels[i].bins[x])/maximum*176));context.lineTo(512,180);context.closePath();context.fillStyle=["#ed747480","#69cf9280","#73a7f580","#aaaaaacc"][i];context.fill();}
      if(result.axis.white!=null){const x=result.axis.white*512;context.beginPath();context.moveTo(x,0);context.lineTo(x,180);context.strokeStyle='#bbbbbb';context.setLineDash([4,4]);context.stroke();context.setLineDash([]);context.fillStyle='#dddddd';context.fillText('0 EV · white',x+4,12);}
      description.textContent=`${h.color.space} · ${h.color.depth==="F32"?"32-bit float":h.color.depth==="F16"?"16-bit float":h.color.depth==="U16"?"16-bit":"8-bit"} · ${Number(h.pixels).toLocaleString()} nontransparent pixels`;
      const labels=["R","G","B","Y"];range.textContent=indices.map(i=>{const c=h.channels[i];return`${labels[i]}: below 0 ${Number(c.below).toLocaleString()}, above 1 ${Number(c.above).toLocaleString()} · black ${Number(c.black).toLocaleString()}, white ${Number(c.white).toLocaleString()}`;}).join("\n");
      canvas.title=["F16","F32"].includes(h.color.depth)?`Linear RGB and luminance: ${result.axis.stops.map(n=>n.toFixed(1)).join(" to ")} stops from reference white. Zero and negative values are counted separately.`:"Profile-encoded document RGB; luminance is linear Y.";
      axis.textContent=canvas.title+" Includes visible paper; excludes transparent pixels and display overlays.";
    };
    const refresh=async()=>{
      if(running)return;failed=null;running=true;const owner=root;control=app.capture_control();status.textContent="Updating · complete composite at full resolution";update.disabled=true;
      try{const next=await app.histogram(control);if(root===owner){result=next;draw();status.textContent=key()===`${next.epoch}:${next.revision}`?(next.sampled_time==null?"Current committed drawing":`Animated effects · snapshot at ${Number(next.sampled_time).toFixed(2)} s`):"Drawing changed · showing previous inspection";}}
      catch(error){if(root===owner&&!control.cancelled()){failed=key();status.textContent=String(error);}}
      finally{control.free();control=null;running=false;if(root===owner)update.disabled=false;}
    };
    const update=button("Refresh",refresh);
    select.onchange=logarithmic.onchange=draw;
    const header=element("header");header.append(title,close);const actions=element("div","histogram-actions");actions.append(select,logLabel,autoLabel,update);
    const axis=element("p");root.append(header,actions,canvas,description,range,status,axis);
    root.addEventListener("close",()=>{clearInterval(timer);control?.cancel();root.remove();root=null;result=null;wanted=null;failed=null;},{once:true});
    document.body.append(root);root.show();refresh();
    timer=setInterval(()=>{const current=key();if(wanted!==current){if(wanted!==undefined)control?.cancel();wanted=current;changed=performance.now();if(result)status.textContent="Drawing changed · showing previous inspection";}
      if(automatic.checked&&!running&&failed!==current&&performance.now()-changed>=300&&(!result||`${result.epoch}:${result.revision}`!==current))refresh();},200);
  }};
}
