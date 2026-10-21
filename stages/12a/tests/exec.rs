use kaam::exec::{self, Ended, Limits};
use kaam::sandbox::Sandbox;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
        None,
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
        None,
    )
    .unwrap();
    assert_eq!((o.text.len(), o.total), (1000, 100_000));
    assert!(
        o.for_model()
            .contains("[output cut: showed 1000 of 100000 bytes]")
    );
}

#[test]
fn timeout_kills_the_whole_group() {
    let marker = std::env::temp_dir().join(format!("kaam-orphan-{}", std::process::id()));
    let cmd = format!("(sleep 2; touch {}) & sleep 30", marker.display());
    let t = Instant::now();
    let o = exec::run(&cmd, Path::new("."), &limits(1, 1024), None).unwrap();
    assert_eq!(o.ended, Ended::TimedOut);
    assert!(t.elapsed() < Duration::from_secs(5));
    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists(), "a background child outlived the timeout");
}

#[test]
fn sandbox_blocks_writes_outside_the_project() {
    let project = std::env::temp_dir().join(format!("kaam-proj-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let outside = home.join(format!(".kaam-sandbox-test-{}", std::process::id()));
    let sb = Sandbox::new(&[project.clone(), "/dev/null".into()]).unwrap();
    let cmd = format!("echo in > inside.txt && echo out > {}", outside.display());
    let o = exec::run(&cmd, &project, &limits(10, 4096), Some(&sb)).unwrap();
    assert!(project.join("inside.txt").exists());
    assert!(!outside.exists());
    assert!(o.text.contains("Permission denied"), "{}", o.text);
    assert_eq!(o.ended, Ended::Exited(2));
}
