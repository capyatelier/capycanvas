//! Compact diagnostics, labels and summaries shared by native hosts.
use crate::{byte_share, byte_size, Chip, FluentArgs, Localizer, Meter, MeterRow, MeterSection, MessageId, Tip};
use layer_render::{MemoryPeak, MemoryPool, ProcessMemoryKind, RendererTelemetry};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct StatRow {
    pub label: String,
    pub value: String,
    pub description: String,
    pub tip: Tip,
}
#[derive(Clone, Debug, Serialize)]
pub struct StatsView {
    pub rows: Vec<StatRow>,
    pub memory: MeterSection,
    pub samples: Vec<f32>,
    /// Raw execution timings for host qualification; keep presentation separate.
    pub gpu_samples: Vec<f32>,
    pub resident_bytes: u64,
    pub frames: u64,
    pub dabs: u64,
    pub effect_passes: u64,
    /// Select › Modify values received and previews drawn since the session
    /// began, for host qualification.
    pub selection_values: u64,
    pub selection_previews: u64,
    pub budget_ms: f32,
    pub chart_label: String,
    /// Insert the chart after this many metric rows on every frontend.
    pub chart_after_rows: usize,
}
const STALE_SECONDS: u64 = 3;
const BUDGET_FPS: f32 = 120.;
fn gpu_name(name: &str) -> String {
    let mut name = ["(R)", "(TM)", "(tm)"].into_iter().fold(name.to_string(), |name, mark| name.replace(mark, ""));
    while name.trim_end().ends_with(')') && let Some(open) = name.rfind('(') { name.truncate(open); }
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn stat(label: impl Into<String>, value: impl Into<String>, tip: Tip) -> StatRow {
    StatRow { label: label.into(), value: value.into(), description: tip.plain(), tip }
}
fn milliseconds(id: MessageId, value: f32, l: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("value", format!("{value:.1}"));
    args.set("fps", format!("{BUDGET_FPS:.0}"));
    l.format(id, &args)
}
fn timing(label: &str, samples: Vec<f32>, text: MessageId, l: &Localizer) -> StatRow {
    let text = l.text(text).to_string();
    if samples.is_empty() { return stat(label, "—", Tip::text(text)); }
    let mut sorted = samples;
    sorted.sort_by(f32::total_cmp);
    let at = |q: f32| sorted[((sorted.len() - 1) as f32 * q).round() as usize];
    let [typical, slow, slowest] = [at(0.5), at(0.95), at(0.99)];
    let line = |value: f32| {
        let mut args = FluentArgs::new();
        args.set("value", format!("{value:.2}"));
        l.format(MessageId::DIAGNOSTICS_MILLISECONDS, &args)
    };
    stat(label, format!("{typical:.2} / {slow:.2} / {slowest:.2}"), Tip::figures(text)
        .line(l.text(MessageId::DIAGNOSTICS_TYPICAL).to_string(), line(typical))
        .line(l.text(MessageId::DIAGNOSTICS_SLOWEST_5).to_string(), line(slow))
        .line(l.text(MessageId::DIAGNOSTICS_SLOWEST_1).to_string(), line(slowest))
        .line(l.text(MessageId::DIAGNOSTICS_BUDGET).to_string(), milliseconds(MessageId::DIAGNOSTICS_BUDGET_VALUE, 1000. / BUDGET_FPS, l)))
}
fn memory(t: &RendererTelemetry, l: &Localizer) -> MeterSection {
    let text = |id| l.text(id).to_string();
    let title = text(MessageId::DIAGNOSTICS_MEMORY_TITLE);
    let key = Tip::text(String::new())
        .line(text(MessageId::DIAGNOSTICS_MEMORY_IN_USE), text(MessageId::DIAGNOSTICS_MEMORY_KEY_IN_USE))
        .line(text(MessageId::DIAGNOSTICS_MEMORY_CACHE), text(MessageId::DIAGNOSTICS_MEMORY_KEY_CACHE))
        .line(text(MessageId::DIAGNOSTICS_MEMORY_REST), text(MessageId::DIAGNOSTICS_MEMORY_KEY_REST))
        .line(text(MessageId::DIAGNOSTICS_MEMORY_ALLOWANCE), text(MessageId::DIAGNOSTICS_MEMORY_KEY_ALLOWANCE));
    let Some(budget) = t.memory_budget.as_deref() else { return MeterSection { title, tip: key, ..Default::default() } };
    let pool_row = |label: String, pool: MemoryPool, whole: Option<u64>, tip: Tip| {
        let (held, tracked, limit) = (pool.in_use_bytes(), pool.physical_bytes, pool.limit());
        let mut tip = tip.line(text(MessageId::DIAGNOSTICS_MEMORY_IN_USE), byte_size(held, l))
            .line(text(MessageId::DIAGNOSTICS_MEMORY_CACHE), byte_size(tracked - held, l));
        let actual = whole.map(|whole| whole.max(tracked)).zip(pool.total_bytes.filter(|total| *total > 0));
        if let Some((whole, _)) = actual { tip = tip.line(text(MessageId::DIAGNOSTICS_MEMORY_REST), byte_size(whole - tracked, l)); }
        if let Some(limit) = limit { tip = tip.line(text(MessageId::DIAGNOSTICS_MEMORY_ALLOWANCE), byte_size(limit, l)); }
        let (value, meter) = match actual {
            Some((whole, total)) => (byte_share(whole, total, l), Meter::usage(total, limit, held, tracked, whole)),
            None => (limit.map_or_else(|| byte_size(tracked, l), |limit| byte_share(tracked, limit, l)), Meter::usage(limit.unwrap_or(0), limit, held, tracked, 0)),
        };
        MeterRow { label, chip: None, value, warning: pool.over_bytes() > 0, meter: Some(meter), tip }
    };
    let process = t.memory.and_then(|sample| sample.process);
    let reading = t.memory.and_then(|sample| sample.gpu);
    let adapter = t.adapter.as_deref();
    let gpu_label = adapter.map(|adapter| gpu_name(&adapter.name)).filter(|name| !name.is_empty())
        .unwrap_or_else(|| text(MessageId::DIAGNOSTICS_MEMORY_GPU_FALLBACK));
    let unified = budget.gpu.filter(|_| budget.gpu_shares_ram);
    let graphics_outside = process.is_some_and(|process| process.kind != ProcessMemoryKind::PhysicalFootprint);
    let whole = process.map(|process| process.bytes + if unified.is_some() && graphics_outside { reading.map_or(0, |reading| reading.bytes) } else { 0 });
    let place = unified.map(|_| if cfg!(target_arch = "wasm32") { text(MessageId::DIAGNOSTICS_MEMORY_BROWSER_GPU) } else {
        let mut args = FluentArgs::new();
        args.set("gpu", gpu_label.as_str());
        l.format(MessageId::DIAGNOSTICS_MEMORY_SHARED_WITH, &args)
    });
    let tracked_only = (whole.is_none() || budget.ram.total_bytes.is_none_or(|total| total == 0)).then(|| text(MessageId::DIAGNOSTICS_MEMORY_TRACKED_ONLY));
    let lead = place.into_iter().chain(tracked_only).collect::<Vec<_>>().join("\n");
    let label = text(if unified.is_some() { MessageId::DIAGNOSTICS_MEMORY_TITLE } else { MessageId::DIAGNOSTICS_MEMORY_RAM_LABEL });
    let mut ram = pool_row(label, budget.ram, whole, Tip::figures(lead));
    let mut over = budget.ram.over_bytes();
    if let Some(pool) = unified {
        over = over.max(pool.over_bytes());
        ram.warning = over > 0;
        let graphics = byte_size(pool.physical_bytes, l);
        ram.tip = ram.tip.line(text(MessageId::DIAGNOSTICS_MEMORY_GRAPHICS), match reading {
            Some(reading) => {
                let mut args = FluentArgs::new();
                args.set("used", graphics);
                args.set("total", byte_size(reading.bytes.max(pool.physical_bytes), l));
                l.format(MessageId::DIAGNOSTICS_MEMORY_OF, &args)
            }
            None => graphics,
        });
    }
    if let Some(process) = process {
        let (MemoryPeak::Lifetime(peak) | MemoryPeak::Sampled(peak)) = process.peak;
        ram.tip = ram.tip.line(text(MessageId::DIAGNOSTICS_MEMORY_PEAK_LABEL), byte_size(peak, l));
    }
    if budget.refusals > 0 { ram.tip = ram.tip.line(text(MessageId::DIAGNOSTICS_MEMORY_REFUSED), budget.refusals.to_string()); }
    let mut rows = vec![ram];
    if let Some(pool) = budget.gpu.filter(|_| unified.is_none()) {
        over += pool.over_bytes();
        let identity = adapter.map(|adapter| [adapter.name.trim(), adapter.api, adapter.driver.as_str()].into_iter()
            .filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
        let mut row = pool_row(gpu_label, pool, reading.map(|reading| reading.bytes), Tip::figures(identity));
        if let Some(available) = reading.and_then(|reading| reading.budget) {
            row.tip = row.tip.line(text(MessageId::DIAGNOSTICS_MEMORY_AVAILABLE), byte_size(available, l));
        }
        rows.push(row);
    }
    let mut warnings = Vec::new();
    if over > 0 {
        let mut args = FluentArgs::new();
        args.set("amount", byte_size(over, l));
        warnings.push(l.format(MessageId::DIAGNOSTICS_MEMORY_OVER, &args));
    }
    if budget.pressure { warnings.push(text(MessageId::DIAGNOSTICS_MEMORY_LOW)); }
    let age = budget.sampled_at.elapsed().as_secs();
    let note = if age > STALE_SECONDS {
        let mut args = FluentArgs::new();
        args.set("seconds", age);
        l.format(MessageId::DIAGNOSTICS_MEMORY_STALE, &args)
    } else { String::new() };
    MeterSection {
        title, note,
        chip: warnings.first().map(|warning| Chip { label: warning.as_str().into(), warning: true }),
        tip: Tip { text: warnings.join(" · "), ..key },
        rows,
    }
}
pub(super) fn view(t: RendererTelemetry, l: &Localizer) -> StatsView {
    let samples = t.cpu.ordered();
    let submitted = t.frame_rate.is_some_and(|rate| rate.source == layer_render::FrameRateSource::Submitted);
    let gpu = if t.gpu_timestamps {
        timing("GPU · ms", t.gpu.ordered(), MessageId::DIAGNOSTICS_GPU_TIP, l)
    } else {
        stat("GPU · ms", "Unavailable", Tip::text(l.text(if cfg!(target_arch = "wasm32") { MessageId::DIAGNOSTICS_GPU_BROWSER_TIP } else { MessageId::DIAGNOSTICS_GPU_UNAVAILABLE_TIP }).to_string()))
    };
    let rows = vec![
        timing("CPU · ms", samples.clone(), MessageId::DIAGNOSTICS_CPU_TIP, l),
        gpu,
        stat(l.text(if submitted { MessageId::DIAGNOSTICS_SUBMITTED_FPS } else { MessageId::DIAGNOSTICS_FPS }).to_string(),
            t.frame_rate.map_or_else(|| "—".into(), |rate| format!("{:.0}", rate.fps)),
            Tip::text(l.text(if submitted { MessageId::DIAGNOSTICS_SUBMITTED_FPS_TIP } else { MessageId::DIAGNOSTICS_FPS_TIP }).to_string())),
    ];
    StatsView {
        memory: memory(&t, l),
        rows,
        samples,
        gpu_samples: if t.gpu_timestamps { t.gpu.ordered() } else { Vec::new() },
        resident_bytes: t.resident_bytes,
        frames: t.submissions,
        dabs: t.dabs,
        effect_passes: t.effect_passes,
        selection_values: 0,
        selection_previews: 0,
        budget_ms: 1000. / BUDGET_FPS,
        chart_label: milliseconds(MessageId::DIAGNOSTICS_CHART_TIP, 1000. / BUDGET_FPS, l),
        chart_after_rows: 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_render::{GpuAdapter, GpuMemory, GpuMemoryKind, MemoryBudgetSummary, MemorySnapshot, ProcessMemory, ProcessMemoryKind};
    use std::sync::Arc;

    const MB: u64 = 1 << 20;

    fn english() -> Arc<Localizer> { Localizer::shared(crate::UiLanguage::English) }

    fn lines(tip: &Tip) -> Vec<(&str, &str)> { tip.lines.iter().map(|line| (line.label.as_str(), line.value.as_str())).collect() }

    #[test]
    fn diagnostics_shows_timings_then_memory_and_keeps_counters_off_screen() {
        let telemetry = RendererTelemetry { submissions: 12, dabs: 34, effect_passes: 5, ..Default::default() };
        let view = view(telemetry, &english());
        assert_eq!(view.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(), ["CPU · ms", "GPU · ms", "FPS"]);
        assert_eq!(view.rows[view.chart_after_rows - 1].label, "GPU · ms");
        assert_eq!((view.frames, view.dabs, view.effect_passes), (12, 34, 5));
        assert_eq!(view.chart_label, "CPU time for the last 120 frames. The dashed line is the 8.3 ms budget.");
        assert_eq!(view.memory.title, "Memory");
        assert!(view.memory.rows.is_empty());
        assert_eq!(lines(&view.memory.tip).iter().map(|line| line.0).collect::<Vec<_>>(), ["In use", "Cache", "Rest of app", "Allowance"]);
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!((json["chart_after_rows"].as_u64(), json["frames"].as_u64()), (Some(2), Some(12)));
    }

    #[test]
    fn timing_tips_name_each_figure_against_the_frame_budget() {
        let mut telemetry = RendererTelemetry { gpu_timestamps: true, ..Default::default() };
        for ms in 1..=100 { telemetry.cpu.push(ms as f32 / 10.); }
        let view = view(telemetry, &english());
        let cpu = &view.rows[0];
        assert_eq!(cpu.value, "5.10 / 9.50 / 9.90");
        assert_eq!(cpu.tip.text, "Time Capy takes to prepare each canvas frame.");
        assert_eq!(lines(&cpu.tip), [("Typical", "5.10 ms"), ("Slowest 5%", "9.50 ms"), ("Slowest 1%", "9.90 ms"), ("Budget", "8.3 ms (120 fps)")]);
        assert!(cpu.tip.figures);
        assert!(cpu.description.starts_with("Time Capy takes") && cpu.description.contains("Typical: 5.10 ms"));
        assert_eq!((view.rows[1].value.as_str(), view.rows[1].tip.lines.len()), ("—", 0));
        let unavailable = super::view(RendererTelemetry::default(), &english());
        assert_eq!((unavailable.rows[1].value.as_str(), unavailable.rows[1].tip.text.as_str()), ("Unavailable", "This graphics card doesn’t report timings."));
        assert_eq!(unavailable.rows[2].tip.text, "Canvas frames shown in the last second. 0 while nothing on the canvas changes.");
    }

    #[test]
    fn fps_uses_presentation_feedback_without_substituting_submission_or_gpu_rates() {
        let localization = english();
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
        let telemetry = RendererTelemetry { frame_rate: Some(layer_render::FrameRate { fps: 120., source: layer_render::FrameRateSource::Submitted }), ..Default::default() };
        let metric = &view(telemetry, &english()).rows[2];
        assert_eq!(metric.label, "Submitted FPS");
        assert_eq!(metric.value, "120");
        assert!(metric.description.contains("doesn’t report when they appear"));
    }

    fn pool(total: Option<u64>, limit: u64, physical: u64, pinned: u64) -> MemoryPool {
        MemoryPool { total_bytes: total.map(|total| total * MB), limit_bytes: limit * MB, physical_bytes: physical * MB, pinned_bytes: pinned * MB }
    }

    fn telemetry(ram: MemoryPool, gpu: Option<MemoryPool>, shares: bool, adapter: &str) -> RendererTelemetry {
        RendererTelemetry {
            memory: Some(MemorySnapshot { sampled_at: web_time::Instant::now(),
                process: Some(ProcessMemory { kind: ProcessMemoryKind::Resident, bytes: 2150 * MB, peak: MemoryPeak::Lifetime(2610 * MB) }),
                gpu: Some(GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: 3020 * MB, budget: Some(11_264 * MB) }) }),
            memory_budget: Some(Arc::new(MemoryBudgetSummary { sampled_at: web_time::Instant::now(), ram, gpu, gpu_shares_ram: shares, pressure: false, refusals: 0 })),
            adapter: Some(Arc::new(GpuAdapter { name: adapter.into(), api: "Vulkan", driver: "NVIDIA 580.95.05".into() })),
            ..Default::default()
        }
    }

    #[test]
    fn discrete_gpu_bars_show_actual_use_against_capacity_with_tracked_parts_inside() {
        let memory = view(telemetry(pool(Some(32_768), 6144, 1430, 880), Some(pool(Some(12_288), 8192, 2350, 1100)), false, "NVIDIA GeForce RTX 4070"), &english()).memory;
        assert_eq!((memory.chip.as_ref(), memory.note.as_str(), memory.tip.text.as_str()), (None, "", ""));
        let [ram, gpu] = &memory.rows[..] else { panic!("{memory:?}") };
        assert_eq!((ram.label.as_str(), ram.value.as_str(), ram.warning, ram.tip.text.as_str()), ("RAM", "2.1 / 32 GB", false, ""));
        assert_eq!(lines(&ram.tip), [("In use", "880 MB"), ("Cache", "550 MB"), ("Rest of app", "720 MB"), ("Allowance", "6 GB"), ("Peak", "2.5 GB")]);
        assert_eq!(ram.meter, Some(Meter::usage(32_768 * MB, Some(6144 * MB), 880 * MB, 1430 * MB, 2150 * MB)));
        assert_eq!((gpu.label.as_str(), gpu.value.as_str(), gpu.chip.as_ref()), ("NVIDIA GeForce RTX 4070", "2.9 / 12 GB", None));
        assert_eq!(gpu.tip.text, "NVIDIA GeForce RTX 4070 · Vulkan · NVIDIA 580.95.05");
        assert_eq!(lines(&gpu.tip), [("In use", "1.1 GB"), ("Cache", "1.2 GB"), ("Rest of app", "670 MB"), ("Allowance", "8 GB"), ("Available to Capy", "11 GB")]);
        assert_eq!(gpu.meter, Some(Meter::usage(12_288 * MB, Some(8192 * MB), 1100 * MB, 2350 * MB, 3020 * MB)));
    }

    #[test]
    fn unified_memory_draws_one_memory_bar_with_a_graphics_line() {
        let memory = view(telemetry(pool(Some(32_768), 8192, 2600, 1200), Some(pool(None, 8192, 1500, 700)), true, "Intel(R) Iris(R) Xe Graphics"), &english()).memory;
        let [bar] = &memory.rows[..] else { panic!("{memory:?}") };
        assert_eq!((bar.label.as_str(), bar.value.as_str(), bar.chip.as_ref(), bar.tip.text.as_str()), ("Memory", "5 / 32 GB", None, "Shared with Intel Iris Xe Graphics."));
        assert_eq!(bar.meter, Some(Meter::usage(32_768 * MB, Some(8192 * MB), 1200 * MB, 2600 * MB, 5170 * MB)));
        assert_eq!(lines(&bar.tip), [("In use", "1.2 GB"), ("Cache", "1.4 GB"), ("Rest of app", "2.5 GB"), ("Allowance", "8 GB"), ("Graphics", "1.5 GB of 2.9 GB"), ("Peak", "2.5 GB")]);
        let mut apple = telemetry(pool(Some(32_768), 8192, 2600, 1200), Some(pool(None, 8192, 1500, 700)), true, "Apple M2 Pro");
        apple.memory.as_mut().unwrap().process.as_mut().unwrap().kind = ProcessMemoryKind::PhysicalFootprint;
        let bar = &view(apple, &english()).memory.rows[0];
        assert_eq!(bar.value, "2.5 / 32 GB", "the footprint already holds graphics, and the bar never shows less than Capy tracks");
    }

    #[test]
    fn pinned_reservations_above_physical_are_all_in_use_and_warn_only_past_the_allowance() {
        let memory = |physical| view(telemetry(pool(Some(8192), 2048, physical, 3000), None, false, ""), &english()).memory;
        let within = memory(1500);
        assert_eq!((within.chip.as_ref(), within.rows[0].warning), (None, false));
        assert_eq!(lines(&within.rows[0].tip)[..2], [("In use", "1.5 GB"), ("Cache", "0 MB")]);
        let past = memory(2300);
        assert_eq!((past.chip, past.rows[0].warning), (Some(Chip { label: "Over by 252 MB".into(), warning: true }), true));
        assert_eq!(lines(&past.rows[0].tip)[..2], [("In use", "2.2 GB"), ("Cache", "0 MB")]);
        assert_eq!(past.rows[0].meter, Some(Meter::usage(8192 * MB, Some(2048 * MB), 2300 * MB, 2300 * MB, 2300 * MB)));
    }

    #[test]
    fn unknown_limits_draw_no_notch_or_allowance_and_keep_a_finite_scale() {
        let gpu = |total: Option<u64>, limit| view(telemetry(pool(Some(32_768), 6144, 1430, 880),
            Some(MemoryPool { total_bytes: total.map(|total| total * MB), limit_bytes: limit, ..pool(None, 0, 2350, 1100) }), false, "NVIDIA GeForce RTX 4070"), &english()).memory;
        for limit in [u64::MAX, 0] {
            let memory = gpu(None, limit);
            assert_eq!((memory.chip.as_ref(), memory.rows[1].value.as_str(), memory.rows[1].warning), (None, "2.3 GB", false));
            assert_eq!(lines(&memory.rows[1].tip), [("In use", "1.1 GB"), ("Cache", "1.2 GB"), ("Available to Capy", "11 GB")]);
            let meter = memory.rows[1].meter.clone().unwrap();
            assert_eq!((meter.limit, meter.segments.last().map(|segment| segment.to)), (1., Some(1.)));
            let memory = gpu(Some(12_288), limit);
            assert_eq!(memory.rows[1].value, "2.9 / 12 GB");
            assert_eq!(lines(&memory.rows[1].tip)[2], ("Rest of app", "670 MB"));
            assert_eq!(memory.rows[1].meter, Some(Meter::usage(12_288 * MB, None, 1100 * MB, 2350 * MB, 3020 * MB)));
            assert_eq!(memory.rows[1].meter.as_ref().map(|meter| meter.limit), Some(1.));
        }
    }

    #[test]
    fn trouble_shows_one_warning_chip_and_the_key_lists_every_warning() {
        let mut telemetry = telemetry(pool(Some(6144), 1536, 1720, 1650), Some(pool(None, 1536, 690, 610)), true, "Adreno (TM) 650");
        let mut budget = *telemetry.memory_budget.take().unwrap();
        budget.pressure = true;
        budget.refusals = 2;
        telemetry.memory_budget = Some(Arc::new(budget));
        let memory = view(telemetry, &english()).memory;
        assert_eq!(memory.chip, Some(Chip { label: "Over by 184 MB".into(), warning: true }));
        assert_eq!(memory.tip.text, "Over by 184 MB · Low memory");
        assert_eq!(memory.tip.lines.len(), 4);
        let [bar] = &memory.rows[..] else { panic!("{memory:?}") };
        assert!(bar.warning);
        assert_eq!(bar.tip.text, "Shared with Adreno 650.");
        assert_eq!(bar.tip.lines.last().map(|line| (line.label.as_str(), line.value.as_str())), Some(("Imports refused", "2")));
        let healthy = view(self::telemetry(pool(Some(6144), 1536, 900, 600), None, true, ""), &english()).memory;
        assert_eq!((healthy.chip, healthy.tip.text.as_str(), healthy.rows.len()), (None, "", 1));
    }

    #[test]
    fn without_system_figures_bars_fall_back_to_tracked_memory_and_the_allowance() {
        let mut telemetry = telemetry(pool(None, 2048, 960, 540), Some(pool(None, 2048, 610, 300)), true, " ");
        telemetry.memory = None;
        telemetry.adapter = Some(Arc::new(GpuAdapter { name: String::new(), api: "WebGPU", driver: String::new() }));
        let memory = view(telemetry, &english()).memory;
        let [bar] = &memory.rows[..] else { panic!("{memory:?}") };
        assert_eq!((bar.value.as_str(), bar.tip.text.as_str()), ("960 MB / 2 GB", "Shared with GPU.\nThe system total isn’t available, so the bar shows Capy’s own use."));
        assert_eq!(bar.meter, Some(Meter::usage(2048 * MB, Some(2048 * MB), 540 * MB, 960 * MB, 0)));
        assert_eq!(lines(&bar.tip), [("In use", "540 MB"), ("Cache", "420 MB"), ("Allowance", "2 GB"), ("Graphics", "610 MB")]);
    }

    #[test]
    fn old_budget_readings_say_how_old_they_are() {
        let mut telemetry = telemetry(pool(Some(8192), 6144, 1430, 880), None, false, "");
        let mut budget = *telemetry.memory_budget.take().unwrap();
        budget.sampled_at -= std::time::Duration::from_secs(42);
        telemetry.memory_budget = Some(Arc::new(budget));
        assert_eq!(view(telemetry, &english()).memory.note, "42 s old");
    }

    #[test]
    fn gpu_names_drop_marks_and_driver_codenames() {
        for (reported, shown) in [("Intel(R) Iris(R) Xe Graphics", "Intel Iris Xe Graphics"), ("AMD Radeon RX 7800 XT (RADV NAVI32)", "AMD Radeon RX 7800 XT"),
            ("Adreno (TM) 650", "Adreno 650"), ("llvmpipe (LLVM 19.1.0, 256 bits)", "llvmpipe"), ("Apple M2 Pro", "Apple M2 Pro")] {
            assert_eq!(gpu_name(reported), shown);
        }
    }
}
