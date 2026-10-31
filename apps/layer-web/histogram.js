import {liveCopy} from './localization.js';

export function scopeGraph({state,element,kind,waveform=false}) {
  const canvas=element('canvas');canvas.dataset.scopeControl='chart';canvas.setAttribute('role','img');
  const context=canvas.getContext('2d',{willReadFrequently:true}),imageCanvas=waveform?document.createElement('canvas'):null;
  const imageContext=imageCanvas?.getContext('2d',{willReadFrequently:true});
  let painted,colors,extent;
  function refresh() {
    const view=state()[kind],plot=view?.plot;
    const bounds=canvas.getBoundingClientRect();
    const size=[Math.max(1,Math.round(bounds.width*devicePixelRatio)),Math.max(1,Math.round(bounds.height*devicePixelRatio))];
    canvas.title=[view?.description,view?.range].filter(Boolean).join('\n');
    canvas.setAttribute('aria-label',canvas.title);
    if(painted===plot&&colors===state().scope_colors&&extent?.every((n,i)=>n===size[i]))return;
    painted=plot;colors=state().scope_colors;extent=size;
    [canvas.width,canvas.height]=size;
    if(!plot)return;
    if(waveform) {
      if(!view.image)return;
      const {width,height,bytes}=view.image;imageCanvas.width=width;imageCanvas.height=height;
      imageContext.putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer,bytes.byteOffset,bytes.byteLength),width,height),0,0);
      context.imageSmoothingEnabled=false;context.drawImage(imageCanvas,0,0,...size);
    } else for(const [channel,bins] of plot) {
      context.fillStyle=`rgba(${colors[channel].join(',')},.55)`;
      const width=size[0]/bins.length;
      context.beginPath();bins.forEach((value,x)=>context.rect(x*width,size[1]*(1-value),width+.1,size[1]*value));context.fill();
    }
  }
  const observer=new ResizeObserver(refresh);observer.observe(canvas);
  return {node:canvas,refresh,dispose:()=>observer.disconnect()};
}

export function scopeFooter({state,element,button,icon,dispatch,kind}) {
  const send=action=>dispatch({type:'histogram',action});
  const node=element('div','scope-footer'),status=element('span','dim'),clipping=element('div','scope-clipping');
  status.dataset.scopeControl='status';node.append(status);
  for(const name of ['shadows','highlights']) {
    const control=button('',()=>send({type:name,enabled:!state().histogram[name]}));
    control.dataset.scopeControl=name;control.append(icon(`tonal-${name}`));clipping.append(control);
  }
  node.append(clipping);
  return {node,refresh(){
    const view=state()[kind];if(!view)return;
    status.textContent=status.title=view.status;
    [...clipping.children].forEach((control,i)=>{
      const name=control.dataset.scopeControl;control.title=state().histogram.labels[i+1]??'';
      control.setAttribute('aria-label',control.title);control.setAttribute('aria-pressed',String(state().histogram[name]));
    });
  }};
}

export function createScope(options) {
  const {state,app,element,dropdown,dispatch,waveform=false,tonal=false}=options;
  const kind=tonal?'tonal_histogram':waveform?'waveform':'histogram',copy=liveCopy(app,'catalog').native_copy;
  const root=element('div','scope-control');root.dataset.scope=kind;
  root.classList.toggle('scope-waveform',waveform);root.classList.toggle('scope-tonal',tonal);
  const toolbar=element('div','scope-toolbar'),source=dropdown(()=>copy.sampler.source,value=>dispatch({type:'histogram',action:{type:'source',index:Number(value)}})),channel=dropdown(()=>copy.color.channel,value=>dispatch({type:'histogram',action:{type:waveform?'waveform_channel':'channel',index:Number(value)}}));
  source.dataset.scopeControl='source';channel.dataset.scopeControl='channel';
  toolbar.append(source,channel);toolbar.hidden=tonal;
  const graph=element('div','scope-graph'),axis=element('div','scope-axis'),lower=element('span'),upper=element('span');
  const log=element('label','scope-log'),logToggle=element('input','panel-check'),logText=element('span');
  logToggle.type='checkbox';logToggle.dataset.scopeControl='log';log.append(logToggle,logText);log.hidden=tonal;
  logToggle.onchange=()=>dispatch({type:'histogram',action:{type:waveform?'waveform_logarithmic':'logarithmic',enabled:logToggle.checked}});
  const plot=scopeGraph({state,element,kind,waveform}),footer=scopeFooter({...options,kind});
  axis.append(lower,upper);graph.append(plot.node,axis);root.append(toolbar,graph,log,footer.node);
  const choices=(select,labels,index)=>{
    select.update({choices:labels.map((label,value)=>({value,label})),value:index});
  };
  let intrinsic;
  return {node:root,dispose:plot.dispose,refresh(){
    const view=state()[kind];if(!view)return;
    choices(source,view.sources,view.source);choices(channel,view.channels,view.channel);
    [lower.textContent,upper.textContent]=view.axis;logToggle.checked=view.logarithmic;logText.textContent=view.labels[0]??'';
    plot.refresh();footer.refresh();
    const next=JSON.stringify([view.sources,view.channels,view.axis,view.status,view.labels[0]]);
    if(next!==intrinsic){intrinsic=next;options.contentChanged?.();}
  }};
}
