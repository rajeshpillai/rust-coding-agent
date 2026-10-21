//! Commands run with writes allowed only below a few folders. Reads are not limited.

use crate::sys::landlock::{self, *};
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const DIR_RIGHTS: u64 = WRITE_FILE
    | REMOVE_DIR
    | REMOVE_FILE
    | MAKE_CHAR
    | MAKE_DIR
    | MAKE_REG
    | MAKE_SOCK
    | MAKE_FIFO
    | MAKE_BLOCK
    | MAKE_SYM;
const FILE_RIGHTS: u64 = WRITE_FILE;

#[derive(Clone, Debug)]
pub struct Sandbox {
    handled: u64,
    rules: Vec<(CString, u64)>,
    pub abi: i32,
    pub writable: Vec<PathBuf>,
}

impl Sandbox {
    /// Fails if this kernel has no Landlock. Paths that do not exist are skipped.
    pub fn new(writable: &[PathBuf]) -> io::Result<Sandbox> {
        let abi = landlock::abi()?;
        let mut extra = 0;
        if abi >= 2 {
            extra |= REFER;
        }
        if abi >= 3 {
            extra |= TRUNCATE;
        }
        let mut rules = vec![];
        let mut kept = vec![];
        for p in writable {
            let Ok(meta) = p.metadata() else { continue };
            let allowed = if meta.is_dir() {
                DIR_RIGHTS | extra
            } else {
                FILE_RIGHTS | (extra & TRUNCATE)
            };
            rules.push((cstring(p)?, allowed));
            kept.push(p.clone());
        }
        Ok(Sandbox {
            handled: DIR_RIGHTS | extra,
            rules,
            abi,
            writable: kept,
        })
    }

    /// Call in the child, between fork and exec.
    pub fn apply(&self) -> io::Result<()> {
        // At most 8 rules, on the stack, so nothing is allocated after fork.
        let mut rules: [(&[u8], u64); 8] = [(&[], 0); 8];
        let n = self.rules.len().min(8);
        for (slot, (path, allowed)) in rules.iter_mut().zip(&self.rules) {
            *slot = (path.as_bytes_with_nul(), *allowed);
        }
        landlock::restrict(self.handled, &rules[..n])
    }
}

fn cstring(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(io::Error::other)
}
