//! Keep the battery numerals identical to Android without changing the UI font.
use pango::prelude::*;
use std::{
    cell::OnceCell,
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
};

thread_local! {
    // Pango/FreeType can reopen this path later. Keep the anonymous file alive
    // for the lifetime of the GTK thread, without installing or caching a font.
    static FONT: OnceCell<Option<File>> = const { OnceCell::new() };
}

pub(super) fn load(context: &pango::Context) {
    FONT.with(|font| {
        font.get_or_init(|| {
            let result = (|| -> Result<File, Box<dyn std::error::Error>> {
                let map = context.font_map().ok_or("No Pango font map")?;
                // SAFETY: valid NUL-terminated name and supported Linux flags.
                let fd = unsafe {
                    libc::memfd_create(c"capy-battery-numerals".as_ptr(), libc::MFD_CLOEXEC)
                };
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                // SAFETY: memfd_create returned a new descriptor owned here.
                let mut file = unsafe { File::from_raw_fd(fd) };
                file.write_all(include_bytes!("../fonts/CapyBatteryNumerals-Bold.ttf"))?;
                map.add_font_file(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
                Ok(file)
            })();
            match result {
                Ok(file) => Some(file),
                Err(error) => {
                    eprintln!("Could not load battery numerals: {error}");
                    None
                }
            }
        });
    });
}
