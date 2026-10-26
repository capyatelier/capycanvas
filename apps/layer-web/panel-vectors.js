import {selectedTabSurface} from './workspace-chrome.js';
import {createPanelVectorQueue} from './panel-vector-queue.js';

const ns='http://www.w3.org/2000/svg';
let sequence=0;
const svg=(name,attributes={})=>{
  const node=document.createElementNS(ns,name);
  for(const [key,value]of Object.entries(attributes))node.setAttribute(key,value);
  return node;
};
const attr=(node,key,value)=>{if(node.getAttribute(key)!==String(value))node.setAttribute(key,value);};

export function createPanelVectors({workspace,groups,project,place,changed,failed}) {
  const columns=new Map(),marked=new Set(),sources=new Set();
  let triangles=new Float32Array();
  project(null).catch(failed);
  const schedule=createPanelVectorQueue(project,failed);

  function record(id) {
    let r=columns.get(id);if(r)return r;
    const key=`panel-vector-${++sequence}`,root=svg('svg',{'aria-hidden':'true',focusable:'false'});
    root.classList.add('column-surface');
    root.innerHTML=`<defs><path id="${key}-outline"/><clipPath id="${key}-clip"><use href="#${key}-outline"/></clipPath>
      <filter id="${key}-shadow" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="3"/><feOffset dy="2"/><feComposite in2="SourceAlpha" operator="out"/></filter>
      <filter id="${key}-tint" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feFlood class="tab-surface-strip" result="strip"/><feComposite in="strip" in2="SourceAlpha" operator="in" result="inside"/><feFlood class="tab-surface-panel" result="panel"/><feComposite in="panel" in2="SourceAlpha" operator="out" result="outside"/><feComposite in="inside" in2="outside" operator="arithmetic" k2="1" k3="1"/></filter>
      <mask id="${key}-holes" maskUnits="userSpaceOnUse"><rect fill="white"/><g fill="black"/></mask></defs>
      <use href="#${key}-outline" fill="black" opacity=".16" filter="url(#${key}-shadow)"/>
      <g clip-path="url(#${key}-clip)"><g filter="url(#${key}-tint)"><g mask="url(#${key}-holes)"><rect fill="transparent"/><g class="vector-headers" fill="white"/></g></g></g>`;
    workspace.append(root);
    r={root,key,outline:root.querySelector('defs > path'),defs:root.querySelector('defs'),
      headers:root.querySelector('.vector-headers'),mask:root.querySelector('mask'),receivers:new Map()};
    columns.set(id,r);return r;
  }
  function masks() {
    const updates=[];
    for(const r of columns.values())if(!r.root.hidden)for(const [id,receiver]of r.receivers) {
      const tab=selectedTabSurface(groups.get(id));
      updates.push({receiver,tab});
    }
    for(const {receiver,tab}of updates) {
      attr(receiver.hole,'d',tab?.d||'');
      attr(receiver.hole,'transform',`translate(${receiver.x+(tab?.x||0)} ${receiver.y+(tab?.y||0)})`);
      for(const [name,value]of Object.entries({x:receiver.x+(tab?.clipX||0),y:receiver.y+(tab?.clipY||0),width:tab?.width||0,height:tab?.height||0}))attr(receiver.holeClip,name,value);
    }
  }
  workspace.addEventListener('panel-tab-surface',masks);
  const tabs=new MutationObserver(masks);
  function apply(layout,result) {
    const live=new Set(),nextMarked=new Set(),nextSources=new Set();
    for(const column of result.columns) {
      live.add(column.id);const r=record(column.id),b=column.bounds;
      r.root.hidden=false;place(r.root,b);attr(r.root,'viewBox',`0 0 ${b.width} ${b.height}`);
      attr(r.outline,'d',column.d);
      for(const filter of r.root.querySelectorAll('filter'))for(const [name,value]of Object.entries(
        filter.id.endsWith('-shadow')?{x:-24,y:-24,width:b.width+48,height:b.height+48}:{x:0,y:0,width:b.width,height:b.height}))attr(filter,name,value);
      for(const node of [r.mask,r.mask.firstElementChild,r.root.querySelector('g[filter] > g > rect')])
        for(const [name,value]of Object.entries({x:0,y:0,width:b.width,height:b.height}))attr(node,name,value);
      const source=workspace.querySelector(`.collapsed-column[data-column="${column.id}"]`);
      if(source) {
        nextSources.add(source);source.classList.add('vector-source');
        let clip=r.defs.querySelector('.vector-source-clip');
        if(!clip){clip=svg('clipPath',{id:`${r.key}-source`});clip.classList.add('vector-source-clip');clip.append(svg('path'));r.defs.append(clip);}
        attr(clip.firstElementChild,'d',column.source.d);source.style.clipPath=`url(#${clip.id})`;
        source.panelVectorRadii=layout.collapsed.find(c=>c.id===column.id).corner_radii;
      }
      const receiverLive=new Set();
      for(const receiver of column.receivers) {
        const node=groups.get(receiver.group);if(!node)continue;
        receiverLive.add(receiver.group);nextMarked.add(node);node.classList.add('vector-panel');
        const group=layout.groups.find(g=>g.id===receiver.group);node.panelVectorRadii=group.corner_radii;
        const preview=node.querySelector('.panel-preview');
        let entry=r.receivers.get(receiver.group);
        if(!entry) {
          const id=`${r.key}-${receiver.group}`,body=svg('clipPath',{id:`${id}-body`}),header=svg('clipPath',{id:`${id}-header`}),holeClip=svg('clipPath',{id:`${id}-hole`});
          body.append(svg('path'));header.append(svg('rect'));holeClip.append(svg('rect'));
          const headerPath=svg('path',{'clip-path':`url(#${header.id})`}),hole=svg('path'),holeGroup=svg('g',{'clip-path':`url(#${holeClip.id})`});
          r.defs.append(body,header,holeClip);r.headers.append(headerPath);holeGroup.append(hole);r.mask.lastElementChild.append(holeGroup);
          entry={body,header,headerPath,hole,holeGroup,holeClipPath:holeClip,holeClip:holeClip.firstElementChild};r.receivers.set(receiver.group,entry);
        }
        const x=receiver.bounds.x-b.x,y=receiver.bounds.y-b.y;
        entry.x=x;entry.y=y;
        attr(entry.body.firstElementChild,'d',receiver.body);preview.style.clipPath=`url(#${entry.body.id})`;
        attr(entry.headerPath,'d',receiver.tabs?receiver.header:'');
        for(const [name,value]of Object.entries({x,y,width:receiver.bounds.width,height:layout.tab_bar_height}))attr(entry.header.firstElementChild,name,value);
      }
      for(const [id,entry]of r.receivers)if(!receiverLive.has(id)) {
        for(const node of [entry.body,entry.header,entry.headerPath,entry.holeGroup,entry.holeClipPath])node.remove();r.receivers.delete(id);
      }
    }
    for(const [id,r]of columns)if(!live.has(id)){r.root.remove();columns.delete(id);}
    for(const node of marked)if(!nextMarked.has(node)) {
      node.classList.remove('vector-panel');node.panelVectorRadii=null;node.querySelector('.panel-preview')?.style.removeProperty('clip-path');
    }
    for(const node of sources)if(!nextSources.has(node)){node.classList.remove('vector-source');node.panelVectorRadii=null;node.style.removeProperty('clip-path');}
    marked.clear();nextMarked.forEach(n=>marked.add(n));sources.clear();nextSources.forEach(n=>sources.add(n));
    tabs.disconnect();for(const node of marked) {const list=node.querySelector('.tab-list');if(list)tabs.observe(list,{attributes:true,subtree:true,attributeFilter:['aria-selected','class']});}
    triangles=result.triangles;
    masks();changed();
  }
  return {schedule,apply,glass:()=>workspace.classList.contains('zen-hidden')?new Float32Array():triangles};
}
