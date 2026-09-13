import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

// Focused review regression: inspect rendered feedback through real contacts,
// rather than treating aria-pressed alone as evidence of the right highlight.
export async function checkTitleBarFeedback({call, evaluate, settle}) {
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const wait = async expression => {
    for (let i = 0; i < 200; i++) { if (await evaluate(expression)) return; await pause(50); }
    throw Error(`Title-bar feedback timeout: ${expression}`);
  };
  const center = selector => evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden feedback target');return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  let device = 'mouse', point, pressed = false;
  const pointer = async (type, p = point) => {
    point = p;
    if (device === 'touch') await call('Input.dispatchTouchEvent', {
      type: {down:'touchStart',move:'touchMove',up:'touchEnd'}[type],
      touchPoints: type === 'up' ? [] : [{id:1,...p}],
    });
    else await call('Input.dispatchMouseEvent', {
      type: {down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],
      ...p, button:'left', buttons:type === 'down' || type === 'move' && pressed ? 1 : 0,
      clickCount:1, pointerType:device,
    });
    if (type !== 'move') pressed = type === 'down';
    await settle();
  };
  const click = async selector => { await pointer('down', await center(selector)); await pointer('up'); await pause(180); };
  const paint = selector => evaluate(`(()=>{const b=document.querySelector(${JSON.stringify(selector)}),c=document.createElement('canvas');c.width=c.height=1;const x=c.getContext('2d',{willReadFrequently:true}),css=getComputedStyle(b).backgroundColor;x.fillStyle=css;x.fillRect(0,0,1,1);return{css,rgba:[...x.getImageData(0,0,1,1).data],selected:b.getAttribute('aria-pressed'),drawer:b.dataset.drawerFacing}})()`);
  const blue = async selector => {
    const value = await paint(selector), [r,g,b,a] = value.rgba;
    assert.equal(value.selected,'true',`${device}: ${JSON.stringify(value)}`); assert.ok(b > g + 50 && g > r + 40,JSON.stringify(value));
    assert.ok(Math.abs(a - 56) <= 1,`Selected tool uses GTK's 22% blue: ${JSON.stringify(value)}`);
  };
  const grey = async (selector, alpha) => {
    const value = await paint(selector), [r,g,b,a] = value.rgba;
    // Unpremultiplying a 10% alpha pixel can spread one-byte rounding over
    // ten RGB values; the theme's neutral text also has a slight blue tint.
    assert.equal(value.selected,'false'); assert.ok(Math.max(r,g,b)-Math.min(r,g,b) <= 12,JSON.stringify(value));
    assert.ok(Math.abs(a-alpha) <= 1,`${device}: Neutral feedback opacity: ${JSON.stringify(value)}`);
  };
  const dir = process.env.LAYER_TEST_ARTIFACTS || 'artifacts/title-bar/feedback';
  await mkdir(dir,{recursive:true});
  await wait('layerApp.startupTimes.complete != null');
  const pageScale = await evaluate('visualViewport.scale');
  await click('.workspace-switcher button[data-workspace-id="builtin:workspace:painter"]');
  await wait('!JSON.parse(layerApp.app.workspace_view()).busy');
  assert.deepEqual(await evaluate('layerApp.state().workspace.layout.bands'),[]);
  assert.equal(await evaluate("[...document.querySelectorAll('.dock-panel')].filter(n=>n.getBoundingClientRect().width>0).length"),0,'Fresh Sketch shows only its title bar');
  const entries = await evaluate('layerApp.state().workspace.layout.header.zones.flat()');
  const tool = command => `[data-header-item="${entries.find(e=>e.item.control?.command===command).id}"] .header-tool`;
  const color = `[data-header-item="${entries.find(e=>e.item.control?.kind==='color').id}"] .header-tool`;
  const brush = tool('brush'), erase = tool('eraser');
  try {
    for (const size of ['small','large','medium']) {
      await send({type:'customize',action:{type:'header',action:{type:'edit',editing:true}}});
      await click(`[data-header-size="${size}"]`); await click('#header-edit-done');
      const pill = await evaluate("(()=>{const p=document.querySelector('.workspace-switcher'),r=p.getBoundingClientRect(),h=document.querySelector('#header').getBoundingClientRect();return{height:r.height,center:r.y+r.height/2,headerCenter:h.y+h.height/2,buttons:[...p.children].map(n=>n.getBoundingClientRect().height)}})()");
      assert.ok(Math.abs(pill.height-34)<.02); assert.ok(pill.buttons.every(height=>Math.abs(height-26)<.02));
      assert.ok(Math.abs(pill.center-pill.headerCenter)<.1,'Pill remains centered without growing');
    }
    for (const theme of ['dark','light']) {
      await send({type:'set_theme',theme});
      for (device of ['mouse','touch','pen']) {
        console.log('Feedback journey',theme,device);
        await click(brush); await blue(brush);
        await pointer('down',await center(brush)); await blue(brush); await pointer('up');
        assert.equal(await evaluate('visualViewport.scale'),pageScale,'Repeated tool taps do not trigger browser double-tap zoom');
        await click(color); await grey(color,26); await blue(brush);
        assert.equal((await paint(color)).drawer,'bottom');
        await click(erase); await blue(erase); assert.equal((await paint(brush)).selected,'false');
        await pointer('down',{x:10,y:2}); await pointer('up');
        await click(brush); await blue(brush); assert.equal((await paint(erase)).selected,'false');
        // Hover stays blue for selected tools and neutral for other tools.
        if (device !== 'touch') {
          await pointer('move',await center(brush)); await blue(brush);
          await pointer('move',await center(erase)); await grey(erase,26);
        }
        const shot = await call('Page.captureScreenshot',{format:'png'});
        await writeFile(`${dir}/${theme}-${device}.png`,Buffer.from(shot.data,'base64'));
      }
    }
    device = 'mouse';
    // Add one action as a fixture; the full title-bar suite covers its bank drop.
    await send({type:'customize',action:{type:'header',action:{type:'edit',editing:true}}});
    await send({type:'customize',action:{type:'header',action:{type:'add',zone:'center',before:null,item:{kind:'tool',control:{kind:'command',command:'zoom_in'}}}}});
    await send({type:'customize',action:{type:'header',action:{type:'edit',editing:false}}});
    const action = await evaluate("layerApp.state().workspace.layout.header.zones.flat().find(e=>e.item.control?.command==='zoom_in').id");
    const selector = `[data-header-item="${action}"] .header-tool`;
    const zoom = await evaluate('layerApp.state().camera.zoom');
    for (device of ['mouse','touch','pen']) {
      console.log('Action press',device);
      await pointer('down',await center(selector)); await grey(selector,41); await pointer('up');
      assert.equal((await paint(selector)).selected,'false');
      assert.equal(await evaluate("document.querySelectorAll('[data-header-pressed]').length"),0,'Release clears contact feedback');
    }
    assert.ok(await evaluate('layerApp.state().camera.zoom') > zoom);
    await send({type:'invoke',command:'undo_workspace'});
    console.log('PASS: minimal Sketch, constant centered pill at every size, 22% selected-tool blue through mouse/touch/pen press and hover, 10% neutral drawer/hover, 16% neutral action press, both themes');
  } finally { if (pressed) await pointer('up'); }
}
