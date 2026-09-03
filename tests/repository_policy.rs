use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

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
            "agent-lowmem-policy-{name}-{nanos}-{}-{serial}",
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

    fn doctor(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(["doctor", "--json"])
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn doctor_human(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .arg("doctor")
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn report(&self) -> serde_json::Value {
        let output = self.doctor();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for RepositoryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn reports_npm_and_pnpm_root_and_workspace_operations() {
    for (fixture_name, manager, workspace, operation, configured) in [
        ("npm-single", "npm", None, "test", false),
        ("npm-workspace", "npm", Some("web"), "test", true),
        ("pnpm-single", "pnpm", None, "test", false),
        ("pnpm-workspace", "pnpm", Some("api"), "lint", true),
    ] {
        let fixture = RepositoryFixture::from_source(fixture_name);
        let report = fixture.report();
        assert_eq!(report["phase"], "repository-policy");
        assert_eq!(report["repository"]["packageManager"]["kind"], manager);
        let operations = report["repository"]["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["workspaceKey"].as_str(), workspace);
        assert_eq!(operations[0]["operationKey"], operation);
        assert_eq!(operations[0]["status"], "runnable");
        assert_eq!(operations[0]["configured"], configured);
        assert!(operations[0]["reason"].is_null());
        let output = serde_json::to_string(&report).unwrap();
        assert!(!output.contains(fixture.root.to_str().unwrap()));
    }
}

#[test]
fn human_report_uses_only_stable_operation_evidence() {
    let fixture = RepositoryFixture::from_source("npm-single");
    let output = fixture.doctor_human();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("Phase: repository-policy"));
    assert!(stdout.contains("Managed runs: unavailable in Phase 2"));
    assert!(stdout.contains("root:test [candidate] runnable (compatible)"));
    assert!(stdout.contains("design the managed runner"));
    assert!(!stdout.contains("vitest run"));
    assert!(!stdout.contains(fixture.root.to_str().unwrap()));
}

#[test]
fn reports_structural_workspace_and_shell_failures_without_guessing() {
    let duplicate = RepositoryFixture::from_source("npm-workspace");
    duplicate.write("apps/duplicate/package.json", r#"{"name":"@acme/web"}"#);
    assert_eq!(
        duplicate.report()["repository"]["failureReason"],
        "workspace-cardinality"
    );

    let mismatch = RepositoryFixture::from_source("npm-workspace");
    mismatch.write(
        ".agent-lowmem.json",
        r#"{"version":1,"packageManager":"npm","workspaces":{"web":{"path":"apps/web","packageName":"@acme/wrong","operations":{"test":{"script":"test","timeoutSeconds":600}}}}}"#,
    );
    assert_eq!(
        mismatch.report()["repository"]["operations"][0]["reason"],
        "workspace-cardinality"
    );

    let zero_match = RepositoryFixture::from_source("npm-workspace");
    zero_match.write(
        ".agent-lowmem.json",
        r#"{"version":1,"packageManager":"npm","workspaces":{"web":{"path":"apps/missing","packageName":"@acme/missing","operations":{"test":{"script":"test","timeoutSeconds":600}}}}}"#,
    );
    assert_eq!(
        zero_match.report()["repository"]["operations"][0]["reason"],
        "workspace-cardinality"
    );

    let unsupported = RepositoryFixture::from_source("npm-workspace");
    unsupported.write(
        "package.json",
        r#"{"name":"bad","packageManager":"npm@12.0.2","workspaces":["apps/**"]}"#,
    );
    assert_eq!(
        unsupported.report()["repository"]["failureReason"],
        "workspace-unsupported"
    );

    let shell = RepositoryFixture::from_source("npm-single");
    shell.write(".npmrc", "script-shell=/bin/bash\n");
    assert_eq!(
        shell.report()["repository"]["failureReason"],
        "script-shell-unsupported"
    );
}

#[cfg(unix)]
#[test]
fn rejects_workspace_symlink_escapes() {
    use std::os::unix::fs::symlink;

    let fixture = RepositoryFixture::from_source("npm-workspace");
    let outside = RepositoryFixture::from_source("npm-single");
    symlink(&outside.root, fixture.root.join("apps/escape")).unwrap();
    assert_eq!(
        fixture.report()["repository"]["failureReason"],
        "workspace-unsupported"
    );
}

#[test]
fn reports_tool_syntax_graph_wrapper_and_denial_failures_redacted() {
    let unrelated_node_file = RepositoryFixture::from_source("npm-single");
    unrelated_node_file.write(".node-version", "lts/*\n");
    assert_eq!(operation_reason(&unrelated_node_file), "none");

    let invalid_node = RepositoryFixture::from_source("pnpm-single");
    invalid_node.write(".node-version", "lts/*\n");
    assert_eq!(operation_reason(&invalid_node), "tool-version-unsupported");

    let unknown = RepositoryFixture::from_source("npm-single");
    unknown.write(
        "node_modules/vitest/package.json",
        r#"{"name":"vitest","version":"4.1.12"}"#,
    );
    assert_eq!(operation_reason(&unknown), "tool-version-unsupported");

    let hostile = RepositoryFixture::from_source("hostile");
    let hostile_output = serde_json::to_string(&hostile.report()).unwrap();
    assert_eq!(operation_reason(&hostile), "script-syntax-unsupported");
    assert!(!hostile_output.contains("SECRET-output"));
    assert!(!hostile_output.contains("tee"));

    let graph = RepositoryFixture::from_source("npm-single");
    let script = std::iter::repeat_n("vitest run", 33)
        .collect::<Vec<_>>()
        .join(" && ");
    graph.write(
        "package.json",
        &format!(
            r#"{{"name":"graph","packageManager":"npm@12.0.2","scripts":{{"test":"{script}"}}}}"#
        ),
    );
    assert_eq!(operation_reason(&graph), "script-graph-too-large");

    let wrapper = RepositoryFixture::from_source("npm-single");
    wrapper.write(
        "package.json",
        r#"{"name":"wrapper","packageManager":"npm@12.0.2","scripts":{"test":"cross-env SECRET=value vitest run"}}"#,
    );
    wrapper.write(
        "node_modules/cross-env/package.json",
        r#"{"name":"cross-env","version":"10.1.0"}"#,
    );
    let wrapper_output = serde_json::to_string(&wrapper.report()).unwrap();
    assert_eq!(operation_reason(&wrapper), "none");
    assert!(!wrapper_output.contains("SECRET"));
    assert!(!wrapper_output.contains("value"));

    for (script, reason) in [
        ("vitest run --watch", "watch-denied"),
        ("vitest run --ui", "ui-denied"),
        ("vitest run --standalone", "background-denied"),
        ("vitest run --maxWorkers=2", "parallel-denied"),
    ] {
        let fixture = RepositoryFixture::from_source("npm-single");
        fixture.write(
            "package.json",
            &format!(r#"{{"name":"denial","packageManager":"npm@12.0.2","scripts":{{"test":"{script}"}}}}"#),
        );
        assert_eq!(operation_reason(&fixture), reason);
    }

    let argument = RepositoryFixture::from_source("npm-single");
    argument.write(
        "package.json",
        r#"{"name":"argument","packageManager":"npm@12.0.2","scripts":{"build":"next build --experimental-build-mode=compile"}}"#,
    );
    argument.write(
        "node_modules/next/package.json",
        r#"{"name":"next","version":"16.3.4"}"#,
    );
    assert_eq!(operation_reason_for(&argument, 0), "argument-denied");
}

#[cfg(unix)]
#[test]
fn doctor_inspection_starts_none_of_the_repository_executables() {
    let fixture = RepositoryFixture::from_source("npm-single");
    let sentinels = fixture.root.join("sentinels");
    fs::create_dir(&sentinels).unwrap();
    let marker = fixture.root.join("child-started");
    let body = "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";
    for name in [
        "git",
        "node",
        "npm",
        "pnpm",
        "vitest",
        "jest",
        "tsc",
        "eslint",
        "next",
        "nest",
        "cross-env",
        "dotenv",
        "rimraf",
    ] {
        let path = sentinels.join(name);
        fs::write(&path, body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["doctor", "--json"])
        .current_dir(&fixture.root)
        .env("PATH", format!("{}:/usr/bin:/bin", sentinels.display()))
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &marker)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(!marker.exists());
}

fn operation_reason(fixture: &RepositoryFixture) -> String {
    operation_reason_for(fixture, 0)
}

fn operation_reason_for(fixture: &RepositoryFixture, index: usize) -> String {
    fixture.report()["repository"]["operations"][index]["reason"]
        .as_str()
        .unwrap_or("none")
        .to_owned()
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
