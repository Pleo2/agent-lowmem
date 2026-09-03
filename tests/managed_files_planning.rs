use agent_lowmem::{
    cli::InitRequest,
    host::{HostReadError, HostSource},
    managed_files::{ManagedAction, ManagedIdentity, plan_init},
    result::Reason,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn generates_only_runnable_canonical_root_operations_and_reports_manual_candidates() {
    let fixture = RepositoryFixture::from_source("npm-single");
    fixture.write(
        "package.json",
        r#"{"name":"root","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","test:unit":"vitest run","build":"next build --experimental-build-mode=compile"}}"#,
    );
    fixture.write(
        "node_modules/next/package.json",
        r#"{"name":"next","version":"16.3.4"}"#,
    );

    let plan = plan_init(&SupportedHost::reference(), &fixture.root, &request()).unwrap();
    let report = plan.report();

    assert_eq!(
        report.manifest_state,
        agent_lowmem::managed_files::ManifestState::Absent
    );

    assert_eq!(
        report
            .operations
            .iter()
            .map(|operation| (
                operation.workspace_key.as_deref(),
                operation.operation_key.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![(None, "test")]
    );
    assert_eq!(report.manual_candidates.len(), 1);
    assert_eq!(report.manual_candidates[0].operation_prefix, "test");
    assert_eq!(report.manual_candidates[0].script_name, "test:unit");
    assert!(report.issues.iter().any(|issue| {
        issue.operation_key.as_deref() == Some("build") && issue.reason == Reason::ArgumentDenied
    }));
    assert!(
        plan.evidence_files()
            .iter()
            .any(|path| *path == "node_modules/vitest/package.json")
    );
    assert!(
        !plan
            .effective_configuration()
            .operations
            .contains_key("build")
    );
}

#[test]
fn generates_all_four_canonical_operations_with_their_fixed_timeouts() {
    let fixture = RepositoryFixture::from_source("npm-single");
    fixture.write(
        "package.json",
        r#"{"name":"root","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","typecheck":"tsc --noEmit","lint":"eslint .","build":"next build"}}"#,
    );
    for (package, version) in [
        ("typescript", "7.0.2"),
        ("eslint", "10.9.1"),
        ("next", "16.3.4"),
    ] {
        fixture.write(
            &format!("node_modules/{package}/package.json"),
            &format!(r#"{{"name":"{package}","version":"{version}"}}"#),
        );
    }

    let plan = plan_init(&SupportedHost::reference(), &fixture.root, &request()).unwrap();
    let config = plan.effective_configuration();

    assert_eq!(config.operations["test"].timeout_seconds, 900);
    assert_eq!(config.operations["typecheck"].timeout_seconds, 900);
    assert_eq!(config.operations["lint"].timeout_seconds, 900);
    assert_eq!(config.operations["build"].timeout_seconds, 1_800);
}

#[test]
fn rejects_init_on_an_unsupported_host_before_repository_planning() {
    let fixture = RepositoryFixture::from_source("npm-single");
    let host = SupportedHost {
        operating_system: "linux",
        ..SupportedHost::reference()
    };

    let failure = plan_init(&host, &fixture.root, &request()).unwrap_err();

    assert_eq!(failure.reason(), Reason::HostUnsupported);
    assert!(failure.report().operations.is_empty());
}

#[test]
fn preserves_a_valid_external_configuration_and_builds_policy_from_its_operations() {
    let fixture = RepositoryFixture::from_source("npm-single");
    fixture.write(
        ".agent-lowmem.json",
        r#"{"version":1,"packageManager":"npm","operations":{"checks":{"script":"test","timeoutSeconds":300}}}"#,
    );

    let plan = plan_init(&SupportedHost::reference(), &fixture.root, &request()).unwrap();
    let report = plan.report();

    assert_eq!(
        file_action(report, ManagedIdentity::Configuration),
        ManagedAction::Preserve
    );
    assert_eq!(report.operations[0].operation_key, "checks");
    assert!(
        plan.effective_configuration()
            .operations
            .contains_key("checks")
    );
    assert!(
        !plan
            .effective_configuration()
            .operations
            .contains_key("test")
    );
}

#[test]
fn plans_agents_creation_append_and_idempotent_replacement_without_losing_exterior_bytes() {
    let absent = RepositoryFixture::from_source("npm-single");
    let absent_plan = plan_init(&SupportedHost::reference(), &absent.root, &request()).unwrap();
    assert_eq!(
        file_action(absent_plan.report(), ManagedIdentity::AgentsPolicy),
        ManagedAction::Create
    );

    let existing = RepositoryFixture::from_source("npm-single");
    existing.write("AGENTS.md", "# Existing policy\n\nKeep this text.\n");
    let append_plan = plan_init(&SupportedHost::reference(), &existing.root, &request()).unwrap();
    assert_eq!(
        file_action(append_plan.report(), ManagedIdentity::AgentsPolicy),
        ManagedAction::Replace
    );
    let target = append_plan.agents_target().unwrap();
    assert!(target.starts_with(b"# Existing policy\n\nKeep this text.\n"));
    assert!(String::from_utf8_lossy(target).contains("agent-lowmem run test"));

    fs::write(existing.root.join("AGENTS.md"), target).unwrap();
    let exact_plan = plan_init(&SupportedHost::reference(), &existing.root, &request()).unwrap();
    assert_eq!(
        file_action(exact_plan.report(), ManagedIdentity::AgentsPolicy),
        ManagedAction::Unchanged
    );
}

#[test]
fn derives_scoped_workspace_keys_and_contains_invalid_or_colliding_workspaces() {
    let fixture = RepositoryFixture::from_source("npm-workspace");
    fs::remove_file(fixture.root.join(".agent-lowmem.json")).unwrap();
    fixture.write(
        "package.json",
        r#"{"name":"workspace-root","private":true,"packageManager":"npm@12.0.2","workspaces":["apps/*"],"scripts":{"test":"vitest run"}}"#,
    );
    fixture.write(
        "node_modules/vitest/package.json",
        r#"{"name":"vitest","version":"4.1.11"}"#,
    );
    fixture.write(
        "apps/invalid/package.json",
        r#"{"name":"@acme/bad_key","private":true,"scripts":{"test":"jest"}}"#,
    );
    fixture.write(
        "apps/invalid/node_modules/jest/package.json",
        r#"{"name":"jest","version":"30.5.1"}"#,
    );

    let plan = plan_init(&SupportedHost::reference(), &fixture.root, &request()).unwrap();
    assert!(
        plan.effective_configuration()
            .operations
            .contains_key("test")
    );
    assert!(
        plan.effective_configuration()
            .workspaces
            .contains_key("web")
    );
    assert!(plan.report().issues.iter().any(|issue| {
        issue.reason == Reason::WorkspaceCardinality
            && issue.workspace_path.as_deref() == Some("apps/invalid")
            && issue.package_name.as_deref() == Some("@acme/bad_key")
    }));

    fixture.write(
        "apps/collision/package.json",
        r#"{"name":"@other/web","private":true,"scripts":{"test":"jest"}}"#,
    );
    fixture.write(
        "apps/collision/node_modules/jest/package.json",
        r#"{"name":"jest","version":"30.5.1"}"#,
    );
    let collision = plan_init(&SupportedHost::reference(), &fixture.root, &request()).unwrap();
    assert!(
        collision
            .effective_configuration()
            .operations
            .contains_key("test")
    );
    assert!(
        !collision
            .effective_configuration()
            .workspaces
            .contains_key("web")
    );
    assert_eq!(
        collision
            .report()
            .issues
            .iter()
            .filter(|issue| issue.reason == Reason::WorkspaceCardinality)
            .count(),
        3
    );
}

#[test]
fn rejects_structural_inputs_and_adopts_exact_generated_configuration() {
    let no_operation = RepositoryFixture::from_source("npm-single");
    no_operation.write(
        "package.json",
        r#"{"name":"root","private":true,"packageManager":"npm@12.0.2","scripts":{}}"#,
    );
    assert_eq!(
        plan_init(&SupportedHost::reference(), &no_operation.root, &request())
            .unwrap_err()
            .reason(),
        Reason::OperationUnsupported
    );

    let lock_mismatch = RepositoryFixture::from_source("npm-single");
    fs::remove_file(lock_mismatch.root.join("package-lock.json")).unwrap();
    assert_eq!(
        plan_init(&SupportedHost::reference(), &lock_mismatch.root, &request())
            .unwrap_err()
            .reason(),
        Reason::PackageManagerUnsupported
    );

    let exact = RepositoryFixture::from_source("npm-single");
    let first = plan_init(&SupportedHost::reference(), &exact.root, &request()).unwrap();
    let bytes = first
        .effective_configuration()
        .deterministic_bytes()
        .unwrap();
    fs::write(exact.root.join(".agent-lowmem.json"), bytes).unwrap();
    let adopted = plan_init(&SupportedHost::reference(), &exact.root, &request()).unwrap();
    assert_eq!(
        file_action(adopted.report(), ManagedIdentity::Configuration),
        ManagedAction::Unchanged
    );
}

#[test]
fn enforces_workspace_and_manual_candidate_caps() {
    let workspace_limit = RepositoryFixture::from_source("npm-single");
    workspace_limit.write(
        "package.json",
        r#"{"name":"root","private":true,"packageManager":"npm@12.0.2","workspaces":["apps/*"],"scripts":{"test":"vitest run"}}"#,
    );
    for index in 0..128 {
        workspace_limit.write(
            &format!("apps/w{index}/package.json"),
            &format!(r#"{{"name":"w{index}","scripts":{{"test":"vitest run"}}}}"#),
        );
    }
    let boundary = plan_init(
        &SupportedHost::reference(),
        &workspace_limit.root,
        &request(),
    )
    .unwrap();
    assert_eq!(boundary.effective_configuration().workspaces.len(), 128);
    workspace_limit.write(
        "apps/w128/package.json",
        r#"{"name":"w128","scripts":{"test":"vitest run"}}"#,
    );
    assert_eq!(
        plan_init(
            &SupportedHost::reference(),
            &workspace_limit.root,
            &request()
        )
        .unwrap_err()
        .reason(),
        Reason::WorkspaceUnsupported
    );

    let candidate_limit = RepositoryFixture::from_source("npm-single");
    let mut scripts = BTreeMap::from([("test".to_owned(), "vitest run".to_owned())]);
    for index in 0..256 {
        scripts.insert(format!("test:c{index}"), "vitest run".to_owned());
    }
    candidate_limit.write(
        "package.json",
        &serde_json::json!({
            "name": "root",
            "private": true,
            "packageManager": "npm@12.0.2",
            "scripts": scripts,
        })
        .to_string(),
    );
    let boundary = plan_init(
        &SupportedHost::reference(),
        &candidate_limit.root,
        &request(),
    )
    .unwrap();
    assert_eq!(boundary.report().manual_candidates.len(), 256);
    scripts.insert("test:overflow".to_owned(), "vitest run".to_owned());
    candidate_limit.write(
        "package.json",
        &serde_json::json!({
            "name": "root",
            "private": true,
            "packageManager": "npm@12.0.2",
            "scripts": scripts,
        })
        .to_string(),
    );
    assert_eq!(
        plan_init(
            &SupportedHost::reference(),
            &candidate_limit.root,
            &request()
        )
        .unwrap_err()
        .reason(),
        Reason::WorkspaceUnsupported
    );
}

#[test]
fn rejects_malformed_managed_destinations_and_redacts_the_public_plan() {
    let malformed = RepositoryFixture::from_source("npm-single");
    malformed.write("AGENTS.md", "<!-- agent-lowmem:start broken -->\n");
    let conflict = plan_init(&SupportedHost::reference(), &malformed.root, &request()).unwrap_err();
    assert_eq!(conflict.reason(), Reason::ManagedFileConflict);
    assert_eq!(conflict.report().result.code, 78);

    let private = RepositoryFixture::from_source("npm-single");
    private.write(
        "package.json",
        r#"{"name":"secret-root","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"SECRET_TOKEN=value vitest run"}}"#,
    );
    let failure = plan_init(&SupportedHost::reference(), &private.root, &request()).unwrap_err();
    let report = serde_json::to_string(failure.report()).unwrap();
    let debug = format!("{failure:?}");
    for secret in ["SECRET_TOKEN", "value", private.root.to_str().unwrap()] {
        assert!(!report.contains(secret));
        assert!(!debug.contains(secret));
    }
}

#[cfg(unix)]
#[test]
fn init_planning_starts_none_of_the_repository_executables() {
    let fixture = RepositoryFixture::from_source("npm-single");
    let sentinels = fixture.root.join("sentinels");
    fs::create_dir(&sentinels).unwrap();
    let marker = fixture.root.join("child-started");
    let body = "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";
    for name in [
        "git", "node", "npm", "pnpm", "vitest", "jest", "tsc", "eslint", "next", "nest",
    ] {
        let path = sentinels.join(name);
        fs::write(&path, body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "planner_child_probe", "--nocapture"])
        .env("AGENT_LOWMEM_PLANNER_PROBE", &fixture.root)
        .env("PATH", format!("{}:/usr/bin:/bin", sentinels.display()))
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists());
}

#[test]
fn planner_child_probe() {
    let Ok(root) = std::env::var("AGENT_LOWMEM_PLANNER_PROBE") else {
        return;
    };
    plan_init(&SupportedHost::reference(), Path::new(&root), &request()).unwrap();
}

fn file_action(
    report: &agent_lowmem::managed_files::ManagedFilesReport,
    identity: ManagedIdentity,
) -> ManagedAction {
    report
        .files
        .iter()
        .find(|file| file.identity == identity)
        .unwrap()
        .action
}

fn request() -> InitRequest {
    InitRequest {
        dry_run: true,
        json: true,
    }
}

struct SupportedHost {
    operating_system: &'static str,
    architecture: &'static str,
    values: BTreeMap<&'static str, &'static str>,
}

impl SupportedHost {
    fn reference() -> Self {
        Self {
            operating_system: "macos",
            architecture: "aarch64",
            values: BTreeMap::from([
                ("kern.osproductversion", "26.6.2"),
                ("hw.model", "Mac14,15"),
                ("machdep.cpu.brand_string", "Apple M2"),
                ("hw.memsize", "8589934592"),
                ("hw.pagesize", "16384"),
            ]),
        }
    }
}

impl HostSource for SupportedHost {
    fn operating_system(&self) -> &str {
        self.operating_system
    }

    fn architecture(&self) -> &str {
        self.architecture
    }

    fn read(&self, key: &'static str) -> Result<String, HostReadError> {
        self.values
            .get(key)
            .map(|value| (*value).to_owned())
            .ok_or(HostReadError::Missing(key))
    }
}

struct RepositoryFixture {
    root: PathBuf,
}

impl RepositoryFixture {
    fn from_source(name: &str) -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-plan-{name}-{nanos}-{}-{serial}",
            std::process::id()
        ));
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/repositories")
                .join(name),
            &root,
        );
        fs::create_dir(root.join(".git")).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
        }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }
}

impl Drop for RepositoryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
