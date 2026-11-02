use super::*;

pub(super) const RADIUS:u32=crate::flood::UNDERFILL_HALF_PIXELS.div_ceil(2);
const HALO:u32=RADIUS;
const SIDE:u32=PAGE_SIZE+2*HALO;
pub(super) struct Capture {
    source:artwork::Capture,
    color:(wgpu::Texture,wgpu::TextureView),
    lines:(wgpu::Texture,wgpu::TextureView),
    cover:(wgpu::Texture,wgpu::TextureView),
    next:(wgpu::Texture,wgpu::TextureView),
    seed:(wgpu::Texture,wgpu::TextureView),
    line_seeds:wgpu::Texture,
    mask:wgpu::Buffer,
    precision:Option<Precision>,
    anchors:wgpu::Buffer,
    top:wgpu::Texture,
    left:wgpu::Texture,
    row:Option<u32>,
    previous:Option<[u32;2]>,
    top_columns:std::collections::BTreeSet<u32>,
    columns:std::collections::BTreeSet<u32>,
}
pub(super) struct Precision {params:wgpu::Buffer}
impl Precision {
    fn new(r:&WgpuRasterizer,input:Option<(&layer_core::FillEdges,SourceTarget)>)->Self {
        use wgpu::util::DeviceExt;
        let working=r.device.working_space();
        let (depth,space)=input.map(|(edges,target)| {
            let scene=edges.snapshot.view();
            if let Some(base)=scene.paint_base(target)
                && let layer_core::color::ColorProfile::Builtin(space)=base.image.interpretation.profile {
                (base.image.interpretation.depth,space)
            }else{(scene.composition().color.depth,scene.composition().color.space)}
        }).unwrap_or((layer_core::color::SampleDepth::F32,working));
        let transform=working.linear_transform(space);
        let mut bytes=Vec::with_capacity(64);
        for column in [0,1,2].map(|column|[transform[0][column] as f32,transform[1][column] as f32,transform[2][column] as f32,0.]) {
            for value in column {bytes.extend(value.to_ne_bytes());}
        }
        let half=r.device.working_format()==wgpu::TextureFormat::Rgba16Float;
        for value in [if depth.is_float(){0}else{depth.maximum()},space.shader_code(),if half{10}else{23},if half{24}else{149}] {bytes.extend(value.to_ne_bytes());}
        let extent=input.and_then(|(edges,target)|edges.anchors.as_ref().map(|anchors| {
            let extent=anchors.extent;let offset=edges.snapshot.view().target_offset(target);
            [extent[0] as i32,extent[1] as i32,offset[0] as i32,offset[1] as i32]
        })).unwrap_or([0;4]);
        for value in extent {bytes.extend(value.to_ne_bytes());}
        let params=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill source precision"),contents:&bytes,usage:wgpu::BufferUsages::UNIFORM});
        Self {params}
    }
}
pub(super) fn layout(device:&PipelineDevice)->wgpu::BindGroupLayout {
    crate::bindings::layout(device,"fill edge inputs",&[
        crate::bindings::texture(0,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::buffer(1,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Storage {read_only:true},false,None),
        crate::bindings::texture(2,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::texture(3,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::buffer(4,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Uniform,false,None),
        crate::bindings::buffer(5,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Storage {read_only:true},false,None),
    ])
}
pub(super) fn binding(r:&WgpuRasterizer,color:&wgpu::TextureView,lines:&wgpu::TextureView,cover:&wgpu::TextureView,mask:&wgpu::Buffer,anchors:&wgpu::Buffer,precision:Option<&Precision>)->wgpu::BindGroup {
    let empty=precision.is_none().then(||Precision::new(r,None));let precision=precision.or(empty.as_ref()).unwrap();
    crate::bindings::group(&r.device,"fill edge inputs",&r.scene_pipelines.edges,[
        wgpu::BindingResource::TextureView(color),mask.as_entire_binding(),wgpu::BindingResource::TextureView(lines),wgpu::BindingResource::TextureView(cover),
        precision.params.as_entire_binding(),anchors.as_entire_binding(),
    ])
}
pub(super) fn shader()->String {
    let selection=include_str!("../selection_clip.wgsl");
    compose_wgsl(&[
        &selection.replace("@group(1) @binding(1)","@group(2) @binding(1)").replace("brush_selection","fill_selection"),
        include_str!("../fill_unmix.wgsl"),
        include_str!("../fill_edges.wgsl"),
    ]).into_owned()
}
impl Capture {
    fn new(r:&WgpuRasterizer,extent:[u32;2],lines:usize)->Self {
        Self {source:Default::default(),color:create_color_target(&r.device,[SIDE,SIDE+1],"fill source halo"),
            lines:create_color_target(&r.device,[SIDE;2],"fill line alpha"),next:create_color_target(&r.device,[SIDE,SIDE+1],"fill line source"),
            cover:create_color_target(&r.device,[SIDE;2],"fill lower line alpha"),
            seed:create_color_target(&r.device,[1;2],"fill seed"),mask:r.unclipped.clone(),precision:None,anchors:r.unclipped.clone(),
            line_seeds:create_color_target(&r.device,[1,lines.max(1) as u32],"fill line seeds").0,
            top:create_color_target(&r.device,[extent[0],HALO],"fill original row border").0,
            left:create_color_target(&r.device,[HALO,SIDE],"fill original page border").0,
            row:None,previous:None,top_columns:Default::default(),columns:Default::default()}
    }
    fn preserve(&mut self,coordinate:[u32;2],crop:PixelRect,extent:[u32;2],encoder:&mut submission::CommandEncoder) {
        if self.row!=Some(coordinate[1]) {
            self.top_columns=if self.row.is_some_and(|row|row+1==coordinate[1]) {std::mem::take(&mut self.columns)}else {Default::default()};
            self.columns.clear();self.previous=None;self.row=Some(coordinate[1]);
        }
        let core=page_rect(coordinate).intersect(PixelRect::full(extent));
        if crop.min_y()<core.min_y() {
            for column in crop.min_x()/PAGE_SIZE..crop.max_x().div_ceil(PAGE_SIZE) {
                if !self.top_columns.contains(&column) {continue;}
                let left=crop.min_x().max(column*PAGE_SIZE);let right=crop.max_x().min((column+1)*PAGE_SIZE);
                copy(encoder,&self.top,&self.color.0,[left,0],[left-crop.min_x(),0],[right-left,core.min_y()-crop.min_y()]);
            }
        }
        if self.previous.is_some_and(|previous|previous[1]==coordinate[1] && previous[0]+1==coordinate[0]) {
            copy(encoder,&self.left,&self.color.0,[0;2],[0;2],[core.min_x()-crop.min_x(),crop.height()]);
        }
        if core.max_x()<extent[0] {
            copy(encoder,&self.color.0,&self.left,[core.max_x()-HALO-crop.min_x(),0],[0;2],[HALO,crop.height()]);
        }
        if core.max_y()<extent[1] {
            copy(encoder,&self.color.0,&self.top,[core.min_x()-crop.min_x(),core.max_y()-HALO-crop.min_y()],[core.min_x(),0],[core.width(),HALO]);
        }
        self.previous=Some(coordinate);self.columns.insert(coordinate[0]);
    }
    fn packet<'a>(snapshot:&'a layer_core::SceneSnapshot,scope:&'a SceneScope,offset:[i64;2],extent:[u32;2],packet:FramePacket<'a>)->FramePacket<'a> {
        let scene=snapshot.view().with_scope(scope).with_offset64(offset.map(|v|-(v as f64)));
        FramePacket {scene,document_extent:extent,selection_overlays:None,inspect_mask:None,dabs:&[],dab_batches:&[],restore_rasters:&[],
            reset_layers:false,commit_rasters:false,composite_all:false,time_seconds:snapshot.context.elapsed,blend_space:scene.composition().blend,..packet}
    }
}
fn copy(encoder:&mut submission::CommandEncoder,source:&wgpu::Texture,destination:&wgpu::Texture,from:[u32;2],to:[u32;2],size:[u32;2]) {
    encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:from[0],y:from[1],z:0},..source.as_image_copy()},
        wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:to[0],y:to[1],z:0},..destination.as_image_copy()},
        wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1});
}
fn coverage(r:&mut WgpuRasterizer,selection:Option<&layer_core::Selection>,extent:[u32;2],encoder:&mut submission::CommandEncoder)->Result<wgpu::Buffer,GpuRasterError> {
    let Some(selection)=selection else{return Ok(r.unclipped.clone());};
    if selection.affine==layer_core::Affine::IDENTITY && !selection.inverted
        && let layer_core::SelectionShape::Pixels(pixels)=&selection.shape {return Ok(r.selection_clip.pixel_buffer(&r.device,pixels));}
    r.selection_clip.prepare(&r.device,encoder,extent,&Arc::new(selection.clone()))?;
    let source=r.selection_clip.buffer.as_ref().unwrap();
    let copy=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill edge coverage"),size:source.size(),usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    encoder.copy_buffer_to_buffer(source,0,&copy,0,source.size());Ok(copy)
}
fn band_intersects(operation:&layer_core::RasterOperation,coordinate:[u32;2],extent:[u32;2],halo:u32)->bool {
    let RasterOperationKind::Fill {underfill:Some(selection),..}=&operation.kind else {return false;};
    let region=page_rect(coordinate).expand(halo,extent);
    if !selection.inverted && selection.affine.0[..4]==[1.,0.,0.,1.]
        && let layer_core::SelectionShape::Pixels(pixels)=&selection.shape {
        let offset=[selection.affine.0[4],selection.affine.0[5]];
        if offset.iter().all(|value|value.fract()==0.) {
            let bounds=DocRect::from(region).translated(offset.map(|value|-(value as i64))).in_frame(pixels.extent());
            return pixels.coverage_bounds_in([bounds.min_x(),bounds.min_y(),bounds.max_x(),bounds.max_y()])!=[0;4];
        }
    }
    !region.intersect(pixel_rect(selection.bounds(),extent)).is_empty()
}
pub(crate) fn read_bounds(operation:&layer_core::RasterOperation,bounds:PixelRect,extent:[u32;2])->PixelRect {
    if matches!(operation.kind,RasterOperationKind::Fill {edges:Some(_),underfill:Some(_),..}) {bounds.expand(HALO,extent)}else{bounds}
}
impl Scene {
    pub(super) fn prepare_fill_edges(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,edges:&layer_core::FillEdges,operation:&layer_core::RasterOperation,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let extent=packet.scene.target_extent(target);let offset=packet.scene.target_offset(target);
        let mut capture=self.fill_capture.take().unwrap_or_else(||Box::new(Capture::new(r,extent,edges.lines.len())));
        let scope=SceneScope::Raw(target);let source=Capture::packet(&edges.snapshot,&scope,offset,extent,packet);
        let [x,y]=[edges.position.x,edges.position.y].map(|v|v.max(0.) as u32);
        let region=PixelRect::new(x,y,x+1,y+1).intersect(PixelRect::full(extent));
        let result=(|| {
            for (index,line) in edges.lines.iter().enumerate() {
                capture.source.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),region,&capture.seed.0,encoder)?;
                copy(encoder,&capture.seed.0,&capture.line_seeds,[0;2],[0,index as u32],[1;2]);
            }
            capture.source.region_into(r,source,region,&capture.seed.0,encoder)?;
            capture.mask=coverage(r,operation.coverage.selection.as_ref(),extent,encoder)?;
            capture.precision=Some(Precision::new(r,Some((edges,target))));
            Ok(())
        })();
        self.fill_capture=Some(capture);result
    }
    pub(super) fn fill_edge_page(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,operation:&layer_core::RasterOperation,coordinate:[u32;2],encoder:&mut submission::CommandEncoder)->Result<Option<(wgpu::BindGroup,[f32;4])>,GpuRasterError> {
        let RasterOperationKind::Fill {edges:Some(edges),..}=&operation.kind else {unreachable!();};
        let extent=packet.scene.target_extent(target);let offset=packet.scene.target_offset(target);
        if !band_intersects(operation,coordinate,extent,HALO) {return Ok(None);}
        self.encode_jobs(r,encoder)?;
        let mut capture=self.fill_capture.take().unwrap();
        let crop=page_rect(coordinate).expand(HALO,extent);
        let result=(|| {
            let scope=SceneScope::Raw(target);let source=Capture::packet(&edges.snapshot,&scope,offset,extent,packet);
            capture.source.region_into(r,source,crop,&capture.color.0,encoder)?;
            capture.preserve(coordinate,crop,extent,encoder);
            if !band_intersects(operation,coordinate,extent,0) {return Ok(None);}
            encoder.copy_texture_to_texture(capture.seed.0.as_image_copy(),wgpu::TexelCopyTextureInfo {texture:&capture.color.0,
                origin:wgpu::Origin3d {x:0,y:SIDE,z:0},mip_level:0,aspect:wgpu::TextureAspect::All},wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1});
            self.jobs.push(Job::Clear(capture.lines.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
            self.jobs.push(Job::Clear(capture.cover.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
            for (index,line) in edges.lines.iter().enumerate() {
                self.jobs.push(Job::Clear(capture.next.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
                capture.source.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),crop,&capture.next.0,encoder)?;
                copy(encoder,&capture.line_seeds,&capture.next.0,[0,index as u32],[0,SIDE],[1;2]);
                let mut data=[0.;32];data[..8].copy_from_slice(&[0.,0.,SIDE as f32,SIDE as f32,SIDE as f32,SIDE as f32,0.,0.]);
                data[8..12].copy_from_slice(&[23.,1.,1.,0.]);
                self.jobs.push(Job::Draw {target:capture.lines.1.clone(),sources:[capture.next.1.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
                    data,over:true,clip:None,source_target:None,edges:None});self.encode_jobs(r,encoder)?;
                if line.fill_above {
                    self.jobs.push(Job::Draw {target:capture.cover.1.clone(),sources:[capture.next.1.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
                        data,over:true,clip:None,source_target:None,edges:None});self.encode_jobs(r,encoder)?;
                }
            }
            use wgpu::util::DeviceExt;
            let core=page_rect(coordinate).intersect(PixelRect::full(extent));
            let origin=[i64::from(core.min_x())+offset[0],i64::from(core.min_y())+offset[1]];
            let mode=edges.anchors.as_ref().is_some_and(|anchors|anchors.windows.first().is_some_and(|window|window.words[0]>>31!=0));
            let mut values=vec![u32::MAX;8+core.width() as usize*core.height() as usize];
            values[..8].copy_from_slice(&[origin[0] as u32,origin[1] as u32,core.width(),core.height(),u32::from(mode),0,0,0]);
            if let Some(anchors)=&edges.anchors {
                for window in anchors.windows.iter().filter(|window| {
                    let [x,y,w,h]=window.bounds;
                    i64::from(x)<origin[0]+i64::from(core.width()) && i64::from(y)<origin[1]+i64::from(core.height())
                        && i64::from(x+w)>origin[0] && i64::from(y+h)>origin[1]
                }) {
                    let [x,y,w,h]=window.bounds;
                    for gy in i64::from(y).max(origin[1])..i64::from(y+h).min(origin[1]+i64::from(core.height())) {
                        for gx in i64::from(x).max(origin[0])..i64::from(x+w).min(origin[0]+i64::from(core.width())) {
                            if let Some(q)=window.get([gx as u32,gy as u32]) {
                                values[8+((gy-origin[1])*i64::from(core.width())+gx-origin[0]) as usize]=q[0]|q[1]<<16;
                            }
                        }
                    }
                }
            }
            let bytes=values.into_iter().flat_map(u32::to_ne_bytes).collect::<Vec<_>>();
            capture.anchors=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill page anchors"),contents:&bytes,usage:wgpu::BufferUsages::STORAGE});
            Ok(Some((binding(r,&capture.color.1,&capture.lines.1,&capture.cover.1,&capture.mask,&capture.anchors,capture.precision.as_ref()),[crop.min_x() as f32,crop.min_y() as f32,crop.width() as f32,crop.height() as f32])))
        })();
        self.fill_capture=Some(capture);result
    }
}
