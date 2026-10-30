import assert from 'node:assert/strict';
import {vectorHarness} from './vector-journey-fixture.mjs';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkVectorTools({call,evaluate,settle}) {
  const {send,invoke,tool,stroke,line,key,color:sampleColor,capture,until}=await vectorHarness({call,evaluate,settle},{directory:'artifacts/vector-tools/web',keyDown:'keyDown'});
  const color=at=>sampleColor(at,true);
  const orange=([r,,b])=>r-b>90;
  const green=([r,g,b])=>g-Math.max(r,b)>25;
  const expect=async(name,at,inked,ink=orange)=>{
    const end=Date.now()+20000;let seen;
    do{seen=await color(at);if(ink(seen)===inked)return;await new Promise(r=>setTimeout(r,100));}while(Date.now()<end);
    assert.equal(ink(seen),inked,`${name}: ${seen}`);
  };
  const selectedLayer=()=>evaluate('Number(layerApp.state().layers.find(l=>l.selected).id)');
  await invoke('new_document');
  await evaluate(createDrawing({width:2000,height:1400}));
  await until('the vector drawing is ready','!layerApp.documents.busy()&&!s.document_file.busy&&layerApp.app.brush_ready()',60000);
  await settle();
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    await send({type:'layer',action:{op:'new_vector'}});
    await invoke('fit_canvas');
    await send({type:'set_color',rgba:[.85,.35,.1,1]});
    await send({type:'set_brush_size',value:24});

    await tool({figure:{shape:'rectangle',paint:'fill'}});
    await stroke(line([400,400],[700,620]));
    await expect(`${theme} rectangle`,[550,510],true);
    await expect(`${theme} beside the rectangle`,[780,510],false);
    await capture(`vector-${theme}-figure`);

    await tool({figure:{shape:'path',paint:'outline'}});
    for(const at of [[1000,400],[1300,400]])await stroke([at,at]);
    await stroke(line([1300,700],[1400,780],4));
    await capture(`vector-${theme}-path-drafting`);
    await key('Enter','Enter',13);
    await expect(`${theme} path`,[1150,400],true);
    await capture(`vector-${theme}-path`);

    await invoke('edit_points');
    await expect(`${theme} before editing`,[1150,325],false);
    await stroke(line([1000,400],[1000,250]));
    await expect(`${theme} edited anchor`,[1150,325],true);
    await capture(`vector-${theme}-edit-points`);

    await expect(`${theme} before restyling`,[1300,550],false);
    await send({type:'effect',action:{op:'set',layer:await selectedLayer(),key:'path_width',value:{kind:'number',value:160}}});
    await expect(`${theme} restyled`,[1300,550],true);
    await capture(`vector-${theme}-properties`);

    await invoke('move');
    await stroke(line([350,350],[750,670]));
    await capture(`vector-${theme}-frame`);
    await expect(`${theme} before scaling`,[780,700],false);
    await stroke(line([700,620],[850,760]));
    await expect(`${theme} scaled`,[780,700],true);
    await capture(`vector-${theme}-scaled`);

    await send({type:'select_brush',id:1});
    await send({type:'set_color',rgba:[.1,.7,.2,1]});
    await stroke(line([300,1100],[1700,1100],40));
    await expect(`${theme} inked`,[1000,1100],true,green);
    await invoke('eraser');
    await send({type:'set_brush_size',value:40});
    await stroke(line([1000,1000],[1000,1200]));
    await expect(`${theme} erased`,[1000,1100],false,green);
    await expect(`${theme} kept`,[700,1100],true,green);
    await expect(`${theme} kept beyond the eraser`,[1300,1100],true,green);
    await capture(`vector-${theme}-erased`);
    await send({type:'layer',action:{op:'delete_selected'}});
    await invoke('pen');
  }
}
