export function createLayerInspector({app, state, workspace, element, button, icon, contentPanel, toggle, invalidate}) {
  const root=element("div","layer-inspector"),stage=element("div","layer-properties-pages");
  const layers=contentPanel("layers"),properties=contentPanel("properties"),label=element("span","layer-properties-label"),summary=element("span","layer-properties-summary");
  const text=element("span","layer-properties-text");text.append(label,summary);
  const footer=button("",()=>toggle(),"layer-properties-disclosure");
  const slot=glyph=>{const node=element("span","layer-properties-icon-slot");node.append(icon(glyph));return node;};
  const chevron=slot("chevron-down");footer.append(slot("properties"),text,chevron);
  const footerRow=element("div","layer-inspector-footer"),layerFooter=layers.querySelector('.layer-footer'),add=layerFooter.querySelector('[data-footer-button="new_layer"]');
  footer.dataset.layerProperties="";stage.append(layers,properties);footerRow.append(footer);root.append(stage,footerRow);
  let open=false,position=0,from=0,started=0,anchor,target,measurement,measurementDirty=true,layoutKey;
  const rows=layers.querySelector(".layer-rows");
  const observer=new ResizeObserver(()=>{measurementDirty=true;invalidate();});
  for(const child of [layers.querySelector(".layer-header"),layers.querySelector(".layer-footer"),properties.firstElementChild])if(child)observer.observe(child);
  root.refreshPanel=()=>{layers.refreshPanel();properties.refreshPanel();measurementDirty=true;layoutKey=null;};
  root.disposePanel=()=>{observer.disconnect();layers.disposePanel?.();properties.disposePanel?.();};
  return {root,refresh(next,full=true,disclosure) {
    root.classList.toggle('simple-layer-inspector',!full);
    if(!full&&add.parentNode!==footerRow){footerRow.prepend(add);add.style.display='';}
    else if(full&&add.parentNode!==layerFooter)layerFooter.prepend(add);
    if(!full){add.style.opacity=next?'0':'1';add.inert=next;}
    else {add.style.opacity='';add.inert=false;}
    const view=state().layer_properties,editing=`${state().document_file.epoch}:${view.layer}`;
    if(target!==editing){target=editing;anchor=null;}
    if(open!==next) {
      if(next) {
        const bounds=rows.getBoundingClientRect(),row=[...rows.children].find(row=>row.getBoundingClientRect().bottom>bounds.top);
        anchor=row?{id:row.querySelector(".layer-row")?.dataset.layer,offset:row.getBoundingClientRect().top-bounds.top}:null;
      }
      from=position;started=performance.now();open=next;
    }
    label.textContent=disclosure??(open?view.disclosure.return_label:view.disclosure.label);
    summary.textContent=open?"":view.disclosure.summary;summary.hidden=!summary.textContent;
    footer.title=[label.textContent,summary.textContent].filter(Boolean).join("\n");footer.setAttribute("aria-label",footer.title);
    footer.disabled=!open&&view.layer==null;footer.classList.toggle("open",open);
    layers.inert=open;properties.inert=!open;
  },measure(now) {
    const progress=matchMedia("(prefers-reduced-motion: reduce)").matches?1:Math.min(1,(now-started)/200);
    position=from+((open?1:0)-from)*(1-(1-progress)**3);
    if(measurementDirty||measurement?.viewport!==workspace.clientHeight) {
      const unit=layers.measureUnit();
      const inset=getComputedStyle(layers),fixed=layers.querySelector(".layer-header").offsetHeight+layerFooter.offsetHeight+parseFloat(inset.paddingTop)+parseFloat(inset.paddingBottom);
      const contents=properties.firstElementChild,style=getComputedStyle(contents),viewport=workspace.clientHeight;
      measurement={viewport,...app.layer_drawer_sizing({measurement:{fixed_height:fixed,unit_height:unit},properties_height:contents.scrollHeight+parseFloat(style.marginTop)+parseFloat(style.marginBottom),viewport_height:viewport})};
      measurementDirty=false;
    }
    const size=measurement;
    footerRow.style.height=`${size.footer_height}px`;
    return {height:size.heights[0]*(1-position)+size.heights[1]*position,heights:size.heights,measurement:size.measurement,properties_height:size.properties_height,running:progress<1};
  },layout() {
    const height=stage.clientHeight;
    const key=`${height}:${position}`;if(layoutKey===key)return;layoutKey=key;
    layers.style.transform=`translateY(${-position*height}px)`;properties.style.transform=`translateY(${(1-position)*height}px)`;
    if(position===0&&anchor) {
      const row=rows.querySelector(`.layer-row[data-layer="${anchor.id}"]`)?.parentElement;
      if(row)rows.scrollTop+=row.getBoundingClientRect().top-rows.getBoundingClientRect().top-anchor.offset;
      anchor=null;
    }
    if(position===0) {
      const row=rows.querySelector(`.layer-row[data-layer="${state().layer_tools.editing_layer?.id}"]`);
      if(row&&target!==root.dataset.revealed){const delta=row.getBoundingClientRect().top-rows.getBoundingClientRect().top;if(delta<0)rows.scrollTop+=delta;else if(delta+row.offsetHeight>rows.clientHeight)rows.scrollTop+=delta+row.offsetHeight-rows.clientHeight;root.dataset.revealed=target;}
    }
  }};
}
