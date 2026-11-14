use super::*;
use std::sync::atomic::{AtomicBool,Ordering};

pub(crate) struct FineInputs {
    pub texture:wgpu::Texture,
    pub view:wgpu::TextureView,
    pub directory:wgpu::Buffer,
    pub capacity:usize,
    columns:u32,
    planes:usize,
    slots:Vec<Option<Page>>,
    desired:std::collections::BTreeSet<[u32;2]>,
    pub pages:std::collections::VecDeque<PixelRect>,
    all_pages:std::collections::VecDeque<PixelRect>,
    pub complete:bool,
    valid:Arc<AtomicBool>,
    allocation:Arc<Allocation>,
}
struct Page {coordinate:[u32;2],valid:Arc<AtomicBool>}
use super::region_batches::Allocation;
impl Drop for FineInputs {
    fn drop(&mut self) {
        if Arc::strong_count(&self.allocation)>1 {self.allocation.retire();}
    }
}
impl FineInputs {
    fn shape(capacity:usize,kept:bool)->[u32;2] {
        let count=capacity as u32*if kept {2} else {1};
        let columns=count.isqrt().max(1);
        [columns,count.div_ceil(columns)]
    }
    pub fn bytes(capacity:usize,kept:bool)->u64 {
        let shape=Self::shape(capacity,kept);
        let entries=(capacity*if kept {2} else {1}).next_power_of_two()*2;
        u64::from(shape[0])*u64::from(shape[1])*u64::from(PAGE_SIZE).pow(2)*16+32+entries as u64*16
    }
    pub fn new(r:&WgpuRasterizer,capacity:usize,kept:bool,coarse_bytes:u64,pages:std::collections::VecDeque<PixelRect>)->Result<Self,GpuRasterError> {
        let shape=Self::shape(capacity,kept);
        let limit=r.device.limits().max_texture_dimension_2d/PAGE_SIZE;
        if capacity==0 || shape.into_iter().any(|n|n>limit) {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        let texture=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("transform fine source pages"),
            size:wgpu::Extent3d {width:shape[0]*PAGE_SIZE,height:shape[1]*PAGE_SIZE,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Float,
            usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::STORAGE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
        let view=texture.create_view(&Default::default());
        let entries=(capacity*if kept {2} else {1}).next_power_of_two()*2;
        let directory=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("transform fine source directory"),
            size:32+entries as u64*16,usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let bytes=texture_bytes(&texture)+directory.size()+coarse_bytes;
        Ok(Self {texture,view,directory,capacity,columns:shape[0],planes:if kept {2} else {1},
            slots:(0..capacity).map(|_|None).collect(),desired:Default::default(),all_pages:pages.clone(),pages,complete:false,
            valid:Arc::new(AtomicBool::new(true)),allocation:Arc::new(Allocation {bytes,retired:r.retired_display_bytes.clone(),pending:Default::default()})})
    }
    pub fn require(&mut self,coordinates:std::collections::BTreeSet<[u32;2]>)->Result<(),GpuRasterError> {
        if coordinates.len()>self.capacity {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        self.desired=coordinates;Ok(())
    }
    pub fn resident(&self,coordinate:[u32;2])->bool {
        self.slots.iter().flatten().any(|page|page.coordinate==coordinate && page.valid.load(Ordering::Acquire))
    }
    pub fn assign(&mut self,coordinate:[u32;2])->Result<usize,GpuRasterError> {
        let index=super::region_batches::slot_for(self.slots.iter().map(|page|page.as_ref().map(|page|page.coordinate)),&self.desired,coordinate)
            .ok_or(GpuRasterError::NotReady)?;
        if self.slots[index].as_ref().is_some_and(|page|page.coordinate==coordinate) {return Ok(index);}
        self.slots[index]=Some(Page {coordinate,valid:Arc::new(AtomicBool::new(false))});
        Ok(index)
    }
    pub fn restart_discarded(&mut self) {
        if !self.valid.swap(true,Ordering::AcqRel) {self.pages=self.all_pages.clone();self.complete=false;}
    }
    pub fn origin(&self,index:usize,plane:usize)->[u32;2] {
        let index=(index*self.planes+plane) as u32;
        [(index%self.columns)*PAGE_SIZE,(index/self.columns)*PAGE_SIZE]
    }
    pub fn storage_bytes(&self)->u64 {texture_bytes(&self.texture)+self.directory.size()}
    pub fn track(&self,encoder:&crate::submission::CommandEncoder) {
        let allocation=self.allocation.clone();encoder.hold(allocation);
    }
    pub fn track_coarse(&self,encoder:&crate::submission::CommandEncoder) {
        crate::submission::CacheWrite::shared(self.valid.clone()).track(encoder);self.track(encoder);
    }
    pub fn written(&self,index:usize,encoder:&crate::submission::CommandEncoder) {
        let page=self.slots[index].as_ref().unwrap();page.valid.store(true,Ordering::Release);
        crate::submission::CacheWrite::shared(page.valid.clone()).track(encoder);
        self.track(encoder);
    }
    pub fn write_directory(&self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,phase:[u32;2])->Result<(),GpuRasterError> {
        if self.desired.iter().any(|c|!self.resident(*c)) {return Err(GpuRasterError::NotReady);}
        let count=(self.directory.size() as usize-32)/16;
        let mut table=vec![[u32::MAX;4];count];
        for (index,page) in self.slots.iter().enumerate().filter_map(|(index,page)|page.as_ref().map(|page|(index,page))) {
            if !page.valid.load(Ordering::Acquire) {continue;}
            for plane in 0..self.planes {
                let key=[page.coordinate[0],page.coordinate[1]|((plane as u32)<<31)];
                let mut slot=key[0].wrapping_mul(1664525).wrapping_add(key[1].wrapping_mul(1013904223)) as usize&(count-1);
                while table[slot][0]!=u32::MAX {slot=(slot+1)&(count-1);}
                let origin=self.origin(index,plane);table[slot]=[key[0],key[1],origin[0],origin[1]];
            }
        }
        let mut bytes=Vec::with_capacity(self.directory.size() as usize);
        for value in [phase[0],phase[1],PAGE_SIZE,(count-1) as u32,0,0,0,0] {bytes.extend_from_slice(&value.to_le_bytes());}
        for entry in table {for value in entry {bytes.extend_from_slice(&value.to_le_bytes());}}
        r.uploads.write(encoder,&self.directory,&bytes)?;self.track(encoder);Ok(())
    }
}
