use layer_color::photo::{DecodeLimits,DecodedPhoto};
use layer_host::photo_reader::{active,arguments};
use std::{fs::File,io::Read,os::fd::AsRawFd,path::Path,process::{Child,Command},sync::atomic::AtomicBool,time::{Duration,Instant}};

const DEADLINE:Duration=Duration::from_secs(120);
struct Job(Child);
impl Drop for Job {fn drop(&mut self) {let _=self.0.kill();let _=self.0.wait();}}
pub(crate) fn read(input:impl Read,limits:DecodeLimits,cancelled:&AtomicBool)->Result<DecodedPhoto,String> {
    decode(input,limits,cancelled,&layer_host::photo_reader::helper()?,DEADLINE)
}
pub(crate) fn decode(input:impl Read,limits:DecodeLimits,cancelled:&AtomicBool,helper:&Path,timeout:Duration)->Result<DecodedPhoto,String> {
    let deadline=Instant::now()+timeout;
    layer_host::photo_reader::decode(input,limits,cancelled,|job| {
        let input=File::open(format!("/proc/self/fd/{}",job.input.as_raw_fd())).map_err(|e|e.to_string())?;
        let allowance=limits.source_bytes.saturating_add(limits.codec_bytes);
        let mut child=Job(Command::new(helper).env_clear().args(arguments(limits)).args([allowance.to_string(),std::process::id().to_string()])
            .stdin(input).stdout(job.output.try_clone().map_err(|e|e.to_string())?).stderr(job.errors.try_clone().map_err(|e|e.to_string())?)
            .spawn().map_err(|e|format!("Photo decoder is unavailable: {e}"))?);
        loop {
            if let Some(status)=child.0.try_wait().map_err(|e|e.to_string())? {return Ok(status.success());}
            active(cancelled)?;
            if Instant::now()>=deadline {return Err("Photo decoding timed out".into());}
            std::thread::sleep(Duration::from_millis(10));
        }
    })
}
