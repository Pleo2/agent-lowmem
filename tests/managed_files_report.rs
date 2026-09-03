use agent_lowmem::{
    managed_files::{
        ManagedAction, ManagedCommand, ManagedFileReport, ManagedFilesReport, ManagedIdentity,
        ManagedIssueReport, ManagedOperationReport, ManagedOutcome, ManagedResult, ManifestState,
        ManualCandidateReport,
    },
    result::Reason,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn serializes_one_schema_valid_sample_for_every_outcome() {
    let cases = [
        (
            ManagedCommand::Init,
            true,
            ManagedOutcome::Planned,
            0,
            Reason::Completed,
        ),
        (
            ManagedCommand::Init,
            false,
            ManagedOutcome::Applied,
            0,
            Reason::Completed,
        ),
        (
            ManagedCommand::Restore,
            false,
            ManagedOutcome::Restored,
            0,
            Reason::Completed,
        ),
        (
            ManagedCommand::Restore,
            true,
            ManagedOutcome::Unchanged,
            0,
            Reason::Completed,
        ),
        (
            ManagedCommand::Init,
            true,
            ManagedOutcome::RecoveryRequired,
            0,
            Reason::ManagedFileConflict,
        ),
        (
            ManagedCommand::Restore,
            false,
            ManagedOutcome::Conflict,
            78,
            Reason::ManagedFileConflict,
        ),
        (
            ManagedCommand::Init,
            false,
            ManagedOutcome::Failed,
            64,
            Reason::HostUnsupported,
        ),
    ];

    for (command, dry_run, outcome, code, reason) in cases {
        let value = serde_json::to_value(report(command, dry_run, outcome, code, reason)).unwrap();
        assert_schema_valid(&value);
    }
}

#[test]
fn rejects_unknown_tokens_extra_fields_invalid_hashes_and_private_data() {
    let base = serde_json::to_value(report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    ))
    .unwrap();

    for (pointer, invalid) in [
        ("/files/0/identity", json!("readme")),
        ("/files/0/action", json!("overwrite")),
        ("/result/reason", json!("child-exit")),
        ("/files/1/beforeSha256", json!("ABC123")),
        ("/issues/1/workspacePath", json!("/Users/example/private")),
    ] {
        let mut value = base.clone();
        *value.pointer_mut(pointer).unwrap() = invalid;
        assert_schema_invalid(&value, pointer);
    }

    for (pointer, name, value) in [
        ("", "timestamp", json!("2026-09-03T12:00:00Z")),
        ("", "transactionId", json!("secret-transaction")),
        ("/files/0", "targetContent", json!("private bytes")),
    ] {
        let mut invalid = base.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(name.into(), value);
        assert_schema_invalid(&invalid, name);
    }
}

#[test]
fn constructor_sorts_every_public_collection_and_rejects_invalid_result_families() {
    let report = report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    );
    let value = serde_json::to_value(report).unwrap();

    assert_eq!(
        value.pointer("/files/0/identity"),
        Some(&json!("configuration"))
    );
    assert_eq!(
        value.pointer("/files/1/identity"),
        Some(&json!("agents-policy"))
    );
    assert_eq!(
        value.pointer("/operations/0/operationKey"),
        Some(&json!("test"))
    );
    assert_eq!(
        value.pointer("/manualCandidates/0/scriptName"),
        Some(&json!("lint:unit"))
    );
    assert_eq!(
        value.pointer("/issues/0/reason"),
        Some(&json!("invalid-config"))
    );
    assert!(value.pointer("/files/0/beforeSha256").is_none());
    assert!(value.pointer("/operations/0/workspaceKey").is_none());

    assert!(ManagedResult::new(124, Reason::DeadlineExceeded).is_err());
    assert!(ManagedResult::new(0, Reason::InternalError).is_err());
    assert!(
        ManagedFilesReport::new(
            ManagedCommand::Init,
            false,
            ManagedOutcome::RecoveryRequired,
            ManagedResult::new(0, Reason::ManagedFileConflict).unwrap(),
            vec![],
            vec![],
            vec![],
            vec![],
            ManifestState::Prepared,
        )
        .is_err()
    );
}

#[test]
fn rust_and_schema_share_the_exact_phase_four_reason_and_code_contract() {
    let excluded = [
        Reason::ChildExit,
        Reason::ChildSignal,
        Reason::DeadlineExceeded,
        Reason::ExternalSignal,
    ];
    let expected_reasons: Vec<_> = Reason::ALL
        .into_iter()
        .filter(|reason| !excluded.contains(reason))
        .map(Reason::as_str)
        .collect();
    assert_eq!(
        schema().pointer("/$defs/reason/enum").unwrap(),
        &json!(expected_reasons)
    );

    for reason in Reason::ALL {
        let code = match reason {
            Reason::Completed => Some(0),
            Reason::InvalidCli | Reason::InvalidConfig => Some(2),
            Reason::HostUnsupported
            | Reason::RepositoryUnsupported
            | Reason::PackageManagerUnsupported
            | Reason::WorkspaceUnsupported
            | Reason::WorkspaceCardinality
            | Reason::OperationUnsupported
            | Reason::ScriptSyntaxUnsupported
            | Reason::ScriptShellUnsupported
            | Reason::ScriptReferenceUnsupported
            | Reason::ScriptGraphTooLarge
            | Reason::WrapperUnsupported
            | Reason::ToolUnsupported
            | Reason::ToolVersionUnsupported
            | Reason::WatchDenied
            | Reason::UiDenied
            | Reason::BackgroundDenied
            | Reason::ParallelDenied
            | Reason::ArgumentDenied
            | Reason::NonfinalInjectionRequired => Some(64),
            Reason::LockHeld | Reason::NestedInvocation => Some(73),
            Reason::EvidenceChanged => Some(75),
            Reason::ManagedFileConflict => Some(78),
            Reason::InternalError => Some(70),
            Reason::ChildExit
            | Reason::ChildSignal
            | Reason::DeadlineExceeded
            | Reason::ExternalSignal => None,
        };

        assert_eq!(
            code.is_some(),
            code.is_some_and(|code| ManagedResult::new(code, reason).is_ok()),
            "unexpected result family for {}",
            reason.as_str()
        );
        assert!(ManagedResult::new(-1, reason).is_err());
    }
    assert!(ManagedResult::new(0, Reason::ManagedFileConflict).is_ok());
}

#[test]
fn typed_construction_rejects_non_public_or_malformed_fields() {
    let mut invalid_hash = report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    );
    invalid_hash.files[0].target_sha256 = Some("ABC123".into());
    assert!(rebuild(invalid_hash).is_err());

    let mut invalid_operation = report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    );
    invalid_operation.operations[0].operation_key = "../test".into();
    assert!(rebuild(invalid_operation).is_err());

    let mut invalid_candidate = report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    );
    invalid_candidate.manual_candidates[0].script_name = "TOKEN=secret".into();
    assert!(rebuild(invalid_candidate).is_err());

    let mut absolute_issue = report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    );
    absolute_issue.issues[1].workspace_path = Some("/Users/private".into());
    assert!(rebuild(absolute_issue).is_err());

    assert!(
        ManagedFilesReport::new(
            ManagedCommand::Init,
            false,
            ManagedOutcome::Restored,
            ManagedResult::new(0, Reason::Completed).unwrap(),
            vec![],
            vec![],
            vec![],
            vec![],
            ManifestState::Absent,
        )
        .is_err()
    );
    assert!(
        ManagedFilesReport::new(
            ManagedCommand::Init,
            false,
            ManagedOutcome::Failed,
            ManagedResult::new(78, Reason::ManagedFileConflict).unwrap(),
            vec![],
            vec![],
            vec![],
            vec![],
            ManifestState::Prepared,
        )
        .is_err()
    );
}

#[test]
fn serialized_report_omits_sensitive_and_ansi_values() {
    let root_path = "/Users/piolinos/private/repository";
    let git_path = "/Users/piolinos/private/repository/.git/worktrees/private";
    let secret = "TOKEN=private-value";
    let raw_script = "node --test --password private";
    let serialized = serde_json::to_string(&report(
        ManagedCommand::Init,
        true,
        ManagedOutcome::Planned,
        0,
        Reason::Completed,
    ))
    .unwrap();

    for forbidden in [
        root_path,
        git_path,
        secret,
        raw_script,
        "transactionId",
        "timestamp",
    ] {
        assert!(!serialized.contains(forbidden));
    }
    assert!(!serialized.contains('\u{1b}'));
}

fn report(
    command: ManagedCommand,
    dry_run: bool,
    outcome: ManagedOutcome,
    code: i32,
    reason: Reason,
) -> ManagedFilesReport {
    ManagedFilesReport::new(
        command,
        dry_run,
        outcome,
        ManagedResult::new(code, reason).unwrap(),
        vec![
            ManagedFileReport {
                identity: ManagedIdentity::AgentsPolicy,
                action: ManagedAction::Replace,
                before_sha256: Some(HASH_A.into()),
                target_sha256: Some(HASH_B.into()),
            },
            ManagedFileReport {
                identity: ManagedIdentity::Configuration,
                action: ManagedAction::Create,
                before_sha256: None,
                target_sha256: Some(HASH_A.into()),
            },
        ],
        vec![
            ManagedOperationReport {
                operation_key: "typecheck".into(),
                workspace_key: Some("web".into()),
            },
            ManagedOperationReport {
                operation_key: "test".into(),
                workspace_key: None,
            },
        ],
        vec![
            ManualCandidateReport {
                operation_prefix: "test".into(),
                script_name: "test:unit".into(),
                workspace_key: Some("web".into()),
            },
            ManualCandidateReport {
                operation_prefix: "lint".into(),
                script_name: "lint:unit".into(),
                workspace_key: None,
            },
        ],
        vec![
            ManagedIssueReport {
                reason: Reason::WorkspaceCardinality,
                operation_key: Some("test".into()),
                workspace_path: Some("packages/web".into()),
                package_name: Some("@example/web".into()),
            },
            ManagedIssueReport {
                reason: Reason::InvalidConfig,
                operation_key: None,
                workspace_path: None,
                package_name: None,
            },
        ],
        ManifestState::Absent,
    )
    .unwrap()
}

fn rebuild(report: ManagedFilesReport) -> Result<ManagedFilesReport, Reason> {
    ManagedFilesReport::new(
        report.command,
        report.dry_run,
        report.outcome,
        report.result,
        report.files,
        report.operations,
        report.manual_candidates,
        report.issues,
        report.manifest_state,
    )
}

fn schema() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/managed-files-result-v1.schema.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn assert_schema_valid(value: &Value) {
    if let Err(error) = validate(&schema(), value, &schema()) {
        panic!("expected schema-valid value: {error}\n{value:#}");
    }
}

fn assert_schema_invalid(value: &Value, label: &str) {
    assert!(
        validate(&schema(), value, &schema()).is_err(),
        "expected schema-invalid {label}: {value:#}"
    );
}

fn validate(schema: &Value, value: &Value, root: &Value) -> Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference
            .strip_prefix('#')
            .ok_or("non-local schema reference")?;
        return validate(
            root.pointer(pointer).ok_or("missing schema reference")?,
            value,
            root,
        );
    }
    if let Some(expected) = schema.get("const") {
        if value != expected {
            return Err("const mismatch".into());
        }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.contains(value) {
            return Err("enum mismatch".into());
        }
    }
    if let Some(expected_type) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected_type {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some(),
            "boolean" => value.is_boolean(),
            _ => false,
        };
        if !matches {
            return Err(format!("type mismatch: {expected_type}"));
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(minimum) = schema.get("minLength").and_then(Value::as_u64) {
            if text.chars().count() < minimum as usize {
                return Err("string shorter than minLength".into());
            }
        }
        if let Some(maximum) = schema.get("maxLength").and_then(Value::as_u64) {
            if text.chars().count() > maximum as usize {
                return Err("string longer than maxLength".into());
            }
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            let matches = match pattern {
                "^[0-9a-f]{64}$" => is_sha256(text),
                "^[a-z][a-z0-9-]{0,31}$" => is_key(text),
                "^(?!.*(?:^|/)\\.{1,2}(?:/|$))[^/\\\\\\u0000]+(?:/[^/\\\\\\u0000]+)*$" => {
                    is_relative_path(text)
                }
                _ => return Err(format!("unsupported test pattern: {pattern}")),
            };
            if !matches {
                return Err("pattern mismatch".into());
            }
        }
    }
    if let Some(object) = value.as_object() {
        let properties = schema.get("properties").and_then(Value::as_object);
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            let allowed: BTreeSet<_> = properties
                .into_iter()
                .flat_map(|properties| properties.keys())
                .collect();
            if object.keys().any(|key| !allowed.contains(key)) {
                return Err("additional property".into());
            }
        }
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("missing required property: {key}"));
                }
            }
        }
        if let Some(properties) = properties {
            for (key, child_schema) in properties {
                if let Some(child) = object.get(key) {
                    validate(child_schema, child, root)?;
                }
            }
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(maximum) = schema.get("maxItems").and_then(Value::as_u64) {
            if array.len() > maximum as usize {
                return Err("array longer than maxItems".into());
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for item in array {
                validate(item_schema, item, root)?;
            }
        }
    }
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        if branches
            .iter()
            .filter(|branch| validate(branch, value, root).is_ok())
            .count()
            != 1
        {
            return Err("oneOf mismatch".into());
        }
    }
    if let Some(clauses) = schema.get("allOf").and_then(Value::as_array) {
        for clause in clauses {
            if clause
                .get("if")
                .is_none_or(|condition| validate(condition, value, root).is_ok())
            {
                if let Some(then) = clause.get("then") {
                    validate(then, value, root)?;
                }
            }
        }
    }
    if let Some(not) = schema.get("not") {
        if validate(not, value, root).is_ok() {
            return Err("not mismatch".into());
        }
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn is_relative_path(value: &str) -> bool {
    !value.starts_with('/')
        && !value.contains(['\\', '\0'])
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
