use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    binary: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-lowmem-release-package-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let binary = root.join("fixture-agent-lowmem");
        fs::write(&binary, b"#!/bin/sh\nprintf 'agent-lowmem 0.1.0\\n'\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let output = root.join("output");
        Self {
            root,
            binary,
            output,
        }
    }

    fn package(&self, version: &str, binary: &Path, output: &Path) -> Output {
        Command::new("sh")
            .arg(script())
            .arg(version)
            .arg(binary)
            .arg(output)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    repository().join("scripts/package-release.sh")
}

#[test]
fn rejects_invalid_inputs_without_creating_release_identities() {
    let fixture = Fixture::new();

    for version in [
        "v0.1.0", "01.0.0", "0.01.0", "0.0.01", "1.2", "1.2.3.4", "1.2.x",
    ] {
        assert!(
            !fixture
                .package(version, &fixture.binary, &fixture.output)
                .status
                .success()
        );
    }

    let missing = fixture.root.join("missing");
    assert!(
        !fixture
            .package("0.1.0", &missing, &fixture.output)
            .status
            .success()
    );

    let non_executable = fixture.root.join("non-executable");
    fs::write(&non_executable, b"not executable").unwrap();
    fs::set_permissions(&non_executable, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !fixture
            .package("0.1.0", &non_executable, &fixture.output)
            .status
            .success()
    );
    assert!(
        !fixture
            .package("0.1.0", &fixture.root, &fixture.output)
            .status
            .success()
    );
    assert!(
        !fixture
            .package("0.1.0", &fixture.binary, &repository())
            .status
            .success()
    );

    let extra = Command::new("sh")
        .arg(script())
        .args(["0.1.0", "binary", "output", "extra"])
        .output()
        .unwrap();
    assert!(!extra.status.success());

    assert!(
        !repository()
            .join("agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz")
            .exists()
    );
    assert!(!repository().join("SHA256SUMS").exists());
}

#[test]
fn creates_the_exact_archive_modes_and_verified_checksum() {
    let fixture = Fixture::new();
    let result = fixture.package("0.1.0", &fixture.binary, &fixture.output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let archive_name = "agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz";
    let archive = fixture.output.join(archive_name);
    let checksum = fixture.output.join("SHA256SUMS");
    let mut identities = fs::read_dir(&fixture.output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    identities.sort();
    assert_eq!(identities, ["SHA256SUMS", archive_name]);

    let members = Command::new("tar")
        .args(["-tzf"])
        .arg(&archive)
        .output()
        .unwrap();
    assert!(members.status.success());
    assert_eq!(
        String::from_utf8(members.stdout).unwrap(),
        "agent-lowmem\nLICENSE.md\nREADME.md\n"
    );

    let listing = Command::new("tar")
        .args(["-tvzf"])
        .arg(&archive)
        .output()
        .unwrap();
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    let lines = listing.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("-rwxr-xr-x ") && lines[0].ends_with(" agent-lowmem"));
    assert!(lines[1].starts_with("-rw-r--r-- ") && lines[1].ends_with(" LICENSE.md"));
    assert!(lines[2].starts_with("-rw-r--r-- ") && lines[2].ends_with(" README.md"));
    assert!(!listing.contains("._"));

    let manifest = fs::read_to_string(checksum).unwrap();
    assert_eq!(manifest.matches('\n').count(), 1);
    let (digest, filename) = manifest.trim_end_matches('\n').split_once("  ").unwrap();
    assert_eq!(filename, archive_name);
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );

    let verified = Command::new("shasum")
        .args(["-a", "256", "-c", "SHA256SUMS"])
        .current_dir(&fixture.output)
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );

    let second_output = fixture.root.join("second-output");
    let second = fixture.package("0.1.0", &fixture.binary, &second_output);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        fs::read(&archive).unwrap(),
        fs::read(second_output.join(archive_name)).unwrap()
    );
    assert_eq!(
        fs::read(fixture.output.join("SHA256SUMS")).unwrap(),
        fs::read(second_output.join("SHA256SUMS")).unwrap()
    );
}
