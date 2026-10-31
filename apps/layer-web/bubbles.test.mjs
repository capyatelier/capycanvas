import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import test from 'node:test';

const source=readFileSync(new URL('bubbles.js',import.meta.url),'utf8');
const layers=readFileSync(new URL('layers.js',import.meta.url),'utf8');
const inputSource=readFileSync(new URL('bubble-input.js',import.meta.url),'utf8');

for(const device of ['mouse','touch','pen'])for(const row of [false,true])test(`${device} drags the card from unused list space while rows retain input: row=${row}`,()=>{
  const actions=[],handlers={},record={view:{id:1,kind:'layers'},center:[100,100],packet:{reach:200}},workspace={setPointerCapture(){},addEventListener(type,handler){handlers[type]=handler;}};
  const node={closest(selector){return selector==='.bubble-card .layer-list'||row&&selector.includes('.layer-row')?this:null;}};
  const context={window:{addEventListener(){}},setTimeout:()=>1,clearTimeout(){}};
  runInNewContext(inputSource.replace('export function','function')+';this.create=createBubbleInput;',context);
  const input=context.create({app:{bubble_hit:()=> 'card'},workspace,records:new Map([[1,record]]),send:action=>actions.push(action),wake(){},menu(){},dismiss(){},viewport:()=>[800,600],colorTouch:{route(){}}});
  let prevented=false;
  const event=(type,x)=>({type,target:node,button:0,buttons:type==='pointerup'?0:1,pointerType:device,pointerId:1,clientX:x,clientY:150,timeStamp:0,preventDefault(){prevented=true;},stopImmediatePropagation(){}});
  input.handle(event('pointerdown',100));assert.equal(prevented,!row,'card backgrounds suppress native text drags');
  if(device==='touch'){prevented=false;handlers.touchstart(event('touchstart',100));assert.equal(prevented,!row,'only card backgrounds suppress native touch scrolling');}
  input.handle(event('pointermove',140));input.handle(event('pointerup',140));
  assert.deepEqual(actions.map(a=>a.phase),row?[]:['down','move','up']);
  assert.ok(actions.every(a=>a.type==='drag'&&a.grab.kind==='bubble'&&a.grab.id===1));
});

function thumbnails() {
  const requests=[],images=[];
  const context={Uint8ClampedArray,Map,ImageData:class {constructor(bytes,width,height){Object.assign(this,{bytes,width,height});}},
    app:{request_layer_thumbnail:(token,id)=>{requests.push({token,id});return true;},take_layer_thumbnail:()=>images.shift(),canvas_work_pending:()=>false,shader_work_pending:()=>false}};
  runInNewContext('let thumbnailRequest=0n;const thumbnailPending=new Map();'+layers.slice(layers.indexOf('export function createLayerThumbnails')).replace('export function','function')+';this.preview=createLayerThumbnails(app,()=>{});',context);
  const canvas=()=>({width:64,height:64,dataset:{},writes:[],getContext(){return{putImageData:(image,x,y)=>this.writes.push({image,x,y})};}});
  const complete=(token,shade)=>images.push([token,32,32,new Uint8Array(32*32*4).fill(shade)]);
  return{...context,requests,canvas,complete};
}

test('layer thumbnail backing uses the delivered image extent',()=>{
  const h=thumbnails(),canvas=h.canvas();
  assert.equal(h.preview.request('layer','revision',9,canvas),true);
  h.complete(h.requests[0].token,255);h.preview.poll();
  assert.deepEqual([canvas.width,canvas.height],[32,32],'a 64px face canvas uses its 32px image backing across its entire CSS size');
  assert.equal(canvas.dataset.previewRevision,'revision');
  assert.equal(canvas.writes.length,1);
  assert.deepEqual([canvas.writes[0].image.width,canvas.writes[0].image.height],[32,32]);
  assert.deepEqual([canvas.writes[0].x,canvas.writes[0].y],[0,0]);
});

test('replacing a layer canvas requests unchanged pixels and rejects its retired reply',()=>{
  const h=thumbnails(),retired=h.canvas(),current=h.canvas();
  h.preview.request('layer','revision',9,retired);
  assert.equal(h.preview.request('layer','revision',9,current),true,'the same revision must fill a replacement canvas after a theme change');
  assert.equal(h.requests.length,2);
  h.complete(h.requests[0].token,80);h.preview.poll();
  assert.equal(retired.writes.length,0);
  assert.equal(current.writes.length,0,'the retired request cannot publish into the replacement canvas');
  h.complete(h.requests[1].token,220);h.preview.poll();
  assert.equal(current.writes.length,1);
  assert.equal(current.writes[0].image.bytes[0],220);
  assert.equal(current.dataset.previewRevision,'revision');
  assert.equal(h.preview.request('layer','revision',9,current),false,'an already filled retained canvas reuses its pixels');
});
function preview() {
  const jobs=[],closed=[],wakes=[];
  let request=0;
  const context={stampBusy:false,stampPending:false,stampWanted:{record:{}},stampGeneration:0,stamp:undefined,active:false,
    app:{bubble_stamp:()=>({request_id:++request,pending:false,size:2,alpha:new Uint8Array([0,255,255,0])})},
    rasterWorker:()=>new Promise(resolve=>jobs.push(resolve)),wake:()=>wakes.push(true),console:{error:assert.fail}};
  runInNewContext(source.slice(source.indexOf('  async function refreshStamp()'),source.indexOf('  function refresh()'))+'this.poll=refreshStamp;',context);
  const image=id=>({width:2,height:2,close:()=>closed.push(id)});
  return {context,jobs,closed,wakes,image};
}

for(const nextContact of [false,true])test(`late dab bitmap retires after contact ${nextContact?'replacement':'release'}`,async()=>{
  const h=preview(),first=h.context.poll();
  assert.equal(h.jobs.length,1);
  h.context.stampGeneration++;
  h.context.stampWanted=nextContact?{record:{}}:null;
  await h.context.poll();
  assert.equal(h.jobs.length,1,'a second contact waits for the existing worker reply');
  h.jobs.shift()(h.image('retired'));await first;
  assert.deepEqual(h.closed,['retired']);
  assert.equal(h.context.stamp,undefined,'retired pixels never become the visible preview');
  assert.equal(h.context.stampBusy,false);
  if(nextContact) {
    assert.ok(h.wakes.length,'the current contact resumes after the older reply');
    const current=h.context.poll(),image=h.image('current');
    h.jobs.shift()(image);await current;
    assert.equal(h.context.stamp.image,image);
    assert.deepEqual(h.closed,['retired']);
  }
});

test('a ready dab replaces and releases the previous bitmap',async()=>{
  const h=preview();h.context.stamp={request_id:0,image:h.image('old')};
  const ready=h.context.poll(),image=h.image('new');h.jobs.shift()(image);await ready;
  assert.equal(h.context.stamp.image,image);
  assert.equal(h.context.stampPending,false);
  assert.deepEqual(h.closed,['old']);
});

function hoverFixture() {
  const counts={reads:0,hits:0,marks:0,dispatches:0,wakes:0,variables:0},classes=new Set(),marks=[],listeners={};
  const style={cursor:'',setProperty(){counts.variables++;}};
  const workspace={style,classList:{add:key=>classes.add(key),remove:key=>classes.delete(key),toggle(key,on){on?classes.add(key):classes.delete(key);}},
    setPointerCapture(){},addEventListener(type,handler){listeners[type]=handler;}};
  const target={closest(){return null;}},text={isConnected:true,style:{cursor:'text'},closest(){return this;}},dom={target};
  const record={view:{id:1,kind:'layers'},center:[100,100],packet:{reach:200},mark:part=>{counts.marks++;counts.wakes++;marks.push(part);}};
  const records=new Map([[1,record]]),context={window:{addEventListener(type,handler){listeners[type]=handler;}},
    document:{elementFromPoint(){counts.reads++;return dom.target;}},setTimeout:()=>1,clearTimeout(){}};
  runInNewContext(inputSource.replace('export function','function')+';this.create=createBubbleInput;',context);
  const input=context.create({app:{bubble_hit:(_id,x,y)=>{counts.hits++;return Math.hypot(x,y)<40?'face':null;}},workspace,records,
    send(){counts.dispatches++;},wake(){counts.wakes++;},menu(){},dismiss(){},viewport:()=>[800,600],leaveCanvas(){},colorTouch:{route(){}}});
  const event=(type,pointerType='pen',node=target)=>({type,pointerId:1,pointerType,target:node,relatedTarget:null,
    clientX:100,clientY:100,buttons:0,preventDefault(){},stopImmediatePropagation(){}});
  const resetCounts=()=>{for(const key of Object.keys(counts))counts[key]=0;};
  return {input,event,classes,style,marks,listeners,record,records,counts,resetCounts,target,text,dom};
}
for(const device of ['mouse','pen'])for(const exit of ['pointerleave','pointerout','pointercancel','blur'])test(`${device} bubble hover clears on ${exit}`,()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove',device));
  assert.equal(h.classes.has('bubble-pointer'),true);
  assert.equal(h.marks.at(-1),'face');
  if(exit==='blur')h.listeners.blur();else if(exit==='pointerleave'||exit==='pointerout')h.listeners[exit]?.(h.event(exit,device));else h.input.handle(h.event(exit,device));
  assert.equal(h.classes.has('bubble-pointer'),false,'exit clears the bubble cursor owner');
  assert.equal(h.style.cursor,'','exit releases the inline workspace cursor');
  assert.equal(h.marks.at(-1),null,'exit clears the visual hover');
});
for(const change of ['removed','moved'])test(`stationary pointer hover follows ${change} bubble geometry`,()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove'));
  if(change==='removed')h.records.delete(1);else h.record.center=[300,300];
  h.input.validate();h.input.refreshHover?.(true);
  assert.equal(h.classes.has('bubble-pointer'),false,'current geometry retires the hovered owner');
  assert.equal(h.style.cursor,'');
  assert.equal(h.marks.at(-1),null,'even a removed hovered record retires its mark');
});
for(const device of ['mouse','pen'])test(`${device} native text keeps its I-beam after geometry publication`,()=>{
  const h=hoverFixture();h.dom.target=h.text;h.input.handle(h.event('pointermove',device,h.text));h.resetCounts();
  h.input.refreshHover?.(true);
  assert.equal(h.style.cursor,'','native text owns its cursor');
  assert.equal(h.classes.has('bubble-pointer'),false);
  assert.equal(h.text.style.cursor,'text');
  assert.equal(h.counts.reads,1,'a changed native child uses one current DOM hit');
});
for(const change of ['moved','removed'])for(const device of ['mouse','pen'])test(`${device} stationary pointer forgets ${change} native text target`,()=>{
  const h=hoverFixture();h.dom.target=h.text;h.input.handle(h.event('pointermove',device,h.text));
  if(change==='removed')h.text.isConnected=false;
  h.dom.target=h.target;h.resetCounts();h.input.refreshHover?.(true);
  assert.equal(h.classes.has('bubble-pointer'),true,'current non-native bubble surface takes ownership');
  assert.equal(h.style.cursor,device==='pen'?'none':'default');
  assert.equal(h.counts.reads,1);
});
test('idle and unchanged validation adds no layout reads, dispatches, wakes or marks',()=>{
  const h=hoverFixture();
  for(let i=0;i<20;i++){h.input.validate();h.input.refreshHover?.(true);}
  assert.deepEqual(h.counts,{reads:0,hits:0,marks:0,dispatches:0,wakes:0,variables:0});
  h.input.handle(h.event('pointermove'));h.resetCounts();
  for(let i=0;i<20;i++){h.input.validate();h.input.refreshHover?.(false);}
  assert.deepEqual(h.counts,{reads:0,hits:0,marks:0,dispatches:0,wakes:0,variables:0});
});
test('hover does not write the unused bubble pointer CSS variable',()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove'));h.listeners.pointerleave?.(h.event('pointerleave'));
  assert.equal(h.counts.variables,0);
});
test('touch motion never acquires hover ownership',()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove','touch'));
  assert.equal(h.classes.has('bubble-pointer'),false);
  assert.equal(h.style.cursor,'');
  assert.equal(h.marks.length,0);
});
test('only the previous and current hovered records are marked',()=>{
  const h=hoverFixture(),otherMarks=[];
  h.records.set(2,{view:{id:2},center:[400,400],mark:part=>otherMarks.push(part)});
  h.input.handle(h.event('pointermove'));
  assert.equal(otherMarks.length,0,'unrelated records are untouched');
  h.listeners.pointerleave?.(h.event('pointerleave'));
  assert.deepEqual(h.marks,['face',null]);
  assert.equal(otherMarks.length,0);
});

for(const device of ['mouse','pen'])test(`${device} native text overlay moving under a stationary pointer takes cursor ownership`,()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove',device));
  assert.equal(h.classes.has('bubble-pointer'),true);
  h.dom.target=h.text;h.resetCounts();h.input.refreshHover?.(true);
  assert.equal(h.style.cursor,'','the current text target supplies its I-beam');
  assert.equal(h.classes.has('bubble-pointer'),false);
  assert.equal(h.text.style.cursor,'text');
  assert.equal(h.counts.reads,1);
  assert.equal(h.counts.dispatches,0);
});
test('workspace pointerout between children preserves current hover',()=>{
  const h=hoverFixture();h.input.handle(h.event('pointermove'));
  const event=h.event('pointerout');event.relatedTarget=h.text;h.resetCounts();
  h.listeners.pointerout?.(event);
  assert.equal(h.classes.has('bubble-pointer'),true);
  assert.deepEqual(h.counts,{reads:0,hits:0,marks:0,dispatches:0,wakes:0,variables:0});
});
