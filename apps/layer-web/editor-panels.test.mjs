import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import test from 'node:test';

const source=readFileSync(new URL('editor-panels.js',import.meta.url),'utf8');

test('Colour applies the shared fit and only queries changed allocations',()=>{
  const node=()=>({style:{}}),requests=[],frame={...node(),clientWidth:280,clientHeight:184},root=node(),stage=node(),wheel=node(),swap=node(),readout=node(),edit=node();
  const choices=['foreground','background','transparent'].map(slot=>({slot,node:node()})),shapes=[node(),node()];
  const fit={size:159,height:184,natural_height:300,wheel:[2,3,155,155],foreground:[4,5,32,32],background:[20,21,24,24],transparent:[40,50,24,24],shapes:[[8,9,20,20],[10,11,20,20]],swap:[12,13,20,20],readout:[14,15,40,20],edit:[16,17,20,20]};
  const context={frame,root,stage,wheel,swap,readout,edit,choices,shapes,view:{hdr:true},frameWidth:0,frameHeight:0,layoutWidth:0,layout:undefined,naturalAspect:'',naturalLayout:undefined,contentChanged(){},app:{color_ui(request){requests.push({...request});return request.type==='fit_layout'?{...fit}:{...fit,size:request.size,height:request.size===128?150:300};}}};
  const begin=source.indexOf('    const place=',source.indexOf('  function colorWheel(')),end=source.indexOf('      const width=layoutWidth;',begin);
  runInNewContext(source.slice(begin,end)+'\n}\nglobalThis.draw=draw;',context);
  context.draw();
  assert.equal(stage.style.width,'159px');assert.equal(stage.style.height,'184px');assert.equal(frame.style.aspectRatio,'280 / 300');
  assert.deepEqual(requests,[{type:'layout',size:128,hdr:true},{type:'layout',size:280,hdr:true},{type:'fit_layout',width:280,height:184,hdr:true}]);
  for(const [control,rect]of [[wheel,fit.wheel],...choices.map(({slot,node})=>[node,fit[slot]]),...shapes.map((node,index)=>[node,fit.shapes[index]]),[swap,fit.swap],[readout,fit.readout],[edit,fit.edit]])assert.deepEqual(control.style,Object.fromEntries(['left','top','width','height'].map((key,index)=>[key,`${rect[index]}px`])));
  context.draw();assert.equal(requests.length,3);
  frame.clientHeight=185;context.draw();
  assert.deepEqual(requests.at(-1),{type:'fit_layout',width:280,height:185,hdr:true});assert.equal(requests.length,4);
});

function harness() {
  const navigators=new Map(),pending=new Map(),sizes=[],reflows=[];
  let next=0,wakes=0;
  const context={navigators,positioning:0,devicePixelRatio:2,
    requestAnimationFrame:fn=>{pending.set(++next,fn);return next;},
    cancelAnimationFrame:id=>pending.delete(id),wake:()=>wakes++,
    ResizeObserver:class {constructor(callback){this.callback=callback;}},
    app:{
      navigator_size(id,width,height,scale){sizes.push([id,width,height,scale]);return [512,256];},
      navigator_geometry(width,height){return {image:{x:1,y:2,width:width-2,height:height-4}};},
      reflow_navigators(){reflows.push(sizes.map(size=>[...size]));return true;},
    },
  };
  runInNewContext(source.slice(source.indexOf('  function measurePositions('),source.indexOf('  window.addEventListener("resize",queuePositions);'))+
    '\nglobalThis.deliver=entries=>navigatorSizes.callback(entries);',context);
  function add(id) {
    const overview={getBoundingClientRect:()=>assert.fail('Position updates must use observer geometry')};
    const record={id,overview,surface:{style:{}},hole:{style:{}},size:''};
    navigators.set(overview,record);return record;
  }
  return {context,navigators,sizes,reflows,pending,add,wakes:()=>wakes,
    deliver:(...records)=>context.deliver(records.map(([record,width,height])=>({target:record.overview,contentRect:{width,height}}))),
    flush(){const callbacks=[...pending.values()];pending.clear();callbacks.forEach(fn=>fn());},
  };
}

test('Navigator observers publish all current extents before one GPU reflow',()=>{
  const h=harness(),a=h.add(1),b=h.add(2);
  h.context.queuePositions();h.context.queuePositions();
  assert.equal(h.pending.size,1);
  h.deliver([a,220,164],[b,140,100]);
  assert.equal(h.pending.size,0);
  assert.deepEqual(h.sizes,[[1,220,164,2],[2,140,100,2]]);
  assert.equal(h.reflows.length,1);assert.equal(h.reflows[0].length,2);
  assert.equal(a.surface.style.width,'256px');assert.equal(a.surface.style.height,'128px');
  assert.equal(a.hole.style.width,'218px');assert.equal(b.hole.style.height,'96px');
  assert.equal(h.wakes(),1);
});

test('Queued camera updates retain the latest observed size and follow density changes',()=>{
  const h=harness(),a=h.add(1);
  h.deliver([a,220,164]);
  h.context.queuePositions();h.deliver([a,220,90]);h.flush();
  h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes,[[1,220,164,2],[1,220,90,2]]);
  assert.equal(h.reflows.length,2);
  assert.equal(a.hole.style.height,'86px');
  h.context.devicePixelRatio=3;h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes.at(-1),[1,220,90,3]);
  assert.equal(h.reflows.length,3);
});

test('Unmeasured and retired Navigator controls cannot publish stale extents',()=>{
  const h=harness(),a=h.add(1),b=h.add(2);
  h.context.queuePositions();h.flush();
  assert.deepEqual(h.sizes,[]);
  h.navigators.delete(a.overview);
  h.deliver([a,220,164],[b,140,100]);
  assert.deepEqual(h.sizes,[[2,140,100,2]]);
  assert.equal(a.hole.style.width,undefined);
});
