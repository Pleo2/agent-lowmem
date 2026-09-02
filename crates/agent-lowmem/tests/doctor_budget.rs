use std::{
    fs,
    path::PathBuf,
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
