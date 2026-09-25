// Shared dock projections. The browser supplies widgets and measured body sizes.
export function createWorkspaceChrome({app,state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,draggable,grip,contentPanel,tabLabel,automaticTabs,releaseTabs}) {
  const columns=new Map(),drawers=new Map(),connections=new Map();
  let resolved,animating=false;
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
    root.style.setProperty("--tile-radius",`${view.tile_corner_radius}px`);
    root.dataset.labeled=String(view.tile_label_lines>0);
    root.style.setProperty("--tile-label-lines",view.tile_label_lines);
    root.style.setProperty("--tile-label-weight",view.tile_label_bold?700:400);
    for(const tile of view.tiles) {
      const node=customization.tileWidget(panel,view,tile);root.append(node);
      if(tiles){const bounds=tiles.find(([id])=>id===tile.id)?.[1];customization.layoutTile(node,bounds,axis);}
    }
    root.refreshPanel=()=>{
      const current=customization.view(panel);
      root.style.setProperty("--tile-icon-size",`${current.tile_icon_size}px`);
      root.style.setProperty("--tile-radius",`${current.tile_corner_radius}px`);
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
            b.dataset.panel=item.panel;b.title=view.title;b.setAttribute("aria-label",view.title);b.append(icon(view.icon));
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
        }
      }
      place(root.querySelector(':scope > .panel-grip'),local(column.grip,column.bounds));
    }
    for(const[id,node]of columns)if(!live.has(id)){node.remove();columns.delete(id);}
    if(layoutOnly)openConnections();else refresh();
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
  function squircleCorner([cx,cy],[sx,sy],[ex,ey]) {
    return Array.from({length:24},(_,i)=>{
      const angle=(i+1)*Math.PI/48,along=Math.sqrt(Math.cos(angle)),across=Math.sqrt(Math.sin(angle));
      return `L ${cx+sx*along+ex*across} ${cy+sy*along+ey*across}`;
    }).join(" ");
  }
  function bridge(node,c) {
    if(!node){node=document.createElementNS("http://www.w3.org/2000/svg","svg");node.classList.add("drawer-bridge");node.setAttribute('aria-hidden','true');workspace.append(node);}
    place(node,c.bounds);node.setAttribute("viewBox",`0 0 ${c.bounds.width} ${c.bounds.height}`);
    const path=node.firstElementChild||document.createElementNS(node.namespaceURI,"path"),[a,b]=c.radii,l=c.length,d=c.depth;
    path.setAttribute("d",`M 0 0 L ${l} 0 L ${l} ${d-b} ${squircleCorner([l+b,d-b],[-b,0],[0,b])} L ${-a} ${d} ${squircleCorner([-a,d-a],[0,a],[a,0])} Z`);
    path.setAttribute("transform",`matrix(${c.transform.join(" ")})`);if(!path.parentNode)node.append(path);
    return node;
  }
  function openConnections() {
    const live=new Set();
    for(const column of resolved.collapsed)if(column.open)for(const[panel,c]of column.open.connections) {
      const id=`${column.id}:${panel}`;live.add(id);
      const node=bridge(connections.get(id),c);node.classList.add('column-connection');connections.set(id,node);
      markSource(columns.get(column.id)?.querySelector(`.column-tab[data-panel="${panel}"]`),column.open.direction);
    }
    for(const[id,node]of connections)if(!live.has(id)){node.remove();connections.delete(id);}
  }
  function dispose(record) {releaseTabs(record.root);customization.discardFields(record.root);for(const body of record.bodies)for(const child of body.children)child.disposePanel?.();record.bridge?.remove();record.shadow.remove();record.root.remove();}
  function refresh() {
    if(!resolved)return;
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
        const root=element("section","content-drawer");root.dataset.drawer=String(id);root.classList.toggle("picker-drawer",!!drawer.compact);root.setAttribute("aria-label","Panel drawer");workspace.append(root);
        const bodies=drawer.columns.map(ids=>{
          const body=element("div","drawer-column");
          for(const panel of ids) {
            const view=customization.view(panel),toolbarPanel=state().workspace.layout.panels.find(p=>p.id===panel)?.content.kind==="toolbar";
            let child;
            if(toolbarPanel){child=toolbar(panel);child.dataset.drawerToolbar=panel;}
            else child=contentPanel(panel,drawer.columns.flat().includes("filter_types"));
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
          [...strip.children].forEach((tile,i)=>customization.layoutTile(tile,geometry.tiles[i],strip.dataset.axis));
        }
      }
      const heights=r.bodies.map(body=>body.scrollHeight+(r.drawer.tabs?resolved.tab_bar_height:0));
      const progress=matchMedia("(prefers-reduced-motion: reduce)").matches?1:Math.min(1,(now-r.started)/160);
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
      r.placement.columns.forEach((bounds,i)=>place(r.bodies[i],bounds));
      if(r.drawer.tabs) {
        let tabs=r.root.querySelector(":scope > .drawer-tabs");
        if(!tabs){
          const item={kind:"group",group:r.drawer.tabs.group};
          tabs=draggable(element("nav","drawer-tabs"),item);customization.target(tabs,item);r.root.append(tabs);
          const strip=element("div","drawer-tab-strip"),automatic=app.group_tab_style(r.drawer.tabs.group)==="automatic";tabs.append(strip);
          for(const panel of r.drawer.tabs.panels){
            const v=customization.view(panel),b=button("",()=>dispatch({type:"select_panel_tab",group:r.drawer.tabs.group,panel}),"dock-tab");
            if(automatic)tabLabel(b,v,true);else{if(v.tab.show_icon)b.append(icon(v.icon));if(v.tab.show_name)b.append(document.createTextNode(v.title));}
            b.dataset.panel=panel;b.setAttribute("aria-label",v.title);b.setAttribute("aria-selected",String(panel===r.drawer.tabs.active));
            customization.target(b,{kind:"panel",panel});strip.append(draggable(b,{kind:"panel",panel}));
          }
          if(automatic)automaticTabs(strip);
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
    openConnections();
    // Geometry is transient and Rust suppresses identical measurement updates.
    dispatch({type:"measure_drawer_tiles",measurements});
    measureColumnDrawers();
    editor.queuePositions();
    if(more&&!animating){animating=true;requestAnimationFrame(animate);}
  }
  workspace.addEventListener("scroll",()=>{if(drawers.size&&!animating){animating=true;requestAnimationFrame(animate);}},true);
  function connectionList(){return[...[...drawers.values()].map(r=>r.connection).filter(Boolean),...(resolved?.collapsed??[]).flatMap(c=>c.open?.connections.map(([,connection])=>connection)??[])];}
  return {arrange,refresh,tabHits,measureColumnDrawers,connections:connectionList,facts(){const r=drawers.get("tool");return{content_drawer:r?.placement?.bounds??null,drawer_connection:r?.connection?.bounds??null};}};
}
