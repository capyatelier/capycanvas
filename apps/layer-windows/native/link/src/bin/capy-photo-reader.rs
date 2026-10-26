#[cfg(target_os = "windows")]
fn run() -> Result<(), String> {
    use std::{fs::File, io::{BufWriter, Write}, os::windows::io::{AsRawHandle, FromRawHandle}};
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let limits = layer_host::photo_reader::limits(&arguments)?;
    let input = unsafe { File::from_raw_handle(std::io::stdin().as_raw_handle()) };
    let mut output = BufWriter::new(unsafe { File::from_raw_handle(std::io::stdout().as_raw_handle()) });
    layer_host::photo_reader::serve(input, &mut output, limits)?;
    output.flush().map_err(|e| e.to_string())
}
#[cfg(target_os = "windows")]
fn terminate_before_library_teardown(code: u32) -> ! {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn TerminateProcess(process: isize, code: u32) -> i32;
    }
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}
#[cfg(not(target_os = "windows"))]
fn run() -> Result<(), String> {
    Err("The Windows photo decoder runs only on Windows".into())
}
#[cfg(not(target_os = "windows"))]
fn terminate_before_library_teardown(code: u32) -> ! {
    std::process::exit(code as i32)
}
fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{}", error.chars().take(1024).collect::<String>());
            1
        }
    };
    terminate_before_library_teardown(code)
}
