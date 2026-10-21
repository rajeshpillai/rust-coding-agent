//! Ctrl+C. The first press cancels the running step; a second press quits.

use crate::sys;
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: c_int) {
    // Only async-signal-safe work here: an atomic swap, and _exit.
    if INTERRUPTED.swap(true, Ordering::SeqCst) {
        sys::exit_now(130);
    }
}

pub fn install() {
    sys::on_signal(sys::SIGINT, on_sigint);
}

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

pub fn clear() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}
