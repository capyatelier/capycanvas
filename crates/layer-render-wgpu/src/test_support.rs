use super::*;
use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PassCounts {
    pub material_encode_accepts:u64,
    pub material_neighborhoods:u64,
    pub material_bind_groups:u64,
    pub transport_bind_groups:u64,
    pub total:u64,
    pub normalizations:u64,
    pub material_batches:u64,
    pub source_input_misses:u64,
    pub raster_output_misses:u64,
    pub raster_write_defers:u64,
    pub store_page_visits:u64,
    pub paint_page_visits:u64,
    pub display_walks:u64,
    pub preview_revision_replays:u64,
    pub unit_acceptances:u64,
    pub projected_cells:u64,
    pub projected_copies:u64,
    pub composite_commits:u64,
    pub declarations:u64,
    pub store_gets:u64,
    pub expected_keys:u64,
    pub brush_acceptances:u64,
    pub preview_computations:u64,
    pub completion_callbacks:u64,
    pub preview_definitions:u64,
    pub warm_restore_extra_tiles:u64,
    pub navigator_checks:u64,
    pub graph_prepares:u64,
    pub signature_reads:u64,
    pub contact_declarations:u64,
    pub destination_units:u64,
    pub raster_admissions:u64,
    pub vector_names:u64,
    pub grant_encoders:u64,
    pub command_encoders:u64,
    pub report_syncs:u64,
    pub scene_dispatches:u64,
    pub fold_inputs:u64,
    pub display_rooms:u64,
    pub memory_updates:u64,
    pub object_lookups:u64,
    pub object_lookup_visits:u64,
    pub room_scans:u64,
    pub composed_texels:u64,
}
std::thread_local! {
    static PASS_COUNTS:std::cell::Cell<PassCounts>=const {std::cell::Cell::new(PassCounts {material_encode_accepts:0,material_neighborhoods:0,material_bind_groups:0,transport_bind_groups:0,total:0,normalizations:0,material_batches:0,source_input_misses:0,raster_output_misses:0,raster_write_defers:0,store_page_visits:0,paint_page_visits:0,display_walks:0,preview_revision_replays:0,unit_acceptances:0,projected_cells:0,projected_copies:0,composite_commits:0,declarations:0,store_gets:0,expected_keys:0,brush_acceptances:0,preview_computations:0,completion_callbacks:0,preview_definitions:0,warm_restore_extra_tiles:0,navigator_checks:0,graph_prepares:0,signature_reads:0,contact_declarations:0,destination_units:0,raster_admissions:0,vector_names:0,grant_encoders:0,report_syncs:0,command_encoders:0,scene_dispatches:0,fold_inputs:0,display_rooms:0,memory_updates:0,object_lookups:0,object_lookup_visits:0,room_scans:0,composed_texels:0})};
}
type PreviewName=(layer_core::SourceTarget,usize,[u32;2],crate::scene::PreviewStage);
std::thread_local! {static PREVIEW_NAMES:std::cell::RefCell<Option<crate::IdMap<PreviewName,u32>>>=const {std::cell::RefCell::new(None)};}
pub(crate) fn record_preview_name(name:PreviewName) {PREVIEW_NAMES.with(|names|if let Some(names)=names.borrow_mut().as_mut() {*names.entry(name).or_default()+=1;});}
type PreviewRegion=(usize,[u32;4],crate::scene::PreviewStage,crate::scene::PreviewPass);
std::thread_local! {static PREVIEW_REGIONS:std::cell::RefCell<Option<crate::IdMap<PreviewRegion,u32>>>=const {std::cell::RefCell::new(None)};}
pub(crate) fn record_preview_region(region:PreviewRegion) {PREVIEW_REGIONS.with(|regions|if let Some(regions)=regions.borrow_mut().as_mut() {*regions.entry(region).or_default()+=1;});}
pub(crate) fn preview_regions<T>(run:impl FnOnce()->T)->(T,crate::IdMap<PreviewRegion,u32>) {
    PREVIEW_REGIONS.with(|regions|*regions.borrow_mut()=Some(Default::default()));
    let value=run();
    (value,PREVIEW_REGIONS.with(|regions|regions.borrow_mut().take().unwrap_or_default()))
}
type TransportRead=(usize,[u32;2],usize,u32);
std::thread_local! {static TRANSPORT_READS:std::cell::RefCell<Option<crate::IdMap<TransportRead,u32>>>=const {std::cell::RefCell::new(None)};}
pub(crate) fn record_transport_read(read:TransportRead) {TRANSPORT_READS.with(|reads|if let Some(reads)=reads.borrow_mut().as_mut() {*reads.entry(read).or_default()+=1;});}
pub(crate) fn transport_reads<T>(run:impl FnOnce()->T)->(T,crate::IdMap<TransportRead,u32>) {
    TRANSPORT_READS.with(|reads|*reads.borrow_mut()=Some(Default::default()));
    let value=run();
    (value,TRANSPORT_READS.with(|reads|reads.borrow_mut().take().unwrap_or_default()))
}
type MaterialRead=([u32;2],Option<crate::scene::PreviewStage>);
std::thread_local! {static MATERIAL_READS:std::cell::RefCell<Option<crate::IdMap<MaterialRead,u32>>>=const {std::cell::RefCell::new(None)};}
pub(crate) fn record_material_read(read:MaterialRead) {MATERIAL_READS.with(|reads|if let Some(reads)=reads.borrow_mut().as_mut() {*reads.entry(read).or_default()+=1;});}
pub(crate) fn material_reads<T>(run:impl FnOnce()->T)->(T,crate::IdMap<MaterialRead,u32>) {
    MATERIAL_READS.with(|reads|*reads.borrow_mut()=Some(Default::default()));
    let value=run();
    (value,MATERIAL_READS.with(|reads|reads.borrow_mut().take().unwrap_or_default()))
}
type SourcePreparation=(layer_core::SourceTarget,u32,[u32;2]);
std::thread_local! {static SOURCE_PREPARATIONS:std::cell::RefCell<Option<crate::IdMap<SourcePreparation,u32>>>=const {std::cell::RefCell::new(None)};}
pub(crate) fn record_source_preparation(page:SourcePreparation) {SOURCE_PREPARATIONS.with(|pages|if let Some(pages)=pages.borrow_mut().as_mut() {*pages.entry(page).or_default()+=1;});}
pub(crate) fn count_source_preparations() {SOURCE_PREPARATIONS.with(|pages|*pages.borrow_mut()=Some(Default::default()));}
pub(crate) fn source_preparations()->[u64;2] {
    SOURCE_PREPARATIONS.with(|pages|pages.borrow().as_ref().map_or([0;2],|pages|[pages.values().map(|count|u64::from(*count)).sum(),pages.len() as u64]))
}
pub(crate) fn preview_names<T>(run:impl FnOnce()->T)->(T,crate::IdMap<PreviewName,u32>) {
    PREVIEW_NAMES.with(|names|*names.borrow_mut()=Some(Default::default()));
    let value=run();
    (value,PREVIEW_NAMES.with(|names|names.borrow_mut().take().unwrap_or_default()))
}
impl PassCounts {
    pub fn snapshot()->Self {PASS_COUNTS.with(std::cell::Cell::get)}
    pub fn delta(self,before:Self)->Self {
        Self {material_encode_accepts:self.material_encode_accepts-before.material_encode_accepts,material_neighborhoods:self.material_neighborhoods-before.material_neighborhoods,material_bind_groups:self.material_bind_groups-before.material_bind_groups,transport_bind_groups:self.transport_bind_groups-before.transport_bind_groups,total:self.total-before.total,normalizations:self.normalizations-before.normalizations,
            material_batches:self.material_batches-before.material_batches,source_input_misses:self.source_input_misses-before.source_input_misses,
            raster_output_misses:self.raster_output_misses-before.raster_output_misses,raster_write_defers:self.raster_write_defers-before.raster_write_defers,
            store_page_visits:self.store_page_visits-before.store_page_visits,paint_page_visits:self.paint_page_visits-before.paint_page_visits,display_walks:self.display_walks-before.display_walks,
            preview_revision_replays:self.preview_revision_replays-before.preview_revision_replays,unit_acceptances:self.unit_acceptances-before.unit_acceptances,
            projected_cells:self.projected_cells-before.projected_cells,projected_copies:self.projected_copies-before.projected_copies,composite_commits:self.composite_commits-before.composite_commits,
            declarations:self.declarations-before.declarations,store_gets:self.store_gets-before.store_gets,expected_keys:self.expected_keys-before.expected_keys,
            brush_acceptances:self.brush_acceptances-before.brush_acceptances,preview_computations:self.preview_computations-before.preview_computations,completion_callbacks:self.completion_callbacks-before.completion_callbacks,preview_definitions:self.preview_definitions-before.preview_definitions,warm_restore_extra_tiles:self.warm_restore_extra_tiles-before.warm_restore_extra_tiles,navigator_checks:self.navigator_checks-before.navigator_checks,graph_prepares:self.graph_prepares-before.graph_prepares,signature_reads:self.signature_reads-before.signature_reads,contact_declarations:self.contact_declarations-before.contact_declarations,destination_units:self.destination_units-before.destination_units,raster_admissions:self.raster_admissions-before.raster_admissions,vector_names:self.vector_names-before.vector_names,grant_encoders:self.grant_encoders-before.grant_encoders,report_syncs:self.report_syncs-before.report_syncs,command_encoders:self.command_encoders-before.command_encoders,scene_dispatches:self.scene_dispatches-before.scene_dispatches,fold_inputs:self.fold_inputs-before.fold_inputs,display_rooms:self.display_rooms-before.display_rooms,memory_updates:self.memory_updates-before.memory_updates,object_lookups:self.object_lookups-before.object_lookups,object_lookup_visits:self.object_lookup_visits-before.object_lookup_visits,room_scans:self.room_scans-before.room_scans,composed_texels:self.composed_texels-before.composed_texels}
    }
    pub fn record(self) {
        PASS_COUNTS.with(|counts| {
            let before=counts.get();
            counts.set(Self {material_encode_accepts:before.material_encode_accepts+self.material_encode_accepts,material_neighborhoods:before.material_neighborhoods+self.material_neighborhoods,material_bind_groups:before.material_bind_groups+self.material_bind_groups,transport_bind_groups:before.transport_bind_groups+self.transport_bind_groups,total:before.total+self.total,normalizations:before.normalizations+self.normalizations,
                material_batches:before.material_batches+self.material_batches,source_input_misses:before.source_input_misses+self.source_input_misses,
                raster_output_misses:before.raster_output_misses+self.raster_output_misses,raster_write_defers:before.raster_write_defers+self.raster_write_defers,
                store_page_visits:before.store_page_visits+self.store_page_visits,paint_page_visits:before.paint_page_visits+self.paint_page_visits,display_walks:before.display_walks+self.display_walks,
                preview_revision_replays:before.preview_revision_replays+self.preview_revision_replays,unit_acceptances:before.unit_acceptances+self.unit_acceptances,
                projected_cells:before.projected_cells+self.projected_cells,projected_copies:before.projected_copies+self.projected_copies,composite_commits:before.composite_commits+self.composite_commits,
                declarations:before.declarations+self.declarations,store_gets:before.store_gets+self.store_gets,expected_keys:before.expected_keys+self.expected_keys,
                brush_acceptances:before.brush_acceptances+self.brush_acceptances,preview_computations:before.preview_computations+self.preview_computations,completion_callbacks:before.completion_callbacks+self.completion_callbacks,preview_definitions:before.preview_definitions+self.preview_definitions,warm_restore_extra_tiles:before.warm_restore_extra_tiles+self.warm_restore_extra_tiles,navigator_checks:before.navigator_checks+self.navigator_checks,graph_prepares:before.graph_prepares+self.graph_prepares,signature_reads:before.signature_reads+self.signature_reads,contact_declarations:before.contact_declarations+self.contact_declarations,destination_units:before.destination_units+self.destination_units,raster_admissions:before.raster_admissions+self.raster_admissions,vector_names:before.vector_names+self.vector_names,grant_encoders:before.grant_encoders+self.grant_encoders,report_syncs:before.report_syncs+self.report_syncs,command_encoders:before.command_encoders+self.command_encoders,scene_dispatches:before.scene_dispatches+self.scene_dispatches,fold_inputs:before.fold_inputs+self.fold_inputs,display_rooms:before.display_rooms+self.display_rooms,memory_updates:before.memory_updates+self.memory_updates,object_lookups:before.object_lookups+self.object_lookups,object_lookup_visits:before.object_lookup_visits+self.object_lookup_visits,room_scans:before.room_scans+self.room_scans,composed_texels:before.composed_texels+self.composed_texels});
        });
    }
}

#[derive(Clone, Debug, PartialEq)]
struct SubmittedBatch {
    batch:DabBatch, dabs:Vec<u8>,
}
#[derive(Clone, Debug, PartialEq)]
struct SubmittedArtwork {
    scene:(u64,u64), batches:Vec<SubmittedBatch>, preview:Vec<(DabBatch,Vec<u8>)>,
}
fn submitted_dabs(dabs:&[Dab])->Vec<u8> {
    dab_bytes(&dabs.iter().copied().map(DabGpu::from).collect::<Vec<_>>()).to_vec()
}
impl SubmittedArtwork {
    fn from_packet(packet:FramePacket<'_>)->Self {
        Self {scene:(packet.scene.owner(),packet.scene.revision()),
            batches:packet.dab_batches.iter().filter(|batch|batch.kind!=DabBatchKind::Preview).map(|batch|SubmittedBatch {
                batch:batch.clone(),
                dabs:submitted_dabs(&packet.dabs[batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize]),
            }).collect(),
            preview:packet.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview).map(|batch| {
                let start=batch.first_dab as usize;let end=start+batch.dab_count as usize;
                (batch.clone(),submitted_dabs(&packet.dabs[start..end]))
            }).collect()}
    }
}
#[derive(Default)]
struct ArtworkRevisionOracle {
    previous:Option<SubmittedArtwork>, seen:Vec<SubmittedArtwork>,
}
impl ArtworkRevisionOracle {
    fn observe(&mut self,submitted:SubmittedArtwork,before:u64,revision:u64)->Result<(), &'static str> {
        if revision!=before {
            if !submitted.batches.is_empty() && self.seen.contains(&submitted) {
                return Err("replayed submitted artwork changed artwork revision");
            }
            if let Some(previous)=&self.previous
                && submitted.scene==previous.scene && submitted.preview==previous.preview
                && (submitted.batches.is_empty() || submitted.batches==previous.batches) {
                if submitted.batches.is_empty() && !submitted.preview.is_empty() {
                    PassCounts {preview_revision_replays:1,..Default::default()}.record();
                } else {return Err("unchanged submitted artwork changed artwork revision");}
            }
        }
        if !self.seen.contains(&submitted) {self.seen.push(submitted.clone());}
        self.previous=Some(submitted);
        Ok(())
    }
}
std::thread_local! {
    static ARTWORK_REVISION_ORACLE:std::cell::RefCell<Option<(ArtworkRevisionOracle,String)>>=const {std::cell::RefCell::new(None)};
}
pub(crate) struct SubmittedArtworkObserver;
impl SubmittedArtworkObserver {
    pub fn new(context:String)->Self {
        ARTWORK_REVISION_ORACLE.with(|observer| {
            assert!(observer.borrow().is_none());
            *observer.borrow_mut()=Some((ArtworkRevisionOracle::default(),context));
        });
        Self
    }
}
impl Drop for SubmittedArtworkObserver {
    fn drop(&mut self) {ARTWORK_REVISION_ORACLE.with(|observer|{observer.borrow_mut().take();});}
}
pub(crate) fn observe_submitted_artwork(packet:FramePacket<'_>,before:u64,revision:u64) {
    ARTWORK_REVISION_ORACLE.with(|observer| {
        if let Some((oracle,context))=&mut *observer.borrow_mut() {
            let submitted=SubmittedArtwork::from_packet(packet);
            let result=oracle.observe(submitted.clone(),before,revision);
            assert!(result.is_ok(),"{context}: {result:?}, revision={before}->{revision}, submitted={submitted:?}");
        }
    });
}

#[test]
fn bounded_calibration_reaches_its_range_for_dispatches_up_to_the_range_width() {
    for dispatch in [0.01,0.3,7.,21.,34.,35.] {
        let mut count=128;let mut calibrated=f64::from(count)*dispatch;
        for _ in 0..3 {if (40. ..=75.).contains(&calibrated) {break;}count=calibration_count(count,calibrated);calibrated=f64::from(count)*dispatch;}
        assert!((40. ..=75.).contains(&calibrated),"{dispatch} ms dispatches calibrated to {calibrated} ms with {count}");
    }
}

#[cfg(test)]
mod artwork_revision_oracle_tests {
    use super::*;
    fn authored(revision:u64)->SubmittedArtwork {SubmittedArtwork {scene:(1,revision),batches:Vec::new(),preview:Vec::new()}}
    #[test]
    fn delayed_new_identity_is_allowed_once() {
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(authored(1),10,10).unwrap();
        oracle.observe(authored(1),10,10).unwrap();
        oracle.observe(authored(2),10,11).unwrap();
        assert!(oracle.observe(authored(2),11,12).is_err());
    }
    #[test]
    fn replayed_batch_cannot_change_artwork_revision() {
        let mut artwork=layer_core::authored::Artwork::new([32;2]).unwrap();
        let target=add_paint(&mut artwork,"paint",[32;2]).1;
        let mut submitted=authored(1);
        let batch=dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Default::default());
        submitted.batches.push(SubmittedBatch {batch,dabs:submitted_dabs(&[crate::tests::test_dab([16.;2],[1.;4],1.)])});
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(authored(0),0,0).unwrap();
        oracle.observe(submitted.clone(),0,1).unwrap();
        let mut next=submitted.clone();
        next.batches[0].dabs=submitted_dabs(&[crate::tests::test_dab([17.,16.],[1.;4],1.)]);
        oracle.observe(next,1,2).unwrap();
        oracle.observe(authored(1),2,2).unwrap();
        assert!(oracle.observe(submitted,2,3).is_err());
    }
    #[test]
    fn a_new_material_update_is_distinct_authored_work() {
        let mut artwork=layer_core::authored::Artwork::new([32;2]).unwrap();
        let target=add_paint(&mut artwork,"paint",[32;2]).1;
        let batch=dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Default::default());
        let mut submitted=authored(1);
        submitted.batches.push(SubmittedBatch {batch,dabs:submitted_dabs(&[crate::tests::test_dab([16.;2],[1.;4],1.)])});
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(submitted.clone(),0,1).unwrap();
        submitted.batches[0].batch.material_update+=1;
        oracle.observe(submitted.clone(),1,2).unwrap();
        assert!(oracle.observe(submitted,2,3).is_err());
    }
    #[test]
    fn pure_camera_revision_bump_fails() {
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(authored(1),0,1).unwrap();
        assert!(oracle.observe(authored(1),1,2).is_err());
    }
    #[test]
    fn renderer_counter_reset_does_not_exempt_a_replayed_identity() {
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(authored(1),8,9).unwrap();
        oracle.observe(authored(1),0,0).unwrap();
        assert!(oracle.observe(authored(1),0,1).is_err());
    }
    #[test]
    fn preview_only_new_identity_is_allowed() {
        let mut artwork=layer_core::authored::Artwork::new([32;2]).unwrap();
        let target=add_paint(&mut artwork,"paint",[32;2]).1;
        let mut batch=dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Default::default());
        batch.kind=DabBatchKind::Preview;
        let mut submitted=authored(1);
        let mut oracle=ArtworkRevisionOracle::default();
        oracle.observe(submitted.clone(),0,1).unwrap();
        submitted.preview.push((batch,submitted_dabs(&[crate::tests::test_dab([16.;2],[1.;4],1.)])));
        oracle.observe(submitted.clone(),1,2).unwrap();
        submitted.preview[0].1=submitted_dabs(&[crate::tests::test_dab([17.,16.],[1.;4],1.)]);
        oracle.observe(submitted,2,3).unwrap();
    }
    #[test]
    fn identical_preview_revisions_are_counted_without_exempting_persistent_replays() {
        let mut artwork=layer_core::authored::Artwork::new([32;2]).unwrap();
        let target=add_paint(&mut artwork,"paint",[32;2]).1;
        let mut batch=dab_batch(target,crate::tests::test_style(BrushExecution::Dry),Default::default());
        batch.kind=DabBatchKind::Preview;
        let mut submitted=authored(1);
        submitted.preview.push((batch.clone(),submitted_dabs(&[crate::tests::test_dab([16.;2],[1.;4],1.)])));
        let mut oracle=ArtworkRevisionOracle::default();let before=PassCounts::snapshot();
        oracle.observe(submitted.clone(),0,1).unwrap();
        oracle.observe(submitted,1,2).unwrap();
        assert_eq!(PassCounts::snapshot().delta(before).preview_revision_replays,1);
        batch.kind=DabBatchKind::Persistent;
        let mut committed=authored(1);committed.batches.push(SubmittedBatch {batch,dabs:Vec::new()});
        oracle.observe(committed.clone(),2,3).unwrap();
        assert!(oracle.observe(committed,3,4).is_err());
    }
}

pub(crate) fn photo_limits() -> layer_color::photo::DecodeLimits {
    layer_color::photo::DecodeLimits::from_memory_budget(layer_color::photo::PhotoMemoryBudget::from_limit(2 << 30))
}

pub(crate) fn mask_texture(r:&WgpuRasterizer,target:layer_core::SourceTarget,coordinate:[u32;2])->&wgpu::Texture {
    &r.layer_masks.pages[&(target,coordinate)].read(r.current_mask_version(target,coordinate).unwrap(),None).unwrap().value().texture
}

pub(crate) fn mask_page_bytes(r:&WgpuRasterizer,target:layer_core::SourceTarget,coordinate:[u32;2])->Vec<u8> {
    let expected=r.current_mask_version(target,coordinate).unwrap();
    if let Some(read)=r.layer_masks.pages.get(&(target,coordinate)).and_then(|page|page.read(&expected,None).ok()) {
        return crate::layer_tests::page_bytes(r,&read.value().texture);
    }
    let source=&r.layer_masks.definitions[&target];
    let data=r.mask_data(target,source).unwrap();
    let mut capture=WgpuRasterizer::native_capture_on_factory(r.factory.clone(),r.device.clone(),r.document_color()).unwrap();
    let page=if let Some(tile)=data.tiles.get(&layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Mask,coordinate}) {
        assert_eq!(tile.content_version().matches(&expected),Some(true),"cold mask must name the selected input");
        let blob=tile.wait_backing().unwrap();
        assert!(matches!(blob.descriptor.depth(),layer_core::color::SampleDepth::U8|layer_core::color::SampleDepth::U16),"cold mask fixture requires a supported native scalar descriptor");
        capture.restore_mask_page(&blob,expected.clone()).unwrap()
    } else {
        assert_eq!(crate::raster::contents::mask_default(source.domain,source.default_coverage,coordinate).matches(&expected),Some(true));
        let mut page=crate::layer_masks::MaskPage::new(&capture.device);
        let mut encoder=crate::submission::CommandEncoder::new(&capture.device,&Default::default());
        page.clear(&mut encoder,f64::from(source.default_coverage),expected.clone()).unwrap();
        encoder.submit(&capture.queue);page
    };
    complete(&capture);
    crate::layer_tests::page_bytes(&capture,&page.read(&expected,None).unwrap().value().texture)
}

pub(crate) fn prepare_raster<T>(r:&mut WgpuRasterizer,mut operation:impl FnMut(&mut WgpuRasterizer)->Result<T,GpuRasterError>)->Result<T,GpuRasterError> {
    let retained=|r:&WgpuRasterizer|r.paint_layers.iter().flat_map(|layer|layer.pages.iter().map(|page|page.active().allocation_identity())
        .chain(layer.watercolor_wetness_pages.iter().map(|page|page.active().allocation_identity())))
        .collect::<Vec<_>>();
    let retained=|r:&WgpuRasterizer|(retained(r),r.layer_masks.pages.values().map(|page|page.allocation_identity()).collect::<Vec<_>>());
    let original=retained(r);let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
    loop {
        match operation(r) {
            Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=> {
                assert_eq!(retained(r),original,"pending preparation published live pages");
                assert!(std::time::Instant::now()<deadline,"raster preparation did not complete");
                r.wait_idle().unwrap();std::thread::sleep(std::time::Duration::from_millis(1));
            },result=>return result,
        }
    }
}

pub(crate) fn add_paint(artwork: &mut layer_core::authored::Artwork, name: impl Into<Arc<str>>, domain: [u32; 2]) -> (OccurrenceHandle, SourceTarget) {
    use layer_core::authored::*;
    let source = artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(),
        domain, raster: Default::default(), base: None, operations: Arc::default(),
    }).unwrap();
    let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(source), name)).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    let at = above_bottom_paper(artwork);
    artwork.stacks.get_mut(stack).unwrap().entries.insert(at, occurrence);
    (occurrence, SourceTarget::Paint(source))
}
pub(crate) fn above_bottom_paper(artwork: &layer_core::authored::Artwork) -> usize {
    let entries = &artwork.stacks.get(artwork.compositions.get(artwork.root).unwrap().result).unwrap().entries;
    let paper = entries.last().and_then(|h| match artwork.occurrences.get(*h)?.content {
        layer_core::authored::OccurrenceContent::Effect(effect) => artwork.effects.get(effect), _ => None,
    }).is_some_and(|effect| layer_core::paper::is_paper(&effect.program));
    entries.len() - usize::from(paper)
}

pub(crate) fn tile_read_cache() -> layer_core::tile_io::ReadCache {
    layer_core::tile_io::ReadCache::new(layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {
        host_default_bytes:Some(1<<30),override_bytes:Some(1<<30),..Default::default()
    }))
}
pub(crate) struct StoredTiles {
    store: Option<layer_core::package::session_store::SessionStore>,
    pub path: std::path::PathBuf,
}
impl StoredTiles {
    pub fn new(tiles:&layer_core::raster_storage::RetainedTiles,cache:layer_core::tile_io::ReadCache)->Self {
        Self::with_reads(tiles,cache,None)
    }
    pub(crate) fn with_reads(tiles:&layer_core::raster_storage::RetainedTiles,cache:layer_core::tile_io::ReadCache,observe:Option<Arc<dyn Fn(u64,usize)+Send+Sync>>)->Self {
        use std::sync::atomic::{AtomicU64,Ordering};
        static NEXT:AtomicU64=AtomicU64::new(0);
        let path=std::env::temp_dir().join(format!("capy-render-store-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        let mut store=pollster::block_on(async {match observe {
            Some(observe)=>layer_core::package::store_io::test_support::open_observed(&path,cache,observe).await,
            None=>layer_core::package::session_store::SessionStore::open_with_cache(&path,cache).await,
        }}).unwrap();
        assert!(pollster::block_on(store.make_durable(tiles,&std::sync::atomic::AtomicBool::new(false))).unwrap()>0);
        Self {store:Some(store),path}
    }
    pub fn restore_session(&mut self,capture:&layer_core::package::session::EditorCapture)->layer_core::package::session::OpenSession {
        let cancel=std::sync::atomic::AtomicBool::new(false);let store=self.store.as_mut().unwrap();
        pollster::block_on(store.checkpoint(capture,layer_core::package::session::SessionMetadata {value:Default::default(),profiles:Vec::new()},&cancel)).unwrap();
        pollster::block_on(store.load(Default::default(),&cancel)).unwrap().unwrap()
    }
    pub fn corrupt(&self) {
        use std::io::{Read,Seek,SeekFrom,Write};
        let path=std::fs::read_dir(self.path.join("packs")).unwrap().map(Result::unwrap)
            .find(|entry|entry.file_type().unwrap().is_file()).unwrap().path();
        let mut file=std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
        let mut byte=[0];file.read_exact(&mut byte).unwrap();byte[0]^=1;
        file.seek(SeekFrom::Start(0)).unwrap();file.write_all(&byte).unwrap();file.sync_data().unwrap();
    }
}
impl Drop for StoredTiles {
    fn drop(&mut self) {drop(self.store.take());let _=std::fs::remove_dir_all(&self.path);}
}
#[derive(Default)]
pub(crate) struct ReadGate {
    open:std::sync::Mutex<bool>,wake:std::sync::Condvar,
    pub reads:std::sync::atomic::AtomicUsize,
    pub readers:std::sync::Mutex<Vec<std::thread::ThreadId>>,
}
impl ReadGate {
    pub fn close(&self) {*self.open.lock().unwrap()=false;}
    pub fn release(&self) {*self.open.lock().unwrap()=true;self.wake.notify_all();}
    fn wait(&self) {
        self.reads.fetch_add(1,std::sync::atomic::Ordering::AcqRel);
        self.readers.lock().unwrap().push(std::thread::current().id());
        let mut open=self.open.lock().unwrap();while !*open {open=self.wake.wait(open).unwrap();}
    }
    pub fn entered(&self) {
        use std::sync::atomic::Ordering;
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while self.reads.load(Ordering::Acquire)==0 {
            assert!(std::time::Instant::now()<deadline,"decode worker did not request stored source bytes");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
pub(crate) struct GatedStore {
    _store:StoredTiles,
    pub gate:Arc<ReadGate>,
}
impl GatedStore {
    pub fn new(tiles:&layer_core::raster_storage::RetainedTiles,corrupt:bool)->Self {
        let cache=tile_read_cache();
        let gate=Arc::new(ReadGate::default());
        let waiting=gate.clone();
        let store=StoredTiles::with_reads(tiles,cache,Some(Arc::new(move |_,_|waiting.wait())));
        if corrupt {store.corrupt();}
        Self {_store:store,gate}
    }
}
impl Drop for GatedStore {
    fn drop(&mut self) {self.gate.release();}
}

pub(crate) fn corrupt_tile(blob: layer_core::raster::TileBlob) -> layer_core::raster::RasterTile {
    use layer_core::{authored::*, raster::*};
    let tile=RasterTile::backed(blob);let mut artwork=Artwork::new([256;2]).unwrap();
    let (_, SourceTarget::Paint(source))=add_paint(&mut artwork,"Corrupt backing",[256;2]) else {unreachable!()};
    let scalar=tile.descriptor().channels==1;
    use layer_core::color::{SampleDepth,SampleType};
    artwork.compositions.get_mut(artwork.root).unwrap().color.depth=match (tile.descriptor().sample,tile.descriptor().bits_per_channel) {
        (SampleType::Unsigned,8)=>SampleDepth::U8,(SampleType::Unsigned,16)=>SampleDepth::U16,
        (SampleType::Float,16)=>SampleDepth::F16,(SampleType::Float,32)=>SampleDepth::F32,_=>panic!("fixture depth"),
    };
    let raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:if scalar {RasterPlane::Mask}else{RasterPlane::Color},coordinate:[0;2]},tile.clone())].into(),watercolor:None});
    if scalar {
        let coverage=artwork.coverage.next_handle();let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,[256;2],Default::default());mask.source.raster=raster;
        artwork.coverage.insert(PortableId::random(),mask.source).unwrap();let owner=artwork.occurrences.iter().next().unwrap().0;artwork.occurrences.get_mut(owner).unwrap().mask=Some(mask.use_);
    } else {artwork.paint.get_mut(source).unwrap().raster=raster;}

    let editor=layer_core::Editor::new(layer_core::Document::from_artwork(artwork).unwrap());
    let store=StoredTiles::new(&editor.retained_tiles(),tile_read_cache());store.corrupt();tile
}

/// Stores `selection` in a mask through the renderer's mask Coverage
/// operation. Stored tiles keep precedence over the materialized pages.
pub(crate) fn materialize_mask(source: &mut layer_core::authored::CoverageSource, selection: layer_core::Selection, color: layer_core::color::DocumentColor) {
    use layer_core::{authored::*, raster::*, CoverageSnapshot, RasterOperation, RasterOperationKind};
    let mut artwork = Artwork::new(source.domain).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    let (owner, _) = add_paint(&mut artwork, "Mask owner", source.domain);
    let mut coverage = CoverageSnapshot::reveal_all(artwork.coverage.next_handle(), source.domain, [0, 0]);
    coverage.source.default_coverage = source.default_coverage;
    coverage.selection = Some(selection);
    let operation = RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind: RasterOperationKind::Coverage };
    let damage = operation.bounds(source.domain);
    let raster = RasterRevision::pending();
    let pending = CoverageSource { raster: raster.clone(), operations: Arc::new(vec![operation]), ..source.clone() };
    let handle = artwork.coverage.insert(PortableId::random(), pending).unwrap();
    artwork.occurrences.get_mut(owner).unwrap().mask = Some(MaskUse { source: handle, enabled: true, linked: true, inverted: false, offset: [0, 0] });
    let document = layer_core::Document::from_artwork(artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let batch = DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
        ..dab_batch(SourceTarget::Coverage(handle), crate::tests::test_style(BrushExecution::Dry), damage) };
    submit_ready(&mut r, FramePacket { dab_batches: std::slice::from_ref(&batch), reset_layers: true, view: view([64; 2]),
        ..packet(document.scene(), source.domain) }).unwrap();
    let mut tiles = raster.wait_data().unwrap().tiles.clone();
    if let Some(Ok(stored)) = source.raster.try_data() { tiles.extend(stored.tiles.iter().map(|(key, tile)| (*key, tile.clone()))); }
    source.raster = RasterRevision::backed(RasterData { tiles, watercolor: None });
}

pub(crate) fn view(extent: [u32; 2]) -> ViewState {
    ViewState {
        width_px: extent[0],
        height_px: extent[1],
        document_to_surface: [1., 0., 0., 1., 0., 0.],
    }
}

pub(crate) fn packet(scene: SceneView<'_>, extent: [u32; 2]) -> FramePacket<'_> {
    FramePacket {
        commit_rasters: true,
        restore_rasters: &[],
        time_seconds: 0.,
        view: view(extent),
        document_extent: extent,
        scene,
        selection_overlays: None,
        inspect_mask: None,
        dabs: &[],
        dab_batches: &[],
        reset_layers: false,
        composite_all: true,
        blend_space: Default::default(),
    }
}

pub(crate) fn dab_batch(target: SourceTarget, style: DabStyle, damage: layer_core::Rect) -> DabBatch {
    DabBatch {
        preview_end: None,
        material_update: 0,
        stroke_id: layer_core::StrokeId(1),
        target,
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: true,
        first_dab: 0,
        dab_count: 1,
        style,
        damage,
    }
}

/// The averaged inputs a reduced-resolution dry prediction evaluates against,
/// gathered from `neighborhood` as encoding does.
pub(crate) fn dry_display(r: &mut WgpuRasterizer, batch: &DabBatch, neighborhood: [&wgpu::TextureView; 9], output: &resident::RasterWrite<'_, PageSurface>)
    -> Option<crate::dry_material::DisplayJob> {
    if !r.dry_display_gathers(batch) { return None; }
    if r.dry_display_inputs.is_empty() {
        let inputs = crate::dry_material::DisplayInputs::new(&r.device);
        r.dry_display_inputs.push(inputs);
    }
    let inputs = &r.dry_display_inputs[0];
    Some(crate::dry_material::DisplayJob { gather: inputs.gather(&r.device, &r.style_buffer, r.pipelines.dry_material.in_place(), r.pipelines.dry_material.gather_layout(), output), inputs: inputs.inputs(r, neighborhood) })
}

pub(crate) fn dry_output(r: &WgpuRasterizer, write: &resident::RasterWrite<'_, PageSurface>) -> wgpu::BindGroup {
    let changed = r.pipelines.dry_material.in_place().then(|| &r.changed_cells.as_ref().unwrap().disabled);
    r.pipelines.dry_material.output(&r.device, &r.style_buffer, write, None, changed)
}

/// Dry kernels as a device without read-write storage builds them.
pub(crate) fn write_only_dry_material(r: &mut WgpuRasterizer) {
    Arc::make_mut(&mut r.pipelines).dry_material = crate::dry_material::Pipelines::new(&r.device,
        [&r.target_layout, &r.material_layout, &r.advanced_texture_layout], false, &crate::dry_material::output_layouts(&r.device, false));
}

pub(crate) fn evict_published_pages(r: &mut WgpuRasterizer) {
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    r.evict_raster(u64::MAX, &[], &[], &mut encoder);
    encoder.submit(&r.queue);
    complete(r);
}

pub(crate) fn evict_sources(r: &mut WgpuRasterizer) {
    r.end_frame_memory();
    r.scene_tiers.display.heap().evict(u64::MAX, crate::residency::Class::Authored);
    r.refresh_memory_limits();
    complete(r);
    r.device.retirement_handle().flush().unwrap();
}

pub(crate) fn complete(r: &WgpuRasterizer) { complete_device(&r.device); }

thread_local! { static PACE: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([crate::frame_pace::MOVING_TAPS, crate::frame_pace::IDLE_TAPS]) }; }

pub(crate) fn with_pace<T>(taps: [u64; 2], run: impl FnOnce() -> T) -> T {
    let previous = PACE.replace(taps);
    let result = run();
    PACE.set(previous);
    result
}

pub(crate) fn pace(r: &mut WgpuRasterizer) { r.frame_pace.pin(0., PACE.get()); }

pub(crate) fn work(motion: bool) -> crate::scene::WorkBudget {
    crate::scene::Scene::begin_paced(motion, PACE.get()[usize::from(!motion)])
}

pub(crate) fn complete_device(device: &crate::PipelineDevice) {
    #[cfg(not(target_arch="wasm32"))]
    device.wait_queue().unwrap();
    #[cfg(target_arch="wasm32")]
    device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(READBACK_TIMEOUT) })
        .unwrap();
}

pub(crate) fn color_sample(r: &mut WgpuRasterizer, request: layer_render::ColorSampleRequest) -> layer_render::ColorSample {
    assert!(r.request_color_sample(request).unwrap());
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        complete(r);
        if let Some(sample) = r.take_color_sample() { return sample.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "color sample must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Submit `frame` once its dependencies are ready, as the engine does.
pub(crate) fn flush_engine(engine: &mut layer_engine::CanvasEngine<WgpuRasterizer>) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        engine.backend().device().poll(wgpu::PollType::Poll).unwrap();
        engine.render_frame().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
}
pub(crate) fn submit_ready(r: &mut WgpuRasterizer, frame: FramePacket<'_>) -> Result<(), GpuRasterError> {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    let preview = r.transform_preview.clone();
    loop {
        r.poll_pending(frame.view)?;
        pace(r);
        if r.can_submit() && let std::task::Poll::Ready(result) = r.submit_when_ready(frame, preview.as_ref()) { return result; }
        assert!(std::time::Instant::now() < deadline, "frame dependencies must become ready");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Submit `frame`, then idle frames as the engine does, until it publishes.
pub(crate) fn submit_published(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    submit_published_observing(r, frame, |_| {});
}

pub(crate) fn submit_published_observing(r: &mut WgpuRasterizer, frame: FramePacket<'_>, mut observe: impl FnMut(&WgpuRasterizer)) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let previews: Vec<_> = frame.dab_batches.iter().filter(|batch| batch.kind == DabBatchKind::Preview).cloned().collect();
    let idle = FramePacket { composite_all: false, reset_layers: false, dabs: if previews.is_empty() { &[] } else { frame.dabs }, dab_batches: &previews, restore_rasters: &[], ..frame };
    submit_ready(r, frame).unwrap();
    observe(r);
    while crate::scene::scale::tests::publication_pending(&r) {
        assert!(std::time::Instant::now() < deadline, "deferred composition must publish");
        r.wait_idle().unwrap();
        submit_ready(r, idle).unwrap();
        observe(r);
    }
}

pub(crate) fn edit_working_layer(r: &mut WgpuRasterizer, frame: FramePacket<'_>, target: SourceTarget) {
    let mut still = crate::layer_tests::dab([0.; 4]);
    still.flow = 0.;
    let batch = DabBatch { stroke_end: true, ..dab_batch(target, crate::tests::test_style(layer_core::BrushExecution::Dry), still.bounds()) };
    submit_settled(r, FramePacket { dabs: std::slice::from_ref(&still), dab_batches: std::slice::from_ref(&batch), composite_all: false, reset_layers: false, ..frame });
}

pub(crate) fn submit_settled(r: &mut WgpuRasterizer, frame: FramePacket<'_>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let idle = FramePacket { dabs: &[], dab_batches: &[], restore_rasters: &[],
        reset_layers: false, composite_all: false, ..frame };
    let installed = r.preparation_wake.clone();
    let (signal, woken) = std::sync::mpsc::channel();
    let forward = installed.clone();
    r.set_preparation_wake(Arc::new(move || { if let Some(wake) = &forward { wake(); } let _ = signal.send(()); }));
    let mut first = true;
    loop {
        r.poll_startup().unwrap();
        r.poll_pending(frame.view).unwrap();
        let next = if first { frame } else { idle };
        let preview = r.transform_preview.clone();
        pace(r);
        if r.can_submit() && let std::task::Poll::Ready(result) = r.submit_when_ready(next, preview.as_ref()) {
            result.unwrap();
            first = false;
        }
        r.wait_idle().unwrap();
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!remaining.is_zero(), "renderer work must settle");
        if first { std::thread::sleep(std::time::Duration::from_millis(1)); continue; }
        if r.next_frame() == layer_render::FrameDemand::Work { continue; }
        if r.settled() { break; }
        assert!(woken.recv_timeout(remaining).is_ok(), "an idle renderer waits on inputs whose completion never woke it");
    }
    match installed { Some(wake) => r.set_preparation_wake(wake), None => r.preparation_wake = None }
}

/// Upload every requested worker decode into the decoded-tile cache.
pub(crate) fn settle_decodes(r: &mut WgpuRasterizer) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while r.moving_decode.as_ref().is_some_and(|(_, queue)| queue.pending()) {
        assert!(std::time::Instant::now() < deadline, "requested decodes must arrive");
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        r.drain_image_decodes(&mut encoder).unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn query_ready<T>(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
    mut query: impl FnMut(&mut WgpuRasterizer, &mut crate::submission::CommandEncoder) -> Result<T, GpuRasterError>) -> T {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        r.drain_image_decodes(encoder).unwrap();
        match query(r, encoder) {
            Ok(value) => return value,
            Err(GpuRasterError::NotReady|GpuRasterError::NotPublished) => {},
            Err(error) => panic!("{error}"),
        }
        scene::Scene::submit_chunk(r, encoder, "test query preparation").unwrap();
        assert!(std::time::Instant::now() < deadline, "query preparation must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn thumbnail(r: &mut WgpuRasterizer, id: u64, target: layer_render::ThumbnailTarget) -> ReadbackImage {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while !r.prepare_thumbnail_batch(target).unwrap() {
        complete(r);
        assert!(std::time::Instant::now() < deadline, "thumbnail preparation must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    r.request_thumbnail(id, target).unwrap();
    loop {
        complete(r);
        if let Some(image) = r.take_thumbnail() { return image.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "thumbnail readback must complete");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn document_texture(r: &WgpuRasterizer) -> &wgpu::Texture {
    let cache = r.scale_display.as_ref().unwrap();
    assert_eq!(cache.plan.level, 0);
    assert_eq!(cache.plan.bounds, PixelRect::full(r.document_extent));
    cache.texture()
}

pub(crate) fn receive_request(r: &mut WgpuRasterizer, request: impl Into<layer_render::RegionTask>) -> layer_render::RegionResult {
    assert!(r.request_region(request.into()).unwrap());
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        if let Some(result) = r.take_region() {
            return mask_result(result.unwrap());
        }
        assert!(std::time::Instant::now() < deadline, "region readback timed out");
        std::thread::yield_now();
    }
}

pub(crate) fn mask_result(output:layer_render::RegionOutput)->layer_render::RegionResult {
    let layer_render::RegionOutput::Mask(result)=output else {panic!("expected region mask")};result
}

/// Solve H(u, v) = p for row-major H. None without a nearby preimage where w > 0.
pub(crate) fn preimage(h: [f64; 9], [x, y]: [f64; 2]) -> Option<[f64; 2]> {
    let [a, b, c, d] = [h[0] - x * h[6], h[1] - x * h[7], h[3] - y * h[6], h[4] - y * h[7]];
    let [e, f] = [x * h[8] - h[2], y * h[8] - h[5]];
    let det = a * d - b * c;
    let [u, v] = [(e * d - b * f) / det, (a * f - e * c) / det];
    (det != 0. && h[6] * u + h[7] * v + h[8] > 0. && u.abs() < 1e9 && v.abs() < 1e9).then_some([u, v])
}

pub(crate) fn page_texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test page"),
        size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

pub(crate) fn upload_layer_page(r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, coordinate: [u32; 2], bytes: &[u8]) {
    let intent = r.raster_intent(target, layer_core::raster::RasterPlane::Color, coordinate).unwrap();
    let expected = r.raster_version_for(scene, target, layer_core::raster::RasterPlane::Color, coordinate).unwrap();
    assert_eq!(intent.version().matches(&expected), Some(true));
    let name = layer_core::raster::ContentName::fresh();
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    let receipt = encoder.submission();
    let page = r.paint_layers.iter_mut().find(|layer| layer.id == target).unwrap().pages.iter_mut()
        .find(|page| page.coordinate == coordinate).unwrap();
    let write = page.active_mut().write(&intent, &receipt).unwrap();
    upload_written_page(&mut r.uploads, &mut encoder, &write, bytes);
    let written = write.encoded(name).publish(&encoder.submission());
    r.record_raster_write(target, written).unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
}

pub(crate) fn upload_written_page(uploads: &mut Uploads, encoder: &mut crate::submission::CommandEncoder,
    output: &resident::RasterWrite<'_, PageSurface>, bytes: &[u8]) {
    let texture = &output.value().texture;
    let bytes_per_row = texture.width() * texture.format().block_copy_size(None).unwrap();
    assert_eq!(bytes.len() as u64, u64::from(bytes_per_row) * u64::from(texture.height()));
    uploads.write_texture(encoder, texture, bytes_per_row, |mapped| mapped.copy_from_slice(bytes)).unwrap();
}

pub(crate) fn upload_page(r: &WgpuRasterizer, texture: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(PAGE_SIZE * texture.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        texture.size(),
    );
}

pub(crate) fn pen(sequence: u64, phase: layer_engine::PenPhase, [x, y]: [f32; 2], flags: layer_engine::SampleFlags) -> layer_engine::PenEvent {
    use layer_engine::{PenEvent, ToolKind};
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: sequence * 8_000_000,
        view_revision: 0,
        surface_position: layer_core::Point { x, y },
        pressure: 1.,
        tilt_radians: [0.; 2],
        twist_radians: 0.,
        distance: 0.,
        phase,
        tool: ToolKind::Pen,
        flags,
    }
}

pub(crate) fn depth_source(extent: [u32; 2], depth: layer_core::color::SampleDepth, space: layer_core::color::RgbSpace,
    max_bytes: usize, pixel: impl Fn(u32, u32) -> [f32; 4]) -> Arc<layer_core::color::source::SourceImage> {
    use layer_core::color::{SampleDepth, ColorProfile, source::*};
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth, profile: ColorProfile::Builtin(space), profile_assumed: false,
    }, max_bytes).unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            for value in pixel(x, y) {
                match depth {
                    SampleDepth::U8 => row.push((value.clamp(0., 1.) * 255.).round() as u8),
                    SampleDepth::U16 => row.extend_from_slice(&((value.clamp(0., 1.) * 65535.).round() as u16).to_le_bytes()),
                    SampleDepth::F16 => row.extend_from_slice(&layer_core::color::f16::from_f32(value).to_bits().to_le_bytes()),
                    SampleDepth::F32 => row.extend_from_slice(&value.to_le_bytes()),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

pub(crate) fn floats(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .as_chunks::<16>().0.iter()
        .map(|p| std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap())))
        .collect()
}

pub(crate) fn float_pixels(r: &WgpuRasterizer, texture: impl Into<crate::source_access::TileTexture>) -> Vec<[f32; 4]> {
    let texture=&texture.into();
    let bytes=crate::layer_tests::page_bytes(r,texture);
    if texture.format()==wgpu::TextureFormat::Rgba16Float {
        bytes.as_chunks::<8>().0.iter().map(|pixel|std::array::from_fn(|channel|
            layer_core::color::f16::from_bits(u16::from_le_bytes(pixel[channel*2..channel*2+2].try_into().unwrap())).to_f32())).collect()
    } else {floats(&bytes)}
}

pub(crate) fn blend_space_pixels(r:&mut WgpuRasterizer)->Vec<[f32;4]> {
    let frame=r.artwork_frame.clone().unwrap();
    let extent=r.document_extent;
    let mut scene=scene::Scene::new(r).unwrap();
    let (texture,_)=create_target(&r.device,[PAGE_SIZE;2],r.device.working_format(),"blend-space oracle");
    let mut pixels=vec![[0.;4];extent[0] as usize*extent[1] as usize];
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let region=page_rect(coordinate).intersect(PixelRect::full(extent));
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        query_ready(r,&mut encoder,|r,encoder|scene.capture_region_prepared(r,frame.packet(extent),&texture,region,scene::Output::Display,encoder));
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        let samples=float_pixels(r,&texture);
        for y in 0..region.height() {
            let row=(region.min_y()+y) as usize*extent[0] as usize+region.min_x() as usize;
            let local=y as usize*PAGE_SIZE as usize;
            pixels[row..row+region.width() as usize].copy_from_slice(&samples[local..local+region.width() as usize]);
        }
    }
    pixels
}

pub(crate) fn prepare_scene_effects(r:&mut WgpuRasterizer,frame:FramePacket<'_>) {
    let mut scene=r.scene.take().unwrap_or_else(||scene::Scene::new(r).unwrap());
    for (layers,execution) in scene::effect_chains(frame.scene,scene::EffectPreparation::Display) {
        scene.effects.prepare(r,frame.scene,&layers,execution,frame.time_seconds,0,frame.blend_space).unwrap();
    }
    pollster::block_on(scene.effects.needs().prepare(&r.compilation)).unwrap();
    r.scene=Some(scene);
}

pub(crate) fn warm_presentation(r: &WgpuRasterizer, format: wgpu::TextureFormat, color: SdrSurfaceColor, view: ViewState) {
    let mut presenter=ViewportPresenter::for_surface(r, format, color).unwrap();
    pollster::block_on(presenter.prepare(r)).unwrap();
    let (_texture, target)=create_target(&r.device, [view.width_px, view.height_px], format, "warm presentation");
    presenter.present(r, &target, view, [0.; 4]).unwrap();
    let deadline=std::time::Instant::now()+READBACK_TIMEOUT;
    while crate::compile::tests::queued_recipes(&r.compilation)!=0 {
        r.compilation.check().unwrap();
        assert!(std::time::Instant::now()<deadline, "presentation recipes finish warming");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

pub(crate) fn float_presenter(r: &WgpuRasterizer) -> ViewportPresenter {
    let mut presenter=ViewportPresenter::for_surface(r, wgpu::TextureFormat::Rgba32Float, SdrSurfaceColor::ExtendedLinearSrgb).unwrap();
    pollster::block_on(presenter.prepare(r)).unwrap();
    presenter
}

pub(crate) fn max_error(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    a.iter().flatten().zip(b.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max)
}

pub(crate) const DISPLAY_HALF_ULP: f32 = 1. / 2048.;
const HALF_MIN_NORMAL: f32 = 1. / 16384.;

pub(crate) fn max_relative_error(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    a.iter().flatten().zip(b.iter().flatten()).map(|(a, b)| (a - b).abs() / a.abs().max(b.abs()).max(HALF_MIN_NORMAL)).fold(0., f32::max)
}

pub(crate) fn max_error_bytes(a: &[u8], b: &[u8]) -> f32 {
    a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0.iter())
        .map(|(a, b)| (f32::from_le_bytes(*a) - f32::from_le_bytes(*b)).abs())
        .fold(0., f32::max)
}

pub(crate) fn staged_renderer(reference: &WgpuRasterizer, color: layer_core::color::DocumentColor) -> WgpuRasterizer {
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(
        reference.adapter.clone(), reference.device().clone(), reference.queue.clone(), color).unwrap();
    renderer.finish_startup_catalog();
    renderer
}

pub(crate) fn wait_startup(renderer: &mut WgpuRasterizer, deadline: std::time::Instant,
    ready: impl Fn(StartupProgress) -> bool, message: std::fmt::Arguments<'_>) {
    loop {
        let progress = renderer.poll_startup().unwrap();
        if ready(progress) { return; }
        assert!(std::time::Instant::now() < deadline, "{message}: {progress:?}, pending jobs: {}",
            renderer.startup.as_ref().map_or(0, |s| s.compiler.pending()));
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Both renderers' canonical composites agree in every working F32 sample.
pub(crate) fn assert_same_canonical(actual: &mut WgpuRasterizer, expected: &mut WgpuRasterizer) {
    let bits = |r: &mut WgpuRasterizer| canonical_pixels(r).into_iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
    let (actual, expected) = (bits(actual), bits(expected));
    let differing = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
    let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
    assert!(actual.len() == expected.len() && differing == 0, "{differing} canonical samples differ, first at {first:?}");
}

pub(crate) fn canonical_pixels(r: &mut WgpuRasterizer) -> Vec<[f32; 4]> {
    let frame = r.artwork_frame.clone().unwrap();
    let extent = r.document_extent;
    let mut capture = crate::artwork::Capture::default();
    let mut pixels = vec![[0.; 4]; extent[0] as usize * extent[1] as usize];
    for coordinate in page_coordinates(PixelRect::full(extent)) {
        let region = page_rect(coordinate).intersect(PixelRect::full(extent));
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let tile = query_ready(r, &mut encoder, |r, encoder| capture.region(r, frame.packet(extent), region, [PAGE_SIZE; 2], encoder));
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let samples = float_pixels(r, &tile.texture);
        for y in 0..region.height() {
            let row = (region.min_y() + y) as usize * extent[0] as usize + region.min_x() as usize;
            let local = y as usize * PAGE_SIZE as usize;
            pixels[row..row + region.width() as usize].copy_from_slice(&samples[local..local + region.width() as usize]);
        }
    }
    pixels
}

pub(crate) fn prepare_lit_presenter(r: &WgpuRasterizer, presenter: &mut crate::ViewportPresenter) {
    pollster::block_on(presenter.prepare(r)).unwrap();
}

#[derive(Clone)]
pub(crate) struct PublishedCoverage {pub plan:display_mips::Plan,pub receipt:usize,pub pages:Vec<usize>,pub written:Vec<u32>}
std::thread_local! {static PUBLISHED_COVERAGE:std::cell::RefCell<std::collections::HashMap<u64,Vec<(crate::submission::Submission,PublishedCoverage)>>>=std::cell::RefCell::new(std::collections::HashMap::new());}
pub(crate) fn coverage_hash<T:std::hash::Hash>(value:&T)->u64 {
    let mut hasher=std::collections::hash_map::DefaultHasher::new();std::hash::Hash::hash(value,&mut hasher);std::hash::Hasher::finish(&hasher)
}
pub(crate) fn published_coverage(geometry:u64)->Option<PublishedCoverage> {
    PUBLISHED_COVERAGE.with(|map|map.borrow().get(&geometry)?.iter().rev().find(|(submission,_)|!submission.discarded()).map(|(_,value)|value.clone()))
}
pub(crate) fn record_published_coverage(geometry:u64,submission:&crate::submission::Submission,value:PublishedCoverage) {
    PUBLISHED_COVERAGE.with(|map|{
        let mut map=map.borrow_mut();let published=map.entry(geometry).or_default();
        let kept=published.drain(..).rfind(|(earlier,_)|!earlier.discarded() && !earlier.same(submission));
        published.extend(kept);published.push((submission.clone(),value));
    });
}
pub(crate) fn storm_surface_area(rect:PixelRect,view:ViewState)->f64 {
    let [a,b,c,d,tx,ty]=view.document_to_surface.map(f64::from);
    let [x0,y0,x1,y1]=[rect.min_x(),rect.min_y(),rect.max_x(),rect.max_y()].map(f64::from);
    let mut polygon=vec![[x0,y0],[x1,y0],[x1,y1],[x0,y1]].into_iter()
        .map(|[x,y]|[a*x+c*y+tx,b*x+d*y+ty]).collect::<Vec<_>>();
    for (axis,bound,upper) in [(0,0.,false),(0,f64::from(view.width_px),true),(1,0.,false),(1,f64::from(view.height_px),true)] {
        if polygon.is_empty() {return 0.;}
        let inside=|point:[f64;2]|if upper {point[axis]<=bound}else{point[axis]>=bound};
        let mut clipped=Vec::new();let mut previous=*polygon.last().unwrap();
        for point in polygon {
            if inside(previous)!=inside(point) {
                let t=(bound-previous[axis])/(point[axis]-previous[axis]);
                clipped.push([previous[0]+t*(point[0]-previous[0]),previous[1]+t*(point[1]-previous[1])]);
            }
            if inside(point) {clipped.push(point);}
            previous=point;
        }
        polygon=clipped;
    }
    polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()).map(|(a,b)|a[0]*b[1]-a[1]*b[0]).sum::<f64>().abs()*0.5
}

fn bounded_work_timestamp_bytes(r:&WgpuRasterizer,result:&wgpu::Buffer)->Vec<u8> {
    let output=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:result.size(),
        usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    let mut commands=r.device.create_command_encoder(&Default::default());
    commands.copy_buffer_to_buffer(result,0,&output,0,result.size());
    r.queue.submit([commands.finish()]);
    let (send,receive)=std::sync::mpsc::channel();
    output.slice(..).map_async(wgpu::MapMode::Read,move|result| {let _=send.send(result);});
    r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv_timeout(std::time::Duration::from_secs(10)).unwrap().unwrap();
    let bytes=output.slice(..).get_mapped_range().unwrap().to_vec();output.unmap();bytes
}
fn calibration_count(count:u32,calibrated:f64)->u32 {((f64::from(count)*57.5/calibrated).round() as u32).clamp(1,4096)}
pub(crate) fn bounded_outstanding_gpu_work(r:&WgpuRasterizer)->(crate::submission::CommandEncoder,impl Fn()->f64+'_,impl Fn(u32)->wgpu::CommandBuffer+'_ ) {
    let module=r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label:Some("bounded outstanding work"), source:wgpu::ShaderSource::Wgsl(r#"
        @group(0) @binding(0) var<storage,read_write> output:array<u32>;
        @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
            var value=output[id.x]+id.x;
            for(var i=0u;i<4096u;i+=1u) { value=(value^(value>>13u))*1664525u+1013904223u; }
            output[id.x]=value;
        }
        "#.into()),
    });
    let pipeline=r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label:None,layout:None,module:&module,entry_point:Some("main"),
        compilation_options:Default::default(),cache:None,
    });
    let storage=r.device.create_buffer(&wgpu::BufferDescriptor {
        label:None,size:65536*4,usage:wgpu::BufferUsages::STORAGE,mapped_at_creation:false,
    });
    let bind=r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:None,
        layout:&pipeline.get_bind_group_layout(0),entries:&[wgpu::BindGroupEntry {
        binding:0,resource:storage.as_entire_binding(),
        }],
    });
    let queries=r.device.create_query_set(&wgpu::QuerySetDescriptor {
        label:None,ty:wgpu::QueryType::Timestamp,count:2,
    });
    let result=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:16,
        usage:wgpu::BufferUsages::QUERY_RESOLVE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:false});
    let record=|count:u32| {
        let mut e=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        { let mut pass=e.begin_compute_pass(&wgpu::ComputePassDescriptor {label:None,
        timestamp_writes:Some(wgpu::ComputePassTimestampWrites {query_set:&queries,
            beginning_of_pass_write_index:Some(0),end_of_pass_write_index:Some(1)})});
        pass.set_pipeline(&pipeline);pass.set_bind_group(0,&bind,&[]);
        for _ in 0..count {pass.dispatch_workgroups(1024,1,1);}
        }
        e.resolve_query_set(&queries,0..2,&result,0);
        e
    };
    let duration=|r: &WgpuRasterizer| {
        let raw=bounded_work_timestamp_bytes(r,&result);
        let start=u64::from_le_bytes(raw[..8].try_into().unwrap());
        let end=u64::from_le_bytes(raw[8..16].try_into().unwrap());
        (end-start) as f64*r.queue.get_timestamp_period() as f64/1e6
    };
    let mut count=128;
    let mut calibrated=0.;
    for _ in 0..4 {
        record(count).submit(&r.queue);
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        calibrated=duration(&r);
        if (40. ..=75.).contains(&calibrated) {break;}
        count=calibration_count(count,calibrated);
    }
    assert!((40. ..=75.).contains(&calibrated),"bounded calibration: {calibrated} ms, {count} dispatches");
    let work=record(count);
    let raw=move|divisor:u32| {
        let mut e=r.device.create_command_encoder(&Default::default());
        {let mut pass=e.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&pipeline);pass.set_bind_group(0,&bind,&[]);
            for _ in 0..(count/divisor).max(1) {pass.dispatch_workgroups(1024,1,1);}
        }
        e.finish()
    };
    (work,move|| {
        let raw=bounded_work_timestamp_bytes(r,&result);
        let start=u64::from_le_bytes(raw[..8].try_into().unwrap());
        let end=u64::from_le_bytes(raw[8..16].try_into().unwrap());
        (end-start) as f64*r.queue.get_timestamp_period() as f64/1e6
    },raw)
}
