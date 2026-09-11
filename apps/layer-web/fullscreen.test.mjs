import assert from "node:assert/strict";

export async function checkDeviceFullscreen({call,evaluate,settle}) {
  const tap = async () => {
    const [x,y] = await evaluate('(()=>{const r=document.querySelector("#fullscreen").getBoundingClientRect();return[r.x+r.width/2,r.y+r.height/2]})()');
    await call("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[{id:1,x,y}]});
    await call("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
  };
  await tap();
  await evaluate('new Promise((resolve,reject)=>{const start=performance.now();function check(){if(document.fullscreenElement && layerApp.state().fullscreen && !document.querySelector("#system-status").hidden)resolve(true);else if(performance.now()-start>10000)reject(Error("Tablet fullscreen failed"));else setTimeout(check,50);}check();})');
  const battery = await evaluate('(async()=>{const b=await navigator.getBattery();return{percent:Math.round(b.level*100),charging:b.charging}})()');
  await settle();
  const label = await evaluate('document.querySelector("#system-battery").getAttribute("aria-label")');
  assert.ok(label.startsWith(`Battery ${battery.percent}%`));
  assert.equal(label.includes("charging"),battery.charging);
  assert.equal(await evaluate('document.querySelector("#system-clock").textContent'),
    await evaluate('new Intl.DateTimeFormat(navigator.languages,{hour:"numeric",minute:"2-digit"}).format(new Date())'));
  if(process.env.LAYER_TEST_ARTIFACTS) {
    const {writeFile} = await import("node:fs/promises");
    const shot = await call("Page.captureScreenshot",{format:"png"});
    await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/tablet-web-fullscreen.png`,Buffer.from(shot.data,"base64"));
  }
  await tap();
  await evaluate('new Promise((resolve,reject)=>{const start=performance.now();function check(){if(!document.fullscreenElement && !layerApp.state().fullscreen && document.querySelector("#system-status").hidden)resolve(true);else if(performance.now()-start>10000)reject(Error("Tablet fullscreen exit failed"));else setTimeout(check,50);}check();})');
  console.log("Tablet fullscreen, native battery reading and locale clock passed",battery);
}

export async function checkFullscreen({call, evaluate, settle, windowId}) {
  const wait = async expression => {
    const deadline = Date.now() + 20000;
    while (Date.now() < deadline) {
      try { if (await evaluate(expression)) return; }
      catch (e) { if (!/context|navigat/i.test(String(e))) throw e; }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw Error(`Timed out: ${expression}`);
  };
  const click = async selector => {
    const [x,y] = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return[r.x+r.width/2,r.y+r.height/2]})()`);
    await call("Input.dispatchMouseEvent",{type:"mousePressed",x,y,button:"left",clickCount:1});
    await call("Input.dispatchMouseEvent",{type:"mouseReleased",x,y,button:"left",clickCount:1});
  };
  assert.equal(await evaluate('document.querySelector("#system-status").hidden'), true);
  assert.equal(await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).application_menus.find(m=>m.id==="view").model.sections.flat().find(i=>i.label==="Full screen").hint'), "");
  await click('#fullscreen');
  await wait('!!document.fullscreenElement && layerApp.state().fullscreen && !document.querySelector("#system-status").hidden');
  await wait('!document.querySelector("#system-battery").hidden');
  assert.equal(await evaluate('document.querySelector("#system-battery").getAttribute("aria-label")'), "Battery 72%, charging");
  await evaluate('window.__statusBattery.level=.08;window.__statusBattery.charging=false;window.__statusBattery.dispatchEvent(new Event("levelchange"));window.__statusBattery.dispatchEvent(new Event("chargingchange"));');
  assert.equal(await evaluate('document.querySelector("#system-battery").classList.contains("low")'), true);
  assert.equal(await evaluate('document.querySelector("#system-battery").getAttribute("aria-label")'), "Battery 8%, low");
  for (const locale of ["en-US", "en-GB"]) {
    await evaluate(`Object.defineProperty(navigator,"languages",{configurable:true,value:[${JSON.stringify(locale)}]});window.dispatchEvent(new Event("languagechange"));`);
    assert.equal(await evaluate('document.querySelector("#system-clock").textContent'),
      await evaluate('new Intl.DateTimeFormat(navigator.languages,{hour:"numeric",minute:"2-digit"}).format(new Date())'));
  }
  assert.ok(await evaluate('(()=>{const r=id=>document.querySelector(id).getBoundingClientRect();return r("#document-title").right<=r("#system-status").left && r("#system-status").right<=r("#fullscreen").left})()'));
  if (process.env.LAYER_TEST_ARTIFACTS) {
    const {writeFile} = await import("node:fs/promises");
    const shot = await call("Page.captureScreenshot",{format:"png"});
    await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/web-fullscreen.png`,Buffer.from(shot.data,"base64"));
  }
  await click('[data-menu="view"] summary');
  await wait('!!document.querySelector("[data-menu=view][open] .popover button")');
  await evaluate('Array.from(document.querySelectorAll("[data-menu=view] button")).find(b=>b.textContent.includes("Full screen")).id="test-menu-fullscreen"');
  await click('#test-menu-fullscreen');
  await wait('!document.fullscreenElement && !layerApp.state().fullscreen && document.querySelector("#system-status").hidden');
  // Browser-owned fullscreen is deliberately separate from the DOM API.
  await call("Browser.setWindowBounds",{windowId,bounds:{windowState:"fullscreen"}},null);
  await wait('!document.fullscreenElement && matchMedia("(display-mode: fullscreen)").matches && layerApp.state().fullscreen && !document.querySelector("#system-status").hidden');
  await call("Browser.setWindowBounds",{windowId,bounds:{windowState:"normal"}},null);
  await wait('!layerApp.state().fullscreen && document.querySelector("#system-status").hidden');
  await call("Browser.setWindowBounds",{windowId,bounds:{windowState:"maximized"}},null);
  await settle();
  assert.equal(await evaluate('layerApp.state().fullscreen'),false,"Maximized is not fullscreen");
  await call("Browser.setWindowBounds",{windowId,bounds:{windowState:"normal"}},null);
  const clockPreference = async value => {
    await evaluate(`layerApp.dispatch({type:"preferences",action:{type:"edit",id:"show_clock",value:${value}}})`);
    await settle();
  };
  await clockPreference(1);
  assert.equal(await evaluate('document.querySelector("#system-clock").hidden'),false);
  assert.equal(await evaluate('document.querySelector("#system-status").hidden'),false);
  assert.equal(await evaluate('document.querySelector("#system-battery").hidden'),true);
  await click('#fullscreen');
  await wait('!!document.fullscreenElement');
  await clockPreference(2);
  assert.equal(await evaluate('document.querySelector("#system-clock").hidden'),true);
  assert.equal(await evaluate('document.querySelector("#system-battery").hidden'),false);
  await click('#fullscreen');
  await wait('!document.fullscreenElement && document.querySelector("#system-status").hidden');
  await clockPreference(0);
  for (const mode of ["absent", "denied"]) {
    const script = await call("Page.addScriptToEvaluateOnNewDocument",{source:`window.__statusBatteryCase=${JSON.stringify(mode)};Object.defineProperty(navigator,"getBattery",{configurable:true,value:${mode==="absent"?"undefined":"()=>Promise.reject(new Error('Battery permission denied'))"}});`});
    await call("Page.reload");
    await wait(`window.__statusBatteryCase===${JSON.stringify(mode)} && !!window.layerApp && !!document.querySelector("#system-status")`);
    await click('#fullscreen');
    await wait('!!document.fullscreenElement && !document.querySelector("#system-status").hidden');
    assert.equal(await evaluate('document.querySelector("#system-battery").hidden'), true);
    assert.ok(await evaluate('document.querySelector("#system-clock").textContent.length>0'));
    await click('#fullscreen');
    await wait('!document.fullscreenElement');
    await call("Page.removeScriptToEvaluateOnNewDocument",{identifier:script.identifier});
  }
  console.log("Fullscreen API/menu, browser fullscreen, locale clocks and visibility preferences, low/charging/missing/denied battery passed");
}
