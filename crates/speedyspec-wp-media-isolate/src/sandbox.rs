//! Locks the probe child down before it reads anything: on Linux, a seccomp filter that kills the process on any
//! system call outside what reading a descriptor and writing stdout need. The parent sees `SIGSYS`.

/// Restricts the current process; exits with status 4 if the filter can't be installed, so a child never reads
/// unsandboxed on Linux.
#[cfg(target_os = "linux")]
pub fn enter() {
    use std::collections::BTreeMap;

    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule,
        TargetArch,
    };

    let calls: Vec<i64> = vec![
        libc::SYS_read,
        libc::SYS_pread64,
        libc::SYS_write,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_fcntl,
        libc::SYS_fstat,
        libc::SYS_newfstatat,
        libc::SYS_statx,
        libc::SYS_lseek,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mremap,
        libc::SYS_brk,
        libc::SYS_madvise,
        libc::SYS_mprotect,
        libc::SYS_futex,
        libc::SYS_clock_gettime,
        libc::SYS_clock_nanosleep,
        libc::SYS_sched_yield,
        libc::SYS_rt_sigreturn,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigaction,
        libc::SYS_sigaltstack,
        libc::SYS_getrandom,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ];
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = calls.into_iter().map(|n| (n, vec![])).collect();
    // abort() (a failed allocation under RLIMIT_AS ends there) signals the process itself; signalling any other
    // process stays forbidden. The child is single-threaded, so its one thread id is its process id.
    // SAFETY: getpid has no preconditions.
    let pid = unsafe { libc::getpid() } as u64;
    for call in [libc::SYS_tkill, libc::SYS_tgkill] {
        let own = SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, pid)
            .and_then(|condition| SeccompRule::new(vec![condition]));
        match own {
            Ok(rule) => {
                rules.insert(call, vec![rule]);
            }
            Err(err) => {
                eprintln!("speedyspec media-probe: seccomp: {err}");
                std::process::exit(4);
            }
        }
    }
    #[cfg(target_arch = "x86_64")]
    let arch = TargetArch::x86_64;
    #[cfg(target_arch = "aarch64")]
    let arch = TargetArch::aarch64;
    let installed = SeccompFilter::new(rules, SeccompAction::KillProcess, SeccompAction::Allow, arch)
        .map_err(|e| e.to_string())
        .and_then(|filter| BpfProgram::try_from(filter).map_err(|e| e.to_string()))
        .and_then(|program| seccompiler::apply_filter(&program).map_err(|e| e.to_string()));
    if let Err(err) = installed {
        eprintln!("speedyspec media-probe: seccomp: {err}");
        std::process::exit(4);
    }
}

/// Elsewhere the child relies on its resource limits and the parent's watch.
#[cfg(not(target_os = "linux"))]
pub fn enter() {}
