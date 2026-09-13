// DOM widgets for shared editor models. Rust owns tool/color/geometry policy.
export function createEditorPanels({ app, state, element, button, icon, numberField, dispatch, asset, wake }) {
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
            } else node.append(icon(item.icon));
            node.append(element("span", "", item.label)); list.append(node); rows.push({node,kind,index:rows.filter(r=>r.kind===kind).length});
          }
        }
      }
      for (const {node,kind,index} of rows) node.setAttribute("aria-pressed", String(view[kind][index].selected));
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
      }
      for (const [id,node] of numbers) node.update(s.tool_settings.find(f=>f.id===id).value);
      for (const [spec,node] of actions) {
        const c=s.commands.find(c=>c.id===spec.command); node.textContent=c.label; node.disabled=!c.enabled; node.title=c.tooltip;
        if(spec.checkable) node.setAttribute("aria-pressed",String(c.selected));
      }
    };
  }
  function colorWheel(root) {
    const stage=element("div","color-wheel-square"),frame=element("div","color-wheel-stage"); frame.append(stage);root.append(frame);
    const wheel=element("canvas","color-wheel"); wheel.setAttribute("aria-label","Color wheel"); stage.append(wheel);
    const swatches=element("div","color-swatches"), choices=[]; stage.append(swatches);
    for(const slot of ["foreground","background","transparent"]) {
      const node=button("",()=>color({op:"select",slot}),"color-swatch"); node.dataset.colorSlot=slot;
      const paint=element("span"); node.append(paint);swatches.append(node);choices.push([node,paint]);
    }
    const space=button("",()=>color({op:"toggle_space"}),"color-space color-utility"); stage.append(space);
    const footer=element("div","color-footer"); root.append(footer);
    const components=element("div","color-components");footer.append(components);
    const swap=button("",()=>color({op:"swap"}),"color-swap color-utility");
    swap.title="Swap foreground and background";swap.setAttribute("aria-label",swap.title);
    swap.append(icon("color-swap"));footer.append(swap);
    let view, fields=[], fieldsKey="", paintKey="";
    function draw() {
      if(!view)return;
      const side=wheel.clientWidth;if(!side)return;
      const scale=Math.min(devicePixelRatio||1,2), pixels=Math.round(side*scale);
      const next=JSON.stringify([view,pixels]);if(next===paintKey)return;paintKey=next;
      wheel.width=wheel.height=pixels;const ctx=wheel.getContext("2d",{willReadFrequently:true});ctx.scale(pixels/side,pixels/side);
      const g=view.geometry,[cx,cy]=g.center.map(v=>v*side),inner=g.inner*side,outer=g.outer*side;
      const hue=ctx.createConicGradient(view.hue_start_degrees*Math.PI/180,cx,cy);
      view.hue_stops.forEach((c,i)=>hue.addColorStop(i/(view.hue_stops.length-1),rgba(c)));
      if(view.space==="hsv") {
        const [x,y,w]=g.square.map(v=>v*side);
        ctx.save();ctx.beginPath();ctx.roundRect(x,y,w,w,Math.min(6,side*.02));ctx.clip();
        const saturation=ctx.createLinearGradient(x,y,x+w,y);saturation.addColorStop(0,"white");saturation.addColorStop(1,rgba(view.hue_color));ctx.fillStyle=saturation;ctx.fillRect(x,y,w,w);
        const value=ctx.createLinearGradient(x,y,x,y+w);value.addColorStop(0,"transparent");value.addColorStop(1,"black");ctx.fillStyle=value;ctx.fillRect(x,y,w,w);ctx.restore();
      } else {
        // Rasterize the shared white/black/hue triangle as a vertex gradient.
        const [a,b,c]=g.triangle.map(p=>p.map(v=>v*pixels));
        const minX=Math.max(0,Math.floor(Math.min(a[0],b[0],c[0]))),minY=Math.max(0,Math.floor(Math.min(a[1],b[1],c[1])));
        const width=Math.ceil(Math.max(a[0],b[0],c[0]))-minX,height=Math.ceil(Math.max(a[1],b[1],c[1]))-minY;
        const image=ctx.createImageData(width,height),d=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1]);
        for(let y=0;y<height;y++)for(let x=0;x<width;x++) {
          const px=minX+x+.5,py=minY+y+.5;
          const white=((b[1]-c[1])*(px-c[0])+(c[0]-b[0])*(py-c[1]))/d;
          const black=((c[1]-a[1])*(px-c[0])+(a[0]-c[0])*(py-c[1]))/d,h=1-white-black;
          if(Math.min(white,black,h)>=0){const i=(y*width+x)*4;for(let j=0;j<3;j++)image.data[i+j]=255*(white+h*view.hue_color[j]);image.data[i+3]=255;}
        }
        ctx.putImageData(image,minX,minY);
      }
      ctx.strokeStyle=hue;ctx.lineWidth=outer-inner;ctx.beginPath();ctx.arc(cx,cy,(inner+outer)/2,0,Math.PI*2);ctx.stroke();
      const radius=Math.min(10,Math.max(6,side*.04));
      for(const [p,fill] of [[view.hue_marker,rgba(view.hue_color)],[view.field_marker,rgba(view.marker_color)]]) {
        ctx.beginPath();ctx.arc(p[0]*side,p[1]*side,radius,0,2*Math.PI);ctx.fillStyle=fill;ctx.fill();ctx.strokeStyle="rgba(0,0,0,.5)";ctx.lineWidth=4;ctx.stroke();ctx.strokeStyle="white";ctx.lineWidth=2;ctx.stroke();
      }
    }
    let contact=null;
    const pick=e=>{const r=wheel.getBoundingClientRect();color({op:"pick",part:contact.part,size:r.width,point:[e.clientX-r.x,e.clientY-r.y]});};
    wheel.addEventListener("pointerdown",e=>{if(e.button!==0)return;const r=wheel.getBoundingClientRect(),part=app.color_wheel_hit(r.width,e.clientX-r.x,e.clientY-r.y);if(!part)return;contact={id:e.pointerId,part};wheel.setPointerCapture(e.pointerId);e.preventDefault();pick(e);});
    wheel.addEventListener("pointermove",e=>{if(contact?.id===e.pointerId)pick(e);});
    for(const name of ["pointerup","pointercancel","lostpointercapture"])wheel.addEventListener(name,e=>{if(contact?.id===e.pointerId)contact=null;});
    const resize=new ResizeObserver(draw);resize.observe(wheel);
    root.navigatorDispose=()=>resize.disconnect();
    return ()=>{
      view=app.color_panel();
      choices.forEach(([node,paint],i)=>{const swatch=view.swatches[i];node.setAttribute("aria-label",swatch.label);node.title=swatch.label;node.setAttribute("aria-pressed",String(swatch.selected));paint.style.background=`linear-gradient(${rgba(swatch.rgba)},${rgba(swatch.rgba)}),repeating-conic-gradient(#8c8c8c 0 25%,#ccc 0 50%) center / 10px 10px`;});
      const mode=view.space==="hsv"?"triangle":"square";
      if(space.dataset.mode!==mode){space.dataset.mode=mode;space.replaceChildren(icon(`color-${mode}`));}
      space.title=view.space==="hsv"?"HSV square · Switch to HLS triangle":"HLS triangle · Switch to HSV square";
      space.setAttribute("aria-label",space.title);
      const key=JSON.stringify(view.components.map(({value,...c})=>c));
      if(key!==fieldsKey){
        fieldsKey=key;components.replaceChildren();
        fields=view.components.map((c,index)=>{
          const group=element("div","color-component"),label=element("span","color-component-label",c.label);
          label.setAttribute("aria-hidden","true");
          const node=numberField(c.numeric,c.name,value=>color({op:"component",index,value}),true);
          node.dataset.colorComponent=index;group.append(label,node);components.append(group);return node;
        });
      }
      fields.forEach((node,i)=>node.update(view.components[i].value));draw();
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
    return()=>{for(const[id,node]of buttons){const c=state().commands.find(c=>c.id===id);if(!node.firstChild)node.append(icon(c.icon));node.title=c.tooltip;node.setAttribute("aria-label",c.label);node.setAttribute("aria-pressed",String(c.selected));node.disabled=!c.enabled;}queuePositions();};
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
