import assert from 'node:assert/strict';

export async function checkEdgeReach({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace');
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon'});
  const split=(id,first,second)=>({kind:'split',id,axis:'vertical',fraction:.5,first,second});
  const wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(180);};
  const snapshot=()=>evaluate('layerApp.state().workspace');
  const resolved=()=>evaluate('layerApp.app.layout(innerWidth,innerHeight)');
  let device='mouse';
  const input=async(type,p)=>{
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
  };
  const swipe=async(from,to,steps=10)=>{
    if(device!=='touch')await call('Input.dispatchMouseEvent',{type:'mouseMoved',...from,pointerType:device});
    await input('down',from);
    for(let i=1;i<=steps;i++)await input('move',{x:from.x+(to.x-from.x)*i/steps,y:from.y+(to.y-from.y)*i/steps});
    await wait(220);await input('up',to);await settle();await wait(180);
  };
  const restore=async bands=>{
    const workspace=structuredClone(saved);
    Object.assign(workspace.layout,{bands,floating:[],collapsed:[],column_stacks:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],next_id:Math.max(60,workspace.layout.next_id)});
    workspace.zen_mode=false;
    await send({type:'restore_workspace',workspace});
  };
  const extent=async id=>(await snapshot()).layout.bands.find(b=>b.id===id).extent;
  try {
    await call('Page.bringToFront');
    for(const theme of ['light','dark']) for(device of ['mouse','pen','touch']) {
      await send({type:'set_theme',theme});
      await restore([{id:40,edge:'left',extent:320,root:tabs(41,['stats'])}]);
      let layout=await resolved(),edge=layout.dividers.find(d=>d.id===40),group=layout.groups.find(g=>g.id===41);
      const y=group.bounds.y+group.bounds.height-60,before=await extent(40);
      await swipe({x:edge.bounds.x-20,y},{x:edge.bounds.x+60,y});
      const grown=await extent(40)-before;
      assert.ok(Math.abs(grown-(60-edge.bounds.width/2))<2,`${theme}/${device}: an outward swipe from inside the column grows it to the contact (${grown})`);
      await send({type:'invoke',command:'undo_workspace'});
      const canvasStart={x:edge.bounds.x+edge.bounds.width+20,y};
      await swipe(canvasStart,{x:canvasStart.x+50,y});
      if(device==='touch')assert.ok(await extent(40)>before+40,`${theme}: touch grabs the edge from the canvas (${await extent(40)-before})`);
      else assert.equal(await extent(40),before,`${theme}/${device}: the canvas keeps pen and mouse`);
      if(device==='touch')await send({type:'invoke',command:'undo_workspace'});
      await swipe({x:edge.bounds.x-20,y},{x:edge.bounds.x-20,y:y-60});
      assert.equal(await extent(40),before,`${theme}/${device}: a vertical swipe never grabs a column edge`);

      await restore([{id:40,edge:'left',extent:320,root:split(42,tabs(41,['stats']),tabs(43,['navigator','proof']))}]);
      layout=await resolved();
      const gap=layout.dividers.find(d=>d.id===42);
      await swipe({x:gap.bounds.x+gap.bounds.width/2,y:gap.bounds.y-10},{x:gap.bounds.x+gap.bounds.width/2,y:gap.bounds.y+50});
      const moved=(await resolved()).dividers.find(d=>d.id===42).bounds.y-gap.bounds.y;
      assert.ok(moved>20,`${theme}/${device}: a vertical swipe beside a gap moves it (${moved})`);

      await restore([{id:40,edge:'left',extent:320,root:tabs(41,['stats'])}]);
      await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});
      layout=await resolved();
      const strip=layout.collapsed[0],tile=strip.groups[0].icons[0].bounds;
      await swipe({x:tile.x+tile.width/2,y:tile.y+tile.height/2},{x:tile.x+tile.width/2+90,y:tile.y+tile.height/2});
      assert.equal((await snapshot()).layout.collapsed.length,0,`${theme}/${device}: an outward swipe from a closed strip opens it`);
    }
    device='mouse';
    await restore([{id:40,edge:'left',extent:420,root:tabs(41,['stats'])}]);
    const layout=await resolved(),edge=layout.dividers.find(d=>d.id===40),group=layout.groups.find(g=>g.id===41);
    const point={x:edge.bounds.x-20,y:group.bounds.y+group.bounds.height-60};
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
    for(const clickCount of [1,2])for(const type of ['mousePressed','mouseReleased'])
      await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount});
    await settle();await wait(180);
    assert.equal(await extent(40),0,'a double click in the column edge reach resets the column to its preferred width');
    console.log('PASS: Web edge reach for column edges, the canvas, gaps, closed strips and double-click reset with mouse, pen and touch in both themes');
  } finally {
    await send({type:'restore_workspace',workspace:saved});
  }
}
