use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct EmptyDirectory {
    root: PathBuf,
}

impl EmptyDirectory {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("agent-lowmem-doctor-budget-{timestamp}"));
        fs::create_dir_all(&root).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
        }
    }
}

impl Drop for EmptyDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
#[ignore = "release-only warm-cache resource gate"]
fn doctor_warm_cache_median_stays_within_phase_one_budget() {
    let fixture = EmptyDirectory::new();
    let binary = env!("CARGO_BIN_EXE_agent-lowmem");

    let warmup = Command::new(binary)
        .arg("doctor")
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(warmup.status.success());

    let mut elapsed_milliseconds = Vec::with_capacity(20);
    for _ in 0..20 {
        let started = Instant::now();
        let output = Command::new(binary)
            .arg("doctor")
            .current_dir(&fixture.root)
            .output()
            .unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        assert!(output.status.success());
        elapsed_milliseconds.push(elapsed);
    }

    elapsed_milliseconds.sort_by(f64::total_cmp);
    let median = elapsed_milliseconds[9];
    let p95 = elapsed_milliseconds[18];
    eprintln!("doctor-budget median_ms={median:.3} p95_ms={p95:.3}");
    assert!(
        median <= 100.0,
        "doctor median {median:.3} ms exceeded 100 ms"
    );
}

#[test]
#[ignore = "release-only repository warm-cache resource gate"]
fn repository_doctor_stays_within_phase_two_budget() {
    let fixture = EmptyDirectory::new();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
        &fixture.root,
    );
    fs::create_dir(fixture.root.join(".git")).unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-lowmem");

    let warmup = Command::new(binary)
        .arg("doctor")
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(warmup.status.success());

    let mut elapsed_milliseconds = Vec::with_capacity(20);
    for _ in 0..20 {
        let started = Instant::now();
        let output = Command::new(binary)
            .arg("doctor")
            .current_dir(&fixture.root)
            .output()
            .unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        assert!(output.status.success());
        elapsed_milliseconds.push(elapsed);
    }

    elapsed_milliseconds.sort_by(f64::total_cmp);
    let median = elapsed_milliseconds[9];
    let p95 = elapsed_milliseconds[18];
    eprintln!("repository-doctor-budget median_ms={median:.3} p95_ms={p95:.3}");
    assert!(
        median <= 300.0,
        "repository doctor median {median:.3} ms exceeded 300 ms"
    );
    assert!(
        p95 <= 500.0,
        "repository doctor p95 {p95:.3} ms exceeded 500 ms"
    );
}

fn copy_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
