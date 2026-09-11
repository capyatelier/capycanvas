import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

// Runs on desktop or tablet Chrome without drawing or changing display metrics.
export async function checkMediumTiles({ call, evaluate, settle }) {
  const directory = process.env.LAYER_TEST_ARTIFACTS || "artifacts/ui/medium-tiles/web";
  await mkdir(directory, { recursive: true });
  const saved = await evaluate("({workspace:layerApp.state().workspace,settings:layerApp.state().settings})");
  const send = async action => {
    await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
    await settle();
    await evaluate("new Promise(resolve=>setTimeout(resolve,280))");
  };
  const customize = action => send({ type: "customize", action });
  const docked = '.dock-group[data-panel="toolbar"] .toolbar-controls';
  const group = panel => evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}))`);
  try {
    await send({ type: "restore_settings", settings: { ...saved.settings, total_zen: false } });
    for (const [style, name, width, height, icon, lines, weight] of [
      ["medium", "Medium Tiles", 54, 54, 24, 0, 400],
      ["medium_labeled", "Medium Labeled Tiles", 108, 54, 16, 2, 400],
      ["labeled", "Large Labeled Tiles", 108, 72, 16, 3, 700],
    ]) {
      const check = async selector => {
        const tiles = await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(selector)});return root?[...root.querySelectorAll('.tool-tile')].filter(n=>n.querySelector('svg')&&n.getClientRects().length).map(n=>{const r=n.getBoundingClientRect(),i=n.querySelector('svg').getBoundingClientRect(),l=n.querySelector('.tile-label'),s=l&&getComputedStyle(l),b=l?.getBoundingClientRect();return {size:[r.width,r.height,i.width,i.height],label:l&&{lines:s.webkitLineClamp,weight:s.fontWeight,height:b.height,inside:b.top>=r.top-.01&&b.bottom<=r.bottom+.01&&b.left>=r.left-.01&&b.right<=r.right+.01}}}):[]})()`);
        assert.ok(tiles.length > 0, `Missing toolbar: ${selector}`);
        for (const tile of tiles) {
          assert.ok(tile.size.every((value, i) => Math.abs(value - [width, height, icon, icon][i]) < .01), `${style}: ${selector}: ${tile.size}`);
          if (lines) {
            assert.equal(tile.label?.lines, String(lines));
            assert.equal(tile.label.weight, String(weight));
            assert.ok(tile.label.height <= lines * 18 + .01 && tile.label.inside, `Label clipped: ${JSON.stringify(tile)}`);
          } else assert.equal(tile.label, null);
        }
      };
      await send({ type: "restore_workspace", workspace: saved.workspace });
      await customize({ type: "set_panel_visible", panel: "toolbar", visible: true });
      await send({ type: "move_panel", panel: "toolbar", target: { kind: "edge", edge: "top", outer: true } });
      const camera = await evaluate("({zoom:layerApp.state().camera.zoom,rotation:layerApp.state().camera.rotation,pixels:[document.querySelector('#canvas').width,document.querySelector('#canvas').height]})");
      // Select each size through the actual menu.
      await evaluate("(()=>{const n=document.querySelector('.dock-group[data-panel=toolbar] .panel-grip'),r=n.getBoundingClientRect();n.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.x+5,clientY:r.y+5}))})()");
      await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(n=>n.textContent===${JSON.stringify(name)}).click()`);
      await settle();
      await check(docked);
      for (const edge of ["left", "right", "bottom", "top"]) {
        await send({ type: "move_panel", panel: "toolbar", target: { kind: "edge", edge, outer: true } });
        await check(docked);
      }
      for (const theme of ["light", "dark"]) {
        await send({ type: "set_theme", theme });
        await check(docked);
        const shot = await call("Page.captureScreenshot", { format: "png" });
        await writeFile(`${directory}/${style}-${theme}.png`, Buffer.from(shot.data, "base64"));
      }
      await send({ type: "move_panel", panel: "toolbar", target: { kind: "float", position: [200, 180] } });
      await check(docked);
      // A collapsible column must contain a content panel.
      await customize({ type: "set_panel_visible", panel: "navigator", visible: true });
      await send({ type: "move_panel", panel: "navigator", target: { kind: "edge", edge: "right", outer: true } });
      const column = (await group("navigator")).id;
      await send({ type: "move_panel", panel: "toolbar", target: { kind: "tab", group: column, index: null } });
      await customize({ type: "set_column_collapsed", group: column, collapsed: true });
      await evaluate("document.querySelector('.collapsed-column [data-panel=toolbar]').click()");
      // Drawer measurement follows its opening animation; wait for geometry,
      // since a fixed delay can observe the initial button size on the tablet.
      await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+5000;function check(){const n=document.querySelector('.content-drawer .toolbar-controls[data-panel="toolbar"] .tool-tile'),r=n?.getBoundingClientRect();if(r&&Math.abs(r.width-${width})<.01&&Math.abs(r.height-${height})<.01)resolve();else if(performance.now()>end)reject(Error('Drawer layout timed out'));else requestAnimationFrame(check);}check();})`);
      await check('.content-drawer .toolbar-controls[data-panel="toolbar"]');
      await customize({ type: "set_column_collapsed", group: column, collapsed: false });
      await send({ type: "move_panel", panel: "toolbar", target: { kind: "edge", edge: "top", outer: true } });
      await send({ type: "invoke", command: "zen_mode" });
      await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:innerWidth/2,clientY:innerHeight/2,pointerType:'mouse',bubbles:true}))");
      await settle();
      await check('.zen-toolbar .toolbar-controls[data-panel="toolbar"]');
      await send({ type: "invoke", command: "zen_mode" });
      assert.deepEqual(await evaluate("({zoom:layerApp.state().camera.zoom,rotation:layerApp.state().camera.rotation,pixels:[document.querySelector('#canvas').width,document.querySelector('#canvas').height]})"), camera);
      // Each size choice survives an actual reload, including the legacy labeled ID.
      await call("Page.reload");
      let ready = false;
      for (let i = 0; i < 300 && !ready; i++) {
        await new Promise(resolve => setTimeout(resolve, 100));
        try { ready = await evaluate("!!window.layerApp?.app.brush_ready()"); } catch { /* navigation replaces the execution context */ }
      }
      assert.ok(ready, "Reloaded app became ready");
      assert.equal(await evaluate("layerApp.app.panel_view('toolbar').tile_style"), style);
      await check(docked);
      assert.equal(await evaluate("document.querySelector('#status').textContent"), "");
      console.log(`PASS: ${name}, dimensions, label styling, all dock edges, floating, drawer, Zen, canvas invariance and reload persistence`);
    }
  } finally {
    await send({ type: "restore_settings", settings: saved.settings });
    await send({ type: "restore_workspace", workspace: saved.workspace });
  }
}
