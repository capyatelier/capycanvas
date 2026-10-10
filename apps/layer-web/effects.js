import {createScope,scopeGraph,scopeFooter} from './histogram.js';
import {liveCopy,bindCopy} from './localization.js';
import {composingKey} from "./text-input.js";
import {gestureNumberField} from "./numeric.js";
import {createCurveEditor} from "./curve-editor.js";
import {strokeRecordingControl} from './stroke-recording.js';
import {colorButton, colorCss} from './color-controls.js';
import {filterPreviewView} from './filter-previews.js';
import {gradientEditor} from './gradient.js';
// Views of the shared Rust effect/property schema; no filter-specific UI logic.
export function createEffectPanels({app,wake,catalog,state,panels,element,button,icon,dispatch,numberField,contentChanged,splitPicker=false,message,openMenu}) {
  const copy=liveCopy(app,"catalog").native_copy.color,common=liveCopy(app,"bootstrap_view").common;
  const send=action=>dispatch({type:"effect",action});
  const adjustments=element("div","filter-picker");adjustments.dataset.control="adjustments";
  const pickerHeader=element("div","filter-picker-header"),category=element("select"),search=element("input"),list=element("div","filter-picker-list");
  const pickerAction=action=>dispatch({type:"filter_picker",action});
  const searchButton=button("",()=>{pickerAction({op:"toggle_search"});if(!search.hidden)search.focus();});searchButton.append(icon("search"));
  category.onchange=()=>pickerAction({op:"category",category:category.value||null});
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
    if(pickerLanguage!==s.localization?.generation){
      pickerLanguage=s.localization?.generation;
      for(const c of s.filter_categories){
        const b=[...typeList.children].find(b=>b.dataset.category===(c.id??""));
        if(b)b.querySelector("span").textContent=c.label;
        const option=[...category.options].find(o=>o.value===(c.id??""));
        if(option)option.textContent=c.label;
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
        const node=button("",()=>dispatch(choice.action),"filter-row"),canvas=element("canvas"),label=element("span"),text=element("span","",choice.label);
        // Already-rasterized GPU previews: display tiny bitmap rows, without
        // allocating another accelerated drawing context for every list item.
        canvas.getContext("2d",{willReadFrequently:true});
        node.dataset.effect=choice.id;node.title=choice.tooltip;canvas.setAttribute("aria-hidden","true");canvas.draggable=false;
        label.append(icon(choice.icon),text);
        if(choice.animated){const mark=icon("animation");mark.classList.add("filter-animation");mark.setAttribute("aria-hidden","true");label.prepend(mark);}
        node.append(canvas,label);row={node,canvas,label:text,key:null};rows.set(choice.id,row);
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
  const title=element("h3"),page=element("select"),body=element("div","property-controls");page.dataset.propertiesPage="";page.onchange=()=>send({op:"select_page",layer:state().layer_properties.layer,page:page.value});const toolbar=element("div","property-toolbar"),actions=element("div","property-actions");
  const header=element("div","properties-header"),layerType=element("span","properties-layer-type");
  title.id="properties-layer-name";layerType.id="properties-layer-type";
  toolbar.append(page,actions);properties.append(title,header,toolbar,body);panels.get("properties").append(properties);
  const addFilter=button(()=>state().layer_properties.add_filter?.title??"",()=>openMenu(addFilter));
  addFilter.prepend(icon("add-filter"));addFilter.append(icon("chevron-down"));
  addFilter.id="properties-add-filter";addFilter.setAttribute("aria-haspopup","menu");
  addFilter.menuModel=()=>state().layer_properties.add_filter;header.append(layerType,addFilter);
  const tonal=createScope({state,app,element,button,icon,dispatch,tonal:true});properties.insertBefore(tonal.node,body);
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
  const resource=element("div","property-row property-resource"),resourceChoice=element("select"),resourceImport=button("",()=>{
    const action=state().layer_properties.actions.find(a=>a.action.op==="import_lookup");if(action)send(action.action);
  });
  resourceChoice.dataset.propertyResource="";resourceImport.dataset.action="import-lookup";
  resourceChoice.onchange=()=>{const action=state().layer_properties.actions[Number(resourceChoice.value)];if(action)send(action.action);};
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
    view.rows.forEach((r,i)=>{const nodes=metricLabels[i];nodes.row.title=r.description;nodes.label.textContent=r.label;nodes.value.textContent=r.value;});chart.setAttribute("aria-label",view.chart_label);
    const max=Math.max(view.budget_ms,...view.samples)*1.1,y=ms=>46*(1-ms/max);
    budget.setAttribute("d",`M0 ${y(view.budget_ms)}H200`);
    line.setAttribute("d",view.samples.map((ms,i)=>`${i?"L":"M"}${i*200/119} ${y(ms)}`).join(" "));
  },200);
  function row(label,input){const r=element("label","property-row"),text=element("span","",label);if(typeof label!=="function")text.title=label;r.append(text,input);return r;}
  function numberEditor(numeric,label,request,valueOnly=false){
    const number=gestureNumberField({numberField,numeric,label,request,send,gesture:(phase,action)=>({op:"gesture",phase,action}),valueOnly});
    number.onReset=()=>send({...request(),op:"reset"});
    return number;
  }
  function curveEditor(layer,key,initial){
    const histogram=scopeGraph({state,element,kind:'tonal_histogram',height:200});
    const footer=scopeFooter({state,element,button,icon,dispatch,kind:'tonal_histogram'});
    const model=c=>({controls:c.curve,points:c.value.value,plot:c.plot,marker:null,modified:c.modified});
    const editor=createCurveEditor({element,button,icon,numberField,dispatch,target:{kind:"effect",layer,key},
      initial:model(initial),label:initial.label,enabled:()=>state().layer_properties.enabled,histogram,footer,reset:true});
    return {...editor,update:c=>editor.update(model(c),c.label)};
  }
  function refresh(){
    refreshPicker();
    const view=state().layer_properties;title.textContent=view.name;title.title=view.title;
    layerType.textContent=view.layer_type;layerType.title=view.layer_type;
    header.hidden=!view.layer_type&&!view.add_filter;
    addFilter.hidden=!view.add_filter;
    resource.hidden=view.resource_label==null;
    if(!resource.hidden){
      const choices=view.actions.flatMap((a,i)=>a.action.op==="lookup_preset"?[[i,a.label]]:[]);
      if(view.resource_selection==null)choices.push([-1,view.resource_name]);
      const key=JSON.stringify(choices);
      if(resourceChoice.dataset.schema!==key){resourceChoice.dataset.schema=key;resourceChoice.replaceChildren(...choices.map(([i,label])=>{const option=element("option","",label);option.value=i;return option;}));}
      resourceChoice.value=view.resource_selection??-1;resourceChoice.title=view.resource_name;resourceChoice.setAttribute("aria-label",view.resource_label);resourceChoice.disabled=!view.enabled;
      const action=view.actions.find(a=>a.action.op==="import_lookup");resourceImport.title=action.label;resourceImport.setAttribute("aria-label",action.label);resourceImport.disabled=!view.enabled;
      const glyph=action.icon.replace(/^layer-/,"").replace(/-symbolic$/,"");
      if(resourceImport.firstChild?.dataset.asset!==glyph)resourceImport.replaceChildren(icon(glyph));
    }
    const pages=JSON.stringify(view.pages.map(p=>p.id));
    if(page.dataset.schema!==pages){page.dataset.schema=pages;page.replaceChildren(...view.pages.map(p=>{const option=element("option","",()=>state().layer_properties.pages.find(v=>v.id===p.id)?.label??"");option.value=p.id;return option;}));}
    page.hidden=view.pages.length<2;page.value=view.page??"";page.disabled=!view.enabled;refreshActions(view);
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
          if(index>0)children.push(element("hr","property-divider"));
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
        else if(c.kind.kind==="choice"){const n=element("select");c.kind.options.forEach((label,i)=>{const o=element("option","",()=>state().layer_properties.controls.find(v=>v.key===c.key)?.kind.options[i]??"");o.value=i;n.append(o);});n.onchange=()=>change(Number(n.value));field={node:row(()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",n),update:c=>n.value=c.value.value,disable:x=>n.disabled=x};}
        else if(c.kind.kind==="color"){const n=colorButton({app,opaque:c.kind.opaque,label:()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",element,button,change,current:()=>`${state().document_file.epoch}:${state().layer_properties.layer}`});let input=n.node,bucket;
          if(c.color_action){bucket=button("",()=>dispatch(c.color_action));bucket.dataset.action=`${c.key.replaceAll("_","-")}-bucket`;bindCopy(bucket,()=>copy.use_selected,"title");bindCopy(bucket,()=>copy.use_selected,"ariaLabel");bucket.append(icon("fill"));input=element("div","color-action-property");input.append(n.node,bucket);}
          field={node:row(()=>state().layer_properties.controls.find(v=>v.key===c.key)?.label??"",input),update:c=>n.update(c.value.value),disable:x=>{n.disable(x);if(bucket)bucket.disabled=x;},dispose:()=>n.dispose()};}
        else if(c.kind.kind==="gradient")field=gradientEditor({app,element,button,icon,dispatch});
        if(field){field.node.dataset.propertyKey=c.key;field.schema=fieldSchema(c);fields.set(c.key,field);}
        }
        if(field)children.push(field.node);
      }
      const retained=new Set(children);for(const child of [...body.children])if(!retained.has(child))child.remove();
      let before=body.firstChild;for(const child of children){if(child===before)before=before.nextSibling;else body.insertBefore(child,before);}
      contentChanged("properties");
    }
    body.classList.toggle("disabled",!view.enabled);
    for(const c of view.controls){const field=fields.get(c.key);field?.update(c);field?.disable?.(!view.enabled);}
  }
  return {refresh,refreshHistograms(){if(state().layer_properties.histogram)tonal.refresh();for(const field of fields.values())field.refreshHistogram?.();},dispose(){tonal.dispose();groupMenus.forEach(menu=>menu.remove());for(const field of fields.values())field.dispose?.();disposePreviews();clearInterval(statsTimer);disposeRecording();}};
}
