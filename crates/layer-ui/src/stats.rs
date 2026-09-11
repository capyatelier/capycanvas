//! Compact diagnostics, labels and summaries shared by native hosts.
use layer_render::RendererTelemetry;
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct StatRow {
    pub label: &'static str,
    pub value: String,
    pub description: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct StatsView {
    pub rows: Vec<StatRow>,
    pub samples: Vec<f32>,
    pub budget_ms: f32,
    pub chart_label: &'static str,
    /// Insert the chart after this many metric rows on every frontend.
    pub chart_after_rows: usize,
}
pub(super) fn view(t: RendererTelemetry) -> StatsView {
    fn percentiles(values: Vec<f32>) -> String {
        if values.is_empty() {
            return "—".into();
        }
        let mut v = values;
        v.sort_by(f32::total_cmp);
        let p = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
        format!("{:.2} / {:.2} / {:.2}", p(0.5), p(0.95), p(0.99))
    }
    let samples = t.cpu.ordered();
    let rows = vec![
        StatRow {
            label: "CPU · ms",
            value: percentiles(samples.clone()),
            description: "Median / p95 / p99: prepare, encode and submit. Not GPU completion or input latency.",
        },
        StatRow {
            label: "GPU · ms",
            value: if t.gpu_timestamps {
                percentiles(t.gpu.ordered())
            } else {
                "Unavailable".into()
            },
            description: "Median / p95 / p99 GPU execution, including GPU scheduling gaps. Excludes presentation. Browser timestamps may be quantized.",
        },
        StatRow {
            label: "Frames",
            value: t.submissions.to_string(),
            description: "Submitted drawing updates. Cached canvas navigation and compositor frames are not counted.",
        },
        StatRow {
            label: "Canvas storage",
            value: format!("{:.1} MiB", t.resident_bytes as f64 / 1048576.),
            description: "Tracked paint, preview, masks, effect parameters, composite and scene scratch allocation. Excludes driver overhead and imported assets; not total VRAM use.",
        },
        StatRow {
            label: "Dabs",
            value: t.dabs.to_string(),
            description: "Brush dabs submitted since renderer creation.",
        },
        StatRow {
            label: "Effect passes",
            value: t.effect_passes.to_string(),
            description: "GPU effect render passes in the latest drawing update. Compatible chains and tile draws share passes.",
        },
        StatRow {
            label: "Pipelines",
            value: t.compiled_effects.to_string(),
            description: "Cached effect pipelines. Parameter edits do not compile new pipelines.",
        },
    ];
    StatsView {
        rows,
        samples,
        budget_ms: 1000. / 120.,
        chart_label: "CPU render · last 120 updates",
        chart_after_rows: 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_orders_chart_and_storage_with_their_metrics() {
        let view = view(RendererTelemetry::default());
        assert_eq!(
            view.rows.iter().map(|row| row.label).collect::<Vec<_>>(),
            [
                "CPU · ms",
                "GPU · ms",
                "Frames",
                "Canvas storage",
                "Dabs",
                "Effect passes",
                "Pipelines"
            ]
        );
        assert_eq!(view.rows[view.chart_after_rows - 1].label, "GPU · ms");
        assert_eq!(view.rows[view.chart_after_rows].label, "Frames");
        assert_eq!(serde_json::to_value(&view).unwrap()["chart_after_rows"], 2);
    }
}
