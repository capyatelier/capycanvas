//! Ownership wrapper around the Rust rav1d decoder's dav1d-compatible API.
use super::container::Result;
use rav1d::{
    include::dav1d::{
        data::Dav1dData,
        dav1d::{Dav1dContext, Dav1dLogger, Dav1dSettings},
        headers::Dav1dSequenceHeader,
        picture::Dav1dPicture,
    },
    src::{error::Rav1dError, lib::*},
};
use std::{
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err("Image read cancelled".into())
    } else {
        Ok(())
    }
}
struct Context(Option<Dav1dContext>);
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns the live handle from open.
        unsafe { dav1d_close(Some(NonNull::from(&mut self.0))) };
    }
}
struct Packet(Dav1dData);
impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: data_create/send_data maintain this initialized packet.
        unsafe { dav1d_data_unref(Some(NonNull::from(&mut self.0))) };
    }
}
pub(super) struct Picture {
    raw: Dav1dPicture,
    pub extent: [u32; 2],
    pub depth: u8,
    pub layout: u32,
    pub cicp: [u16; 3],
    pub full_range: bool,
}
impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: the picture retains the decoder's Arc-owned planes and is
        // released exactly once, after all safe pixel access through &self.
        unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.raw))) };
    }
}
impl Picture {
    pub fn plane_extent(&self, plane: usize) -> [u32; 2] {
        if plane == 0 {
            self.extent
        } else {
            [
                if self.layout == 1 || self.layout == 2 {
                    self.extent[0].div_ceil(2)
                } else {
                    self.extent[0]
                },
                if self.layout == 1 {
                    self.extent[1].div_ceil(2)
                } else {
                    self.extent[1]
                },
            ]
        }
    }
    pub fn sample(&self, plane: usize, x: u32, y: u32) -> u16 {
        assert!(plane < 3 && (plane == 0 || self.layout != 0));
        let extent = self.plane_extent(plane);
        assert!(x < extent[0] && y < extent[1]);
        let stride = self.raw.stride[usize::from(plane != 0)];
        let ptr = self.raw.data[plane]
            .expect("validated AV1 plane")
            .cast::<u8>()
            .as_ptr();
        // SAFETY: get_picture validation checks plane presence and stride.
        // Coordinates are bounded above; the owned picture keeps storage live.
        unsafe {
            let ptr = ptr
                .offset(y as isize * stride)
                .add(x as usize * if self.depth == 8 { 1 } else { 2 });
            if self.depth == 8 {
                u16::from(*ptr)
            } else {
                ptr.cast::<u16>().read_unaligned()
            }
        }
    }
}

// Validate every sequence header, including later spatial layers, before any
// decoder plane allocation. A byte-count frame limit alone cannot constrain
// pathological aspect ratios or a later sequence header replacing the first.
fn headers(bytes: &[u8], expected: [u32; 2], cancel: &AtomicBool) -> Result<(bool, usize)> {
    let mut at = 0usize;
    let mut found = false;
    let mut frames = 0usize;
    let mut count = 0usize;
    while at < bytes.len() {
        check(cancel)?;
        count += 1;
        if count > 65_536 {
            return Err("Too many AV1 OBUs".into());
        }
        let start = at;
        let header = bytes[at];
        at += 1;
        if header & 0x81 != 0 || header & 2 == 0 {
            return Err("Invalid AVIF AV1 OBU header".into());
        }
        let kind = (header >> 3) & 15;
        if header & 4 != 0 {
            let ext = *bytes.get(at).ok_or("Truncated AV1 extension")?;
            at += 1;
            if ext & 7 != 0 {
                return Err("Invalid AV1 OBU extension".into());
            }
        }
        let mut len = 0u64;
        let mut complete = false;
        for shift in (0..56).step_by(7) {
            let b = *bytes.get(at).ok_or("Truncated AV1 OBU length")?;
            at += 1;
            len |= u64::from(b & 127) << shift;
            if b & 128 == 0 {
                complete = true;
                break;
            }
        }
        if !complete {
            return Err("Invalid AV1 OBU length".into());
        }
        let len = usize::try_from(len).map_err(|_| "AV1 OBU size overflow")?;
        at = at
            .checked_add(len)
            .filter(|&n| n <= bytes.len())
            .ok_or("Truncated AV1 OBU")?;
        if kind == 1 {
            let mut out = MaybeUninit::<Dav1dSequenceHeader>::uninit();
            // SAFETY: the output is writable and the input is this complete,
            // immutable OBU slice. The API does not retain either pointer.
            let status = unsafe {
                dav1d_parse_sequence_header(
                    NonNull::new(out.as_mut_ptr()),
                    NonNull::new(bytes[start..at].as_ptr().cast_mut()),
                    at - start,
                )
            }
            .0;
            if status != 0 {
                return Err(format!("Invalid AV1 sequence header ({status})"));
            }
            // SAFETY: success fully initialized the sequence header.
            let out = unsafe { out.assume_init() };
            if out.max_width <= 0
                || out.max_height <= 0
                || out.max_width as u32 > expected[0]
                || out.max_height as u32 > expected[1]
            {
                return Err("AV1 sequence dimensions exceed the admitted image".into());
            }
            found = true;
        }
        if matches!(kind, 3 | 6) {
            frames += 1;
        }
    }
    Ok((found, frames))
}

pub(super) fn decode(
    bytes: &[u8],
    config: &[u8],
    expected: [u32; 2],
    budget: usize,
    cancel: &AtomicBool,
) -> Result<Picture> {
    check(cancel)?;
    super::super::validate_extent(expected, 32768)?;
    let (in_config, config_frames) = headers(config, expected, cancel)?;
    let (in_data, frames) = headers(bytes, expected, cancel)?;
    if (!in_config && !in_data) || config_frames != 0 {
        return Err("Missing AV1 sequence header or invalid configuration OBUs".into());
    }
    let packet_size = config
        .len()
        .checked_add(bytes.len())
        .ok_or("AV1 packet size overflow")?;
    let aligned =
        (expected[0] as usize).div_ceil(128) * 128 * (expected[1] as usize).div_ceil(128) * 128;
    // Single-thread decode avoids full-frame parallel coefficient storage.
    // Reserve planes for all possible retained frame references (at most nine),
    // filtering/super-resolution scratch, and fixed decoder state. Caller
    // separately admits the encoded input, composed image and packing band.
    let required = aligned
        .checked_mul(8 * frames.clamp(1, 9) + 16)
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .and_then(|n| n.checked_add(packet_size))
        .ok_or("AV1 workspace size overflow")?;
    if required > budget {
        return Err("AV1 decoder exceeds the codec budget".into());
    }
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: output points to writable storage of the settings type.
    unsafe { dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap()) };
    // SAFETY: default_settings initializes all fields.
    let mut settings = unsafe { settings.assume_init() };
    settings.n_threads = 1;
    settings.max_frame_delay = 1;
    settings.all_layers = 0;
    settings.frame_size_limit = expected[0]
        .checked_mul(expected[1])
        .ok_or("AV1 image size overflow")?;
    settings.strict_std_compliance = 1;
    // SAFETY: absent cookie and callback require no lifetime/thread contract.
    settings.logger = unsafe { Dav1dLogger::new(None, None) };
    let mut context = Context(None);
    // SAFETY: initialized settings and writable exclusively owned output.
    let status = unsafe {
        dav1d_open(
            Some(NonNull::from(&mut context.0)),
            Some(NonNull::from(&mut settings)),
        )
    }
    .0;
    if status != 0 {
        return Err(format!("Cannot initialize Rust AV1 decoder ({status})"));
    }
    let mut packet = Packet(Dav1dData::default());
    // SAFETY: initialized empty packet and an admitted input size.
    let ptr = unsafe { dav1d_data_create(Some(NonNull::from(&mut packet.0)), packet_size) };
    if ptr.is_null() {
        return Err("AV1 packet allocation failed".into());
    }
    // SAFETY: data_create allocated packet_size bytes; both disjoint input
    // slices fit exactly and remain valid throughout the copies.
    unsafe {
        ptr.copy_from_nonoverlapping(config.as_ptr(), config.len());
        ptr.add(config.len())
            .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
    }
    let again = -(Rav1dError::EAGAIN as i32);
    let mut result = None;
    let mut rounds = 0;
    loop {
        check(cancel)?;
        rounds += 1;
        if rounds > 65_536 {
            return Err("AV1 decoder did not finish the image".into());
        }
        if packet.0.sz > 0 {
            // SAFETY: live context and exclusively owned initialized packet;
            // send_data updates its ownership fields even on errors.
            let status =
                unsafe { dav1d_send_data(context.0, Some(NonNull::from(&mut packet.0))) }.0;
            if status != 0 && status != again {
                return Err(format!("Invalid AV1 image ({status})"));
            }
        }
        let mut picture = Picture {
            raw: Dav1dPicture::default(),
            extent: expected,
            depth: 8,
            layout: 0,
            cicp: [2; 3],
            full_range: false,
        };
        // SAFETY: live context and fresh writable picture, released on every
        // path by its owner. No references into it escape this wrapper.
        let status =
            unsafe { dav1d_get_picture(context.0, Some(NonNull::from(&mut picture.raw))) }.0;
        if status == again {
            if packet.0.sz == 0 {
                break;
            }
            return Err("AV1 decoder made no progress".into());
        }
        if status != 0 {
            return Err(format!("Cannot decode AV1 image ({status})"));
        }
        let p = &picture.raw.p;
        if p.w <= 0
            || p.h <= 0
            || [p.w as u32, p.h as u32] != expected
            || !matches!(p.bpc, 8 | 10 | 12)
            || p.layout > 3
        {
            return Err("Decoded AV1 image disagrees with its container".into());
        }
        picture.depth = p.bpc as u8;
        picture.layout = p.layout;
        for plane in 0..if p.layout == 0 { 1 } else { 3 } {
            let extent = picture.plane_extent(plane);
            let row_bytes = extent[0] as usize * if p.bpc == 8 { 1 } else { 2 };
            if picture.raw.data[plane].is_none()
                || picture.raw.stride[usize::from(plane != 0)].unsigned_abs() < row_bytes
            {
                return Err("Invalid decoded AV1 plane".into());
            }
        }
        let header = picture
            .raw
            .seq_hdr
            .ok_or("Missing decoded AV1 color header")?;
        // SAFETY: the picture retains this immutable sequence-header allocation.
        let header = unsafe { header.as_ref() };
        picture.cicp = [header.pri as u16, header.trc as u16, header.mtrx as u16];
        picture.full_range = header.color_range != 0;
        result = Some(picture);
    }
    check(cancel)?;
    result.ok_or_else(|| "AV1 payload contains no decoded image".into())
}
