//! Compact diagnostics, labels and summaries shared by native hosts.
use layer_render::{RendererTelemetry, MemorySnapshot, ProcessMemoryKind, MemoryPeak, GpuMemoryKind};
use crate::{FluentArgs, MessageId};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct StatRow {
    pub label: String,
    pub value: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct StatsView {
    pub rows: Vec<StatRow>,
    pub samples: Vec<f32>,
    /// Raw execution timings for host qualification; keep presentation separate.
    pub gpu_samples: Vec<f32>,
    pub resident_bytes: u64,
    /// Select › Modify values received and previews drawn since the session
    /// began, for host qualification.
    pub selection_values: u64,
    pub selection_previews: u64,
    pub budget_ms: f32,
    pub chart_label: &'static str,
    /// Insert the chart after this many metric rows on every frontend.
    pub chart_after_rows: usize,
}
fn memory_rows(memory: Option<MemorySnapshot>, localization: &crate::Localizer) -> [StatRow; 3] {
    let age = memory.map_or_else(|| localization.text(MessageId::DIAGNOSTICS_MEMORY_NO_SAMPLE).to_string(), |sample| {
        let mut args = FluentArgs::new();
        args.set("seconds", format!("{:.1}", sample.sampled_at.elapsed().as_secs_f64()));
        localization.format(MessageId::DIAGNOSTICS_MEMORY_AGE, &args)
    });
    let value = |bytes: u64| {
        let mut args = FluentArgs::new();
        args.set("value", format!("{:.1}", bytes as f64 / 1048576.));
        localization.format(MessageId::DIAGNOSTICS_MEMORY_MIB, &args)
    };
    let unavailable = || localization.text(MessageId::DIAGNOSTICS_MEMORY_UNAVAILABLE).to_string();
    let row = |label, value, scope: &str, gpu| {
        let mut args = FluentArgs::new();
        args.set("scope", scope);
        args.set("age", age.as_str());
        StatRow { label: localization.text(label).to_string(), value,
            description: localization.format(if gpu { MessageId::DIAGNOSTICS_MEMORY_GPU_DESCRIPTION } else { MessageId::DIAGNOSTICS_MEMORY_DESCRIPTION }, &args) }
    };
    let process = memory.and_then(|sample| sample.process);
    let (process_label, current_scope) = process.map_or((MessageId::DIAGNOSTICS_MEMORY_PROCESS, MessageId::DIAGNOSTICS_MEMORY_PROCESS_UNAVAILABLE), |process| match process.kind {
        ProcessMemoryKind::Resident => (MessageId::DIAGNOSTICS_MEMORY_RESIDENT_LABEL, MessageId::DIAGNOSTICS_MEMORY_RESIDENT),
        ProcessMemoryKind::PrivateCommit => (MessageId::DIAGNOSTICS_MEMORY_PRIVATE_COMMIT_LABEL, MessageId::DIAGNOSTICS_MEMORY_PRIVATE_COMMIT),
        ProcessMemoryKind::PhysicalFootprint => (MessageId::DIAGNOSTICS_MEMORY_FOOTPRINT_LABEL, MessageId::DIAGNOSTICS_MEMORY_FOOTPRINT),
    });
    let (peak, peak_label, peak_scope) = process.map_or_else(|| (unavailable(), MessageId::DIAGNOSTICS_MEMORY_PEAK, MessageId::DIAGNOSTICS_MEMORY_PEAK_UNAVAILABLE), |process| match process.peak {
        MemoryPeak::Lifetime(bytes) => (value(bytes), MessageId::DIAGNOSTICS_MEMORY_LIFETIME_LABEL, MessageId::DIAGNOSTICS_MEMORY_LIFETIME_PEAK),
        MemoryPeak::Sampled(bytes) => (value(bytes), MessageId::DIAGNOSTICS_MEMORY_SAMPLED_LABEL, MessageId::DIAGNOSTICS_MEMORY_SAMPLED_PEAK),
    });
    let peak_scope = if process.is_some() {
        let mut args = FluentArgs::new();
        let metric = localization.text(process_label);
        args.set("metric", metric.as_ref());
        localization.format(peak_scope, &args)
    } else { localization.text(peak_scope).to_string() };
    let (gpu_value, gpu_label, gpu_scope) = memory.and_then(|sample| sample.gpu).map_or_else(|| (unavailable(), MessageId::DIAGNOSTICS_MEMORY_GPU, MessageId::DIAGNOSTICS_MEMORY_GPU_UNAVAILABLE), |gpu| {
        let mut args = FluentArgs::new();
        args.set("used", format!("{:.1}", gpu.bytes as f64 / 1048576.));
        let id = if let Some(budget) = gpu.budget {
            args.set("budget", format!("{:.1}", budget as f64 / 1048576.));
            MessageId::DIAGNOSTICS_MEMORY_BUDGET
        } else { MessageId::DIAGNOSTICS_MEMORY_NO_BUDGET };
        let (label, scope) = match gpu.kind {
            GpuMemoryKind::VulkanHeap => (MessageId::DIAGNOSTICS_MEMORY_VULKAN_LABEL, MessageId::DIAGNOSTICS_MEMORY_VULKAN),
            GpuMemoryKind::DxgiLocal => (MessageId::DIAGNOSTICS_MEMORY_DXGI_LABEL, MessageId::DIAGNOSTICS_MEMORY_DXGI),
            GpuMemoryKind::MetalAllocated => (MessageId::DIAGNOSTICS_MEMORY_METAL_LABEL, MessageId::DIAGNOSTICS_MEMORY_METAL),
        };
        (localization.format(id, &args), label, scope)
    });
    [
        row(process_label, process.map_or_else(unavailable, |process| value(process.bytes)), &localization.text(current_scope), false),
        row(peak_label, peak, &peak_scope, false),
        row(gpu_label, gpu_value, &localization.text(gpu_scope), true),
    ]
}
fn budget_rows(budget: Option<&layer_render::MemoryBudgetSummary>, localization: &crate::Localizer) -> [StatRow; 4] {
    let age = budget.map_or_else(|| localization.text(MessageId::DIAGNOSTICS_MEMORY_NO_SAMPLE).to_string(), |sample| {
        let mut args = FluentArgs::new();
        args.set("seconds", format!("{:.1}", sample.sampled_at.elapsed().as_secs_f64()));
        localization.format(MessageId::DIAGNOSTICS_MEMORY_AGE, &args)
    });
    let pair = |ram: u64, gpu: u64| {
        let mut args = FluentArgs::new();
        args.set("ram", format!("{:.1}", ram as f64 / 1048576.));
        args.set("gpu", format!("{:.1}", gpu as f64 / 1048576.));
        localization.format(MessageId::DIAGNOSTICS_BUDGET_PAIR, &args)
    };
    let row = |label, scope, bytes: Option<(u64, u64)>| {
        let mut args = FluentArgs::new();
        let scope = localization.text(scope);
        args.set("scope", scope.as_ref());
        args.set("age", age.as_str());
        StatRow { label: localization.text(label).to_string(),
            value: bytes.map_or_else(|| localization.text(MessageId::DIAGNOSTICS_MEMORY_UNAVAILABLE).to_string(), |(ram, gpu)| pair(ram, gpu)),
            description: localization.format(MessageId::DIAGNOSTICS_MEMORY_DESCRIPTION, &args) }
    };
    [
        row(MessageId::DIAGNOSTICS_BUDGET_LIMIT, MessageId::DIAGNOSTICS_BUDGET_LIMIT_DESCRIPTION, budget.map(|b| (b.ram_limit_bytes, b.gpu_limit_bytes))),
        row(MessageId::DIAGNOSTICS_BUDGET_PHYSICAL, MessageId::DIAGNOSTICS_BUDGET_PHYSICAL_DESCRIPTION, budget.map(|b| (b.ram_physical_bytes, b.gpu_physical_bytes))),
        row(MessageId::DIAGNOSTICS_BUDGET_PINNED, MessageId::DIAGNOSTICS_BUDGET_PINNED_DESCRIPTION, budget.map(|b| (b.ram_pinned_bytes, b.gpu_pinned_bytes))),
        row(MessageId::DIAGNOSTICS_BUDGET_DEBT, MessageId::DIAGNOSTICS_BUDGET_DEBT_DESCRIPTION, budget.map(|b| (b.ram_debt_bytes, b.gpu_debt_bytes))),
    ]
}
pub(super) fn view(t: RendererTelemetry, localization: &crate::Localizer) -> StatsView {
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
    let submitted = t.frame_rate.is_some_and(|rate| rate.source == layer_render::FrameRateSource::Submitted);
    let mut rows = vec![
        StatRow {
            label: "CPU · ms".into(),
            value: percentiles(samples.clone()),
            description: "Median / p95 / p99: prepare, encode and submit. Not GPU completion or input latency.".into(),
        },
        StatRow {
            label: "GPU · ms".into(),
            value: if t.gpu_timestamps {
                percentiles(t.gpu.ordered())
            } else {
                "Unavailable".into()
            },
            description: "Median / p95 / p99 GPU execution, including GPU scheduling gaps. Excludes presentation. Browser timestamps may be quantized.".into(),
        },
        StatRow {
            label: localization.text(if submitted { crate::MessageId::DIAGNOSTICS_SUBMITTED_FPS } else { crate::MessageId::DIAGNOSTICS_FPS }).to_string(),
            value: t.frame_rate.map_or_else(|| "—".into(), |rate| format!("{:.0}", rate.fps)),
            description: localization.text(if submitted { crate::MessageId::DIAGNOSTICS_SUBMITTED_FPS_DESCRIPTION } else { crate::MessageId::DIAGNOSTICS_FPS_DESCRIPTION }).to_string(),
        },
        StatRow {
            label: "Frames".into(),
            value: t.submissions.to_string(),
            description: "Submitted drawing updates. Cached canvas navigation and compositor frames are not counted.".into(),
        },
        StatRow {
            label: "Canvas storage".into(),
            value: format!("{:.1} MiB", t.resident_bytes as f64 / 1048576.),
            description: "Tracked paint, preview, proof, masks, effect parameters, composite and scene scratch allocation. Excludes driver overhead and imported assets; not total VRAM use.".into(),
        },
        StatRow {
            label: "Dabs".into(),
            value: t.dabs.to_string(),
            description: "Brush dabs submitted since renderer creation.".into(),
        },
        StatRow {
            label: "Effect passes".into(),
            value: t.effect_passes.to_string(),
            description: "GPU effect render passes in the latest drawing update. Compatible chains and tile draws share passes.".into(),
        },
        StatRow {
            label: "Pipelines".into(),
            value: t.compiled_effects.to_string(),
            description: "Cached effect pipelines. Parameter edits do not compile new pipelines.".into(),
        },
    ];
    rows.extend(memory_rows(t.memory, localization));
    rows.extend(budget_rows(t.memory_budget.as_deref(), localization));
    StatsView {
        rows,
        samples,
        gpu_samples: if t.gpu_timestamps { t.gpu.ordered() } else { Vec::new() },
        resident_bytes: t.resident_bytes,
        selection_values: 0,
        selection_previews: 0,
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
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        let view = view(RendererTelemetry::default(), &localization);
        assert_eq!(
            view.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(),
            [
                "CPU · ms",
                "GPU · ms",
                "FPS",
                "Frames",
                "Canvas storage",
                "Dabs",
                "Effect passes",
                "Pipelines",
                "Process memory",
                "Process peak",
                "GPU memory",
                "Memory limit",
                "Tracked memory",
                "Pinned memory",
                "Memory debt"
            ]
        );
        assert_eq!(view.rows[view.chart_after_rows - 1].label, "GPU · ms");
        assert_eq!(view.rows[view.chart_after_rows].label, "FPS");
        assert_eq!(serde_json::to_value(&view).unwrap()["chart_after_rows"], 2);
    }

    #[test]
    fn fps_uses_presentation_feedback_without_substituting_submission_or_gpu_rates() {
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        let mut telemetry = RendererTelemetry { submissions: 1200, ..Default::default() };
        telemetry.gpu.push(1.);
        assert_eq!(view(telemetry.clone(), &localization).rows[2].value, "—");
        telemetry.frame_rate = Some(layer_render::FrameRate { fps: 119., source: layer_render::FrameRateSource::Presented });
        assert_eq!(view(telemetry.clone(), &localization).rows[2].value, "119");
        telemetry.frame_rate = Some(layer_render::FrameRate { fps: 0., source: layer_render::FrameRateSource::Presented });
        assert_eq!(view(telemetry, &localization).rows[2].value, "0");
    }

    #[test]
    fn submission_fallback_is_explicit_and_does_not_claim_presentation() {
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        let telemetry = RendererTelemetry { frame_rate: Some(layer_render::FrameRate { fps: 120., source: layer_render::FrameRateSource::Submitted }), ..Default::default() };
        let metric = &view(telemetry, &localization).rows[2];
        assert_eq!(metric.label, "Submitted FPS");
        assert_eq!(metric.value, "120");
        assert!(metric.description.contains("unavailable"));
    }

    fn memory_view(process: Option<layer_render::ProcessMemory>, gpu: Option<layer_render::GpuMemory>, age: u64) -> StatsView {
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        view(RendererTelemetry { resident_bytes: 1048576, memory: Some(MemorySnapshot {
            sampled_at: std::time::Instant::now() - std::time::Duration::from_secs(age), process, gpu,
        }), ..Default::default() }, &localization)
    }

    #[test]
    fn memory_unavailable_is_not_zero_and_old_samples_keep_their_age() {
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        let absent = view(RendererTelemetry::default(), &localization);
        for row in &absent.rows[8..11] {
            assert_eq!(row.value, "Unavailable");
            assert!(row.description.contains("No sample available"));
        }
        let old = memory_view(None, None, 42);
        for row in &old.rows[8..11] {
            assert_eq!(row.value, "Unavailable");
            let age = row.description.split("Sample age: ").nth(1).unwrap().split_whitespace().next().unwrap().parse::<f64>().unwrap();
            assert!(age >= 42.);
        }
    }

    #[test]
    fn process_metrics_keep_their_scope_mib_units_and_peak_semantics() {
        use layer_render::ProcessMemory;
        for (kind, scope) in [
            (ProcessMemoryKind::Resident, "including shared pages"),
            (ProcessMemoryKind::PrivateCommit, "including pages outside RAM"),
            (ProcessMemoryKind::PhysicalFootprint, "including compressed memory"),
        ] {
            let metric = memory_view(Some(ProcessMemory { kind, bytes: 10 * 1048576, peak: MemoryPeak::Lifetime(12 * 1048576) }), None, 0);
            assert_eq!(metric.rows[4].value, "1.0 MiB");
            assert_eq!(metric.rows[8].value, "10.0 MiB");
            assert!(metric.rows[8].description.contains(scope));
            assert_eq!(metric.rows[9].value, "12.0 MiB");
            assert!(metric.rows[9].description.contains(&metric.rows[8].label));
            assert!(metric.rows[9].description.contains("Operating-system lifetime peak"));
        }
        let sampled = memory_view(Some(ProcessMemory { kind: ProcessMemoryKind::Resident, bytes: 0, peak: MemoryPeak::Sampled(1572864) }), None, 0);
        assert_eq!(sampled.rows[8].value, "0.0 MiB");
        assert_eq!(sampled.rows[9].value, "1.5 MiB");
        assert!(sampled.rows[9].description.contains("sampling can miss earlier peaks"));
        assert!(!sampled.rows[9].description.contains("lifetime"));
    }

    #[test]
    fn driver_memory_keeps_raw_budget_scope_and_unified_memory_overlap() {
        use layer_render::GpuMemory;
        for (kind, scope) in [
            (GpuMemoryKind::VulkanHeap, "Vulkan heap with most headroom"),
            (GpuMemoryKind::DxgiLocal, "local memory segment"),
            (GpuMemoryKind::MetalAllocated, "recommended working set"),
        ] {
            let metric = memory_view(None, Some(GpuMemory { kind, bytes: 3 * 1048576, budget: Some(8 * 1048576) }), 0);
            assert_eq!(metric.rows[10].value, "3.0 / 8.0 MiB");
            assert!(metric.rows[10].description.contains(scope));
            assert!(metric.rows[10].description.contains("may overlap"));
            assert!(metric.rows[10].description.contains("do not add"));
            if kind == GpuMemoryKind::MetalAllocated { assert!(metric.rows[10].description.contains("not free memory or a hard limit")); }
        }
        let partial = memory_view(None, Some(GpuMemory { kind: GpuMemoryKind::MetalAllocated, bytes: 1048576, budget: None }), 0);
        assert_eq!(partial.rows[10].value, "1.0 / — MiB");
        assert!(partial.rows[10].description.contains("— means unavailable"));
    }

    #[test]
    fn budget_rows_use_the_copied_snapshot_and_preserve_per_device_debt() {
        let budget = layer_core::MemoryBudget::new();
        budget.update_ram(layer_core::MemoryFacts { total_bytes: Some(16 << 30), available_bytes: Some(12 << 30), override_bytes: Some(384 * 1048576), ..Default::default() }, 0);
        let device = budget.register_device(layer_core::DeviceMemoryFacts { budget_bytes: Some(10 * 1048576), ..Default::default() });
        let owner = budget.register_owner(layer_core::MemoryOwnerKind::Authored, layer_core::MemoryClass::Pinned, Some(device));
        owner.report(layer_core::MemoryReport { ram_bytes: 385 * 1048576, pinned_ram_bytes: 385 * 1048576,
            gpu_bytes: 9 * 1048576, pinned_gpu_bytes: 9 * 1048576, ..Default::default() });
        budget.register_device(layer_core::DeviceMemoryFacts { budget_bytes: Some(100 * 1048576), ..Default::default() });
        let mut summary = layer_render::MemoryBudgetSummary::new(&budget.snapshot());
        summary.sampled_at -= std::time::Duration::from_secs(42);
        owner.report(Default::default());
        let localization = crate::Localizer::shared(crate::UiLanguage::English);
        let view = view(RendererTelemetry { memory_budget: Some(std::sync::Arc::new(summary)), ..Default::default() }, &localization);
        assert_eq!(view.rows.len(), 15);
        assert_eq!(view.rows[11].value, "RAM 384.0 MiB · GPU 88.0 MiB");
        assert_eq!(view.rows[12].value, "RAM 385.0 MiB · GPU 9.0 MiB");
        assert_eq!(view.rows[13].value, view.rows[12].value);
        assert_eq!(view.rows[14].value, "RAM 1.0 MiB · GPU 1.0 MiB");
        for row in &view.rows[11..] { assert!(row.description.contains("Sample age: 42.")); }
        assert!(view.rows[11].description.contains("do not add"));
    }

}
