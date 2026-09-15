//! The opaque C codec owns all libjpeg state. Its setjmp never crosses a Rust
//! frame; callbacks catch panics and return an error before C raises one.
use std::ffi::{c_char, c_int, c_void};
use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;

#[repr(C)]
#[derive(Default)]
pub(super) struct Info {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub precision: u32,
    pub adobe: u32,
    pub transform: u32,
    pub multiple_scans: u32,
}
type ReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, usize) -> isize;
type WriteFn = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> c_int;
unsafe extern "C" {
    fn capy_jpeg_decoder_new(
        read: ReadFn,
        io: *mut c_void,
        info: *mut Info,
        error: *mut c_char,
    ) -> *mut c_void;
    fn capy_jpeg_decoder_start(codec: *mut c_void, budget: usize, error: *mut c_char) -> c_int;
    fn capy_jpeg_decoder_row(
        codec: *mut c_void,
        row: *mut u8,
        size: usize,
        error: *mut c_char,
    ) -> c_int;
    fn capy_jpeg_decoder_finish(codec: *mut c_void, error: *mut c_char) -> c_int;
    fn capy_jpeg_decoder_free(codec: *mut c_void);
    fn capy_jpeg_encoder_new(
        write: WriteFn,
        io: *mut c_void,
        w: u32,
        h: u32,
        channels: c_int,
        quality: c_int,
        density_unit: c_int,
        density_x: u32,
        density_y: u32,
        error: *mut c_char,
    ) -> *mut c_void;
    fn capy_jpeg_encoder_marker(
        codec: *mut c_void,
        marker: c_int,
        bytes: *const u8,
        size: usize,
        error: *mut c_char,
    ) -> c_int;
    fn capy_jpeg_encoder_row(
        codec: *mut c_void,
        row: *const u8,
        size: usize,
        error: *mut c_char,
    ) -> c_int;
    fn capy_jpeg_encoder_finish(codec: *mut c_void, error: *mut c_char) -> c_int;
    fn capy_jpeg_encoder_free(codec: *mut c_void);
}

struct Io<T> {
    value: T,
    error: Option<String>,
}
fn callback<T, V>(io: &mut Io<T>, f: impl FnOnce(&mut T) -> std::io::Result<V>) -> Option<V> {
    if io.error.is_some() {
        return None;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        f(&mut io.value).map_err(|e| e.to_string())
    })) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            io.error = Some(error);
            None
        }
        Err(payload) => {
            // A custom I/O implementation may even panic with a payload whose
            // destructor panics. Neither unwind may leave this C callback.
            std::mem::forget(payload);
            io.error = Some("JPEG I/O callback panicked".into());
            None
        }
    }
}
unsafe extern "C" fn read<T: Read>(opaque: *mut c_void, out: *mut u8, len: usize) -> isize {
    // Both pointers belong to the live codec call: Io is boxed, and C supplies
    // its fixed writable input buffer. Read cannot outlive this callback.
    let io = unsafe { &mut *opaque.cast::<Io<T>>() };
    let out = unsafe { std::slice::from_raw_parts_mut(out, len) };
    callback(io, |r| {
        loop {
            match r.read(out) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    })
    .map_or(-1, |n| n as isize)
}
unsafe extern "C" fn write<T: Write>(opaque: *mut c_void, bytes: *const u8, len: usize) -> c_int {
    let io = unsafe { &mut *opaque.cast::<Io<T>>() };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    c_int::from(callback(io, |w| w.write_all(bytes)).is_some())
}
fn message<T>(io: &mut Io<T>, bytes: &[u8; 512]) -> String {
    io.error.take().unwrap_or_else(|| {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        if end == 0 {
            "JPEG codec did not complete the requested operation".into()
        } else {
            String::from_utf8_lossy(&bytes[..end]).into_owned()
        }
    })
}

pub(super) struct Decoder<R> {
    codec: NonNull<c_void>,
    io: Box<Io<R>>,
    failed: bool,
    pub info: Info,
}
impl<R: Read> Decoder<R> {
    pub fn new(input: R) -> Result<Self, String> {
        let mut io = Box::new(Io {
            value: input,
            error: None,
        });
        let mut info = Info::default();
        let mut error = [0u8; 512];
        let codec = unsafe {
            capy_jpeg_decoder_new(
                read::<R>,
                (&mut *io as *mut Io<R>).cast(),
                &mut info,
                error.as_mut_ptr().cast(),
            )
        };
        let codec = NonNull::new(codec).ok_or_else(|| message(&mut io, &error))?;
        Ok(Self {
            codec,
            io,
            info,
            failed: false,
        })
    }
    fn call(&mut self, f: impl FnOnce(*mut c_void, *mut c_char) -> c_int) -> Result<(), String> {
        if self.failed {
            return Err("JPEG codec is unavailable after a failed operation".into());
        }
        let mut error = [0u8; 512];
        if f(self.codec.as_ptr(), error.as_mut_ptr().cast()) != 0 {
            Ok(())
        } else {
            self.failed = true;
            Err(message(&mut self.io, &error))
        }
    }
    pub fn start(&mut self, budget: usize) -> Result<(), String> {
        self.call(|c, e| unsafe { capy_jpeg_decoder_start(c, budget, e) })
    }
    pub fn row(&mut self, row: &mut [u8]) -> Result<(), String> {
        self.call(|c, e| unsafe { capy_jpeg_decoder_row(c, row.as_mut_ptr(), row.len(), e) })
    }
    pub fn finish(&mut self) -> Result<(), String> {
        self.call(|c, e| unsafe { capy_jpeg_decoder_finish(c, e) })
    }
}
impl<R> Drop for Decoder<R> {
    fn drop(&mut self) {
        // Destruction is safe even after an error, never uses I/O callbacks,
        // and runs before the boxed I/O owner is released.
        unsafe { capy_jpeg_decoder_free(self.codec.as_ptr()) }
    }
}

pub(super) struct Encoder<W> {
    codec: NonNull<c_void>,
    io: Box<Io<W>>,
    failed: bool,
}
impl<W: Write> Encoder<W> {
    pub fn new(
        output: W,
        [w, h]: [u32; 2],
        channels: usize,
        quality: u8,
        resolution: Option<layer_core::ImageResolution>,
    ) -> Result<Self, String> {
        let (unit, [x, y]) = resolution
            .map(layer_core::ImageResolution::jfif_density)
            .transpose()?
            .unwrap_or((0, [1, 1]));
        let mut io = Box::new(Io {
            value: output,
            error: None,
        });
        let mut error = [0u8; 512];
        let codec = unsafe {
            capy_jpeg_encoder_new(
                write::<W>,
                (&mut *io as *mut Io<W>).cast(),
                w,
                h,
                channels as c_int,
                quality as c_int,
                c_int::from(unit),
                u32::from(x),
                u32::from(y),
                error.as_mut_ptr().cast(),
            )
        };
        let codec = NonNull::new(codec).ok_or_else(|| message(&mut io, &error))?;
        Ok(Self {
            codec,
            io,
            failed: false,
        })
    }
    fn call(&mut self, f: impl FnOnce(*mut c_void, *mut c_char) -> c_int) -> Result<(), String> {
        if self.failed {
            return Err("JPEG codec is unavailable after a failed operation".into());
        }
        let mut error = [0u8; 512];
        if f(self.codec.as_ptr(), error.as_mut_ptr().cast()) != 0 {
            Ok(())
        } else {
            self.failed = true;
            Err(message(&mut self.io, &error))
        }
    }
    pub fn profile(&mut self, profile: &[u8]) -> Result<(), String> {
        const PAYLOAD: usize = 65519;
        let count: u8 = profile
            .len()
            .div_ceil(PAYLOAD)
            .try_into()
            .map_err(|_| "JPEG ICC profile is too large")?;
        for (index, chunk) in profile.chunks(PAYLOAD).enumerate() {
            let mut data = Vec::with_capacity(chunk.len() + 14);
            data.extend_from_slice(b"ICC_PROFILE\0");
            data.extend_from_slice(&[index as u8 + 1, count]);
            data.extend_from_slice(chunk);
            self.marker(2, &data)?;
        }
        Ok(())
    }
    pub fn marker(&mut self, marker: u8, data: &[u8]) -> Result<(), String> {
        self.call(|c, e| unsafe {
            capy_jpeg_encoder_marker(c, c_int::from(marker), data.as_ptr(), data.len(), e)
        })
    }
    pub fn row(&mut self, row: &[u8]) -> Result<(), String> {
        self.call(|c, e| unsafe { capy_jpeg_encoder_row(c, row.as_ptr(), row.len(), e) })
    }
    pub fn finish(&mut self) -> Result<(), String> {
        self.call(|c, e| unsafe { capy_jpeg_encoder_finish(c, e) })
    }
}
impl<W> Drop for Encoder<W> {
    fn drop(&mut self) {
        unsafe { capy_jpeg_encoder_free(self.codec.as_ptr()) }
    }
}
