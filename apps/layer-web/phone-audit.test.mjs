import assert from 'node:assert/strict';
import test from 'node:test';
import {measurePhoneAuditElements,phoneAuditMatrix,phoneAuditFrameSpecifications,phoneAuditDrawerSpecifications} from './phone-audit.mjs';

test('phone frame IDs preserve authored tiles and expose actual slider parts',()=>{
  const specs=phoneAuditFrameSpecifications([{id:88,control:{kind:'color'}},{id:91,control:{kind:'divider'}},{id:204,control:{kind:'command',command:'eraser'}}]);
  assert.deepEqual(specs.filter(s=>s.id.startsWith('tile.')).map(s=>[s.id,s.selector]),[
    ['tile.settings','#phone-tool-settings'],['tile.88','#phone-tool-88'],['tile.204','#phone-tool-204']]);
  assert.equal(new Set(specs.map(s=>s.id)).size,specs.length);
  assert.equal(specs.find(s=>s.id==='capy').selector,'#phone-frame #zen-capy');
  for(const name of ['size','strength']){
    const track=specs.find(s=>s.id===`slider.${name}.track`),handle=specs.find(s=>s.id===`slider.${name}.handle`);
    assert.equal(track.role,'trough');assert.equal(handle.role,'slider');
    assert.ok(track.selector.includes('.interval-trough'));assert.ok(handle.selector.includes('.toolbar-slider-thumb'));
  }
  assert.deepEqual(phoneAuditFrameSpecifications([]).filter(s=>/^tile\.\d+$/.test(s.id)),[]);
});

test('phone audit covers both themes at DPR1 and actual 16pt enlarged text',()=>{
  assert.equal(phoneAuditMatrix.length,10);
  assert.deepEqual([...new Set(phoneAuditMatrix.map(c=>c.viewport.join('x')))],['412x891','891x412','320x568','320x320','280x560']);
  for(const entry of phoneAuditMatrix){
    assert.equal(entry.dpr,1);
    assert.equal(entry.text_scale,entry.text_pt/11);
    assert.equal(entry.text_pt,entry.viewport.join('x')==='320x320'?16:11);
  }
});

test('phone audit measures DOM allocations and keeps missing separate from hidden',()=>{
  const original=globalThis.getComputedStyle;
  globalThis.getComputedStyle=node=>node.style;
  try{
    const node=(bounds,visible=true)=>({id:'actual',tagName:'DIV',style:{visibility:'visible',display:'block'},
      getClientRects:()=>visible?[bounds]:[],getBoundingClientRect:()=>bounds});
    const visible=node({x:-2.25,y:8.5,width:48.75,height:47.25}),hidden=node({x:999,y:999,width:44,height:44},false);
    const root={querySelectorAll:selector=>({'.visible':[visible],'.hidden':[hidden]}[selector]||[])};
    const spec=(id,selector,optional=false)=>({id,role:'tile',selector,optional});
    const result=measurePhoneAuditElements([spec('tile.1','.visible'),spec('tile.2','.hidden'),spec('tile.3','.absent'),spec('native.close','.absent',true)],root);
    assert.deepEqual(result.elements.map(e=>[e.id,e.bounds,e.visible]),[['tile.1',[-2.25,8.5,48.75,47.25],true],['tile.2',null,false]]);
    assert.deepEqual(result.missing,['tile.3']);
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.visible'),spec('tile.1','.hidden')],root),/Duplicate/);
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.many')],{querySelectorAll:()=>[visible,hidden]}),/Ambiguous/);
    visible.getBoundingClientRect=()=>({x:NaN,y:0,width:48,height:48});
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.visible')],root),/Nonfinite/);
  } finally {globalThis.getComputedStyle=original;}
});

test('drawer allocations distinguish the Color stage from its painted disc',t=>{
  const css=globalThis.CSS;globalThis.CSS={escape:value=>value};t.after(()=>globalThis.CSS=css);
  const nodes=Object.fromEntries(['drawer','column','stage','color','disc'].map(id=>[id,{id}]));
  const drawer=nodes.drawer;drawer.dataset={drawer:'tool'};
  drawer.querySelectorAll=selector=>selector===':scope > .drawer-contents > .drawer-column'?[nodes.column]:[];
  drawer.querySelector=selector=>({'.color-wheel-stage':nodes.stage,'.color-wheel-control':nodes.color,'.color-wheel-control canvas':nodes.disc}[selector]??null);
  const root={querySelectorAll:selector=>selector==='.content-drawer'?[drawer]:selector.startsWith('#')?[nodes[selector.slice(1)]]:[]};
  const specs=phoneAuditDrawerSpecifications(root);
  assert.equal(new Set(specs.map(spec=>spec.id)).size,specs.length);
  assert.deepEqual(specs.filter(spec=>spec.id==='drawer.color.wheel'||spec.id==='drawer.color.disc').map(spec=>[spec.id,spec.role,spec.selector]),[
    ['drawer.color.wheel','color-wheel','#stage'],['drawer.color.disc','context','#disc'],
  ]);
});
