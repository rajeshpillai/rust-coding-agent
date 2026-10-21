use kaam::exec::{self, Ended, Limits};
use std::path::Path;
use std::time::Duration;

fn limits(secs: u64, max: usize) -> Limits {
    Limits {
        timeout: Duration::from_secs(secs),
        max_bytes: max,
    }
}

#[test]
fn merges_stdout_and_stderr_in_order() {
    let o = exec::run(
        "echo one; echo two >&2; echo three",
        Path::new("."),
        &limits(10, 1024),
    )
    .unwrap();
    assert_eq!(o.text, "one\ntwo\nthree\n");
    assert_eq!(o.ended, Ended::Exited(0));
}

#[test]
fn caps_the_output_but_counts_all_of_it() {
    let o = exec::run(
        "head -c 100000 /dev/zero | tr '\\0' x",
        Path::new("."),
        &limits(10, 1000),
    )
    .unwrap();
    assert_eq!((o.text.len(), o.total), (1000, 100_000));
    assert!(
        o.for_model()
            .contains("[output cut: showed 1000 of 100000 bytes]")
    );
}
