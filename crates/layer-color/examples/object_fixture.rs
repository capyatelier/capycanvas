use layer_core::{DocumentNames, Editor, EvaluationContext, SourceTarget, authored::{Affine64, Image, ImageInterpolation, ImageObject}, color::SampleDepth};
use std::{fs::File, io::{BufReader, BufWriter, Write}, sync::{Arc, atomic::AtomicBool}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let input = args.first().ok_or("PHOTO.jpg OUTPUT.capy [IMAGE_COUNT] [shared|unshared] [linear|nearest]")?;
    let output = args.get(1).ok_or("OUTPUT.capy is required")?;
    let count: usize = args.get(2).map_or(Ok(4), |value|value.parse())?;
    let shared = match args.get(3).map(String::as_str).unwrap_or("shared") {
        "shared" => true, "unshared" => false, _ => return Err("Use shared or unshared".into()),
    };
    let interpolation = match args.get(4).map(String::as_str).unwrap_or("linear") {
        "linear" => ImageInterpolation::Linear, "nearest" => ImageInterpolation::Nearest, _ => return Err("Use linear or nearest".into()),
    };
    if !(1..=32).contains(&count) { return Err("IMAGE_COUNT must be 1–32".into()); }
    let decode = || layer_color::photo::read_photo(BufReader::new(File::open(input).map_err(|e|e.to_string())?), Default::default());
    let source = decode()?;
    let extent = source.extent;
    let mut document = layer_color::photo_project(source, Default::default(),
        DocumentNames {paint:"Photo".into(),paper:"Paper".into()}, SampleDepth::U8)?;
    let SourceTarget::Paint(paint) = document.working.target.ok_or("Photo paint target is unavailable")? else { return Err("Photo source is not paint".into()); };
    let image = document.artwork.paint.get(paint).and_then(|paint|paint.base.as_ref()).ok_or("Photo base image is unavailable")?.image.clone();
    let (layer, edit) = document.create_object_layer_edit("Images", None, 0)?;
    document.apply(edit)?;
    document.artwork.occurrences.get_mut(layer).ok_or("Object layer is unavailable")?.opacity = 0.35;
    for index in 0..count {
        let source = if shared { image.clone() } else { Image::new(Arc::new(decode()?)) };
        let mut object = ImageObject::new(source, format!("Image {}",index+1));
        object.interpolation = interpolation;
        let phase = index as f64 / count as f64 * std::f64::consts::TAU;
        object.affine = Affine64([0.55,0.,0.,0.55,
            extent[0] as f64 * (0.225 + 0.08 * phase.cos()),
            extent[1] as f64 * (0.225 + 0.08 * phase.sin())]);
        let (_, edit) = document.add_image_object_edit(layer, object, index)?;
        document.apply(edit)?;
    }
    let capture = Editor::new(document.clone()).capture(0, EvaluationContext::default())?;
    let cancelled = AtomicBool::new(false);
    let prepared = layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancelled)?;
    let mut writer = BufWriter::new(File::create(output)?);
    prepared.write(&mut writer, &cancelled)?;
    writer.flush()?;
    println!("{}×{}; {} images; {} image identities; {}",extent[0],extent[1],count,
        document.artwork.images()?.len(),if shared {"shared"} else {"unshared"});
    Ok(())
}
