import assert from "node:assert/strict";

// Real DOM -> Wasm ingress, with deterministic future samples. Chrome may
// legitimately return an empty prediction list for synthetic CDP pen motion.
export async function checkPrediction({call, evaluate, settle}) {
  const action = async value => {
    await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);
    await settle();
  };
  const saved = await evaluate("layerApp.state().settings");
  const capability = await evaluate("typeof PointerEvent.prototype.getPredictedEvents === 'function'");
  await evaluate(`(() => {
    window.predictionTest = { batches: [], pen: layerApp.app.pen, descriptor: Object.getOwnPropertyDescriptor(PointerEvent.prototype, 'getPredictedEvents') };
    layerApp.app.pen = function(records, revision) { predictionTest.batches.push(Array.from(records)); return predictionTest.pen.call(this, records, revision); };
    Object.defineProperty(PointerEvent.prototype, 'getPredictedEvents', {configurable:true, value() {
      return [{pointerId:this.pointerId, pointerType:this.pointerType, clientX:this.clientX+16, clientY:this.clientY,
        pressure:this.pressure, buttons:this.buttons, timeStamp:this.timeStamp+8}];
    }});
  })()`);
  try {
    for (const available of [false, true]) {
      await evaluate(`layerApp.app.prediction_availability(${available})`);
      await action({type:'restore_settings', settings:{...saved, feedback:true, platform_prediction:true, prediction_ms:23, tip_lock:.3}});
      await action({type:'open_settings', page:'input'});
      assert.equal(await evaluate("document.querySelector('#setting-platform-prediction').disabled"), !available);
      for (const id of ['prediction-horizon', 'tip-lock'])
        assert.equal(await evaluate(`document.querySelector('#setting-${id}').disabled`), available);
      assert.equal(await evaluate("layerApp.state().settings.prediction_ms"), 23);
      assert.ok(Math.abs(await evaluate("layerApp.state().settings.tip_lock") - .3) < .000001);
    }
    for (const enabled of [true, false]) {
      await action({type:'preferences', action:{type:'edit', id:'platform_prediction', value:enabled}});
      await action({type:'close_settings'});
      await evaluate("predictionTest.batches=[]");
      const origin = await evaluate("(() => {const r=layerApp.canvas.getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()");
      await call('Input.dispatchMouseEvent', {type:'mousePressed', ...origin, button:'left', buttons:1, clickCount:1, pointerType:'pen', force:.7});
      for (let i=1;i<=4;i++) {
        await call('Input.dispatchMouseEvent', {type:'mouseMoved', x:origin.x+i*8, y:origin.y, buttons:1, pointerType:'pen', force:.7-i*.1});
        await settle();
      }
      await call('Input.dispatchMouseEvent', {type:'mouseReleased', x:origin.x+32, y:origin.y, button:'left', buttons:0, clickCount:1, pointerType:'pen'});
      await settle();
      const records = await evaluate("predictionTest.batches.flatMap(batch=>Array.from({length:batch.length/11},(_,i)=>batch.slice(i*11,i*11+11)))");
      assert.ok(records.some(r=>r[1]===1) && records.some(r=>r[1]===3), 'real contact reaches the engine');
      const predicted = records.filter(r=>r[9]&1);
      assert.equal(predicted.length>0, enabled, 'toggle controls browser prediction ingress');
      assert.ok(predicted.every(r=>r[1]===2 && r[10]===0), 'only provisional pen moves');
      await action({type:'open_settings', page:'input'});
      for (const id of ['prediction-horizon', 'tip-lock'])
        assert.equal(await evaluate(`document.querySelector('#setting-${id}').disabled`), enabled);
    }
  } finally {
    await evaluate(`(() => {
      layerApp.app.pen=predictionTest.pen;
      if(predictionTest.descriptor) Object.defineProperty(PointerEvent.prototype,'getPredictedEvents',predictionTest.descriptor);
      else delete PointerEvent.prototype.getPredictedEvents;
      delete window.predictionTest;
      layerApp.app.prediction_availability(${capability});
    })()`);
    await action({type:'restore_settings', settings:saved});
    await action({type:'close_settings'});
  }
}
