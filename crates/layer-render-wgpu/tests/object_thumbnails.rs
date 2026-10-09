//! Image rows preview each object's own image in bounded batches, independent
//! of its placement and visibility, and share work between users of an image.
mod support;
use layer_core::*;
use layer_core::authored::{Affine64, Image, ImageObject};
use layer_render::{CanvasRenderer, ThumbnailTarget};
use support::*;

fn halves(extent: [u32; 2]) -> Image {
    color::source::rgba8_source(extent, |x, _| if x < extent[0] / 2 { [230, 20, 20, 255] } else { [20, 20, 230, 255] }).into()
}

fn pixel(bytes: &[u8], [x, y]: [usize; 2]) -> [u8; 4] {
    bytes[(y * 32 + x) * 4..][..4].try_into().unwrap()
}

#[test]
fn image_layer_rows_preview_each_pose_regardless_of_visibility() {
    let mut doc = named_document(&["Ink"], SIZE, BlendSpace::Linear);
    let image = halves([1024, 512]);
    let mut layers = Vec::new();
    for (index, (affine, visible)) in [([0., 0.2, -0.2, 0., 300., 40.], true), ([-0.1, 0., 0., 0.1, 200., 50.], false), ([-0.1, 0., 0., 0.1, 5000., -900.], false)].into_iter().enumerate() {
        let mut object = ImageObject::new(image.clone());
        object.affine = Affine64(affine);
        let (layer, edit) = doc.create_object_layer_edit("Photo", object, None, index).unwrap();
        doc.apply(edit).unwrap();
        doc.artwork.occurrences.get_mut(layer).unwrap().visible = visible;
        layers.push(layer);
    }
    let (mut engine, _) = engine(doc);
    engine.render_frame_at(1).unwrap();
    let (_, rotated) = layer_thumbnail(&mut engine, ThumbnailTarget::Occurrence(layers[0]));
    let red = pixel(&rotated, [16, 8]);
    let blue = pixel(&rotated, [16, 24]);
    assert!(red[0] > 150 && red[2] < 80, "the rotated source left half appears above: {red:?}");
    assert!(blue[2] > 150 && blue[0] < 80, "the rotated source right half appears below: {blue:?}");
    assert_ne!(pixel(&rotated, [2, 16]), red, "the tall rotated image is letterboxed");
    let (_, hidden) = layer_thumbnail(&mut engine, ThumbnailTarget::Occurrence(layers[1]));
    let blue=pixel(&hidden,[8,16]);let red=pixel(&hidden,[24,16]);
    assert!(blue[2]>150 && blue[0]<80,"hidden mirrored source previews on the left: {blue:?}");
    assert!(red[0]>150 && red[2]<80,"hidden mirrored source previews on the right: {red:?}");
    assert_ne!(rotated,hidden,"different poses have different occurrence thumbnails");
    let (_, outside)=layer_thumbnail(&mut engine,ThumbnailTarget::Occurrence(layers[2]));
    assert!(outside.chunks_exact(4).all(|pixel|pixel[0]==pixel[1] && pixel[1]==pixel[2]),"an ordinary row shows no pixels for a placement wholly outside the canvas");
    let scene=engine.document().scene();
    assert!(std::sync::Arc::ptr_eq(scene.object_layer(layers[0]).unwrap().image.storage(),scene.object_layer(layers[1]).unwrap().image.storage()));
    let object=scene.object_handle(layers[0]).unwrap();
    assert_eq!(ThumbnailTarget::from_wire_id(object.wire_id()),None,"object records are source identities, not thumbnail rows");
    let missing = ThumbnailTarget::Occurrence(authored::OccurrenceHandle::from_index(99));
    assert!(engine.backend_mut().prepare_thumbnail_batch(missing).is_err());
    assert_eq!(ThumbnailTarget::from_wire_id(layers[0].index() as u64+1), Some(ThumbnailTarget::Occurrence(layers[0])));
}

fn layer_thumbnail(engine: &mut Engine, target: ThumbnailTarget) -> (usize, Vec<u8>) {
    let mut batches = 1;
    while !engine.backend_mut().prepare_thumbnail_batch(target).unwrap() {
        engine.backend_mut().wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(16));
        batches += 1;
        assert!(batches < 64, "object layer thumbnails complete in bounded batches");
    }
    engine.backend_mut().request_thumbnail(9, target).unwrap();
    engine.backend_mut().wait_idle().unwrap();
    (batches, engine.backend_mut().take_thumbnail().unwrap().unwrap().bytes)
}

#[test]
fn object_layer_rows_preview_placed_content_from_one_coarse_image() {
    let mut doc = named_document(&["Ink"], [4096, 2048], BlendSpace::Linear);
    let mut layers = Vec::new();
    for visible in [true, false] {
        let mut object = ImageObject::new(halves([1024, 512]));
        object.affine = Affine64([2., 0., 0., 2., 1024., 512.]);
        let (layer, edit) = doc.create_object_layer_edit("Photo", object, None, 0).unwrap();
        doc.apply(edit).unwrap();
        doc.artwork.occurrences.get_mut(layer).unwrap().visible = visible;
        layers.push(layer);
    }
    let (mut engine, _) = engine(doc);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut clock = 1;
    loop {
        engine.render_frame_at(clock).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.backend().has_pending_work() { break; }
        assert!(std::time::Instant::now() < deadline, "the visible layer settles");
        clock += 8_000_000;
    }
    let (batches, visible) = layer_thumbnail(&mut engine, ThumbnailTarget::Occurrence(layers[0]));
    assert_eq!(batches, 1, "a visible layer previews its objects from their prepared image levels");
    let (batches, hidden) = layer_thumbnail(&mut engine, ThumbnailTarget::Occurrence(layers[1]));
    assert!(batches < 8, "a hidden layer evaluates one coarse image instead of every document page: {batches}");
    for bytes in [&visible, &hidden] {
        let red = pixel(bytes, [8, 16]);
        let blue = pixel(bytes, [24, 16]);
        assert!(red[0] > 150 && red[2] < 80, "the placed image fills the framed row: {red:?}");
        assert!(blue[2] > 150 && blue[0] < 80, "the placed image fills the framed row: {blue:?}");
        assert_ne!(pixel(bytes, [16, 2]), red, "a wide placement is letterboxed");
    }
}
