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
    origin: PathBuf,
    scanner: PathBuf,
    evidence: PathBuf,
    invocation: PathBuf,
}

impl Fixture {
    fn new(scanner_exit: i32) -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-publication-audit-{}-{id}",
            std::process::id()
        ));
        let repository = base.join("repository");
        let origin = base.join("origin.git");
        fs::create_dir_all(repository.join("scripts")).unwrap();
        fs::copy(auditor(), repository.join("scripts/audit-publication.sh")).unwrap();
        fs::write(repository.join("README.md"), "# fixture\n").unwrap();

        git(&repository, &["init", "-b", "main"]);
        git(&repository, &["config", "user.name", "Agent Lowmem Test"]);
        git(
            &repository,
            &["config", "user.email", "test@agentlowmem.dev"],
        );
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "-m", "test: initialize fixture"]);
        git(
            &base,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                origin.to_str().unwrap(),
            ],
        );
        git(
            &repository,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&repository, &["push", "-u", "origin", "main"]);

        let invocation = base.join("scanner-invocation.txt");
        let scanner = base.join("gitleaks");
        write_executable(
            &scanner,
            &format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = stdin ]; then cat >/dev/null; exit 1; fi\nprintf '%s\\n' \"$@\" > '{}'\nexit {scanner_exit}\n",
                invocation.display()
            ),
        );

        Self {
            evidence: base.join("publication-evidence.txt"),
            base,
            repository,
            origin,
            scanner,
            invocation,
        }
    }

    fn run(&self) -> Output {
        Command::new("sh")
            .arg(self.repository.join("scripts/audit-publication.sh"))
            .arg(&self.scanner)
            .arg(&self.evidence)
            .current_dir(&self.repository)
            .output()
            .unwrap()
    }

    fn assert_failure_is_redacted(&self, result: Output) {
        assert!(!result.status.success());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(!stderr.contains(self.base.to_str().unwrap()));
        assert!(!self.evidence.exists());
    }

    fn commit_and_push(&self, path: &str, contents: &str) {
        fs::write(self.repository.join(path), contents).unwrap();
        git(&self.repository, &["add", path]);
        git(&self.repository, &["commit", "-m", "test: add fixture"]);
        git(&self.repository, &["push", "origin", "main"]);
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

fn auditor() -> PathBuf {
    project_root().join("scripts/audit-publication.sh")
}

fn git(directory: &Path, arguments: &[&str]) -> Output {
    let result = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn rejects_invalid_arguments_before_starting_the_scanner() {
    assert!(
        !Command::new("sh")
            .arg(auditor())
            .output()
            .unwrap()
            .status
            .success()
    );

    let fixture = Fixture::new(0);
    let missing = fixture.base.join("missing-gitleaks");
    let result = Command::new("sh")
        .arg(fixture.repository.join("scripts/audit-publication.sh"))
        .arg(missing)
        .arg(&fixture.evidence)
        .current_dir(&fixture.repository)
        .output()
        .unwrap();
    fixture.assert_failure_is_redacted(result);

    let internal_evidence = fixture.repository.join("evidence.txt");
    let result = Command::new("sh")
        .arg(fixture.repository.join("scripts/audit-publication.sh"))
        .arg(&fixture.scanner)
        .arg(internal_evidence)
        .current_dir(&fixture.repository)
        .output()
        .unwrap();
    fixture.assert_failure_is_redacted(result);
    assert!(!fixture.invocation.exists());
}

#[test]
fn rejects_dirty_or_divergent_repository_before_scanning() {
    let dirty = Fixture::new(0);
    fs::write(dirty.repository.join("untracked.txt"), "dirty\n").unwrap();
    let result = dirty.run();
    dirty.assert_failure_is_redacted(result);
    assert!(!dirty.invocation.exists());

    let divergent = Fixture::new(0);
    fs::write(divergent.repository.join("local.txt"), "local\n").unwrap();
    git(&divergent.repository, &["add", "local.txt"]);
    git(&divergent.repository, &["commit", "-m", "test: diverge"]);
    let result = divergent.run();
    divergent.assert_failure_is_redacted(result);
    assert!(!divergent.invocation.exists());
}

#[test]
fn rejects_submodules_lfs_pointers_and_suspicious_paths_in_any_ref() {
    let submodule = Fixture::new(0);
    submodule.commit_and_push(".gitmodules", "[submodule \"vendor\"]\n\tpath = vendor\n");
    let result = submodule.run();
    submodule.assert_failure_is_redacted(result);

    let lfs = Fixture::new(0);
    lfs.commit_and_push(
        "large.bin",
        "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 1\n",
    );
    let result = lfs.run();
    lfs.assert_failure_is_redacted(result);

    let suspicious = Fixture::new(0);
    suspicious.commit_and_push("credentials.json", "{}\n");
    let result = suspicious.run();
    suspicious.assert_failure_is_redacted(result);
}

#[test]
fn rejects_shallow_repositories_and_corrupt_objects() {
    let source = Fixture::new(0);
    let shallow = source.base.join("shallow");
    let clone_url = format!("file://{}", source.origin.display());
    let result = Command::new("git")
        .args(["clone", "--depth", "1", &clone_url])
        .arg(&shallow)
        .output()
        .unwrap();
    assert!(result.status.success());
    let result = Command::new("sh")
        .arg(shallow.join("scripts/audit-publication.sh"))
        .arg(&source.scanner)
        .arg(source.base.join("shallow-evidence.txt"))
        .current_dir(&shallow)
        .output()
        .unwrap();
    assert!(!result.status.success());

    let corrupt = Fixture::new(0);
    let head = git(&corrupt.repository, &["rev-parse", "HEAD"]);
    let head = String::from_utf8(head.stdout).unwrap();
    let head = head.trim();
    let object = corrupt
        .repository
        .join(".git/objects")
        .join(&head[..2])
        .join(&head[2..]);
    fs::set_permissions(&object, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(object, "corrupt\n").unwrap();
    let result = corrupt.run();
    corrupt.assert_failure_is_redacted(result);
}

#[test]
fn invokes_gitleaks_for_all_refs_and_writes_only_bounded_evidence() {
    let fixture = Fixture::new(0);
    let result = fixture.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let invocation = fs::read_to_string(&fixture.invocation).unwrap();
    assert_eq!(
        invocation,
        "detect\n--source\n.\n--redact\n--no-banner\n--exit-code\n1\n--log-opts=--all\n"
    );
    let evidence = fs::read_to_string(&fixture.evidence).unwrap();
    assert!(evidence.contains("schema=agent-lowmem-publication-audit-v1\n"));
    assert!(evidence.contains("scan=pass\nstatus=pass\n"));
    assert!(!evidence.contains(fixture.base.to_str().unwrap()));
    assert!(!evidence.contains("candidate"));
    assert_eq!(
        fs::metadata(&fixture.evidence)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn scanner_findings_fail_without_echoing_scanner_output() {
    let fixture = Fixture::new(1);
    let result = fixture.run();
    fixture.assert_failure_is_redacted(result);
    assert!(fixture.invocation.exists());
}

#[test]
fn scanner_must_detect_the_built_in_canary_before_repository_scanning() {
    let fixture = Fixture::new(0);
    write_executable(&fixture.scanner, "#!/bin/sh\ncat >/dev/null\nexit 0\n");
    let result = fixture.run();
    fixture.assert_failure_is_redacted(result);
    assert!(!fixture.invocation.exists());
}
