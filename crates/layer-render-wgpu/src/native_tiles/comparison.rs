use crate::*;

pub(crate) const RESULT_BYTES:u64=((PAGE_SIZE/8)*(PAGE_SIZE/8)*4) as u64;
pub(crate) const STRIDE:u64=RESULT_BYTES;

pub(super) fn scratch(device:&wgpu::Device)->wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {label:Some("native canonical comparison scratch"),
        size:2*STRIDE,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false})
}

struct Storage {
    buffer:wgpu::Buffer,
    _charge:crate::residency::Charge,
}

#[derive(Clone)]
pub(crate) struct CanonicalComparisons {storage:Arc<Storage>,offset:u64,len:usize}
impl CanonicalComparisons {
    pub fn bytes(count:usize)->Result<u64,GpuRasterError> {
        (count as u64).checked_mul(STRIDE).filter(|_|count!=0).ok_or(GpuRasterError::InvalidExtent)
    }
    pub fn reserved(r:&mut WgpuRasterizer,count:usize,allocation:&mut crate::residency::Reservation)->Result<Self,GpuRasterError> {
        let bytes=Self::bytes(count)?;
        if bytes>r.device.limits().max_buffer_size {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("native capture canonical comparisons"),
            size:bytes,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false});
        let storage=Self {storage:Arc::new(Storage {buffer,_charge:r.raster_buffers.charge(bytes)}),offset:0,len:count};
        drop(allocation.split(bytes).ok_or(GpuRasterError::Invariant("canonical comparisons exceed their working reservation"))?);
        Ok(storage)
    }
    pub fn len(&self)->usize {self.len}
    pub fn buffer(&self)->&wgpu::Buffer {&self.storage.buffer}
    pub fn offset(&self)->u64 {self.offset}
    pub fn window(&self,start:usize,count:usize)->Self {
        assert!(start+count<=self.len);
        Self {storage:self.storage.clone(),offset:self.offset+start as u64*STRIDE,len:count}
    }
    pub fn result(&self,index:usize,name:Arc<layer_core::raster::CanonicalName>)->CanonicalReadback {
        assert!(index<self.len());
        CanonicalReadback {storage:self.clone(),index,name}
    }
}

#[derive(Clone)]
pub(crate) struct CanonicalReadback {
    storage:CanonicalComparisons,
    index:usize,
    name:Arc<layer_core::raster::CanonicalName>,
}
impl CanonicalReadback {
    pub fn copy(&self,encoder:&mut wgpu::CommandEncoder,buffer:&wgpu::Buffer,offset:u64) {
        encoder.copy_buffer_to_buffer(self.storage.buffer(),self.storage.offset()+self.index as u64*STRIDE,buffer,offset,RESULT_BYTES);
    }
    pub fn resolve(&self,bytes:&[u8])->Result<(),String> {
        if bytes.len()!=RESULT_BYTES as usize {return Err("Invalid canonical comparison size".into());}
        let mut changed=false;
        for word in bytes.chunks_exact(4) {
            match u32::from_le_bytes(word.try_into().unwrap()) {0=>{},1=>changed=true,_=>return Err("Invalid canonical comparison result".into()),}
        }
        self.name.resolve(changed).map_err(str::to_owned)
    }
}
