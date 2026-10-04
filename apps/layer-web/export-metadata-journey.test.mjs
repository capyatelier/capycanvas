import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkExportMetadata({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/photo-m3/export-metadata-web';
  await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>60000)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1600)));else setTimeout(poll,30)}catch(e){reject(e)}}poll()})`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing enabled '+${JSON.stringify(label)});b.click()})()`);
  const set=(label,value)=>evaluate(`(()=>{const n=document.querySelector('dialog[open] [aria-label="'+${JSON.stringify(label)}+'"]');n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);
  const shown=label=>evaluate(`(()=>{const n=document.querySelector('dialog[open] [aria-label="'+${JSON.stringify(label)}+'"]');return !!n&&!n.closest('label').hidden})()`);
  const idle=()=>wait('!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
  const open=async name=>{
    const epoch=await evaluate('Number(layerApp.state().document_file.epoch)');
    await evaluate(`exportMetadata.openName=${JSON.stringify(name)}`);await invoke('open_document');
    await wait(`Number(layerApp.state().document_file.epoch)!==${epoch}&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()`);
  };
  const saved=async(name,finish)=>{
    await evaluate('exportMetadata.last=null');await finish();await idle();await wait('!!exportMetadata.last');
    await evaluate(`exportMetadata.files.set(${JSON.stringify(name)},exportMetadata.last.slice());exportMetadata.handles.set(${JSON.stringify(name)},exportMetadata.target)`);
    const bytes=Buffer.from(await evaluate('Array.from(exportMetadata.last)'));
    await writeFile(`${directory}/${name}`,bytes);
    return bytes;
  };
  const nativeClick=async expression=>{
    const point=await evaluate(`(()=>{const n=${expression};if(!n||n.disabled)throw Error('Missing enabled export menu control');n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;if(!r.width||!r.height||!n.contains(document.elementFromPoint(x,y)))throw Error('Obstructed export menu control');return{x,y}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
  };
  const menu=async command=>{
    await nativeClick(`[...document.querySelectorAll('.header-menu[data-menu=file] summary,.header-menu-labels-compact summary,.header-menu-overflow summary')].find(n=>n.checkVisibility())`);
    for(let depth=0;depth<3;depth++){
      const found=await evaluate(`(()=>{const rows=[...document.querySelectorAll('.header-menu[open] [role=menu] button')];return rows.some(n=>n.menuItem?.action?.type==='invoke'&&n.menuItem.action.command===${JSON.stringify(command)})})()`);
      if(found){await nativeClick(`[...document.querySelectorAll('.header-menu[open] [role=menu] button')].find(n=>n.menuItem?.action?.type==='invoke'&&n.menuItem.action.command===${JSON.stringify(command)})`);return;}
      await nativeClick(`(()=>{const has=item=>item.action?.type==='invoke'&&item.action.command===${JSON.stringify(command)}||item.sections?.flat().some(has);return [...document.querySelectorAll('.header-menu[open] [role=menu] button')].find(n=>n.menuItem&&has(n.menuItem))})()`);
    }
    assert.fail('Export command absent from the native File menu');
  };
  const checkpoint=()=>evaluate(`JSON.parse(JSON.stringify({file:layerApp.state().document_file,layers:layerApp.state().layers,selected:layerApp.app.document_tabs(0).selected},(_,v)=>typeof v==='bigint'?String(v):v))`);
  const checkExportAgain=async()=>{
    const timings=[];
    for(const width of [640,1100])for(const theme of ['light','dark']){
      await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});await send({type:'set_theme',theme});
      await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await set('Format','Png');
      const normalStart=Date.now();const original=await saved(`again-${width}-${theme}.png`,()=>click('Choose File…'));const normalMs=Date.now()-normalStart;
      const picks=await evaluate('exportMetadata.picks'),master=await evaluate('JSON.stringify(layerApp.state().document_file.location)');
      const cleanRepeatStart=Date.now(),cleanRepeat=await saved(`again-clean-${width}-${theme}.png`,()=>menu('export_again')),cleanRepeatMs=Date.now()-cleanRepeatStart;assert.deepEqual(cleanRepeat,original,'Clean Export Again retains exact recipe and source bytes');
      const beforeEdit=await checkpoint();await send({type:'effect',action:{op:'insert',effect:'invert'}});await idle();const edited=await checkpoint();
      const repeatStart=Date.now();const repeated=await saved(`again-edited-${width}-${theme}.png`,()=>menu('export_again'));timings.push({width,theme,extent:[160,120],normal_ms:normalMs,clean_repeat_ms:cleanRepeatMs,edited_repeat_ms:Date.now()-repeatStart,normal_bytes:original.length,repeat_bytes:repeated.length});
      assert.notDeepEqual(repeated,original,'Export Again writes the edited artwork');
      assert.equal(await evaluate('exportMetadata.picks'),picks,'Export Again reuses the same native target without a picker');
      assert.equal(await evaluate(`!!document.querySelector('dialog[open] [aria-label="Format"]')`),false,'Export Again skips options');
      assert.deepEqual(await checkpoint(),edited,'Export Again leaves master, dirty state and editor history unchanged');
      assert.equal(await evaluate('JSON.stringify(layerApp.state().document_file.location)'),master);
      await writeFile(`${directory}/export-again-${width}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
      if(width===640){
        const aborts=await evaluate('exportMetadata.aborts');await evaluate("exportMetadata.mode='fail';exportMetadata.last=null");await menu('export_again');await idle();
        await wait('!!layerApp.state().host_error');assert.equal(await evaluate('exportMetadata.last'),null);assert.equal(await evaluate('exportMetadata.aborts'),aborts+1);assert.deepEqual(await evaluate('JSON.stringify(layerApp.state().document_file.location)'),master);
        await evaluate("exportMetadata.mode='success'");await saved(`retry-${theme}.png`,()=>menu('export_again'));
        assert.equal(await evaluate('exportMetadata.picks'),picks,'Failed write preserves the repeat target for retry');
        const remembered=await evaluate('layerApp.state().document_file.export_uri');
        await evaluate("exportMetadata.mode='cancel'");await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await click('Choose File…');await idle();
        assert.equal(await evaluate('layerApp.state().document_file.export_uri'),remembered,'Cancelled replacement export preserves the previous repeat target');
        await evaluate("exportMetadata.mode='success'");const previousTarget=await evaluate('exportMetadata.picks');await saved(`after-cancel-${theme}.png`,()=>menu('export_again'));assert.equal(await evaluate('exportMetadata.picks'),previousTarget);
        await invoke('undo');await idle();
        assert.deepEqual((await checkpoint()).layers,beforeEdit.layers,'Export failure does not consume the artwork Undo');
      }else{await invoke('undo');await idle();assert.deepEqual((await checkpoint()).layers,beforeEdit.layers);}
    }
    await open('camera.jpg');const protectedMaster=await checkpoint(),writes=await evaluate('exportMetadata.writes');
    await evaluate("exportMetadata.mode='master'");await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await set('Format','Jpeg');await click('Choose File…');await idle();await wait('!!layerApp.state().host_error');
    assert.equal(await evaluate('exportMetadata.writes'),writes,'Export refuses the master handle before opening a write');assert.deepEqual(await checkpoint(),protectedMaster,'Master protection preserves the document');await evaluate("exportMetadata.mode='success'");
    await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await set('Format','Png');await saved('first-owner.png',()=>click('Choose File…'));
    await writeFile(`${directory}/export-again-timings.json`,JSON.stringify(timings,null,2));
    const owner=await evaluate('String(layerApp.app.document_tabs(0).selected)'),ownerTarget=await evaluate('layerApp.state().document_file.export_uri');
    await open('camera.jpg');
    assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='export_again').enabled"),false,'A different drawing does not inherit the previous export');
    await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await set('Format','Png');await saved('second-owner.png',()=>click('Choose File…'));
    assert.notEqual(await evaluate('layerApp.state().document_file.export_uri'),ownerTarget);
    await evaluate(`layerApp.documents.select(BigInt(${JSON.stringify(owner)}))`);await idle();assert.equal(await evaluate('layerApp.state().document_file.export_uri'),ownerTarget,'Switching tabs restores its own repeat destination');
    const picks=await evaluate('exportMetadata.picks');await saved('first-owner-repeat.png',()=>menu('export_again'));assert.equal(await evaluate('exportMetadata.picks'),picks);
    await evaluate(`exportMetadata.downloads=[];exportMetadata.anchor=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){if(this.download){exportMetadata.downloads.push({name:this.download,bytes:fetch(this.href).then(r=>r.arrayBuffer()).then(b=>Array.from(new Uint8Array(b)))});return;}return exportMetadata.anchor.call(this)};delete window.showSaveFilePicker`);
    try{
      await menu('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Format"]')`);await set('Format','Png');await click('Choose File…');await wait(`!![...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Download')`);await click('Download');await click('File saved');await idle();
      const count=await evaluate('exportMetadata.downloads.length');assert.equal(count,1);
      await menu('export_again');await wait(`!![...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Download')`);await click('Cancel');await idle();assert.equal(await evaluate('exportMetadata.downloads.length'),count,'Cancelled fallback confirms no download');
      await menu('export_again');await wait(`!![...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Download')`);await click('Download');await click('File saved');await idle();assert.equal(await evaluate('exportMetadata.downloads.length'),count+1,'Export Again repeats the browser download confirmation');
      const outputs=await evaluate('Promise.all(exportMetadata.downloads.map(async d=>({name:d.name,bytes:await d.bytes})))');assert.deepEqual(outputs[0],outputs[1],'Unedited fallback exports retain name and exact bytes');
    }finally{await evaluate('HTMLAnchorElement.prototype.click=exportMetadata.anchor;window.showSaveFilePicker=exportMetadata.savePicker');}
  };
  const initialTheme=await evaluate('layerApp.state().settings.theme ?? null');
  await evaluate(`window.exportMetadata={files:new Map(),handles:new Map(),openPicker:window.showOpenFilePicker,savePicker:window.showSaveFilePicker,picks:0,writes:0,aborts:0,mode:'success'};
    exportMetadata.dismiss=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>['Keep for Later','Discard Changes'].includes(b.textContent))?.click(),50);
    window.showOpenFilePicker=async()=>{const name=exportMetadata.openName;const handle={name,async isSameEntry(other){return other===this||other===exportMetadata.handles.get(name)},async getFile(){return new File([exportMetadata.files.get(name)],name)}};exportMetadata.openHandle=handle;return[handle]};
    window.showSaveFilePicker=async options=>{exportMetadata.picks++;if(exportMetadata.mode==='master')return exportMetadata.openHandle;if(exportMetadata.mode==='cancel')throw new DOMException('Cancelled','AbortError');let committed;const handle={name:options.suggestedName,async queryPermission(){return 'granted'},async getFile(){if(!committed)throw new DOMException('Owned file is missing','NotFoundError');return new File([committed],this.name)},async isSameEntry(other){return other===this},async createWritable(){let bytes;exportMetadata.writes++;return{async write(b){if(exportMetadata.mode==='fail')throw Error('Owned export write failure');bytes=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){committed=bytes;exportMetadata.last=bytes;exportMetadata.target=handle},async abort(){exportMetadata.aborts++}}}};return handle};`);
  await evaluate(`(async()=>{
    const canvas=new OffscreenCanvas(160,120),context=canvas.getContext('2d');
    const gradient=context.createLinearGradient(0,0,160,120);gradient.addColorStop(0,'#123456');gradient.addColorStop(1,'#e0c070');
    context.fillStyle=gradient;context.fillRect(0,0,160,120);
    const jpeg=new Uint8Array(await (await canvas.convertToBlob({type:'image/jpeg',quality:0.92})).arrayBuffer());
    const text=s=>[...new TextEncoder().encode(s),0];
    const u16=n=>[n&255,n>>8],u32=n=>[n&255,(n>>8)&255,(n>>16)&255,n>>>24];
    const directories=[
      [[0x010f,2,text('Capycam')],[0x0110,2,text('C-1')],[0x013b,2,text('Ada Painter')],[0x8298,2,text('(c) 2026 Ada Painter')]],
      [[0x829a,5,[...u32(1),...u32(250)]],[0x9003,2,text('2026:09:01 10:00:00')],[0xa434,2,text('Capy 35mm F1.8')]],
      [[0x0001,2,text('N')],[0x0012,2,text('WGS-84')]],
    ];
    const size=entries=>6+12*entries.length+entries.filter(e=>e[2].length>4).reduce((n,e)=>n+e[2].length+e[2].length%2,0);
    const exifAt=8+size(directories[0])+24,gpsAt=exifAt+size(directories[1]);
    directories[0].push([0x8769,4,u32(exifAt)],[0x8825,4,u32(gpsAt)]);
    const tiff=[0x49,0x49,0x2a,0,8,0,0,0];
    for(const entries of directories){
      let dataAt=tiff.length+6+12*entries.length;const data=[];
      tiff.push(...u16(entries.length));
      for(const [tag,kind,value] of entries){
        const width=kind===5?8:kind===4?4:1;
        tiff.push(...u16(tag),...u16(kind),...u32(value.length/width));
        if(value.length<=4)tiff.push(...value,...Array(4-value.length).fill(0));
        else{tiff.push(...u32(dataAt));data.push(...value);if(value.length%2)data.push(0);dataAt+=value.length+value.length%2;}
      }
      tiff.push(...u32(0),...data);
    }
    const xmp='<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/" photoshop:City="Lisbon" exif:GPSLatitude="38,42.5N"><dc:creator><rdf:Seq><rdf:li>Ada Painter</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>';
    const segment=payload=>[0xff,0xe1,(payload.length+2)>>8,(payload.length+2)&255,...payload];
    const exif=segment([...new TextEncoder().encode('Exif'),0,0,...tiff]);
    const packet=segment([...text('http://ns.adobe.com/xap/1.0/'),...new TextEncoder().encode(xmp)]);
    exportMetadata.files.set('camera.jpg',new Uint8Array([...jpeg.slice(0,2),...exif,...packet,...jpeg.slice(2)]));
  })()`);
  const contains=(bytes,text)=>bytes.includes(Buffer.from(text));
  try {
    await open('camera.jpg');
    assert.equal(await evaluate('layerApp.app.export_form().metadata'),true,'the opened photo keeps its metadata');
    for(const [format,name] of [['Jpeg','copy.jpg'],['Webp','copy.webp']]) {
      await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Metadata"]')`);
      await set('Format',format);await settle();
      assert.deepEqual(await evaluate(`[...document.querySelector('dialog[open] [aria-label="Metadata"]').options].map(o=>o.textContent)`),['All','Copyright & Contact','None']);
      assert.equal(await evaluate(`document.querySelector('dialog[open] [aria-label="Metadata"]').value`),'All');
      assert.ok(await shown('Metadata')&&await shown('Remove location'));
      assert.equal(await evaluate(`document.querySelector('dialog[open] [aria-label="Remove location"]').checked`),true,'location is removed by default');
      await set('Metadata','CopyrightContact');
      assert.equal(await shown('Remove location'),false,'Copyright & Contact never keeps a location');
      await set('Metadata','All');
      if(format==='Jpeg'){
        await evaluate(`document.querySelector('dialog[open] [aria-label="Metadata"]').scrollIntoView({block:'center'})`);
        for(const theme of ['light','dark']){
          await send({type:'set_theme',theme});
          await writeFile(`${directory}/metadata-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
        }
        await send({type:'set_theme',theme:initialTheme});
      }
      const bytes=await saved(name,()=>click('Choose File…'));
      for(const kept of ['Capycam','C-1','Capy 35mm F1.8','Ada Painter','(c) 2026 Ada Painter','2026:09:01 10:00:00'])
        assert.ok(contains(bytes,kept),`${name} keeps ${kept}`);
      for(const location of ['WGS-84','Lisbon','GPSLatitude']) assert.ok(!contains(bytes,location),`${name} leaves out ${location}`);
    }
    await saved('camera.capy',()=>invoke('save_document'));
    await open('camera.capy');
    assert.equal(await evaluate('layerApp.app.export_form().metadata'),true,'the saved drawing keeps the photo metadata');
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Metadata"]')`);
    await set('Format','Jpeg');await set('Metadata','None');
    const none=await saved('none.jpg',()=>click('Choose File…'));
    for(const dropped of ['Capycam','Ada Painter']) assert.ok(!contains(none,dropped),`None leaves out ${dropped}`);
    await checkExportAgain();
    console.log('Web export keeps camera, lens and copyright without location in JPEG and WebP; the saved drawing keeps the metadata; None removes it.');
  } finally {
    await evaluate('clearInterval(exportMetadata.dismiss);window.showOpenFilePicker=exportMetadata.openPicker;window.showSaveFilePicker=exportMetadata.savePicker');
    await send({type:'set_theme',theme:initialTheme});
  }
}
