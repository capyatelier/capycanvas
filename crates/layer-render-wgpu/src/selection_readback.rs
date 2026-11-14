use super::*;
use layer_render::RegionResult;

pub(super) struct Capture {
    pub buffer: wgpu::Buffer,
    pub size: u64,
    pub coverage_size: u64,
    pub underfill_offset: Option<u64>,
    pub seal_records: usize,
    pub seed_offset:Option<u64>,
}
struct Captured {
    translucent_seed:bool,
    colour_on_solid: bool,
    pixels: std::sync::Arc<layer_core::SelectionPixels>,
    underfill: Option<std::sync::Arc<layer_core::SelectionPixels>>,
    active: usize,
    line_bounded: bool,
    stale: bool,
    extra: Vec<u8>,
    seals: Vec<layer_render::RegionSeal>,
    next:Option<[u32;2]>,
}
fn read_coverage(bytes:&[u8],extent:[u32;2],byte_coverage:bool,coverage_size:u64,underfill_offset:Option<u64>,seal_records:usize,seed_offset:Option<u64>)->Result<Captured,GpuRasterError> {
    let read=|offset:usize|u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap());
    let count=extent[0].div_ceil(if byte_coverage {4}else{8}) as usize*extent[1] as usize;
    let copy_words=|offset:usize,count:usize| {
        let source=&bytes[offset..offset+count*4];
        let mut words=std::sync::Arc::<[u32]>::new_uninit_slice(count);
        // SAFETY: the fresh, uniquely owned allocation has count
        // u32 slots. Copy exactly that many initialized bytes;
        // every u32 bit pattern is valid, in native byte order.
        unsafe {
            std::ptr::copy_nonoverlapping(source.as_ptr(),std::sync::Arc::get_mut(&mut words).unwrap().as_mut_ptr().cast::<u8>(),source.len());
            words.assume_init()
        }
    };
    let bounds_at=|offset:usize|if read(offset+16)==0 {[0;4]}else{std::array::from_fn(|i|read(offset+i*4))};
    let bounds=bounds_at(coverage_size as usize);
    let words=copy_words(32,count);
    let pixels=if byte_coverage {layer_core::SelectionPixels::bytes(extent,bounds,words)}else{layer_core::SelectionPixels::new(extent,bounds,words)}
        .map_err(|error|GpuRasterError::MapFailed(error.to_string()))?;
    let band_count=extent[0].div_ceil(8) as usize*extent[1] as usize;
    let underfill=underfill_offset.map(|offset|
        layer_core::SelectionPixels::new(extent,bounds_at(offset as usize-32),copy_words(offset as usize,band_count))
            .map(std::sync::Arc::new).map_err(|error|GpuRasterError::MapFailed(error.to_string()))).transpose()?;
    let extra_start=underfill_offset.map_or(coverage_size as usize+32,|offset|offset as usize+band_count*4);
    let coord=|value:u32|[value&65535,value>>16];
    let seals=(0..seal_records).filter_map(|record| {
        let at=extra_start+record*20;
        (read(at+16)!=0).then(||layer_render::RegionSeal {id:read(at),start:coord(read(at+4)),end:coord(read(at+8)),
            bend:(read(at+12)!=u32::MAX).then(||coord(read(at+12)))})
    }).collect();
    let next=read(coverage_size as usize+24);
    Ok(Captured {translucent_seed:seed_offset.is_some_and(|offset|f32::from_le_bytes(bytes[offset as usize+12..offset as usize+16].try_into().unwrap())<0.99999),colour_on_solid:read(coverage_size as usize+28)&1!=0,next:(next!=u32::MAX).then(||coord(next)),pixels:std::sync::Arc::new(pixels),underfill,active:read(24) as usize,line_bounded:read(28)!=0,
        stale:read(coverage_size as usize+20)!=0,extra:bytes[extra_start+seal_records*20..seed_offset.map_or(bytes.len(),|offset|offset as usize)].to_vec(),seals})
}

pub(super) fn capture_selection<T:Send+'static>(
    device:&crate::PipelineDevice,
    capture:Capture,
    extent:[u32;2],
    byte_coverage:bool,
    request_id:u64,
    gaps:bool,
    tx:mpsc::Sender<Result<T,GpuRasterError>>,
    convert:impl FnOnce(RegionResult,bool,&[u8])->T+Send+'static,
) {
    let Capture {buffer,size,coverage_size,underfill_offset,seal_records,seed_offset}=capture;
    crate::raster::map_then(device,None,&buffer,size,move|bytes|read_coverage(bytes,extent,byte_coverage,coverage_size,underfill_offset,seal_records,seed_offset),move|part| {
        let result=part.and_then(|primary| {
            if gaps && primary.active>=5 {return Err(GpuRasterError::MapFailed("Invalid fill reach".into()));}
            let masks=gaps.then(||layer_render::RegionGapMasks {next:primary.next,active:primary.active,line_bounded:primary.line_bounded,seals:primary.seals.into()});
            Ok(convert(RegionResult {translucent_seed:primary.translucent_seed,colour_on_solid:primary.colour_on_solid,request_id,tonal_sample:None,pixels:primary.pixels,underfill:primary.underfill,
                placement:layer_core::Affine::IDENTITY,gaps:masks},primary.stale,&primary.extra))
        });
        let _=tx.send(result);
    });
}
