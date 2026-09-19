use rav1d::include::dav1d::{data::Dav1dData, dav1d::Dav1dSettings, picture::Dav1dPicture};
use rav1d::src::lib::*;
use std::{mem::MaybeUninit, ptr::NonNull};

fn check(input: &[u8], depth: i32) {
    // This isolated fixture driver owns the context, packet, and picture for
    // each call and releases them after reading the decoder-owned planes.
    unsafe {
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap());
        let mut settings = settings.assume_init();
        settings.n_threads = 1;
        settings.max_frame_delay = 1;
        settings.frame_size_limit = 64 * 32;
        settings.strict_std_compliance = 1;
        settings.logger = rav1d::include::dav1d::dav1d::Dav1dLogger::new(None, None);
        let mut context = None;
        assert_eq!(
            dav1d_open(
                Some(NonNull::from(&mut context)),
                Some(NonNull::from(&mut settings))
            )
            .0,
            0
        );
        let mut packet = Dav1dData::default();
        let data = dav1d_data_create(Some(NonNull::from(&mut packet)), input.len());
        assert!(!data.is_null());
        data.copy_from_nonoverlapping(input.as_ptr(), input.len());
        assert_eq!(
            dav1d_send_data(context, Some(NonNull::from(&mut packet))).0,
            0
        );
        let mut picture = Dav1dPicture::default();
        assert_eq!(
            dav1d_get_picture(context, Some(NonNull::from(&mut picture))).0,
            0
        );
        assert_eq!((picture.p.w, picture.p.h, picture.p.bpc), (64, 32, depth));
        // Identity matrix encodes G/B/R directly as Y/U/V in full-range 4:4:4.
        let factors = [37, 17, 7];
        let offsets = [19, 301, 151];
        for (plane, channel) in [1, 2, 0].into_iter().enumerate() {
            let data = picture.data[plane].unwrap().cast::<u8>().as_ptr();
            let stride = picture.stride[usize::from(plane != 0)];
            for y in 0..32 {
                for x in 0..64 {
                    let at = data
                        .offset(y * stride)
                        .add(x * if depth == 8 { 1 } else { 2 });
                    let actual = if depth == 8 {
                        *at as u16
                    } else {
                        at.cast::<u16>().read_unaligned()
                    };
                    let expected = (((y as usize * 64 + x) * factors[channel] + offsets[channel])
                        & ((1 << depth) - 1)) as u16;
                    assert_eq!(actual, expected, "depth={depth} plane={plane} x={x} y={y}");
                }
            }
        }
        dav1d_picture_unref(Some(NonNull::from(&mut picture)));
        dav1d_data_unref(Some(NonNull::from(&mut packet)));
        dav1d_close(Some(NonNull::from(&mut context)));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn verify_all_depths() -> u32 {
    check(include_bytes!("../fixtures/p3-8bit.obu"), 8);
    check(include_bytes!("../fixtures/p3-10bit.obu"), 10);
    check(include_bytes!("../fixtures/p3-12bit.obu"), 12);
    3
}

#[test]
fn lossless_native_rust() {
    assert_eq!(verify_all_depths(), 3);
}
