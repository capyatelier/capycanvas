import assert from "node:assert/strict";

export async function readyCanvas({evaluate,settle}) {
  await evaluate("new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){const error=layerApp.state().host_error;if(error)reject(Error(error));else if(layerApp.app.brush_ready()&&!layerApp.app.canvas_work_pending()&&!layerApp.state().document_file.busy&&!layerApp.documents.busy())resolve();else if(performance.now()>end)reject(Error('Drawing did not become ready'));else setTimeout(check,25)}check()})");
  await settle();
  await evaluate("layerApp.app.wait_for_canvas()");
  await settle();
}

export function readyCanvasPixels({evaluate,settle,capturePixels}) {
  return async clip => {
    await readyCanvas({evaluate,settle});
    return capturePixels(clip);
  };
}

export async function waitForPixels(capturePixels,settle,expected,message,clip) {
  const end=performance.now()+30000;
  for (;;) {
    const pixels=await capturePixels(clip);
    if(expected(pixels))return pixels;
    assert.ok(performance.now()<end,`${message}; last pixels: ${JSON.stringify(pixels)}`);
    await settle();
  }
}
