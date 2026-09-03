use agent_lowmem::{
    result::{ExitResult, Origin, Reason},
    result_file::{
        RunResultRecord, validate_result_destination, write_result_atomic,
        write_validated_result_atomic,
    },
};
use rustix::fs::Mode;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn temporary_identity_is_not_exhausted_by_the_legacy_predictable_namespace() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "unpredictable_temporary_child", "--nocapture"])
        .env("AGENT_LOWMEM_ATOMIC_TEMP_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unpredictable_temporary_child() {
    if std::env::var_os("AGENT_LOWMEM_ATOMIC_TEMP_CHILD").is_none() {
        return;
    }
    let fixture = Fixture::new();
    for serial in 0..16 {
        fs::write(
            fixture.0.join(format!(
                ".agent-lowmem-result-{}-{serial}.tmp",
                std::process::id()
            )),
            b"occupied",
        )
        .unwrap();
    }

    write_result_atomic(&fixture.0, "result.json", &record()).unwrap();

    assert!(fixture.0.join("result.json").is_file());
    assert_eq!(temporary_entries(&fixture.0).len(), 16);
}

#[test]
fn exact_mode_is_independent_of_permissive_and_restrictive_umasks() {
    for (name, value) in [("permissive", "000"), ("restrictive", "077")] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "umask_child", "--nocapture"])
            .env("AGENT_LOWMEM_ATOMIC_UMASK", value)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn umask_child() {
    let Some(value) = std::env::var_os("AGENT_LOWMEM_ATOMIC_UMASK") else {
        return;
    };
    let fixture = Fixture::new();
    let requested = match value.to_str().unwrap() {
        "000" => Mode::empty(),
        "077" => Mode::RWXG | Mode::RWXO,
        _ => panic!("unexpected child umask"),
    };
    let previous = rustix::process::umask(requested);
    write_result_atomic(&fixture.0, "result.json", &record()).unwrap();
    rustix::process::umask(previous);
    assert_eq!(
        fs::metadata(fixture.0.join("result.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn validated_destination_remains_bound_to_its_original_parent() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    fs::create_dir(fixture.0.join("artifacts")).unwrap();
    let destination = validate_result_destination(&fixture.0, "artifacts/result.json").unwrap();
    fs::rename(fixture.0.join("artifacts"), fixture.0.join("moved")).unwrap();
    symlink(&outside.0, fixture.0.join("artifacts")).unwrap();

    write_validated_result_atomic(&destination, &record()).unwrap();

    assert!(fixture.0.join("moved/result.json").is_file());
    assert!(!outside.0.join("result.json").exists());
    assert!(temporary_entries(&fixture.0.join("moved")).is_empty());
}

#[test]
fn replaces_a_large_existing_regular_result_without_a_size_policy_change() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("result.json"), vec![b'x'; 1_048_577]).unwrap();

    write_result_atomic(&fixture.0, "result.json", &record()).unwrap();

    assert!(
        fs::read_to_string(fixture.0.join("result.json"))
            .unwrap()
            .contains("\"schemaVersion\": 1")
    );
}

fn temporary_entries(path: &std::path::Path) -> Vec<String> {
    fs::read_dir(path)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".agent-lowmem-") && name.ends_with(".tmp"))
        .collect()
}

fn record() -> RunResultRecord {
    RunResultRecord::at_unix_seconds(
        ExitResult::new(Origin::Child, 0, Reason::Completed),
        true,
        None,
        951_782_400,
    )
    .unwrap()
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-lowmem-atomic-managed-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
