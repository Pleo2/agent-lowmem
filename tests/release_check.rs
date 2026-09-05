use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    repository: PathBuf,
    output: PathBuf,
    evidence: PathBuf,
    audit: PathBuf,
    cargo_marker: PathBuf,
    audit_marker: PathBuf,
    path: String,
}

impl Fixture {
    fn new(fake_architecture: Option<&str>) -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-release-check-{}-{id}",
            std::process::id()
        ));
        let repository = base.join("repository");
        let origin = base.join("origin.git");
        let bin = base.join("bin");
        fs::create_dir_all(repository.join("scripts")).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::copy(checker(), repository.join("scripts/check-release.sh")).unwrap();
        fs::copy(
            project_root().join("scripts/package-release.sh"),
            repository.join("scripts/package-release.sh"),
        )
        .unwrap();
        fs::write(
            repository.join("Cargo.toml"),
            "[package]\nname = \"agent-lowmem\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(repository.join("Cargo.lock"), "version = 4\n").unwrap();
        fs::write(repository.join("README.md"), "# Agent Lowmem\n").unwrap();
        fs::write(repository.join("LICENSE.md"), "fixture license\n").unwrap();

        git(&repository, ["init", "-b", "main"]);
        git(&repository, ["config", "user.name", "Agent Lowmem Test"]);
        git(
            &repository,
            ["config", "user.email", "test@agentlowmem.dev"],
        );
        git(&repository, ["add", "."]);
        git(&repository, ["commit", "-m", "test: initialize fixture"]);
        git(
            &base,
            [
                "init",
                "--bare",
                "--initial-branch=main",
                origin.to_str().unwrap(),
            ],
        );
        git(
            &repository,
            ["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repository, ["push", "-u", "origin", "main"]);

        let cargo_marker = base.join("cargo-started");
        write_executable(
            &bin.join("cargo"),
            &format!("#!/bin/sh\n: > '{}'\nexit 99\n", cargo_marker.display()),
        );
        if let Some(architecture) = fake_architecture {
            write_executable(
                &bin.join("uname"),
                &format!("#!/bin/sh\nprintf '%s\\n' '{architecture}'\n"),
            );
        }
        let audit_marker = base.join("audit-started");
        let audit = base.join("cargo-audit");
        write_executable(
            &audit,
            &format!("#!/bin/sh\n: > '{}'\nexit 99\n", audit_marker.display()),
        );
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

        Self {
            output: repository.join("dist"),
            evidence: base.join("release-evidence.txt"),
            base,
            repository,
            audit,
            cargo_marker,
            audit_marker,
            path,
        }
    }

    fn run(&self, output: &Path, evidence: &Path, audit: &Path) -> Output {
        Command::new("sh")
            .arg(self.repository.join("scripts/check-release.sh"))
            .arg("0.1.0")
            .arg(audit)
            .arg(output)
            .arg(evidence)
            .env("PATH", &self.path)
            .current_dir(&self.repository)
            .output()
            .unwrap()
    }

    fn assert_preflight_failure(&self, result: Output) {
        assert!(!result.status.success());
        assert!(!self.cargo_marker.exists());
        assert!(!self.audit_marker.exists());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(!stderr.contains(self.base.to_str().unwrap()));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn checker() -> PathBuf {
    project_root().join("scripts/check-release.sh")
}

fn git<const N: usize>(directory: &Path, arguments: [&str; N]) {
    let result = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn rejects_unsafe_arguments_before_starting_release_tools() {
    let no_arguments = Command::new("sh").arg(checker()).output().unwrap();
    assert!(!no_arguments.status.success());

    let fixture = Fixture::new(None);
    fixture.assert_preflight_failure(fixture.run(
        &fixture.output,
        &fixture.evidence,
        &fixture.base.join("missing-audit"),
    ));
    fixture.assert_preflight_failure(fixture.run(
        &fixture.repository,
        &fixture.evidence,
        &fixture.audit,
    ));
    fixture.assert_preflight_failure(fixture.run(
        &fixture.output,
        &fixture.repository.join("evidence.txt"),
        &fixture.audit,
    ));
}

#[test]
fn rejects_wrong_architecture_dirty_state_and_main_divergence_before_cargo() {
    let wrong_arch = Fixture::new(Some("x86_64"));
    wrong_arch.assert_preflight_failure(wrong_arch.run(
        &wrong_arch.output,
        &wrong_arch.evidence,
        &wrong_arch.audit,
    ));

    let dirty = Fixture::new(None);
    fs::write(dirty.repository.join("untracked.txt"), "dirty\n").unwrap();
    dirty.assert_preflight_failure(dirty.run(&dirty.output, &dirty.evidence, &dirty.audit));

    let divergent = Fixture::new(None);
    fs::write(divergent.repository.join("local.txt"), "local\n").unwrap();
    git(&divergent.repository, ["add", "local.txt"]);
    git(&divergent.repository, ["commit", "-m", "test: diverge"]);
    divergent.assert_preflight_failure(divergent.run(
        &divergent.output,
        &divergent.evidence,
        &divergent.audit,
    ));
}
