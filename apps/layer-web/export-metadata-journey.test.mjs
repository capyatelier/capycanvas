import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Open a camera JPEG, then export JPEG and WebP through the real dialog and
// output worker: camera, lens, dates and copyright stay, the location goes.
// Only the OS picker handles are supplied by the harness.
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
    await evaluate(`exportMetadata.files.set(${JSON.stringify(name)},exportMetadata.last.slice())`);
    const bytes=Buffer.from(await evaluate('Array.from(exportMetadata.last)'));
    await writeFile(`${directory}/${name}`,bytes);
    return bytes;
  };
  const initialTheme=await evaluate('layerApp.state().settings.theme ?? null');
  await evaluate(`window.exportMetadata={files:new Map()};
    exportMetadata.dismiss=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>['Keep for Later','Discard Changes'].includes(b.textContent))?.click(),50);
    window.showOpenFilePicker=async()=>[{name:exportMetadata.openName,async getFile(){return new File([exportMetadata.files.get(exportMetadata.openName)],exportMetadata.openName)}}];
    window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(b){bytes=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){exportMetadata.last=bytes},async abort(){}}}});`);
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
    console.log('Web export keeps camera, lens and copyright without location in JPEG and WebP; the saved drawing keeps the metadata; None removes it.');
  } finally {
    await evaluate('clearInterval(exportMetadata.dismiss)');
    await send({type:'set_theme',theme:initialTheme});
  }
}
