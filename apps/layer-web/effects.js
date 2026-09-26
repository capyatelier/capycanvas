import {strokeRecordingControl} from './stroke-recording.js';
import {colorButton, colorCss} from './color-controls.js';
import {filterPreviewView} from './filter-previews.js';
// Views of the shared Rust effect/property schema; no filter-specific UI logic.
export function createEffectPanels({app,wake,catalog,state,panels,element,button,icon,dispatch,numberField,contentChanged,splitPicker=false,message}) {
  const send=action=>dispatch({type:"effect",action});
  const adjustments=element("div","filter-picker");adjustments.dataset.control="adjustments";
  const pickerHeader=element("div","filter-picker-header"),category=element("select"),search=element("input"),list=element("div","filter-picker-list");
  const pickerAction=action=>dispatch({type:"filter_picker",action});
  const searchButton=button("",()=>{pickerAction({op:"toggle_search"});if(!search.hidden)search.focus();});searchButton.append(icon("search"));
  category.onchange=()=>pickerAction({op:"category",category:category.value||null});
  search.type="search";search.maxLength=120;search.oninput=()=>pickerAction({op:"search",query:search.value});
  search.onkeydown=e=>{e.stopPropagation();if(e.key==="Escape"){e.preventDefault();pickerAction({op:"toggle_search"});}};
  const categoryIcon=element("span","filter-category-icon");
  pickerHeader.append(categoryIcon,category,search,searchButton);adjustments.append(pickerHeader,list);
  const types=element("div","filter-types");types.dataset.control="filter_types";
  const typeList=element("div","filter-type-list"),cancel=button("Cancel",()=>send({op:"cancel_filter"}),"cancel-filter");
  types.append(typeList,cancel);panels.get("filter_types")?.append(types);
  pickerHeader.hidden=splitPicker;
  const rows=new Map();let visibleIds=null,catalogRevision=null;
  function refreshPicker(){
    const s=state(),picker=s.filter_picker;
    if(catalogRevision!==s.filter_catalog_revision){
      catalogRevision=s.filter_catalog_revision;visibleIds=null;rows.clear();
      typeList.replaceChildren(...s.filter_categories.map(c=>{const b=button("",()=>pickerAction({op:"category",category:c.id}),"filter-type");b.dataset.category=c.id??"";b.append(icon(c.icon),element("span","",c.label));return b;}));
      category.replaceChildren(...s.filter_categories.map(c=>{const option=element("option","",c.label);option.value=c.id??"";return option;}));
    }
    category.hidden=picker.search!=null;category.value=picker.category??"";
    categoryIcon.hidden=category.hidden;
    const categoryGlyph=s.filter_categories.find(c=>c.id===picker.category)?.icon??"adjustments";
    if(categoryIcon.firstChild?.dataset.asset!==categoryGlyph)categoryIcon.replaceChildren(icon(categoryGlyph));
    search.hidden=picker.search==null;search.placeholder=picker.search_label;searchButton.title=picker.search_label;
    if(search.value!==(picker.search??""))search.value=picker.search??"";
    for(const b of typeList.children)b.setAttribute("aria-pressed",String(b.dataset.category===(picker.category??"")));
    for(const [id,row] of rows)row.node.setAttribute("aria-pressed",String(picker.selected===id));
    const ids=s.adjustments.map(c=>c.id).join(",");if(ids===visibleIds)return;visibleIds=ids;
    const children=[];let section;
    for(const choice of s.adjustments){
      if(!splitPicker&&section!==choice.category){const heading=element("h3","filter-category",choice.category_label);heading.prepend(icon(choice.category_icon));children.push(heading);section=choice.category;}
      let row=rows.get(choice.id);
      if(!row){
        const node=button("",()=>dispatch(choice.action),"filter-row"),canvas=element("canvas"),label=element("span","",choice.label);
        // Already-rasterized GPU previews: display tiny bitmap rows, without
        // allocating another accelerated drawing context for every list item.
        canvas.getContext("2d",{willReadFrequently:true});
        node.dataset.effect=choice.id;node.title=choice.tooltip;canvas.setAttribute("aria-hidden","true");canvas.draggable=false;
        label.prepend(icon(choice.icon));
        if(choice.animated){const mark=icon("animation");mark.classList.add("filter-animation");mark.setAttribute("aria-hidden","true");label.prepend(mark);}
        node.append(canvas,label);row={node,canvas,key:null};rows.set(choice.id,row);
      }
      row.node.setAttribute("aria-pressed",String(picker.selected===choice.id));children.push(row.node);
    }
    if(!children.length)children.push(element("p","dim",picker.empty_label));list.replaceChildren(...children);contentChanged("adjustments");
  }
  const disposePreviews=filterPreviewView(app,wake,()=>{
    if(!adjustments.isConnected||!adjustments.clientHeight)return null;
    const width=Math.min(512,Math.max(80,Math.round(Math.max(1,list.clientWidth-12)*devicePixelRatio))),height=Math.min(128,Math.round(40*devicePixelRatio));
    const viewport=panels.get("adjustments").getBoundingClientRect(),filters=[];
    for(const choice of state().adjustments){const rect=rows.get(choice.id)?.node.getBoundingClientRect();if(rect?.height&&rect.bottom>Math.max(0,viewport.top)&&rect.top<Math.min(innerHeight,viewport.bottom))filters.push(choice.id);}
    return {filters,width,height};
  },(images,key)=>{
    for(const [id,row] of rows){
      const pixels=images.get(id);
      if(!pixels){if(row.key!==null){row.canvas.width=0;row.key=null;}continue;}
      if(row.key===key)continue;
      row.canvas.width=pixels.width;row.canvas.height=pixels.height;
      row.canvas.getContext("2d").putImageData(pixels,0,0);row.key=key;
    }
  });
  panels.get("adjustments").append(adjustments);
  const properties=element("div","effect-properties");properties.dataset.control="properties";
  const title=element("h3"),body=element("div","property-controls");properties.append(title,body);panels.get("properties").append(properties);
  const stats=element("div","renderer-stats");stats.dataset.control="stats";panels.get("stats").append(stats);
  const recordButton=button("Start stroke recording",()=>{}); stats.append(recordButton);
  const disposeRecording=strokeRecordingControl(app,recordButton,message);
  let schema,fields=new Map(),metricLabels=[];
  const svg=(tag,attributes={})=>{const e=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [k,v] of Object.entries(attributes))e.setAttribute(k,v);return e;};
  const chart=svg("svg",{viewBox:"0 0 200 46",class:"renderer-chart","aria-hidden":"true"});
  const line=svg("path",{fill:"none",stroke:"currentColor","stroke-width":1.5}),budget=svg("path",{stroke:"currentColor","stroke-dasharray":"3 3",opacity:.3});chart.append(budget,line);
  const statsTimer = setInterval(()=>{
    if(!stats.isConnected||!stats.getClientRects().length||document.hidden)return;
    const view=app.renderer_stats();
    if(!metricLabels.length){for(const [index,metric] of view.rows.entries()){const row=element("div","property-row"),value=element("span","numeric");row.title=metric.description;row.append(element("span","",metric.label),value);stats.insertBefore(row,recordButton);metricLabels.push(value);if(index+1===Number(view.chart_after_rows))stats.insertBefore(chart,recordButton);}contentChanged("stats");}
    view.rows.forEach((r,i)=>metricLabels[i].textContent=r.value);chart.setAttribute("aria-label",view.chart_label);
    const max=Math.max(view.budget_ms,...view.samples)*1.1,y=ms=>46*(1-ms/max);
    budget.setAttribute("d",`M0 ${y(view.budget_ms)}H200`);
    line.setAttribute("d",view.samples.map((ms,i)=>`${i?"L":"M"}${i*200/119} ${y(ms)}`).join(" "));
  },200);
  function row(label,input){const r=element("label","property-row"),text=element("span","",label);text.title=label;r.append(text,input);return r;}
  function curveEditor(layer,key){
    const graph=svg("svg",{viewBox:"0 0 200 200",preserveAspectRatio:"none",class:"curve-editor",role:"img","aria-label":"Tone curve"});
    const grid=svg("path",{d:"M50 0V200M100 0V200M150 0V200M0 50H200M0 100H200M0 150H200",stroke:"currentColor",opacity:.2});
    const path=svg("path",{fill:"none",stroke:"currentColor","stroke-width":1.5}),points=svg("g",{fill:"currentColor"});graph.append(grid,path,points);
    const white=svg("path",{fill:"none",stroke:"currentColor","stroke-dasharray":"3 3",opacity:.7}),axis=svg("text",{x:5,y:13,fill:"currentColor","font-size":10});graph.append(white,axis);
    let control,drag,pressed,origin,lastTap=null;
    const node=element("div","curve-field"),reset=button("",()=>{lastTap=null;send({op:"reset",layer,key});});
    reset.append(icon("reset"));reset.title="Reset curve";reset.dataset.action="curve-reset";node.append(graph,reset);
    node.title="Click to add a point and drag to shape the curve. Double-click a point or drag it off the graph to remove it.";
    const position=e=>{const b=graph.getBoundingClientRect();return [(e.clientX-b.left)/b.width,1-(e.clientY-b.top)/b.height];};
    const nearest=p=>control.value.value.findIndex(q=>Math.hypot((q[0]-p[0])*graph.clientWidth,(q[1]-p[1])*graph.clientHeight)<12);
    const gesture=(phase,index,point,remove=false)=>send({op:"gesture",phase,action:{op:"curve_point",layer,key,index,point,remove}});
    graph.onpointerdown=e=>{
      if(e.button)return;e.preventDefault();e.stopPropagation();const point=position(e);let index=nearest(point);
      pressed=index>=0;origin=[e.clientX,e.clientY];
      if(pressed)gesture("down",index,control.value.value[index]);
      else{gesture("down",null,point);index=control.value.value.findIndex(q=>Math.abs(q[0]-point[0])<.002);}
      drag=index;graph.setPointerCapture(e.pointerId);
    };
    graph.onpointermove=e=>{if(drag==null||drag<0)return;e.preventDefault();gesture("move",drag,position(e));};
    graph.onpointerup=e=>{
      if(drag==null)return;
      if(drag<0)gesture("cancel",null,[0,0]);
      else{
        const tapped=pressed&&Math.hypot(e.clientX-origin[0],e.clientY-origin[1])<4;
        const double=tapped&&lastTap?.index===drag&&e.timeStamp-lastTap.time<400;
        lastTap=tapped&&!double?{index:drag,time:e.timeStamp}:null;
        gesture("up",drag,position(e),double);
      }
      drag=null;
    };
    graph.onpointercancel=()=>{if(drag==null)return;gesture("cancel",drag<0?null:drag,[0,0]);drag=null;};
    graph.oncontextmenu=e=>{e.preventDefault();e.stopPropagation();const index=nearest(position(e));if(index>=0)send({op:"curve_point",layer,key,index,point:[0,0],remove:true});};
    return {node,update:c=>{reset.hidden=!c.modified;control=c;const {curve_max:peak,curve_white:reference}=state().layer_properties;white.setAttribute("d",peak?`M${200*reference} 0V200M0 ${200-200*reference}H200`:"");axis.textContent=peak?`SDR white · 0 EV | ${peak} · +${Math.log2(peak)} EV`:"Output / Input";path.setAttribute("d",c.plot.map(([x,y],i)=>`${i?"L":"M"}${x*200} ${(1-y)*200}`).join(" "));points.replaceChildren(...c.value.value.map(([x,y])=>svg("circle",{cx:x*200,cy:(1-y)*200,r:3.5})));}};
  }
  function refresh(){
    refreshPicker();
    const view=state().layer_properties;title.textContent=view.title;title.title=view.description;
    const next=JSON.stringify([String(view.layer),view.controls.map(c=>[c.key,c.kind,c.label,c.section,c.color_action])],(_,v)=>typeof v==="bigint"?String(v):v);
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
        else if(c.kind.kind==="color"){const n=colorButton({app,label:c.label,element,button,change,current:()=>`${state().document_file.epoch}:${state().layer_properties.layer}`});const line=c.color_action?element("div","paper-color-property"):row(c.label,n.node);let bucket;
          if(c.color_action){bucket=button("",()=>dispatch(c.color_action));bucket.dataset.action="paper-color-bucket";bucket.title="Use selected color";bucket.append(icon("fill"));line.append(n.node,bucket);}
          field={node:line,update:c=>n.update(c.value.value),disable:x=>{n.disable(x);if(bucket)bucket.disabled=x;}};}
        else if(c.kind.kind==="gradient")field=gradientEditor(view.layer,c.key);
        if(field){if(c.kind.kind!=="curve")body.append(field.node);fields.set(c.key,field);}
      }
      contentChanged("properties");
    }
    body.classList.toggle("disabled",!view.enabled);
    for(const c of view.controls){const field=fields.get(c.key);field?.update(c);field?.disable?.(!view.enabled);}
  }
  return {refresh,dispose(){disposePreviews();clearInterval(statsTimer);disposeRecording();}};
  function gradientEditor(layer,key) {
    const node=element("div","gradient-editor"),bar=element("div","gradient-ramp"),stopsRow=element("div","gradient-stops");
    let stops=[],selected=0,rampKey;
    const change=(index,position,color=null,remove=false)=>send({op:"gradient_stop",layer,key,index,position,color,remove});
    const color=colorButton({app,label:"Color stop",element,button,change:value=>change(selected,stops[selected].position,value),current:()=>`${state().document_file.epoch}:${state().layer_properties.layer}:${selected}`});
    const position=numberField(catalog.opacity,"Position",value=>change(selected,value));
    const opacity=numberField(catalog.opacity,"Opacity",value=>change(selected,stops[selected].position,{...stops[selected].color,rgba:[...stops[selected].color.rgba.slice(0,3),value]}));
    const remove=button("",()=>{const i=selected;selected=Math.max(0,i-1);change(i,0,null,true);});remove.append(icon("minus"));remove.title="Remove color stop";
    const reset=button("",()=>send({op:"reset",layer,key}));reset.append(icon("reset"));reset.title="Reset gradient";
    const controls=element("div","property-row");controls.append(element("span","","Color"),color.node,remove,reset);
    node.append(bar,stopsRow,position,controls,opacity);
    bar.onclick=e=>{const b=bar.getBoundingClientRect(),p=Math.max(0,Math.min(1,(e.clientX-b.left)/b.width));selected=stops.filter(s=>s.position<p).length;change(null,p);};
    function update(c) {
      stops=c.value.value;selected=Math.min(selected,stops.length-1);
      const nextRamp=JSON.stringify([state().colors.rgb_space,stops]);
      if(nextRamp!==rampKey){
        rampKey=nextRamp;
        const samples=app.color_ui({type:"gradient",stops,document_space:state().colors.rgb_space});
        bar.style.background=`linear-gradient(to right,${samples.map((p,i)=>`${colorCss(p)} ${i*100/(samples.length-1)}%`).join(",")})`;
      }
      const previews=app.color_ui({type:"preview",colors:stops.map(s=>s.color)});
      stopsRow.replaceChildren(...stops.map((s,i)=>{const b=button("",()=>{selected=i;update(c);});b.style.left=`${s.position*100}%`;b.style.background=colorCss(previews[i]);b.classList.toggle("selected",selected===i);b.title=`Color stop ${i+1}`;return b;}));
      const s=stops[selected];color.update(s.color);
      position.update(s.position);position.setDisabled(selected===0||selected===stops.length-1);remove.disabled=selected===0||selected===stops.length-1;
      opacity.update(s.color.rgba[3]);
    }
    return {node,update};
  }
}
