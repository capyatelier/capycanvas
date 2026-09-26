//! Dump lossless recording records for analysis/training.
use layer_engine::recording;
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["dump", path] => {
            let records = recording::read(std::fs::File::open(path)?)?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for record in records { serde_json::to_writer(&mut out, &record)?; writeln!(out)?; }
        }
        _ => return Err("usage: stroke-recording dump FILE.capystrokes".into()),
    }
    Ok(())
}
