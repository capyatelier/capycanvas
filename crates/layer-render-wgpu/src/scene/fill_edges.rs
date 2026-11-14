use super::*;

pub(super) const RADIUS:u32=crate::flood::UNDERFILL_HALF_PIXELS.div_ceil(2);
pub(crate) fn seed_rect(position:layer_core::Point,extent:[u32;2])->PixelRect {
    let [x,y]=[position.x,position.y].map(|value|value.max(0.) as u32);
    PixelRect::new(x,y,x+1,y+1).intersect(PixelRect::full(extent))
}
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
    external:Vec<bool>,
    mask:wgpu::Buffer,
    precision:Option<Precision>,
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
        let mut bytes=Vec::with_capacity(96);
        for column in [0,1,2].map(|column|[transform[0][column] as f32,transform[1][column] as f32,transform[2][column] as f32,0.]) {
            for value in column {bytes.extend(value.to_ne_bytes());}
        }
        let half=r.device.working_format()==wgpu::TextureFormat::Rgba16Float;
        for value in [if depth.is_float(){0}else{depth.maximum()},space.shader_code(),if half{10}else{23},if half{24}else{149}] {bytes.extend(value.to_ne_bytes());}
        let extent=input.map(|(edges,target)| {
            let scene=edges.snapshot.view();let size=scene.composition().size;let offset=scene.target_offset(target);
            [size[0] as i32,size[1] as i32,offset[0] as i32,offset[1] as i32]
        }).unwrap_or([0;4]);
        for value in extent {bytes.extend(value.to_ne_bytes());}
        for value in [u32::from(input.is_some_and(|(edges,_)|edges.colour_on_solid)),0,0,0] {bytes.extend(value.to_ne_bytes());}
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
    ])
}
pub(super) fn binding(r:&WgpuRasterizer,color:&wgpu::TextureView,lines:&wgpu::TextureView,cover:&wgpu::TextureView,mask:&wgpu::Buffer,precision:Option<&Precision>)->wgpu::BindGroup {
    let empty=precision.is_none().then(||Precision::new(r,None));let precision=precision.or(empty.as_ref()).unwrap();
    crate::bindings::group(&r.device,"fill edge inputs",&r.scene_pipelines.edges,[
        wgpu::BindingResource::TextureView(color),mask.as_entire_binding(),wgpu::BindingResource::TextureView(lines),wgpu::BindingResource::TextureView(cover),
        precision.params.as_entire_binding(),
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
            lines:create_target(&r.device,[SIDE;2],r.device.scalar_format(),"fill line alpha"),next:create_color_target(&r.device,[SIDE,SIDE+1],"fill line source"),
            cover:create_target(&r.device,[SIDE;2],r.device.scalar_format(),"fill lower line alpha"),
            seed:create_color_target(&r.device,[1;2],"fill seed"),mask:r.unclipped.clone(),precision:None,
            line_seeds:create_color_target(&r.device,[1,lines.max(1) as u32],"fill line seeds").0,external:Vec::new(),
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
fn external(edges:&layer_core::FillEdges,packet:FramePacket<'_>,preview:Option<&layer_render::FillPreview>)->Vec<bool> {
    let edited:Vec<_>=packet.dab_batches.iter().map(|batch|batch.target).chain(preview.map(|fill|fill.target)).collect();
    edges.lines.iter().map(|line|!crate::snapshot::capture_targets(line.snapshot.view().with_scope(&line.scope),&line.scope).iter()
        .any(|target|edited.contains(target))).collect()
}
fn same_edges(a:&layer_core::FillEdges,b:&layer_core::FillEdges)->bool {
    Arc::ptr_eq(&a.snapshot,&b.snapshot) && a.colour_on_solid==b.colour_on_solid && a.translucent_seed==b.translucent_seed && a.position==b.position && a.tolerance==b.tolerance
        && a.lines.len()==b.lines.len() && a.lines.iter().zip(&b.lines).all(|(a,b)|
            Arc::ptr_eq(&a.snapshot,&b.snapshot) && a.scope==b.scope && a.fill_above==b.fill_above)
}
pub(crate) fn read_bounds(operation:&layer_core::RasterOperation,bounds:PixelRect,extent:[u32;2])->PixelRect {
    let RasterOperationKind::Fill {edges:Some(edges),underfill:Some(_),..}=&operation.kind else {return bounds;};
    DocRect::from(bounds).aligned(PAGE_SIZE).in_frame(extent).expand(HALO,extent).union(seed_rect(edges.position,extent))
}

type Texture=(wgpu::Texture,wgpu::TextureView);
struct LineCapture<'a> {
    source:&'a mut artwork::Capture,next:&'a Texture,seeds:&'a wgpu::Texture,
}
impl LineCapture<'_> {
    fn capture(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,index:usize,crop:PixelRect,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        self.source.region_into(r,packet,crop,&self.next.0,encoder)?;
        copy(encoder,&self.seeds,&self.next.0,[0,index as u32],[0,SIDE],[1;2]);
        Ok(())
    }
    fn draw(scene:&mut Scene,r:&WgpuRasterizer,next:&Texture,target:&wgpu::TextureView,crop:PixelRect) {
        let [w,h]=[crop.width(),crop.height()].map(|v|v as f32);
        let mut data=[0.;32];data[..8].copy_from_slice(&[0.,0.,w,h,SIDE as f32,SIDE as f32,0.,0.]);
        data[8..12].copy_from_slice(&[23.,1.,1.,0.]);
        scene.jobs.push(Job::Draw {target:target.clone(),sources:[next.1.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
            data,over:true,clip:None,source_target:None,edges:None,pipeline:None});
    }
}

impl Scene {
    fn fill_target(&mut self,r:&mut WgpuRasterizer,target:SourceTarget,region:PixelRect,texture:&Texture,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let mut tiles=Vec::new();let mut leases=Vec::new();
        for coordinate in page_coordinates(region) {
            let tile=r.raw_layer_tile(target,coordinate,encoder)?;
            if let Some(tile)=&tile {leases.extend(r.source_tiles.borrow().lease(&tile.view));}
            tiles.push((coordinate,tile));
        }
        if tiles.is_empty() || tiles.iter().any(|(_,tile)|tile.is_none()) {
            self.jobs.push(Job::Clear(texture.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
        }
        for (coordinate,tile) in tiles {
            let Some(tile)=tile else {continue;};let overlap=page_rect(coordinate).intersect(region);
            encoder.copy_texture_to_texture(tile.texture.copy_at([overlap.min_x()-coordinate[0]*PAGE_SIZE,overlap.min_y()-coordinate[1]*PAGE_SIZE]),
                wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:overlap.min_x()-region.min_x(),y:overlap.min_y()-region.min_y(),z:0},..texture.0.as_image_copy()},
                wgpu::Extent3d {width:overlap.width(),height:overlap.height(),depth_or_array_layers:1});
        }
        Ok(())
    }
    pub(super) fn prepare_fill_edges(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,edges:&layer_core::FillEdges,operation:&layer_core::RasterOperation,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let extent=packet.scene.target_extent(target);let offset=packet.scene.target_offset(target);
        let mut capture=self.fill_capture.take().unwrap_or_else(||Box::new(Capture::new(r,extent,edges.lines.len())));
        capture.external=external(edges,packet,r.fill_preview.as_ref());
        let prepared=r.fill_edges_preflight.as_ref().and_then(|state|state.input(target,edges,&capture.external,extent,offset).map(|input|state.line_seeds[input].clone()));
        let region=seed_rect(edges.position,extent);
        let result=(|| {
            for (index,line) in edges.lines.iter().enumerate() {
                if capture.external[index] && let Some(prepared)=&prepared {
                    copy(encoder,prepared,&capture.line_seeds,[0,index as u32],[0,index as u32],[1;2]);
                }else {
                    capture.source.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),region,&capture.seed.0,encoder)?;
                    copy(encoder,&capture.seed.0,&capture.line_seeds,[0;2],[0,index as u32],[1;2]);
                }
            }
            self.fill_target(r,target,region,&capture.seed,encoder)?;
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
            self.fill_target(r,target,crop,&capture.color,encoder)?;
            capture.preserve(coordinate,crop,extent,encoder);
            if !band_intersects(operation,coordinate,extent,0) {return Ok(None);}
            encoder.copy_texture_to_texture(capture.seed.0.as_image_copy(),wgpu::TexelCopyTextureInfo {texture:&capture.color.0,
                origin:wgpu::Origin3d {x:0,y:SIDE,z:0},mip_level:0,aspect:wgpu::TextureAspect::All},wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1});
            let prepared=r.fill_edges_preflight.as_ref().and_then(|state|state.input(target,edges,&capture.external,extent,offset)
                .and_then(|input|state.pages.iter().find(|(key,_,_)|*key==(input,coordinate))))
                .map(|(_,lines,cover)|(lines.clone(),cover.clone()));
            let (lines,cover)=if let Some((lines,cover))=&prepared && capture.external.iter().all(|v|*v) {(&lines.1,&cover.1)}else {
                if let Some((lines,cover))=&prepared {
                    copy(encoder,&lines.0,&capture.lines.0,[0;2],[0;2],[SIDE;2]);copy(encoder,&cover.0,&capture.cover.0,[0;2],[0;2],[SIDE;2]);
                }else {
                    self.jobs.push(Job::Clear(capture.lines.1.clone(),wgpu::Color::TRANSPARENT));
                    self.jobs.push(Job::Clear(capture.cover.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
                }
                for (index,line) in edges.lines.iter().enumerate() {
                    if prepared.is_some() && capture.external[index] {continue;}
                    LineCapture {source:&mut capture.source,next:&capture.next,seeds:&capture.line_seeds}
                        .capture(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),index,crop,encoder)?;
                    LineCapture::draw(self,r,&capture.next,&capture.lines.1,crop);self.encode_jobs(r,encoder)?;
                    if line.fill_above {LineCapture::draw(self,r,&capture.next,&capture.cover.1,crop);self.encode_jobs(r,encoder)?;}
                }
                (&capture.lines.1,&capture.cover.1)
            };
            Ok(Some((binding(r,&capture.color.1,lines,cover,&capture.mask,capture.precision.as_ref()),[crop.min_x() as f32,crop.min_y() as f32,crop.width() as f32,crop.height() as f32])))
        })();
        self.fill_capture=Some(capture);result
    }
}

struct Input {
    target:SourceTarget,operation:layer_core::RasterOperation,external:Vec<bool>,extent:[u32;2],offset:[i64;2],
}
enum Step {Seed(usize),Page(usize,[u32;2])}
pub(crate) struct FillEdgesPreflight {
    owner:u64,color:layer_core::color::DocumentColor,inputs:Vec<Input>,steps:Vec<Step>,next:usize,
    capture:artwork::Capture,scene:Option<Box<Scene>>,seed:Texture,scratch:Vec<Texture>,zero:Texture,line_seeds:Vec<wgpu::Texture>,
    pages:Vec<((usize,[u32;2]),Texture,Texture)>,failure:Option<GpuRasterError>,
}
impl FillEdgesPreflight {
    pub fn matches(&self,packet:FramePacket<'_>,color:layer_core::color::DocumentColor)->bool {
        self.owner==packet.scene.owner() && self.color==color
    }
    pub fn pending(&self)->bool {self.failure.is_none() && self.next<self.steps.len()}
    fn input(&self,target:SourceTarget,edges:&layer_core::FillEdges,external:&[bool],extent:[u32;2],offset:[i64;2])->Option<usize> {
        self.inputs.iter().position(|input|input.target==target && input.external==external && input.extent==extent && input.offset==offset
            && matches!(&input.operation.kind,RasterOperationKind::Fill {edges:Some(stored),..} if same_edges(stored,edges)))
    }
    fn advance(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        if self.scene.is_none() {self.scene=Some(Scene::new(r)?);}
        while let Some(step)=self.steps.get(self.next) {
            let (Step::Seed(input)|Step::Page(input,_))=*step;
            let input=&self.inputs[input];let extent=input.extent;let offset=input.offset;
            let RasterOperationKind::Fill {edges:Some(edges),..}=&input.operation.kind else {unreachable!();};
            let scene=self.scene.as_mut().unwrap();
            match *step {
                Step::Seed(id)=> {
                    let region=seed_rect(edges.position,extent);
                    for (index,line) in edges.lines.iter().enumerate().filter(|(index,_)|input.external[*index]) {
                        self.capture.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),region,&self.seed.0,encoder)?;
                        copy(encoder,&self.seed.0,&self.line_seeds[id],[0;2],[0,index as u32],[1;2]);
                    }
                }
                Step::Page(id,coordinate)=> {
                    let lines=create_target(&r.device,[SIDE;2],r.device.scalar_format(),"prepared fill lines");
                    let cover=if edges.lines.iter().enumerate().any(|(index,line)|input.external[index] && line.fill_above) {
                        create_target(&r.device,[SIDE;2],r.device.scalar_format(),"prepared fill cover")
                    }else{self.zero.clone()};
                    let crop=page_rect(coordinate).expand(HALO,extent);
                    for ((index,line),next) in edges.lines.iter().enumerate().filter(|(index,_)|input.external[*index]).zip(&self.scratch) {
                        LineCapture {source:&mut self.capture,next,seeds:&self.line_seeds[id]}
                            .capture(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),index,crop,encoder)?;
                    }
                    for next in self.scratch.iter().take(input.external.iter().filter(|v|**v).count()) {LineCapture::draw(scene,r,next,&lines.1,crop);}
                    scene.encode_jobs(r,encoder)?;
                    for ((_,line),next) in edges.lines.iter().enumerate().filter(|(index,_)|input.external[*index]).zip(&self.scratch) {
                        if line.fill_above {LineCapture::draw(scene,r,next,&cover.1,crop);}
                    }
                    scene.encode_jobs(r,encoder)?;
                    self.pages.push(((id,coordinate),lines,cover));
                }
            }
            self.next+=1;
        }
        Ok(())
    }
}
impl WgpuRasterizer {
    pub(crate) fn prepare_fill_edges(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker {return true;}
        let requested:Vec<_>=packet.dab_batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index)=batch.kind else {return None;};
            Some((batch.target,packet.scene.operations(batch.target)?.get(index as usize)?,batch_pixel_rect(batch,packet.scene.target_extent(batch.target))))
        }).chain(self.fill_preview.iter().map(|fill|(fill.target,&fill.operation,pixel_rect(fill.operation.bounds(packet.scene.target_extent(fill.target)),packet.scene.target_extent(fill.target)))))
            .filter_map(|(target,operation,damage)| {
                let RasterOperationKind::Fill {edges:Some(edges),underfill:Some(_),..}=&operation.kind else {return None;};
                Some((target,operation,damage,external(edges,packet,self.fill_preview.as_ref())))
            }).collect();
        if requested.is_empty() {self.fill_edges_preflight=None;return true;}
        if !self.compilation.require([&self.scene_pipelines.pipeline[fill_line_pipeline(&self.device)]],startup::DOCUMENT) {return false;}
        let mut state=if let Some(state)=self.fill_edges_preflight.take().filter(|state|requested.iter().all(|(target,operation,_,external)|
            matches!(&operation.kind,RasterOperationKind::Fill {edges:Some(edges),..} if state.input(*target,edges,external,
                packet.scene.target_extent(*target),packet.scene.target_offset(*target)).is_some()))) {state}else {
            let inputs:Vec<_>=requested.iter().map(|(target,operation,_,external)|Input {target:*target,operation:(**operation).clone(),external:external.clone(),
                extent:packet.scene.target_extent(*target),offset:packet.scene.target_offset(*target)}).collect();
            let steps=inputs.iter().enumerate().filter(|(_,input)|input.external.iter().any(|external|*external)).flat_map(|(id,input)| {
                std::iter::once(Step::Seed(id)).chain(page_coordinates(requested[id].2).filter(move |coordinate|band_intersects(&input.operation,*coordinate,input.extent,0)).map(move |coordinate|Step::Page(id,coordinate)))
            }).collect::<Vec<_>>();
            let scratch_count=inputs.iter().map(|input|input.external.iter().filter(|v|**v).count()).max().unwrap_or(0);
            let scalar_bytes=u64::from(SIDE).pow(2)*u64::from(self.device.scalar_format().block_copy_size(None).unwrap());
            let scratch_bytes=scratch_count as u64*u64::from(SIDE)*u64::from(SIDE+1)*u64::from(self.device.working_format().block_copy_size(None).unwrap());
            let bytes=(steps.iter().filter(|step|matches!(step,Step::Page(..))).count() as u64).checked_mul(2*scalar_bytes).and_then(|bytes|bytes.checked_add(scratch_bytes+scalar_bytes));
            let failure=bytes.ok_or(GpuRasterError::SizeOverflow).and_then(|bytes|self.admit_preparation_bytes(self.projected_restore_bytes(),bytes)).err();
            let scratch_count=if failure.is_none(){scratch_count}else{0};
            let line_seeds=inputs.iter().map(|input|create_color_target(&self.device,[1,input.external.len().max(1) as u32],"prepared fill line seeds").0).collect();
            FillEdgesPreflight {owner:packet.scene.owner(),color:self.document_color(),inputs,steps,next:0,capture:Default::default(),scene:None,
                seed:create_color_target(&self.device,[1;2],"prepared fill seed"),
                scratch:(0..scratch_count).map(|_|create_color_target(&self.device,[SIDE,SIDE+1],"prepared fill line source")).collect(),
                zero:create_target(&self.device,[SIDE;2],self.device.scalar_format(),"prepared fill empty cover"),line_seeds,pages:Vec::new(),failure}
        };
        if state.pending() {
            let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
            let result=self.drain_image_decodes(&mut encoder).and_then(|()|state.advance(self,packet,&mut encoder));
            if let Err(error)=result && !matches!(error,GpuRasterError::NotReady) {state.failure=Some(error);}
            if encoder.has_work() {self.uploads.finish(&encoder);self.last_submission=encoder.submit(&self.queue);}
        }
        let ready=!state.pending();self.fill_edges_preflight=Some(state);ready
    }
    pub(crate) fn take_fill_edges_failure(&mut self)->Option<GpuRasterError> {
        self.fill_edges_preflight.as_ref()?.failure.as_ref()?;
        self.fill_edges_preflight.take()?.failure
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layer_tests::placement,test_support::{packet,submit_ready}};

    fn fixture()->(layer_core::Document,SourceTarget,layer_core::RasterOperation) {
        fixture_extent([384;2])
    }
    fn fixture_extent(extent:[u32;2])->(layer_core::Document,SourceTarget,layer_core::RasterOperation) {
        let mut document=placement::paint_document(extent,"Fill target");
        let target=placement::target(&document);
        let image=layer_core::color::source::rgba8_source(extent,|x,_|if (252..260).contains(&x) {[24,48,80,192]}else{[0;4]});
        let reference=placement::append_paint(&mut document,"Reference",layer_core::authored::PaintSource {domain:extent,color_mode:Default::default(),
            raster:Default::default(),base:Some(layer_core::authored::PaintBase::new(image.into())),operations:Default::default()}).1;
        let rectangle=|x0,x1|layer_core::Selection::polygon(vec![layer_core::Point{x:x0,y:0.},layer_core::Point{x:x1,y:0.},
            layer_core::Point{x:x1,y:extent[1] as f32},layer_core::Point{x:x0,y:extent[1] as f32}]).unwrap();
        let mut coverage=layer_core::CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(),extent,[0;2]);
        coverage.source.default_coverage=0.;coverage.selection=Some(rectangle(0.,256.));
        let snapshot=document.snapshot();
        let operation=layer_core::RasterOperation {placement:layer_core::Affine::IDENTITY,coverage,kind:RasterOperationKind::Fill {
            color:[0.1,0.4,0.7,1.],texture:Default::default(),alpha_locked:false,underfill:Some(rectangle(252.,256.)),
            edges:Some(Box::new(layer_core::FillEdges { translucent_seed: false,snapshot:snapshot.clone(),position:layer_core::Point{x:240.,y:192.},tolerance:0.,colour_on_solid:false,
                lines:vec![layer_core::FillLine {snapshot,scope:SceneScope::Raw(reference),fill_above:false}]}))}};
        (document,target,operation)
    }
    fn commit(document:&mut layer_core::Document,target:SourceTarget,operation:layer_core::RasterOperation)->DabBatch {
        let batch=DabBatch {kind:DabBatchKind::RasterOperation(0),dab_count:0,damage:operation.bounds([384;2]),
            ..crate::test_support::dab_batch(target,crate::tests::test_style(BrushExecution::Dry),layer_core::Rect::from_extent([384;2]))};
        let SourceTarget::Paint(handle)=target else {unreachable!()};
        let mut source=document.artwork.paint.get(handle).unwrap().clone();source.operations=Arc::new(vec![operation]);
        document.apply(layer_core::Edit::Paint(layer_core::authored::RecordChange::replace(&document.artwork.paint,handle,Some(source)).unwrap())).unwrap();
        batch
    }
    #[test]
    fn fill_line_readiness_matches_the_draw_attachment_without_float32_blending() {
        let mut results=Vec::new();
        for omitted in [wgpu::Features::FLOAT32_BLENDABLE,wgpu::Features::empty()] {
            let reference=WgpuRasterizer::native_headless_without(Default::default(),omitted).unwrap();
            let mut r=crate::test_support::staged_renderer(&reference,Default::default());
            let portable=omitted.contains(wgpu::Features::FLOAT32_BLENDABLE);
            assert_eq!(r.device.portable_blend(),portable);
            let (mut document,target,operation)=fixture();
            let batch=commit(&mut document,target,operation);
            let brush=layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
            r.prepare_startup(&document,&brush,false).unwrap();
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(60);
            crate::test_support::wait_startup(&mut r,deadline,|progress|progress.brush_ready,
                format_args!("Fill line startup timed out"));
            assert_eq!(r.scene_pipelines.pipeline[FILL_LINES_PIPELINE].ready(),!portable,
                "fill readiness must require the attachment's pipeline");
            let frame=FramePacket {reset_layers:true,dab_batches:std::slice::from_ref(&batch),..packet(document.scene(),[384;2])};
            submit_ready(&mut r,frame).unwrap();
            assert_eq!(r.scene_pipelines.pipeline[FILL_LINES_PIPELINE].ready(),!portable,
                "portable fill line draws must leave the scalar recipe cold");
            let pixels=r.readback_srgb_rgba8().unwrap();
            assert!(pixels[(192*384+240)*4+3]>0,"the edge fill must draw");
            results.push(pixels);
        }
        assert_eq!(results[0],results[1]);
    }
    #[test]
    fn ready_fill_preview_reuses_seed_and_page_steps_at_commit() {
        let mut results=Vec::new();
        for preview in [false,true] {
            let (mut document,target,operation)=fixture();let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r,FramePacket {reset_layers:true,..packet(document.scene(),[384;2])}).unwrap();
            let retained=if preview {
                r.set_fill_preview(Some(layer_render::FillPreview {transaction:42,revision:0,target,operation:operation.clone()}));
                submit_ready(&mut r,packet(document.scene(),[384;2])).unwrap();
                let state=r.fill_edges_preflight.as_ref().unwrap();assert!(!state.pending());assert!(!state.pages.is_empty());
                Some((state.line_seeds.clone(),state.steps.len(),state.next))
            }else{None};
            let batch=commit(&mut document,target,operation);r.set_fill_preview(None);
            let frame=FramePacket {dab_batches:std::slice::from_ref(&batch),..packet(document.scene(),[384;2])};
            crate::test_support::prepare_raster(&mut r,|r|if r.raster_dependencies_ready(frame,None){Ok(())}else{Err(GpuRasterError::NotReady)}).unwrap();
            if let Some((seeds,steps,next))=retained {
                let state=r.fill_edges_preflight.as_ref().unwrap();assert_eq!(state.line_seeds,seeds);
                assert_eq!((state.steps.len(),state.next),(steps,next));assert_eq!(next,steps);
            }
            submit_ready(&mut r,frame).unwrap();results.push(r.readback_srgb_rgba8().unwrap());
        }
        assert_eq!(results[0],results[1]);
    }
    #[test]
    fn equal_preview_key_with_a_different_reference_snapshot_rebuilds_captures() {
        let (document,target,operation)=fixture();let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit_ready(&mut r,FramePacket {reset_layers:true,..packet(document.scene(),[384;2])}).unwrap();
        let mut preview=layer_render::FillPreview {transaction:42,revision:0,target,operation};
        r.set_fill_preview(Some(preview.clone()));submit_ready(&mut r,packet(document.scene(),[384;2])).unwrap();
        let seeds=r.fill_edges_preflight.as_ref().unwrap().line_seeds.clone();let pixels=r.readback_srgb_rgba8().unwrap();
        let RasterOperationKind::Fill {edges:Some(edges),..}=&mut preview.operation.kind else {unreachable!()};
        let old=edges.lines[0].snapshot.clone();edges.lines[0].snapshot=Arc::new((*old).clone());
        assert_eq!(old,edges.lines[0].snapshot);assert!(!Arc::ptr_eq(&old,&edges.lines[0].snapshot));
        r.set_fill_preview(Some(preview));submit_ready(&mut r,packet(document.scene(),[384;2])).unwrap();
        assert_ne!(r.fill_edges_preflight.as_ref().unwrap().line_seeds,seeds);
        assert_eq!(r.readback_srgb_rgba8().unwrap(),pixels);
    }
    #[test]
    fn three_external_lines_match_when_prepared_pages_are_dropped() {
        let mut results=Vec::new();
        for live in [false,true] {
            let (mut document,target,mut operation)=fixture();
            for index in 0..2 {
                let image=layer_core::color::source::rgba8_source([384;2],move|x,y| {
                    let ink=if index==0 {(220..230).contains(&y)}else{(i64::from(x)+i64::from(y)-480).abs()<8};
                    if ink {[80,24,48,128]}else{[0;4]}
                });
                let reference=placement::append_paint(&mut document,"Reference",layer_core::authored::PaintSource {domain:[384;2],color_mode:Default::default(),
                    raster:Default::default(),base:Some(layer_core::authored::PaintBase::new(image.into())),operations:Default::default()}).1;
                let RasterOperationKind::Fill {edges:Some(edges),..}=&mut operation.kind else {unreachable!()};
                edges.lines.push(layer_core::FillLine {snapshot:document.snapshot(),scope:SceneScope::Raw(reference),fill_above:true});
            }
            let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            submit_ready(&mut r,FramePacket {reset_layers:true,..packet(document.scene(),[384;2])}).unwrap();
            let before=r.readback_srgb_rgba8().unwrap();
            let batch=commit(&mut document,target,operation);
            let frame=FramePacket {dab_batches:std::slice::from_ref(&batch),..packet(document.scene(),[384;2])};
            crate::test_support::prepare_raster(&mut r,|r|if r.raster_dependencies_ready(frame,None){Ok(())}else{Err(GpuRasterError::NotReady)}).unwrap();
            let state=r.fill_edges_preflight.as_ref().unwrap();
            assert!(!state.pending());assert_eq!(state.inputs[0].external,[true;3]);assert!(!state.pages.is_empty());
            if live {r.fill_edges_preflight=None;}
            r.submit(frame).unwrap();let pixels=r.readback_srgb_rgba8().unwrap();assert_ne!(pixels,before);results.push(pixels);
        }
        assert_eq!(results[0],results[1]);
    }
    #[test]
    fn fill_preflight_retries_after_memory_pressure() {
        let extent=[384,8192];let (mut document,target,mut operation)=fixture_extent(extent);
        let RasterOperationKind::Fill {edges:Some(edges),..}=&mut operation.kind else {unreachable!()};
        let SceneScope::Raw(SourceTarget::Paint(reference))=edges.lines[0].scope else {unreachable!()};
        let image=layer_core::color::source::rgba8_source(extent,|x,_|if (252..258).contains(&x) {[24,48,80,192]}else{[0;4]});
        document.artwork.paint.get_mut(reference).unwrap().base=Some(layer_core::authored::PaintBase::new(image.into()));
        edges.snapshot=document.snapshot();edges.lines[0].snapshot=edges.snapshot.clone();
        let source=document.artwork.paint.get(reference).unwrap().clone();
        for _ in 1..8 {
            let reference=placement::append_paint(&mut document,"Reference",source.clone()).1;
            edges.lines.push(layer_core::FillLine {snapshot:document.snapshot(),scope:SceneScope::Raw(reference),fill_above:false});
        }
        let references:Vec<_>=document.scene().order().iter().copied().filter(|handle|document.scene().source_target(*handle).is_some_and(|source|source!=target)).collect();
        for reference in references {document.artwork.occurrences.get_mut(reference).unwrap().visible=false;}
        let mut r=crate::memory::memory_tests::renderer(Default::default(),layer_core::DeviceMemoryFacts {unified:true,..Default::default()}).unwrap();
        let frame=FramePacket {view:layer_render::ViewState {width_px:384,height_px:384,document_to_surface:[0.046875,0.,0.,0.046875,0.,0.]},..packet(document.scene(),extent)};
        submit_ready(&mut r,frame).unwrap();
        let budget=r.factory.memory_budget().clone();budget.set_ram_override(Some(384<<20));
        r.publish_memory();budget.flush_reports();r.refresh_memory_limits();
        let pressure=budget.admit_import(budget.import_headroom_bytes()).unwrap();r.refresh_memory_limits();
        let preview=layer_render::FillPreview {transaction:42,revision:0,target,operation};
        r.set_fill_preview(Some(preview.clone()));
        crate::test_support::prepare_raster(&mut r,|r|if r.prepare_fill_edges(frame){Ok(())}else{Err(GpuRasterError::NotReady)}).unwrap();
        assert!(matches!(r.submit(frame),Err(GpuRasterError::SourceWorkingSetExceeded|GpuRasterError::NotReady)));
        drop(pressure);r.refresh_memory_limits();crate::test_support::prepare_raster(&mut r,|r|submit_ready(r,frame)).unwrap();
        let pixels=r.readback_srgb_rgba8().unwrap();
        let mut control=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        submit_ready(&mut control,FramePacket {reset_layers:true,..frame}).unwrap();
        control.set_fill_preview(Some(preview));submit_ready(&mut control,frame).unwrap();
        assert!(pixels==control.readback_srgb_rgba8().unwrap());
    }
}
