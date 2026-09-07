use layer_core::{
    BrushCombine, BrushCurve, BrushMapping, BrushSensor, BrushSnapshot, BrushTarget, Point,
    StrokePoint,
};
use layer_engine::{DabGenerator, PenEvent, PenPhase, SampleFlags, ToolKind, input_queue};
use std::{hint::black_box, time::Instant};

const SAMPLES: usize = 1_000_000;

fn main() {
    benchmark_queue();
    benchmark_dabs();
}

fn benchmark_queue() {
    let (mut producer, mut consumer) = input_queue(4_096);
    let event = PenEvent {
        device_id: 1,
        sequence: 0,
        timestamp_ns: 0,
        view_revision: 0,
        surface_position: Point { x: 1.0, y: 1.0 },
        pressure: 0.5,
        tilt_radians: [0.0; 2],
        twist_radians: 0.0,
        distance: 0.0,
        phase: PenPhase::Move,
        tool: ToolKind::Pen,
        flags: SampleFlags::NONE,
    };
    let started = Instant::now();
    for sequence in 0..SAMPLES {
        let mut sample = event;
        sample.sequence = sequence as u64;
        producer.push(sample).unwrap();
        black_box(consumer.pop().unwrap());
    }
    report("SPSC push + pop", SAMPLES, started.elapsed());
}

fn benchmark_dabs() {
    benchmark_brush("stroke resample", BrushSnapshot::default());
    let mapping = BrushMapping {
        sensor: BrushSensor::Pressure,
        target: BrushTarget::Rotation,
        combine: BrushCombine::Add,
        input_min: 0.0,
        input_max: 1.0,
        output_scale: 0.001,
        output_bias: 0.0,
        curve: BrushCurve::LINEAR,
    };
    let mut mappings = vec![BrushMapping::pressure_size()];
    mappings.extend(std::iter::repeat_n(mapping, 31));
    benchmark_brush(
        "32-map dynamics",
        BrushSnapshot {
            mappings: mappings.into(),
            ..BrushSnapshot::default()
        },
    );
}

fn benchmark_brush(name: &str, brush: BrushSnapshot) {
    let mut generator = DabGenerator::default();
    let mut dabs = Vec::with_capacity(SAMPLES * 2);
    generator.reset_for_stroke(layer_core::StrokeId(1), &brush);
    let started = Instant::now();
    for index in 0..SAMPLES {
        let point = StrokePoint {
            position: Point {
                x: index as f32 * 0.65,
                y: ((index % 97) as f32 * 0.03).sin() * 20.0,
            },
            pressure: 0.2 + (index % 800) as f32 / 1_000.0,
            tilt: [0.0; 2],
            twist: 0.0,
            elapsed_micros: index as u32,
        };
        black_box(generator.append(point, &brush, &mut dabs));
    }
    report(name, SAMPLES, started.elapsed());
    println!("  generated {} dabs", dabs.len());
}

fn report(name: &str, iterations: usize, elapsed: std::time::Duration) {
    let rate = iterations as f64 / elapsed.as_secs_f64();
    println!("{name:>18}: {rate:>12.0} ops/s ({elapsed:?})");
}
