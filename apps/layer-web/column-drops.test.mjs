import assert from 'node:assert/strict';

// CDP delivers browser mouse, touch and pen input through the production path.
export async function checkColumnDrops({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace');
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon'});
  const wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(220);};
  const snapshot=()=>evaluate('layerApp.state().workspace');
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const center=b=>({x:b.x+b.width/2,y:b.y+b.height/2});
  let device='mouse',point,down=false;
  const input=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
    down=type!=='up';await settle();
  };
  try {
    await call('Page.bringToFront');
    for(const edge of ['left','right']) {
      const fixture=structuredClone(saved);
      Object.assign(fixture.layout,{bands:[
        {id:40,edge,extent:252,root:{kind:'split',id:41,axis:'vertical',fraction:.5,first:tabs(42,['brushes']),second:tabs(43,['sizes'])}},
        {id:44,edge:edge==='left'?'right':'left',extent:252,root:tabs(45,['layers','properties','adjustments'])},
      ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
      fixture.zen_mode=false;
      for(device of ['mouse','touch','pen']) {
        for(const mode of [-8,-5,0,5,8,'cancel']) {
          const merge=typeof mode==='number'&&Math.abs(mode)>6;
          await send({type:'restore_workspace',workspace:fixture});
          for(const group of [42,45])await send({type:'customize',action:{type:'set_column_collapsed',group,collapsed:true}});
          const before=await snapshot();
          const tile=await rect('.column-tab[data-panel=sizes]');
          const separator=await evaluate("document.querySelector('.column-tab[data-panel=sizes]').previousElementSibling.getBoundingClientRect().toJSON()");
          const divider=center(separator);
          assert.equal(divider.y,tile.y-6,'DOM and shared separator alignment');
          const destination={x:divider.x,y:divider.y+(mode==='cancel'?5:mode)};
          await input('down',center(await rect('.column-tab[data-panel=layers]')));await wait(650);
          await input('move',await evaluate('({x:innerWidth*.5,y:innerHeight*.55})'));
          await input('move',destination);await wait(100);
          assert.equal(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"),false,'Drag closes the held menu');
          assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),false);
          if(!merge) {
            const hint=await rect('.drop-indicator');
            assert.ok(Math.abs(center(hint).y-divider.y)<1,`${edge} ${device} ${mode}: preview centered on separator`);
          }
          if(mode==='cancel') {
            await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
            await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
          }
          await input('up');await wait(250);
          assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),true);
          if(mode==='cancel') {assert.deepEqual(await snapshot(),before);continue;}
          const after=await snapshot();
          const column=await evaluate('layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.id===41)');
          assert.equal(after.layout.floating.length,0);
          if(merge) {
            assert.equal(column.groups.length,2);
            assert.ok(column.groups[mode<0?0:1].icons.some(i=>i.panel==='layers'),'Adjacent tile edge joins the existing group');
          } else {
            assert.equal(column.groups.length,3,`${edge} ${device} ${mode}: creates a new group`);
            assert.deepEqual(column.groups[1].icons.map(i=>i.panel),['layers']);
          }
          await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),before);
          await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),after);
        }
      }
    }
    for(const edge of ['top','left']) {
      const fixture=structuredClone(saved);
      Object.assign(fixture.layout,{bands:[{id:40,edge,extent:36,root:tabs(41,['toolbar'])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(42,fixture.layout.next_id)});
      fixture.zen_mode=false;
      const ids=Array.from({length:5},()=>fixture.layout.next_tile_id++);
      const controls=[{kind:'command',command:'brush'},{kind:'command',command:'eraser'},{kind:'divider'},{kind:'command',command:'lasso'},{kind:'command',command:'hand'}];
      fixture.layout.panels.find(p=>p.id==='toolbar').content.tiles=ids.map((id,i)=>({id,control:controls[i]}));
      const tile=id=>`.toolbar-controls [data-tile="${id}"]`;
      for(device of ['mouse','touch','pen'])for(const mode of [-5,5,8,'cancel']) {
        await send({type:'restore_workspace',workspace:fixture});
        const before=await snapshot(),divider=center(await rect(tile(ids[2])));
        const point={...divider};point[edge==='top'?'x':'y']+=mode==='cancel'?5:mode;
        await input('down',center(await rect(tile(ids[0]))));await wait(650);
        await input('move',point);await wait(100);
        assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),false);
        if(mode!==8) {
          const hint=center(await rect('.drop-indicator'));
          assert.ok(Math.abs(hint.x-divider.x)<1&&Math.abs(hint.y-divider.y)<1,`${edge} ${device} ${mode}: toolbar preview on divider`);
        }
        if(mode==='cancel') {
          await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
          await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        }
        await input('up');await wait(250);
        assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),true);
        if(mode==='cancel') {assert.deepEqual(await snapshot(),before);continue;}
        const after=await snapshot(),tiles=after.layout.panels.find(p=>p.id==='toolbar').content.tiles;
        assert.deepEqual(tiles.map(t=>t.id),mode===8?[ids[1],ids[2],ids[0],ids[3],ids[4]]:[ids[1],ids[2],ids[0],fixture.layout.next_tile_id,ids[3],ids[4]]);
        assert.equal(tiles.filter(t=>t.control.kind==='divider').length,mode===8?1:2);
        await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),before);
        await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),after);
        if(mode!==8) {
          const last=await rect(tile(ids[4]));
          const point=edge==='top'?{x:last.x+last.width-3,y:last.y+last.height/2}:{x:last.x+last.width/2,y:last.y+last.height-3};
          await input('down',center(await rect(tile(ids[0]))));await wait(650);
          await input('move',point);await input('up');await wait(250);
          const collapsed=await snapshot(),remaining=collapsed.layout.panels.find(p=>p.id==='toolbar').content.tiles;
          assert.deepEqual(remaining.map(t=>t.id),[ids[1],ids[2],ids[3],ids[4],ids[0]],'An emptied group keeps only its first divider');
          assert.equal(await evaluate(`document.querySelectorAll(${JSON.stringify(tile(fixture.layout.next_tile_id))}).length`),0,'Redundant divider DOM is removed');
          await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),after,'One undo restores the nonempty group and its divider IDs');
          await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),collapsed);
        }
      }
    }
    console.log('PASS: browser mouse/touch/pen column and toolbar targets, separate tool groups, empty-group cleanup, cancellation and undo/redo');
  } finally {
    if(down)await input('up');
    await send({type:'restore_workspace',workspace:saved});
  }
}
