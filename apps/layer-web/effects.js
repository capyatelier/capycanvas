let previewRequest=0n;
const previewJobs=new Map();
// Views of the shared Rust effect/property schema; no filter-specific UI logic.
// Host I/O only: Rust validates the filenames, definitions, shaders and atomic
// publication. This also accepts external packages without rebuilding Wasm.
export async function fetchFilterPackage(app, manifestUrl, mode, moduleUrl=name=>new URL(name,manifestUrl)) {
  const read=async url=>{const response=await fetch(url,{cache:"no-cache"});if(!response.ok)throw new Error(`Filter package: HTTP ${response.status}`);return response.text();};
  const manifest=await read(manifestUrl),names=app.filter_package_modules(manifest);
  const modules=Object.fromEntries(await Promise.all(names.map(async name=>[name,await read(moduleUrl(name))])));
  return app.load_filter_package(manifest,modules,mode);
}
export function createEffectPanels({app,catalog,state,panels,element,button,icon,dispatch,numberField,contentChanged}) {
  const send=action=>dispatch({type:"effect",action});
  const adjustments=element("div","filter-picker");adjustments.dataset.control="adjustments";
  const pickerHeader=element("div","filter-picker-header"),category=element("select"),search=element("input"),list=element("div","filter-picker-list");
  const pickerAction=action=>dispatch({type:"filter_picker",action});
  const searchButton=button("",()=>{pickerAction({op:"toggle_search"});if(!search.hidden)search.focus();});searchButton.append(icon("search"));
  category.onchange=()=>pickerAction({op:"category",category:category.value||null});
  search.type="search";search.maxLength=120;search.oninput=()=>pickerAction({op:"search",query:search.value});
  search.onkeydown=e=>{e.stopPropagation();if(e.key==="Escape"){e.preventDefault();pickerAction({op:"toggle_search"});}};
  pickerHeader.append(category,search,searchButton);adjustments.append(pickerHeader,list);
  const rows=new Map();let visibleIds=null,catalogRevision=null,pending=null,polling=false;
  function refreshPicker(){
    const s=state(),picker=s.filter_picker;
    if(catalogRevision!==s.filter_catalog_revision){
      catalogRevision=s.filter_catalog_revision;visibleIds=null;rows.clear();
      category.replaceChildren(...s.filter_categories.map(c=>{const option=element("option","",c.label);option.value=c.id??"";return option;}));
    }
    category.hidden=picker.search!=null;category.value=picker.category??"";
    search.hidden=picker.search==null;search.placeholder=picker.search_label;searchButton.title=picker.search_label;
    if(search.value!==(picker.search??""))search.value=picker.search??"";
    const ids=s.adjustments.map(c=>c.id).join(",");if(ids===visibleIds)return;visibleIds=ids;
    const children=[];let section;
    for(const choice of s.adjustments){
      if(section!==choice.category){children.push(element("h3","filter-category",choice.category_label));section=choice.category;}
      let row=rows.get(choice.id);
      if(!row){
        const node=button("",()=>dispatch(choice.action),"filter-row"),canvas=element("canvas"),label=element("span","",choice.label);
        // Already-rasterized GPU previews: display tiny bitmap rows, without
        // allocating another accelerated drawing context for every list item.
        canvas.getContext("2d",{willReadFrequently:true});
        node.dataset.effect=choice.id;node.title=choice.tooltip;canvas.setAttribute("aria-hidden","true");canvas.draggable=false;
        if(choice.animated){const mark=icon("animation");mark.classList.add("filter-animation");mark.setAttribute("aria-hidden","true");label.prepend(mark);}
        node.append(canvas,label);row={node,canvas,key:null};rows.set(choice.id,row);
      }
      children.push(row.node);
    }
    if(!children.length)children.push(element("p","dim",picker.empty_label));list.replaceChildren(...children);contentChanged("adjustments");
  }
  const previewKey=()=>{
    const width=Math.min(512,Math.max(80,Math.round(Math.max(1,list.clientWidth-12)*devicePixelRatio))),height=Math.min(128,Math.round(40*devicePixelRatio));
    return {width,height,key:`${state().document_file.epoch}:${app.filter_preview_revision().map(String).join(":")}:${width}:${height}`};
  };
  function pollPreviews(){
    polling=false;if(!pending)return;
    try {
      const result=app.take_filter_previews();
      if(result){
        const [id]=result, deliver=previewJobs.get(id);
        previewJobs.delete(id); deliver?.(result);
      }
    }catch(error){if(pending)previewJobs.delete(pending.id);pending=null;console.warn("Filter previews unavailable",error);}
    if(pending){polling=true;requestAnimationFrame(pollPreviews);}
  }
  const previewTimer = setInterval(()=>{
    if(document.hidden||!adjustments.isConnected||!adjustments.clientHeight)return;
    if(pending){if(!polling){polling=true;requestAnimationFrame(pollPreviews);}return;}
    try {
      const info=previewKey(),viewport=panels.get("adjustments").getBoundingClientRect(),filters=[];
      for(const choice of state().adjustments){const row=rows.get(choice.id),rect=row?.node.getBoundingClientRect();if(rect?.height&&rect.bottom>Math.max(0,viewport.top)&&rect.top<Math.min(innerHeight,viewport.bottom)&&row.key!==info.key)filters.push(choice.id);if(filters.length===8)break;}
      if(filters.length){const id=++previewRequest;if(app.request_filter_previews(id,filters,info.width,info.height)){
        pending={id,key:info.key};
        previewJobs.set(id,result=>{
          const job=pending;pending=null;if(!job||job.key!==previewKey().key)return;
          const [,width,height,filters]=result,bytes=result.bytes,rowHeight=height/filters.length,rowBytes=width*rowHeight*4;
          filters.forEach((id,i)=>{const row=rows.get(id);if(!row)return;row.canvas.width=width;row.canvas.height=rowHeight;
            const pixels=new Uint8ClampedArray(bytes.buffer,bytes.byteOffset+i*rowBytes,rowBytes);
            row.canvas.getContext("2d").putImageData(new ImageData(pixels,width,rowHeight),0,0);row.key=job.key;});
        });
        polling=true;requestAnimationFrame(pollPreviews);
      }}
    }catch(error){console.warn("Filter previews unavailable",error);}
  },200);
  panels.get("adjustments").append(adjustments);
  const properties=element("div","effect-properties");properties.dataset.control="properties";
  const title=element("h3"),body=element("div","property-controls");properties.append(title,body);panels.get("properties").append(properties);
  const stats=element("div","renderer-stats");stats.dataset.control="stats";panels.get("stats").append(stats);
  let schema,fields=new Map(),metricLabels=[];
  const svg=(tag,attributes={})=>{const e=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [k,v] of Object.entries(attributes))e.setAttribute(k,v);return e;};
  const chart=svg("svg",{viewBox:"0 0 200 46",class:"renderer-chart","aria-hidden":"true"});
  const line=svg("path",{fill:"none",stroke:"currentColor","stroke-width":1.5}),budget=svg("path",{stroke:"currentColor","stroke-dasharray":"3 3",opacity:.3});chart.append(budget,line);
  const statsTimer = setInterval(()=>{
    if(!stats.isConnected||!stats.getClientRects().length||document.hidden)return;
    const view=app.renderer_stats();
    if(!metricLabels.length){for(const metric of view.rows){const row=element("div","property-row"),value=element("span","numeric");row.title=metric.description;row.append(element("span","",metric.label),value);stats.append(row);metricLabels.push(value);}stats.append(chart);contentChanged("stats");}
    view.rows.forEach((r,i)=>metricLabels[i].textContent=r.value);chart.setAttribute("aria-label",view.chart_label);
    const max=Math.max(view.budget_ms,...view.samples)*1.1,y=ms=>46*(1-ms/max);
    budget.setAttribute("d",`M0 ${y(view.budget_ms)}H200`);
    line.setAttribute("d",view.samples.map((ms,i)=>`${i?"L":"M"}${i*200/119} ${y(ms)}`).join(" "));
  },200);
  function row(label,input){const r=element("label","property-row"),text=element("span","",label);text.title=label;r.append(text,input);return r;}
  function curveEditor(layer,key){
    const graph=svg("svg",{viewBox:"0 0 200 200",class:"curve-editor",role:"img","aria-label":"Tone curve"});
    const grid=svg("path",{d:"M50 0V200M100 0V200M150 0V200M0 50H200M0 100H200M0 150H200",stroke:"currentColor",opacity:.2});
    const path=svg("path",{fill:"none",stroke:"currentColor","stroke-width":1.5}),points=svg("g",{fill:"currentColor"});graph.append(grid,path,points);
    let control,drag;
    const position=e=>{const b=graph.getBoundingClientRect();return [(e.clientX-b.left)/b.width,1-(e.clientY-b.top)/b.height];};
    const nearest=p=>control.value.value.findIndex(q=>Math.hypot((q[0]-p[0])*graph.clientWidth,(q[1]-p[1])*graph.clientHeight)<12);
    graph.onpointerdown=e=>{
      if(e.button)return;e.preventDefault();e.stopPropagation();const point=position(e);let index=nearest(point);
      if(index<0){send({op:"curve_point",layer,key,index:null,point,remove:false});index=control.value.value.findIndex(q=>Math.abs(q[0]-point[0])<.002);}
      if(index>=0){drag=index;graph.setPointerCapture(e.pointerId);}
    };
    graph.onpointermove=e=>{if(drag==null)return;e.preventDefault();send({op:"curve_point",layer,key,index:drag,point:position(e),remove:false});};
    graph.onpointerup=graph.onpointercancel=()=>{drag=null;};
    graph.oncontextmenu=e=>{e.preventDefault();e.stopPropagation();const index=nearest(position(e));if(index>=0)send({op:"curve_point",layer,key,index,point:[0,0],remove:true});};
    return {node:graph,update:c=>{control=c;path.setAttribute("d",c.plot.map(([x,y],i)=>`${i?"L":"M"}${x*200} ${(1-y)*200}`).join(" "));points.replaceChildren(...c.value.value.map(([x,y])=>svg("circle",{cx:x*200,cy:(1-y)*200,r:3.5})));}};
  }
  function refresh(){
    refreshPicker();
    const view=state().layer_properties;title.textContent=view.title;title.title=view.description;
    const next=JSON.stringify([String(view.layer),view.controls.map(c=>[c.key,c.kind,c.label,c.section])]);
    if(schema!==next){
      schema=next;body.replaceChildren();fields.clear();
      const curves=view.controls.filter(c=>c.kind.kind==="curve");let curveBox;
      if(curves.length){const select=element("select"),stack=element("div","curve-stack");curveBox=stack;
        for(const c of curves){const option=element("option","",c.label);option.value=c.key;select.append(option);}
        select.onchange=()=>{for(const child of stack.children)child.toggleAttribute("hidden",child.dataset.key!==select.value);};body.append(select,stack);
      }
      let section=null;
      for(const [index,c] of view.controls.entries()){
        if(section!==c.section){
          if(index>0)body.append(element("hr","property-divider"));
          section=c.section;
          if(section)body.append(element("h4","property-section",section));
        }
        const change=value=>send({op:"set",layer:view.layer,key:c.key,value:{kind:c.kind.kind,value}});let field;
        if(c.kind.kind==="number") {const n=numberField(c.kind.numeric,c.label,value=>change(value));field={node:n,update:c=>n.update(c.value.value),disable:x=>n.setDisabled(x)};}
        else if(c.kind.kind==="curve"){field=curveEditor(view.layer,c.key);field.node.dataset.key=c.key;field.node.toggleAttribute("hidden",c!==curves[0]);curveBox.append(field.node);}
        else if(c.kind.kind==="toggle"){const n=element("input");n.type="checkbox";n.onchange=()=>change(n.checked);field={node:row(c.label,n),update:c=>n.checked=c.value.value,disable:x=>n.disabled=x};}
        else if(c.kind.kind==="choice"){const n=element("select");c.kind.options.forEach((label,i)=>{const o=element("option","",label);o.value=i;n.append(o);});n.onchange=()=>change(Number(n.value));field={node:row(c.label,n),update:c=>n.value=c.value.value,disable:x=>n.disabled=x};}
        else if(c.kind.kind==="color"){const n=element("input");n.type="color";n.oninput=()=>change([1,3,5].map(i=>parseInt(n.value.slice(i,i+2),16)/255).concat(1));field={node:row(c.label,n),update:c=>n.value="#"+c.value.value.slice(0,3).map(x=>Math.round(x*255).toString(16).padStart(2,"0")).join(""),disable:x=>n.disabled=x};}
        else if(c.kind.kind==="gradient")field=gradientEditor(view.layer,c.key);
        if(field){if(c.kind.kind!=="curve")body.append(field.node);fields.set(c.key,field);}
      }
      contentChanged("properties");
    }
    body.classList.toggle("disabled",!view.enabled);
    for(const c of view.controls){const field=fields.get(c.key);field?.update(c);field?.disable?.(!view.enabled);}
  }
  return {refresh,dispose(){clearInterval(previewTimer);clearInterval(statsTimer);if(pending)previewJobs.delete(pending.id);pending=null;}};
  function gradientEditor(layer,key) {
    const node=element("div","gradient-editor"),bar=element("div","gradient-ramp"),stopsRow=element("div","gradient-stops");
    let stops=[],selected=0;
    const change=(index,position,color=null,remove=false)=>send({op:"gradient_stop",layer,key,index,position,color,remove});
    const color=element("input");color.type="color";color.setAttribute("aria-label","Color stop");
    color.oninput=()=>change(selected,stops[selected].position,[1,3,5].map(i=>parseInt(color.value.slice(i,i+2),16)/255).concat(stops[selected].color[3]));
    const position=numberField(catalog.opacity,"Position",value=>change(selected,value));
    const opacity=numberField(catalog.opacity,"Opacity",value=>change(selected,stops[selected].position,[...stops[selected].color.slice(0,3),value]));
    const remove=button("",()=>{const i=selected;selected=Math.max(0,i-1);change(i,0,null,true);});remove.append(icon("minus"));remove.title="Remove color stop";
    const reset=button("",()=>send({op:"reset",layer,key}));reset.append(icon("undo"));reset.title="Reset gradient";
    const controls=element("div","property-row");controls.append(element("span","","Color"),color,remove,reset);
    node.append(bar,stopsRow,position,controls,opacity);
    bar.onclick=e=>{const b=bar.getBoundingClientRect(),p=Math.max(0,Math.min(1,(e.clientX-b.left)/b.width));selected=stops.filter(s=>s.position<p).length;change(null,p);};
    const rgba=c=>`rgba(${c.slice(0,3).map(v=>v*255).join(",")},${c[3]})`;
    function update(c) {
      stops=c.value.value;selected=Math.min(selected,stops.length-1);
      bar.style.background=`linear-gradient(to right,${stops.map(s=>`${rgba(s.color)} ${s.position*100}%`).join(",")})`;
      stopsRow.replaceChildren(...stops.map((s,i)=>{const b=button("",()=>{selected=i;update(c);});b.style.left=`${s.position*100}%`;b.style.background=rgba(s.color);b.classList.toggle("selected",selected===i);b.title=`Color stop ${i+1}`;return b;}));
      const s=stops[selected];color.value="#"+s.color.slice(0,3).map(x=>Math.round(x*255).toString(16).padStart(2,"0")).join("");
      position.update(s.position);position.setDisabled(selected===0||selected===stops.length-1);remove.disabled=selected===0||selected===stops.length-1;
      opacity.update(s.color[3]);
    }
    return {node,update};
  }
}
