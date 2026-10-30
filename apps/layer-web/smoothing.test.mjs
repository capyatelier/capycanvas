import assert from "node:assert/strict";
import {createDrawing} from "./new-drawing-fixture.mjs";

export async function checkSmoothing({call, evaluate, settle}) {
  const inPage = (fn, ...args) => evaluate(`(${fn})(...${JSON.stringify(args)})`);
  const action = async value => { await inPage(value => layerApp.dispatch(value), value); await settle(); };
  const saved = await evaluate("layerApp.state().settings");
  const workspace = await evaluate("layerApp.state().workspace");
  const brush = await evaluate("layerApp.state().brush");
  const waitForInk = () => inPage(() => new Promise((resolve, reject) => {
    const end = performance.now() + 30000;
    const poll = () => {
      if (layerApp.app.brush_ready() && !layerApp.app.canvas_work_pending() && !layerApp.state().document_file.busy) resolve();
      else if (performance.now() > end) reject(Error("Smoothing contact did not finish"));
      else setTimeout(poll, 20);
    };
    poll();
  }));
  const send = async event => { await call("Input.dispatchMouseEvent", {...event, pointerType:"pen"}); await settle(); };
  const away = () => send({type:"mouseMoved", x:1, y:1, buttons:0});
  const pixels = async point => {
    const shot = await call("Page.captureScreenshot", {format:"png"});
    return inPage(async (data, point) => {
      const image = new Image(); image.src = "data:image/png;base64," + data; await image.decode();
      const scale = image.width / innerWidth, canvas = document.createElement("canvas");
      canvas.width = Math.round(100 * scale); canvas.height = Math.round(70 * scale);
      const context = canvas.getContext("2d", {willReadFrequently:true});
      context.drawImage(image, (point.x - 50) * scale, (point.y - 35) * scale,
        canvas.width, canvas.height, 0, 0, canvas.width, canvas.height);
      return Array.from(context.getImageData(0, 0, canvas.width, canvas.height).data);
    }, shot.data, point);
  };
  try {
    await action({type:"invoke",command:"new_document"});
    await evaluate(createDrawing());
    await waitForInk();
    await action({type:"select_brush", id:1});
    await action({type:"set_brush_size", value:18});
    await action({type:"set_color", rgba:[0.05,0.1,0.3,1]});
    await waitForInk();
    await action({type:"customize",action:{type:"set_panel_visible",panel:"tool_settings",visible:true}});
    await action({type:"move_panel", panel:"tool_settings", target:{kind:"float",position:[10,70]}});
    for (const theme of ["light","dark"]) {
      await action({type:"set_theme", theme});
      await inPage(() => {
        const slider = document.querySelector('[data-tool-setting="smoothing_strength"] .number-slider');
        slider.value = .75; slider.dispatchEvent(new Event("input", {bubbles:true}));
      });
      await settle();
      assert.equal(await evaluate('document.querySelector(\'[data-tool-setting="smoothing_strength"] .number-value\').textContent'), "30");
      const control = await evaluate('layerApp.state().tool_settings.find(c=>c.id==="smoothing_strength")');
      assert.equal(control.label, "Smoothing");
      assert.equal(control.group, "");
      assert.equal(control.group_id, "");
    }
    await action({type:"restore_workspace", workspace});
    for (const theme of ["light","dark"]) for (const feedback of [false,true]) {
      await action({type:"restore_settings", settings:{...saved, theme, cursor:"none", feedback}});
      for (const strength of [0,.06,.5,1]) {
        await action({type:"set_tool_setting", id:"smoothing_strength", value:strength});
        const control = await evaluate('layerApp.state().tool_settings.find(c=>c.id==="smoothing_strength")');
        assert.ok(Math.abs(control.value - strength) < 1e-7);
        assert.deepEqual([control.numeric.min,control.numeric.max,control.numeric.scale], [0,1,100]);
        await waitForInk();
        const point = await inPage(() => {
          const camera = layerApp.app.camera(), rect = layerApp.canvas.getBoundingClientRect(), area = camera.work_area;
          return {x:rect.x + (area[0] + area[2]/2)*rect.width/camera.viewport[0],
            y:rect.y + (area[1] + area[3]/2)*rect.height/camera.viewport[1]};
        });
        await away(); const empty = await pixels(point);
        const revision = await evaluate("String(layerApp.state().document_file.revision)");
        await send({type:"mousePressed", x:point.x-35, y:point.y, button:"left", buttons:1, clickCount:1, force:.7});
        for (let i=1;i<=14;i++) await send({type:"mouseMoved", x:point.x-35+i*5,
          y:point.y+Math.sin(i*Math.PI/7)*15, button:"left", buttons:1, force:.7});
        await send({type:"mouseReleased", x:point.x+35, y:point.y, button:"left", buttons:0, clickCount:1, force:0});
        await waitForInk(); await away();
        const painted = await pixels(point);
        assert.notEqual(await evaluate("String(layerApp.state().document_file.revision)"), revision);
        assert.ok(empty.reduce((n,value,i)=>n+(Math.abs(value-painted[i])>8?1:0),0)>20, `${theme}, strength ${strength}, preview ${feedback}: visible ink`);
        await action({type:"invoke", command:"undo"}); await waitForInk();
        assert.deepEqual(await pixels(point), empty);
        await action({type:"invoke", command:"redo"}); await waitForInk();
        assert.deepEqual(await pixels(point), painted);
        await action({type:"invoke", command:"undo"}); await waitForInk();
      }
    }
    await action({type:"reset_tool_setting", id:"smoothing_strength"});
    assert.equal(await evaluate('Math.round(layerApp.state().tool_settings.find(c=>c.id==="smoothing_strength").value*100)'), 6);
    console.log("PASS: smoothing controls, light/dark, strengths 0/6/50/100, preview on/off, pen pixels and exact undo/redo");
  } finally {
    await action({type:"restore_workspace", workspace});
    await action({type:"restore_settings", settings:saved});
    await action({type:"select_brush", id:brush.preset});
    await action({type:"set_brush_size", value:brush.diameter});
    await action({type:"set_color", rgba:brush.color});
  }
}
