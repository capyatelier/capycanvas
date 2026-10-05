export const json = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? Number(v) : v);
export const rgba = values => `rgba(${values.slice(0,3).map(v => v * 255).join(",")},${values[3] ?? 1})`;

export function wheelPainter({canvas, worker, request, hueStops, repaint}) {
  const ctx=canvas.getContext("2d",{willReadFrequently:true});
  const field=document.createElement("canvas"),ring=document.createElement("canvas");
  const fieldContext=field.getContext("2d",{willReadFrequently:true}),ringContext=ring.getContext("2d",{willReadFrequently:true});
  let fieldJob=null,fieldPending=null,fieldEpoch=0,fieldGeometry='',fieldKey='',ringKey='',disposed=false;
  function installField(bytes,side,key){
    if(field.width!==side)field.width=field.height=side;
    fieldContext.putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer,bytes.byteOffset,bytes.byteLength),side,side),0,0);
    fieldKey=key;
  }
  async function renderField(){
    if(fieldJob||!fieldPending||disposed)return;
    const job=fieldPending;fieldPending=null;fieldJob=job;
    try {
      const bytes=await worker({operation:'color-field',metadata:job.metadata,buffers:[]});
      if(!disposed&&job.epoch===fieldEpoch){
        if(fieldPending?.key===job.key)fieldPending=null;
        installField(bytes,job.side,job.key);repaint();
      }
    } catch(error) { if(!disposed)console.error('Color field preview',error); }
    finally {fieldJob=null;renderField();}
  }
  return {
    invalidate(){fieldEpoch++;fieldPending=null;},
    dispose(){disposed=true;fieldPending=null;fieldEpoch++;},
    paint(view,side,pixels){
      if(canvas.width!==pixels){canvas.width=canvas.height=pixels;}
      ctx.setTransform(1,0,0,1,0,0);ctx.clearRect(0,0,pixels,pixels);ctx.scale(pixels/side,pixels/side);
      const g=view.geometry,[cx,cy]=g.center.map(v=>v*side),inner=g.inner*side,outer=g.outer*side;
      const fieldPixels=view.shape==="circle"?Math.ceil(side):pixels;
      const fieldLayout=json([view.rgb_space,view.shape,view.rendition]);
      if(fieldLayout!==fieldGeometry){fieldGeometry=fieldLayout;fieldEpoch++;fieldPending=null;}
      const key=json([view.rgb_space,view.shape,view.wheel_components[0],view.intensity,view.rendition,fieldPixels]);
      if(key!==fieldKey) {
        if(fieldJob?.key===key&&fieldJob.epoch===fieldEpoch)fieldPending=null;
        else if(fieldPending?.key!==key||fieldPending.epoch!==fieldEpoch){
          fieldPending={key,side:fieldPixels,metadata:request(fieldPixels),epoch:fieldEpoch};renderField();
        }
      } else if(fieldJob||fieldPending){fieldPending=null;fieldEpoch++;}
      ctx.save();
      if(view.shape==="circle"){ctx.beginPath();ctx.arc(cx,cy,g.disc_radius*side,0,2*Math.PI);ctx.clip();}
      else if(view.shape==="square"){const [x,y,w]=g.square.map(v=>v*side);ctx.beginPath();ctx.roundRect(x,y,w,w,Math.min(6,side*.02));ctx.clip();}
      if(field.width)ctx.drawImage(field,0,0,side,side);ctx.restore();
      // The ring depends on the color model and size, never the selected hue.
      // Retain its raster so a drag only repaints the changing field and markers.
      const nextRing=json([view.rgb_space,view.shape,pixels,side,g,view.wheel_hue_start_degrees]);
      if(nextRing!==ringKey){
        ringKey=nextRing;ring.width=ring.height=pixels;ringContext.scale(pixels/side,pixels/side);
        const hue=ringContext.createConicGradient(view.wheel_hue_start_degrees*Math.PI/180,cx,cy);
        hueStops(view).forEach(stop=>hue.addColorStop(stop.offset,rgba(stop.color)));
        ringContext.strokeStyle=hue;ringContext.lineWidth=outer-inner;ringContext.beginPath();ringContext.arc(cx,cy,(inner+outer)/2,0,Math.PI*2);ringContext.stroke();
      }
      ctx.drawImage(ring,0,0,side,side);
      const radius=Math.min(10,Math.max(6,side*.04));
      for(const [p,fill] of [[view.wheel_hue_marker,rgba(view.wheel_hue_color)],[view.wheel_marker,rgba(view.marker_color)]]) {
        ctx.beginPath();ctx.arc(p[0]*side,p[1]*side,radius,0,2*Math.PI);ctx.fillStyle=fill;ctx.fill();ctx.strokeStyle="rgba(0,0,0,.5)";ctx.lineWidth=4;ctx.stroke();ctx.strokeStyle="white";ctx.lineWidth=2;ctx.stroke();
      }
    },
  };
}

export function wheelPicker(canvas, hit, pick) {
  let contact=null;
  const send=e=>{const r=canvas.getBoundingClientRect();pick({op:"pick_wheel",part:contact.part,size:r.width,point:[e.clientX-r.x,e.clientY-r.y]});};
  canvas.addEventListener("pointerdown",e=>{if(e.button!==0)return;const r=canvas.getBoundingClientRect(),part=hit(r.width,[e.clientX-r.x,e.clientY-r.y]);if(!part)return;contact={id:e.pointerId,part};canvas.setPointerCapture(e.pointerId);e.preventDefault();send(e);});
  canvas.addEventListener("pointermove",e=>{if(contact?.id===e.pointerId)send(e);});
  for(const name of ["pointerup","pointercancel","lostpointercapture"])canvas.addEventListener(name,e=>{if(contact?.id===e.pointerId)contact=null;});
}

export const hueStopCache = app => {
  const stops=new Map();
  return view=>{const key=`${view.rgb_space}:${view.shape}`;if(!stops.has(key))stops.set(key,app.color_ui({type:'hue_stops',shape:view.shape,space:view.rgb_space}));return stops.get(key);};
};
export const wheelHit = app => (size,point,shape) => app.color_ui({type:'wheel_hit',size,point,shape});
