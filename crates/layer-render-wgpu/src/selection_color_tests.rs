#[test]
fn global_color_selection_includes_disconnected_islands_and_refines_them() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let mut flood = Flood::new(&r.device);
    let extent = [259u32, 35u32];
    let [w, h] = extent;
    let island =
        |x: u32, y: u32| (2..12).contains(&y) && ((2..12).contains(&x) || (245..257).contains(&x));
    let pixels: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let x = i % w;
            let y = i / w;
            if island(x, y) {
                [if x > 100 { 240 } else { 255 }, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        })
        .collect();
    let source = source(&r, extent, &pixels);
    for (tolerance, expansion, smoothing) in [
        (0., 0, 0.),
        (0.2, 0, 0.),
        (0.2, 1, 0.),
        (0.2, -1, 0.),
        (0.2, 0, 1.),
    ] {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let region = flood
            .encode_input(
                &r.device,
                &mut encoder,
                &source,
                extent,
                [5, 5],
                tolerance,
                None,
                RegionRefinement {
                    gap_closing: 0,
                    expansion,
                    smoothing,
                },
                None,
                false,
            )
            .unwrap();
        encoder.submit(&r.queue);
        let (words, _bounds) = read_region(&r, &region);
        let at = |x: u32, y: u32| {
            (words[8 + (y * w.div_ceil(8) + x / 8) as usize] >> ((x % 8) * 4)) & 15
        };
        for y in 0..h {
            for x in 0..w {
                let eligible = |x: i32, y: i32| {
                    x >= 0 && y >= 0 && island(x as u32, y as u32) && (tolerance > 0. || x < 100)
                };
                let original = eligible(x as i32, y as i32);
                if smoothing == 0. {
                    let expected = if expansion == 0 {
                        original
                    } else {
                        let neighbours = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)));
                        if expansion > 0 {
                            neighbours
                                .into_iter()
                                .any(|(dx, dy)| eligible(x as i32 + dx, y as i32 + dy))
                        } else {
                            neighbours
                                .into_iter()
                                .all(|(dx, dy)| eligible(x as i32 + dx, y as i32 + dy))
                        }
                    };
                    assert_eq!(
                        at(x, y),
                        if expected { 4 } else { 0 },
                        "{x},{y} tolerance={tolerance} expansion={expansion}"
                    );
                }
            }
        }
        assert_eq!(at(5, 5), 4);
        assert_eq!(at(250, 5), if tolerance > 0. { 4 } else { 0 });
        assert_eq!(at(100, 5), 0, "separate islands must not be bridged");
    }
}
