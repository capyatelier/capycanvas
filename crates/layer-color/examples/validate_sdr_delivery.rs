//! Compare a host's SDR delivery to GTK's shared local-tone policy, using its
//! lossless EXR delivery as the edited master. This is a host-parity check, not
//! an independent validation of the shared tone-mapping algorithm.
//! cargo run --release -p layer-color --example validate_sdr_delivery -- MASTER.exr SDR.png RENDITION.json
use layer_core::color::{RgbSpace, hdr::SdrRendition};
use std::{fs::File,io::BufReader};

fn main() -> Result<(),String> {
    let args:Vec<_>=std::env::args().skip(1).collect();
    if args.len()!=3{return Err("Expected MASTER.exr SDR.png RENDITION.json".into())}
    let read=|path:&str|layer_color::photo::read_photo(BufReader::new(File::open(path).map_err(|e|e.to_string())?),Default::default());
    let master=read(&args[0])?;let output=read(&args[1])?;
    if master.extent!=output.extent{return Err("Delivery dimensions differ".into())}
    let recipe:SdrRendition=serde_json::from_reader(File::open(&args[2]).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    recipe.validate().map_err(str::to_string)?;
    let [width,height]=master.extent;
    let decoder=layer_color::WorkingDecoder::new(&master.interpretation,RgbSpace::Srgb,Default::default())?;
    let mut bytes=vec![0;master.row_bytes()];let mut pixels=vec![[0.;4];width as usize*height as usize];
    for (y,row) in pixels.chunks_exact_mut(width as usize).enumerate(){master.rows().read(y as u32,&mut bytes)?;decoder.decode_pixels(&bytes,row)?;}
    let guide=layer_color::build_local_tone_guide(master.extent,RgbSpace::Srgb,||false,|y,row|{row.copy_from_slice(&pixels[y as usize*width as usize..(y as usize+1)*width as usize]);Ok(())})?;
    let mapper=recipe.mapper(RgbSpace::Srgb,RgbSpace::Srgb);
    let decoder=layer_color::WorkingDecoder::new(&output.interpretation,RgbSpace::Srgb,Default::default())?;
    let mut bytes=vec![0;output.row_bytes()];let mut row=vec![[0.;4];width as usize];
    let mut maximum=0f64;let mut sum=0f64;
    for y in 0..height {
        output.rows().read(y,&mut bytes)?;decoder.decode_pixels(&bytes,&mut row)?;
        for (x,p) in row.iter().enumerate() {
            let expected=mapper.map_local_premultiplied(pixels[y as usize*width as usize+x],[x as f32+0.5,y as f32+0.5],&guide);
            if (p[3]-expected[3]).abs()>1./255.{return Err(format!("Alpha differs at {x},{y}"))}
            for c in 0..3 {
                let code=|v:f32,a:f32|if a>0.{RgbSpace::Srgb.encode(f64::from(v/a)).clamp(0.,1.)*255.}else{0.};
                let error=(code(p[c],p[3])-code(expected[c],expected[3])).abs();
                maximum=maximum.max(error);sum+=error;
            }
        }
    }
    println!("{}",serde_json::json!({"pixels":width as u64*height as u64,"max_srgb8_code_error":maximum,"mean_srgb8_code_error":sum/f64::from(width*height*3),"tolerance":2.,"reference":"GTK shared local-tone policy; lossless edited EXR master"}));
    if maximum>2.{return Err("SDR output differs from the GTK mapping policy".into())}
    Ok(())
}
