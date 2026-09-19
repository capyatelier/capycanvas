import assert from 'node:assert/strict';

// Read actual submitted swapchain pixels, before the browser presents them.
// COPY_SRC and the interceptors exist only for this check; production canvases
// retain their normal usage. One row per surface bounds readback memory.
export async function checkHdrDisplay({evaluate}, hdr) {
  const samples=await evaluate(`(async()=>{
    const canvases=[layerApp.canvas,...document.querySelectorAll('.navigator-surface')];
    const contexts=new Map(canvases.map(canvas=>{
      const context=canvas.getContext('webgpu'),config=context?.getConfiguration();
      return [context,config?{config,name:canvas===layerApp.canvas?'Canvas':'Navigator'}:null];
    }).filter(([,v])=>v));
    const getTexture=GPUCanvasContext.prototype.getCurrentTexture,submit=GPUQueue.prototype.submit;
    const jobs=[];let pending=null;
    for(const [context,{config}]of contexts)context.configure({...config,usage:config.usage|GPUTextureUsage.COPY_SRC});
    GPUCanvasContext.prototype.getCurrentTexture=function(){
      const texture=getTexture.call(this),entry=contexts.get(this);
      if(entry&&!entry.captured){entry.captured=true;pending={texture,entry};}
      return texture;
    };
    GPUQueue.prototype.submit=function(commands){
      submit.call(this,commands);
      if(!pending)return;
      const {texture,entry}=pending;pending=null;
      const {device,format}=entry.config,half=format==='rgba16float';
      const row=Math.ceil(texture.width*(half?8:4)/256)*256;
      const buffer=device.createBuffer({size:row,usage:GPUBufferUsage.COPY_DST|GPUBufferUsage.MAP_READ});
      const encoder=device.createCommandEncoder();
      encoder.copyTextureToBuffer({texture,origin:{x:0,y:Math.floor(texture.height/2)}},{buffer,bytesPerRow:row},{width:texture.width,height:1});
      submit.call(this,[encoder.finish()]);
      jobs.push((async()=>{
        try{
          await buffer.mapAsync(GPUMapMode.READ);const data=new DataView(buffer.getMappedRange());
          const decode=h=>(h&0x8000?-1:1)*((h>>10&31)?(1+(h&1023)/1024)*2**((h>>10&31)-15):(h&1023)*2**-24);
          let min=Infinity,max=-Infinity;
          for(let x=0;x<texture.width;x++)for(let c=0;c<3;c++){
            const v=half?decode(data.getUint16((x*4+c)*2,true)):data.getUint8(x*4+c)/255;
            min=Math.min(min,v);max=Math.max(max,v);
          }
          return {surface:entry.name,format,toneMapping:entry.config.toneMapping?.mode??'standard',min,max,bytes:row};
        }finally{buffer.destroy();}
      })());
    };
    try{
      layerApp.wake();const started=performance.now();
      while(jobs.length<contexts.size){if(performance.now()-started>15000)throw Error('Missing surface submission');await new Promise(r=>setTimeout(r,20));}
      return await Promise.all(jobs);
    }finally{
      GPUCanvasContext.prototype.getCurrentTexture=getTexture;GPUQueue.prototype.submit=submit;
      for(const [context,{config}]of contexts)context.configure(config);
      layerApp.wake();
    }
  })()`);
  assert.ok(samples.some(s=>s.surface==='Canvas')&&samples.some(s=>s.surface==='Navigator'));
  for(const sample of samples){
    assert.equal(sample.toneMapping,hdr?'extended':'standard');
    assert.ok(hdr?sample.max>1:sample.max<=1,JSON.stringify(sample));
  }
  return samples;
}
