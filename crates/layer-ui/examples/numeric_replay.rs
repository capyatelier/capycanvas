use layer_ui::{Localizer, NumericRequest, ToolbarUiRequest, UiLanguage, toolbar_ui};
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead, Write};

#[derive(Deserialize)]
struct Sample {
    boot_ns: u64,
    request: Option<NumericRequest>,
    toolbar_request: Option<ToolbarUiRequest>,
    #[serde(default)]
    previous_float_value: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() { return Err("expected JSONL on stdin".into()); }
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut previous: Option<f64> = None;
    for line in io::stdin().lock().lines() {
        let sample: Sample = serde_json::from_str(&line?)?;
        let result = match (sample.request, sample.toolbar_request) {
            (Some(mut request), None) => {
                if sample.previous_float_value {
                    let value = previous.ok_or("previous value requested before a successful result")?;
                    request.value = f64::from(value as f32);
                }
                match request.resolve() {
                    Ok(resolved) => {
                        previous = Some(resolved.value);
                        let float_value = f64::from(resolved.value as f32);
                        json!({ "boot_ns": sample.boot_ns, "resolved": resolved, "float_value": float_value })
                    }
                    Err(error) => json!({ "boot_ns": sample.boot_ns, "error": error }),
                }
            }
            (None, Some(request @ ToolbarUiRequest::SliderBookmarkValue { .. })) => {
                match toolbar_ui(request, &Localizer::shared(UiLanguage::English)) {
                    Ok(value) => {
                        let value = value.as_f64().ok_or("bookmark result must be numeric")?;
                        previous = Some(value);
                        json!({ "boot_ns": sample.boot_ns, "resolved": { "value": value }, "float_value": f64::from(value as f32) })
                    }
                    Err(error) => json!({ "boot_ns": sample.boot_ns, "error": error }),
                }
            }
            _ => return Err("expected one numeric request or slider_bookmark_value toolbar_request".into()),
        };
        serde_json::to_writer(&mut output, &result)?;
        writeln!(output)?;
    }
    output.flush()?;
    Ok(())
}
