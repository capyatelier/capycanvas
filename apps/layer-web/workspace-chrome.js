import {liveCopy} from "./localization.js";
import {createLayerInspector} from "./layer-inspector.js";
// Shared dock projections. The browser supplies widgets and measured body sizes.
function squircleCorner([cx,cy],[sx,sy],[ex,ey]) {
  return Array.from({length:24},(_,i)=>{
    const angle=(i+1)*Math.PI/48,along=Math.sqrt(Math.cos(angle)),across=Math.sqrt(Math.sin(angle));
    return `L ${cx+sx*along+ex*across} ${cy+sy*along+ey*across}`;
  }).join(" ");
}
const hoverFeet = [
  `path("M 6 0 L 6 6 L 0 6 ${squircleCorner([0,0],[0,6],[6,0])} Z")`,
  `path("M 0 0 L 0 6 L 6 6 ${squircleCorner([6,0],[0,6],[-6,0])} Z")`,
];
const tabSizes=new WeakMap(),tabGeometry=new WeakMap();let tabSurfaceId=0;
export function selectedTabSurface(group) {
  const tab=group?.querySelector('.tab-list > .dock-tab[aria-selected="true"]');
  return tab&&!tab.classList.contains('dragged-tab-source')?tabGeometry.get(tab):null;
}
function drawTabSurface(tab,size){
  const surface=tab.querySelector(':scope > .tab-surface');if(!surface||size.width<=0||size.height<=0)return;
  const style=getComputedStyle(tab),w=size.width,h=size.height,r=Math.min(parseFloat(style.borderTopLeftRadius),w/2,h/2),foot=6;
  const squircle=['squircle','superellipse(2)'].includes(style.cornerTopLeftShape||style.cornerShape);
  const corner=(center,start,end,sweep)=>squircle?squircleCorner(center,start,end):`A ${Math.abs(start[0]||start[1])} ${Math.abs(start[0]||start[1])} 0 0 ${sweep} ${center[0]+end[0]} ${center[1]+end[1]}`;
  const face=`M ${r} 0 H ${w-r} ${corner([w-r,r],[0,-r],[r,0],1)} V ${h-foot} ${corner([w+foot,h-foot],[-foot,0],[0,foot],0)} H ${-foot} ${corner([-foot,h-foot],[0,foot],[foot,0],0)} V ${r} ${corner([r,r],[-r,0],[0,-r],1)} Z`;
  const list=tab.parentElement,group=tab.closest('.dock-group');
  const filter=surface.querySelector('filter'),extent=list.clientWidth;
  if(group&&list.matches('.tab-list')) {
    const origin=group.getBoundingClientRect(),bounds=tab.getBoundingClientRect(),clip=list.getBoundingClientRect();
    tabGeometry.set(tab,{d:face,x:bounds.x-origin.x,y:bounds.y-origin.y,clipX:clip.x-origin.x,
      clipY:clip.y-origin.y,width:clip.width,height:clip.height,scroll:list.scrollLeft});
  }
  for(const [name,value]of Object.entries({x:-extent,y:0,width:2*extent+w,height:h}))filter.setAttribute(name,value);
  surface.setAttribute('viewBox',`0 0 ${w} ${h}`);surface.lastElementChild.setAttribute('d',face);
  tab.dispatchEvent(new Event('panel-tab-surface',{bubbles:true}));
}
const tabSurfaces=new ResizeObserver(entries=>{
  for(const entry of entries){
    if(!entry.target.matches('.dock-tab')){for(const tab of entry.target.children){const size=tabSizes.get(tab);if(size)drawTabSurface(tab,size);}continue;}
    const box=entry.borderBoxSize[0],size={width:box.inlineSize,height:box.blockSize};tabSizes.set(entry.target,size);drawTabSurface(entry.target,size);
    if(entry.target.parentElement?.matches('.tab-list,.drawer-tab-strip'))tabSurfaces.observe(entry.target.parentElement);
  }
});
document.addEventListener('scroll',event=>{
  const list=event.target;if(!list.matches?.('.tab-list'))return;
  const scroll=list.scrollLeft;
  for(const tab of list.children) {
    const geometry=tabGeometry.get(tab);if(!geometry)continue;
    geometry.x+=geometry.scroll-scroll;geometry.scroll=scroll;
  }
  list.dispatchEvent(new Event('panel-tab-surface',{bubbles:true}));
},true);
export function releaseTabSurfaces(root){for(const node of root.querySelectorAll('.dock-tab,.tab-list,.drawer-tab-strip'))tabSurfaces.unobserve(node);}
export function orderPanelTabs(strip,panels){
  if(!strip||panels.every((id,index)=>strip.children[index]?.dataset.panel===id))return false;
  const tabs=new Map([...strip.children].map(tab=>[tab.dataset.panel,tab]));
  const pinned=!strip.moveBefore&&[...tabs.values()].find(tab=>tab.contains(document.activeElement));
  if(pinned){
    const index=panels.indexOf(pinned.dataset.panel);
    for(const id of panels.slice(0,index))strip.insertBefore(tabs.get(id),pinned);
    for(const id of panels.slice(index+1))strip.append(tabs.get(id));
  }
  panels.forEach((id,index)=>{
    const tab=tabs.get(id);tab.dataset.index=index;
    if(strip.children[index]===tab)return;
    if(strip.moveBefore)strip.moveBefore(tab,strip.children[index]??null);
    else strip.insertBefore(tab,strip.children[index]??null);
  });
  for(const tab of strip.children){const size=tabSizes.get(tab);if(size)drawTabSurface(tab,size);}
  return true;
}
export function prepareTabPreview(tab){const surface=tab.querySelector(':scope > .tab-surface');if(surface){const id=`tab-surface-${++tabSurfaceId}`;surface.querySelector('filter').id=id;surface.lastElementChild.setAttribute('filter',`url(#${id})`);const box=surface.viewBox.baseVal;if(box.width>0)tabSizes.set(tab,{width:box.width,height:box.height});}}
export function tabHover(tab, presentation) {
  if(!tab.classList.contains('column-tab')&&!tab.querySelector(':scope > .tab-surface')){
    const surface=document.createElementNS('http://www.w3.org/2000/svg','svg');surface.classList.add('tab-surface');surface.setAttribute('aria-hidden','true');surface.setAttribute('focusable','false');
    surface.innerHTML='<defs><filter color-interpolation-filters="sRGB" filterUnits="userSpaceOnUse"><feFlood class="tab-surface-strip" result="strip"/><feComposite in="strip" in2="SourceAlpha" operator="out" result="outside"/><feFlood class="tab-surface-panel" result="panel"/><feComposite in="panel" in2="SourceAlpha" operator="in" result="inside"/><feComposite in="outside" in2="inside" operator="arithmetic" k2="1" k3="1"/></filter></defs><path fill="white"/>';
    tab.prepend(surface);prepareTabPreview(tab);
    const size=tabSizes.get(tab);if(size)drawTabSurface(tab,size);
  }
  if(!tab.classList.contains('column-tab'))tabSurfaces.observe(tab);
  if(tab.parentElement?.matches('.tab-list,.drawer-tab-strip'))tabSurfaces.observe(tab.parentElement);
  const [left,right]=presentation.hover_bottom;
  if(tab.dataset.hoverLeft===String(left)&&tab.dataset.hoverRight===String(right))return;
  const size=tabSizes.get(tab);if(size)drawTabSurface(tab,size);
  tab.dataset.hoverLeft=left;tab.dataset.hoverRight=right;
  tab.style.setProperty('--hover-left-radius',`${Math.max(0,left)}px`);
  tab.style.setProperty('--hover-right-radius',`${Math.max(0,right)}px`);
  tab.style.setProperty('--hover-left-foot',hoverFeet[0]);
  tab.style.setProperty('--hover-right-foot',hoverFeet[1]);
}
export function createWorkspaceChrome({app,state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,draggable,grip,contentPanel,tabLabel,automaticTabs,releaseTabs}) {
  const columns=new Map(),drawers=new Map(),catalog=liveCopy(app,"catalog");
  let resolved,animating=false;
  const send=action=>dispatch({type:"customize",action});
  const local=(b,origin)=>({...b,x:b.x-origin.x,y:b.y-origin.y});
  function contentHeight(body) {
    const top=body.getBoundingClientRect().top-body.scrollTop;
    return [...body.children].reduce((height,child)=>{
      const bounds=child.getBoundingClientRect();
      return bounds.height?Math.max(height,bounds.bottom-top+(parseFloat(getComputedStyle(child).marginBottom)||0)):height;
    },0);
  }
  function intersect(a,b) {
    const x=Math.max(a.x,b.x),y=Math.max(a.y,b.y),right=Math.min(a.right,b.right),bottom=Math.min(a.bottom,b.bottom);
    return right>x&&bottom>y?{x,y,width:right-x,height:bottom-y}:null;
  }
  function measureColumnDrawers() {
    const measurements=[];
    for(const r of drawers.values())if(!r.closing&&r.drawer.tabs&&r.placement) {
      const b=r.root.getBoundingClientRect();
      if(b.width>0&&b.height>0)measurements.push({group:r.drawer.tabs.group,bounds:{x:b.x,y:b.y,width:b.width,height:b.height}});
    }
    dispatch({type:"measure_column_drawers",measurements});
  }
  function tabHits() {
    return [...drawers.values()].filter(r=>!r.closing&&r.drawer.tabs&&r.placement).flatMap(r=>{
      const strip=r.root.querySelector('.drawer-tab-strip');if(!strip)return[];
      const clip=intersect(strip.getBoundingClientRect(),r.root.getBoundingClientRect());if(!clip)return[];
      const rect={...clip,right:clip.x+clip.width,bottom:clip.y+clip.height};
      return [...strip.children].flatMap((tab,index)=>{
        const bounds=intersect(tab.getBoundingClientRect(),rect);
        return bounds?[{group:r.drawer.tabs.group,index,bounds}]:[];
      });
    });
  }
  function toolbar(panel,tiles,axis='vertical') {
    const view=customization.view(panel),root=element("div","toolbar-controls");
    root.dataset.panel=panel;root.dataset.axis=axis;customization.present(root,view);
    for(const tile of view.tiles) {
      const node=customization.tileWidget(panel,view,tile);root.append(node);
      if(tiles){const bounds=tiles.find(([id])=>id===tile.id)?.[1];customization.layoutTile(node,bounds,axis);}
    }
    root.refreshPanel=()=>{
      const current=customization.view(panel);customization.present(root,current);
      for(const tile of current.tiles) {
        customization.refreshTile([...root.children].find(n=>Number(n.dataset.tile)===tile.id),tile);
      }
    };
    return root;
  }
  function arrange(layout,layoutOnly=false) {
    resolved=layout;const live=new Set();
    for(const column of layout.collapsed) {
      live.add(column.id);let root=columns.get(column.id);
      // Keep held tiles and their capture alive while geometry or selection changes.
      const key=JSON.stringify(column.groups.map(g=>[g.group,g.icons.map(i=>{
        const v=customization.view(i.panel);return[i.panel,v.title,v.icon];
      })]));
      if(!root){root=element("section","collapsed-column");root.dataset.column=column.id;workspace.append(root);columns.set(column.id,root);}
      place(root,column.bounds);root.dataset.stack=column.stack;
      if(root.dataset.key!==key) {
        root.dataset.key=key;root.replaceChildren();
        customization.target(root,{kind:"column",column:column.id});
        const content=element("div","collapsed-content");root.append(content);
        content.onwheel=e=>{e.preventDefault();const old=state().workspace.layout.column_scroll.find(([id])=>id===column.id)?.[1]||0;dispatch({type:"measure_column_scroll",column:column.id,offset:Math.max(0,old+e.deltaY)});};
        for(const [index,group] of column.groups.entries()) {
          if(index>0) {
            const divider=element('div','tile-divider column-divider');divider.dataset.group=group.group;
            divider.setAttribute('role','separator');divider.setAttribute('aria-orientation','horizontal');content.append(divider);
          }
          for(const item of group.icons) {
            const view=customization.view(item.panel),b=button("",()=>send({type:"toggle_column_drawer",group:group.group,panel:item.panel}),"dock-tab column-tab");
            b.dataset.panel=item.panel;b.title=view.title;b.setAttribute("aria-label",view.title);b.append(icon(view.icon,true));
            customization.target(b,{kind:"panel",panel:item.panel});
            content.append(draggable(b,{kind:"panel",panel:item.panel},"hold"));
          }
        }
        root.append(grip({kind:"column",column:column.id}));
      }
      const content=root.querySelector('.collapsed-content');place(content,local(column.content,column.bounds));
      for(const group of column.groups) {
        const divider=content.querySelector(`.column-divider[data-group="${group.group}"]`);
        if(divider)place(divider,{x:0,y:group.bounds.y-column.content.y-10,width:column.content.width,height:8});
        for(const item of group.icons) {
          const b=content.querySelector(`.column-tab[data-panel="${item.panel}"]`);
          b.setAttribute("aria-selected",String(column.open?group.active===item.panel:state().customization.column_drawers.some(d=>d.anchor.column===column.id&&d.anchor.origin===item.panel)));
          place(b,local(item.bounds,column.content));
          b.style.borderRadius=item.corner_radii.map(r=>`calc(${r}px * var(--corner-fit))`).join(' ');
        }
      }
      place(root.querySelector(':scope > .panel-grip'),local(column.grip,column.bounds));
    }
    for(const[id,node]of columns)if(!live.has(id)){node.remove();columns.delete(id);}
    if(!layoutOnly)refresh();
  }
  function markSource(source,direction) {
    if(!source)return;
    source.dataset.drawerFacing=direction;
    // Flatten only ancestor corners whose rounding reaches this active tile.
    const a=source.getBoundingClientRect(),facing={top:[0,1],right:[1,2],bottom:[2,3],left:[0,3]}[direction],vertical=direction==='top'||direction==='bottom';
    for(let node=source.parentElement;node&&node!==workspace;node=node.parentElement) {
      const b=node.getBoundingClientRect(),corners=[[b.left,b.top],[b.right,b.top],[b.right,b.bottom],[b.left,b.bottom]],style=getComputedStyle(node);
      const radii=[style.borderTopLeftRadius,style.borderTopRightRadius,style.borderBottomRightRadius,style.borderBottomLeftRadius].map(r=>Math.max(.5,parseFloat(r)||0));
      const square=facing.filter(i=>{const[x,y]=corners[i],[rx,ry]=vertical?[radii[i],.5]:[.5,radii[i]];return x>=a.left-rx&&x<=a.right+rx&&y>=a.top-ry&&y<=a.bottom+ry;});
      if(square.length)node.dataset.drawerSourceCorners=[...new Set([...(node.dataset.drawerSourceCorners?.split(' ')||[]),...square])].join(' ');
    }
  }
  function bridge(node,c) {
    if(!node){node=document.createElementNS("http://www.w3.org/2000/svg","svg");node.classList.add("drawer-bridge");node.setAttribute('aria-hidden','true');workspace.append(node);}
    place(node,c.bounds);node.setAttribute("viewBox",`0 0 ${c.bounds.width} ${c.bounds.height}`);
    const path=node.firstElementChild||document.createElementNS(node.namespaceURI,"path"),[a,b]=c.radii,l=c.length,d=c.depth;
    path.setAttribute("d",`M 0 0 L ${l} 0 L ${l} ${d-b} ${squircleCorner([l+b,d-b],[-b,0],[0,b])} L ${-a} ${d} ${squircleCorner([-a,d-a],[0,a],[a,0])} Z`);
    path.setAttribute("transform",`matrix(${c.transform.join(" ")})`);if(!path.parentNode)node.append(path);
    return node;
  }
  function dispose(record) {releaseTabs(record.root);customization.discardFields(record.root);for(const body of record.bodies)for(const child of body.children)child.disposePanel?.();record.bridge?.remove();record.shadow.remove();record.root.remove();}
  function refresh() {
    if(!resolved)return;
    const live=new Set();
    for(const drawer of [...state().customization.column_drawers,...(state().customization.drawer?[state().customization.drawer]:[])]) {
      const column=drawer.anchor.kind==="column"?drawer.anchor.column:null,id=column??"tool";
      live.add(id);
      const {tool_set,layer_properties,...structure}=drawer;
      const tabKey=t=>JSON.stringify(t&&[t.group,[...t.panels].sort()]);
      if(drawer.tabs)structure.tabs=tabKey(drawer.tabs);
      let record=drawers.get(id);const key=JSON.stringify([structure,layer_properties!=null,!!tool_set,drawer.columns.flat().map(id=>state().workspace.layout.panels.find(p=>p.id===id))]);
      if(record?.key!==key) {
        // A new opener must move the body and connector together. Interpolating
        // from the old body leaves the new anchor disconnected during the move.
        const switching=record&&JSON.stringify(record.drawer.anchor)!==JSON.stringify(drawer.anchor);
        const from=record?.placement;
        if(record)for(const body of record.bodies){customization.discardFields(body);for(const child of body.children)child.disposePanel?.();body.remove();}
        const shadow=record?.shadow??element("div","drawer-shadow");shadow.setAttribute("aria-hidden","true");
        const root=record?.root??element("section","content-drawer");
        if(!record){workspace.append(shadow,root);root.dataset.drawer=String(id);root.setAttribute("aria-label","Panel drawer");}
        root.classList.toggle("picker-drawer",!!drawer.compact);
        const tabs=root.querySelector(":scope > .drawer-tabs");
        if(tabs&&tabKey(record.drawer.tabs)!==tabKey(drawer.tabs)){releaseTabs(tabs);tabs.remove();}
        let inspector;
        const bodies=drawer.columns.map(ids=>{
          const body=element("div","drawer-column");
          for(const panel of ids) {
            const view=customization.view(panel),toolbarPanel=state().workspace.layout.panels.find(p=>p.id===panel)?.content.kind==="toolbar";
            let child;
            if(panel==="layers"&&layer_properties!=null){inspector=createLayerInspector({app,state,workspace,element,button,icon,contentPanel,toggle:()=>send({type:"toggle_layer_drawer_properties"}),invalidate:()=>{if(!animating){animating=true;requestAnimationFrame(animate);}}});child=inspector.root;body.classList.add("layer-inspector-column");}
            else if(toolbarPanel){child=toolbar(panel);child.dataset.drawerToolbar=panel;}
            else child=contentPanel(panel,drawer.columns.flat().includes("filter_types"),panel==="brushes"&&tool_set?()=>state().customization.drawer?.tool_set:null);
            body.append(child);
          }
          root.append(body);return body;
        });
        record=Object.assign(record??{},{key,root,shadow,bodies,column,drawer,inspector,from,started:performance.now()-(switching?catalog.panel_expansion_ms:0),placement:from});drawers.set(id,record);
      }
      record.drawer=drawer;
      record.tabStyle=drawer.tabs?app.group_tab_style(drawer.tabs.group):null;
      const header=record.root.querySelector(':scope > .drawer-tabs');
      if(header&&drawer.tabs&&header.dataset.tabStyle!==record.tabStyle){releaseTabs(header);header.remove();}
      else if(header&&drawer.tabs&&orderPanelTabs(header.firstElementChild,drawer.tabs.panels)&&record.tabStyle==='automatic')automaticTabs(header.firstElementChild);
      record.closing=false;
      record.root.inert=false;
      record.inspector?.refresh(layer_properties);
      for(const body of record.bodies)for(const child of body.children)child.refreshPanel?.();
    }
    for(const[id,r]of drawers)if(!live.has(id)&&!r.closing){r.closing=true;r.root.inert=true;r.from=r.placement;r.started=performance.now();}
    measureColumnDrawers();
    if(!animating){animating=true;requestAnimationFrame(animate);}
  }
  function animate(now) {
    animating=false;let more=false;const measurements=[];
    for(const node of workspace.querySelectorAll('[data-drawer-facing]'))delete node.dataset.drawerFacing;
    for(const node of workspace.querySelectorAll('[data-drawer-source-corners]'))delete node.dataset.drawerSourceCorners;
    for(const[id,r]of drawers) {
      const measured=r.placement;
      if(!r.closing) for(const [index,body] of r.bodies.entries()) {
        const width=measured?.columns?.[index]?.width||232;
        body.style.width=`${width}px`;
        for(const strip of body.querySelectorAll("[data-drawer-toolbar]")) {
          const geometry=app.drawer_toolbar(strip.dataset.drawerToolbar,width);
          strip.style.height=`${geometry.content_height}px`;
          [...strip.children].forEach((tile,i)=>customization.layoutTile(tile,geometry.tiles[i],strip.dataset.axis,geometry.tile_style));
        }
      }
      const page=r.inspector?.measure(now);
      const heights=r.bodies.map(body=>page?.height??contentHeight(body)+(r.drawer.tabs?resolved.tab_bar_height:0));
      const progress=matchMedia("(prefers-reduced-motion: reduce)").matches?1:Math.min(1,(now-r.started)/catalog.panel_expansion_ms);
      const result=app.drawer({viewport:[workspace.clientWidth,workspace.clientHeight],column:r.column,heights,progress,from:r.from??null,closing:!!r.closing});
      if(!result || (r.closing&&progress===1)){dispose(r);drawers.delete(id);continue;}
      r.placement=result.placement;r.connection=result.connection;place(r.root,r.placement.bounds);
      place(r.shadow,r.placement.bounds);
      const anchor=r.drawer.anchor;
      const source=anchor.kind==="header"?workspace.querySelector(`[data-header-item="${anchor.id}"]:not([hidden]) .header-tool`):r.column!=null?workspace.querySelector(`.collapsed-column[data-column="${r.column}"] .column-tab[data-panel="${anchor.origin}"]`):
        [...workspace.querySelectorAll(`.toolbar-controls[data-panel="${anchor.panel}"] > [data-tile="${anchor.tile}"] > button`)].find(node=>node.getBoundingClientRect().width>0);
      r.shadow.style.zIndex=source?.closest('.content-drawer')?"1798":"0";
      if(source&&r.connection)markSource(source,r.placement.direction);
      if(r.connection) {
        const c=r.connection;
        r.bridge=bridge(r.bridge,c);r.bridge.style.zIndex=r.column==null?"1899":"1799";
        r.root.style.borderRadius=c.square_corners.map(square=>square?"0":"var(--surface-radius)").join(" ");
        const b=r.placement.bounds,x=c.bounds.x-b.x,y=c.bounds.y-b.y;
        r.shadow.style.clipPath=`path(evenodd,"M-64 -64H${b.width+64}V${b.height+64}H-64Z M${x} ${y}h${c.bounds.width}v${c.bounds.height}h${-c.bounds.width}Z")`;
      } else {r.bridge?.remove();r.bridge=null;r.root.style.borderRadius="var(--surface-radius)";r.shadow.style.clipPath="";}
      r.shadow.style.borderRadius=r.root.style.borderRadius;

      r.root.style.zIndex=r.column==null?"1900":"1800";
      r.placement.columns.forEach((bounds,i)=>{
        more ||= bounds.width!==(measured?.columns?.[i]?.width||232);
        place(r.bodies[i],bounds);
      });
      r.inspector?.layout();
      if(r.drawer.tabs) {
        let tabs=r.root.querySelector(":scope > .drawer-tabs");
        if(!tabs&&!r.closing){
          const item={kind:"group",group:r.drawer.tabs.group};
          tabs=draggable(element("nav","drawer-tabs"),item);customization.target(tabs,item);r.root.append(tabs);
          tabs.dataset.tabStyle=r.tabStyle;
          const strip=element("div","drawer-tab-strip"),automatic=tabs.dataset.tabStyle==="automatic";tabs.append(strip);
          for(const panel of r.drawer.tabs.panels){
            const v=customization.view(panel),b=button("",()=>dispatch({type:"select_panel_tab",group:r.drawer.tabs.group,panel}),"dock-tab");
            b.dataset.panel=panel;b.dataset.group=r.drawer.tabs.group;b.setAttribute("aria-label",v.title);
            customization.target(b,{kind:"panel",panel});strip.append(draggable(b,{kind:"panel",panel}));
          }
          if(automatic)automaticTabs(strip);
          const handle=grip(item);handle.classList.add("column-drawer-grip");tabs.append(handle);
        }
        if(tabs) {
        const automatic=tabs.dataset.tabStyle==="automatic";
        if(!r.closing)for(const b of tabs.querySelectorAll('.dock-tab')) {
          const v=customization.view(b.dataset.panel),key=JSON.stringify([v.title,v.icon,automatic?null:[v.tab.show_icon,v.tab.show_name]]);
          b.setAttribute('aria-selected',String(b.dataset.panel===r.drawer.tabs.active));
          tabHover(b,v.tab);
          if(b.dataset.labelKey!==key){b.dataset.labelKey=key;b.replaceChildren();tabLabel(b,v,automatic);b.title=v.title;b.setAttribute('aria-label',v.title);if(automatic)automaticTabs(b.parentElement);}
        }
        // Shared placement includes tabs in the first column body.
        const body=r.bodies[0],b=r.placement.columns[0];place(tabs,{...b,height:resolved.tab_bar_height});
        place(body,{...b,y:b.y+resolved.tab_bar_height,height:Math.max(0,b.height-resolved.tab_bar_height)});
        }
      }
      if(r.column!=null)for(const tile of r.root.querySelectorAll(".toolbar-controls > [data-tile]")) {
        const b=tile.getBoundingClientRect(),p=r.root.getBoundingClientRect();
        const x=Math.max(b.x,p.x),y=Math.max(b.y,p.y),right=Math.min(b.right,p.right),bottom=Math.min(b.bottom,p.bottom);
        if(right>x&&bottom>y)measurements.push({column:r.column,anchor:{panel:tile.parentElement.dataset.panel,tile:Number(tile.dataset.tile)},bounds:{x,y,width:right-x,height:bottom-y}});
      }
      more ||= progress<1||!!page?.running;
    }
    // Geometry is transient and Rust suppresses identical measurement updates.
    dispatch({type:"measure_drawer_tiles",measurements});
    measureColumnDrawers();
    editor.queuePositions();
    if(more&&!animating){animating=true;requestAnimationFrame(animate);}
  }
  workspace.addEventListener("scroll",()=>{if(drawers.size&&!animating){animating=true;requestAnimationFrame(animate);}},true);
  function connectionList(){return [...drawers.values()].map(r=>r.connection).filter(Boolean);}
  return {arrange,refresh,tabHits,measureColumnDrawers,connections:connectionList,facts(){const r=drawers.get("tool");return{content_drawer:r?.placement?.bounds??null,drawer_connection:r?.connection?.bounds??null};}};
}
