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
