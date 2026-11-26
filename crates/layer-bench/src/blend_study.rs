//! Paints fixed blending scenes with each blender through the real engine and
//! GPU renderer, then writes one PNG per scene and an index page.
use super::*;
use layer_core::BrushSnapshot;
use std::f32::consts::TAU;

const PANEL: u32 = 360;
const COLUMNS: u32 = 8;
const ROWS: u32 = 2;
const PAPER: [f32; 4] = [0.93, 0.92, 0.88, 1.0];

pub(crate) fn srgb(hex: u32) -> [f32; 4] {
    let channel = |shift: u32| {
        let v = ((hex >> shift) & 255) as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [channel(16), channel(8), channel(0), 1.0]
}

pub struct Scene {
    pub key: &'static str,
    pub title: &'static str,
    pub note: &'static str,
    seed: fn(&mut Painter),
    blend: fn(&mut Painter, &BrushSnapshot),
}

pub struct Painter<'a> {
    pub(crate) canvas: &'a mut Canvas,
    pub(crate) origin: [f32; 2],
}

impl Painter<'_> {
    fn stroke(&mut self, snapshot: &BrushSnapshot, points: &[(f32, f32, f32)]) -> Result<(), String> {
        self.canvas.engine.set_brush(snapshot.clone()).map_err(|e| e.to_string())?;
        self.canvas.engine.set_tool(StrokeTool::Brush);
        let count = points.len();
        for (index, &(x, y, pressure)) in points.iter().enumerate() {
            let sample = Sample { x: self.origin[0] + x, y: self.origin[1] + y, pressure };
            let event = self.canvas.next_event(sample, phase(index, count));
            self.canvas.submit(&[event])?;
            if index % 6 == 0 || index + 1 == count {
                self.canvas.draw()?;
            }
        }
        self.canvas.drain_submitted()?;
        self.canvas.wait_idle()
    }

    fn ink(&mut self, color: [f32; 4], diameter: f32, points: &[(f32, f32)]) {
        let mut pen = default_brush(Preset::GPen);
        pen.diameter = diameter;
        pen.color_rgba_linear = color;
        pen.opacity = 1.0;
        pen.flow = 1.0;
        pen.stabilization = BrushStabilization { strength: 0.0, pressure_smoothing: 0.0, ..pen.stabilization };
        pen.mappings = std::sync::Arc::from([]);
        pen.taper = Default::default();
        let points: Vec<_> = points.iter().map(|&(x, y)| (x, y, 1.0)).collect();
        self.stroke(&pen, &points).expect("seed stroke");
    }

    fn fill(&mut self, color: [f32; 4], rect: [f32; 4]) {
        let [x0, y0, x1, y1] = rect;
        let diameter = 24.0;
        let radius = diameter / 2.0;
        let mut y = y0 + radius;
        loop {
            let row = y.min(y1 - radius);
            self.ink(color, diameter, &line((x0 + radius, row), (x1 - radius, row), 3.0));
            if row >= y1 - radius { break; }
            y += radius;
        }
    }

    fn dot(&mut self, color: [f32; 4], diameter: f32, at: (f32, f32)) {
        self.ink(color, diameter, &[at, (at.0 + 0.01, at.1)]);
    }

    fn texture(&mut self, preset: Preset, color: [f32; 4], diameter: f32, points: &[(f32, f32)]) {
        let mut brush = default_brush(preset);
        brush.diameter = diameter;
        brush.color_rgba_linear = color;
        brush.stabilization = BrushStabilization { strength: 0.0, pressure_smoothing: 0.0, ..brush.stabilization };
        let points: Vec<_> = points.iter().map(|&(x, y)| (x, y, 0.8)).collect();
        self.stroke(&brush, &points).expect("texture stroke");
    }

    pub(crate) fn blend(&mut self, brush: &BrushSnapshot, points: &[(f32, f32, f32)]) {
        self.stroke(brush, points).expect("blend stroke");
    }
}

pub(crate) fn line(from: (f32, f32), to: (f32, f32), step: f32) -> Vec<(f32, f32)> {
    let length = (to.0 - from.0).hypot(to.1 - from.1);
    let count = (length / step).ceil().max(1.0) as usize;
    (0..=count).map(|i| {
        let t = i as f32 / count as f32;
        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
    }).collect()
}

pub(crate) fn pressured(points: Vec<(f32, f32)>, pressure: f32) -> Vec<(f32, f32, f32)> {
    let count = points.len().max(2) - 1;
    points.into_iter().enumerate().map(|(i, (x, y))| {
        let t = i as f32 / count as f32;
        let ramp = (t / 0.08).min((1.0 - t) / 0.08).clamp(0.0, 1.0);
        (x, y, pressure * (0.35 + 0.65 * ramp))
    }).collect()
}

pub(crate) fn polyline(points: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let mut result = vec![points[0]];
    for pair in points.windows(2) {
        result.extend(line(pair[0], pair[1], step).into_iter().skip(1));
    }
    result
}

fn arc(center: (f32, f32), radius: f32, from: f32, to: f32, step: f32) -> Vec<(f32, f32)> {
    let count = ((to - from).abs() * radius / step).ceil().max(1.0) as usize;
    (0..=count).map(|i| {
        let a = from + (to - from) * i as f32 / count as f32;
        (center.0 + radius * a.cos(), center.1 + radius * a.sin())
    }).collect()
}

const YELLOW: u32 = 0xf2c418;
const RED: u32 = 0xc8261d;
const BLUE: u32 = 0x1d4fc8;

pub fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            key: "along", title: "Along the edge",
            note: "Yellow over red, as in issue 14. Three strokes run back and forth along the boundary.",
            seed: |p| {
                p.fill(srgb(RED), [20.0, 20.0, 340.0, 340.0]);
                p.fill(srgb(YELLOW), [20.0, 20.0, 340.0, 180.0]);
            },
            blend: |p, b| {
                for (i, y) in [180.0, 186.0, 174.0].into_iter().enumerate() {
                    let (a, z) = if i % 2 == 0 { (50.0, 310.0) } else { (310.0, 50.0) };
                    p.blend(b, &pressured(line((a, y), (z, y), 2.0), 0.85));
                }
            },
        },
        Scene {
            key: "across", title: "Across the edge",
            note: "The same edge, crossed by one zigzag stroke moving right.",
            seed: |p| {
                p.fill(srgb(RED), [20.0, 20.0, 340.0, 340.0]);
                p.fill(srgb(YELLOW), [20.0, 20.0, 340.0, 180.0]);
            },
            blend: |p, b| {
                let mut points = vec![];
                for i in 0..=10 {
                    let x = 50.0 + i as f32 * 26.0;
                    points.push((x, if i % 2 == 0 { 110.0 } else { 250.0 }));
                }
                p.blend(b, &pressured(polyline(&points, 2.0), 0.85));
            },
        },
        Scene {
            key: "sphere", title: "Cel sphere",
            note: "Three hard tone bands on a sphere, blended with strokes that follow the form.",
            seed: |p| {
                p.fill(srgb(0xd9d4c7), [20.0, 20.0, 340.0, 340.0]);
                p.dot(srgb(0x3a2f5c), 250.0, (180.0, 185.0));
                p.dot(srgb(0x8a5a9c), 190.0, (158.0, 160.0));
                p.dot(srgb(0xe8a3b8), 110.0, (138.0, 135.0));
            },
            blend: |p, b| {
                for (radius, from, to) in [(95.0, 3.6, 0.2), (70.0, 0.4, 3.9), (45.0, 3.4, 0.3), (110.0, 0.5, 2.6)] {
                    p.blend(b, &pressured(arc((150.0, 165.0), radius, from, to, 2.0), 0.8));
                }
            },
        },
        Scene {
            key: "edge", title: "Edge touch",
            note: "Blue left, red right. The stroke runs down the red side and only its left rim reaches the blue.",
            seed: |p| {
                p.fill(srgb(RED), [20.0, 20.0, 340.0, 340.0]);
                p.fill(srgb(BLUE), [20.0, 20.0, 140.0, 340.0]);
            },
            blend: |p, b| {
                let x = 140.0 + b.diameter * 0.38;
                p.blend(b, &pressured(line((x, 40.0), (x, 320.0), 2.0), 0.85));
                p.blend(b, &pressured(line((x, 320.0), (x, 40.0), 2.0), 0.85));
            },
        },
        Scene {
            key: "pull", title: "Long pull",
            note: "Three blobs pulled right into bare canvas by one stroke each.",
            seed: |p| {
                p.dot(srgb(RED), 70.0, (70.0, 80.0));
                p.dot(srgb(BLUE), 70.0, (70.0, 180.0));
                p.dot(srgb(YELLOW), 70.0, (70.0, 280.0));
            },
            blend: |p, b| {
                for y in [80.0, 180.0, 280.0] {
                    p.blend(b, &pressured(line((55.0, y), (330.0, y), 2.0), 0.9));
                }
            },
        },
        Scene {
            key: "stripes", title: "Fine detail",
            note: "Thin stripes crossed by two horizontal strokes: how much structure survives.",
            seed: |p| {
                p.fill(srgb(0xf4efe1), [20.0, 20.0, 340.0, 340.0]);
                let colors = [0x22303f, 0xd1495b, 0x00798c, 0xedae49];
                for i in 0..19 {
                    let x = 30.0 + i as f32 * 16.0;
                    p.ink(srgb(colors[i % 4]), 6.0, &line((x, 30.0), (x, 330.0), 3.0));
                }
            },
            blend: |p, b| {
                p.blend(b, &pressured(line((40.0, 120.0), (320.0, 120.0), 2.0), 0.85));
                p.blend(b, &pressured(line((320.0, 240.0), (40.0, 240.0), 2.0), 0.85));
            },
        },
        Scene {
            key: "texture", title: "Grain",
            note: "Chalk scribbles in three colors, blended with two horizontal strokes.",
            seed: |p| {
                p.fill(srgb(0x2b2b33), [20.0, 20.0, 340.0, 340.0]);
                for (i, color) in [0xe9c46a, 0x2a9d8f, 0xe76f51].into_iter().enumerate() {
                    let y0 = 60.0 + i as f32 * 100.0;
                    let mut points = vec![];
                    for k in 0..=8 {
                        points.push((40.0 + k as f32 * 35.0, y0 + if k % 2 == 0 { -40.0 } else { 40.0 }));
                    }
                    p.texture(Preset::Chalk, srgb(color), 70.0, &polyline(&points, 3.0));
                }
            },
            blend: |p, b| {
                p.blend(b, &pressured(line((40.0, 110.0), (320.0, 110.0), 2.0), 0.85));
                p.blend(b, &pressured(line((320.0, 250.0), (40.0, 250.0), 2.0), 0.85));
            },
        },
        Scene {
            key: "scrub", title: "Scrubbing",
            note: "Five color columns scrubbed with short back-and-forth strokes.",
            seed: |p| {
                for (i, color) in [0x264653, 0x2a9d8f, 0xe9c46a, 0xf4a261, 0xe76f51].into_iter().enumerate() {
                    let x = 20.0 + i as f32 * 64.0;
                    p.fill(srgb(color), [x, 20.0, x + 64.0, 340.0]);
                }
            },
            blend: |p, b| {
                for row in 0..3 {
                    let y = 80.0 + row as f32 * 100.0;
                    let mut points = vec![];
                    for k in 0..=12 {
                        let x = 40.0 + k as f32 * 23.0;
                        points.push((x + if k % 2 == 0 { 0.0 } else { 30.0 }, y + if k % 2 == 0 { -14.0 } else { 14.0 }));
                    }
                    p.blend(b, &pressured(polyline(&points, 2.0), 0.8));
                }
            },
        },
        Scene {
            key: "glow", title: "Soft shapes",
            note: "Overlapping shapes on transparency; a looping stroke passes through all of them and out into bare canvas.",
            seed: |p| {
                p.dot(srgb(0x6a4c93), 150.0, (130.0, 140.0));
                p.dot(srgb(0x1982c4), 120.0, (230.0, 200.0));
                p.dot(srgb(0x8ac926), 90.0, (140.0, 250.0));
            },
            blend: |p, b| {
                let points: Vec<_> = (0..=180).map(|i| {
                    let t = i as f32 / 180.0 * TAU * 1.25;
                    (180.0 + 120.0 * (t).cos() * (1.0 - 0.3 * (t * 0.5).sin()), 185.0 + 110.0 * (t).sin())
                }).collect();
                p.blend(b, &pressured(points, 0.8));
            },
        },
        Scene {
            key: "pressure", title: "Pressure",
            note: "Stripes of three colors; each stroke runs from a light touch at the left to full pressure at the right.",
            seed: |p| {
                for (i, color) in [0x355070, 0xb56576, 0xeaac8b].into_iter().enumerate() {
                    let y = 20.0 + i as f32 * 107.0;
                    p.fill(srgb(color), [20.0, y, 340.0, y + 107.0]);
                }
            },
            blend: |p, b| {
                for y in [70.0, 127.0, 180.0, 234.0, 290.0] {
                    let points: Vec<_> = line((40.0, y), (320.0, y), 2.0).into_iter().enumerate()
                        .map(|(i, (x, y))| (x, y, (0.08 + i as f32 / 140.0).min(1.0))).collect();
                    p.blend(b, &points);
                }
            },
        },
        Scene {
            key: "lineart", title: "Inside the lines",
            note: "Flat fills with hard shadow shapes inside black line art; strokes run over the lines.",
            seed: |p| {
                p.fill(srgb(0xf6e7d7), [20.0, 20.0, 340.0, 340.0]);
                p.dot(srgb(0xf2c9a8), 220.0, (180.0, 180.0));
                p.dot(srgb(0xd99a7e), 150.0, (215.0, 215.0));
                p.dot(srgb(0xb8705e), 80.0, (245.0, 245.0));
                p.ink(srgb(0x8fb3d9), 60.0, &line((40.0, 60.0), (320.0, 60.0), 3.0));
                p.ink(srgb(0x5d84b3), 30.0, &line((40.0, 75.0), (320.0, 75.0), 3.0));
                p.ink(srgb(0x1b1b22), 5.0, &arc((180.0, 180.0), 110.0, 0.0, TAU, 2.0));
                p.ink(srgb(0x1b1b22), 5.0, &line((30.0, 98.0), (330.0, 98.0), 2.0));
                p.ink(srgb(0x1b1b22), 4.0, &arc((150.0, 160.0), 24.0, 0.0, TAU, 2.0));
            },
            blend: |p, b| {
                for (from, to) in [((60.0, 150.0), (300.0, 150.0)), ((300.0, 215.0), (60.0, 215.0)), ((80.0, 270.0), (290.0, 270.0)), ((60.0, 85.0), (300.0, 85.0))] {
                    p.blend(b, &pressured(line(from, to, 2.0), 0.85));
                }
                p.blend(b, &pressured(arc((180.0, 180.0), 85.0, 0.3, 2.8, 2.0), 0.85));
            },
        },
        Scene {
            key: "hair", title: "Hair",
            note: "Rough chalky locks in three tones; one stroke scrubs across the locks, two run along them.",
            seed: |p| {
                p.fill(srgb(0x3b2a20), [20.0, 20.0, 340.0, 340.0]);
                for (i, color) in [0x6b4a33, 0xa8774f, 0xd9a86c, 0x8a5c3c, 0xc48e5c, 0x5a3d2a].into_iter().cycle().take(16).enumerate() {
                    let x0 = 20.0 + i as f32 * 20.0;
                    let phase = (i * 5 % 7) as f32;
                    let points: Vec<_> = (0..=60).map(|k| {
                        let t = k as f32 / 60.0;
                        (x0 + 8.0 * (t * 3.1 + phase).sin() + 40.0 * t * t, 25.0 + 310.0 * t)
                    }).collect();
                    p.texture(Preset::Chalk, srgb(color), 22.0 + (i % 3) as f32 * 8.0, &points);
                }
            },
            blend: |p, b| {
                p.blend(b, &pressured(line((40.0, 120.0), (330.0, 120.0), 2.0), 0.85));
                for x in [120.0, 230.0] {
                    let points: Vec<_> = (0..=150).map(|k| { let t = k as f32 / 150.0; (x + 40.0 * t * t, 40.0 + 290.0 * t) }).collect();
                    p.blend(b, &pressured(points, 0.85));
                }
            },
        },
        Scene {
            key: "knife", title: "Color blocks",
            note: "Four flat blocks over a textured underlayer, scraped by three diagonal strokes of rising pressure.",
            seed: |p| {
                p.fill(srgb(0x30343f), [20.0, 20.0, 340.0, 340.0]);
                for (i, color) in [0xe4572e, 0xf3a712, 0x29335c, 0x669bbc].into_iter().enumerate() {
                    let (x, y) = (30.0 + (i % 2) as f32 * 155.0, 30.0 + (i / 2) as f32 * 155.0);
                    p.fill(srgb(color), [x, y, x + 145.0, y + 145.0]);
                }
            },
            blend: |p, b| {
                for (k, (from, to)) in [((50.0, 60.0), (310.0, 150.0)), ((40.0, 170.0), (320.0, 200.0)), ((60.0, 300.0), (300.0, 230.0))].into_iter().enumerate() {
                    let points: Vec<_> = line(from, to, 2.0).into_iter().enumerate()
                        .map(|(i, (x, y))| (x, y, ((0.35 + 0.3 * k as f32) * (0.6 + i as f32 / 300.0)).min(1.0))).collect();
                    p.blend(b, &points);
                }
            },
        },
        Scene {
            key: "brushwork", title: "Brushwork",
            note: "Warm and cool textured strokes meeting in the middle; blended across the seam and in a loop.",
            seed: |p| {
                p.fill(srgb(0xe9dcc4), [20.0, 20.0, 340.0, 340.0]);
                for (i, (color, y)) in [(0xc8553d, 40.0), (0xe08e45, 85.0), (0xb5452f, 130.0), (0xd9774a, 175.0), (0xc8553d, 220.0), (0xe08e45, 265.0), (0xa63d2a, 310.0)].into_iter().enumerate() {
                    let wobble = (i % 3) as f32 * 6.0;
                    p.texture(Preset::BristlePaintbrush, srgb(color), 46.0, &polyline(&[(40.0, y + wobble), (120.0, y - 8.0), (195.0, y + 6.0)], 3.0));
                }
                for (i, (color, y)) in [(0x2f6690, 50.0), (0x3a7ca5, 95.0), (0x16425b, 140.0), (0x81a4cd, 185.0), (0x2f6690, 230.0), (0x3a7ca5, 275.0), (0x16425b, 320.0)].into_iter().enumerate() {
                    let wobble = (i % 2) as f32 * 7.0;
                    p.texture(Preset::BristlePaintbrush, srgb(color), 46.0, &polyline(&[(320.0, y - wobble), (250.0, y + 8.0), (165.0, y - 4.0)], 3.0));
                }
            },
            blend: |p, b| {
                p.blend(b, &pressured(line((180.0, 30.0), (180.0, 330.0), 2.0), 0.85));
                p.blend(b, &pressured(line((150.0, 330.0), (210.0, 30.0), 2.0), 0.85));
                p.blend(b, &pressured(arc((180.0, 180.0), 90.0, 0.0, TAU, 2.0), 0.8));
            },
        },
        Scene {
            key: "portrait", title: "Rough painting",
            note: "Blocky color patches of a landscape, blended in loose diagonal strokes.",
            seed: |p| {
                p.fill(srgb(0x8fb8de), [20.0, 20.0, 340.0, 150.0]);
                p.fill(srgb(0xf3d9a4), [20.0, 120.0, 340.0, 170.0]);
                p.fill(srgb(0x4f6d7a), [20.0, 160.0, 340.0, 230.0]);
                p.fill(srgb(0x6b8f3c), [20.0, 220.0, 340.0, 340.0]);
                p.dot(srgb(0xfdf0c2), 60.0, (260.0, 80.0));
                for (x, y) in [(70.0, 250.0), (130.0, 270.0), (200.0, 255.0), (290.0, 280.0)] {
                    p.dot(srgb(0x2f4a1e), 46.0, (x, y));
                }
            },
            blend: |p, b| {
                for (from, to) in [((40.0, 40.0), (320.0, 150.0)), ((40.0, 150.0), (320.0, 240.0)), ((40.0, 240.0), (320.0, 320.0)), ((320.0, 60.0), (60.0, 120.0))] {
                    p.blend(b, &pressured(line(from, to, 2.0), 0.85));
                }
            },
        },
    ]
}

pub struct Blender {
    pub key: String,
    pub title: String,
    pub brush: Option<BrushSnapshot>,
}

pub(crate) fn prepared(mut brush: BrushSnapshot, diameter: f32) -> BrushSnapshot {
    brush.diameter = diameter * brush.aspect.min(1.0);
    brush.stabilization = BrushStabilization { strength: 0.0, pressure_smoothing: 0.0, ..brush.stabilization };
    brush
}

/// Every brush in the Blend set, after the scenes before blending.
pub fn catalog() -> Vec<Blender> {
    std::iter::once(Blender { key: "before".into(), title: "Before".into(), brush: None })
        .chain(layer_ui::brush_catalog().filter(|choice| choice.group.tool() == layer_ui::Tool::Blend).map(|choice| Blender {
            key: choice.id.to_string(),
            title: choice.label.to_string(),
            brush: layer_ui::preset(choice.id).ok().map(default_brush),
        }))
        .collect()
}

pub fn run(directory: &Path, blenders: &[Blender]) -> Result<(), Box<dyn Error>> {
    let scenes = scenes();
    fs::create_dir_all(directory)?;
    for blender in blenders {
        let mut canvas = Canvas::configured([PANEL * COLUMNS, PANEL * ROWS], PAPER)?;
        for (index, scene) in scenes.iter().enumerate() {
            let origin = [(index as u32 % COLUMNS * PANEL) as f32, (index as u32 / COLUMNS * PANEL) as f32];
            let mut painter = Painter { canvas: &mut canvas, origin };
            (scene.seed)(&mut painter);
            if let Some(brush) = &blender.brush {
                let diameter = if scene.key == "pull" || scene.key == "stripes" { 70.0 } else { 90.0 };
                (scene.blend)(&mut painter, &prepared(brush.clone(), diameter));
            }
        }
        let [width, _] = canvas.engine.backend().document_extent();
        let rgba = {
            canvas.write_png(&directory.join(format!("{}.png", blender.key)))?;
            canvas.engine.backend_mut().readback_srgb_rgba8().map_err(|e| e.to_string())?
        };
        let folder = directory.join(&blender.key);
        fs::create_dir_all(&folder)?;
        for (index, scene) in scenes.iter().enumerate() {
            let [x0, y0] = [index as u32 % COLUMNS * PANEL, index as u32 / COLUMNS * PANEL];
            let panel: Vec<u8> = (y0..y0 + PANEL).flat_map(|y| {
                let start = ((y * width + x0) * 4) as usize;
                rgba[start..start + (PANEL * 4) as usize].to_vec()
            }).collect();
            write_rgba(&folder.join(format!("{}.png", scene.key)), PANEL, PANEL, &panel)?;
        }
        eprintln!("blend study: {}", blender.title);
    }
    write_index(directory, blenders, &scenes)
}

fn write_rgba(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn write_index(directory: &Path, blenders: &[Blender], scenes: &[Scene]) -> Result<(), Box<dyn Error>> {
    let mut html = String::from("<!doctype html><meta charset=utf-8><title>Blend study</title><style>\
        body{font:13px system-ui;margin:16px}table{border-collapse:collapse}td,th{padding:4px;vertical-align:top;text-align:left}\
        img{width:180px;height:180px;display:block}th small{display:block;font-weight:normal;max-width:180px}</style><table><tr><th></th>");
    for scene in scenes {
        html.push_str(&format!("<th>{}<small>{}</small></th>", scene.title, scene.note));
    }
    for blender in blenders {
        html.push_str(&format!("</tr><tr><th>{}</th>", blender.title));
        for scene in scenes {
            html.push_str(&format!("<td><img src=\"{}/{}.png\"></td>", blender.key, scene.key));
        }
    }
    html.push_str("</tr></table>");
    fs::write(directory.join("index.html"), html)?;
    Ok(())
}

pub(crate) fn scrub(from: f32, to: f32, top: f32, bottom: f32, step: f32) -> Vec<(f32, f32, f32)> {
    let mut points = vec![];
    let mut x = from;
    let mut down = true;
    while x <= to {
        points.push((x, if down { top } else { bottom }));
        down = !down;
        x += step;
    }
    pressured(polyline(&points, 2.0), 0.9)
}




