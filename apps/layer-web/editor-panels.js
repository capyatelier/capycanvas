import { createNavigationControls } from "./navigation-controls.js";
import {createScope} from './histogram.js';
import {liveCopy,bindCopy} from './localization.js';
import { createRasterWorker } from './raster-worker-client.js';
import { chooseColor } from './color-editor.js';
import { wheelPainter, wheelPicker, hueStopCache, wheelHit, rgba } from './color-wheel.js';
import { colorTouchInput } from './color-touch.js';
import { createRangeControl } from './range-control.js';
import { actionField, checklistField, choiceField } from './toolbar-components.js';
import {gradientEditor} from './gradient.js';
// DOM widgets for shared editor models. Rust owns tool/color/geometry policy.
export function createEditorPanels({ selectionUi, app, wasmModule, state, element, button, icon, dropdown, numberField, dispatch, asset, wake, applyChange, contentChanged, showContext }) {
  const catalog=liveCopy(app,"catalog"),copy=catalog.native_copy;
  const updates = new Map(), navigators = new Map(), pendingPaints = new Set();
  let positioning = 0, nextNavigator = 1;
  const fieldWorker = createRasterWorker(wasmModule);
  const displayColors=()=>state().layer_tools.mask_editing?.colors??state().colors;
  const color = action => dispatch({ type: "color", action });
  const control = (kind, readToolSet = null) => {
    const root = element("div", `${kind.replaceAll("_", "-")}-control`);
    root.dataset.control = kind;
    let refresh;
    if (["brushes", "brush_sets", "sculpt_sets", "tools"].includes(kind)) refresh = toolSet(root, kind, readToolSet);
    else if (kind === "tool_settings") refresh = toolSettings(root);
    else if (kind === "color_wheel") refresh = colorWheel(root);
    else if (kind === "navigator") refresh = navigatorPanel(root);
    else if (["histogram","waveform"].includes(kind)) {
      const scope=createScope({state,app,element,button,icon,dropdown,dispatch,waveform:kind==="waveform",contentChanged:()=>contentChanged(kind)});
      root.append(scope.node);root.disposeSettings=scope.dispose;refresh=scope.refresh;
    }
    else return null;
    updates.set(root, refresh); refresh();
    root.refreshEditor = refresh;
    root.disposeEditor = () => { updates.delete(root); root.navigatorDispose?.(); root.disposeSettings?.(); };
    return root;
  };
  function toolSet(root, panel, readView) {
    let key = "", rows = [];
    return () => {
      const view = readView?.() || state().tool_panels[panel] || state().tool_set;
      root.classList.toggle('tonal-tool-list', state().tool_extra.some(o=>o.Choice?.id==='tonal-tones'));
      const next = JSON.stringify([view.groups, view.subtools].map(items => items.map(({label,selected,enabled,disabled_reason,...item})=>item)),(_,value)=>typeof value==='bigint'?String(value):value);
      if (next !== key) {
        key = next; rows = []; root.replaceChildren();
        for (const [kind, items] of [["groups",view.groups], ["subtools",view.subtools]]) {
          if (!items.length) continue;
          if (kind === "subtools" && view.groups.length) root.append(element("div", "tile-divider column-divider tool-set-divider"));
          const list = element("div", `tool-${kind}`); root.append(list);
          for (const [index,item] of items.entries()) {
            const read=()=>(readView?.()||state().tool_panels[panel]||state().tool_set)[kind][index];
            const node = button("", () => dispatch(item.action), "tool-choice-button");
            node.bubbleChoice = item.action;
            if(app.bubble_grab({type:'choice',action:item.action}))node.dataset.bubbleSource='';
            if(item.action.type==='color_picker')node.dataset.pickerStyle='';
            let image;
            if (item.preview != null) {
              image = element("img", "brush-preview"); image.alt = ""; image.draggable = false;
              node.append(image); node.dataset.brush = item.preview;
            } else if (kind === "groups") node.append(icon(item.icon));
            const label = element("span", "tool-choice-label");
            if (item.preview != null || kind === "subtools") label.append(icon(item.icon));
            label.append(element("span", "tool-choice-name", ()=>read()?.label??''));
            node.append(label); list.append(node); rows.push({node,image,kind,index});
          }
        }
        contentChanged(panel);
      }
      for (const {node,image,kind,index} of rows) {
        const item=view[kind][index];node.disabled=item.enabled===false;node.classList.toggle('unavailable-tool',item.enabled!==false&&!!item.disabled_reason);node.dataset.toolChoice=item.label;node.setAttribute("aria-label",item.label);node.title=item.disabled_reason??app.action_tooltip(item.label,item.action);
        if(image){const source=asset(`brush-previews/${item.preview}-${state().theme}.webp`);if(image.getAttribute('src')!==source)image.src=source;}
        const pressed=String(item.selected);if(node.getAttribute("aria-pressed")!==pressed)node.setAttribute("aria-pressed",pressed);
      }
    };
  }
  function toolSettings(root) {
    let key = "", numbers = [], actions = [], actionGroups = [], range, choices = [], sampler, gradient;
    root.disposeSettings = () => { gradient?.dispose();gradient=null; range?.dispose(); numbers.forEach(([,n])=>n.dispose()); };
    return () => {
      const s = state(),editing=s.layer_tools.editing_layer;
      const owner=[String(s.document_file.epoch),String(s.toolbar_context_generation),s.brush.preset,String(editing?.id??''),editing?.mask_selected,String(editing?.mask_id??'')];
      const picking = ['pick_visible','pick_layer'].includes(s.layer_tools.tool);
      const compact = s.tool_extra.some(o=>o.Choice?.id==='tonal-tones');
      root.classList.toggle('tonal-settings', compact);
      root.classList.toggle('picker-settings',picking);
      if (picking) {
        const picker=s.color_picker;
        const next=JSON.stringify([owner,picker.can_sample_layer,picker.sample_sizes]);
        if(key!==next){
          key=next;root.disposeSettings();range=null;numbers=[];root.replaceChildren();
          const choice=(id,label,values,select)=>{
            const row=element('label','picker-setting'),input=dropdown(label,select);row.append(element('span','',label),input);
            input.dataset.pickerSetting=id;input.update({choices:values.map(([value,label])=>({value,label}))});root.append(row);return input;
          };
          sampler={
            source:choice('source',()=>copy.sampler.source,(picker.can_sample_layer?[false,true]:[false]).map(value=>[value,()=>value?copy.sampler.selected_layer:copy.sampler.visible_color]),
              value=>dispatch({type:'color_picker',action:{kind:'source',layer:value==='true'}})),
            size:choice('sample_size',()=>copy.sampler.sample_size,picker.sample_sizes.map(value=>[value,()=>copy.sampler.sizes.find(([width])=>width===value)[1]]),
              value=>dispatch({type:'set_color_sample_size',width:Number(value)})),
          };
          contentChanged('tool_settings');
        }
        sampler.source.update({value:picker.layer});sampler.size.update({value:picker.sample_width});return;
      }
      const next = JSON.stringify([owner,s.tool_settings.map(f=>[f.id,f.numeric,f.group_id]),s.tool_actions.map(a=>[a.command,a.checkable]),s.tool_extra.map(o=>o.Choice?[o.Choice.id,o.Choice.items.map(i=>[i.icon,i.action])]:o.Checklist?[o.Checklist.id,o.Checklist.items.map(i=>i.action)]:["gradient",o.Gradient.gradient.destination])],(k,v)=>k==='endpoint_labels'?undefined:typeof v==='bigint'?String(v):v);
      if (next !== key) {
        const previousBody=root.querySelector('.tool-settings-body'),focused=document.activeElement;
        const retainedChoice=previousBody?.contains(focused)&&focused.matches('[data-tool-action-group]')?focused:null;
        const retainedRows=new Map();
        key = next; root.disposeSettings(); range=null;
        for(const child of [...root.children])if(child!==previousBody)child.remove();
        numbers=[]; actions=[]; actionGroups=[]; choices=[]; let group="";
        const modes=element('div','tool-mode-section'),body=element('div','tool-settings-body');
        const modeGroups=new Map();
        for(const spec of s.tool_actions.filter(a=>a.segmented)){
          let bar=modeGroups.get(spec.group);
          if(!bar){bar=element('div','selection-modes');bar.setAttribute('role','group');bindCopy(bar,()=>catalog.tool_action_groups.find(g=>g.id===spec.group).label,'ariaLabel');bar.dataset.toolActionGroup=spec.group;modeGroups.set(spec.group,bar);modes.append(bar);}
          const node=button('',()=>dispatch({type:'invoke',command:spec.command}),'tool-setting-action');node.dataset.toolAction=spec.command;bar.append(node);actions.push([spec,node,null]);
        }
        if(modes.children.length){
          root.insertBefore(modes,previousBody);
          if(!compact&&modeGroups.has("selection-mode"))modes.append(selectionUi.menuButton(()=>copy.tool_controls.selection_menu,'selection'));
        }
        if(!previousBody)root.append(body);
        const choiceRow=(id,change)=>{
          const row=element('label','tool-action-choice'),retained=retainedChoice?.dataset.toolActionGroup===id;
          const input=retained?retainedChoice:dropdown('',value=>input.toolChoiceChange(value)),caption=retained?retainedChoice.parentElement.querySelector('span'):element('span');
          if(retained)retainedRows.set(row,retainedChoice.parentElement);else row.append(caption,input);
          input.toolChoiceChange=change;input.dataset.toolActionGroup=id;
          return {row,input,update(label,items,value){caption.textContent=label;input.update({label,choices:items,value,disabled:items.every(i=>i.enabled===false)});}};
        };
        const beside=new Map();let grouped;
        for (const option of s.tool_extra) {
          if(option.Gradient){gradient=gradientEditor({app,element,button,icon,dropdown,dispatch});body.append(gradient.node);continue;}
          if(option.Checklist){const field=checklistField({element,button,icon},option.Checklist,dispatch);choices.push([option.Checklist.id,field]);body.append(field.row);continue;}
          const spec=option.Choice;
          if(!spec.segmented){
            const field=choiceRow(spec.id,value=>dispatch(state().tool_extra.find(o=>o.Choice?.id===spec.id).Choice.items[Number(value)].action));
            field.input.dataset.toolExtraChoice=spec.id;
            choices.push([spec.id,{update(option){const spec=option.Choice;field.update(spec.label,spec.items.map((item,index)=>({value:String(index),label:item.label,enabled:item.enabled})),String(spec.items.findIndex(i=>i.selected)));}}]);body.append(field.row);continue;
          }
          const field=choiceField({app,element,button,icon},spec,dispatch),bar=field.row;
          bar.dataset.toolChoiceBar=spec.id;
          [...bar.children].forEach((node,index)=>node.dataset.toolChoiceTone=String(index));
          choices.push([spec.id,field]);
          if(spec.beside)beside.set(spec.beside,{spec,bar});else body.append(bar);
        }
        for (const field of s.tool_settings) {
          if (compact && field.id==='tonal_upper') continue;
          if (compact && field.id==='tonal_lower') {
            const bounds=[field,s.tool_settings.find(f=>f.id==='tonal_upper')];
            range=createRangeControl({app,bounds,label:'Range in stops relative to reference white (0)',icon,
              onChange:(index,value)=>dispatch({type:'set_tool_setting',id:bounds[index].id,value})});
            body.append(range);continue;
          }
          if(field.group_id!==group){
            group=field.group_id;grouped=null;const extra=beside.get(field.id);
            if(group)body.append(element('h3','',()=>state().tool_extra.find(o=>o.Choice?.beside===field.id)?.Choice.label??state().tool_settings.find(f=>f.id===field.id)?.group??''));
            if(extra){const row=element('div','tool-position-group');grouped=element('div','tool-position-numbers');row.append(extra.bar,grouped);body.append(row);}
          }
          const node=numberField(field.numeric,()=>state().tool_settings.find(f=>f.id===field.id)?.label??"",value=>{if(key===next)dispatch({type:"set_tool_setting",id:field.id,value});},compact||!!grouped);
          node.onReset=()=>{if(key===next)dispatch({type:'reset_tool_setting',id:field.id});};
          if(field.preview)node.onEditPhase=phase=>{if(key===next)dispatch({type:'tool_setting_contact',id:field.id,phase});};
          node.dataset.toolSetting=field.id;
          if(grouped)node.querySelector('.number-track').hidden=true;
          if(compact||grouped){const row=element('label','tonal-numeric-row');row.append(element('span','',()=>state().tool_settings.find(f=>f.id===field.id)?.label??''),node);(grouped??body).append(row);}else body.append(node);
          numbers.push([field.id,node]);
        }
        const groups=new Set(),rows=new Map();
        for(const spec of s.tool_actions.filter(a=>!a.segmented)){
          if(spec.group){
            if(groups.has(spec.group))continue;groups.add(spec.group);
            const field=choiceRow(spec.group,command=>dispatch({type:'invoke',command}));
            body.append(field.row);actionGroups.push({id:spec.group,...field});continue;
          }
          const command=s.commands.find(c=>c.id===spec.command);
          const field=spec.checkable?actionField({element,button,icon},{state:command,checkable:spec.checkable},dispatch,{checkbox:true}):null;
          const node=field?.row??button('',()=>dispatch({type:'invoke',command:spec.command}),'tool-setting-action');node.dataset.toolAction=spec.command;
          if(spec.icon_only)node.classList.add('tool-icon-action');
          let parent=body;
          if(spec.row){
            if(!rows.has(spec.row)){const row=element('div','tool-action-row');row.dataset.toolActionRow=spec.row;body.append(row);rows.set(spec.row,row);}
            parent=rows.get(spec.row);
          }
          parent.append(node);
          actions.push([spec,node,field]);
        }
        if(previousBody){
          const children=[...body.children].map(child=>retainedRows.get(child)??child),retained=new Set(children);
          for(const child of [...previousBody.children])if(!retained.has(child))child.remove();
          let before=previousBody.firstChild;
          for(const child of children){if(child===before)before=before.nextSibling;else previousBody.insertBefore(child,before);}
        }
        contentChanged("tool_settings");
      }
      for (const [id,node] of numbers) node.update(s.tool_settings.find(f=>f.id===id).value);
      if(range) {range.relabel(['tonal_lower','tonal_upper'].map(id=>s.tool_settings.find(f=>f.id===id)),copy.tool_controls.range_hint);range.update(['tonal_lower','tonal_upper'].map(id=>s.tool_settings.find(f=>f.id===id).value));}
      for(const [id,field] of choices)field.update(s.tool_extra.find(o=>(o.Choice??o.Checklist)?.id===id));
      if(gradient)gradient.update(s.tool_extra.find(o=>o.Gradient).Gradient);
      for(const {id,update} of actionGroups){
        const group=catalog.tool_action_groups.find(g=>g.id===id),commands=s.tool_actions.filter(a=>a.group===id).map(a=>s.commands.find(c=>c.id===a.command));
        update(group.label,commands.map(c=>({value:c.id,label:c.label,enabled:c.enabled})),commands.find(c=>c.selected)?.id??commands[0]?.id);
      }
      for (const [spec,node,field] of actions) {
        const c=s.commands.find(c=>c.id===spec.command);
        if (field) { field.update({Action:{state:c,checkable:spec.checkable}}); continue; }
        if(!node.firstChild) {
          node.append(icon(c.icon));
          if (!spec.segmented&&!spec.icon_only) node.append(element("span","",c.label));
        }
        const caption=s.canvas_bar?.completion.find(item=>item.option.Action?.state.id===spec.command)?.label??c.label;
        const text=node.querySelector("span");if(text)text.textContent=caption;
        node.setAttribute("aria-label", c.label);
        node.disabled=!c.enabled; node.title=c.tooltip;
        if(spec.checkable) node.setAttribute("aria-pressed",String(c.selected));
      }
    };
  }
  function colorWheel(root) {
    const stage=element("div","color-wheel-square"),frame=element("div","color-wheel-stage");frame.append(stage);root.append(frame);
    const edit=button("",async()=>{
      const slot=displayColors().slot;
      const selected=await chooseColor({app,slot,element,button});
      if(selected)color(selected.intensity==null?{op:"set_slot",slot,color:selected.color}:{op:"set_slot_intensity",slot,color:selected.color,stops:selected.intensity});
    },"color-edit color-utility");
    bindCopy(edit,()=>copy.color.edit_menu,"title");bindCopy(edit,()=>copy.color.edit,"ariaLabel");edit.append(icon("pencil"));stage.append(edit);
    const wheel=element("canvas","color-wheel");bindCopy(wheel,()=>copy.color.wheel,"ariaLabel");stage.append(wheel);
    const choices=["background","foreground","transparent"].map(slot=>{
      const node=button("",()=>color({op:"select",slot}),"color-swatch");node.dataset.colorSlot=slot;if(slot!=='transparent')node.dataset.context=JSON.stringify({kind:'color_slot',slot});node.ondblclick=()=>{if(slot!=='transparent'){color({op:'select',slot});edit.click();}};
      const paint=element("span");node.append(paint);stage.append(node);return{slot,node,paint};
    });
    const shapes=[0,1].map(i=>{const node=button("",()=>color({op:"shape",shape:view.other_shapes[i]}),"color-shape");stage.append(node);return node;});
    const swap=button("",()=>color({op:"swap"}),"color-swap color-utility");
    bindCopy(swap,()=>copy.color.swap,"title");bindCopy(swap,()=>copy.color.swap,"ariaLabel");swap.append(icon("color-swap"));stage.append(swap);
    const arc=document.createElementNS("http://www.w3.org/2000/svg","svg"),track=document.createElementNS(arc.namespaceURI,"path"),markerShadow=document.createElementNS(arc.namespaceURI,"circle"),marker=document.createElementNS(arc.namespaceURI,"circle");
    // Chrome arbitrates touch scrolling on the SVG viewport, not its path.
    arc.classList.add('color-intensity');arc.style.cssText='position:absolute;inset:0;overflow:visible;pointer-events:none;touch-action:none';
    track.setAttribute('fill','none');track.setAttribute('stroke','transparent');track.setAttribute('stroke-linecap','round');track.style.pointerEvents='stroke';track.style.touchAction='none';track.setAttribute('tabindex','0');track.setAttribute('role','slider');bindCopy(track,()=>copy.color.intensity,"ariaLabel");
    const ramp=document.createElementNS(arc.namespaceURI,'g'),caption=document.createElementNS(arc.namespaceURI,'g');ramp.style.pointerEvents='none';caption.style.pointerEvents='none';caption.setAttribute('fill','currentColor');caption.setAttribute('opacity','.8');caption.style.fontFamily='"Adwaita Sans",system-ui,sans-serif';arc.append(ramp);
    // Match the wheel marker: the white ring and dark edge stay visible on every ramp color.
    markerShadow.setAttribute('fill','none');markerShadow.setAttribute('stroke','rgba(0,0,0,.5)');markerShadow.setAttribute('stroke-width','4');
    marker.setAttribute('fill','white');marker.setAttribute('stroke','white');marker.setAttribute('stroke-width','2');arc.append(track,markerShadow,marker,caption);stage.append(arc);
    let arcContact=null;
    const arcPick=e=>{const r=stage.getBoundingClientRect();const hit=app.color_ui({type:'arc',size:r.width,point:[e.clientX-r.x,e.clientY-r.y]});color({op:'hdr_intensity',stops:-2+8*hit.fraction});};
    track.onpointerdown=e=>{if(e.button!==0||displayColors().slot==='transparent')return;arcContact=e.pointerId;track.setPointerCapture(e.pointerId);e.preventDefault();arcPick(e);};track.onpointermove=e=>{if(e.pointerId===arcContact)arcPick(e)};track.onpointerup=e=>{if(e.pointerId===arcContact){arcPick(e);arcContact=null;}};
    for(const event of ['pointercancel','lostpointercapture'])track.addEventListener(event,e=>{if(e.pointerId===arcContact)arcContact=null;});
    track.ondblclick=()=>color({op:'hdr_intensity',stops:0});track.onkeydown=e=>{if(['ArrowLeft','ArrowDown','ArrowRight','ArrowUp','Home'].includes(e.key)){e.preventDefault();color({op:'hdr_intensity',stops:e.key==='Home'?0:Math.max(-2,Math.min(6,view.intensity+(['ArrowLeft','ArrowDown'].includes(e.key)?-.1:.1)))});}};
    const readout=button("",()=>color({op:"toggle_readout"}),"color-readout"),numbers=element("canvas");
    numbers.setAttribute("aria-hidden","true");readout.append(numbers);stage.append(readout);
    let view,layout,layoutWidth=0,frameWidth=0,frameHeight=0,naturalLayout,naturalAspect="",paintKey="",previewing=false;
    const painter=wheelPainter({canvas:wheel,worker:fieldWorker,request:pixels=>app.color_field_request(pixels),hueStops:hueStopCache(app),repaint:()=>{paintKey='';queuePaint();}});
    const place=(node,[x,y,w,h])=>Object.assign(node.style,{left:`${x}px`,top:`${y}px`,width:`${w}px`,height:`${h}px`});
    function draw() {
      const availableWidth=frame.clientWidth;if(!view||availableWidth<128)return;
      const geometry=size=>app.color_ui({type:"layout",size,hdr:view.hdr});
      const resized=availableWidth!==frameWidth||layout?.hdr!==view.hdr;
      if(resized){
        const minimum=Math.ceil(geometry(128).height);naturalLayout=geometry(availableWidth);
        frame.style.minHeight=root.style.minHeight=`${minimum}px`;
        const aspect=`${availableWidth} / ${naturalLayout.height}`;
        if(aspect!==naturalAspect){naturalAspect=aspect;frame.style.aspectRatio=aspect;contentChanged("color");}
      }
      const height=frame.clientHeight;
      if(resized||height!==frameHeight){
        // Fit the complete footer. Reading the frame avoids stage-size feedback;
        // cache geometry across color changes, just as for the wheel raster.
        let width=availableWidth;
        if(naturalLayout.height>height){
          let low=128,high=availableWidth;
          while(low<high){const middle=Math.ceil((low+high)/2);if(geometry(middle).height<=height)low=middle;else high=middle-1;}
          width=low;
        }
        frameWidth=availableWidth;frameHeight=height;layoutWidth=width;layout=geometry(width);layout.hdr=view.hdr;
        stage.style.width=`${width}px`;stage.style.height=`${layout.height}px`;
        place(wheel,layout.wheel);
        choices.forEach(({slot,node})=>place(node,layout[slot]));
        shapes.forEach((node,i)=>place(node,layout.shapes[i]));
        place(swap,layout.swap);place(readout,layout.readout);place(edit,layout.edit);
      }
      const width=layoutWidth;
      arc.hidden=!view.hdr;arc.style.display=view.hdr?'':'none';
      if(view.hdr){const g=app.color_ui({type:'arc',size:width,fraction:(view.intensity+2)/8});const a=g.geometry,start=app.color_ui({type:'arc',size:width,fraction:0}).point,end=app.color_ui({type:'arc',size:width,fraction:1}).point;arc.setAttribute('width',width);arc.setAttribute('height',layout.height);arc.style.width=`${width}px`;arc.style.height=`${layout.height}px`;track.setAttribute('d',`M${start} A${a.radius} ${a.radius} 0 0 0 ${end}`);track.setAttribute('stroke-width',a.width);for(const node of [markerShadow,marker]){node.setAttribute('cx',g.point[0]);node.setAttribute('cy',g.point[1]);node.setAttribute('r',a.marker_radius);}track.setAttribute('aria-valuenow',view.intensity);track.setAttribute('aria-valuetext',`${view.intensity.toFixed(1)} EV`);track.setAttribute('aria-valuemin','-2');track.setAttribute('aria-valuemax','6');
        g.path.slice(1).forEach((p,i)=>{let segment=ramp.children[i];if(!segment){segment=document.createElementNS(arc.namespaceURI,'path');segment.setAttribute('stroke-linecap','round');ramp.append(segment);}segment.setAttribute('d',`M${g.path[i]} L${p}`);segment.setAttribute('stroke',rgba(view.intensity_ramp[i]));segment.setAttribute('stroke-width',a.width);});marker.setAttribute('fill',rgba(view.marker_color));
        const font=layout.intensity_caption.font_size,text=`${view.intensity>=0?'+':''}${view.intensity.toFixed(2)} EV`,metrics=numbers.getContext('2d',{willReadFrequently:true});
        metrics.font=`${font}px "Adwaita Sans",system-ui,sans-serif`;
        const digit=Math.max(...[...'0123456789'].map(c=>metrics.measureText(c).width)),advances=[...text].map(c=>/[0-9 ]/.test(c)?digit:metrics.measureText(c).width);
        const placements=app.color_ui({type:'intensity_caption',size:width,advances});
        while(caption.children.length>text.length)caption.lastElementChild.remove();
        [...text].forEach((c,i)=>{let glyph=caption.children[i];if(!glyph){glyph=document.createElementNS(arc.namespaceURI,'text');glyph.setAttribute('text-anchor','middle');caption.append(glyph);}const [x,y,rotation]=placements[i];glyph.setAttribute('transform',`translate(${x} ${y}) rotate(${rotation})`);glyph.setAttribute('font-size',font);glyph.textContent=c;});
      }
      shapes.forEach((node,i)=>{node.firstElementChild.style.transform=`rotate(${layout.shape_rotations[i]}deg)`;});
      const half=layout.readout[2],r=layout.wheel[2]*view.geometry.outer+2;
      // The readout's curved hit area cannot intercept hue picking underneath.
      readout.style.clipPath=`path("M0 0H${half}V${half-r}A${r} ${r} 0 0 0 ${half-r} ${half}H0Z")`;
      const side=layout.wheel[2],scale=Math.min(devicePixelRatio||1,2),pixels=Math.ceil(side*scale);
      const ink=getComputedStyle(readout).color,focus=readout.matches(":focus-visible");
      const next=JSON.stringify([view,pixels,width,ink,focus]);if(next===paintKey)return;paintKey=next;
      painter.paint(view,side,pixels);
      drawReadout(half,scale,ink,focus);
    }
    function drawReadout(half,scale,ink,focus) {
      const pixels=Math.ceil(half*scale);if(numbers.width!==pixels){numbers.width=numbers.height=pixels;}
      const ctx=numbers.getContext("2d",{willReadFrequently:true});ctx.setTransform(1,0,0,1,0,0);ctx.clearRect(0,0,pixels,pixels);ctx.scale(pixels/half,pixels/half);
      const radius=layout.readout_radius,family='"Adwaita Sans",system-ui,sans-serif';
      let font=Math.min(12,Math.max(9,half*2*.044));
      ctx.font=`bold ${font}px ${family}`;ctx.fillStyle=ink;ctx.globalAlpha=.9;ctx.fillText(view.readout_label,2,font+1);
      if(focus){ctx.strokeStyle=ink;ctx.lineWidth=1.5;ctx.beginPath();ctx.roundRect(1,1,ctx.measureText(view.readout_label).width+5,font+4,5);ctx.stroke();}
      const rgb=view.readout==="rgb";
      const texts=view.readout_layout_text,available=radius*Math.PI/2-4;
      let widths,total,digitAdvance;
      const advance=c=>/[0-9 ]/.test(c)?digitAdvance:ctx.measureText(c).width;
      for(;;font-=.25) {
        ctx.font=`${font}px ${family}`;
        digitAdvance=Math.max(...[..."0123456789"].map(c=>ctx.measureText(c).width));
        widths=texts.map(text=>[...text].reduce((n,c)=>n+advance(c),0)+(rgb?font*.8+2:0));total=widths.reduce((a,b)=>a+b,0);
        if(total+6<=available||font<=8)break;
      }
      const chip=font*.8,gap=Math.min(radius*.24,Math.max(3,(available-total)*.5));
      let cursor=-(total+gap*2)*.5;
      const at=(angle,paint)=>{ctx.save();ctx.translate(half+radius*Math.cos(angle),half+radius*Math.sin(angle));ctx.rotate(angle+Math.PI/2);paint();ctx.restore();};
      texts.forEach((text,i)=>{
        const width=widths[i],mid=-135*Math.PI/180+(cursor+width*.5)/radius;cursor+=width+gap;
        let along=-width*.5;
        if(rgb){at(mid+(along+chip*.5)/radius,()=>{ctx.globalAlpha=1;ctx.fillStyle=["rgb(93% 31% 36%)","rgb(25% 73% 43%)","rgb(29% 56% 98%)"][i];ctx.beginPath();ctx.roundRect(-chip*.5,-font*.76,chip,chip,2);ctx.fill();});along+=chip+2;}
        for(const glyph of text){const cell=advance(glyph);at(mid+(along+cell*.5)/radius,()=>{ctx.globalAlpha=.8;ctx.fillStyle=ink;ctx.fillText(glyph,-ctx.measureText(glyph).width*.5,0);});along+=cell;}
      });
    }
    const hit=wheelHit(app);
    const picker=wheelPicker(wheel,(size,point)=>hit(size,point,view.shape),color);
    const touches=colorTouchInput({app,applyChange,wake});
    const inside=e=>{
      if(e.target===track)return true;
      if(e.target!==wheel||!view)return false;
      const r=wheel.getBoundingClientRect(),[x,y]=view.geometry.center;
      return Math.hypot((e.clientX-r.x)/r.width-x,(e.clientY-r.y)/r.height-y)<=view.geometry.outer;
    };
    let pressed=null;
    const contact=phase=>dispatch({type:'color_contact',phase});
    const history=e=>{
      if(e.type==='pointerdown'&&e.button===0&&pressed==null&&inside(e)){pressed=e.pointerId;contact('down');}
      if(!touches.route(e,inside(e)))return;
      if(e.type==='pointerdown'){picker.cancel();arcContact=null;pressed=null;}
      e.preventDefault();e.stopImmediatePropagation();
    };
    const release=e=>{if(e.pointerId!==pressed)return;pressed=null;contact(e.type==='pointerup'?'up':'cancel');};
    for(const type of ['pointerdown','pointermove','pointerup','pointercancel'])stage.addEventListener(type,history,true);
    for(const type of ['pointerup','pointercancel','lostpointercapture'])stage.addEventListener(type,release);
    wheel.menuModel=()=>app.context_menu({kind:'color_wheel'});
    stage.addEventListener('contextmenu',e=>{if(!inside(e))return;e.preventDefault();e.stopPropagation();showContext(wheel,[e.clientX,e.clientY]);});
    // Preserve native button activation instead of treating Space as canvas pan.
    for(const name of ["keydown","keyup"])root.addEventListener(name,e=>{if(e.target.closest("button")&&(e.key===" "||e.key==="Enter"))e.stopPropagation();});
    for(const name of ["focus","blur"])readout.addEventListener(name,draw);
    let resizeFrame;
    function flushPaint() { pendingPaints.delete(flushPaint); cancelAnimationFrame(resizeFrame); resizeFrame=null; draw(); }
    function queuePaint() { pendingPaints.add(flushPaint); if(!resizeFrame)resizeFrame=requestAnimationFrame(flushPaint); }
    const resize=new ResizeObserver(queuePaint);resize.observe(frame);
    root.navigatorDispose=()=>{painter.dispose();touches.dispose();if(pressed!=null){pressed=null;contact('up');}resize.disconnect();cancelAnimationFrame(resizeFrame);pendingPaints.delete(flushPaint)};
    return ()=>{
      const preview=app.color_preview();
      const active=preview.picker.preview!=null;
      if(previewing!==active)painter.invalidate();
      previewing=active;view=preview.view;
      edit.disabled=displayColors().slot==="transparent";
      choices.forEach(choice=>{
        const zIndex=choice.slot===view.front_swatch?"1":"0";
        if(choice.node.style.zIndex!==zIndex)choice.node.style.zIndex=zIndex;
        const {slot,node,paint}=choice,swatch=view.swatches.find(s=>s.slot===slot),key=JSON.stringify(swatch);if(choice.key===key)return;choice.key=key;
        if(node.title!==swatch.label){node.setAttribute("aria-label",swatch.label);node.title=swatch.label;}
        const pressed=String(swatch.selected);if(node.getAttribute("aria-pressed")!==pressed)node.setAttribute("aria-pressed",pressed);
        paint.style.background=`linear-gradient(${rgba(swatch.rgba)},${rgba(swatch.rgba)}),repeating-conic-gradient(var(--checker-dark) 0 25%,var(--checker-light) 0 50%) 0 0 / 10px 10px`;
      });
      shapes.forEach((node,i)=>{const shape=view.other_shapes[i];if(node.dataset.colorShape!==shape){node.dataset.colorShape=shape;node.replaceChildren(icon(`color-${shape}`));}node.title=`Use ${shape==="triangle"?"HLS":shape==="circle"?"Okhsv":"HSV"} ${shape}`;node.setAttribute("aria-label",node.title);});
      readout.setAttribute("aria-label",view.readout_description);queuePaint();
    };
  }
  function navigatorPanel(root) {
    const overview=element("div","navigator-overview"),surface=element("canvas","navigator-surface"),clip=element("div","navigator-clip"),hole=element("div","overview-hole");clip.append(surface);overview.append(clip,hole);root.append(overview);
    const controls=createNavigationControls({element,button,icon,dispatch,commands:liveCopy(app,"catalog").navigator_commands});root.append(controls);
    const record={id:nextNavigator++,root,overview,surface,hole,size:""};navigators.set(overview,record);
    app.navigator_surface(record.id,surface);
    navigatorSizes.observe(overview);
    root.navigatorDispose=()=>{navigatorSizes.unobserve(overview);navigators.delete(overview);app.remove_navigator_surface(record.id);};
    let contact=null;
    const send=(e,phase)=>{const r=overview.getBoundingClientRect();dispatch({type:"navigator",phase,position:[e.clientX-r.x,e.clientY-r.y],viewport:[r.width,r.height]});};
    overview.addEventListener("pointerdown",e=>{if(e.button!==0)return;contact=e.pointerId;overview.setPointerCapture(contact);e.preventDefault();send(e,"down");});
    overview.addEventListener("pointermove",e=>{if(contact===e.pointerId)send(e,"move");});
    overview.addEventListener("pointerup",e=>{if(contact===e.pointerId){send(e,"up");contact=null;}});
    for(const name of ["pointercancel","lostpointercapture"])overview.addEventListener(name,e=>{if(contact===e.pointerId){send(e,"cancel");contact=null;}});
    return()=>{
      const aspect=app.navigator_aspect();
      if(aspect!==record.aspect){record.aspect=aspect;overview.style.aspectRatio=`1 / ${aspect}`;contentChanged("navigator");}
      controls.update(state().commands);
      queuePositions();
    };
  }
  function measurePositions() {
    let resized=false;
    for(const record of navigators.values()) {
      const r=record.extent;if(!r)continue;
      const {hole}=record;
      // CSS transforms do not resize this native GPU canvas. DOM compositing
      // supplies scrolling, stacking and clipping without per-motion JS/GPU work.
      const scale=devicePixelRatio||1;
      const size=JSON.stringify([r.width,r.height,scale]);
      if(size!==record.size){
        record.size=size;
        const capacity=app.navigator_size(record.id,r.width,r.height,scale);
        // Native pixels remain 1:1. The surrounding clip follows live layout,
        // while spare capacity avoids reallocating a GPU surface every frame.
        record.surface.style.width=`${capacity[0]/scale}px`;
        record.surface.style.height=`${capacity[1]/scale}px`;
        resized=true;
      }
      const g=app.navigator_geometry(r.width,r.height);
      hole.hidden=!g;
      if(g)Object.assign(hole.style,{left:`${g.image.x}px`,top:`${g.image.y}px`,width:`${g.image.width}px`,height:`${g.image.height}px`});
    }
    return resized;
  }
  function updatePositions(){if(positioning)cancelAnimationFrame(positioning);positioning=0;if(measurePositions()&&app.reflow_navigators())wake();}
  function queuePositions(){if(!positioning)positioning=requestAnimationFrame(updatePositions);}
  const navigatorSizes=new ResizeObserver(entries=>{
    for(const entry of entries) {
      const record=navigators.get(entry.target);
      if(record)record.extent={width:entry.contentRect.width,height:entry.contentRect.height};
    }
    updatePositions();
  });
  window.addEventListener("resize",queuePositions);
  return {control,queuePositions,
    // Paint UI rasters after controls, geometry and theme writes have settled.
    // This avoids rasterizing the old color-wheel size before arrange() resizes it.
    flushPaint(){for(const paint of [...pendingPaints])paint();},
    refreshColorPreview(){for(const [root,fn] of updates)if(root.isConnected&&root.dataset.control==="color_wheel")fn();},
    refresh(){for(const fn of updates.values())fn();}};
}
