//! Replay collected samples through the production predictor, writing CSV to stdout.
use std::io;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: prediction-replay TRACE.capystrokes|TRACE.jsonl[.gz] [--frames PREVIEW.jsonl] [--algorithm optimized|previous]")?;
    let mut frames = None;
    let mut algorithm = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" if frames.is_none() => {
                frames = Some(args.next().ok_or("--frames requires a JSONL path")?)
            }
            "--algorithm" if algorithm.is_none() => {
                algorithm = Some(match args.next().as_deref() {
                    Some("optimized") => layer_engine::PredictionAlgorithm::Optimized,
                    Some("previous") => layer_engine::PredictionAlgorithm::Previous,
                    _ => return Err("--algorithm requires optimized or previous".into()),
                })
            }
            _ => {
                return Err(
                    "expected --frames PATH or --algorithm optimized|previous (once each)".into(),
                );
            }
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
    let mut frames = frames
        .map(std::fs::File::create)
        .transpose()?
        .map(io::BufWriter::new);
    let summary = layer_engine::prediction_bench::replay_with_options(
        reader,
        csv,
        frames.as_mut().map(|w| w as &mut dyn io::Write),
        algorithm,
    )?;
    eprintln!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
