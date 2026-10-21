//! The few C functions we call, declared by hand. Linux only.
//!
//! Each declaration must match the C prototype exactly (see the man page named beside it); the
//! compiler cannot check that, which is why every call is `unsafe`.

use std::ffi::{c_char, c_int, c_long};
use std::io;

pub const SIGINT: c_int = 2; // asm-generic/signal.h
pub const SIGKILL: c_int = 9;

unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int; // kill(2)
    fn signal(signum: c_int, handler: usize) -> usize; // signal(2)
    fn prctl(option: c_int, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> c_int; // prctl(2)
    fn syscall(number: c_long, ...) -> c_long; // syscall(2)
    fn open(path: *const c_char, flags: c_int, ...) -> c_int; // open(2)
    fn close(fd: c_int) -> c_int; // close(2)
    fn _exit(status: c_int) -> !; // _exit(2)
}

fn check(r: c_long) -> io::Result<c_long> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

/// Sends `sig` to every process in group `pgid`. `ESRCH` (the group is gone) is not an error.
pub fn kill_group(pgid: u32, sig: c_int) -> io::Result<()> {
    // SAFETY: kill takes two integers. A negative pid means "the process group".
    match check(unsafe { kill(-(pgid as c_int), sig) } as c_long) {
        Err(e) if e.raw_os_error() == Some(3) => Ok(()), // ESRCH
        r => r.map(drop),
    }
}

/// Installs `handler` for `signum`. glibc's signal() keeps the handler installed and restarts
/// interrupted reads (BSD semantics, signal(2) "Portability").
pub fn on_signal(signum: c_int, handler: extern "C" fn(c_int)) {
    // SAFETY: the handler is an `extern "C" fn(c_int)`, the type signal() expects.
    unsafe { signal(signum, handler as usize) };
}

/// Ends the process at once, without unwinding or flushing. Safe inside a signal handler.
pub fn exit_now(status: c_int) -> ! {
    // SAFETY: _exit is async-signal-safe and never returns.
    unsafe { _exit(status) }
}

pub mod landlock {
    //! Landlock: a process limits what it can do to the filesystem, for itself and its children.
    //! Numbers and structs from linux/landlock.h and asm/unistd_64.h.
    use super::*;

    const SYS_CREATE_RULESET: c_long = 444;
    const SYS_ADD_RULE: c_long = 445;
    const SYS_RESTRICT_SELF: c_long = 446;
    const CREATE_RULESET_VERSION: u32 = 1;
    const RULE_PATH_BENEATH: c_int = 1;
    const PR_SET_NO_NEW_PRIVS: c_int = 38;
    const O_PATH: c_int = 0o10000000;
    const O_CLOEXEC: c_int = 0o2000000;

    pub const WRITE_FILE: u64 = 1 << 1;
    pub const REMOVE_DIR: u64 = 1 << 4;
    pub const REMOVE_FILE: u64 = 1 << 5;
    pub const MAKE_CHAR: u64 = 1 << 6;
    pub const MAKE_DIR: u64 = 1 << 7;
    pub const MAKE_REG: u64 = 1 << 8;
    pub const MAKE_SOCK: u64 = 1 << 9;
    pub const MAKE_FIFO: u64 = 1 << 10;
    pub const MAKE_BLOCK: u64 = 1 << 11;
    pub const MAKE_SYM: u64 = 1 << 12;
    pub const REFER: u64 = 1 << 13; // ABI 2
    pub const TRUNCATE: u64 = 1 << 14; // ABI 3

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
        handled_access_net: u64,
    }

    #[repr(C, packed)]
    struct PathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    /// The Landlock ABI version this kernel supports, or an error if Landlock is off.
    pub fn abi() -> io::Result<i32> {
        // SAFETY: with a null attr, size 0 and the VERSION flag, the kernel reads no memory.
        check(unsafe {
            syscall(
                SYS_CREATE_RULESET,
                std::ptr::null::<u8>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        })
        .map(|v| v as i32)
    }

    /// Forbids `handled` everywhere except below each `(path, allowed)`. Runs in the child between
    /// fork and exec: only system calls, no allocation. `path` must be NUL-terminated.
    pub fn restrict(handled: u64, rules: &[(&[u8], u64)]) -> io::Result<()> {
        // SAFETY: every pointer passed below points at a live, correctly laid out value, and
        // every path is NUL-terminated (checked by the caller that built them).
        unsafe {
            check(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) as c_long)?;
            let attr = RulesetAttr {
                handled_access_fs: handled,
                handled_access_net: 0,
            };
            let ruleset = check(syscall(SYS_CREATE_RULESET, &attr, size_of::<RulesetAttr>(), 0u32))? as c_int;
            for (path, allowed) in rules {
                let fd = check(open(path.as_ptr().cast(), O_PATH | O_CLOEXEC) as c_long)? as c_int;
                let rule = PathBeneathAttr {
                    allowed_access: *allowed,
                    parent_fd: fd,
                };
                let r = syscall(SYS_ADD_RULE, ruleset, RULE_PATH_BENEATH, &rule, 0u32);
                close(fd);
                check(r)?;
            }
            check(syscall(SYS_RESTRICT_SELF, ruleset, 0u32))?;
            close(ruleset);
        }
        Ok(())
    }
}
