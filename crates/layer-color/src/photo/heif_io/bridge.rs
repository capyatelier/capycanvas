//! The C bridge owns the versioned libheif structs; this ABI stays small and is
//! checked before use. No encoded input supplies a library name or search path.
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    path::PathBuf,
    ptr::NonNull,
    sync::OnceLock,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Info {
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    pub storage_bpp: u32,
    pub alpha: u32,
    pub premultiplied: u32,
    pub images: u32,
    pub quarter_turns: u32,
    pub nclx: u32,
    pub primaries: u32,
    pub transfer: u32,
    pub matrix: u32,
    pub full_range: u32,
    pub chromaticities: [f32; 8],
    pub icc_bytes: u64,
    pub exif_bytes: u64,
    pub plane_width: u32,
    pub plane_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    pub plane_turns: u32,
    pub mirror: u32,
    pub first_frame: u32,
    pub gain_map: u32,
}
pub(super) type Cancel = unsafe extern "C" fn(*mut c_void) -> c_int;
struct Api {
    _library: libloading::Library,
    open: unsafe extern "C" fn(
        *const u8,
        usize,
        u64,
        u32,
        u32,
        Cancel,
        *mut c_void,
        *mut *mut c_void,
        *mut Info,
        *mut c_char,
    ) -> c_int,
    close: unsafe extern "C" fn(*mut c_void),
    metadata: unsafe extern "C" fn(*mut c_void, c_int, *mut u8, usize, *mut c_char) -> c_int,
    decode: unsafe extern "C" fn(
        *mut c_void,
        u64,
        Cancel,
        *mut c_void,
        *mut Info,
        *mut *const u8,
        *mut usize,
        *mut c_char,
    ) -> c_int,
}
static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn paths() -> Vec<PathBuf> {
    if let Some(directory) = std::env::var_os("CAPY_PHOTO_CODEC_DIR") {
        return vec![PathBuf::from(directory).join("libcapy_photo.so.1")];
    }
    let Ok(executable) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(parent) = executable.parent() else {
        return Vec::new();
    };
    vec![
        parent.join("../lib/capycanvas/photo/libcapy_photo.so.1"),
        parent.join("../photo-codecs/prefix/lib/libcapy_photo.so.1"),
        parent.join("../../photo-codecs/prefix/lib/libcapy_photo.so.1"),
    ]
}
fn api() -> Result<&'static Api, String> {
    API.get_or_init(|| {
        let path = paths().into_iter().find(|p| p.is_file())
            .ok_or("HEIF/AVIF photo codecs are not installed")?;
        // SAFETY: a trusted installed/developer bundle, held for process lifetime.
        // The C bridge's ABI number and structure size are checked before calls.
        unsafe {
            let library = libloading::Library::new(&path).map_err(|e| format!("Cannot load HEIF/AVIF photo codecs: {e}"))?;
            macro_rules! symbol { ($name:literal, $ty:ty) => { *library.get::<$ty>(concat!($name, "\0").as_bytes()).map_err(|e| e.to_string())? }; }
            let abi = symbol!("capy_photo_abi", unsafe extern "C" fn() -> u32);
            let size = symbol!("capy_photo_info_size", unsafe extern "C" fn() -> usize);
            let decoder = symbol!("capy_photo_decoder", unsafe extern "C" fn(c_int) -> c_int);
            let version = symbol!("capy_photo_version", unsafe extern "C" fn() -> *const c_char);
            if abi() != 3 || size() != std::mem::size_of::<Info>() {
                return Err("The HEIF/AVIF codec bridge is incompatible".into());
            }
            let version = version();
            if version.is_null() { return Err("Missing HEIF codec version".into()); }
            let parts: Vec<u32> = CStr::from_ptr(version).to_string_lossy().split('.')
                .map(str::parse).collect::<Result<_, _>>().map_err(|_| "Invalid HEIF codec version")?;
            let avif_version = symbol!("capy_photo_avif_version", unsafe extern "C" fn() -> *const c_char)();
            if avif_version.is_null() { return Err("Missing AVIF codec version".into()); }
            let avif_parts: Vec<u32> = CStr::from_ptr(avif_version).to_string_lossy().split('.')
                .map(str::parse).collect::<Result<_, _>>().map_err(|_| "Invalid AVIF codec version")?;
            if parts.as_slice() < [1, 23, 4].as_slice() || avif_parts.as_slice() < [1, 4, 2].as_slice()
                || decoder(1) == 0 || decoder(4) == 0 {
                return Err("HEIF/AVIF requires libheif 1.23.4 and libavif 1.4.2 or newer with HEVC and AV1 decoders".into());
            }
            Ok(Api {
                open: symbol!("capy_photo_open", _), close: symbol!("capy_photo_close", _),
                metadata: symbol!("capy_photo_metadata", _), decode: symbol!("capy_photo_decode", _),
                _library: library,
            })
        }
    }).as_ref().map_err(Clone::clone)
}
pub(super) fn available() -> bool {
    api().is_ok()
}

fn call(action: impl FnOnce(*mut c_char) -> c_int) -> Result<(), String> {
    let mut message = [0 as c_char; 512];
    if action(message.as_mut_ptr()) != 0 {
        return Ok(());
    }
    // The bridge's failure path writes a bounded, terminated diagnostic.
    Err(unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

pub(super) struct Photo {
    handle: NonNull<c_void>,
    api: &'static Api,
    // This input is borrowed by libheif and must outlive its context.
    _encoded: Vec<u8>,
    pub info: Info,
}
unsafe extern "C" fn cancel(data: *mut c_void) -> c_int {
    // The AtomicBool outlives this synchronous FFI call and all codec threads.
    // The bridge retires the borrowed pointer before returning, including errors.
    i32::from(
        unsafe { &*data.cast::<std::sync::atomic::AtomicBool>() }
            .load(std::sync::atomic::Ordering::Acquire),
    )
}
impl Photo {
    pub(super) fn encoded(&self) -> &[u8] { &self._encoded }
    pub fn open(
        encoded: Vec<u8>,
        budget: usize,
        dimension: u32,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        let api = api()?;
        let mut handle = std::ptr::null_mut();
        let mut info = Info::default();
        call(|message| unsafe {
            (api.open)(
                encoded.as_ptr(),
                encoded.len(),
                budget as u64,
                dimension,
                crate::MAX_ICC_BYTES as u32,
                cancel,
                std::ptr::from_ref(cancelled).cast_mut().cast(),
                &mut handle,
                &mut info,
                message,
            )
        })?;
        let handle = NonNull::new(handle).ok_or("HEIF returned no image context")?;
        Ok(Self {
            handle,
            api,
            _encoded: encoded,
            info,
        })
    }
    pub fn metadata(&self, exif: bool) -> Result<Vec<u8>, String> {
        let size = if exif {
            self.info.exif_bytes
        } else {
            self.info.icc_bytes
        };
        let size = usize::try_from(size)
            .ok()
            .filter(|v| *v <= crate::MAX_ICC_BYTES)
            .ok_or("HEIF metadata exceeds the limit")?;
        if size == 0 {
            return Ok(Vec::new());
        }
        let mut bytes = super::super::raster_io::allocate(size)?;
        call(|message| unsafe {
            (self.api.metadata)(
                self.handle.as_ptr(),
                i32::from(exif),
                bytes.as_mut_ptr(),
                size,
                message,
            )
        })?;
        Ok(bytes)
    }
    pub fn decode(
        &mut self,
        budget: usize,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Plane<'_>, String> {
        let mut pixels = std::ptr::null();
        let mut stride = 0;
        call(|message| unsafe {
            (self.api.decode)(
                self.handle.as_ptr(),
                budget as u64,
                cancel,
                std::ptr::from_ref(cancelled).cast_mut().cast(),
                &mut self.info,
                &mut pixels,
                &mut stride,
                message,
            )
        })?;
        if !matches!(self.info.storage_bpp, 4 | 8) {
            return Err("Invalid HEIF sample storage".into());
        }
        let row_bytes = (self.info.plane_width as usize)
            .checked_mul(self.info.storage_bpp as usize)
            .ok_or("HEIF row size overflow")?;
        let length = stride
            .checked_mul(self.info.plane_height as usize)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or("HEIF decoded plane size overflow")?;
        if pixels.is_null() || stride < row_bytes {
            return Err("Invalid HEIF decoded plane".into());
        }
        let i = self.info;
        if i.crop_width == 0
            || i.crop_height == 0
            || i.plane_turns > 3
            || i.mirror > 2
            || i.crop_x
                .checked_add(i.crop_width)
                .is_none_or(|x| x > i.plane_width)
            || i.crop_y
                .checked_add(i.crop_height)
                .is_none_or(|y| y > i.plane_height)
            || [i.width, i.height]
                != if i.plane_turns % 2 == 0 {
                    [i.crop_width, i.crop_height]
                } else {
                    [i.crop_height, i.crop_width]
                }
        {
            return Err("Invalid HEIF/AVIF display geometry".into());
        }
        Ok(Plane {
            info: self.info,
            stride,
            row_bytes,
            // The C bridge gets dimensions and pitch from the decoded channel,
            // not its untrusted item header. This borrow cannot outlive Photo.
            pixels: unsafe { std::slice::from_raw_parts(pixels, length) },
        })
    }
}
impl Drop for Photo {
    fn drop(&mut self) {
        unsafe { (self.api.close)(self.handle.as_ptr()) };
    }
}
pub(super) struct Plane<'a> {
    pub info: Info,
    stride: usize,
    row_bytes: usize,
    pixels: &'a [u8],
}
impl Plane<'_> {
    fn row(&self, y: usize) -> &[u8] {
        &self.pixels[y * self.stride..y * self.stride + self.row_bytes]
    }
    pub fn pixel(&self, x: u32, y: u32) -> &[u8] {
        let i = self.info;
        // Display order is crop, rotate counterclockwise, then mirror. Invert
        // it to gather original samples once; no resampling is involved.
        let x = if i.mirror == 2 { i.width - 1 - x } else { x };
        let y = if i.mirror == 1 { i.height - 1 - y } else { y };
        let (sx, sy) = match i.plane_turns {
            1 => (i.crop_width - 1 - y, x),
            2 => (i.crop_width - 1 - x, i.crop_height - 1 - y),
            3 => (y, i.crop_height - 1 - x),
            _ => (x, y),
        };
        let bpp = i.storage_bpp as usize;
        let start = (sx + i.crop_x) as usize * bpp;
        &self.row((sy + i.crop_y) as usize)[start..start + bpp]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    #[ignore = "requires LAYER_AVIF_REFERENCES and the native codec bundle"]
    fn heif_avif_cancellation_inside_native_parser_releases_context() {
        unsafe extern "C" fn stop_after_reads(data: *mut c_void) -> c_int {
            let reads = unsafe { &*data.cast::<AtomicUsize>() };
            i32::from(reads.fetch_add(1, Ordering::Relaxed) >= 8)
        }
        let root = std::path::PathBuf::from(
            std::env::var_os("LAYER_AVIF_REFERENCES").expect("AVIF references"),
        );
        let bytes = std::fs::read(root.join("sequence-different-poster.avif")).unwrap();
        let api = api().unwrap();
        let reads = AtomicUsize::new(0);
        let mut handle = std::ptr::null_mut();
        let mut info = Info::default();
        let message = call(|message| unsafe {
            (api.open)(
                bytes.as_ptr(),
                bytes.len(),
                128 * 1024 * 1024,
                32768,
                crate::MAX_ICC_BYTES as u32,
                stop_after_reads,
                std::ptr::from_ref(&reads).cast_mut().cast(),
                &mut handle,
                &mut info,
                message,
            )
        })
        .unwrap_err();
        assert!(message.contains("cancelled"), "{message}");
        assert!(
            handle.is_null(),
            "cancelled native parsing must not publish a context"
        );
        assert!(
            reads.load(Ordering::Relaxed) > 8,
            "cancellation must occur after native parsing starts"
        );
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let mut retry = Photo::open(bytes, 128 * 1024 * 1024, 32768, &cancelled).unwrap();
        assert_eq!(
            retry
                .decode(128 * 1024 * 1024, &cancelled)
                .unwrap()
                .info
                .first_frame,
            1
        );
    }
}
