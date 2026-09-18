import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';

// Independent browser decoder qualification on an SDR compositor. This does
// not claim physical HDR display qualification or browser-app HDR editing.
export async function checkGainmapInterchange({evaluate}) {
  const directory=new URL('../../artifacts/color-m4/gainmap-interchange/',import.meta.url);
  const results=[];
  const control=await readFile(new URL('jpeg-control.jpg',directory));
  const gpuControl=await evaluate(`(async()=>{const image=new Image();image.src=${JSON.stringify(`data:image/jpeg;base64,${control.toString('base64')}`)};await image.decode();const canvas=document.createElement('canvas');canvas.width=32;canvas.height=24;const c=canvas.getContext('2d');c.drawImage(image,0,0);return [...c.getImageData(16,12,1,1).data];})()`);
  results.push({file:'jpeg-control.jpg',acceleratedCanvas:gpuControl});

  for(const format of ['jpeg','avif'])for(const rendition of ['neutral','dark']) {
    const stem=`${format}-${rendition}`, extension=format==='jpeg'?'jpg':'avif';
    const file=await readFile(new URL(`${stem}.${extension}`,directory));
    const reference=JSON.parse(await readFile(new URL(`${stem}.json`,directory),'utf8'));
    const result=await evaluate(`(async()=>{
      const image=new Image();image.src=${JSON.stringify(`data:image/${format};base64,${file.toString('base64')}`)};await image.decode();
      const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;
      const c=canvas.getContext('2d',{colorSpace:'srgb',willReadFrequently:true});c.drawImage(image,0,0);
      return {extent:[image.width,image.height],pixel:[...c.getImageData(16,12,1,1).data],browser:navigator.userAgent,hdr:matchMedia('(dynamic-range: high)').matches};
    })()`);
    assert.deepEqual(result.extent,[32,24]);
    assert.equal(result.hdr,false,'This test qualifies the authored SDR fallback on an SDR display');
    for(let c=0;c<3;c++)assert.ok(Math.abs(result.pixel[c]-reference[c])<=2,`${stem}: browser ${result.pixel} vs encoded SDR ${reference}`);
    assert.equal(result.pixel[3],255);results.push({file:`${stem}.${extension}`,reference,...result});
  }
  const color=await readFile(new URL('../../artifacts/color-m4/gainmap-ui/Opaque edited HDR.jpg',import.meta.url));
  const legacy=JSON.parse(await readFile(new URL('legacy-sdr-reference.json',directory),'utf8'));
  const colorPixel=await evaluate(`(async()=>{const image=new Image();image.src=${JSON.stringify(`data:image/jpeg;base64,${color.toString('base64')}`)};await image.decode();const canvas=document.createElement('canvas');canvas.width=64;canvas.height=48;const c=canvas.getContext('2d',{willReadFrequently:true});c.drawImage(image,0,0);return [...c.getImageData(32,24,1,1).data];})()`);
  for(let c=0;c<3;c++)assert.ok(Math.abs(colorPixel[c]-legacy.pixel[c])<=2,`Chrome vs independent LCMS SDR color: ${colorPixel} ${legacy.pixel}`);
  results.push({file:'Opaque edited HDR.jpg',pixel:colorPixel,legacy});
  const alpha=await readFile(new URL('../../artifacts/color-m4/gainmap-ui/Transparent edited HDR.avif',import.meta.url));
  const pixel=await evaluate(`(async()=>{const image=new Image();image.src=${JSON.stringify(`data:image/avif;base64,${alpha.toString('base64')}`)};await image.decode();const canvas=document.createElement('canvas');canvas.width=64;canvas.height=48;const c=canvas.getContext('2d',{willReadFrequently:true});c.drawImage(image,0,0);return [...c.getImageData(32,24,1,1).data];})()`);
  assert.ok(Math.abs(pixel[3]-255*32/63)<1.1,`independent AVIF alpha: ${pixel}`);
  results.push({file:'Transparent edited HDR.avif',pixel});
  await writeFile(new URL('browser-sdr-results.json',directory),JSON.stringify(results,null,2)+'\n');
  console.log('Gain-map JPEG and AVIF: independent Chrome SDR bases match; AVIF alpha preserved. Physical HDR unqualified.');
}
