use crate::{
    adapter::ControlDecision,
    atomic_file::HeldDirectory,
    configuration::valid_relative_path,
    policy::PolicyTarget,
    repository::{PackageManagerReport, RunPlan},
    result::{ExitResult, Origin, Reason},
    supervisor::{CleanupAction, SupervisionReport},
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_RFC3339_SECONDS: u64 = 253_402_300_799;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockState {
    NotAcquired,
    Acquired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceRecheckState {
    NotRun,
    Matched,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnState {
    NotAttempted,
    Failed,
    Started,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLifecycle {
    pub lock: LockState,
    pub evidence_recheck: EvidenceRecheckState,
    pub spawn: SpawnState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultEvidence {
    relative_path: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResultDetails {
    operation_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_key: Option<String>,
    package_manager: PackageManagerReport,
    evidence: Vec<ResultEvidence>,
    graph_depth: u8,
    leaf_count: usize,
    applied_controls: Vec<String>,
    disclosures: Vec<String>,
    timeout_seconds: u16,
    warning_emitted: bool,
    elapsed_millis: u64,
    lock_state: LockState,
    evidence_recheck_state: EvidenceRecheckState,
    spawn_state: SpawnState,
    cleanup_action: CleanupAction,
    cleanup_complete: bool,
    forwarded_argument_count: usize,
}

impl RunResultDetails {
    pub fn from_plan(
        plan: &RunPlan,
        lifecycle: RunLifecycle,
        supervision: Option<&SupervisionReport>,
    ) -> Self {
        let policy = plan.policy();
        let workspace_key = match &policy.target {
            PolicyTarget::Root => None,
            PolicyTarget::Workspace { key, .. } => Some(key.clone()),
        };
        let applied_controls = policy
            .leaves
            .iter()
            .filter_map(|leaf| match leaf.control {
                ControlDecision::AlreadyControlled => Some("already-controlled"),
                ControlDecision::RequiresSuffix(_) => Some("suffix-injected"),
                ControlDecision::NoControl => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let (warning_emitted, elapsed_millis, cleanup_action, cleanup_complete) = supervision
            .map(|report| {
                (
                    report.warning_emitted,
                    report.elapsed_millis,
                    report.cleanup_action,
                    report.cleanup_complete,
                )
            })
            .unwrap_or((false, 0, CleanupAction::None, true));

        Self {
            operation_key: policy.operation_key.clone(),
            workspace_key,
            package_manager: plan.package_manager().clone(),
            evidence: plan
                .evidence()
                .files()
                .iter()
                .map(|file| ResultEvidence {
                    relative_path: file.relative_path().to_owned(),
                    sha256: file.hex(),
                })
                .collect(),
            graph_depth: policy.graph_depth,
            leaf_count: policy.leaves.len(),
            applied_controls,
            disclosures: policy.disclosures.clone(),
            timeout_seconds: policy.timeout_seconds,
            warning_emitted,
            elapsed_millis,
            lock_state: lifecycle.lock,
            evidence_recheck_state: lifecycle.evidence_recheck,
            spawn_state: lifecycle.spawn,
            cleanup_action,
            cleanup_complete,
            forwarded_argument_count: plan.forwarded_argument_count(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResultRecord {
    schema_version: u8,
    timestamp: String,
    origin: Origin,
    code: i32,
    reason: Reason,
    message: &'static str,
    next_action: &'static str,
    child_started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<RunResultDetails>,
}

impl RunResultRecord {
    pub fn now(
        result: ExitResult,
        child_started: bool,
        details: Option<RunResultDetails>,
    ) -> Result<Self, Reason> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Reason::InternalError)?
            .as_secs();
        Self::at_unix_seconds(result, child_started, details, seconds)
    }

    pub fn at_unix_seconds(
        result: ExitResult,
        child_started: bool,
        details: Option<RunResultDetails>,
        seconds: u64,
    ) -> Result<Self, Reason> {
        let child_state_valid = match result.origin {
            Origin::Preflight => !child_started,
            Origin::Child | Origin::SupervisorTimeout | Origin::ExternalSignal => child_started,
            Origin::Internal => true,
        };
        if !result.is_valid() || !child_state_valid {
            return Err(Reason::InternalError);
        }
        Ok(Self {
            schema_version: 1,
            timestamp: format_rfc3339_utc(seconds)?,
            origin: result.origin,
            code: result.code,
            reason: result.reason,
            message: result.reason.message(),
            next_action: result.reason.next_action(),
            child_started,
            details,
        })
    }
}

#[derive(Debug)]
pub struct ValidatedResultDestination {
    parent: HeldDirectory,
    file_name: String,
}

pub fn validate_result_destination(
    root: &Path,
    relative_path: &str,
) -> Result<ValidatedResultDestination, Reason> {
    if !valid_relative_path(relative_path) {
        return Err(Reason::ManagedFileConflict);
    }
    let components = relative_path.split('/').collect::<Vec<_>>();
    let mut parent = HeldDirectory::open(root, None)?;
    for component in &components[..components.len() - 1] {
        parent = HeldDirectory::open_child(&parent, component, None)?;
    }
    let file_name = components
        .last()
        .expect("validated non-empty path")
        .to_string();
    parent.ensure_replaceable(&file_name)?;
    Ok(ValidatedResultDestination { parent, file_name })
}

pub fn write_validated_result_atomic(
    destination: &ValidatedResultDestination,
    record: &RunResultRecord,
) -> Result<(), Reason> {
    let mut bytes = serde_json::to_vec_pretty(record).map_err(|_| Reason::InternalError)?;
    bytes.push(b'\n');
    let expected = destination.parent.precondition(&destination.file_name)?;
    destination
        .parent
        .replace_atomic(&destination.file_name, &expected, &bytes, 0o600)
}

pub fn write_result_atomic(
    root: &Path,
    relative_path: &str,
    record: &RunResultRecord,
) -> Result<(), Reason> {
    let destination = validate_result_destination(root, relative_path)?;
    write_validated_result_atomic(&destination, record)
}

fn format_rfc3339_utc(seconds: u64) -> Result<String, Reason> {
    if seconds > MAX_RFC3339_SECONDS {
        return Err(Reason::InternalError);
    }
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let shifted = days_since_epoch + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{RunSelection, plan_run};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[cfg(unix)]
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "agent-lowmem-result-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            Self(fs::canonicalize(root).unwrap())
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
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn formats_unix_time_as_utc_rfc3339() {
        assert_eq!(format_rfc3339_utc(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_rfc3339_utc(951_782_400).unwrap(),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            format_rfc3339_utc(MAX_RFC3339_SECONDS).unwrap(),
            "9999-12-31T23:59:59Z"
        );
        assert_eq!(
            format_rfc3339_utc(MAX_RFC3339_SECONDS + 1),
            Err(Reason::InternalError)
        );
    }

    #[test]
    fn writes_mode_0600_and_replaces_in_the_same_directory() {
        let fixture = Fixture::new();
        let target = fixture.0.join("result.json");
        let old = fixture.0.join("old-result.json");
        fs::write(&target, "old").unwrap();
        fs::hard_link(&target, &old).unwrap();

        write_result_atomic(&fixture.0, "result.json", &Fixture::record()).unwrap();

        assert_eq!(fs::read_to_string(old).unwrap(), "old");
        assert!(
            fs::read_to_string(&target)
                .unwrap()
                .contains("\"schemaVersion\": 1")
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn validates_early_then_writes_through_the_held_parent_descriptor() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("artifacts")).unwrap();
        let destination = validate_result_destination(&fixture.0, "artifacts/result.json").unwrap();
        write_validated_result_atomic(&destination, &Fixture::record()).unwrap();
        assert!(fixture.0.join("artifacts/result.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_escape_symlink_and_special_file_destinations_without_temp_residue() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = Fixture::new();
        symlink(outside.0.join("outside.json"), fixture.0.join("link.json")).unwrap();
        let socket = UnixListener::bind(fixture.0.join("socket.json")).unwrap();

        for path in [
            "../outside.json",
            "/tmp/outside.json",
            "link.json",
            "socket.json",
        ] {
            assert_eq!(
                write_result_atomic(&fixture.0, path, &Fixture::record()),
                Err(Reason::ManagedFileConflict)
            );
        }
        drop(socket);
        assert!(fs::read_dir(&fixture.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        }));
    }

    #[test]
    fn structured_record_matches_the_closed_schema_and_omits_sensitive_values() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join(".git")).unwrap();
        fs::create_dir_all(fixture.0.join("node_modules/vitest")).unwrap();
        fs::write(
            fixture.0.join("package.json"),
            r#"{"name":"safe","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        fs::write(
            fixture.0.join("package-lock.json"),
            r#"{"lockfileVersion":3}"#,
        )
        .unwrap();
        fs::write(
            fixture.0.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":300}}}"#,
        )
        .unwrap();
        fs::write(
            fixture.0.join("node_modules/vitest/package.json"),
            r#"{"name":"vitest","version":"4.1.11"}"#,
        )
        .unwrap();
        let secret_argument = "SECRET-ARGUMENT-VALUE";
        let plan = plan_run(
            &fixture.0,
            &RunSelection::root("test", vec![secret_argument.to_owned()]),
        )
        .unwrap();
        let details = RunResultDetails::from_plan(
            &plan,
            RunLifecycle {
                lock: LockState::Acquired,
                evidence_recheck: EvidenceRecheckState::Matched,
                spawn: SpawnState::Started,
            },
            None,
        );
        let record = RunResultRecord::at_unix_seconds(
            ExitResult::new(Origin::Child, 0, Reason::Completed),
            true,
            Some(details),
            951_782_400,
        )
        .unwrap();
        let value = serde_json::to_value(&record).unwrap();
        let serialized = serde_json::to_string(&record).unwrap();

        assert_eq!(value["timestamp"], "2000-02-29T00:00:00Z");
        assert_eq!(value["details"]["graphDepth"], 0);
        assert_eq!(value["details"]["leafCount"], 1);
        assert_eq!(
            value["details"]["appliedControls"],
            serde_json::json!(["suffix-injected"])
        );
        assert_eq!(value["details"]["forwardedArgumentCount"], 1);
        assert!(!serialized.contains(secret_argument));
        assert!(!serialized.contains("SECRET_ENVIRONMENT_VALUE"));
        assert!(!serialized.contains(fixture.0.to_str().unwrap()));
        assert!(!serialized.contains("vitest run"));
        assert!(!serialized.contains("packageName"));

        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schemas/result-v1.schema.json")).unwrap();
        assert_closed_object_matches(&schema, &value);
        assert_closed_object_matches(&schema["properties"]["details"], &value["details"]);
        assert_closed_object_matches(
            &schema["properties"]["details"]["properties"]["packageManager"],
            &value["details"]["packageManager"],
        );
        for evidence in value["details"]["evidence"].as_array().unwrap() {
            assert_closed_object_matches(
                &schema["properties"]["details"]["properties"]["evidence"]["items"],
                evidence,
            );
        }
        assert_eq!(
            schema["properties"]["details"]["properties"]["lockState"]["enum"],
            serde_json::json!(["not-acquired", "acquired"])
        );
        assert_eq!(
            schema["properties"]["details"]["properties"]["evidenceRecheckState"]["enum"],
            serde_json::json!(["not-run", "matched", "changed"])
        );
        assert_eq!(
            schema["properties"]["details"]["properties"]["spawnState"]["enum"],
            serde_json::json!(["not-attempted", "failed", "started"])
        );
        let details_properties = &schema["properties"]["details"]["properties"];
        assert_eq!(
            details_properties["operationKey"]["maxLength"],
            crate::configuration::MAX_KEY_BYTES
        );
        assert_eq!(
            details_properties["workspaceKey"]["maxLength"],
            crate::configuration::MAX_KEY_BYTES
        );
        assert_eq!(
            details_properties["timeoutSeconds"]["minimum"],
            crate::configuration::MIN_TIMEOUT_SECONDS
        );
        assert_eq!(
            details_properties["timeoutSeconds"]["maximum"],
            crate::configuration::MAX_TIMEOUT_SECONDS
        );
    }

    #[test]
    fn rejects_inconsistent_result_and_child_state() {
        assert_eq!(
            RunResultRecord::at_unix_seconds(
                ExitResult::new(Origin::Child, 0, Reason::Completed),
                false,
                None,
                0,
            ),
            Err(Reason::InternalError)
        );
    }

    fn assert_closed_object_matches(schema: &serde_json::Value, value: &serde_json::Value) {
        assert_eq!(schema["additionalProperties"], false);
        let properties = schema["properties"].as_object().unwrap();
        let required = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let actual = value.as_object().unwrap();
        assert!(required.iter().all(|key| actual.contains_key(*key)));
        assert!(actual.keys().all(|key| properties.contains_key(key)));
    }
}
