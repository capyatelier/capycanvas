import {createBubbleInput} from './bubble-input.js';
import {colorTouchInput} from './color-touch.js';
import {createLayerInspector} from './layer-inspector.js';
import {wheelPainter,hueStopCache,rgba,json} from './color-wheel.js';
import {createLayerThumbnails} from './layers.js';
import {bubbleColor} from './bubble-color.js';
import {bubbleShadow} from './bubble-shadow.js';

const svgNS='http://www.w3.org/2000/svg';
const smooth=t=>t*t*(3-2*t);
const ease=t=>1-(1-t)**3;
const duration=(reduced,ms)=>reduced?0:ms;
const place=(node,center,size)=>Object.assign(node.style,{left:`${center[0]-size[0]/2}px`,top:`${center[1]-size[1]/2}px`,width:`${size[0]}px`,height:`${size[1]}px`});
const svg=(tag,attributes={})=>{const n=document.createElementNS(svgNS,tag);for(const [k,v]of Object.entries(attributes))n.setAttribute(k,v);return n;};
const path=(a,b,r,sweep)=>`M${a} A${r} ${r} 0 ${Math.abs(sweep)>180?1:0} ${sweep>0?1:0} ${b}`;
const cornerShape=CSS.supports('corner-shape','squircle');
const itemScale=node=>node.classList.contains('bubble-hot')?(node.classList.contains('bubble-brush')?1.06:1.12):1;
const enabledAlpha=node=>node.classList.contains('bubble-disabled')?.45:1;

export function createBubbles({app,workspace,element,button,icon,state,dispatch,applyChange,wake,glassChanged,customization,contentPanel,rasterWorker,asset,cancelWorkspace,viewport,leaveCanvas}) {
  const root=element('div','tool-bubbles'),stampCanvas=element('canvas','bubble-stamp');
  root.id='tool-bubbles';workspace.append(stampCanvas,root);
  const records=new Map(),reduced=matchMedia('(prefers-reduced-motion: reduce)'),hueStops=hueStopCache(app);
  const thumbnails=createLayerThumbnails(app,wake);
  let dirty=true,active=false,stamp,stampBusy=false,stampPending=false,stampWanted=null,stampGeneration=0,stroke=false,restoreAt=null;
  const send=(action,plain=false)=>{dispatch(plain?action:{type:'bubble',action});active=true;wake();};
  const menu=(r,p)=>{r.root.menuModel=()=>app.context_menu({kind:'bubble',id:r.view.id});customization.openMenu(r.root,p);};
  const input=createBubbleInput({app,workspace,records,send,wake,menu,dismiss:()=>customization.dismissContext(),cancelWorkspace,viewport,leaveCanvas,colorTouch:colorTouchInput({app,applyChange,wake})});
  function surface(r,node,center,size,radius=size[1]/2,shape=0) {
    place(node,center,size);const box={node,center,size,radius,shape};node.bubbleSurface=box;r.boxes.push(box);return node;
  }
  function glyph(node,name) {
    if(node.dataset.glyph!==name){node.replaceChildren(icon(name));node.dataset.glyph=name;}
  }
  function transition(r,key,node,show,ms,fromScale,delay=0) {
    const previous=r.transitions.get(key);
    if(previous?.show===show)return;
    const value=previous?.value??(show?0:1);
    const t={show,node,from:value,value,to:show?1:0,start:null,ms:duration(reduced.matches,ms),fromScale,delay:duration(reduced.matches,delay)};
    r.transitions.set(key,t);node.bubbleTransition=t;
    node.hidden=false;active=true;
  }
  function retire(r,node) {
    if(!node||r.retiring.includes(node))return;
    node.inert=true;
    transition(r,node,node,false,120,.6);r.retiring.push(node);
  }
  function record(packet) {
    const v=packet.view,n=element('div',`tool-bubble bubble-${v.kind}`);n.dataset.bubble=v.id;
    const body=element('div','bubble-body');n.append(body);root.append(n);
    const face=button('',()=>send({type:'tap',id:v.id,position:[0,0]}),'bubble-face bubble-surface');
    face.setAttribute('aria-label',v.label);body.append(face);
    const halo=svg('svg',{'class':'bubble-halo',overflow:'visible'});body.prepend(halo);
    const chooser=element('div','bubble-chooser'),beads=element('div','bubble-beads color-wheel-control');body.append(chooser,beads);
    const readout=element('div','bubble-readout bubble-surface');readout.append(element('span'));body.append(readout);
    const r={root:n,body,face,halo,chooser,beads,readout,packet,view:v,center:[0,0],boxes:[],arcs:[],retiring:[],transitions:new Map(),items:new Map(),beadNodes:new Map(),ringNodes:new Map(),fade:1,fadeTarget:1,pinch:1,scale:1};
    r.mark=(part,pressed)=>{
      const key=json([part,pressed]);if(r.markKey===key)return;r.markKey=key;
      for(const node of [face,...r.items.values()].map(x=>x.node??x).concat(r.halves||[])) {
        node.classList.remove('bubble-hovered','bubble-pressed','bubble-hot');
      }
      const node=part==='face'&&!['color','history'].includes(v.kind)?face:part?.half!=null?r.halves?.[part.half]:part?.item!=null?r.items.get(part.item)?.node:null;
      if(node){node.classList.add(pressed?'bubble-pressed':'bubble-hovered');if(part?.item!=null&&pressed)node.classList.add('bubble-hot');}
      r.hotItem=part?.item;active=true;wake();
    };
    if(v.kind==='color') {
      r.wheel=element('canvas','bubble-wheel');face.append(r.wheel);
      r.painter=wheelPainter({canvas:r.wheel,worker:rasterWorker,request:side=>app.color_field_request(side),hueStops,repaint:()=>{r.wheelKey=null;dirty=true;wake();}});
    }
    if(v.kind==='history') {
      r.halves=v.buttons.map((b,i)=>{const node=button('',()=>send({type:'tap',id:v.id,position:packet.history[i]}),'bubble-half');face.append(node);return node;});
    }
    return r;
  }
  function refreshRecord(r,packet) {
    const v=packet.view,previous=r.view;r.view=v;r.packet=packet;r.boxes=[];
    r.root.setAttribute('aria-label',v.label);r.face.setAttribute('aria-label',v.label);r.face.title=v.label;
    r.face.classList.toggle('bubble-selected',v.selected&&['dial','brush','medium','controls'].includes(v.kind));r.face.classList.toggle('bubble-disabled',!v.enabled);
    surface(r,r.face,[0,0],v.face,v.kind==='layers'?20:v.face[1]/2,v.kind==='layers'?1:0);
    if(v.kind==='color') {
      const view=app.color_panel(),side=v.diameter-14,key=json([view,side,devicePixelRatio]);
      const color=packet.color;
      if(color){r.color??=bubbleColor(r.face,r.body,v.id,send);r.color.refresh(color);}
      else if(r.color){r.color.dispose();r.color=null;}
      if(color){const b=color.bounds;place(r.face,[b.x+b.width/2,b.y+b.height/2],[b.width,b.height]);r.boxes.pop();}
      place(r.wheel,color?[-color.bounds.x,-color.bounds.y]:[v.face[0]/2,v.face[1]/2],[side,side]);
      if(r.wheelKey!==key){r.wheelKey=key;r.painter.paint(view,side,Math.ceil(side*Math.min(2,devicePixelRatio||1)));}
    } else if(v.kind==='history') {
      r.halves.forEach((node,i)=>{const b=v.buttons[i];glyph(node,b.icon);node.disabled=!b.enabled;node.title=b.label;node.setAttribute('aria-label',b.label);place(node,[v.face[0]/2+packet.history[i][0],v.face[1]/2],[v.face[1],v.face[1]]);});
    } else {
      const thumbnailOwner=v.kind==='layers'?json([state().document_file.epoch,v.layer]):null;
      const key=json([v.icon,v.preview,v.variants,v.dots,state().theme,thumbnailOwner,v.layers]);
      if(r.faceKey!==key) {
        r.faceKey=key;r.face.replaceChildren();
        if(v.preview!=null){const img=element('img','bubble-brush-preview');img.src=asset(`brush-previews/${v.preview}-${state().theme}.webp`);img.alt='';img.draggable=false;r.face.append(img);}
        r.face.append(icon(v.icon));
        if(v.variants)r.face.append(customization.variationMarker());
        for(const[center,i]of packet.dots.map((center,i)=>[center,i])){const dot=element('i',`bubble-dot${v.dots[i]?' selected':''}`);place(dot,center.map(x=>x+v.face[0]/2),[3.5,3.5]);r.face.append(dot);}
        if(v.kind==='layers') {
          if(r.thumbnailOwner!==thumbnailOwner){r.thumbnailOwner=thumbnailOwner;r.thumbnail=element('canvas','bubble-layer-thumbnail');r.thumbnail.width=r.thumbnail.height=64;}
          r.face.replaceChildren(r.thumbnail);
          const badge=element('span','bubble-count bubble-surface');badge.append(element('span','',String(v.layers)));r.face.append(badge);
          r.badge=badge;
        }
      }
      if(r.badge) {
        const hidden=r.face.hidden;r.face.hidden=false;
        const [center,size]=app.bubble_count_badge(r.badge.firstChild.scrollWidth,Math.max(22,r.badge.firstChild.offsetHeight));
        r.face.hidden=hidden;
        place(r.badge,center.map((x,i)=>x+v.face[i]/2),size);
        r.boxes.push({node:r.badge,center,size,radius:size[1]/2,shape:0});
      }
    }
    const hadRings=r.ringNodes.size>0;
    if(v.rings.length) {
      r.arcs=[];
      const ids=new Set();
      v.rings.forEach((ring,i)=>{
        ids.add(ring.setting);let nodes=r.ringNodes.get(ring.setting);
        if(!nodes) {
          const g=svg('g'),band=svg('path',{'class':'bubble-band'}),track=svg('path',{'class':'bubble-track'}),fill=svg('path',{'class':'bubble-fill'}),thumb=svg('circle',{'class':'bubble-thumb'}),symbol=icon(ring.icon);
          symbol.setAttribute('width',16);symbol.setAttribute('height',16);g.append(band,track,fill,thumb,symbol);r.halo.append(g);nodes={g,band,track,fill,thumb,symbol};r.ringNodes.set(ring.setting,nodes);
        }
        const a=ring.arc,g=packet.rings[i],angle=Math.atan2(g.icon[1],g.icon[0]),sweep=a.sweep+Math.sign(a.sweep)*21/a.radius*180/Math.PI;
        nodes.g.style.opacity=ring.enabled?'1':'.4';nodes.g.setAttribute('aria-label',`${ring.label}: ${ring.text}`);
        nodes.band.setAttribute('d',path(g.icon,g.end,a.radius,sweep));nodes.band.setAttribute('stroke-width',a.band);
        nodes.track.setAttribute('d',path(g.start,g.end,a.radius,a.sweep));
        nodes.fill.setAttribute('d',path(g.start,g.thumb,a.radius,a.sweep*ring.fill));nodes.fill.style.display=ring.fill>0?'':'none';
        for(const[k,value]of Object.entries({cx:g.thumb[0],cy:g.thumb[1],r:ring.active?7.5:7}))nodes.thumb.setAttribute(k,value);
        nodes.symbol.setAttribute('x',g.icon[0]-8);nodes.symbol.setAttribute('y',g.icon[1]-8);
        r.arcs.push({radius:a.radius,band:a.band,start:angle,sweep:sweep*Math.PI/180,strength:ring.enabled?1:.4});
      });
      for(const [id,n]of r.ringNodes)if(!ids.has(id)){n.g.remove();r.ringNodes.delete(id);}
    }
    if(v.rings.length||hadRings)transition(r,'halo',r.halo,v.rings.length>0,v.rings.length?160:120,.85);
    r.readout.hidden=!v.readout;
    if(v.readout){r.readout.firstChild.textContent=v.readout;surface(r,r.readout,packet.readout,[r.readout.firstChild.scrollWidth+14,r.readout.firstChild.offsetHeight+4]);}
    const liveItems=new Set();let fresh=0;
    v.chooser.forEach((item,i)=>{
      liveItems.add(i);let slot=r.items.get(i);const key=json([item.kind,item.label,item.preview,item.icon,state().theme]);
      if(!slot||slot.key!==key) {
        retire(r,slot?.node);slot?.tag?.remove();const node=button('',()=>{const current=r.view.chooser[i];if(current){send({type:'choose',id:v.id,phase:'down',position:current.center});send({type:'choose',id:v.id,phase:'up',position:current.center});}},'bubble-choice bubble-surface');
        node.classList.toggle('bubble-brush',item.kind==='brush');node.setAttribute('aria-label',item.label);node.title=item.label;
        if(item.preview!=null){const img=element('img');img.src=asset(`brush-previews/${item.preview}-${state().theme}.webp`);img.alt='';img.draggable=false;node.append(img,element('span','',item.label));}else node.append(icon(item.icon));
        r.chooser.append(node);slot={node,key};r.items.set(i,slot);transition(r,node,node,true,180,.6,fresh++*(item.kind==='brush'?16:14));
      }
      slot.node.disabled=!item.enabled;slot.node.classList.toggle('bubble-disabled',!item.enabled);slot.node.classList.toggle('bubble-selected',item.selected);
      surface(r,slot.node,item.center,item.kind==='brush'?[152,48]:[44,44]);
      slot.shown=item.shown;
      if(item.kind!=='brush') {
        if(!slot.tag){slot.tag=element('div','bubble-tag bubble-surface');slot.tag.append(element('span'));r.chooser.append(slot.tag);}
        slot.tag.firstChild.textContent=item.label;slot.tag.hidden=false;
        slot.tag.style.width='max-content';
        const width=Math.min(96,Math.ceil(parseFloat(getComputedStyle(slot.tag.firstChild).width))+18),height=slot.tag.firstChild.offsetHeight+6;
        const b=app.bubble_chooser_tag(...item.center,width);surface(r,slot.tag,[b.x+b.width/2,b.y+b.height/2],[b.width,height],9);
      } else if(slot.tag)slot.tag.hidden=true;
    });
    for(const [i,slot]of r.items)if(!liveItems.has(i)){slot.tag?.remove();retire(r,slot.node);r.items.delete(i);}
    const liveBeads=new Set();
    const tweenBeads=previous.diameter===v.diameter&&!input.turning(v.id)&&!reduced.matches;
    v.beads.forEach((bead,i)=>{
      const p=bead.placement,id=json(p.bead);liveBeads.add(id);let slot=r.beadNodes.get(id);
      if(!slot){const node=button('',()=>send({type:'tap',id:v.id,position:slot.center}),packet.bead_colors[i]?'color-swatch':'bubble-surface');node.append(element('span'));r.beads.append(node);slot={node};r.beadNodes.set(id,slot);}
      if(json(slot.target)!==json(p)) {
        slot.tween=tweenBeads?{from:slot.shown??{...p,alpha:0},to:p,start:null}:null;
        slot.shown=slot.tween?.from??p;slot.target=p;
      }
      slot.gone=false;slot.center=p.center;const n=slot.node;n.title=bead.label;n.setAttribute('aria-label',bead.label);n.setAttribute('aria-pressed',bead.selected);n.dataset.colorSlot=p.bead.kind;
      n.style.zIndex=i;
      if(packet.bead_colors[i])n.firstElementChild.style.background=`linear-gradient(${rgba(packet.bead_colors[i])},${rgba(packet.bead_colors[i])}),repeating-conic-gradient(var(--checker-dark) 0 25%,var(--checker-light) 0 50%) 0 0 / 10px 10px`;
      else {glyph(n,'palette');surface(r,n,p.center,[p.size,p.size]);n.classList.toggle('bubble-selected',bead.selected);}
    });
    for(const[id,slot]of r.beadNodes)if(!liveBeads.has(id)){
      if(!tweenBeads){slot.node.remove();r.beadNodes.delete(id);continue;}
      if(!slot.gone){slot.gone=true;slot.tween={from:slot.shown,to:{...slot.shown,alpha:0},start:null};}
      if(slot.node.bubbleSurface)r.boxes.push(slot.node.bubbleSurface);
    }
    r.hints?.remove();r.hints=element('div','bubble-hints');r.body.append(r.hints);
    for(const[center,alpha]of v.hints){const hint=element('i');place(hint,center,[5,5]);hint.style.opacity=alpha*.35;r.hints.append(hint);}
    if(v.pill) {
      if(!r.pill){r.pill=element('div','bubble-pill bubble-surface');r.pill.append(button('',()=>send({type:'palette',id:v.id,step:-1})),button('',()=>{r.pill.menuModel=()=>app.context_menu({kind:'bubble_palettes',id:v.id});customization.openMenu(r.pill);}),button('',()=>send({type:'palette',id:v.id,step:1})));r.pill.firstChild.append(icon('go-previous'));r.pill.lastChild.append(icon('go-next'));r.body.append(r.pill);}
      r.pill.hidden=false;r.pill.children[1].textContent=v.pill;const b=packet.pill;surface(r,r.pill,[b.x+b.width/2,b.y+b.height/2],[b.width,b.height]);
    } else if(r.pill)r.pill.hidden=true;
    if(v.card) {
      if(!r.card) {
        const card=element('div','bubble-card bubble-surface dock-panel content-drawer');r.body.append(card);
        const inspector=createLayerInspector({app,state,workspace,element,button,icon,contentPanel,toggle:()=>send({type:'properties',id:v.id}),invalidate:()=>{active=true;wake();}});
        card.append(inspector.root);r.card={root:card,inspector};
      }
      r.card.inspector.root.refreshPanel();r.card.inspector.refresh(v.card.properties,v.card.full,v.card.full?undefined:v.card.disclosure);
      r.card.root.classList.toggle('bubble-simple',!v.card.full);
      surface(r,r.card.root,[0,0],v.card.size,18,1);transition(r,'card',r.card.root,true,200,.6);r.face.hidden=true;r.transitions.delete('face');delete r.face.bubbleTransition;
    } else if(r.card) {
      transition(r,'card',r.card.root,false,150,.6);
      if(r.card.root.bubbleSurface)r.boxes.push(r.card.root.bubbleSurface);
      r.face.hidden=r.transitions.get('card')?.value>0;
    }
    if(v.stamp)stampWanted={...v.stamp,color:packet.stamp_color,record:r};
  }
  async function refreshStamp() {
    if(stampBusy||!stampWanted)return;stampBusy=true;
    const generation=stampGeneration,owner=stampWanted.record;
    try {
      const next=app.bubble_stamp(stamp?.request_id);
      stampPending=!next||next.pending;
      if(next?.alpha.length&&next.request_id!==stamp?.request_id) {
        const image=await rasterWorker({operation:'alpha-image',metadata:{size:next.size},buffers:[next.alpha]});
        if(generation!==stampGeneration||stampWanted?.record!==owner)image.close();
        else {stamp?.image.close();stamp={...next,image};}
        active=true;wake();
      }
    } catch(error) {console.error(error);if(generation===stampGeneration){stampWanted=null;stampPending=false;}}
    finally {stampBusy=false;if(generation!==stampGeneration&&stampWanted){active=true;wake();}}
  }
  function refresh() {
    dirty=true;active=true;wake();
  }
  function fade(r,to) {
    if(r.fadeTarget===to)return;
    r.fadeTarget=to;r.fadeTween={from:r.fade,to,start:null};
  }
  function tick(now) {
    if(!active&&!dirty)return false;
    let hoverChanged=dirty;
    if(dirty) {
      dirty=false;const stampOwner=stampWanted?.record;stampWanted=null;const packets=app.bubbles(),live=new Set(packets.map(p=>p.view.id));
      for(const[id,r]of records)if(!live.has(id)){r.painter?.dispose();r.card?.inspector.root.disposePanel();thumbnails.forget(`bubble:${id}`);r.root.remove();records.delete(id);}
      for(const packet of packets){let r=records.get(packet.view.id);if(!r){r=record(packet);records.set(packet.view.id,r);}refreshRecord(r,packet);}
      if(stampOwner!==stampWanted?.record){stampGeneration++;stamp?.image.close();stamp=undefined;stampPending=false;}
      input.validate();
    }
    const motion=app.bubble_motion(now,reduced.matches);active=motion.moving;
    const [down,faded]=motion.fade;
    if(down){restoreAt=null;for(const r of records.values()){const fadedNow=faded.includes(r.view.id);hoverChanged||=r.faded!==fadedNow;r.faded=fadedNow;fade(r,r.faded?.2:1);}}
    else if(stroke)restoreAt=now+180;
    stroke=down;
    if(restoreAt!=null){active=true;if(now>=restoreAt){restoreAt=null;for(const r of records.values()){hoverChanged||=r.faded;r.faded=false;fade(r,1);}}}
    for(const p of motion.placements) {
      const r=records.get(p.id);if(!r)continue;
      hoverChanged||=r.center[0]!==p.center[0]||r.center[1]!==p.center[1]||r.scale!==(p.homing?.82:p.lifted?1.04:1)*r.pinch;
      r.center=p.center;r.scale=(p.homing?.82:p.lifted?1.04:1)*r.pinch;r.homeAlpha=p.homing?.55:1;
      r.root.style.transform=`translate(${p.center[0]}px,${p.center[1]}px)`;r.body.style.transform=`scale(${r.scale})`;
      r.root.classList.toggle('bubble-lifted',p.lifted);r.color?.shadow(p.lifted);
      for(const box of r.boxes)if(r.view.kind!=='layers'||box.node!==r.face)bubbleShadow(box.node,box.size,box.radius,p.lifted,false,enabledAlpha(box.node));
      if(r.fadeTween){const t=r.fadeTween;t.start??=now;const progress=reduced.matches?1:Math.min(1,(now-t.start)/140);r.fade=t.from+(t.to-t.from)*smooth(progress);if(progress===1)r.fadeTween=null;else active=true;}
      r.root.style.opacity=r.fade*r.homeAlpha;
      for(const[key,t]of r.transitions) {
        t.start??=now;const progress=t.ms?Math.max(0,Math.min(1,(now-t.start-t.delay)/t.ms)):1;
        const value=t.from+(t.to-t.from)*ease(progress);hoverChanged||=t.value!==value;t.value=value;t.node.style.opacity=t.value*enabledAlpha(t.node);t.node.style.scale=(t.fromScale+(1-t.fromScale)*t.value)*itemScale(t.node);
        if(progress<1)active=true;
        else if(!t.show){t.node.hidden=true;if(key==='card'&&r.face.hidden){r.face.hidden=false;transition(r,'face',r.face,true,180,.6);}if(key==='halo'){r.arcs=[];}}
      }
      r.retiring=r.retiring.filter(node=>{if(!node.hidden)return true;node.remove();r.transitions.delete(node);return false;});
      for(const[i,slot]of r.items)if(slot.tag)slot.tag.hidden=!(slot.shown||r.hotItem===i)||slot.node.bubbleTransition.value<1;
      for(const[id,slot]of r.beadNodes) {
        const t=slot.tween;
        if(t) {
          t.start??=now;const progress=Math.min(1,(now-t.start)/220),amount=ease(progress),mix=(a,b)=>a+(b-a)*amount;
          hoverChanged=true;slot.shown={center:t.from.center.map((v,i)=>mix(v,t.to.center[i])),size:mix(t.from.size,t.to.size),alpha:mix(t.from.alpha,t.to.alpha)};
          if(progress===1){slot.tween=null;if(slot.gone){slot.node.remove();r.beadNodes.delete(id);continue;}}else active=true;
        }
        const p=slot.shown;place(slot.node,p.center,[p.size,p.size]);slot.node.style.opacity=p.alpha;slot.node.bubbleAlpha=p.alpha;
        if(!slot.node.bubbleSurface)bubbleShadow(slot.node,[p.size,p.size],p.size/2,false,true);
        if(slot.node.bubbleSurface)Object.assign(slot.node.bubbleSurface,{center:p.center,size:[p.size,p.size],radius:p.size/2});
      }
      if(r.view.card) {
        const measurement=r.card.inspector.measure(now);r.card.inspector.layout();active||=measurement.running;hoverChanged||=measurement.running;
        if(json(r.view.card.metrics)!==json([measurement.measurement,measurement.properties_height]))send({type:'measure_card',id:p.id,measurement:measurement.measurement,properties_height:measurement.properties_height});
      }
    }
    input.refreshHover(hoverChanged);
    stampCanvas.hidden=!stampWanted||!stamp;
    if(stampWanted) {
      refreshStamp();if(stampPending)active=true;const s=stampWanted;if(!s)return active||dirty;const diameter=s.diameter*(stamp?.extent||1);
      place(stampCanvas,s.center.map((x,i)=>x+s.record.center[i]),[diameter,diameter]);
      if(stamp){const size=stamp.image.width;if(stampCanvas.width!==size)stampCanvas.width=stampCanvas.height=size;const ctx=stampCanvas.getContext('2d');ctx.clearRect(0,0,size,size);ctx.drawImage(stamp.image,0,0);ctx.globalCompositeOperation='source-in';ctx.fillStyle=rgba(s.color);ctx.fillRect(0,0,size,size);ctx.globalCompositeOperation='source-over';}
    }
    workspace.classList.toggle('bubble-dab-active',!!stampWanted);glassChanged();return active||dirty;
  }
  function glass() {
    const boxes=[],arcs=[],colors=[];
    for(const r of records.values()) {
      const strength=r.fade*(r.homeAlpha??1),scale=r.scale;
      if(r.packet.color){const g=r.packet.color.glass;colors.push([...r.center,g[0]*scale,g[1]*scale,g[2]*scale,g[3],strength]);}
      for(const b of r.boxes.concat(r.retiring.map(node=>node.bubbleSurface).filter(Boolean))) {
        if(b.node.closest('[hidden]'))continue;
        const t=b.node.bubbleTransition;
        const factor=(t?t.fromScale+(1-t.fromScale)*t.value:1)*itemScale(b.node),s=scale*factor,alpha=strength*(t?.value??1)*(b.node.bubbleAlpha??1)*enabledAlpha(b.node);
        if(alpha<=0)continue;
        boxes.push([r.center[0]+b.center[0]*scale-b.size[0]*s/2,r.center[1]+b.center[1]*scale-b.size[1]*s/2,...b.size.map(v=>v*s),...Array(4).fill(b.radius*s*(b.shape&&!cornerShape?.54:1)),cornerShape?b.shape:0,alpha]);
      }
      const t=r.transitions.get('halo'),s=scale*(t?t.fromScale+(1-t.fromScale)*t.value:1);
      for(const a of r.arcs)arcs.push([...r.center,a.radius*s,a.band*s,a.start,a.sweep,strength*a.strength*(t?.value??1)]);
    }
    return {boxes,arcs,colors};
  }
  reduced.addEventListener('change',()=>{active=true;wake();});
  setInterval(()=>{
    if(!records.size||state().settings_open)return;
    thumbnails.poll();
    for(const r of records.values())if(r.thumbnail&&!r.face.hidden&&r.view.layer!=null) {
      const layer=state().layers.find(layer=>String(layer.id)===String(r.view.layer));
      if(layer?.has_thumbnail)thumbnails.request(`bubble:${r.view.id}`,`${state().document_file.epoch}:${r.view.layer}:${layer.paint_revision}`,layer.id,r.thumbnail);
    }
  },120);
  return {refresh,tick,glass,handle:input.handle,busy:input.busy,takeTile(item,position) {
    const grab=app.bubble_grab({type:'grab',grab:item,held:true});
    if(!grab||!app.can_take_bubble(grab,...position,...viewport()))return false;
    send({type:'drag',grab,phase:'down',position,viewport:viewport()});
    send({type:'drag',grab,phase:'up',position,viewport:viewport()});return true;
  }};
}
