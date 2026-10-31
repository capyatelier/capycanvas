import assert from 'node:assert/strict';
import {test} from 'node:test';
import {scopeGraph,createScope} from './histogram.js';
import {FakeElement} from './fake-dom.mjs';
import {createDropdown} from './dropdown.js';

function harness(t,waveform=false) {
  const previous={document:globalThis.document,ResizeObserver:globalThis.ResizeObserver,ImageData:globalThis.ImageData,devicePixelRatio:globalThis.devicePixelRatio};
  const canvases=[],observers=[];
  const canvas=()=>{let width=0,height=0;const node={dataset:{},attributes:{},clientWidth:200,clientHeight:160,operations:[],getBoundingClientRect(){return {width:this.clientWidth,height:this.clientHeight};},setAttribute(k,v){this.attributes[k]=v;},getContext(){return context;},get width(){return width;},set width(v){width=v;this.operations=[];},get height(){return height;},set height(v){height=v;}};const context={beginPath(){this.path=[];},rect(...args){this.path.push(args);},fill(){node.operations.push({type:'fill',color:this.fillStyle,path:this.path});},putImageData(image,...args){node.operations.push({type:'pixels',image,args});},drawImage(...args){node.operations.push({type:'image',args,smoothing:this.imageSmoothingEnabled});}};canvases.push(node);return node;};
  Object.assign(globalThis,{document:{createElement:canvas},devicePixelRatio:1,ImageData:class {constructor(data,width,height){Object.assign(this,{data,width,height});}},ResizeObserver:class {constructor(callback){this.callback=callback;this.disconnected=false;observers.push(this);}observe(node){this.node=node;}disconnect(){this.disconnected=true;}}});
  t.after(()=>Object.assign(globalThis,previous));
  const colors=[[255,0,0],[0,255,0],[0,0,255],[200,200,200]],view={plot:[[0,[0,.5,1]],[1,[1,.5,0]]],description:'Current input',range:'0–1',image:{width:2,height:1,bytes:Uint8Array.of(255,0,0,255,0,255,0,255)}},state={scope_colors:colors,histogram:view,waveform:view};
  const graph=scopeGraph({state:()=>state,element:canvas,kind:waveform?'waveform':'histogram',waveform});
  return {graph,state,view,canvases,observers,canvas};
}

test('shared histogram publication keeps the canvas and does not repaint retained plot/colors/extent',t=>{
  const h=harness(t);h.graph.refresh();const canvas=h.graph.node,first=canvas.operations;
  assert.equal(first.length,2);assert.equal(first[0].color,'rgba(255,0,0,.55)');assert.deepEqual(first[0].path[2],[200/3*2,0,200/3+.1,160]);
  h.graph.refresh();assert.equal(canvas.operations,first);
  h.view.description='Localized source';h.graph.refresh();assert.equal(canvas.attributes['aria-label'],'Localized source\n0–1');assert.equal(canvas.operations,first,'Caption changes do not redo plot work');
  h.view.plot=[[2,[1,0]]];h.graph.refresh();assert.equal(canvas.operations.length,1);assert.equal(canvas.operations[0].color,'rgba(0,0,255,.55)');
  h.state.scope_colors=h.state.scope_colors.map(c=>c.map(n=>Math.floor(n*.8)));h.graph.refresh();assert.equal(canvas.operations[0].color,'rgba(0,0,204,.55)');
  canvas.clientWidth=320;h.observers[0].callback();assert.equal(canvas.width,320);assert.equal(canvas.height,160);
  canvas.clientHeight=116;h.observers[0].callback();assert.equal(canvas.height,116);assert.equal(canvas.operations[0].path[0][3],116);assert.equal(h.graph.node,canvas,'Resizing retains the plot canvas');
  h.view.plot=null;h.graph.refresh();assert.equal(canvas.operations.length,0,'Retired publication clears the actual plot');
  h.graph.dispose();assert.equal(h.observers[0].disconnected,true);
});

test('Waveform draws the exact shared RGBA projection without host color conversion or smoothing',t=>{
  const h=harness(t,true);h.graph.refresh();const [canvas,image]=h.canvases;
  assert.equal(image.operations.length,1);assert.deepEqual(Array.from(image.operations[0].image.data),Array.from(h.view.image.bytes));assert.equal(image.operations[0].image.width,2);assert.equal(image.operations[0].image.height,1);
  assert.deepEqual(canvas.operations[0].args,[image,0,0,200,160]);assert.equal(canvas.operations[0].smoothing,false);
  const first=canvas.operations;h.graph.refresh();assert.equal(canvas.operations,first);
  canvas.clientHeight=136;h.observers[0].callback();assert.equal(canvas.height,136);assert.deepEqual(canvas.operations[0].args,[image,0,0,200,136]);assert.equal(canvas.operations[0].smoothing,false);assert.equal(h.graph.node,canvas);
  h.view.plot=null;h.view.image=null;h.graph.refresh();assert.equal(canvas.operations.length,0);
  h.graph.dispose();assert.equal(h.observers[0].disconnected,true);
});

for(const waveform of [false,true]) test(`${waveform?'Waveform':'Histogram'} late copy refresh invalidates intrinsic content without invalidating plot frames`,t=>{
  const h=harness(t,waveform),kind=waveform?'waveform':'histogram';
  Object.assign(h.view,{sources:[],source:0,channels:[],channel:0,axis:['',''],labels:['','',''],status:'',logarithmic:false});
  const element=(tag,classes='',text='')=>{
    if(tag==='canvas')return h.canvas();
    const node=new FakeElement();Object.assign(node,{tagName:tag,children:[],dataset:{},attributes:new Map(),listeners:{},textContent:text,className:classes,classList:{toggle(){}}});return node;
  };
  const button=(label,activate,classes)=>{const node=element('button',classes,label);node.addEventListener('click',activate);return node;};
  const dropdown=(label,change)=>createDropdown({element,button,icon:()=>element('svg'),openMenu:()=>assert.fail('Refreshing copy must not open menus')},label,change);
  let scope,cached,invalidations=0;
  const snapshot=()=>{
    const [toolbar,graph,log,footer]=scope.node.children;
    return {sources:toolbar.children[0].children.map(node=>node.textContent),channels:toolbar.children[1].children.map(node=>node.textContent),axis:graph.children[1].children.map(node=>node.textContent),status:footer.children[0].textContent,log:log.children[1].textContent};
  };
  scope=createScope({state:()=>h.state,app:{catalog:()=>({native_copy:{sampler:{source:'Source'},color:{channel:'Channel'}}})},element,dropdown,button:()=>element('button'),icon:()=>element('svg'),dispatch:()=>assert.fail('Refreshing copy must not dispatch'),waveform,contentChanged:()=>{invalidations++;cached=snapshot();}});
  scope.refresh();cached=snapshot();const initial=invalidations;
  Object.assign(h.view,{sources:['Visible','Selected layer'],channels:['RGB','Luminance'],axis:['0','255'],labels:['Log counts','Shadows','Highlights'],status:'Exact'});
  scope.refresh();assert.equal(invalidations,initial+1,'Late intrinsic copy replaces the retained measurement clone');assert.deepEqual(cached,snapshot());assert.equal(cached.log,'Log counts');assert.equal(cached.status,'Exact');
  const measured=invalidations,canvas=scope.node.children[1].children[0];
  h.view.plot=[[0,[1,0]]];scope.refresh();assert.equal(invalidations,measured,'Histogram bins do not rebuild intrinsic content');
  h.state.scope_colors=h.state.scope_colors.map(color=>color.map(value=>value/2));scope.refresh();assert.equal(invalidations,measured,'Graph colors do not rebuild intrinsic content');
  h.view.logarithmic=true;h.state.histogram.shadows=true;h.view.source=1;h.view.channel=1;scope.refresh();assert.equal(invalidations,measured,'Control values retain their measurement clone');
  h.view.labels[0]='Localized count scale';scope.refresh();assert.equal(invalidations,measured+1);assert.equal(cached.log,'Localized count scale');assert.equal(scope.node.children[1].children[0],canvas,'The native canvas survives copy refresh');
  scope.dispose();h.graph.dispose();
});
