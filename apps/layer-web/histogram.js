import {liveCopy} from './localization.js';

export function scopeGraph({state,element,kind,waveform=false,height=160}) {
  const canvas=element('canvas');canvas.dataset.scopeControl='chart';canvas.setAttribute('role','img');
  const context=canvas.getContext('2d',{willReadFrequently:true}),imageCanvas=waveform?document.createElement('canvas'):null;
  const imageContext=imageCanvas?.getContext('2d',{willReadFrequently:true});
  let painted,colors,extent;
  function refresh() {
    const view=state()[kind],plot=view?.plot;
    const size=[Math.max(1,Math.round(canvas.clientWidth*devicePixelRatio)),Math.round(height*devicePixelRatio)];
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

export function scopeFooter({state,element,button,icon,dispatch,kind,logarithmic=false}) {
  const send=action=>dispatch({type:'histogram',action});
  const node=element('div','scope-footer'),status=element('span','dim'),clipping=element('div','scope-clipping');
  status.dataset.scopeControl='status';node.append(status);
  const log=element('input'),text=element('span');log.type='checkbox';log.dataset.scopeControl='log';
  if(logarithmic) {const label=element('label','scope-log');label.append(log,text);node.append(label);}
  log.onchange=()=>send({type:kind==='waveform'?'waveform_logarithmic':'logarithmic',enabled:log.checked});
  for(const name of ['shadows','highlights']) {
    const control=button('',()=>send({type:name,enabled:!state().histogram[name]}));
    control.dataset.scopeControl=name;control.append(icon(`tonal-${name}`));clipping.append(control);
  }
  node.append(clipping);
  return {node,refresh(){
    const view=state()[kind];if(!view)return;
    status.textContent=status.title=view.status;log.checked=view.logarithmic;text.textContent=view.labels[0]??'';
    [...clipping.children].forEach((control,i)=>{
      const name=control.dataset.scopeControl;control.title=state().histogram.labels[i+1]??'';
      control.setAttribute('aria-label',control.title);control.setAttribute('aria-pressed',String(state().histogram[name]));
    });
  }};
}

export function createScope(options) {
  const {state,app,element,dispatch,waveform=false,tonal=false}=options;
  const kind=tonal?'tonal_histogram':waveform?'waveform':'histogram',copy=liveCopy(app,'catalog').native_copy;
  const root=element('div','scope-control');root.dataset.scope=kind;
  root.classList.toggle('scope-waveform',waveform);root.classList.toggle('scope-tonal',tonal);
  const toolbar=element('div','scope-toolbar'),source=element('select'),channel=element('select');
  source.dataset.scopeControl='source';channel.dataset.scopeControl='channel';
  source.onchange=()=>dispatch({type:'histogram',action:{type:'source',index:Number(source.value)}});
  channel.onchange=()=>dispatch({type:'histogram',action:{type:waveform?'waveform_channel':'channel',index:Number(channel.value)}});
  toolbar.append(source,channel);toolbar.hidden=tonal;
  const graph=element('div','scope-graph'),axis=element('div','scope-axis'),lower=element('span'),upper=element('span');
  const plot=scopeGraph({state,element,kind,waveform,height:tonal?120:160}),footer=scopeFooter({...options,kind,logarithmic:!tonal});
  axis.append(lower,upper);graph.append(plot.node,axis);root.append(toolbar,graph,footer.node);
  const choices=(select,labels,index)=>{
    const key=JSON.stringify(labels);
    if(select.dataset.schema!==key){select.dataset.schema=key;select.replaceChildren(...labels.map((label,i)=>{const option=element('option','',label);option.value=i;return option;}));}
    select.value=index;
  };
  return {node:root,dispose:plot.dispose,refresh(){
    const view=state()[kind];if(!view)return;
    choices(source,view.sources,view.source);choices(channel,view.channels,view.channel);
    source.title=copy.sampler.source;source.setAttribute('aria-label',source.title);
    channel.title=copy.color.channel;channel.setAttribute('aria-label',channel.title);
    [lower.textContent,upper.textContent]=view.axis;plot.refresh();footer.refresh();
  }};
}
