use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn json_inspection_uses_the_current_github_repository_and_reports_active_workflows() {
    let fixture = Fixture::new();

    let output = fixture.run(&["github", "inspect", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["repository"], "Pleo2/agent-lowmem");
    assert_eq!(report["workflowCount"], 2);
    assert_eq!(report["inspectedWorkflowCount"], 2);
    assert_eq!(report["activeWorkflowCount"], 1);
    assert_eq!(report["result"]["origin"], "child");
    assert_eq!(report["result"]["code"], 0);
    assert_eq!(report["result"]["reason"], "completed");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "agent-lowmem: result origin=child code=0 reason=completed\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("gh-arguments")).unwrap(),
        concat!(
            "api\n--method\nGET\n",
            "repos/Pleo2/agent-lowmem/actions/workflows?per_page=100\n",
            "--jq\n",
            "{total_count: .total_count, workflows: [.workflows[] | {state: .state}]}\n"
        )
    );
}

#[test]
fn human_inspection_uses_terminal_branding_without_escape_codes_when_redirected() {
    let fixture = Fixture::new();

    let output = fixture.run(&["github", "inspect"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with(
        "agent_lowmem\nGitHub repository: Pleo2/agent-lowmem\nWorkflows: 2 total, 2 inspected, 1 active\n"
    ));
    assert!(!stdout.contains('\u{1b}'));
}

#[test]
fn public_json_report_has_a_closed_versioned_schema() {
    let schema_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/github-inspect-v1.schema.json");

    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();

    assert_eq!(
        schema["$id"],
        "https://agentlowmem.dev/schema/github-inspect-v1.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["required"],
        serde_json::json!([
            "schemaVersion",
            "repository",
            "workflowCount",
            "inspectedWorkflowCount",
            "activeWorkflowCount",
            "recommendations",
            "result"
        ])
    );
    assert_eq!(schema["properties"]["schemaVersion"]["const"], 1);
    assert_eq!(
        schema["properties"]["result"]["properties"]["reason"]["const"],
        "completed"
    );
}

#[test]
fn missing_github_cli_is_a_closed_preflight_failure() {
    let fixture = Fixture::without_gh();

    let output = fixture.run(&["github", "inspect", "--json"]);

    assert_eq!(output.status.code(), Some(64));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "agent-lowmem: result origin=preflight code=64 reason=tool-unsupported\n"
    );
}

#[test]
fn github_api_failure_preserves_the_child_exit_code_without_stderr_disclosure() {
    let fixture = Fixture::with_gh("#!/bin/sh\nprintf 'PRIVATE API ERROR' >&2\nexit 42\n");

    let output = fixture.run(&["github", "inspect", "--json"]);

    assert_eq!(output.status.code(), Some(42));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "agent-lowmem: result origin=child code=42 reason=child-exit\n"
    );
}

#[test]
fn malformed_api_output_is_an_internal_failure_without_echoing_the_payload() {
    let fixture = Fixture::with_gh("#!/bin/sh\nprintf 'PRIVATE MALFORMED RESPONSE'\n");

    let output = fixture.run(&["github", "inspect", "--json"]);

    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "agent-lowmem: result origin=internal code=70 reason=internal-error\n"
    );
}

#[test]
fn github_command_grammar_is_strict() {
    let fixture = Fixture::new();

    for arguments in [
        vec!["github"],
        vec!["github", "inspect", "--js"],
        vec!["github", "inspect", "--json", "extra"],
    ] {
        let output = fixture.run(&arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .ends_with("code=2 reason=invalid-cli\n")
        );
    }
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self::without_gh();
        write_executable(
            &fixture.bin.join("gh"),
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s\\n' '{{\"total_count\":2,\"workflows\":[{{\"name\":\"CI\",\"path\":\".github/workflows/ci.yml\",\"state\":\"active\"}},{{\"name\":\"Nightly\",\"path\":\".github/workflows/nightly.yml\",\"state\":\"disabled_manually\"}}]}}'\n",
                fixture.root.join("gh-arguments").display()
            ),
        );
        fixture
    }

    fn with_gh(script: &str) -> Self {
        let fixture = Self::without_gh();
        write_executable(&fixture.bin.join("gh"), script);
        fixture
    }

    fn without_gh() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-github-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        let bin = base.join("bin");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(
            root.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:Pleo2/agent-lowmem.git\n",
        )
        .unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
            base,
            bin,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(arguments)
            .current_dir(&self.root)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}
