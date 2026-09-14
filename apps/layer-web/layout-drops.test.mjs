import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Actual browser pointer capture and shared docking policy, including the header.
export async function checkLayoutDrops({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace');
  const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/layout-drops/web';
  await mkdir(output,{recursive:true});
  const wait=async()=>{await settle();await evaluate('new Promise(r=>setTimeout(r,90))');};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const snapshot=()=>evaluate('layerApp.state().workspace');
  const resolved=()=>evaluate('layerApp.app.layout(innerWidth,innerHeight)');
  const tabs=(id,panels,active=panels[0])=>({kind:'tabs',id,panels,active,tab_style:'icon_name'});
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const center=b=>({x:b.x+b.width/2,y:b.y+b.height/2});
  let device='mouse',point,down=false;
  const input=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
    down=type!=='up';await wait();
  };
  const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await wait();};
  try {
    await call('Page.bringToFront');
    for(const [theme,edge,scale] of [['dark','left',1],['light','right',2]]) {
      await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:scale,mobile:false});
      await send({type:'set_theme',theme});
      for(device of ['mouse','touch','pen'])for(const target of ['menubar','stack-menubar','body','tabs-top','tabs-lower'])for(const source of ['panel','group','toolbar','column']) {
        if(source==='column'&&!target.includes('menubar'))continue;
        const label=`${theme}/${device}/${source}/${target}`;
        console.log(`Layout drop: ${label}`);
        const fixture=structuredClone(saved);
        Object.assign(fixture.layout,{bands:[
          {id:40,edge,extent:252,root:{kind:'split',id:41,axis:'vertical',fraction:.5,first:tabs(42,['brushes']),second:tabs(43,['sizes'])}},
          {id:44,edge:edge==='left'?'right':'left',extent:310,root:tabs(45,['layers','adjustments','properties'])},
          {id:46,edge:'top',extent:36,root:tabs(47,['toolbar'])},
        ],floating:[],collapsed:[],column_stacks:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(50,fixture.layout.next_id)});
        fixture.zen_mode=false;
        await send({type:'restore_workspace',workspace:fixture});
        if(target==='stack-menubar')await send({type:'customize',action:{type:'set_column_collapsed',group:42,collapsed:true}});
        if(source==='column')await send({type:'customize',action:{type:'set_column_collapsed',group:45,collapsed:true}});
        if(source==='group') {
          await send({type:'select_panel_tab',group:45,panel:'properties'});
          await send({type:'move_group',group:45,target:{kind:'float',position:[700,300]}});
        }
        const before=await snapshot(),layout=await resolved(),group=target==='tabs-lower'?43:42;
        const b=target==='stack-menubar'?layout.collapsed.find(c=>c.id===41).bounds:layout.groups.find(g=>g.id===group).bounds;
        const selector=source==='column'?'.collapsed-column[data-column="45"] > .panel-grip':source==='toolbar'?'.toolbar-controls[data-panel="toolbar"] > .panel-grip':`.dock-group[data-group="45"] ${source==='panel'?'.dock-tab[data-panel="layers"]':'.dock-tabs > .panel-grip'}`;
        const start=center(await rect(selector));
        const destination={x:b.x+b.width/2,y:target.includes('menubar')?(await rect('#header')).height/2:target.startsWith('tabs')?b.y+layout.tab_bar_height+3:b.y+b.height/2};
        if(target.startsWith('tabs')) {
          const tab=await rect(`.dock-group[data-group="${group}"] .dock-tab`);
          destination.x=target==='tabs-top'?tab.x+4:tab.right-4;
        }
        const begin=async()=>{await input('down',start);await input('move',{x:720,y:550});await input('move',destination);};
        await begin();
        const hint=await evaluate("JSON.parse(JSON.stringify(layerApp.app.workspace_update().drag?.drop_hint??null,(_,v)=>typeof v==='bigint'?Number(v):v))");
        const expected=target==='menubar'?{kind:'split',group:42,edge:'top'}:target==='stack-menubar'?{kind:'stack_column',column:41,before:true}:{kind:'tab',group,index:target==='tabs-lower'?1:0};
        assert.deepEqual(hint?.target,expected,label);
        const preview=await evaluate(`(()=>{const n=document.querySelector('.drop-indicator'),s=getComputedStyle(n);return{hidden:n.hidden,body:n.classList.contains('drop-body'),bounds:n.getBoundingClientRect().toJSON(),background:s.backgroundColor,border:s.borderTopWidth};})()`);
        assert.equal(preview.hidden,false,label);
        assert.equal(preview.body,target==='body',label);
        assert.ok(Math.abs(preview.bounds.x-hint.bounds.x)<1&&Math.abs(preview.bounds.width-hint.bounds.width)<1,`${label}: native preview geometry`);
        if(target==='body') {
          assert.equal(preview.border,'2px');
          assert.ok(preview.background.includes('0.25'),`${label}: translucent fill ${preview.background}`);
          assert.equal(hint.bounds.y,b.y+layout.tab_bar_height);
          assert.equal(hint.bounds.width,b.width);
        } else if(target.startsWith('tabs')) {
          assert.equal(hint.bounds.width,3);assert.equal(hint.bounds.y,b.y);
        } else assert.ok(hint.bounds.height<=3);
        if(device==='mouse'&&source==='group') {
          const shot=await call('Page.captureScreenshot',{format:'png'});
          await writeFile(`${output}/${theme}-${target}.png`,Buffer.from(shot.data,'base64'));
        }
        if(source==='panel') {
          await escape();await input('up');assert.deepEqual(await snapshot(),before,`${label}: cancel`);
          await begin();
        }
        await input('up');
        assert.equal(await evaluate('document.querySelector(".drop-indicator").hidden'),true);
        const after=await snapshot(),r=await resolved();
        const moved=source==='toolbar'?['toolbar']:['group','column'].includes(source)?['layers','adjustments','properties']:['layers'];
        if(target==='body'||target.startsWith('tabs')) {
          const g=r.groups.find(g=>g.id===group),old=target==='tabs-lower'?['sizes']:['brushes'];
          assert.deepEqual(g.panels,target==='tabs-lower'?[...old,...moved]:[...moved,...old],`${label}: order`);
          assert.equal(g.active,source==='group'?'properties':moved[0],`${label}: selected content`);
        } else if(target==='stack-menubar') {
          const stack=after.layout.column_stacks.find(s=>s.members.includes(41));
          assert.equal(stack.members.length,2);assert.equal(stack.members[1],41);assert.equal(stack.drawers,false);
        } else {
          assert.ok(r.groups.find(g=>g.panels.includes(moved[0])).bounds.y<r.groups.find(g=>g.id===42).bounds.y);
          assert.equal(after.layout.collapsed.length,0);
        }
        await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),before,`${label}: undo`);
        await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),after,`${label}: redo`);
      }
    }
    console.log('PASS: Web menu-bar/body/tab drops, panel/group/toolbar/column sources, mouse/touch/pen, both sides/themes, 1x/2x, cancellation and history');
  } finally {
    if(down)await input('up');
    await call('Emulation.clearDeviceMetricsOverride');
    await send({type:'restore_workspace',workspace:saved});
  }
}
