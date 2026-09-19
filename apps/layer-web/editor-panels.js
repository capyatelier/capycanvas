import { chooseColor } from './color-controls.js';
// DOM widgets for shared editor models. Rust owns tool/color/geometry policy.
export function createEditorPanels({ app, state, element, button, icon, numberField, dispatch, asset, wake, applyChange, contentChanged }) {
  const updates = new Map(), navigators = new Set();
  let positioning = 0, nextNavigator = 1;
  const color = action => dispatch({ type: "color", action });
  const rgba = values => `rgba(${values.slice(0,3).map(v => v * 255).join(",")},${values[3] ?? 1})`;
  const control = (kind) => {
    const root = element("div", `${kind.replaceAll("_", "-")}-control`);
    root.dataset.control = kind;
    let refresh;
    if (kind === "brushes") refresh = toolSet(root);
    else if (kind === "tool_settings") refresh = toolSettings(root);
    else if (kind === "color_wheel") refresh = colorWheel(root);
    else if (kind === "navigator") refresh = navigatorPanel(root);
    else return null;
    updates.set(root, refresh); refresh();
    root.disposeEditor = () => { updates.delete(root); root.navigatorDispose?.(); };
    return root;
  };
  function toolSet(root) {
    let key = "", rows = [];
    return () => {
      const view = state().tool_set;
      const next = JSON.stringify([view.groups, view.subtools].map(items => items.map(({selected, ...item}) => item))) + state().theme;
      if (next !== key) {
        key = next; rows = []; root.replaceChildren();
        for (const [kind, items] of [["groups",view.groups], ["subtools",view.subtools]]) {
          const list = element("div", `tool-${kind}`); root.append(list);
          for (const item of items) {
            const node = button("", () => dispatch(item.action), "tool-choice-button");
            node.dataset.toolChoice = item.label; node.setAttribute("aria-label", item.label);
            node.title = app.action_tooltip(item.label, item.action);
            if (item.preview != null) {
              const image = element("img", "brush-preview"); image.src = asset(`brush-previews/${item.preview}-${state().theme}.png`); image.alt = ""; image.draggable = false;
              node.append(image); node.dataset.brush = item.preview;
            } else if (kind === "groups") node.append(icon(item.icon));
            const label = element("span", "tool-choice-label");
            if (item.preview != null || kind === "subtools") label.append(icon(item.icon));
            label.append(element("span", "tool-choice-name", item.label));
            node.append(label); list.append(node); rows.push({node,kind,index:rows.filter(r=>r.kind===kind).length});
          }
        }
        contentChanged("brushes");
      }
      for (const {node,kind,index} of rows) {
        const pressed=String(view[kind][index].selected);if(node.getAttribute("aria-pressed")!==pressed)node.setAttribute("aria-pressed",pressed);
      }
    };
  }
  function toolSettings(root) {
    let key = "", numbers = [], actions = [];
    return () => {
      const s = state();
      const next = JSON.stringify([s.tool_settings.map(({value,...field})=>field),s.tool_actions]);
      if (next !== key) {
        key = next; root.replaceChildren(); numbers=[]; actions=[]; let group="";
        for (const field of s.tool_settings) {
          if (field.group && field.group !== group) root.append(element("h3", "", field.group)); group=field.group;
          const node=numberField(field.numeric,field.label,value=>dispatch({type:"set_tool_setting",id:field.id,value}));
          node.dataset.toolSetting=field.id; root.append(node); numbers.push([field.id,node]);
        }
        for (const spec of s.tool_actions) {
          const node=button("",()=>dispatch({type:"invoke",command:spec.command}),"tool-setting-action");
          node.dataset.toolAction=spec.command; root.append(node); actions.push([spec,node]);
        }
        contentChanged("tool_settings");
      }
      for (const [id,node] of numbers) node.update(s.tool_settings.find(f=>f.id===id).value);
      for (const [spec,node] of actions) {
        const c=s.commands.find(c=>c.id===spec.command);
        if(!node.firstChild) node.append(icon(c.icon),element("span","",c.label));
        node.disabled=!c.enabled; node.title=c.tooltip;
        if(spec.checkable) node.setAttribute("aria-pressed",String(c.selected));
      }
    };
  }
  function colorWheel(root) {
    const stage=element("div","color-wheel-square"),frame=element("div","color-wheel-stage");frame.append(stage);root.append(frame);
    const edit=button("",async()=>{
      const slot=state().colors.slot==="background"?"background":"foreground";
      let intensity;const selected=await chooseColor({app,color:state().colors[slot],element,button,intensity:app.color_panel().hdr?app.color_panel().intensity:null,onIntensity:v=>intensity=v});
      if(selected)color(intensity==null?{op:"set_slot",slot,color:selected}:{op:"set_slot_intensity",slot,color:selected,stops:intensity});
    },"color-edit color-utility");
    edit.title="Edit Color…";edit.setAttribute("aria-label","Edit Color");edit.append(icon("pencil"));stage.append(edit);
    const wheel=element("canvas","color-wheel");wheel.setAttribute("aria-label","Color wheel");stage.append(wheel);
    // Paint order also controls hit testing in the intentional swatch overlap.
    const choices=["background","foreground","transparent"].map(slot=>{
      const node=button("",()=>color({op:"select",slot}),"color-swatch");node.dataset.colorSlot=slot;node.ondblclick=()=>{if(slot!=='transparent'){color({op:'select',slot});edit.click();}};
      const paint=element("span");node.append(paint);stage.append(node);return{slot,node,paint};
    });
    const shapes=[0,1].map(i=>{const node=button("",()=>color({op:"shape",shape:view.other_shapes[i]}),"color-shape");stage.append(node);return node;});
    const swap=button("",()=>color({op:"swap"}),"color-swap color-utility");
    swap.title="Swap foreground and background";swap.setAttribute("aria-label",swap.title);swap.append(icon("color-swap"));stage.append(swap);
    const arc=document.createElementNS("http://www.w3.org/2000/svg","svg"),track=document.createElementNS(arc.namespaceURI,"path"),marker=document.createElementNS(arc.namespaceURI,"circle");
    arc.classList.add('color-intensity');arc.style.cssText='position:absolute;inset:0;overflow:visible;pointer-events:none';
    track.setAttribute('fill','none');track.setAttribute('stroke','transparent');track.setAttribute('stroke-linecap','round');track.style.pointerEvents='stroke';track.style.touchAction='none';track.setAttribute('tabindex','0');track.setAttribute('role','slider');track.setAttribute('aria-label','Color intensity');
    const ramp=document.createElementNS(arc.namespaceURI,'g'),caption=document.createElementNS(arc.namespaceURI,'text');ramp.style.pointerEvents='none';caption.style.pointerEvents='none';caption.setAttribute('fill','currentColor');arc.append(ramp);
    marker.setAttribute('fill','white');marker.setAttribute('stroke','black');marker.setAttribute('stroke-width','2');arc.append(track,marker,caption);stage.append(arc);
    let arcContact=null,originalIntensity=0;
    const arcPick=e=>{const r=stage.getBoundingClientRect();const hit=app.color_ui({type:'arc',size:r.width,point:[e.clientX-r.x,e.clientY-r.y]});color({op:'hdr_intensity',stops:-2+8*hit.fraction});};
    track.onpointerdown=e=>{if(e.button!==0||state().colors.slot==='transparent')return;arcContact=e.pointerId;originalIntensity=view.intensity;track.setPointerCapture(e.pointerId);e.preventDefault();arcPick(e);};track.onpointermove=e=>{if(e.pointerId===arcContact)arcPick(e)};track.onpointerup=e=>{if(e.pointerId===arcContact){arcPick(e);arcContact=null;}};
    for(const event of ['pointercancel','lostpointercapture'])track.addEventListener(event,e=>{if(e.pointerId===arcContact){arcContact=null;color({op:'hdr_intensity',stops:originalIntensity});}});
    track.ondblclick=()=>color({op:'hdr_intensity',stops:0});track.onkeydown=e=>{if(['ArrowLeft','ArrowDown','ArrowRight','ArrowUp','Home'].includes(e.key)){e.preventDefault();color({op:'hdr_intensity',stops:e.key==='Home'?0:Math.max(-2,Math.min(6,view.intensity+(['ArrowLeft','ArrowDown'].includes(e.key)?-.1:.1)))});}};
    const readout=button("",()=>color({op:"toggle_readout"}),"color-readout"),numbers=element("canvas");
    numbers.setAttribute("aria-hidden","true");readout.append(numbers);stage.append(readout);
    let view,layout,layoutWidth=0,paintKey="",fieldKey="",ringKey="";
    const field=document.createElement("canvas"),ring=document.createElement("canvas");
    const ctx=wheel.getContext("2d",{willReadFrequently:true});
    const fieldContext=field.getContext("2d",{willReadFrequently:true}),ringContext=ring.getContext("2d",{willReadFrequently:true});
    const place=(node,[x,y,w,h])=>Object.assign(node.style,{left:`${x}px`,top:`${y}px`,width:`${w}px`,height:`${h}px`});
    function draw() {
      const width=stage.clientWidth;if(!view||width<128)return;
      if(width!==layoutWidth||layout?.hdr!==view.hdr){
        layoutWidth=width;layout=app.color_ui({type:"layout",size:width,hdr:view.hdr});layout.hdr=view.hdr;stage.style.height=`${layout.height}px`;const outer=frame.clientWidth;const ratio=app.color_ui({type:"layout",size:outer,hdr:view.hdr}).height/outer;frame.style.aspectRatio=`1 / ${ratio}`;stage.style.width=`min(100cqw,${100/ratio}cqh)`;
        place(wheel,layout.wheel);
        choices.forEach(({slot,node})=>place(node,layout[slot]));
        shapes.forEach((node,i)=>place(node,layout.shapes[i]));
        place(swap,layout.swap);place(readout,layout.readout);place(edit,layout.edit);
      }
      arc.hidden=!view.hdr;arc.style.display=view.hdr?'':'none';
      if(view.hdr){const g=app.color_ui({type:'arc',size:width,fraction:(view.intensity+2)/8});const a=g.geometry,start=app.color_ui({type:'arc',size:width,fraction:0}).point,end=app.color_ui({type:'arc',size:width,fraction:1}).point;arc.setAttribute('width',width);arc.setAttribute('height',layout.height);track.setAttribute('d',`M${start} A${a.radius} ${a.radius} 0 0 0 ${end}`);track.setAttribute('stroke-width',a.width);marker.setAttribute('cx',g.point[0]);marker.setAttribute('cy',g.point[1]);marker.setAttribute('r',a.marker_radius);track.setAttribute('aria-valuenow',view.intensity);track.setAttribute('aria-valuetext',`${view.intensity.toFixed(1)} EV`);track.setAttribute('aria-valuemin','-2');track.setAttribute('aria-valuemax','6');
        ramp.replaceChildren(...g.path.slice(1).map((p,i)=>{const segment=document.createElementNS(arc.namespaceURI,'path');segment.setAttribute('d',`M${g.path[i]} L${p}`);segment.setAttribute('stroke',rgba(view.intensity_ramp[i]));segment.setAttribute('stroke-width',a.width);segment.setAttribute('stroke-linecap','round');return segment;}));marker.setAttribute('fill',rgba(view.marker_color));
        const font=Math.min(12,Math.max(9,width*.044)),radius=a.radius+a.width/2+font+3,x=a.center[0]+radius*Math.cos(76*Math.PI/180),y=a.center[1]+radius*Math.sin(76*Math.PI/180);caption.setAttribute('x',x);caption.setAttribute('y',y);caption.setAttribute('text-anchor','middle');caption.setAttribute('font-size',font);caption.setAttribute('transform',`rotate(-14 ${x} ${y})`);caption.textContent=`${view.intensity>=0?'+':''}${view.intensity.toFixed(2)} EV`;
      }
      shapes.forEach((node,i)=>{node.firstElementChild.style.transform=`rotate(${layout.shape_rotations[i]}deg)`;});
      const half=layout.readout[2],r=layout.wheel[2]*view.geometry.outer+2;
      // The readout's curved hit area cannot intercept hue picking underneath.
      readout.style.clipPath=`path("M0 0H${half}V${half-r}A${r} ${r} 0 0 0 ${half-r} ${half}H0Z")`;
      const side=layout.wheel[2],scale=Math.min(devicePixelRatio||1,2),pixels=Math.ceil(side*scale);
      const ink=getComputedStyle(readout).color,focus=readout.matches(":focus-visible");
      const next=JSON.stringify([view,pixels,width,ink,focus]);if(next===paintKey)return;paintKey=next;
      if(wheel.width!==pixels){wheel.width=wheel.height=pixels;}
      ctx.setTransform(1,0,0,1,0,0);ctx.clearRect(0,0,pixels,pixels);ctx.scale(pixels/side,pixels/side);
      const g=view.geometry,[cx,cy]=g.center.map(v=>v*side),inner=g.inner*side,outer=g.outer*side;
      const fieldPixels=view.shape==="circle"?Math.ceil(side):pixels;
      const key=JSON.stringify([view.rgb_space,view.shape,view.wheel_components[0],view.intensity,view.rendition,fieldPixels]);
      if(key!==fieldKey) {
        fieldKey=key;if(field.width!==fieldPixels){field.width=field.height=fieldPixels;}
        const bytes=app.color_field_pixels(fieldPixels);
        fieldContext.putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer,bytes.byteOffset,bytes.byteLength),fieldPixels,fieldPixels),0,0);
      }
      ctx.save();
      if(view.shape==="circle"){ctx.beginPath();ctx.arc(cx,cy,g.disc_radius*side,0,2*Math.PI);ctx.clip();}
      else if(view.shape==="square"){const [x,y,w]=g.square.map(v=>v*side);ctx.beginPath();ctx.roundRect(x,y,w,w,Math.min(6,side*.02));ctx.clip();}
      ctx.drawImage(field,0,0,side,side);ctx.restore();
      // The ring depends on the color model and size, never the selected hue.
      // Retain its raster so a drag only repaints the changing field and markers.
      const nextRing=JSON.stringify([view.rgb_space,view.shape,pixels,side,g,view.wheel_hue_start_degrees]);
      if(nextRing!==ringKey){
        ringKey=nextRing;ring.width=ring.height=pixels;ringContext.scale(pixels/side,pixels/side);
        const hue=ringContext.createConicGradient(view.wheel_hue_start_degrees*Math.PI/180,cx,cy);
        app.color_hue_stops().forEach(stop=>hue.addColorStop(stop.offset,rgba(stop.color)));
        ringContext.strokeStyle=hue;ringContext.lineWidth=outer-inner;ringContext.beginPath();ringContext.arc(cx,cy,(inner+outer)/2,0,Math.PI*2);ringContext.stroke();
      }
      ctx.drawImage(ring,0,0,side,side);
      const radius=Math.min(10,Math.max(6,side*.04));
      for(const [p,fill] of [[view.wheel_hue_marker,rgba(view.wheel_hue_color)],[view.wheel_marker,rgba(view.marker_color)]]) {
        ctx.beginPath();ctx.arc(p[0]*side,p[1]*side,radius,0,2*Math.PI);ctx.fillStyle=fill;ctx.fill();ctx.strokeStyle="rgba(0,0,0,.5)";ctx.lineWidth=4;ctx.stroke();ctx.strokeStyle="white";ctx.lineWidth=2;ctx.stroke();
      }
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
    let contact=null;
    const pick=e=>{const r=wheel.getBoundingClientRect();color({op:"pick_wheel",part:contact.part,size:r.width,point:[e.clientX-r.x,e.clientY-r.y]});};
    wheel.addEventListener("pointerdown",e=>{if(e.button!==0)return;const r=wheel.getBoundingClientRect(),part=app.color_wheel_hit(r.width,e.clientX-r.x,e.clientY-r.y);if(!part)return;contact={id:e.pointerId,part};wheel.setPointerCapture(e.pointerId);e.preventDefault();pick(e);});
    wheel.addEventListener("pointermove",e=>{if(contact?.id===e.pointerId)pick(e);});
    for(const name of ["pointerup","pointercancel","lostpointercapture"])wheel.addEventListener(name,e=>{if(contact?.id===e.pointerId)contact=null;});
    // Preserve native button activation instead of treating Space as canvas pan.
    for(const name of ["keydown","keyup"])root.addEventListener(name,e=>{if(e.target.closest("button")&&(e.key===" "||e.key==="Enter"))e.stopPropagation();});
    for(const name of ["focus","blur"])readout.addEventListener(name,draw);
    let resizeFrame;const resize=new ResizeObserver(()=>{if(!resizeFrame)resizeFrame=requestAnimationFrame(()=>{resizeFrame=null;draw()})});resize.observe(stage);
    root.navigatorDispose=()=>{resize.disconnect();cancelAnimationFrame(resizeFrame)};
    return ()=>{
      view=app.color_panel();
      edit.disabled=state().colors.slot==="transparent";
      choices.forEach(choice=>{
        const {slot,node,paint}=choice,swatch=view.swatches.find(s=>s.slot===slot),key=JSON.stringify(swatch);if(choice.key===key)return;choice.key=key;
        if(node.title!==swatch.label){node.setAttribute("aria-label",swatch.label);node.title=swatch.label;}
        const pressed=String(swatch.selected);if(node.getAttribute("aria-pressed")!==pressed)node.setAttribute("aria-pressed",pressed);
        paint.style.background=`linear-gradient(${rgba(swatch.rgba)},${rgba(swatch.rgba)}),repeating-conic-gradient(#ccc 0 25%,#8c8c8c 0 50%) 0 0 / 10px 10px`;
      });
      shapes.forEach((node,i)=>{const shape=view.other_shapes[i];if(node.dataset.colorShape!==shape){node.dataset.colorShape=shape;node.replaceChildren(icon(`color-${shape}`));}node.title=`Use ${shape==="triangle"?"HLS":shape==="circle"?"Okhsv":"HSV"} ${shape}`;node.setAttribute("aria-label",node.title);});
      readout.setAttribute("aria-label",view.readout_description);draw();
    };
  }
  function navigatorPanel(root) {
    const overview=element("div","navigator-overview"),surface=element("canvas","navigator-surface"),clip=element("div","navigator-clip"),hole=element("div","overview-hole");clip.append(surface);overview.append(clip,hole);root.append(overview);
    const controls=element("div","navigator-buttons");root.append(controls);
    const buttons=["zoom_out","zoom_in","rotate_left","rotate_right","flip_horizontal","flip_vertical"].map(id=>{const node=button("",()=>dispatch({type:"invoke",command:id}));node.dataset.navigatorCommand=id;controls.append(node);return[id,node];});
    const record={id:nextNavigator++,root,overview,surface,hole,size:""};navigators.add(record);
    app.navigator_surface(record.id,surface);
    const resize=new ResizeObserver(queuePositions);resize.observe(overview);
    root.navigatorDispose=()=>{resize.disconnect();navigators.delete(record);app.remove_navigator_surface(record.id);};
    let contact=null;
    const send=(e,phase)=>{const r=overview.getBoundingClientRect();dispatch({type:"navigator",phase,position:[e.clientX-r.x,e.clientY-r.y],viewport:[r.width,r.height]});};
    overview.addEventListener("pointerdown",e=>{if(e.button!==0)return;contact=e.pointerId;overview.setPointerCapture(contact);e.preventDefault();send(e,"down");});
    overview.addEventListener("pointermove",e=>{if(contact===e.pointerId)send(e,"move");});
    overview.addEventListener("pointerup",e=>{if(contact===e.pointerId){send(e,"up");contact=null;}});
    for(const name of ["pointercancel","lostpointercapture"])overview.addEventListener(name,e=>{if(contact===e.pointerId){send(e,"cancel");contact=null;}});
    let commandsKey="";
    return()=>{
      const commands=buttons.map(([id])=>state().commands.find(c=>c.id===id)),key=JSON.stringify(commands);
      if(key!==commandsKey){commandsKey=key;buttons.forEach(([,node],i)=>{const c=commands[i];if(!node.firstChild)node.append(icon(c.icon));node.title=c.tooltip;node.setAttribute("aria-label",c.label);node.setAttribute("aria-pressed",String(c.selected));node.disabled=!c.enabled;});}
      queuePositions();
    };
  }
  function measurePositions() {
    let resized=false;
    for(const record of navigators) {
      const {overview,hole}=record;
      // CSS transforms do not resize this native GPU canvas. DOM compositing
      // supplies scrolling, stacking and clipping without per-motion JS/GPU work.
      const r=overview.getBoundingClientRect(),scale=devicePixelRatio||1;
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
  function updatePositions(){positioning=0;if(measurePositions()&&app.reflow_navigators())wake();}
  function queuePositions(){if(!positioning)positioning=requestAnimationFrame(updatePositions);}
  function flushPositions(){if(positioning)cancelAnimationFrame(positioning);positioning=0;return measurePositions();}
  window.addEventListener("resize",queuePositions);
  return {control,queuePositions,flushPositions,refresh(){for(const fn of updates.values())fn();}};
}
