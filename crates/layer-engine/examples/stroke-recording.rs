//! Convert legacy datasets or dump lossless recording records for analysis/training.
use layer_engine::{
    prediction_bench::{Contact, DatasetHeader},
    recording::{self, Record},
};
use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["dump", path] => {
            let records = recording::read(std::fs::File::open(path)?)?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for record in records { serde_json::to_writer(&mut out, &record)?; writeln!(out)?; }
        }
        ["convert", source, destination] => {
            let file = std::fs::File::open(source)?;
            let input: Box<dyn io::Read> = if source.ends_with(".gz") { Box::new(flate2::read::GzDecoder::new(file)) } else { Box::new(file) };
            let mut lines = io::BufReader::new(input).lines();
            let header: DatasetHeader = serde_json::from_str(&lines.next().ok_or("missing header")??)?;
            if header.format != "capy-pen-dataset" || header.version != 1 { return Err("unsupported legacy dataset".into()); }
            let mut data = Vec::new(); let mut count = 0; let mut contacts = 0; let mut events = 0;
            let mut append = |record| { recording::write_record(&mut data, &record); count += 1; };
            append(Record::Metadata(serde_json::json!({"format":"capy-pen-recording", "version":2, "raw_input":false, "source_metadata":header.metadata, "converted_from":"capy-pen-dataset-v1"}).to_string()));
            for line in lines {
                let contact: Contact = serde_json::from_str(&line?)?;
                contacts += 1; events += contact.events.len();
                append(Record::Begin { id: contact.id, timestamp_ns: 0, policy: contact.policy });
                for event in contact.events { append(Record::Predictor(event)); }
                append(Record::End { cancelled: contact.cancelled, interrupted: false });
            }
            if contacts != header.contacts || events != header.events { return Err("incomplete legacy dataset".into()); }
            recording::write_record(&mut data, &Record::Footer { records: count, duration_ns: 0, reason: recording::StopReason::Manual });
            if data.len() > recording::MAX_BYTES { return Err("converted dataset exceeds size limit".into()); }
            let bytes = recording::compress(&data)?;
            layer_engine::prediction_bench::replay(io::BufReader::new(bytes.as_slice()), io::sink())?;
            std::fs::write(destination, &bytes)?;
            eprintln!("Converted {contacts} contacts, {events} events to {} bytes", bytes.len());
        }
        _ => return Err("usage: stroke-recording dump FILE.capystrokes | convert OLD.jsonl[.gz] NEW.capystrokes".into()),
    }
    Ok(())
}
