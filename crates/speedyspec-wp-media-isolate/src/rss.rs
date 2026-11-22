//! A child's resident memory on macOS, where `RLIMIT_AS` and `RLIMIT_DATA` aren't enforced.

/// Bytes resident for `pid`, if the kernel says.
pub fn resident(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_taskinfo.
    let n = unsafe {
        libc::proc_pidinfo(pid as libc::c_int, libc::PROC_PIDTASKINFO, 0, (&raw mut info).cast(), size)
    };
    (n == size).then_some(info.pti_resident_size)
}
