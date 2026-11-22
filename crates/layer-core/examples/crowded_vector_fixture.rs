//! Hatching under a translucent Tones layer, with the active "Ink" layer between.
//! Usage: crowded_vector_fixture OUTPUT.capy WIDTH HEIGHT PATHS [--paint]
mod capy_fixture;
use std::{fs::File, io::{BufWriter, Write}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [output, width, height, count] = [1, 2, 3, 4].map(|i| args.get(i).cloned().ok_or("Usage: crowded_vector_fixture OUTPUT.capy WIDTH HEIGHT PATHS [--paint]"));
    let (output, width, height, count): (String, u32, u32, u64) = (output?, width?.parse()?, height?.parse()?, count?.parse()?);
    let document = capy_fixture::crowded_vector(width, height, count, args.iter().any(|arg| arg == "--paint"))?;
    let mut writer = BufWriter::new(File::create(output)?);
    capy_fixture::package(document.artwork, capy_fixture::Ids::new(&args[2..].join(" ")), &mut writer)?;
    writer.flush()?;
    Ok(())
}
