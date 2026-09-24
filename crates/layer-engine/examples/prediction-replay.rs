//! Replay collected samples through the production predictor, writing CSV to stdout.
use std::io;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or(
        "usage: prediction-replay TRACE.capystrokes|TRACE.jsonl[.gz] [--frames PREVIEW.jsonl]",
    )?;
    let mut frames = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" if frames.is_none() => {
                frames = Some(args.next().ok_or("--frames requires a JSONL path")?)
            }
            _ => return Err("expected --frames PATH (once)".into()),
        }
    }
    let file = std::fs::File::open(&path)?;
    let input: Box<dyn io::Read> = if path.ends_with(".gz") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let reader = io::BufReader::new(input);
    let csv = io::BufWriter::new(io::stdout().lock());
    let summary = match frames {
        Some(path) => layer_engine::prediction_bench::replay_with_frames(
            reader,
            csv,
            io::BufWriter::new(std::fs::File::create(path)?),
        )?,
        None => layer_engine::prediction_bench::replay(reader, csv)?,
    };
    eprintln!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
