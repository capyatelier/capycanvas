import assert from "node:assert/strict";

export async function checkDragCursors({ call, evaluate, settle }) {
  await call('Page.bringToFront');
  await evaluate("new Promise(resolve=>{const ready=()=>{const view=JSON.parse(layerApp.app.workspace_view());if(layerApp.startupTimes.complete&&view.ready&&!view.busy)resolve();else setTimeout(ready,50)};ready()})");
  const wait = async () => {
    await settle(); await evaluate("new Promise(r=>setTimeout(r,180))");
    await evaluate("Promise.all([...document.querySelectorAll('.neighbor-tab-preview')].flatMap(n=>n.getAnimations()).map(a=>a.finished))");
  };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await wait(); };
  const saved = await evaluate("layerApp.state().workspace");
  const fixture = structuredClone(saved);
  const tabs = (id, panels) => ({ kind: "tabs", id, panels, active: panels[0], tab_style: "icon" });
  Object.assign(fixture.layout, { bands: [
    { id: 40, edge: "left", extent: 252, root: tabs(41, ["brushes", "sizes"]) },
    { id: 42, edge: "right", extent: 252, root: tabs(43, ["layers", "properties"]) },
  ], floating: [], collapsed: [], column_scroll: [], fit_tab_groups: [], fit_height_groups:[], column_stacks:[], next_id: Math.max(44, fixture.layout.next_id) });
  fixture.zen_mode = false;
  const center = b => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });
  const rect = selector => evaluate(`(()=>{const b=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height}})()`);
  const cursor = (selector = "#workspace") => evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).cursor`);
  const noSlide = async () => assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay, .dragged-tab-source').length"), 0);
  const clean = async () => {
    assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor ?? null"), null);
    await noSlide();
  };
  const away = await evaluate("({x:innerWidth/2,y:innerHeight/2})");
  let held = false, last = away;
  const move = async p => { last = p; await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...p, button: held ? "left" : "none", buttons: held ? 1 : 0 }); await wait(); };
  const press = async p => { await move(p); held = true; await call("Input.dispatchMouseEvent", { type: "mousePressed", ...p, button: "left", buttons: 1, clickCount: 1 }); };
  const release = async () => { await call("Input.dispatchMouseEvent", { type: "mouseReleased", ...last, button: "left", buttons: 0, clickCount: 1 }); held = false; await wait(); };
  try {
    for(const theme of ['light','dark'])for(const drawer of [false,true]){
      const automatic=structuredClone(fixture);
      const panels=drawer?['brushes','sizes','layers','adjustments']:['toolbar','tool_settings','color','palettes','layers','adjustments'];
      const [first,second]=panels;
      automatic.layout.bands=[{id:42,edge:'right',extent:304,root:{...tabs(43,panels),tab_style:'automatic'}}];
      await send({type:'restore_workspace',workspace:automatic});await send({type:'set_theme',theme});
      if(drawer){
        await send({type:'customize',action:{type:'set_column_collapsed',group:43,collapsed:true}});
        await send({type:'customize',action:{type:'set_column_drawers',column:43,drawers:true}});
        await evaluate(`document.querySelector('.collapsed-column[data-column="43"] [data-panel="${first}"]').click()`);await wait();
      }
      const strip=drawer?'.drawer-tab-strip':'[data-group="43"] .tab-list';
      const names=()=>evaluate(`Object.fromEntries([...document.querySelector(${JSON.stringify(strip)}).children].map(tab=>[tab.dataset.panel,!tab.lastElementChild.hidden]))`);
      const before=await names(),extent=await rect(strip);
      assert.equal(before[first],true,`${theme} drawer=${drawer}: first automatic tab is named`);
      assert.equal(before[second],false,`${theme} drawer=${drawer}: next tab starts icon-only`);
      const source=`${strip} [data-panel="${first}"]`,start=center(await rect(source));
      const neighbor=await rect(`${strip} [data-panel="${second}"]`);
      await press(start);await move({x:start.x+neighbor.width/2+2,y:start.y});await release();await clean();
      const reordered=await names();
      assert.deepEqual(await evaluate(`[...document.querySelector(${JSON.stringify(strip)}).children].map(tab=>tab.dataset.panel)`),[second,first,...panels.slice(2)]);
      await send({type:'customize',action:{type:'set_tab_style',group:43,style:'icon'}});
      await send({type:'customize',action:{type:'set_tab_style',group:43,style:'automatic'}});
      assert.equal((await rect(strip)).width,extent.width,'The fresh automatic strip has the same extent');
      assert.notDeepEqual(await names(),before,'Reordering changes automatic name priority');
      assert.deepEqual(reordered,await names(),`${theme} drawer=${drawer}: post-drop names match a fresh build of the same order`);
    }
    for(const theme of ['light','dark'])for(const drawer of [false,true]){
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      if(drawer){
        await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});
        await send({type:'customize',action:{type:'set_column_drawers',column:41,drawers:true}});
        await evaluate(`document.querySelector('.collapsed-column[data-column="41"] [data-panel="brushes"]').click()`);await wait();
      }
      const selector=drawer?'.drawer-tab-strip .dock-tab[data-panel="brushes"]':'.tab-list .dock-tab[data-panel="layers"]';
      await evaluate(`window.focusedPanelTab=document.querySelector(${JSON.stringify(selector)});window.focusedPanelNeighbor=window.focusedPanelTab.nextElementSibling;window.focusedPanelBody=focusedPanelTab.closest('.dock-group,.content-drawer').querySelector('.panel-frame,.drawer-column');window.focusedPanelNeighbor.focus()`);
      for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Tab',code:'Tab',windowsVirtualKeyCode:9,modifiers:8});
      assert.deepEqual(await evaluate("({same:document.activeElement===window.focusedPanelTab,visible:window.focusedPanelTab.matches(':focus-visible'),tag:document.activeElement.tagName,panel:document.activeElement.dataset.panel})"),{same:true,visible:true,tag:'BUTTON',panel:drawer?'brushes':'layers'},`${theme} drawer=${drawer}: native keyboard focus reaches the selected tab`);
      const start=center(await rect(selector));
      const neighbor=await evaluate("window.focusedPanelNeighbor.getBoundingClientRect().width");
      await press(start);await wait();await move({x:start.x+12,y:start.y});await move({x:start.x+neighbor/2+2,y:start.y});
      assert.equal(await evaluate("!!document.querySelector('.dragged-tab-preview')"),true);
      assert.equal(await evaluate('document.activeElement===window.focusedPanelTab'),true,`${theme} drawer=${drawer}: sliding retains focus on the real tab`);
      await release();await clean();
      assert.deepEqual(await evaluate("({source:window.focusedPanelTab.isConnected,neighbor:window.focusedPanelNeighbor.isConnected,body:focusedPanelBody.isConnected,order:[...window.focusedPanelTab.parentElement.children].map(tab=>tab.dataset.panel)})"),
        {source:true,neighbor:true,body:true,order:drawer?['sizes','brushes']:['properties','layers']},`${theme} drawer=${drawer}: committed reorder retains both native tabs and their content`);
      assert.equal(await evaluate('document.activeElement===window.focusedPanelTab'),true,`${theme} drawer=${drawer}: release retains the focused tab`);
      await evaluate('focusedPanelTab.parentElement.moveBefore=undefined');
      const reverse=center(await rect(selector));
      await press(reverse);await wait();await move({x:reverse.x-12,y:reverse.y});await move({x:reverse.x-neighbor/2-2,y:reverse.y});await release();await clean();
      assert.deepEqual(await evaluate("({source:focusedPanelTab.isConnected,neighbor:focusedPanelNeighbor.isConnected,focused:document.activeElement===focusedPanelTab,order:[...focusedPanelTab.parentElement.children].map(tab=>tab.dataset.panel)})"),
        {source:true,neighbor:true,focused:true,order:drawer?['brushes','sizes']:['layers','properties']},`${theme} drawer=${drawer}: reverse reorder preserves focus without atomic DOM moves`);
      await evaluate('delete focusedPanelTab.parentElement.moveBefore');
      console.log(JSON.stringify({theme,drawer,keyboardFocusedTabRetained:true}));
    }
    await evaluate(`(()=>{
      const send=Worker.prototype.postMessage,workers=new Map();
      const gate=window.tabVectorGate={held:false,replies:[],release(){this.held=false;for(const reply of this.replies.splice(0))reply()},restore(){
        this.release();Worker.prototype.postMessage=send;for(const [worker,record] of workers)worker.onmessage=record.reply;delete window.tabVectorGate;
      }};
      Worker.prototype.postMessage=function(message,...args){
        if(message?.request?.operation==='panel-vectors'){
          let record=workers.get(this);
          if(!record){
            record={ids:new Set(),reply:this.onmessage};workers.set(this,record);const worker=this;
            this.onmessage=function(event){
              if(record.ids.delete(event.data?.id)&&gate.held){gate.replies.push(()=>record.reply.call(worker,event));return}
              return record.reply.call(worker,event);
            };
          }
          record.ids.add(message.id);
        }
        return send.call(this,message,...args);
      };
    })()`);
    for(const theme of ['light','dark']){
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});
      await send({type:'customize',action:{type:'set_column_drawers',column:41,drawers:false}});
      await evaluate(`document.querySelector('.collapsed-column[data-column="41"] [data-panel="brushes"]').click()`);await wait();
      const selector='.tab-list .dock-tab[data-panel="brushes"]';
      await evaluate(`window.focusedPanelTab=document.querySelector(${JSON.stringify(selector)});window.focusedPanelNeighbor=focusedPanelTab.nextElementSibling`);
      const start=center(await rect(selector)),width=await evaluate('focusedPanelNeighbor.getBoundingClientRect().width');
      await press(start);await wait();await move({x:start.x+12,y:start.y});await move({x:start.x+width/2+2,y:start.y});
      assert.equal(await evaluate("!!document.querySelector('.dragged-tab-preview')"),true);
      await evaluate('tabVectorGate.held=true');await release();
      for(let i=0;i<100&&!await evaluate('tabVectorGate.replies.length');i++)await evaluate('new Promise(r=>setTimeout(r,20))');
      assert.ok(await evaluate('tabVectorGate.replies.length'),'The real layout worker reply is withheld');
      assert.deepEqual(await evaluate(`({core:layerApp.state().workspace.layout.bands[0].root.panels,
        dom:[...focusedPanelTab.parentElement.children].map(t=>t.dataset.panel),source:focusedPanelTab.isConnected,neighbor:focusedPanelNeighbor.isConnected,
        hidden:focusedPanelTab.classList.contains('dragged-tab-source'),preview:!!document.querySelector('.tab-slide-overlay')})`),
        {core:['sizes','brushes'],dom:['brushes','sizes'],source:true,neighbor:true,hidden:true,preview:true},`${theme}: the preview covers the old layout until the committed geometry arrives`);
      await evaluate(`window.pendingDropCanvasPresses=0;document.querySelector('#canvas').addEventListener('pointerdown',()=>pendingDropCanvasPresses++,{once:true})`);
      await press(center(await rect('#canvas')));await release();
      assert.equal(await evaluate('pendingDropCanvasPresses'),1,`${theme}: canvas input arrives while the drop layout is pending`);
      await press(center(await rect('.layer-blend')));await release();
      assert.equal(await evaluate("!!document.querySelector('.panel-context-menu:popover-open')"),true,`${theme}: panel controls work while the drop layout is pending`);
      for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await press(center(await rect('.tab-list .dock-tab[data-panel="properties"]')));
      assert.equal(await evaluate('layerApp.state().workspace.layout.bands.find(b=>b.id===42).root.active'),'properties',`${theme}: the pending layout preserves immediate tab selection on press`);
      await release();
      assert.equal(await evaluate('layerApp.state().workspace.layout.bands.find(b=>b.id===42).root.active'),'properties',`${theme}: tab selection works while the drop layout is pending`);
      await evaluate('tabVectorGate.release()');await wait();await clean();
      assert.deepEqual(await evaluate(`({dom:[...focusedPanelTab.parentElement.children].map(t=>t.dataset.panel),source:focusedPanelTab.isConnected,neighbor:focusedPanelNeighbor.isConnected})`),
        {dom:['sizes','brushes'],source:true,neighbor:true},`${theme}: publication hands the preview back to the retained tabs`);
      await send({type:'set_theme',theme:theme==='light'?'dark':'light'});
      const unchanged=center(await rect(selector));
      await press(unchanged);await wait();await move({x:unchanged.x+12,y:unchanged.y});await release();await clean();
      assert.deepEqual(await evaluate('layerApp.state().workspace.layout.bands[0].root.panels'),['sizes','brushes'],`${theme}: dropping in the same slot also completes the handoff`);
    }
    for(const theme of ['light','dark']){
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});
      await send({type:'customize',action:{type:'set_column_drawers',column:41,drawers:false}});
      await evaluate(`document.querySelector('.collapsed-column[data-column="41"] [data-panel="brushes"]').click()`);await wait();
      await press(center(await rect('.tab-list .dock-tab[data-panel="properties"]')));await wait();await move(away);
      const selector='.floating-panel[data-panel="properties"]';
      await evaluate(`window.floatingDropNode=document.querySelector(${JSON.stringify(selector)})`);
      assert.equal(await evaluate('!!floatingDropNode'),true,'The tab is torn into a real floating group');
      await move({x:away.x+120,y:away.y+80});
      const displayed=await rect(selector);
      await evaluate('tabVectorGate.held=true');await release();
      for(let i=0;i<100&&!await evaluate('tabVectorGate.replies.length');i++)await evaluate('new Promise(r=>setTimeout(r,20))');
      assert.ok(await evaluate('tabVectorGate.replies.length'),'The floating release layout reply is withheld');
      assert.deepEqual(await rect(selector),displayed,`${theme}: a floating tab keeps its displayed position until the release layout arrives`);
      await evaluate('tabVectorGate.release()');await wait();await clean();
      const final=await rect(selector);
      assert.equal(final.x,displayed.x);assert.equal(final.y,displayed.y);
      assert.equal(await evaluate('floatingDropNode.isConnected'),true,'The committed floating panel retains its native controls');
    }
    await evaluate('tabVectorGate.restore()');
    for (const end of ["release", "cancel", "capture", "blur"]) {
      await send({ type: "restore_workspace", workspace: fixture });
      const selector = '.dock-tab[data-panel="layers"]';
      assert.equal(await cursor(selector), "default");
      const original = await rect(selector), start = center(original);
      const neighbor = await rect('.dock-tab[data-panel="properties"]');
      await press(start); await move({ x: start.x + 2, y: start.y }); await clean();
      for (const dx of [17, 19, 19, 17, 50, 24, -16]) {
        await move({ x: start.x + dx, y: start.y + 8 });
        const preview = await rect('.dragged-tab-preview');
        assert.equal(preview.x, original.x + Math.max(0, dx), "Attached tab follows the pointer within its strip");
        assert.equal(preview.y, original.y, "Attached tab stays on its original row");
        assert.equal(preview.width, original.width);
        assert.deepEqual(await rect(selector), original, "The tab's insertion slot stays fixed");
        assert.deepEqual(await rect('.dock-tab[data-panel="properties"]'), neighbor);
        assert.equal((await rect('.neighbor-tab-preview')).x, neighbor.x - (dx >= neighbor.width / 2 ? original.width : 0),
          "The dragged edge crosses a frozen midpoint; the same boundary reverses the swap");
        assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0);
      }
      const clip = await rect('.tab-slide-overlay');
      for (const x of [clip.x - 40, clip.x + clip.width + 10]) {
        await move({ x, y: start.y });
        const preview = await rect('.dragged-tab-preview');
        assert.equal(preview.x, x < clip.x ? clip.x : clip.x + clip.width - preview.width);
        assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0,
          "Visual clamping does not change the undocking threshold");
      }
      await move(away);
      await noSlide();
      assert.equal(await cursor(), "grabbing");
      assert.equal(await cursor("#canvas"), "grabbing", "Canvas cannot hide the drag cursor");
      assert.equal(await cursor('.dock-tab[data-panel="brushes"]'), "grabbing");
      if (end === "cancel") await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");
      if (end === "capture") await evaluate("document.querySelector('#workspace').releasePointerCapture(1)");
      if (end === "blur") await evaluate("window.dispatchEvent(new Event('blur'))");
      await release(); await clean();
      assert.notEqual(await cursor("#canvas"), "grabbing");
    }
    // Attached slides clean up without disturbing normal insertion or cancellation.
    for (const end of ["release", "cancel", "capture", "blur"]) {
      await send({ type: "restore_workspace", workspace: fixture });
      const selector = '.dock-tab[data-panel="properties"]';
      const source = await rect(selector);
      const first = await rect('.dock-tab[data-panel="layers"]');
      const start = center(source);
      await press(start);
      // Release just past halfway, before the pointer reaches the old midpoint.
      const target = { x: start.x - first.width / 2 - 1, y: start.y };
      await move(target);
      assert.equal(await evaluate("document.querySelectorAll('.dragged-tab-preview').length"), 1);
      assert.equal((await rect('.neighbor-tab-preview')).x, first.x + source.width);
      await move(target);
      assert.equal((await rect('.neighbor-tab-preview')).x, first.x + source.width,
        "Moving neighbors cannot oscillate the insertion decision");
      if (end === "cancel") await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");
      if (end === "capture") await evaluate("document.querySelector('#workspace').releasePointerCapture(1)");
      if (end === "blur") await evaluate("window.dispatchEvent(new Event('blur'))");
      await release(); await clean();
      assert.deepEqual(await evaluate("layerApp.state().workspace.layout.bands.find(b=>b.id===42).root.panels"),
        end === "release" ? ["properties", "layers"] : ["layers", "properties"]);
      assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0);
    }
    await send({ type: "restore_workspace", workspace: fixture });
    await send({ type: "customize", action: { type: "set_column_collapsed", group: 41, collapsed: true } });
    await press(center(await rect('.collapsed-column[data-column="41"] > .panel-grip')));
    await move(away);
    assert.equal(await cursor(), "grabbing");
    const target = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.id===43).bounds");
    await move({ x: target.x - 20, y: target.y + target.height / 2 });
    assert.equal(await cursor(), "grabbing");
    await move(away); assert.equal(await cursor(), "grabbing");
    await release(); await clean();

    // Pointer capture keeps the resize cursor when the divider is rebuilt.
    await send({ type: "restore_workspace", workspace: fixture });
    const divider = await evaluate("layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.id===40)");
    const start = center(divider.bounds);
    await press(start); await move({ x: start.x + 40, y: start.y });
    assert.equal(await cursor(), "col-resize");
    assert.equal(await cursor("#canvas"), "col-resize");
    await release(); await clean();
    console.log("Web drag feedback: attached tab sliding, insertion, tear-off, cursors, resize, release, cancellation, capture loss, and blur passed.");
  } finally {
    if (held) await release();
    await evaluate('window.tabVectorGate?.restore()');
    await send({ type: "restore_workspace", workspace: saved });
    await evaluate('delete window.focusedPanelTab;delete window.focusedPanelNeighbor;delete window.focusedPanelBody;delete window.floatingDropNode');
  }
}
