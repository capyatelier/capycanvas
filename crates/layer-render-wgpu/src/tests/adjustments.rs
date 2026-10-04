use super::*;
use layer_core::EffectInstance;
use super::native_effects::{empty_document,insert_effect,mask,refresh};
use layer_core::authored::{PortableId,Occurrence,OccurrenceContent,Stack,PaintSource};

// Keep the established ten-filter baseline stable as the catalog grows.
fn pointwise_baseline() -> [&'static layer_core::EffectDefinition; 10] {
    [
        fixture("curves"),
        fixture("levels"),
        fixture("brightness_contrast"),
        fixture("hue_saturation"),
        fixture("color_balance"),
        fixture("exposure"),
        fixture("vibrance"),
        fixture("black_white"),
        fixture("gradient_map"),
        fixture("posterize"),
    ]
}

#[test]
fn all_effects_incremental_masks_groups_and_clipping_match_full_recomposition() {
    let mut r =
        WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
    let extent=[333,291];
    let mut document=empty_document(extent,Default::default());
    for (i,kind) in pointwise_baseline().into_iter().enumerate() {
        let h=insert_effect(&mut document,EffectInstance::new(kind.program()));
        let occurrence=document.artwork.occurrences.get_mut(h).unwrap();occurrence.clipped=true;occurrence.opacity=0.7;
        if i%2==0 {mask(&mut document,h,0.4);}
    }
    let source=document.artwork.paint.insert(PortableId::random(),PaintSource {domain:extent,original:None,raster:Default::default(),operations:Default::default()}).unwrap();
    let base=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),"Translucent paint")).unwrap();
    let root=document.composition().result;
    document.artwork.stacks.get_mut(root).unwrap().entries.push(base);
    let children=std::mem::take(&mut document.artwork.stacks.get_mut(root).unwrap().entries);
    let stack=document.artwork.stacks.insert(PortableId::random(),Stack {entries:children}).unwrap();
    let group=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"Isolated group")).unwrap();
    document.artwork.stacks.get_mut(root).unwrap().entries.push(group);refresh(&mut document);
    let mut view = test_view();
    view.width_px = 333;
    view.height_px = 291;
    let mut dab = test_dab([255., 150.], [0.8, 0.2, 0.1, 0.65], 1.);
    dab.radii = [45.; 2];
    let batch = crate::test_support::dab_batch(
        layer_core::authored::SourceTarget::Paint(source),
        test_style(BrushExecution::Dry),
        Rect { min: Point { x: 209., y: 104. }, max: Point { x: 301., y: 196. } },
    );
    let render = |r: &mut WgpuRasterizer, all, dabs: &[Dab], batches: &[DabBatch]| {
        r.submit(FramePacket {
            view,
            dabs,
            dab_batches: batches,
            composite_all: all,
            ..packet(document.scene(), [333, 291])
        })
        .unwrap();
        r.readback_srgb_rgba8().unwrap()
    };
    render(&mut r, true, &[], &[]);
    let incremental = render(&mut r, false, &[dab], &[batch]);
    let full = render(&mut r, true, &[], &[]);
    assert_eq!(
        incremental, full,
        "damage crossing a tile boundary must match full composite"
    );
    let p = &full[(150 * 333 + 255) * 4..][..4];
    assert!(
        (160..=170).contains(&p[3]),
        "adjustments and masks preserve base alpha: {p:?}"
    );
    assert_eq!(
        &full[..4],
        &[0; 4],
        "clipped adjustments cannot create coverage"
    );
}
