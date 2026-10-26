import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import test from 'node:test';

const source=readFileSync(new URL('editor-panels.js',import.meta.url),'utf8');

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
