import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export function panelBridgeFixture(saved,edge='left') {
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon'});
  const split=(id,first,second)=>({kind:'split',id,axis:'vertical',fraction:.5,first,second});
  const fixture=structuredClone(saved);
  Object.assign(fixture.layout,{bands:[{id:40,edge,extent:42,root:split(51,tabs(42,['color']),split(41,split(45,tabs(43,['brushes','sizes']),tabs(44,['navigator'])),tabs(50,['tool_settings'])))},{id:60,edge:edge==='left'?'right':'left',extent:252,root:tabs(61,['layers','properties'])}],floating:[],collapsed:[51,42,45,50].map(root=>({root,expanded_width:246})),column_stacks:[{column:51,members:[42,45,50],drawers:false,auto_hide:false}],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],next_id:Math.max(62,fixture.layout.next_id)});fixture.zen_mode=false;
  return fixture;
}

export async function checkPanelBridges({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace'),transparency=await evaluate('layerApp.state().settings.transparency');
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/panel-bridges/web';await mkdir(dir,{recursive:true});
  const records=[],wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(180);};
  const layout=()=>evaluate("layerApp.app.layout(document.querySelector('#workspace').clientWidth,document.querySelector('#workspace').clientHeight)");
  const mouse=(type,p)=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  let held=false,enteredFullscreen=false;
  try {
    if(!await evaluate('!!document.fullscreenElement')){await call('Runtime.evaluate',{expression:'document.documentElement.requestFullscreen().then(()=>true)',awaitPromise:true,userGesture:true});enteredFullscreen=true;await wait(500);}
    for(const theme of ['light','dark'])for(const edge of ['left','right'])for(const alpha of [0,1]) {
      const fixture=panelBridgeFixture(saved,edge);
      await send({type:'set_theme',theme});await send({type:'preferences',action:{type:'edit',id:'transparency',value:alpha}});await send({type:'restore_workspace',workspace:fixture});
      await evaluate('document.querySelector(".column-tab[data-panel=brushes]").click()');await settle();await wait(250);
      let l=await layout();const col=l.collapsed.find(c=>c.id===45),anchor=col.groups.flatMap(g=>g.icons).find(i=>i.panel==='navigator').bounds;
      assert.deepEqual(await evaluate(`document.querySelector('.collapsed-column[data-column="45"]').panelVectorRadii`),col.corner_radii,'Source clipping retains its authored corners');
      const pick=await evaluate(`(()=>{const w=document.querySelector('#workspace'),n=[...document.querySelectorAll('.divider')].find(n=>n.dragAction?.type==='drag_divider'&&n.dragAction.id===45),b=n.getBoundingClientRect(),p={x:b.x+b.width/2,y:b.y+b.height/2},h=document.elementFromPoint(p.x,p.y);return{bounds:b.toJSON(),point:p,hit:h?.closest('[data-workspace-drag]')?.dataset.workspaceDrag,workspace:[w.clientWidth,w.clientHeight],inner:[innerWidth,innerHeight],origin:w.getBoundingClientRect().toJSON(),visible:document.visibilityState,focus:document.hasFocus()};})()`);
      assert.equal(JSON.parse(pick.hit||'{}').id,45,'Native divider is the actual pickup target');
      const divider=l.dividers.find(d=>d.id===45).bounds;
      for(const key of ['x','y','width','height'])assert.ok(Math.abs(divider[key]-pick.bounds[key])<1,'Native divider matches the current shared layout before pickup');
      const start=pick.point;
      await mouse('mousePressed',start);held=true;
      const positions=[anchor.y-6,anchor.y-3,anchor.y,anchor.y+anchor.height-6,anchor.y+anchor.height-3,anchor.y+anchor.height,anchor.y+anchor.height+.25,anchor.y+anchor.height+1,anchor.y+anchor.height+6,anchor.y+anchor.height+18];
      for(const [key,gap]of positions.entries()) {
        await mouse('mouseMoved',{x:start.x,y:gap+3});await settle();await wait(250);l=await layout();
        const receiverBottom=l.groups.find(g=>g.id===43).bounds.y+l.groups.find(g=>g.id===43).bounds.height;
        assert.ok(Math.abs(receiverBottom-gap)*await evaluate('devicePixelRatio')<=1.01,`Actual receiving edge ${receiverBottom} reaches requested gap ${gap}`);
        const name=`bridge-${theme}-${edge}-${alpha}-${key}`,shot=(await call('Page.captureScreenshot',{format:'png'})).data;
        await writeFile(`${dir}/${name}.png`,Buffer.from(shot,'base64'));
        const probes=await evaluate(`(async()=>{const im=new Image();im.src='data:image/png;base64,${shot}';await im.decode();const c=new OffscreenCanvas(im.width,im.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(im,0,0);const scale=devicePixelRatio,atPixel=(px,py)=>Array.from(x.getImageData(px,py,1,1).data),at=(px,py)=>atPixel(Math.floor(px*scale),Math.floor(py*scale)),top=Math.ceil(${anchor.y}*scale),bottom=Math.floor(${anchor.y+anchor.height}*scale),face=${edge==='left'?`Math.floor(${anchor.x+anchor.width}*scale)+1`:`Math.ceil(${anchor.x}*scale)-2`};return {scale,top,bottom,face_x:face,face:Array.from({length:Math.max(0,bottom-top)},(_,i)=>atPixel(face,top+i)),source:at(${anchor.x+anchor.width*.1},${anchor.y+anchor.height*.5}),corner:at(${edge==='left'?anchor.x+1:anchor.x+anchor.width-2},${anchor.y+1}),palette:layerApp.state().palette};})()`);
        records.push({name,target:gap,receiver_bottom:receiverBottom,pick,anchor,probes});
        await writeFile(`${dir}/keyframes.json`,JSON.stringify(records,null,2));
        if(alpha===0)for(const [row,pixel]of probes.face.entries()) {
          const expected=probes.palette.panel.match(/[0-9a-f]{2}/gi).map(v=>parseInt(v,16));
          assert.ok(pixel.slice(0,3).every((v,i)=>Math.abs(v-expected[i])<=2),`${name} face row${row}: ${pixel} expected${expected}`);
        }
        assert.equal(probes.source[3],255,`${name} source remains visible`);
        const selected=probes.palette.selection.match(/[0-9a-f]{2}/gi).map(v=>parseInt(v,16));
        if(alpha===0)assert.ok(probes.source.slice(0,3).every((v,i)=>Math.abs(v-selected[i])<=2),`${name} retains selected source fill`);
        assert.ok(probes.corner.slice(0,3).some((v,i)=>Math.abs(v-selected[i])>8),`${name} source outer corner stays rounded`);
      }
      await mouse('mouseReleased',start);held=false;
      await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();await wait(250);
      assert.equal(await evaluate('document.querySelectorAll(".column-surface,.vector-panel,.vector-source").length'),0,'Closing retires pending vector geometry');
      await evaluate('document.querySelector(".column-tab[data-panel=brushes]").click()');await settle();await wait(250);
      assert.equal(await evaluate('document.querySelectorAll(".column-surface").length'),1,'Reopening prepares a fresh combined surface');
    }
    console.log(`PASS: Web panel bridge ${records.length} rendered keyframes, both edges/themes/transparency, source corners, close and reopen`);
  } finally {
    if(held)await mouse('mouseReleased',{x:700,y:500});
    await send({type:'restore_workspace',workspace:saved});await send({type:'preferences',action:{type:'edit',id:'transparency',value:['off','low','medium','high'].indexOf(transparency)}});
    if(enteredFullscreen)await evaluate('document.fullscreenElement?document.exitFullscreen():null');
  }
}
