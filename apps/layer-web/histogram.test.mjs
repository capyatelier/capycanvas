import assert from 'node:assert/strict';
import {test} from 'node:test';
import {scopeGraph} from './histogram.js';

function harness(t,waveform=false) {
  const previous={document:globalThis.document,ResizeObserver:globalThis.ResizeObserver,ImageData:globalThis.ImageData,devicePixelRatio:globalThis.devicePixelRatio};
  const canvases=[],observers=[];
  const canvas=()=>{let width=0,height=0;const node={dataset:{},attributes:{},clientWidth:200,operations:[],setAttribute(k,v){this.attributes[k]=v;},getContext(){return context;},get width(){return width;},set width(v){width=v;this.operations=[];},get height(){return height;},set height(v){height=v;}};const context={beginPath(){this.path=[];},rect(...args){this.path.push(args);},fill(){node.operations.push({type:'fill',color:this.fillStyle,path:this.path});},putImageData(image,...args){node.operations.push({type:'pixels',image,args});},drawImage(...args){node.operations.push({type:'image',args,smoothing:this.imageSmoothingEnabled});}};canvases.push(node);return node;};
  Object.assign(globalThis,{document:{createElement:canvas},devicePixelRatio:1,ImageData:class {constructor(data,width,height){Object.assign(this,{data,width,height});}},ResizeObserver:class {constructor(callback){this.callback=callback;this.disconnected=false;observers.push(this);}observe(node){this.node=node;}disconnect(){this.disconnected=true;}}});
  t.after(()=>Object.assign(globalThis,previous));
  const colors=[[255,0,0],[0,255,0],[0,0,255],[200,200,200]],view={plot:[[0,[0,.5,1]],[1,[1,.5,0]]],description:'Current input',range:'0–1',image:{width:2,height:1,bytes:Uint8Array.of(255,0,0,255,0,255,0,255)}},state={scope_colors:colors,histogram:view,waveform:view};
  const graph=scopeGraph({state:()=>state,element:canvas,kind:waveform?'waveform':'histogram',waveform});
  return {graph,state,view,canvases,observers};
}

test('shared histogram publication keeps the canvas and does not repaint retained plot/colors/extent',t=>{
  const h=harness(t);h.graph.refresh();const canvas=h.graph.node,first=canvas.operations;
  assert.equal(first.length,2);assert.equal(first[0].color,'rgba(255,0,0,.55)');assert.deepEqual(first[0].path[2],[200/3*2,0,200/3+.1,160]);
  h.graph.refresh();assert.equal(canvas.operations,first);
  h.view.description='Localized source';h.graph.refresh();assert.equal(canvas.attributes['aria-label'],'Localized source\n0–1');assert.equal(canvas.operations,first,'Caption changes do not redo plot work');
  h.view.plot=[[2,[1,0]]];h.graph.refresh();assert.equal(canvas.operations.length,1);assert.equal(canvas.operations[0].color,'rgba(0,0,255,.55)');
  h.state.scope_colors=h.state.scope_colors.map(c=>c.map(n=>Math.floor(n*.8)));h.graph.refresh();assert.equal(canvas.operations[0].color,'rgba(0,0,204,.55)');
  canvas.clientWidth=320;h.observers[0].callback();assert.equal(canvas.width,320);assert.equal(canvas.height,160);
  h.view.plot=null;h.graph.refresh();assert.equal(canvas.operations.length,0,'Retired publication clears the actual plot');
  h.graph.dispose();assert.equal(h.observers[0].disconnected,true);
});

test('Waveform draws the exact shared RGBA projection without host color conversion or smoothing',t=>{
  const h=harness(t,true);h.graph.refresh();const [canvas,image]=h.canvases;
  assert.equal(image.operations.length,1);assert.deepEqual(Array.from(image.operations[0].image.data),Array.from(h.view.image.bytes));assert.equal(image.operations[0].image.width,2);assert.equal(image.operations[0].image.height,1);
  assert.deepEqual(canvas.operations[0].args,[image,0,0,200,160]);assert.equal(canvas.operations[0].smoothing,false);
  const first=canvas.operations;h.graph.refresh();assert.equal(canvas.operations,first);
  h.view.plot=null;h.view.image=null;h.graph.refresh();assert.equal(canvas.operations.length,0);
  h.graph.dispose();assert.equal(h.observers[0].disconnected,true);
});
