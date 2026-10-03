import {liveCopy,bindCopy} from "./localization.js";
// One cancellable, full-resolution inspection per open window. Navigation and
// editing remain available; a stale result is labeled until its successor ends.
export function createHistogram({app,element,button}) {
  const copy=liveCopy(app,"catalog").native_copy.color,common=liveCopy(app,"bootstrap_view").common,depthCopy=liveCopy(app,"document_color_copy");
  let root,control,running=false,result,wanted,failed,changed=0,timer,done=Promise.resolve(),present=()=>{};
  const key=()=>{const f=app.state().document_file;return`${f.epoch}:${f.revision}`;};
  return {localize(){present();},async retire(){clearInterval(timer);control?.cancel();const old=root;if(old?.isConnected){const closed=new Promise(resolve=>old.addEventListener('close',resolve,{once:true}));if(old.open)old.close();await closed;}await done;},open(){
    if(root){root.focus();return;}
    root=element("dialog","histogram-dialog");bindCopy(root,()=>copy.histogram,"ariaLabel");
    const title=element("h2"),close=button("",()=>root.close()),status=element("p"),description=element("p"),range=element("p");
    bindCopy(title,()=>copy.histogram);bindCopy(close,()=>common.close);bindCopy(close,()=>common.close,"ariaLabel");
    const select=element("select");bindCopy(select,()=>copy.channel,"ariaLabel");
    [()=>"RGB",()=>copy.red,()=>copy.green,()=>copy.blue,()=>copy.luminance].forEach((name,i)=>{const option=element("option");bindCopy(option,name);option.value=i;select.append(option);});
    const logarithmic=element("input");logarithmic.type="checkbox";
    const logLabel=element("label");bindCopy(logLabel,()=>copy.log_scale);bindCopy(logarithmic,()=>copy.log_scale,"ariaLabel");logLabel.prepend(logarithmic);
    const automatic=element("input");automatic.type="checkbox";automatic.checked=true;
    const autoLabel=element("label");bindCopy(autoLabel,()=>copy.auto_update);bindCopy(automatic,()=>copy.auto_update,"ariaLabel");autoLabel.prepend(automatic);
    const canvas=element("canvas");canvas.width=512;canvas.height=180;bindCopy(canvas,()=>app.native_caption({type:"inspection_graph",channel:select.options[Number(select.value)]?.textContent??"RGB"}),"ariaLabel");
    const draw=()=>{
      if(!result)return;const h=result.histogram,channel=Number(select.value),indices=channel?[channel-1]:[0,1,2];
      const [start,end]=result.axis.bins.map(Number),bins=Array.from({length:end-start},(_,i)=>i+start);
      const scale=n=>logarithmic.checked?Math.log1p(Number(n)):Number(n);
      const maximum=Math.max(1,...indices.flatMap(i=>bins.map(x=>scale(h.channels[i].bins[x]))));
      const context=canvas.getContext("2d",{willReadFrequently:true});context.clearRect(0,0,512,180);
      for(const i of indices){context.beginPath();context.moveTo(0,180);bins.forEach((x,j)=>context.lineTo(j/(bins.length-1)*512,180-scale(h.channels[i].bins[x])/maximum*176));context.lineTo(512,180);context.closePath();context.fillStyle=["#ed747480","#69cf9280","#73a7f580","#aaaaaacc"][i];context.fill();}
      if(result.axis.white!=null){const x=result.axis.white*512;context.beginPath();context.moveTo(x,0);context.lineTo(x,180);context.strokeStyle='#bbbbbb';context.setLineDash([4,4]);context.stroke();context.setLineDash([]);context.fillStyle='#dddddd';context.fillText(`0 EV · ${copy.sdr_white}`,x+4,12);}

    };
    const channels=()=>Number(select.value)?[Number(select.value)-1]:[0,1,2];
    const statusText=()=>{
      if(running)return copy.inspection_updating;
      if(failed)return copy.inspection_failed;
      if(!result)return copy.inspection_preparing;
      const current=result.sampled_time==null?copy.inspection_current:app.native_caption({type:"inspection_sample",seconds:Number(result.sampled_time)});
      return key()===`${result.epoch}:${result.revision}`?current:app.native_caption({type:"inspection_changed",status:current});
    };
    bindCopy(status,statusText);
    const descriptionRead=()=>{if(!result)return "";const h=result.histogram,depth={F32:"depth_float32",F16:"depth_float16",U16:"depth_16",U8:"depth_8"}[h.color.depth];return `${h.color.space} · ${depthCopy[depth]} · ${app.native_caption({type:"inspection_pixels",sampled:Number(h.pixels),transparent:Number(h.transparent)})}`;};bindCopy(description,descriptionRead);
    const rangeRead=()=>!result?"":channels().map(i=>{const c=result.histogram.channels[i];return `${["R","G","B","Y"][i]}: ${app.native_caption({type:"inspection_channel",below:Number(c.below),above:Number(c.above),black:Number(c.black),white:Number(c.white)})}`;}).join("\n");bindCopy(range,rangeRead);
    const help=()=>result&&["F16","F32"].includes(result.histogram.color.depth)?copy.inspection_hdr_help:copy.inspection_help;
    bindCopy(canvas,help,"title");
    const axis=element("p"),axisRead=()=>`${result?.axis.stops?app.native_caption({type:"inspection_range",start:Number(result.axis.stops[0]),end:Number(result.axis.stops[1])})+"\n":""}${help()}`;bindCopy(axis,axisRead);
    present=()=>{draw();bindCopy(status,statusText);bindCopy(description,descriptionRead);bindCopy(range,rangeRead);bindCopy(axis,axisRead);bindCopy(canvas,help,"title");bindCopy(canvas,()=>app.native_caption({type:"inspection_graph",channel:select.options[Number(select.value)]?.textContent??"RGB"}),"ariaLabel");};
    const refresh=async()=>{
      if(running)return;failed=null;running=true;const owner=root;control=app.capture_control();present();update.disabled=true;
      let settled;done=new Promise(resolve=>settled=resolve);
      try{const next=await app.histogram(control);if(root===owner){result=next;}}
      catch(error){if(root===owner&&!control.cancelled()){failed=key();console.error(error);}}
      finally{control.free();control=null;running=false;if(root===owner)update.disabled=false;if(root)present();settled();}
    };
    const update=button("",refresh);bindCopy(update,()=>copy.refresh);bindCopy(update,()=>copy.refresh,"ariaLabel");
    select.onchange=logarithmic.onchange=()=>present();
    const header=element("header");header.append(title,close);const actions=element("div","histogram-actions");actions.append(select,logLabel,autoLabel,update);
    root.append(header,actions,canvas,description,range,status,axis);
    root.addEventListener("close",()=>{clearInterval(timer);control?.cancel();root.remove();root=null;result=null;wanted=null;failed=null;present=()=>{};},{once:true});
    document.body.append(root);root.show();refresh();
    timer=setInterval(()=>{const current=key();if(wanted!==current){if(wanted!==undefined)control?.cancel();wanted=current;changed=performance.now();if(result)present();}
      if(automatic.checked&&!running&&failed!==current&&performance.now()-changed>=300&&(!result||`${result.epoch}:${result.revision}`!==current))refresh();},200);
  }};
}
