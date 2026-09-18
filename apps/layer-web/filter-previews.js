// One clock and native image cache per editor, shared by docked/drawer views.
// Rust decides generations, admission, batching, cancellation and retention.
const editors=new WeakMap();
export function filterPreviewView(app,geometry,present){
  let editor=editors.get(app);
  if(!editor){
    const views=new Set(),images=new Map();let key=null,timer=null,frame=null;
    const cancelWake=()=>{clearTimeout(timer);cancelAnimationFrame(frame);timer=frame=null;};
    const poll=()=>{
      cancelWake();let wait=200;
      try{
        const visible=document.hidden?[]:[...views].map(v=>v.geometry()).filter(Boolean);
        const width=Math.max(80,...visible.map(v=>v.width)),height=Math.max(1,...visible.map(v=>v.height));
        const ids=[...new Set(visible.flatMap(v=>v.filters))];
        const status=app.poll_filter_previews(ids,{key,rows:[...images.keys()]},width,height,performance.now());
        if(key!==status.key){images.clear();key=status.key;}
        const retained=new Set(status.retained);
        for(const id of images.keys())if(!retained.has(id))images.delete(id);
        if(status.atlas){
          const [,width,totalHeight,filters]=status.atlas,bytes=status.bytes,height=totalHeight/filters.length,rowBytes=width*height*4;
          // Independent rows let shared retention release an evicted atlas.
          filters.forEach((id,i)=>images.set(id,new ImageData(new Uint8ClampedArray(bytes.subarray(i*rowBytes,(i+1)*rowBytes)),width,height)));
        }
        for(const view of views)view.present(images,key);
        if(status.error)console.warn("Filter previews unavailable",status.error);
        wait=status.wait_ms;
      }catch(error){wait=1000;console.warn("Filter previews unavailable",error);}
      if(views.size){if(wait===0)frame=requestAnimationFrame(poll);else timer=setTimeout(poll,wait);}
    };
    document.addEventListener("visibilitychange",poll);
    editor={views,poll,dispose(){cancelWake();document.removeEventListener("visibilitychange",poll);}};
    editors.set(app,editor);
  }
  const view={geometry,present};editor.views.add(view);
  editor.poll();
  return ()=>{
    editor.views.delete(view);editor.poll();
    if(!editor.views.size){editor.dispose();editors.delete(app);}
  };
}
