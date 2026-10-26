#[path="../photo_reader/sandbox.rs"]
mod sandbox;
use std::{fs::File,io::Write,os::fd::{FromRawFd,AsRawFd}};

fn run()->Result<(),String> {
    let args:Vec<String>=std::env::args().skip(1).collect();
    let [_,_,_,allowance,parent]=args.as_slice() else {return Err("Invalid photo decoder arguments".into());};
    let number=|value:&String|value.parse::<usize>().map_err(|e|e.to_string());
    sandbox::parent_death(number(parent)?)?;
    let limits=layer_host::photo_reader::limits(&args[..3])?;
    let input=unsafe {File::from_raw_fd(0)};
    if !input.metadata().map_err(|e|e.to_string())?.is_file() || unsafe {libc::fcntl(input.as_raw_fd(),libc::F_GETFL)}&libc::O_ACCMODE!=libc::O_RDONLY {return Err("Photo decoder requires read-only file input".into());}
    sandbox::restrict(number(allowance)?)?;
    let mut output=std::io::stdout().lock();
    layer_host::photo_reader::serve(input,&mut output,limits)?;
    output.flush().map_err(|e|e.to_string())
}
fn main() {
    if let Err(error)=run() {eprintln!("{}",error.chars().take(1024).collect::<String>());std::process::exit(1);}
}
