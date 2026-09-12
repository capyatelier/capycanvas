// Shared dock projections. The browser supplies widgets and measured body sizes.
export function createWorkspaceChrome({app,state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,draggable,grip,contentPanel}) {
  const columns=new Map(),drawers=new Map(),zen=element("div","zen-toolbars");
  workspace.append(zen);
  let resolved,zenKey="",animating=false;
  const send=action=>dispatch({type:"customize",action});
  const local=(b,origin)=>({...b,x:b.x-origin.x,y:b.y-origin.y});
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
    root.dataset.panel=panel;root.dataset.tileStyle=view.tile_style;root.dataset.axis=axis;
    root.style.setProperty("--tile-icon-size",`${view.tile_icon_size}px`);
    root.dataset.labeled=String(view.tile_label_lines>0);
    root.style.setProperty("--tile-label-lines",view.tile_label_lines);
    root.style.setProperty("--tile-label-weight",view.tile_label_bold?700:400);
    for(const tile of view.tiles) {
      const node=element("div","tile-button tool-tile");node.dataset.tile=tile.id;
      if(tile.control.kind==="divider"){node.classList.add("tile-divider");node.setAttribute("role","separator");}
      else {const b=button("",()=>dispatch({type:"activate_tile",panel,tile:tile.id}));
        b.disabled=!tile.enabled;b.title=tile.tooltip;b.setAttribute("aria-label",tile.label);b.setAttribute("aria-pressed",tile.selected);
        b.append(icon(tile.icon));if(view.tile_label_lines>0) b.append(element("span","tile-label",tile.label));node.append(b);}
      customization.target(node,{kind:"tile",panel,tile:tile.id});
      root.append(draggable(node,{kind:"tile",panel,tile:tile.id}));
      if(tiles){const bounds=tiles.find(([id])=>id===tile.id)?.[1];if(bounds)place(node,bounds);else node.hidden=true;}
    }
    root.refreshPanel=()=>{
      const current=customization.view(panel);
      root.style.setProperty("--tile-icon-size",`${current.tile_icon_size}px`);
      for(const tile of current.tiles) {
        const row=[...root.children].find(n=>Number(n.dataset.tile)===tile.id),b=row?.querySelector("button");
        if(!b)continue;b.disabled=!tile.enabled;b.title=tile.tooltip;b.setAttribute("aria-pressed",String(tile.selected));b.setAttribute("aria-label",tile.label);
        const glyph=b.querySelector("svg");if(glyph?.dataset.asset!==tile.icon)glyph?.replaceWith(icon(tile.icon));
      }
    };
    return root;
  }
  function arrange(layout) {
    resolved=layout;const live=new Set();
    for(const column of layout.collapsed) {
      live.add(column.id);let root=columns.get(column.id);
      const key=JSON.stringify([column,state().customization.column_drawers,state().workspace.layout.panels.map(p=>[p.id,p.name])]);
      if(root?.dataset.key===key)continue;
      if(!root){root=element("section","collapsed-column");root.dataset.column=column.id;workspace.append(root);columns.set(column.id,root);}
      root.dataset.key=key;root.replaceChildren();place(root,column.bounds);
      customization.target(root,{kind:"group",group:column.groups[0]?.group ?? column.id});
      const expand=button("»",()=>send({type:"set_column_collapsed",group:column.id,collapsed:false}),"column-expand");
      expand.setAttribute("aria-label","Expand column");place(expand,local(column.expand,column.bounds));root.append(expand);
      const content=element("div","collapsed-content");place(content,local(column.content,column.bounds));root.append(content);
      content.onwheel=e=>{e.preventDefault();const old=state().workspace.layout.column_scroll.find(([id])=>id===column.id)?.[1]||0;dispatch({type:"measure_column_scroll",column:column.id,offset:Math.max(0,old+e.deltaY)});};
      for(const group of column.groups) {
        const b=group.bounds,divider=element('div','tile-divider column-divider');
        divider.setAttribute('role','separator');divider.setAttribute('aria-orientation','horizontal');
        place(divider,{x:0,y:b.y-column.content.y-10,width:column.content.width,height:8});
        content.append(divider);
        for(const item of group.icons) {
          const view=customization.view(item.panel),b=button("",()=>send({type:"toggle_column_drawer",group:group.group,panel:item.panel}),"dock-tab column-tab");
          b.dataset.panel=item.panel;b.title=view.title;b.setAttribute("aria-label",view.title);b.append(icon(view.icon));
          b.setAttribute("aria-selected",String(state().customization.column_drawers.some(d=>d.anchor.column===column.id&&d.anchor.origin===item.panel)));
          place(b,local(item.bounds,column.content));customization.target(b,{kind:"panel",panel:item.panel});
          content.append(draggable(b,{kind:"panel",panel:item.panel}));
        }
      }
      const handle=grip({kind:"column",column:column.id});place(handle,local(column.grip,column.bounds));root.append(handle);
    }
    for(const[id,node]of columns)if(!live.has(id)){node.remove();columns.delete(id);}
    refresh();
  }
  function dispose(record) {customization.discardFields(record.root);for(const body of record.bodies)for(const child of body.children)child.disposePanel?.();record.bridge?.remove();record.shadow.remove();record.root.remove();}
  function refresh() {
    if(!resolved)return;
    const [partial_zen,zen_toolbars]=app.workspace_projection(workspace.clientWidth,workspace.clientHeight);
    const model={partial_zen,zen_toolbars};
    workspace.classList.toggle("partial-zen",model.partial_zen);
    const key=model.partial_zen ? JSON.stringify([model.zen_toolbars,model.zen_toolbars.sections.map(s=>customization.view(s.panel)?.tiles)]) : "";
    if(key!==zenKey) {
      zenKey=key;zen.replaceChildren();
      for(const section of model.zen_toolbars.sections) {
        const root=element("div","zen-toolbar");root.dataset.edge=section.edge;place(root,section.bounds);
        root.append(toolbar(section.panel,section.tiles,['left','right'].includes(section.edge)?'vertical':'horizontal'));zen.append(root);
      }
    }
    const live=new Set();
    for(const drawer of [...state().customization.column_drawers,...(state().customization.drawer?[state().customization.drawer]:[])]) {
      const column=drawer.anchor.kind==="column"?drawer.anchor.column:null,id=column??"tool";
      live.add(id);
      let record=drawers.get(id);const key=JSON.stringify([drawer,drawer.columns.flat().map(id=>state().workspace.layout.panels.find(p=>p.id===id))]);
      if(record?.key!==key) {
        // A new opener must move the body and connector together. Interpolating
        // from the old body leaves the new anchor disconnected during the move.
        const switching=record&&JSON.stringify(record.drawer.anchor)!==JSON.stringify(drawer.anchor);
        const from=record?.placement;if(record)dispose(record);
        const shadow=element("div","drawer-shadow");shadow.setAttribute("aria-hidden","true");workspace.append(shadow);
        const root=element("section","content-drawer");root.dataset.drawer=String(id);root.setAttribute("aria-label","Panel drawer");workspace.append(root);
        const bodies=drawer.columns.map(ids=>{
          const body=element("div","drawer-column");
          for(const panel of ids) {
            const view=customization.view(panel),toolbarPanel=state().workspace.layout.panels.find(p=>p.id===panel)?.content.kind==="toolbar";
            let child;
            if(toolbarPanel){child=toolbar(panel);child.dataset.drawerToolbar=panel;}
            else child=contentPanel(panel);
            body.append(child);
          }
          root.append(body);return body;
        });
        record={key,root,shadow,bodies,column,drawer,from,started:performance.now()-(switching?160:0),placement:from};drawers.set(id,record);
      }
      record.closing=false;
      record.root.inert=false;
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
      const width=r.placement?.columns?.[0]?.width||232;
      if(!r.closing) for(const body of r.bodies) {
        body.style.width=`${width}px`;
        for(const strip of body.querySelectorAll("[data-drawer-toolbar]")) {
          const geometry=app.drawer_toolbar(strip.dataset.drawerToolbar,width);
          strip.style.height=`${geometry.content_height}px`;
          [...strip.children].forEach((tile,i)=>place(tile,geometry.tiles[i]));
        }
      }
      const heights=r.bodies.map(body=>body.scrollHeight+(r.drawer.tabs?resolved.tab_bar_height:0));
      const progress=matchMedia("(prefers-reduced-motion: reduce)").matches?1:Math.min(1,(now-r.started)/160);
      const result=app.drawer({viewport:[workspace.clientWidth,workspace.clientHeight],column:r.column,heights,progress,from:r.from??null,closing:!!r.closing});
      if(!result || (r.closing&&progress===1)){dispose(r);drawers.delete(id);continue;}
      r.placement=result.placement;r.connection=result.connection;place(r.root,r.placement.bounds);
      place(r.shadow,r.placement.bounds);
      const anchor=r.drawer.anchor;
      const source=r.column!=null?workspace.querySelector(`.collapsed-column[data-column="${r.column}"] .column-tab[data-panel="${anchor.origin}"]`):
        [...workspace.querySelectorAll(`.toolbar-controls[data-panel="${anchor.panel}"] > [data-tile="${anchor.tile}"] > button`)].find(node=>node.getBoundingClientRect().width>0);
      r.shadow.style.zIndex=source?.closest('.content-drawer')?"1798":"0";
      if(source&&r.connection&&!r.closing) {
        source.dataset.drawerFacing=r.placement.direction;
        // Flatten only ancestor corners reached by the source, so clipping cannot cut its join.
        const a=source.getBoundingClientRect(),facing={top:[0,1],right:[1,2],bottom:[2,3],left:[0,3]}[r.placement.direction];
        for(let node=source.parentElement;node&&node!==workspace;node=node.parentElement) {
          const b=node.getBoundingClientRect(),corners=[[b.left,b.top],[b.right,b.top],[b.right,b.bottom],[b.left,b.bottom]];
          const square=facing.filter(i=>{const[x,y]=corners[i];return x>=a.left-.5&&x<=a.right+.5&&y>=a.top-.5&&y<=a.bottom+.5;});
          if(square.length)node.dataset.drawerSourceCorners=[...new Set([...(node.dataset.drawerSourceCorners?.split(' ')||[]),...square])].join(' ');
        }
      }
      if(r.connection) {
        const c=r.connection;
        if(!r.bridge){r.bridge=document.createElementNS("http://www.w3.org/2000/svg","svg");r.bridge.classList.add("drawer-bridge");workspace.append(r.bridge);}
        place(r.bridge,c.bounds);r.bridge.style.zIndex=r.column==null?"1899":"1799";
        r.bridge.setAttribute("viewBox",`0 0 ${c.bounds.width} ${c.bounds.height}`);
        const path=document.createElementNS(r.bridge.namespaceURI,"path"),[a,b]=c.radii,l=c.length,d=c.depth,k=.5522848;
        path.setAttribute("d",`M 0 0 L ${l} 0 L ${l} ${d-b} C ${l} ${d-b+b*k} ${l+b-b*k} ${d} ${l+b} ${d} L ${-a} ${d} C ${-a+a*k} ${d} 0 ${d-a+a*k} 0 ${d-a} Z`);
        path.setAttribute("transform",`matrix(${c.transform.join(" ")})`);r.bridge.replaceChildren(path);
        r.root.style.borderRadius=c.square_corners.map(square=>square?"0":"8px").join(" ");
      } else {r.bridge?.remove();r.bridge=null;r.root.style.borderRadius="8px";}
      r.shadow.style.borderRadius=r.root.style.borderRadius;

      r.root.style.zIndex=r.column==null?"1900":"1800";
      r.placement.columns.forEach((bounds,i)=>place(r.bodies[i],bounds));
      if(r.drawer.tabs) {
        let tabs=r.root.querySelector(":scope > .drawer-tabs");
        if(!tabs){
          const item={kind:"group",group:r.drawer.tabs.group};
          tabs=draggable(element("nav","drawer-tabs"),item);customization.target(tabs,item);r.root.append(tabs);
          const strip=element("div","drawer-tab-strip");tabs.append(strip);
          for(const panel of r.drawer.tabs.panels){
            const v=customization.view(panel),b=button("",()=>dispatch({type:"select_panel_tab",group:r.drawer.tabs.group,panel}),"dock-tab");
            if(v.tab.show_icon)b.append(icon(v.icon));if(v.tab.show_name)b.append(document.createTextNode(v.title));
            b.dataset.panel=panel;b.setAttribute("aria-label",v.title);b.setAttribute("aria-selected",String(panel===r.drawer.tabs.active));
            customization.target(b,{kind:"panel",panel});strip.append(draggable(b,{kind:"panel",panel}));
          }
          const handle=grip(item);handle.classList.add("column-drawer-grip");handle.setAttribute("aria-label","Move panel group");tabs.append(handle);
        }
        // Shared placement includes tabs in the first column body.
        const body=r.bodies[0],b=r.placement.columns[0];place(tabs,{...b,height:resolved.tab_bar_height});
        place(body,{...b,y:b.y+resolved.tab_bar_height,height:Math.max(0,b.height-resolved.tab_bar_height)});
      }
      if(r.column!=null)for(const tile of r.root.querySelectorAll(".toolbar-controls > [data-tile]")) {
        const b=tile.getBoundingClientRect(),p=r.root.getBoundingClientRect();
        const x=Math.max(b.x,p.x),y=Math.max(b.y,p.y),right=Math.min(b.right,p.right),bottom=Math.min(b.bottom,p.bottom);
        if(right>x&&bottom>y)measurements.push({column:r.column,anchor:{panel:tile.parentElement.dataset.panel,tile:Number(tile.dataset.tile)},bounds:{x,y,width:right-x,height:bottom-y}});
      }
      more ||= progress<1;
    }
    // Geometry is transient and Rust suppresses identical measurement updates.
    dispatch({type:"measure_drawer_tiles",measurements});
    measureColumnDrawers();
    editor.queuePositions();
    if(more&&!animating){animating=true;requestAnimationFrame(animate);}
  }
  workspace.addEventListener("scroll",()=>{if(drawers.size&&!animating){animating=true;requestAnimationFrame(animate);}},true);
  return {arrange,refresh,tabHits,measureColumnDrawers,facts(){const r=drawers.get("tool");return{content_drawer:r?.placement?.bounds??null,drawer_connection:r?.connection?.bounds??null};}};
}
