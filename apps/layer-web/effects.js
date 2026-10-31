import {createScope,scopeGraph,scopeFooter} from './histogram.js';
import {liveCopy,bindCopy} from './localization.js';
import {composingKey} from "./text-input.js";
import {gestureNumberField} from "./numeric.js";
import {createCurveEditor} from "./curve-editor.js";
import {strokeRecordingControl} from './stroke-recording.js';
import {colorButton, colorCss} from './color-controls.js';
import {filterPreviewView} from './filter-previews.js';
import {gradientEditor} from './gradient.js';
import {chooseTexture,paperThumbnail} from './paper-picker.js';
// Views of the shared Rust effect/property schema; no filter-specific UI logic.
export function createEffectPanels({app,wake,catalog,state,panels,element,button,icon,dropdown,dispatch,numberField,contentChanged,splitPicker=false,message,asset}) {
  const copy=liveCopy(app,"catalog").native_copy.color,common=liveCopy(app,"bootstrap_view").common;
  const send=action=>dispatch({type:"effect",action});
  const adjustments=element("div","filter-picker");adjustments.dataset.control="adjustments";
  const pickerAction=action=>dispatch({type:"filter_picker",action});
  const pickerHeader=element("div","filter-picker-header"),category=dropdown(()=>state().filter_categories.find(c=>c.id===state().filter_picker.category)?.label??'',value=>pickerAction({op:'category',category:value||null})),search=element("input"),list=element("div","filter-picker-list");
  const searchButton=button("",()=>{pickerAction({op:"toggle_search"});if(!search.hidden)search.focus();});searchButton.append(icon("search"));
  search.type="search";search.maxLength=120;search.oninput=()=>pickerAction({op:"search",query:search.value});
  search.onkeydown=e=>{if(composingKey(e))return;e.stopPropagation();if(e.key==="Escape"){e.preventDefault();pickerAction({op:"toggle_search"});}};
  const categoryIcon=element("span","filter-category-icon");
  pickerHeader.append(categoryIcon,category,search,searchButton);adjustments.append(pickerHeader,list);
  const types=element("div","filter-types");types.dataset.control="filter_types";
  const typeList=element("div","filter-type-list"),cancel=button(()=>common.cancel,()=>send({op:"cancel_filter"}),"cancel-filter");
  types.append(typeList,cancel);panels.get("filter_types")?.append(types);
  pickerHeader.hidden=splitPicker;
  const rows=new Map();let visibleIds=null,catalogRevision=null,pickerLanguage;
  function refreshPicker(){
    const s=state(),picker=s.filter_picker;
    if(catalogRevision!==s.filter_catalog_revision){
      catalogRevision=s.filter_catalog_revision;visibleIds=null;rows.clear();
      typeList.replaceChildren(...s.filter_categories.map(c=>{const b=button("",()=>pickerAction({op:"category",category:c.id}),"filter-type");b.dataset.category=c.id??"";b.append(icon(c.icon),element("span","",c.label));return b;}));
    }
    category.hidden=picker.search!=null;category.update({choices:s.filter_categories.map(c=>({value:c.id??'',label:c.label})),value:picker.category??''});
    categoryIcon.hidden=category.hidden;
    const categoryGlyph=s.filter_categories.find(c=>c.id===picker.category)?.icon??"adjustments";
    if(categoryIcon.firstChild?.dataset.asset!==categoryGlyph)categoryIcon.replaceChildren(icon(categoryGlyph));
    search.hidden=picker.search==null;search.placeholder=picker.search_label;searchButton.title=picker.search_label;
    if(search.value!==(picker.search??""))search.value=picker.search??"";
    for(const b of typeList.children)b.setAttribute("aria-pressed",String(b.dataset.category===(picker.category??"")));
    for(const [id,row] of rows)row.node.setAttribute("aria-pressed",String(picker.selected===id));
    if(pickerLanguage!==s.localization?.generation){
      pickerLanguage=s.localization?.generation;
      for(const c of s.filter_categories){
        const b=[...typeList.children].find(b=>b.dataset.category===(c.id??""));
        if(b)b.querySelector("span").textContent=c.label;
      }
      for(const choice of s.adjustments){
        const row=rows.get(choice.id);
        if(row){row.label.textContent=choice.label;row.node.title=choice.tooltip;}
        const heading=[...list.children].find(node=>node.dataset.category===choice.category);
        if(heading)heading.querySelector("span").textContent=choice.category_label;
      }
      const empty=list.querySelector("p");if(empty)empty.textContent=picker.empty_label;
    }
    const ids=s.adjustments.map(c=>c.id).join(",");if(ids===visibleIds)return;visibleIds=ids;
    const children=[];let section;
    for(const choice of s.adjustments){
      if(!splitPicker&&section!==choice.category){const heading=element("h3","filter-category");heading.dataset.category=choice.category;heading.append(icon(choice.category_icon),element("span","",choice.category_label));children.push(heading);section=choice.category;}
      let row=rows.get(choice.id);
      if(!row){
        const node=button("",()=>dispatch(choice.action),"filter-row"),label=element("span"),text=element("span","",choice.label);
        const canvas=choice.bundled_preview?null:element("canvas"),preview=canvas??element("img");
        // Already-rasterized GPU previews: display tiny bitmap rows, without
        // allocating another accelerated drawing context for every list item.
        if(canvas)canvas.getContext("2d",{willReadFrequently:true});else{preview.src=asset(`filter-previews/${choice.id}.webp`);preview.alt="";}
        node.dataset.effect=choice.id;node.title=choice.tooltip;preview.setAttribute("aria-hidden","true");preview.draggable=false;
        label.append(icon(choice.icon));
        if(choice.animated){const mark=icon("animation");mark.classList.add("filter-animation");mark.setAttribute("aria-hidden","true");label.append(mark);}
        label.append(text);
        node.append(preview,label);row={node,canvas,label:text,key:null};rows.set(choice.id,row);
      }
      row.node.setAttribute("aria-pressed",String(picker.selected===choice.id));children.push(row.node);
    }
    if(!children.length)children.push(element("p","dim",picker.empty_label));list.replaceChildren(...children);contentChanged("adjustments");
  }
  const disposePreviews=filterPreviewView(app,wake,()=>{
    if(!adjustments.isConnected||!adjustments.clientHeight)return null;
    const viewport=panels.get("adjustments").getBoundingClientRect(),filters=[];
    for(const choice of state().adjustments){if(choice.bundled_preview)continue;const rect=rows.get(choice.id)?.node.getBoundingClientRect();if(rect?.height&&rect.bottom>Math.max(0,viewport.top)&&rect.top<Math.min(innerHeight,viewport.bottom))filters.push(choice.id);}
    return {filters};
  },(images,key)=>{
    for(const [id,row] of rows){
      if(!row.canvas)continue;
      const pixels=images.get(id);
      if(!pixels){if(row.key!==null){row.canvas.width=0;row.key=null;}continue;}
      if(row.key===key)continue;
      row.canvas.width=pixels.width;row.canvas.height=pixels.height;
      row.canvas.getContext("2d").putImageData(pixels,0,0);row.key=key;
    }
  });
  panels.get("adjustments").append(adjustments);
  const properties=element("div","effect-properties");properties.dataset.control="properties";
  const title=element("h3"),page=dropdown(()=>state().layer_properties.pages.find(p=>p.id===state().layer_properties.page)?.label??'',value=>send({op:'select_page',layer:state().layer_properties.layer,page:value})),body=element("div","property-controls");page.dataset.propertiesPage="";const toolbar=element("div","property-toolbar"),actions=element("div","property-actions");
  const header=element("div","properties-header"),layerType=element("span","properties-layer-type");
  title.id="properties-layer-name";layerType.id="properties-layer-type";
  toolbar.append(page,actions);properties.append(title,header,toolbar,body);panels.get("properties").append(properties);
  const addFilter=dropdown(()=>state().layer_properties.add_filter?.title??'');addFilter.prepend(icon('add-filter'));
  addFilter.id="properties-add-filter";header.append(layerType,addFilter);
  const tonal=createScope({state,app,element,button,icon,dropdown,dispatch,tonal:true});properties.insertBefore(tonal.node,body);
  let actionKey,actionNodes=[],groupMenus=[];
  function refreshActions(view) {
    const key=JSON.stringify(view.actions.map(a=>[a.action,a.icon,a.group?.id]),(_,v)=>typeof v==="bigint"?String(v):v);
    if(key!==actionKey) {
      actionKey=key;groupMenus.forEach(menu=>menu.remove());groupMenus=[];actionNodes=[];actions.replaceChildren();
      const groups=new Set();
      const create=(index,parent,caption)=>{
        const action=view.actions[index],node=button("",()=>{if(parent.popover)parent.hidePopover();send(action.action);});
        node.dataset.propertyAction=index;
        if(action.icon&&!caption)node.append(icon(action.icon.replace(/^layer-/,"").replace(/-symbolic$/,"")));
        else node.append(element('span'));
        parent.append(node);actionNodes.push({index,node,caption:caption||!action.icon});return node;
      };
      view.actions.forEach((action,index)=>{
        if(["lookup_preset","import_lookup"].includes(action.action.op))return;
        if(!action.group){create(index,actions,false);return;}
        if(groups.has(action.group.id))return;groups.add(action.group.id);
        const menu=element('div','toolbar-choice-menu toolbar-editor-popover panel');menu.popover='auto';menu.setAttribute('role','menu');
        const opener=button('',()=>{
          menu.showPopover();const a=opener.getBoundingClientRect(),b=menu.getBoundingClientRect();
          menu.style.left=`${Math.max(6,Math.min(a.left,innerWidth-b.width-6))}px`;
          menu.style.top=`${Math.max(6,Math.min(a.bottom+4,innerHeight-b.height-6))}px`;
        });
        opener.dataset.propertyActionGroup=action.group.id;
        if(action.icon)opener.append(icon(action.icon.replace(/^layer-/,"").replace(/-symbolic$/,"")));else opener.append(element('span'));
        opener.setAttribute('aria-haspopup','menu');actions.append(opener);properties.append(menu);groupMenus.push(menu);
        actionNodes.push({index,node:opener,group:true,caption:!action.icon});
        view.actions.forEach((choice,i)=>{if(choice.group?.id===action.group.id)create(i,menu,true).setAttribute('role','menuitem');});
      });
    }
    for(const {index,node,caption,group} of actionNodes) {
      const action=view.actions[index],label=group?action.group.label:action.label;
      node.title=label;node.setAttribute('aria-label',label);node.disabled=!view.enabled;if(caption)node.querySelector('span').textContent=label;
    }
    toolbar.hidden=page.hidden&&!actions.children.length;
  }
  const resource=element("div","property-row property-resource"),resourceChoice=dropdown(()=>state().layer_properties.resource_label,value=>{const action=state().layer_properties.actions[Number(value)];if(action)send(action.action);}),resourceImport=button("",()=>{
    const action=state().layer_properties.actions.find(a=>a.action.op==="import_lookup");if(action)send(action.action);
  });
  resourceChoice.dataset.propertyResource="";resourceImport.dataset.action="import-lookup";
  resource.append(resourceChoice,resourceImport);properties.insertBefore(resource,body);
  const stats=element("div","renderer-stats");stats.dataset.control="stats";panels.get("stats").append(stats);
  const recordButton=button("Start stroke recording",()=>{}); stats.append(recordButton);
  const disposeRecording=strokeRecordingControl(app,recordButton,message);
  let schema,fieldOwner,fields=new Map(),metricLabels=[];
  const svg=(tag,attributes={})=>{const e=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [k,v] of Object.entries(attributes))e.setAttribute(k,v);return e;};
  const chart=svg("svg",{viewBox:"0 0 200 46",class:"renderer-chart","aria-hidden":"true"});
  const line=svg("path",{fill:"none",stroke:"currentColor","stroke-width":1.5}),budget=svg("path",{stroke:"currentColor","stroke-dasharray":"3 3",opacity:.3});chart.append(budget,line);
  const statsTimer = setInterval(()=>{
    if(!stats.isConnected||!stats.getClientRects().length||document.hidden)return;
    const view=app.renderer_stats();
    if(!metricLabels.length){for(const [index,metric] of view.rows.entries()){const row=element("div","property-row"),value=element("span","numeric");row.title=metric.description;row.append(element("span","",metric.label),value);stats.insertBefore(row,recordButton);metricLabels.push({row,label:row.firstChild,value});if(index+1===Number(view.chart_after_rows))stats.insertBefore(chart,recordButton);}contentChanged("stats");}
    view.rows.forEach((r,i)=>{const nodes=metricLabels[i];if(nodes.row.title!==r.description)nodes.row.title=r.description;if(nodes.label.textContent!==r.label)nodes.label.textContent=r.label;if(nodes.value.textContent!==r.value)nodes.value.textContent=r.value;});
    if(chart.getAttribute("aria-label")!==view.chart_label)chart.setAttribute("aria-label",view.chart_label);
    const max=Math.max(view.budget_ms,...view.samples)*1.1,y=ms=>46*(1-ms/max);
    const budgetPath=`M0 ${y(view.budget_ms)}H200`,samplePath=view.samples.map((ms,i)=>`${i?"L":"M"}${i*200/119} ${y(ms)}`).join(" ");
    if(budget.getAttribute("d")!==budgetPath)budget.setAttribute("d",budgetPath);
    if(line.getAttribute("d")!==samplePath)line.setAttribute("d",samplePath);
  },200);
  function row(label,input){const r=element("label","property-row"),text=element("span","",label);if(typeof label!=="function")text.title=label;r.append(text,input);return r;}
  function numberEditor(numeric,label,request,valueOnly=false){
    const number=gestureNumberField({numberField,numeric,label,request,send,gesture:(phase,action)=>({op:"gesture",phase,action}),valueOnly});
    number.onReset=()=>send({...request(),op:"reset"});
    return number;
  }
  function curveEditor(layer,key,initial){
    const histogram=scopeGraph({state,element,kind:'tonal_histogram'});
    const footer=scopeFooter({state,element,button,icon,dispatch,kind:'tonal_histogram'});
    const model=c=>({controls:c.curve,points:c.value.value,plot:c.plot,marker:null,modified:c.modified});
    const editor=createCurveEditor({element,button,icon,numberField,dispatch,target:{kind:"effect",layer,key},
      initial:model(initial),label:initial.label,enabled:()=>{const view=state().layer_properties;return view.enabled&&!view.controls.find(c=>c.key===key)?.disabled;},histogram,footer,reset:true});
    return {...editor,update:c=>editor.update(model(c),c.label)};
  }
  function refresh(){
    refreshPicker();
    const view=state().layer_properties;title.textContent=view.name;title.title=view.title;
    layerType.textContent=view.layer_type;layerType.title=view.layer_type;
    header.hidden=!view.layer_type&&!view.add_filter;
    addFilter.hidden=!view.add_filter;
    addFilter.update({menu:()=>state().layer_properties.add_filter});
    resource.hidden=view.resource_label==null;
    if(!resource.hidden){
      const choices=view.actions.flatMap((a,i)=>a.action.op==="lookup_preset"?[[i,a.label]]:[]);
      if(view.resource_selection==null)choices.push([-1,view.resource_name]);
      resourceChoice.update({choices:choices.map(([value,label])=>({value,label})),value:view.resource_selection??-1,disabled:!view.enabled});resourceChoice.title=view.resource_name;
      const action=view.actions.find(a=>a.action.op==="import_lookup");resourceImport.title=action.label;resourceImport.setAttribute("aria-label",action.label);resourceImport.disabled=!view.enabled;
      const glyph=action.icon.replace(/^layer-/,"").replace(/-symbolic$/,"");
      if(resourceImport.firstChild?.dataset.asset!==glyph)resourceImport.replaceChildren(icon(glyph));
    }
    page.update({choices:view.pages.map(p=>({value:p.id,label:p.label})),value:view.page??'',disabled:!view.enabled});page.hidden=view.pages.length<2;refreshActions(view);
    tonal.node.hidden=!view.histogram;if(view.histogram)tonal.refresh();
    const owner=`${state().document_file.epoch}:${view.layer}`;
    const fieldSchema=c=>JSON.stringify([c.kind.kind,c.kind.numeric,c.kind.options?.length,c.color_action],(_,v)=>typeof v==="bigint"?String(v):v);
    const next=JSON.stringify([owner,view.controls.map(c=>[c.key,fieldSchema(c),c.section_id])]);
    if(schema!==next){
      schema=next;
      const controls=new Map(view.controls.map(c=>[c.key,c]));
      for(const [key,field] of fields)if(fieldOwner!==owner||!controls.has(key)||field.schema!==fieldSchema(controls.get(key))){field.dispose?.();field.node.remove();fields.delete(key);}
      fieldOwner=owner;
      let section=JSON.stringify(null);const children=[];
      for(const [index,c] of view.controls.entries()){
        const identity=JSON.stringify(c.section_id);
        if(section!==identity){
          section=identity;
          if(c.section)children.push(element("h4","property-section",()=>state().layer_properties.controls.find(v=>v.key===c.key)?.section??""));
        }
        const change=value=>send({op:"set",layer:view.layer,key:c.key,value:{kind:c.kind.kind,value}});let field=fields.get(c.key);
        if(!field){
        if(c.kind.kind==="number") {const n=numberEditor(c.kind.numeric,()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",()=>({op:"number",layer:view.layer,key:c.key}));field={node:n,update:c=>n.update(c.value.value),disable:x=>n.setDisabled(x),dispose:()=>n.dispose()};}
        else if(c.kind.kind==="curve") {
          let curve=curveEditor(view.layer,c.key,c),domain=JSON.stringify(c.curve.domain);
          const node=element('div');node.append(curve.node);
          field={node,update(c){
            const next=JSON.stringify(c.curve.domain);
            if(next!==domain){domain=next;curve.dispose();curve=curveEditor(view.layer,c.key,c);node.replaceChildren(curve.node);}
            else curve.update(c);
          },refreshHistogram:()=>curve.refreshHistogram(),dispose:()=>curve.dispose()};
        }
        else if(c.kind.kind==="toggle"){const n=element("input");n.type="checkbox";n.onchange=()=>change(n.checked);field={node:row(()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",n),update:c=>n.checked=c.value.value,disable:x=>n.disabled=x};}
        else if(c.kind.kind==="choice"&&c.paper){const thumbnail=paperThumbnail({app,element,icon,size:28}),name=element("span","property-texture-name"),n=button("",()=>chooseTexture({app,layer:state().layer_properties.layer,state,dispatch,element,button,icon,message}),"property-texture");
          n.id="property-texture";n.append(thumbnail.node,name);
          field={node:row(()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",n),update:c=>{const tile=c.paper.tiles[c.value.value];thumbnail.show(tile?.id??null);name.textContent=tile?.label??"";n.title=n.ariaLabel=c.label;n.setAttribute("aria-description",tile?.label??"");},disable:x=>n.disabled=x};}
        else if(c.kind.kind==="choice"){const label=()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??'',n=dropdown(label,value=>change(Number(value)));field={node:row(label,n),update:c=>n.update({choices:c.kind.options.map((label,value)=>({value,label})),value:c.value.value}),disable:x=>n.disabled=x};}
        else if(c.kind.kind==="color"){const n=colorButton({app,opaque:c.kind.opaque,label:()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",element,button,change,current:()=>`${state().document_file.epoch}:${state().layer_properties.layer}`});let input=n.node,bucket;
          if(c.color_action){bucket=button("",()=>dispatch(c.color_action));bucket.dataset.action=`${c.key.replaceAll("_","-")}-bucket`;bindCopy(bucket,()=>copy.use_selected,"title");bindCopy(bucket,()=>copy.use_selected,"ariaLabel");bucket.append(icon("fill"));input=element("div","color-action-property");input.append(n.node,bucket);}
          field={node:row(()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",input),update:c=>n.update(c.value.value),disable:x=>{n.disable(x);if(bucket)bucket.disabled=x;},dispose:()=>n.dispose()};}
        else if(c.kind.kind==="gradient")field=gradientEditor({app,element,button,icon,dropdown,dispatch});
        if(field){field.node.dataset.propertyKey=c.key;field.schema=fieldSchema(c);fields.set(c.key,field);}
        }
        if(field)children.push(field.node);
      }
      const retained=new Set(children);for(const child of [...body.children])if(!retained.has(child))child.remove();
      let before=body.firstChild;for(const child of children){if(child===before)before=before.nextSibling;else body.insertBefore(child,before);}
      contentChanged("properties");
    }
    body.classList.toggle("disabled",!view.enabled);
    for(const c of view.controls){const field=fields.get(c.key);field?.update(c);field?.disable?.(!view.enabled||!!c.disabled);field?.node.classList.toggle("disabled",view.enabled&&!!c.disabled);}
  }
  return {refresh,refreshHistograms(){if(state().layer_properties.histogram)tonal.refresh();for(const field of fields.values())field.refreshHistogram?.();},dispose(){tonal.dispose();groupMenus.forEach(menu=>menu.remove());for(const field of fields.values())field.dispose?.();disposePreviews();clearInterval(statsTimer);disposeRecording();}};
}
