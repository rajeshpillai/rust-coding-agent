//! The few C functions we call, declared by hand. Linux only.
//!
//! Each declaration must match the C prototype exactly (see the man page named beside it); the
//! compiler cannot check that, which is why every call is `unsafe`.

use std::ffi::{c_int, c_long};
use std::io;

pub const SIGINT: c_int = 2; // asm-generic/signal.h
pub const SIGKILL: c_int = 9;

unsafe extern "C" {
    fn kill(pid: c_int, sig: c_int) -> c_int; // kill(2)
    fn signal(signum: c_int, handler: usize) -> usize; // signal(2)
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
