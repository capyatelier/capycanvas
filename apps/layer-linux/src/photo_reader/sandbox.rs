use seccompiler::{BpfProgram,SeccompAction,SeccompFilter};

pub fn parent_death(parent:usize)->Result<(),String> {
    let parent=libc::pid_t::try_from(parent).map_err(|e|e.to_string())?;
    if unsafe {libc::getppid()}!=parent {return Err("Photo decoder parent changed".into());}
    if unsafe {libc::prctl(libc::PR_SET_PDEATHSIG,libc::SIGKILL,0,0,0)}!=0 {return Err(std::io::Error::last_os_error().to_string());}
    if unsafe {libc::getppid()}!=parent {return Err("Photo decoder parent changed".into());}Ok(())
}

pub fn restrict(bytes:usize)->Result<(),String> {
    if std::fs::read_dir("/proc/self/task").map_err(|e|e.to_string())?.count()!=1 {return Err("Photo decoder must start on one thread".into());}
    let pages=std::fs::read_to_string("/proc/self/statm").map_err(|e|e.to_string())?
        .split_whitespace().next().ok_or("Missing decoder memory size")?.parse::<u64>().map_err(|e|e.to_string())?;
    let page=unsafe {libc::sysconf(libc::_SC_PAGESIZE)};if page<=0 {return Err("Missing system page size".into());}
    let address_space=pages.checked_mul(page as u64).and_then(|base|base.checked_add(bytes as u64)).ok_or("Decoder memory limit overflow")?;
    for (resource,maximum) in [(libc::RLIMIT_AS,address_space),(libc::RLIMIT_CPU,60),(libc::RLIMIT_CORE,0),(libc::RLIMIT_NPROC,0)] {
        let limit=libc::rlimit {rlim_cur:maximum,rlim_max:maximum};
        if unsafe {libc::setrlimit(resource,&limit)}!=0 {return Err(std::io::Error::last_os_error().to_string());}
    }
    if unsafe {libc::close_range(3,u32::MAX,0)}!=0 {return Err(std::io::Error::last_os_error().to_string());}
    let calls=[libc::SYS_read,libc::SYS_write,libc::SYS_readv,libc::SYS_writev,libc::SYS_pread64,libc::SYS_lseek,
        libc::SYS_close,libc::SYS_fstat,libc::SYS_brk,libc::SYS_mmap,libc::SYS_munmap,libc::SYS_mremap,
        libc::SYS_mprotect,libc::SYS_madvise,libc::SYS_futex,libc::SYS_clock_gettime,libc::SYS_clock_nanosleep,
        libc::SYS_nanosleep,libc::SYS_sched_yield,libc::SYS_sched_getaffinity,libc::SYS_getrandom,
        libc::SYS_rt_sigaction,libc::SYS_rt_sigprocmask,libc::SYS_rt_sigreturn,libc::SYS_sigaltstack,
        libc::SYS_getpid,libc::SYS_gettid,libc::SYS_exit,libc::SYS_exit_group];
    let filter:BpfProgram=SeccompFilter::new(calls.into_iter().map(|call|(call,Vec::new())).collect(),
        SeccompAction::Errno(libc::EPERM as u32),SeccompAction::Allow,std::env::consts::ARCH.try_into().map_err(|e|format!("{e}"))?)
        .map_err(|e|e.to_string())?.try_into().map_err(|e|format!("{e}"))?;
    seccompiler::apply_filter(&filter).map_err(|e|e.to_string())
}
