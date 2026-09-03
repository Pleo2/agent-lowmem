use crate::{
    agents_policy::{
        AgentsDocumentState, MAX_AGENTS_BYTES, inspect_agents, inspect_agents_forced,
        plan_agents_edit, render_policy_body,
    },
    atomic_file::{FilePrecondition, HeldDirectory, OptionalFile, read_optional_bounded},
    cli::{InitRequest, RestoreRequest},
    configuration::{
        AgentLowmemConfig, CANONICAL_OPERATIONS, MAX_CONFIG_WORKSPACES, OperationConfig,
        WorkspaceConfig, parse_config, valid_key, valid_package_name, valid_relative_path,
    },
    evidence::{EvidenceReader, EvidenceSnapshot},
    host::{HostSource, inspect_host},
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    policy::PolicyTarget,
    repository::{
        GitRepository, OperationStatus, PackageManagerKind, PackageManagerReport,
        analyze_managed_operation, find_git_repository, inspect_repository, repository_hash,
    },
    restoration::{
        AgentsRestoration, ConfigurationRestoration, DestinationAction, JournalState,
        MAX_CONFIGURATION_BYTES, MAX_MANIFEST_BYTES, ManagedSpan, OwnedBytes, Ownership,
        PriorManagedState, RecoveryClassification, RecoveryPlan, RestorationManifest,
        classify_prepared, parse_manifest, recover_prepared, serialize_manifest,
    },
    result::Reason,
    workspace::{expand_workspace_patterns, parse_npm_workspaces, parse_pnpm_workspace},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Write as _},
    fs::{File, symlink_metadata},
    io::ErrorKind,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const MANAGED_FILES_SCHEMA_VERSION: u8 = 1;
pub const MAX_MANUAL_CANDIDATES: usize = 256;

const CONFIGURATION_PATH: &str = ".agent-lowmem.json";
const AGENTS_PATH: &str = "AGENTS.md";
const JOURNAL_PATH: &str = "agent-lowmem/restoration-v1.json";

#[derive(Deserialize)]
struct PlannerManifest {
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

struct DiscoveredWorkspace {
    key: String,
    path: String,
    package_name: String,
    scripts: BTreeMap<String, String>,
}

#[derive(Clone, PartialEq, Eq)]
struct PlannedFile {
    ownership: Ownership,
    action: ManagedAction,
    before: OptionalFile,
    target: Option<Vec<u8>>,
    target_mode: Option<u32>,
}

#[derive(Clone, PartialEq, Eq)]
struct PlannedJournal {
    before: OptionalFile,
    prepared: RestorationManifest,
    applied: Option<RestorationManifest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedRequest {
    Init(InitRequest),
    Restore(RestoreRequest),
}

impl ManagedRequest {
    const fn dry_run(self) -> bool {
        match self {
            Self::Init(request) => request.dry_run,
            Self::Restore(request) => request.dry_run,
        }
    }

    const fn json(self) -> bool {
        match self {
            Self::Init(request) => request.json,
            Self::Restore(request) => request.json,
        }
    }
}

pub struct ManagedFilesPlan {
    command: ManagedCommand,
    request: ManagedRequest,
    root: GitRepository,
    repository_hash: [u8; 32],
    evidence: EvidenceSnapshot,
    configuration: PlannedFile,
    agents_policy: PlannedFile,
    journal: Option<PlannedJournal>,
    effective_config: Option<AgentLowmemConfig>,
    human_diff: Option<String>,
    report: ManagedFilesReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedFilesOutcome {
    pub report: ManagedFilesReport,
    pub human_diff: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaultPoint {
    PreparedDurable,
    ConfigurationWritten,
    AgentsWritten,
    TargetsVerified,
    AppliedJournalDurable,
    #[cfg(test)]
    Never,
}

pub(crate) trait TransactionFaults {
    fn fail_at(&self, point: FaultPoint) -> bool;
}

struct NoTransactionFaults;

impl TransactionFaults for NoTransactionFaults {
    fn fail_at(&self, _point: FaultPoint) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedRecovery {
    root: GitRepository,
    repository_hash: [u8; 32],
    plan: RecoveryPlan,
}

impl fmt::Debug for ManagedFilesPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedFilesPlan")
            .field("command", &self.command)
            .field("dry_run", &self.request.dry_run())
            .field("json", &self.request.json())
            .field("root_resolved", &self.root.root().is_absolute())
            .field("repository_sha256", &hex_digest(&self.repository_hash))
            .field("evidence_file_count", &self.evidence.files().len())
            .field("configuration_action", &self.configuration.action)
            .field("agents_policy_action", &self.agents_policy.action)
            .field(
                "journal_present",
                &self
                    .journal
                    .as_ref()
                    .is_some_and(|journal| optional_bytes(&journal.before).is_some()),
            )
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl ManagedFilesPlan {
    pub fn report(&self) -> &ManagedFilesReport {
        &self.report
    }

    pub fn effective_configuration(&self) -> &AgentLowmemConfig {
        self.effective_config
            .as_ref()
            .expect("only init plans expose an effective configuration")
    }

    pub fn evidence_files(&self) -> Vec<&str> {
        self.evidence
            .files()
            .iter()
            .map(|file| file.relative_path())
            .collect()
    }

    pub fn agents_target(&self) -> Option<&[u8]> {
        self.agents_policy.target.as_deref()
    }
}

pub fn render_managed_human(outcome: &ManagedFilesOutcome) -> String {
    let mut output = format!(
        "Managed files: {} ({})\n",
        outcome.report.command.as_str(),
        outcome.report.outcome.as_str()
    );
    if let Some(diff) = &outcome.human_diff {
        output.push('\n');
        output.push_str(diff);
    }
    if !outcome.report.issues.is_empty() {
        use std::fmt::Write as _;
        writeln!(
            output,
            "\nManual review required: {} managed-file issue(s).",
            outcome.report.issues.len()
        )
        .expect("writing to a String cannot fail");
    }
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedFilesFailure {
    reason: Reason,
    report: ManagedFilesReport,
}

impl ManagedFilesFailure {
    pub const fn reason(&self) -> Reason {
        self.reason
    }

    pub const fn report(&self) -> &ManagedFilesReport {
        &self.report
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFilesReport {
    pub schema_version: u8,
    pub command: ManagedCommand,
    pub dry_run: bool,
    pub outcome: ManagedOutcome,
    pub result: ManagedResult,
    pub files: Vec<ManagedFileReport>,
    pub operations: Vec<ManagedOperationReport>,
    pub manual_candidates: Vec<ManualCandidateReport>,
    pub issues: Vec<ManagedIssueReport>,
    pub manifest_state: ManifestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedCommand {
    Init,
    Restore,
}

impl ManagedCommand {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedOutcome {
    Planned,
    Applied,
    Restored,
    Unchanged,
    RecoveryRequired,
    Conflict,
    Failed,
}

impl ManagedOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Applied => "applied",
            Self::Restored => "restored",
            Self::Unchanged => "unchanged",
            Self::RecoveryRequired => "recovery-required",
            Self::Conflict => "conflict",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedIdentity {
    Configuration,
    AgentsPolicy,
    RestorationManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedAction {
    Create,
    Replace,
    Remove,
    Unchanged,
    Preserve,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManifestState {
    Absent,
    Prepared,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ManagedResult {
    pub code: i32,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFileReport {
    pub identity: ManagedIdentity,
    pub action: ManagedAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedOperationReport {
    pub operation_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualCandidateReport {
    pub operation_prefix: String,
    pub script_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedIssueReport {
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
}

impl ManagedResult {
    pub fn new(code: i32, reason: Reason) -> Result<Self, Reason> {
        let result = Self { code, reason };
        if result.is_valid() {
            Ok(result)
        } else {
            Err(Reason::InternalError)
        }
    }

    pub const fn is_valid(self) -> bool {
        match self.reason {
            Reason::Completed => self.code == 0,
            Reason::ManagedFileConflict => self.code == 0 || self.code == 78,
            Reason::InvalidCli | Reason::InvalidConfig => self.code == 2,
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
            | Reason::NonfinalInjectionRequired => self.code == 64,
            Reason::LockHeld | Reason::NestedInvocation => self.code == 73,
            Reason::EvidenceChanged => self.code == 75,
            Reason::InternalError => self.code == 70,
            Reason::ChildExit
            | Reason::ChildSignal
            | Reason::DeadlineExceeded
            | Reason::ExternalSignal => false,
        }
    }
}

impl ManagedFilesReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: ManagedCommand,
        dry_run: bool,
        outcome: ManagedOutcome,
        result: ManagedResult,
        mut files: Vec<ManagedFileReport>,
        mut operations: Vec<ManagedOperationReport>,
        mut manual_candidates: Vec<ManualCandidateReport>,
        mut issues: Vec<ManagedIssueReport>,
        manifest_state: ManifestState,
    ) -> Result<Self, Reason> {
        if !result.is_valid()
            || !valid_outcome(command, dry_run, outcome, result)
            || !files.iter().all(valid_file)
            || !operations.iter().all(valid_operation)
            || manual_candidates.len() > MAX_MANUAL_CANDIDATES
            || !manual_candidates.iter().all(valid_candidate)
            || !issues.iter().all(valid_issue)
        {
            return Err(Reason::InternalError);
        }

        files.sort_by_key(|file| identity_rank(file.identity));
        operations.sort_by(|left, right| {
            (&left.workspace_key, &left.operation_key)
                .cmp(&(&right.workspace_key, &right.operation_key))
        });
        manual_candidates.sort_by(|left, right| {
            (
                &left.operation_prefix,
                &left.script_name,
                &left.workspace_key,
            )
                .cmp(&(
                    &right.operation_prefix,
                    &right.script_name,
                    &right.workspace_key,
                ))
        });
        issues.sort_by(|left, right| {
            (
                left.reason.as_str(),
                &left.workspace_path,
                &left.package_name,
                &left.operation_key,
            )
                .cmp(&(
                    right.reason.as_str(),
                    &right.workspace_path,
                    &right.package_name,
                    &right.operation_key,
                ))
        });

        Ok(Self {
            schema_version: MANAGED_FILES_SCHEMA_VERSION,
            command,
            dry_run,
            outcome,
            result,
            files,
            operations,
            manual_candidates,
            issues,
            manifest_state,
        })
    }
}

const fn valid_outcome(
    command: ManagedCommand,
    dry_run: bool,
    outcome: ManagedOutcome,
    result: ManagedResult,
) -> bool {
    if matches!(
        (command, outcome),
        (ManagedCommand::Init, ManagedOutcome::Restored)
            | (ManagedCommand::Restore, ManagedOutcome::Applied)
    ) {
        return false;
    }

    match outcome {
        ManagedOutcome::Planned => {
            dry_run && result.code == 0 && matches!(result.reason, Reason::Completed)
        }
        ManagedOutcome::Applied | ManagedOutcome::Restored => {
            !dry_run && result.code == 0 && matches!(result.reason, Reason::Completed)
        }
        ManagedOutcome::Unchanged => result.code == 0 && matches!(result.reason, Reason::Completed),
        ManagedOutcome::RecoveryRequired => {
            dry_run && result.code == 0 && matches!(result.reason, Reason::ManagedFileConflict)
        }
        ManagedOutcome::Conflict => {
            result.code == 78 && matches!(result.reason, Reason::ManagedFileConflict)
        }
        ManagedOutcome::Failed => {
            result.code != 0 && !matches!(result.reason, Reason::ManagedFileConflict)
        }
    }
}

fn valid_file(file: &ManagedFileReport) -> bool {
    file.before_sha256.as_deref().is_none_or(valid_sha256)
        && file.target_sha256.as_deref().is_none_or(valid_sha256)
}

fn valid_operation(operation: &ManagedOperationReport) -> bool {
    valid_key(&operation.operation_key) && operation.workspace_key.as_deref().is_none_or(valid_key)
}

fn valid_candidate(candidate: &ManualCandidateReport) -> bool {
    matches!(
        candidate.operation_prefix.as_str(),
        "test" | "typecheck" | "lint" | "build"
    ) && candidate
        .script_name
        .strip_prefix(&candidate.operation_prefix)
        .is_some_and(|suffix| suffix.starts_with(':') && suffix.len() > 1)
        && !candidate.script_name.contains(['\0', '\n', '\r'])
        && candidate.workspace_key.as_deref().is_none_or(valid_key)
}

fn valid_issue(issue: &ManagedIssueReport) -> bool {
    phase_four_reason(issue.reason)
        && issue.operation_key.as_deref().is_none_or(valid_key)
        && issue
            .workspace_path
            .as_deref()
            .is_none_or(valid_relative_path)
        && issue.package_name.as_deref().is_none_or(valid_package_name)
}

const fn phase_four_reason(reason: Reason) -> bool {
    !matches!(
        reason,
        Reason::ChildExit | Reason::ChildSignal | Reason::DeadlineExceeded | Reason::ExternalSignal
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

const fn identity_rank(identity: ManagedIdentity) -> u8 {
    match identity {
        ManagedIdentity::Configuration => 0,
        ManagedIdentity::AgentsPolicy => 1,
        ManagedIdentity::RestorationManifest => 2,
    }
}

pub fn plan_init(
    source: &impl HostSource,
    start: &Path,
    request: &InitRequest,
) -> Result<ManagedFilesPlan, ManagedFilesFailure> {
    if !inspect_host(source).runtime_supported {
        return Err(planning_failure(
            *request,
            Reason::HostUnsupported,
            ManifestState::Absent,
        ));
    }
    plan_init_supported(start, request)
}

fn plan_init_supported(
    start: &Path,
    request: &InitRequest,
) -> Result<ManagedFilesPlan, ManagedFilesFailure> {
    plan_init_inner(start, *request)
        .map_err(|reason| planning_failure(*request, reason, inspect_manifest_state(start)))
}

pub fn plan_restore(
    start: &Path,
    request: &RestoreRequest,
) -> Result<ManagedFilesPlan, ManagedFilesFailure> {
    plan_restore_inner(start, *request)
        .map_err(|reason| restore_planning_failure(*request, reason, inspect_manifest_state(start)))
}

pub fn execute_restore(
    start: &Path,
    runtime: &Path,
    request: &RestoreRequest,
) -> ManagedFilesOutcome {
    execute_restore_core(start, runtime, request, || {}, &NoTransactionFaults)
}

pub fn execute_init(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &InitRequest,
) -> ManagedFilesOutcome {
    execute_init_core(source, start, runtime, request, || {}, &NoTransactionFaults)
}

fn execute_init_core(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &InitRequest,
    post_lock_hook: impl FnOnce(),
    faults: &impl TransactionFaults,
) -> ManagedFilesOutcome {
    if !inspect_host(source).runtime_supported {
        return ManagedFilesOutcome {
            report: planning_failure(*request, Reason::HostUnsupported, ManifestState::Absent)
                .report,
            human_diff: None,
        };
    }
    match inspect_prepared_recovery(start) {
        Ok(Some(_recovery)) if request.dry_run => {
            return recovery_required_outcome(*request);
        }
        Ok(Some(recovery_before)) => {
            let acquired_at = match unix_seconds_now() {
                Ok(value) => value,
                Err(reason) => {
                    return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
                }
            };
            let record = match ProcessIdentity::current().and_then(|owner| {
                LeaseRecord::new(owner, recovery_before.repository_hash, "init", acquired_at)
            }) {
                Ok(record) => record,
                Err(reason) => {
                    return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
                }
            };
            let _lease = match UserLease::acquire(runtime, record) {
                Ok(lease) => lease,
                Err(reason) => {
                    return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
                }
            };
            post_lock_hook();
            let recovery_after = match inspect_prepared_recovery(recovery_before.root.root()) {
                Ok(Some(recovery)) if recovery == recovery_before => recovery,
                Ok(_) | Err(_) => {
                    return recovery_failure_outcome(
                        *request,
                        Reason::EvidenceChanged,
                        ManifestState::Prepared,
                    );
                }
            };
            let repository = match HeldDirectory::open(recovery_after.root.root(), None) {
                Ok(directory) => directory,
                Err(reason) => {
                    return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
                }
            };
            let metadata = match HeldDirectory::open(recovery_after.root.metadata(), None) {
                Ok(directory) => directory,
                Err(reason) => {
                    return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
                }
            };
            if let Err(reason) = recover_prepared(&repository, &metadata, &recovery_after.plan) {
                return recovery_failure_outcome(*request, reason, ManifestState::Prepared);
            }
            let recovered_plan_before =
                match plan_init_supported(recovery_after.root.root(), request) {
                    Ok(plan) => plan,
                    Err(failure) => {
                        return ManagedFilesOutcome {
                            report: failure.report,
                            human_diff: None,
                        };
                    }
                };
            let recovered_plan =
                match plan_init_supported(recovered_plan_before.root.root(), request) {
                    Ok(plan) if managed_plans_match(&recovered_plan_before, &plan) => plan,
                    Ok(_) | Err(_) => {
                        return managed_failure_outcome(
                            &recovered_plan_before,
                            Reason::EvidenceChanged,
                        );
                    }
                };
            return apply_init_transaction(&recovered_plan, faults)
                .map(|()| ManagedFilesOutcome {
                    report: recovered_plan.report.clone(),
                    human_diff: recovered_plan.human_diff.clone(),
                })
                .unwrap_or_else(|reason| managed_failure_outcome(&recovered_plan, reason));
        }
        Ok(None) => {}
        Err(reason) => {
            return recovery_failure_outcome(*request, reason, inspect_manifest_state(start));
        }
    }

    let plan_before = match plan_init_supported(start, request) {
        Ok(plan) => plan,
        Err(failure) => {
            return ManagedFilesOutcome {
                report: failure.report,
                human_diff: None,
            };
        }
    };
    if request.dry_run {
        return ManagedFilesOutcome {
            report: plan_before.report.clone(),
            human_diff: plan_before.human_diff.clone(),
        };
    }

    let acquired_at = match unix_seconds_now() {
        Ok(value) => value,
        Err(reason) => return managed_failure_outcome(&plan_before, reason),
    };
    let record = match ProcessIdentity::current()
        .and_then(|owner| LeaseRecord::new(owner, plan_before.repository_hash, "init", acquired_at))
    {
        Ok(record) => record,
        Err(reason) => return managed_failure_outcome(&plan_before, reason),
    };
    let _lease = match UserLease::acquire(runtime, record) {
        Ok(lease) => lease,
        Err(reason) => return managed_failure_outcome(&plan_before, reason),
    };

    post_lock_hook();
    let plan_after = match plan_init_supported(plan_before.root.root(), request) {
        Ok(plan) if managed_plans_match(&plan_before, &plan) => plan,
        Ok(_) | Err(_) => return managed_failure_outcome(&plan_before, Reason::EvidenceChanged),
    };
    if previous_applied_matches(&plan_after) {
        return unchanged_outcome(&plan_after);
    }

    match apply_init_transaction(&plan_after, faults) {
        Ok(()) => ManagedFilesOutcome {
            report: plan_after.report.clone(),
            human_diff: plan_after.human_diff.clone(),
        },
        Err(reason) => managed_failure_outcome(&plan_after, reason),
    }
}

fn execute_restore_core(
    start: &Path,
    runtime: &Path,
    request: &RestoreRequest,
    post_lock_hook: impl FnOnce(),
    faults: &impl TransactionFaults,
) -> ManagedFilesOutcome {
    match inspect_prepared_recovery(start) {
        Ok(Some(_)) if request.dry_run => return restore_recovery_required_outcome(*request),
        Ok(Some(recovery_before)) => {
            let record = match restore_lease_record(&recovery_before.root, "restore") {
                Ok(record) => record,
                Err(reason) => {
                    return restore_recovery_failure_outcome(
                        *request,
                        reason,
                        ManifestState::Prepared,
                    );
                }
            };
            let _lease = match UserLease::acquire(runtime, record) {
                Ok(lease) => lease,
                Err(reason) => {
                    return restore_recovery_failure_outcome(
                        *request,
                        reason,
                        ManifestState::Prepared,
                    );
                }
            };
            post_lock_hook();
            let recovery_after = match inspect_prepared_recovery(recovery_before.root.root()) {
                Ok(Some(recovery)) if recovery == recovery_before => recovery,
                Ok(_) | Err(_) => {
                    return restore_recovery_failure_outcome(
                        *request,
                        Reason::EvidenceChanged,
                        ManifestState::Prepared,
                    );
                }
            };
            let repository = match HeldDirectory::open(recovery_after.root.root(), None) {
                Ok(directory) => directory,
                Err(reason) => {
                    return restore_recovery_failure_outcome(
                        *request,
                        reason,
                        ManifestState::Prepared,
                    );
                }
            };
            let metadata = match HeldDirectory::open(recovery_after.root.metadata(), None) {
                Ok(directory) => directory,
                Err(reason) => {
                    return restore_recovery_failure_outcome(
                        *request,
                        reason,
                        ManifestState::Prepared,
                    );
                }
            };
            if let Err(reason) = recover_prepared(&repository, &metadata, &recovery_after.plan) {
                return restore_recovery_failure_outcome(*request, reason, ManifestState::Prepared);
            }
            return execute_restore_locked(recovery_after.root.root(), request, faults);
        }
        Ok(None) => {}
        Err(reason) => {
            return restore_recovery_failure_outcome(
                *request,
                reason,
                inspect_manifest_state(start),
            );
        }
    }

    let plan_before = match plan_restore(start, request) {
        Ok(plan) => plan,
        Err(failure) => {
            return ManagedFilesOutcome {
                report: failure.report,
                human_diff: None,
            };
        }
    };
    if request.dry_run {
        return ManagedFilesOutcome {
            report: plan_before.report.clone(),
            human_diff: plan_before.human_diff.clone(),
        };
    }
    let record = match restore_lease_record(&plan_before.root, "restore") {
        Ok(record) => record,
        Err(reason) => return managed_failure_outcome(&plan_before, reason),
    };
    let _lease = match UserLease::acquire(runtime, record) {
        Ok(lease) => lease,
        Err(reason) => return managed_failure_outcome(&plan_before, reason),
    };
    post_lock_hook();
    let plan_after = match plan_restore(plan_before.root.root(), request) {
        Ok(plan) if managed_plans_match(&plan_before, &plan) => plan,
        Ok(_) | Err(_) => return managed_failure_outcome(&plan_before, Reason::EvidenceChanged),
    };
    if plan_is_unchanged(&plan_after) {
        return unchanged_outcome(&plan_after);
    }
    apply_restore_transaction(&plan_after, faults)
        .map(|()| ManagedFilesOutcome {
            report: plan_after.report.clone(),
            human_diff: plan_after.human_diff.clone(),
        })
        .unwrap_or_else(|reason| managed_failure_outcome(&plan_after, reason))
}

fn execute_restore_locked(
    start: &Path,
    request: &RestoreRequest,
    faults: &impl TransactionFaults,
) -> ManagedFilesOutcome {
    let before = match plan_restore(start, request) {
        Ok(plan) => plan,
        Err(failure) => {
            return ManagedFilesOutcome {
                report: failure.report,
                human_diff: None,
            };
        }
    };
    let after = match plan_restore(before.root.root(), request) {
        Ok(plan) if managed_plans_match(&before, &plan) => plan,
        Ok(_) | Err(_) => return managed_failure_outcome(&before, Reason::EvidenceChanged),
    };
    if plan_is_unchanged(&after) {
        return unchanged_outcome(&after);
    }
    apply_restore_transaction(&after, faults)
        .map(|()| ManagedFilesOutcome {
            report: after.report.clone(),
            human_diff: after.human_diff.clone(),
        })
        .unwrap_or_else(|reason| managed_failure_outcome(&after, reason))
}

fn restore_lease_record(root: &GitRepository, operation: &str) -> Result<LeaseRecord, Reason> {
    let acquired_at = unix_seconds_now()?;
    let owner = ProcessIdentity::current()?;
    LeaseRecord::new(owner, repository_hash(root.root()), operation, acquired_at)
}

fn plan_is_unchanged(plan: &ManagedFilesPlan) -> bool {
    plan.journal.is_none()
        && matches!(
            plan.configuration.action,
            ManagedAction::Unchanged | ManagedAction::Preserve
        )
        && matches!(
            plan.agents_policy.action,
            ManagedAction::Unchanged | ManagedAction::Preserve
        )
}

fn inspect_prepared_recovery(start: &Path) -> Result<Option<PreparedRecovery>, Reason> {
    let Some(root) = find_git_repository(start).map_err(|_| Reason::RepositoryUnsupported)? else {
        return Ok(None);
    };
    let metadata_file = File::open(root.metadata()).map_err(|_| Reason::RepositoryUnsupported)?;
    let discovered = read_optional_bounded(&metadata_file, JOURNAL_PATH, MAX_MANIFEST_BYTES)?;
    if optional_bytes(&discovered).is_none() {
        return Ok(None);
    }

    let metadata = HeldDirectory::open(root.metadata(), None)?;
    let private = HeldDirectory::open_child(&metadata, "agent-lowmem", Some(0o700))?;
    let journal = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    if !valid_private_journal(&journal) {
        return Err(Reason::ManagedFileConflict);
    }
    let manifest = parse_manifest(optional_bytes(&journal).ok_or(Reason::ManagedFileConflict)?)?;
    if manifest.state != JournalState::Prepared {
        return Ok(None);
    }
    let expected_hash = repository_hash(root.root());
    if manifest.repository_sha256 != hex_digest(&expected_hash) {
        return Err(Reason::ManagedFileConflict);
    }
    let repository = HeldDirectory::open(root.root(), None)?;
    match classify_prepared(&repository, &manifest)? {
        RecoveryClassification::Recoverable(plan) => Ok(Some(PreparedRecovery {
            root,
            repository_hash: expected_hash,
            plan,
        })),
        RecoveryClassification::Conflict => Err(Reason::ManagedFileConflict),
        RecoveryClassification::NotRequired => Ok(None),
    }
}

fn apply_init_transaction(
    plan: &ManagedFilesPlan,
    faults: &impl TransactionFaults,
) -> Result<(), Reason> {
    let journal = plan.journal.as_ref().ok_or(Reason::InternalError)?;
    let repository = HeldDirectory::open(plan.root.root(), None)?;
    let metadata = HeldDirectory::open(plan.root.metadata(), None)?;
    repository.ensure_replaceable(CONFIGURATION_PATH)?;
    repository.ensure_replaceable(AGENTS_PATH)?;
    if repository.precondition(CONFIGURATION_PATH)?
        != FilePrecondition::from(&plan.configuration.before)
        || repository.precondition(AGENTS_PATH)?
            != FilePrecondition::from(&plan.agents_policy.before)
    {
        return Err(Reason::EvidenceChanged);
    }

    let (private, private_created) =
        HeldDirectory::open_or_create_private_tracked(&metadata, "agent-lowmem", 0o700)?;
    let journal_before = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    if journal_before != journal.before {
        if private_created {
            drop(private);
            let _ = metadata.remove_empty_child("agent-lowmem");
        }
        return Err(Reason::EvidenceChanged);
    }
    if !valid_private_journal(&journal_before) {
        return Err(Reason::ManagedFileConflict);
    }

    let prepared_bytes = serialize_manifest(&journal.prepared)?;
    let applied_bytes = serialize_manifest(journal.applied.as_ref().ok_or(Reason::InternalError)?)?;
    let mut configuration_written = false;
    let mut agents_written = false;
    let mut journal_is_applied = false;
    let transaction = (|| {
        private.replace_atomic(
            "restoration-v1.json",
            &FilePrecondition::from(&journal_before),
            &prepared_bytes,
            0o600,
        )?;
        fault(faults, FaultPoint::PreparedDurable)?;

        configuration_written =
            apply_planned_file(&repository, CONFIGURATION_PATH, &plan.configuration)?;
        fault(faults, FaultPoint::ConfigurationWritten)?;
        agents_written = apply_planned_file(&repository, AGENTS_PATH, &plan.agents_policy)?;
        fault(faults, FaultPoint::AgentsWritten)?;

        verify_planned_file(&repository, CONFIGURATION_PATH, &plan.configuration)?;
        verify_planned_file(&repository, AGENTS_PATH, &plan.agents_policy)?;
        fault(faults, FaultPoint::TargetsVerified)?;

        let prepared = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
        if optional_bytes(&prepared) != Some(prepared_bytes.as_slice()) {
            return Err(Reason::ManagedFileConflict);
        }
        private.replace_atomic(
            "restoration-v1.json",
            &FilePrecondition::from(&prepared),
            &applied_bytes,
            0o600,
        )?;
        journal_is_applied = true;
        fault(faults, FaultPoint::AppliedJournalDurable)
    })();

    if let Err(reason) = transaction {
        let rolled_back = rollback_init(
            plan,
            &repository,
            &private,
            configuration_written,
            agents_written,
            journal_is_applied,
        )
        .is_ok();
        drop(private);
        if rolled_back && private_created && metadata.remove_empty_child("agent-lowmem").is_err() {
            return Err(Reason::InternalError);
        }
        return if rolled_back {
            Err(reason)
        } else {
            Err(Reason::InternalError)
        };
    }
    Ok(())
}

fn apply_restore_transaction(
    plan: &ManagedFilesPlan,
    faults: &impl TransactionFaults,
) -> Result<(), Reason> {
    let journal = plan.journal.as_ref().ok_or(Reason::InternalError)?;
    let repository = HeldDirectory::open(plan.root.root(), None)?;
    let metadata = HeldDirectory::open(plan.root.metadata(), None)?;
    repository.ensure_replaceable(CONFIGURATION_PATH)?;
    repository.ensure_replaceable(AGENTS_PATH)?;
    if repository.precondition(CONFIGURATION_PATH)?
        != FilePrecondition::from(&plan.configuration.before)
        || repository.precondition(AGENTS_PATH)?
            != FilePrecondition::from(&plan.agents_policy.before)
    {
        return Err(Reason::EvidenceChanged);
    }

    let (private, private_created) =
        HeldDirectory::open_or_create_private_tracked(&metadata, "agent-lowmem", 0o700)?;
    let journal_before = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    if journal_before != journal.before {
        if private_created {
            drop(private);
            let _ = metadata.remove_empty_child("agent-lowmem");
        }
        return Err(Reason::EvidenceChanged);
    }
    if !valid_private_journal(&journal_before) {
        return Err(Reason::ManagedFileConflict);
    }

    let prepared_bytes = serialize_manifest(&journal.prepared)?;
    let mut configuration_written = false;
    let mut agents_written = false;
    let transaction = (|| {
        private.replace_atomic(
            "restoration-v1.json",
            &FilePrecondition::from(&journal_before),
            &prepared_bytes,
            0o600,
        )?;
        fault(faults, FaultPoint::PreparedDurable)?;

        configuration_written =
            apply_planned_file(&repository, CONFIGURATION_PATH, &plan.configuration)?;
        fault(faults, FaultPoint::ConfigurationWritten)?;
        agents_written = apply_planned_file(&repository, AGENTS_PATH, &plan.agents_policy)?;
        fault(faults, FaultPoint::AgentsWritten)?;

        verify_planned_file(&repository, CONFIGURATION_PATH, &plan.configuration)?;
        verify_planned_file(&repository, AGENTS_PATH, &plan.agents_policy)?;
        fault(faults, FaultPoint::TargetsVerified)?;

        let current = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
        if optional_bytes(&current) != Some(prepared_bytes.as_slice()) {
            return Err(Reason::ManagedFileConflict);
        }
        private.remove_exact("restoration-v1.json", &FilePrecondition::from(&current))
    })();

    if let Err(reason) = transaction {
        let rolled_back = rollback_restore(
            plan,
            &repository,
            &private,
            configuration_written,
            agents_written,
        )
        .is_ok();
        drop(private);
        if rolled_back && private_created {
            let _ = metadata.remove_empty_child("agent-lowmem");
        }
        return if rolled_back {
            Err(reason)
        } else {
            Err(Reason::InternalError)
        };
    }

    drop(private);
    let _ = metadata.remove_empty_child("agent-lowmem");
    Ok(())
}

fn rollback_restore(
    plan: &ManagedFilesPlan,
    repository: &HeldDirectory,
    private: &HeldDirectory,
    configuration_written: bool,
    agents_written: bool,
) -> Result<(), Reason> {
    let journal = plan.journal.as_ref().ok_or(Reason::InternalError)?;
    if agents_written {
        rollback_planned_file(repository, AGENTS_PATH, &plan.agents_policy)?;
    }
    if configuration_written {
        rollback_planned_file(repository, CONFIGURATION_PATH, &plan.configuration)?;
    }
    let current = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    let prepared = serialize_manifest(&journal.prepared)?;
    if optional_bytes(&current) != Some(prepared.as_slice()) {
        return Err(Reason::InternalError);
    }
    restore_optional_file(private, "restoration-v1.json", &current, &journal.before)
}

fn apply_planned_file(
    directory: &HeldDirectory,
    name: &str,
    file: &PlannedFile,
) -> Result<bool, Reason> {
    if matches!(
        file.action,
        ManagedAction::Unchanged | ManagedAction::Preserve
    ) {
        return Ok(false);
    }
    match file.target.as_deref() {
        Some(target) => {
            let mode = file.target_mode.ok_or(Reason::InternalError)?;
            directory.replace_atomic(name, &FilePrecondition::from(&file.before), target, mode)?;
        }
        None if file.action == ManagedAction::Remove => {
            directory.remove_exact(name, &FilePrecondition::from(&file.before))?;
        }
        None => return Err(Reason::InternalError),
    }
    Ok(true)
}

fn verify_planned_file(
    directory: &HeldDirectory,
    name: &str,
    file: &PlannedFile,
) -> Result<(), Reason> {
    let current = directory.read_optional(
        name,
        if name == AGENTS_PATH {
            MAX_AGENTS_BYTES
        } else {
            MAX_CONFIGURATION_BYTES
        },
    )?;
    if matches!(file.action, ManagedAction::Preserve) {
        return (current == file.before)
            .then_some(())
            .ok_or(Reason::ManagedFileConflict);
    }
    optional_matches_target(&current, file)
        .then_some(())
        .ok_or(Reason::ManagedFileConflict)
}

fn rollback_init(
    plan: &ManagedFilesPlan,
    repository: &HeldDirectory,
    private: &HeldDirectory,
    configuration_written: bool,
    agents_written: bool,
    journal_is_applied: bool,
) -> Result<(), Reason> {
    let journal = plan.journal.as_ref().ok_or(Reason::InternalError)?;
    if journal_is_applied {
        let current = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
        let applied = serialize_manifest(journal.applied.as_ref().ok_or(Reason::InternalError)?)?;
        if optional_bytes(&current) != Some(applied.as_slice()) {
            return Err(Reason::InternalError);
        }
        private.replace_atomic(
            "restoration-v1.json",
            &FilePrecondition::from(&current),
            &serialize_manifest(&journal.prepared)?,
            0o600,
        )?;
    }
    if agents_written {
        rollback_planned_file(repository, AGENTS_PATH, &plan.agents_policy)?;
    }
    if configuration_written {
        rollback_planned_file(repository, CONFIGURATION_PATH, &plan.configuration)?;
    }
    let current = private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?;
    restore_optional_file(private, "restoration-v1.json", &current, &journal.before)
}

fn rollback_planned_file(
    directory: &HeldDirectory,
    name: &str,
    file: &PlannedFile,
) -> Result<(), Reason> {
    let limit = if name == AGENTS_PATH {
        MAX_AGENTS_BYTES
    } else {
        MAX_CONFIGURATION_BYTES
    };
    let current = directory.read_optional(name, limit)?;
    if !optional_matches_target(&current, file) {
        return Err(Reason::InternalError);
    }
    restore_optional_file(directory, name, &current, &file.before)
}

fn restore_optional_file(
    directory: &HeldDirectory,
    name: &str,
    current: &OptionalFile,
    before: &OptionalFile,
) -> Result<(), Reason> {
    match before {
        OptionalFile::Absent => directory.remove_exact(name, &FilePrecondition::from(current)),
        OptionalFile::Regular { bytes, mode, .. } => {
            directory.replace_atomic(name, &FilePrecondition::from(current), bytes, *mode)
        }
    }
}

fn optional_matches_target(current: &OptionalFile, file: &PlannedFile) -> bool {
    match (current, file.target.as_deref(), file.target_mode) {
        (OptionalFile::Absent, None, None) => true,
        (OptionalFile::Regular { bytes, mode, .. }, Some(target), Some(target_mode)) => {
            bytes == target && *mode == target_mode
        }
        _ => false,
    }
}

fn fault(faults: &impl TransactionFaults, point: FaultPoint) -> Result<(), Reason> {
    if faults.fail_at(point) {
        Err(Reason::InternalError)
    } else {
        Ok(())
    }
}

fn plan_restore_inner(start: &Path, request: RestoreRequest) -> Result<ManagedFilesPlan, Reason> {
    let root = find_git_repository(start)
        .map_err(|_| Reason::RepositoryUnsupported)?
        .ok_or(Reason::RepositoryUnsupported)?;
    let repository = HeldDirectory::open(root.root(), None)?;
    let configuration_before =
        repository.read_optional(CONFIGURATION_PATH, MAX_CONFIGURATION_BYTES)?;
    let agents_before = repository.read_optional(AGENTS_PATH, MAX_AGENTS_BYTES)?;
    let metadata = HeldDirectory::open(root.metadata(), None)?;
    let metadata_file = File::open(root.metadata()).map_err(|_| Reason::RepositoryUnsupported)?;
    let discovered = read_optional_bounded(&metadata_file, JOURNAL_PATH, MAX_MANIFEST_BYTES)?;
    let journal_before = match HeldDirectory::open_child(&metadata, "agent-lowmem", Some(0o700)) {
        Ok(private) => private.read_optional("restoration-v1.json", MAX_MANIFEST_BYTES)?,
        Err(_reason)
            if optional_bytes(&discovered).is_none()
                && symlink_metadata(root.metadata().join("agent-lowmem"))
                    .is_err_and(|error| error.kind() == ErrorKind::NotFound) =>
        {
            OptionalFile::Absent
        }
        Err(reason) => return Err(reason),
    };
    if !valid_private_journal(&journal_before) {
        return Err(Reason::ManagedFileConflict);
    }
    let applied = optional_bytes(&journal_before)
        .map(parse_manifest)
        .transpose()?;
    if applied
        .as_ref()
        .is_some_and(|manifest| manifest.state != JournalState::Applied)
    {
        return Err(Reason::ManagedFileConflict);
    }
    let repository_digest = repository_hash(root.root());
    if applied
        .as_ref()
        .is_some_and(|manifest| manifest.repository_sha256 != hex_digest(&repository_digest))
    {
        return Err(Reason::ManagedFileConflict);
    }

    let mut issues = Vec::new();
    let (configuration, agents_policy, restore_manifest, manifest_state) = match applied {
        Some(applied) => {
            let configuration = plan_journaled_configuration_restore(
                &configuration_before,
                &applied.configuration,
            )?;
            let (agents_policy, agents_restoration) = plan_agents_restore(
                &agents_before,
                &applied.agents_policy,
                request.force_managed_block,
            )?;
            let configuration_restoration =
                restore_configuration_manifest(&configuration, &applied.configuration)?;
            let prepared = RestorationManifest::new(
                JournalState::Prepared,
                hex_digest(&repository_digest),
                configuration_restoration,
                agents_restoration,
                Some(Box::new(without_previous(applied)?)),
            )?;
            (
                configuration,
                agents_policy,
                Some(PlannedJournal {
                    before: journal_before.clone(),
                    prepared,
                    applied: None,
                }),
                ManifestState::Applied,
            )
        }
        None => {
            let configuration = plan_fresh_configuration_restore(
                root.root(),
                configuration_before.clone(),
                &mut issues,
            );
            let (agents_policy, agents_restoration) =
                plan_fresh_agents_restore(&agents_before, request.force_managed_block)?;
            let needs_write = is_mutating_action(configuration.action)
                || is_mutating_action(agents_policy.action);
            let journal = if needs_write {
                let agents_restoration = agents_restoration.ok_or(Reason::ManagedFileConflict)?;
                let configuration_restoration = fresh_configuration_manifest(&configuration)?;
                Some(PlannedJournal {
                    before: journal_before.clone(),
                    prepared: RestorationManifest::new(
                        JournalState::Prepared,
                        hex_digest(&repository_digest),
                        configuration_restoration,
                        agents_restoration,
                        None,
                    )?,
                    applied: None,
                })
            } else {
                None
            };
            (configuration, agents_policy, journal, ManifestState::Absent)
        }
    };

    let mut files = vec![
        file_report(ManagedIdentity::Configuration, &configuration),
        file_report(ManagedIdentity::AgentsPolicy, &agents_policy),
    ];
    if restore_manifest.is_some() || optional_bytes(&journal_before).is_some() {
        files.push(ManagedFileReport {
            identity: ManagedIdentity::RestorationManifest,
            action: ManagedAction::Remove,
            before_sha256: optional_digest(&journal_before),
            target_sha256: None,
        });
    }
    let unchanged = restore_manifest.is_none()
        && !is_mutating_action(configuration.action)
        && !is_mutating_action(agents_policy.action);
    let outcome = if unchanged {
        ManagedOutcome::Unchanged
    } else if request.dry_run {
        ManagedOutcome::Planned
    } else {
        ManagedOutcome::Restored
    };
    let human_diff = render_plan_diff(
        ManagedCommand::Restore,
        &configuration,
        &agents_policy,
        restore_manifest.as_ref(),
    )?;
    let report = ManagedFilesReport::new(
        ManagedCommand::Restore,
        request.dry_run,
        outcome,
        ManagedResult::new(0, Reason::Completed)?,
        files,
        Vec::new(),
        Vec::new(),
        issues,
        manifest_state,
    )?;
    Ok(ManagedFilesPlan {
        command: ManagedCommand::Restore,
        request: ManagedRequest::Restore(request),
        root,
        repository_hash: repository_digest,
        evidence: EvidenceSnapshot::new(Vec::new())?,
        configuration,
        agents_policy,
        journal: restore_manifest,
        effective_config: None,
        human_diff,
        report,
    })
}

fn is_mutating_action(action: ManagedAction) -> bool {
    matches!(
        action,
        ManagedAction::Create | ManagedAction::Replace | ManagedAction::Remove
    )
}

fn plan_journaled_configuration_restore(
    current: &OptionalFile,
    recorded: &ConfigurationRestoration,
) -> Result<PlannedFile, Reason> {
    if recorded.ownership == Ownership::External {
        let matches = optional_mode(current).map(|mode| mode as u16) == recorded.before_mode
            && optional_digest(current).as_deref() == recorded.external_sha256.as_deref();
        return matches
            .then(|| PlannedFile {
                ownership: Ownership::External,
                action: ManagedAction::Preserve,
                before: current.clone(),
                target: None,
                target_mode: None,
            })
            .ok_or(Reason::ManagedFileConflict);
    }
    if !optional_matches_owned(current, recorded.target.as_ref(), recorded.target_mode) {
        return Err(Reason::ManagedFileConflict);
    }
    let baseline = recorded
        .stable_baseline
        .as_ref()
        .ok_or(Reason::ManagedFileConflict)?;
    let (target, target_mode) = match baseline {
        PriorManagedState::Absent => (None, None),
        PriorManagedState::Bytes(bytes) => (
            Some(bytes.bytes.clone()),
            Some(u32::from(
                recorded.before_mode.ok_or(Reason::ManagedFileConflict)?,
            )),
        ),
    };
    Ok(restore_planned_file(
        Ownership::Managed,
        current.clone(),
        target,
        target_mode,
    ))
}

fn plan_agents_restore(
    current: &OptionalFile,
    recorded: &AgentsRestoration,
    force: bool,
) -> Result<(PlannedFile, AgentsRestoration), Reason> {
    let OptionalFile::Regular { bytes, mode, .. } = current else {
        return Err(Reason::ManagedFileConflict);
    };
    if Some(*mode as u16) != recorded.target_mode {
        return Err(Reason::ManagedFileConflict);
    }
    let state = if force {
        inspect_agents_forced(Some(bytes.clone()))?
    } else {
        inspect_agents(Some(bytes.clone()))?
    };
    let AgentsDocumentState::OneBlock(block) = state else {
        return Err(Reason::ManagedFileConflict);
    };
    let expected = recorded
        .target
        .as_ref()
        .ok_or(Reason::ManagedFileConflict)?;
    let expected_block = expected
        .bytes
        .strip_prefix(recorded.inserted_separator.as_slice())
        .unwrap_or(expected.bytes.as_slice());
    if !force && block.managed_bytes() != expected_block {
        return Err(Reason::ManagedFileConflict);
    }
    let owned_start = block
        .span
        .start
        .checked_sub(recorded.inserted_separator.len())
        .ok_or(Reason::ManagedFileConflict)?;
    if bytes.get(owned_start..block.span.start) != Some(recorded.inserted_separator.as_slice()) {
        return Err(Reason::ManagedFileConflict);
    }
    let mut target = Vec::with_capacity(bytes.len() - (block.span.end - owned_start));
    target.extend_from_slice(&bytes[..owned_start]);
    target.extend_from_slice(&bytes[block.span.end..]);
    let delete_document = target.is_empty() && recorded.document_was_absent;
    let target_bytes = (!delete_document).then_some(target.clone());
    let planned = restore_planned_file(
        Ownership::Managed,
        current.clone(),
        target_bytes,
        (!delete_document).then_some(*mode),
    );
    let immediate = OwnedBytes::new(block.managed_bytes().to_vec())?;
    let restoration = AgentsRestoration {
        ownership: Ownership::Managed,
        action: DestinationAction::Remove,
        document_was_absent: delete_document,
        immediate_before: PriorManagedState::Bytes(immediate),
        target: None,
        stable_baseline: recorded.stable_baseline.clone(),
        before_mode: Some(*mode as u16),
        target_mode: None,
        managed_span: ManagedSpan {
            start: u32::try_from(block.span.start).map_err(|_| Reason::ManagedFileConflict)?,
            end: u32::try_from(block.span.end).map_err(|_| Reason::ManagedFileConflict)?,
        },
        inserted_separator: recorded.inserted_separator.clone(),
        prefix_sha256: digest_bytes(&bytes[..owned_start]),
        suffix_sha256: digest_bytes(&bytes[block.span.end..]),
    };
    Ok((planned, restoration))
}

fn plan_fresh_configuration_restore(
    root: &Path,
    current: OptionalFile,
    issues: &mut Vec<ManagedIssueReport>,
) -> PlannedFile {
    let Some(bytes) = optional_bytes(&current) else {
        return restore_planned_file(Ownership::Managed, current, None, None);
    };
    let reproducible = reproduce_configuration(root).is_some_and(|generated| generated == bytes);
    if reproducible {
        restore_planned_file(Ownership::Managed, current, None, None)
    } else {
        issues.push(ManagedIssueReport {
            reason: Reason::ManagedFileConflict,
            operation_key: None,
            workspace_path: None,
            package_name: None,
        });
        PlannedFile {
            ownership: Ownership::External,
            action: ManagedAction::Preserve,
            before: current,
            target: None,
            target_mode: None,
        }
    }
}

fn reproduce_configuration(root: &Path) -> Option<Vec<u8>> {
    let report = inspect_repository(root);
    if report.failure_reason.is_some() {
        return None;
    }
    let package_manager = report.package_manager.as_ref()?;
    let directory = File::open(root).ok()?;
    let package =
        read_optional_bounded(&directory, "package.json", MAX_CONFIGURATION_BYTES).ok()?;
    let package_bytes = optional_bytes(&package)?;
    let manifest: PlannerManifest = serde_json::from_slice(package_bytes).ok()?;
    let (configuration, _, _, _, _) =
        generate_configuration(root, package_manager, package_bytes, &manifest.scripts).ok()?;
    configuration
        .has_operations()
        .then(|| configuration.deterministic_bytes().ok())
        .flatten()
}

fn plan_fresh_agents_restore(
    current: &OptionalFile,
    force: bool,
) -> Result<(PlannedFile, Option<AgentsRestoration>), Reason> {
    let OptionalFile::Regular { bytes, mode, .. } = current else {
        return Ok((
            restore_planned_file(Ownership::Managed, current.clone(), None, None),
            Some(no_block_agents_restoration(&[], None, true)?),
        ));
    };
    let state = if force {
        inspect_agents_forced(Some(bytes.clone()))?
    } else {
        inspect_agents(Some(bytes.clone()))?
    };
    let AgentsDocumentState::OneBlock(block) = state else {
        return Ok((
            PlannedFile {
                ownership: Ownership::External,
                action: ManagedAction::Preserve,
                before: current.clone(),
                target: None,
                target_mode: None,
            },
            Some(no_block_agents_restoration(bytes, Some(*mode), false)?),
        ));
    };
    let mut target = Vec::with_capacity(bytes.len() - block.span.len());
    target.extend_from_slice(&bytes[..block.span.start]);
    target.extend_from_slice(&bytes[block.span.end..]);
    let delete_document = target.is_empty();
    let planned = restore_planned_file(
        Ownership::Managed,
        current.clone(),
        (!delete_document).then_some(target),
        (!delete_document).then_some(*mode),
    );
    let restoration = AgentsRestoration {
        ownership: Ownership::Managed,
        action: DestinationAction::Remove,
        document_was_absent: delete_document,
        immediate_before: PriorManagedState::Bytes(OwnedBytes::new(
            block.managed_bytes().to_vec(),
        )?),
        target: None,
        stable_baseline: PriorManagedState::Absent,
        before_mode: Some(*mode as u16),
        target_mode: None,
        managed_span: ManagedSpan {
            start: u32::try_from(block.span.start).map_err(|_| Reason::ManagedFileConflict)?,
            end: u32::try_from(block.span.end).map_err(|_| Reason::ManagedFileConflict)?,
        },
        inserted_separator: Vec::new(),
        prefix_sha256: digest_bytes(&bytes[..block.span.start]),
        suffix_sha256: digest_bytes(&bytes[block.span.end..]),
    };
    Ok((planned, Some(restoration)))
}

fn no_block_agents_restoration(
    bytes: &[u8],
    mode: Option<u32>,
    document_was_absent: bool,
) -> Result<AgentsRestoration, Reason> {
    let position = u32::try_from(bytes.len()).map_err(|_| Reason::ManagedFileConflict)?;
    Ok(AgentsRestoration {
        ownership: Ownership::Managed,
        action: DestinationAction::Unchanged,
        document_was_absent,
        immediate_before: PriorManagedState::Absent,
        target: None,
        stable_baseline: PriorManagedState::Absent,
        before_mode: mode.map(|mode| mode as u16),
        target_mode: mode.map(|mode| mode as u16),
        managed_span: ManagedSpan {
            start: position,
            end: position,
        },
        inserted_separator: Vec::new(),
        prefix_sha256: digest_bytes(bytes),
        suffix_sha256: digest_bytes(&[]),
    })
}

fn restore_configuration_manifest(
    file: &PlannedFile,
    recorded: &ConfigurationRestoration,
) -> Result<ConfigurationRestoration, Reason> {
    if file.ownership == Ownership::External {
        return Ok(recorded.clone());
    }
    Ok(ConfigurationRestoration {
        ownership: Ownership::Managed,
        action: destination_action(file.action),
        immediate_before: Some(prior_state(&file.before)?),
        target: file.target.clone().map(OwnedBytes::new).transpose()?,
        stable_baseline: recorded.stable_baseline.clone(),
        before_mode: optional_mode(&file.before).map(|mode| mode as u16),
        target_mode: file.target_mode.map(|mode| mode as u16),
        external_sha256: None,
    })
}

fn fresh_configuration_manifest(file: &PlannedFile) -> Result<ConfigurationRestoration, Reason> {
    if file.ownership == Ownership::External {
        return Ok(ConfigurationRestoration {
            ownership: Ownership::External,
            action: DestinationAction::Preserve,
            immediate_before: None,
            target: None,
            stable_baseline: None,
            before_mode: optional_mode(&file.before).map(|mode| mode as u16),
            target_mode: None,
            external_sha256: optional_digest(&file.before),
        });
    }
    Ok(ConfigurationRestoration {
        ownership: Ownership::Managed,
        action: destination_action(file.action),
        immediate_before: Some(prior_state(&file.before)?),
        target: file.target.clone().map(OwnedBytes::new).transpose()?,
        stable_baseline: Some(PriorManagedState::Absent),
        before_mode: optional_mode(&file.before).map(|mode| mode as u16),
        target_mode: file.target_mode.map(|mode| mode as u16),
        external_sha256: None,
    })
}

fn optional_matches_owned(
    current: &OptionalFile,
    target: Option<&OwnedBytes>,
    mode: Option<u16>,
) -> bool {
    match (current, target) {
        (OptionalFile::Absent, None) => mode.is_none(),
        (
            OptionalFile::Regular {
                bytes,
                mode: actual,
                ..
            },
            Some(target),
        ) => bytes == &target.bytes && Some(*actual as u16) == mode,
        _ => false,
    }
}

fn restore_planned_file(
    ownership: Ownership,
    before: OptionalFile,
    target: Option<Vec<u8>>,
    target_mode: Option<u32>,
) -> PlannedFile {
    let action = match (optional_bytes(&before), target.as_deref()) {
        (None, None) => ManagedAction::Unchanged,
        (Some(current), Some(next)) if current == next => ManagedAction::Unchanged,
        (Some(_), Some(_)) => ManagedAction::Replace,
        (Some(_), None) => ManagedAction::Remove,
        (None, Some(_)) => ManagedAction::Create,
    };
    PlannedFile {
        ownership,
        action,
        before,
        target,
        target_mode,
    }
}

fn plan_init_inner(start: &Path, request: InitRequest) -> Result<ManagedFilesPlan, Reason> {
    let root = find_git_repository(start)
        .map_err(|_| Reason::RepositoryUnsupported)?
        .ok_or(Reason::RepositoryUnsupported)?;
    let root_directory = File::open(root.root()).map_err(|_| Reason::RepositoryUnsupported)?;
    let config_before =
        read_optional_bounded(&root_directory, CONFIGURATION_PATH, MAX_CONFIGURATION_BYTES)?;
    let agents_before = read_optional_bounded(&root_directory, AGENTS_PATH, MAX_AGENTS_BYTES)?;
    let metadata_directory =
        File::open(root.metadata()).map_err(|_| Reason::RepositoryUnsupported)?;
    let journal_before =
        read_optional_bounded(&metadata_directory, JOURNAL_PATH, MAX_MANIFEST_BYTES)?;
    let previous_applied = parse_previous_manifest(&journal_before)?;
    let manifest_state = if previous_applied.is_some() {
        ManifestState::Applied
    } else {
        ManifestState::Absent
    };

    let repository_report = inspect_repository(start);
    let package_manager = repository_report.package_manager.clone().ok_or(
        repository_report
            .failure_reason
            .unwrap_or(Reason::RepositoryUnsupported),
    )?;
    if let Some(reason) = repository_report.failure_reason {
        return Err(reason);
    }
    let package_state =
        read_optional_bounded(&root_directory, "package.json", MAX_CONFIGURATION_BYTES)?;
    let package_bytes = optional_bytes(&package_state).ok_or(Reason::RepositoryUnsupported)?;
    let manifest: PlannerManifest =
        serde_json::from_slice(package_bytes).map_err(|_| Reason::RepositoryUnsupported)?;
    let (generated, mut operations, mut candidates, mut issues, mut evidence_paths) =
        generate_configuration(
            root.root(),
            &package_manager,
            package_bytes,
            &manifest.scripts,
        )?;

    let (effective_config, configuration) = match optional_bytes(&config_before) {
        None => {
            if !generated.has_operations() {
                return Err(Reason::OperationUnsupported);
            }
            let target = generated.deterministic_bytes()?;
            (
                generated,
                planned_file(
                    Ownership::Managed,
                    config_before.clone(),
                    Some(target),
                    0o600,
                ),
            )
        }
        Some(bytes) => {
            let parsed = parse_config(bytes).map_err(|error| error.reason())?;
            if parsed.package_manager != package_manager.kind {
                return Err(Reason::InvalidConfig);
            }
            let generated_bytes = generated.deterministic_bytes()?;
            let ownership = previous_applied
                .as_ref()
                .map(|previous| previous.configuration.ownership)
                .unwrap_or_else(|| {
                    if bytes == generated_bytes {
                        Ownership::Managed
                    } else {
                        Ownership::External
                    }
                });
            if ownership == Ownership::Managed {
                if !generated.has_operations() {
                    return Err(Reason::OperationUnsupported);
                }
                if let Some(previous) = &previous_applied {
                    let expected = previous
                        .configuration
                        .target
                        .as_ref()
                        .map(|target| target.bytes.as_slice())
                        .ok_or(Reason::ManagedFileConflict)?;
                    if bytes != expected {
                        return Err(Reason::ManagedFileConflict);
                    }
                }
                (
                    generated,
                    planned_file(
                        ownership,
                        config_before.clone(),
                        Some(generated_bytes),
                        0o600,
                    ),
                )
            } else {
                if let Some(rejected) = repository_report
                    .operations
                    .iter()
                    .find(|operation| operation.status == OperationStatus::Rejected)
                {
                    return Err(rejected.reason.unwrap_or(Reason::OperationUnsupported));
                }
                operations = repository_report
                    .operations
                    .iter()
                    .map(operation_report)
                    .collect();
                evidence_paths.extend(
                    repository_report
                        .operations
                        .iter()
                        .flat_map(|operation| operation.evidence_files.iter().cloned()),
                );
                candidates.clear();
                issues.clear();
                (
                    parsed,
                    planned_file(ownership, config_before.clone(), None, 0o600),
                )
            }
        }
    };

    let body = render_policy_body(&effective_config)?;
    let agents_state = inspect_agents(optional_bytes(&agents_before).map(ToOwned::to_owned))?;
    validate_previous_agents(previous_applied.as_ref(), &agents_state)?;
    let current_managed = managed_block_bytes(&agents_state);
    let document_was_absent = matches!(agents_state, AgentsDocumentState::Absent);
    let agents_edit = plan_agents_edit(agents_state, &body)?;
    let agents_policy = planned_file(
        Ownership::Managed,
        agents_before.clone(),
        Some(agents_edit.target_bytes.clone()),
        0o600,
    );

    evidence_paths.extend([
        "package.json".to_owned(),
        selected_lock(package_manager.kind).to_owned(),
    ]);
    if optional_bytes(&config_before).is_some() {
        evidence_paths.push(CONFIGURATION_PATH.to_owned());
    }
    let evidence = evidence_snapshot(root.root(), evidence_paths)?;
    let repository_digest = repository_hash(root.root());
    let repository_sha256 = hex_digest(&repository_digest);
    let config_restoration = configuration_restoration(
        &configuration,
        previous_applied
            .as_ref()
            .map(|previous| &previous.configuration),
    )?;
    let agents_restoration = agents_restoration(
        &agents_before,
        current_managed,
        &agents_edit,
        document_was_absent,
        previous_applied
            .as_ref()
            .map(|previous| &previous.agents_policy),
    )?;
    let previous = previous_applied.map(without_previous).transpose()?;
    let prepared = RestorationManifest::new(
        JournalState::Prepared,
        repository_sha256.clone(),
        config_restoration.clone(),
        agents_restoration.clone(),
        previous.clone().map(Box::new),
    )?;
    let applied = RestorationManifest::new(
        JournalState::Applied,
        repository_sha256,
        config_restoration,
        agents_restoration,
        previous.map(Box::new),
    )?;
    let journal = PlannedJournal {
        before: journal_before.clone(),
        prepared,
        applied: Some(applied),
    };
    let applied_journal_bytes =
        serialize_manifest(journal.applied.as_ref().ok_or(Reason::InternalError)?)?;
    let files = vec![
        file_report(ManagedIdentity::Configuration, &configuration),
        file_report(ManagedIdentity::AgentsPolicy, &agents_policy),
        ManagedFileReport {
            identity: ManagedIdentity::RestorationManifest,
            action: if optional_bytes(&journal_before).is_none() {
                ManagedAction::Create
            } else {
                ManagedAction::Replace
            },
            before_sha256: optional_digest(&journal_before),
            target_sha256: Some(digest_bytes(&applied_journal_bytes)),
        },
    ];
    let report = ManagedFilesReport::new(
        ManagedCommand::Init,
        request.dry_run,
        if request.dry_run {
            ManagedOutcome::Planned
        } else {
            ManagedOutcome::Applied
        },
        ManagedResult::new(0, Reason::Completed)?,
        files,
        operations,
        candidates,
        issues,
        manifest_state,
    )?;
    let human_diff = render_plan_diff(
        ManagedCommand::Init,
        &configuration,
        &agents_policy,
        Some(&journal),
    )?;
    Ok(ManagedFilesPlan {
        command: ManagedCommand::Init,
        request: ManagedRequest::Init(request),
        root,
        repository_hash: repository_digest,
        evidence,
        configuration,
        agents_policy,
        journal: Some(journal),
        effective_config: Some(effective_config),
        human_diff,
        report,
    })
}

#[allow(dead_code)] // Task 9 consumes this after acquiring the managed-files lock.
pub(crate) fn managed_plans_match(before: &ManagedFilesPlan, after: &ManagedFilesPlan) -> bool {
    before.command == after.command
        && before.request == after.request
        && before.root == after.root
        && before.repository_hash == after.repository_hash
        && before.evidence == after.evidence
        && before.configuration == after.configuration
        && before.agents_policy == after.agents_policy
        && before.journal == after.journal
        && before.effective_config == after.effective_config
        && before.human_diff == after.human_diff
        && before.report == after.report
}

type GeneratedConfiguration = (
    AgentLowmemConfig,
    Vec<ManagedOperationReport>,
    Vec<ManualCandidateReport>,
    Vec<ManagedIssueReport>,
    Vec<String>,
);

fn generate_configuration(
    root: &Path,
    package_manager: &PackageManagerReport,
    root_bytes: &[u8],
    root_scripts: &BTreeMap<String, String>,
) -> Result<GeneratedConfiguration, Reason> {
    let patterns = match package_manager.kind {
        PackageManagerKind::Npm => {
            parse_npm_workspaces(root_bytes).map_err(|error| error.reason())?
        }
        PackageManagerKind::Pnpm => {
            let root_directory = File::open(root).map_err(|_| Reason::WorkspaceUnsupported)?;
            let state = read_optional_bounded(
                &root_directory,
                "pnpm-workspace.yaml",
                MAX_CONFIGURATION_BYTES,
            )?;
            let bytes = optional_bytes(&state).ok_or(Reason::WorkspaceUnsupported)?;
            parse_pnpm_workspace(bytes)
                .map_err(|error| error.reason())?
                .patterns
        }
    };
    let workspace_candidates =
        expand_workspace_patterns(root, &patterns).map_err(|error| error.reason())?;
    if workspace_candidates.len() > MAX_CONFIG_WORKSPACES {
        return Err(Reason::WorkspaceUnsupported);
    }

    let mut discovered = Vec::new();
    let mut key_indices: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for candidate in workspace_candidates {
        let key = candidate
            .package_name
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        let package_path = format!("{}/package.json", candidate.relative_path);
        let root_directory = File::open(root).map_err(|_| Reason::WorkspaceUnsupported)?;
        let state = read_optional_bounded(&root_directory, &package_path, MAX_CONFIGURATION_BYTES)?;
        let bytes = optional_bytes(&state).ok_or(Reason::WorkspaceUnsupported)?;
        let manifest: PlannerManifest =
            serde_json::from_slice(bytes).map_err(|_| Reason::WorkspaceUnsupported)?;
        let index = discovered.len();
        key_indices.entry(key.clone()).or_default().push(index);
        discovered.push(DiscoveredWorkspace {
            key,
            path: candidate.relative_path,
            package_name: candidate.package_name,
            scripts: manifest.scripts,
        });
    }
    let invalid = discovered
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (!valid_key(&workspace.key)
                || key_indices
                    .get(&workspace.key)
                    .is_some_and(|indices| indices.len() > 1))
            .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let mut issues = invalid
        .iter()
        .map(|index| {
            let workspace = &discovered[*index];
            ManagedIssueReport {
                reason: Reason::WorkspaceCardinality,
                operation_key: None,
                workspace_path: Some(workspace.path.clone()),
                package_name: Some(workspace.package_name.clone()),
            }
        })
        .collect::<Vec<_>>();
    let mut config = AgentLowmemConfig {
        version: 1,
        package_manager: package_manager.kind,
        operations: BTreeMap::new(),
        workspaces: BTreeMap::new(),
    };
    let mut operations = Vec::new();
    let mut manual = Vec::new();
    let mut evidence = Vec::new();
    collect_generated_operations(
        root,
        root,
        PolicyTarget::Root,
        None,
        root_scripts,
        package_manager,
        &mut config.operations,
        &mut operations,
        &mut manual,
        &mut issues,
        &mut evidence,
    );
    for (index, workspace) in discovered.into_iter().enumerate() {
        if invalid.contains(&index) {
            continue;
        }
        let mut workspace_operations = BTreeMap::new();
        collect_generated_operations(
            root,
            &root.join(&workspace.path),
            PolicyTarget::Workspace {
                key: workspace.key.clone(),
                package_name: workspace.package_name.clone(),
            },
            Some(&workspace.key),
            &workspace.scripts,
            package_manager,
            &mut workspace_operations,
            &mut operations,
            &mut manual,
            &mut issues,
            &mut evidence,
        );
        if !workspace_operations.is_empty() {
            config.workspaces.insert(
                workspace.key,
                WorkspaceConfig {
                    path: workspace.path,
                    package_name: workspace.package_name,
                    operations: workspace_operations,
                },
            );
        }
    }
    if manual.len() > MAX_MANUAL_CANDIDATES {
        return Err(Reason::WorkspaceUnsupported);
    }
    Ok((config, operations, manual, issues, evidence))
}

#[allow(clippy::too_many_arguments)]
fn collect_generated_operations(
    root: &Path,
    selected_package: &Path,
    target: PolicyTarget,
    workspace_key: Option<&str>,
    scripts: &BTreeMap<String, String>,
    package_manager: &PackageManagerReport,
    output: &mut BTreeMap<String, OperationConfig>,
    operations: &mut Vec<ManagedOperationReport>,
    manual: &mut Vec<ManualCandidateReport>,
    issues: &mut Vec<ManagedIssueReport>,
    evidence: &mut Vec<String>,
) {
    for canonical in CANONICAL_OPERATIONS {
        if !scripts.contains_key(canonical.script) {
            continue;
        }
        let operation = OperationConfig {
            script: canonical.script.to_owned(),
            timeout_seconds: canonical.timeout_seconds,
        };
        let summary = analyze_managed_operation(
            root,
            selected_package,
            target.clone(),
            canonical.key,
            &operation,
            scripts,
            package_manager,
            false,
        );
        evidence.extend(summary.evidence_files.iter().cloned());
        if summary.status == OperationStatus::Runnable {
            output.insert(canonical.key.to_owned(), operation);
            operations.push(operation_report(&summary));
        } else {
            issues.push(ManagedIssueReport {
                reason: summary.reason.unwrap_or(Reason::OperationUnsupported),
                operation_key: Some(canonical.key.to_owned()),
                workspace_path: workspace_key.map(|_| {
                    selected_package
                        .strip_prefix(root)
                        .unwrap_or(selected_package)
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/")
                }),
                package_name: match &target {
                    PolicyTarget::Workspace { package_name, .. } => Some(package_name.clone()),
                    PolicyTarget::Root => None,
                },
            });
        }
    }
    for script_name in scripts.keys() {
        for canonical in CANONICAL_OPERATIONS {
            if !script_name
                .strip_prefix(canonical.key)
                .is_some_and(|suffix| suffix.starts_with(':') && suffix.len() > 1)
            {
                continue;
            }
            let operation = OperationConfig {
                script: script_name.clone(),
                timeout_seconds: canonical.timeout_seconds,
            };
            let summary = analyze_managed_operation(
                root,
                selected_package,
                target.clone(),
                canonical.key,
                &operation,
                scripts,
                package_manager,
                false,
            );
            evidence.extend(summary.evidence_files.iter().cloned());
            if summary.status == OperationStatus::Runnable {
                manual.push(ManualCandidateReport {
                    operation_prefix: canonical.key.to_owned(),
                    script_name: script_name.clone(),
                    workspace_key: workspace_key.map(ToOwned::to_owned),
                });
            }
        }
    }
}

fn evidence_snapshot(root: &Path, paths: Vec<String>) -> Result<EvidenceSnapshot, Reason> {
    let reader = EvidenceReader::new(root)?;
    let mut digests = Vec::new();
    for path in paths.into_iter().collect::<BTreeSet<_>>() {
        digests.push(reader.read(&path)?.digest());
    }
    EvidenceSnapshot::new(digests)
}

fn selected_lock(kind: PackageManagerKind) -> &'static str {
    match kind {
        PackageManagerKind::Npm => "package-lock.json",
        PackageManagerKind::Pnpm => "pnpm-lock.yaml",
    }
}

fn planned_file(
    ownership: Ownership,
    before: OptionalFile,
    target: Option<Vec<u8>>,
    default_mode: u32,
) -> PlannedFile {
    let action = match (ownership, optional_bytes(&before), target.as_deref()) {
        (Ownership::External, _, _) => ManagedAction::Preserve,
        (_, None, Some(_)) => ManagedAction::Create,
        (_, Some(current), Some(next)) if current == next => ManagedAction::Unchanged,
        (_, Some(_), Some(_)) => ManagedAction::Replace,
        _ => ManagedAction::Preserve,
    };
    let target_mode = target
        .as_ref()
        .map(|_| optional_mode(&before).unwrap_or(default_mode));
    PlannedFile {
        ownership,
        action,
        before,
        target,
        target_mode,
    }
}

fn configuration_restoration(
    file: &PlannedFile,
    previous: Option<&ConfigurationRestoration>,
) -> Result<ConfigurationRestoration, Reason> {
    if file.ownership == Ownership::External {
        return Ok(ConfigurationRestoration {
            ownership: Ownership::External,
            action: DestinationAction::Preserve,
            immediate_before: None,
            target: None,
            stable_baseline: None,
            before_mode: optional_mode(&file.before).map(|mode| mode as u16),
            target_mode: None,
            external_sha256: optional_digest(&file.before),
        });
    }
    let immediate = prior_state(&file.before)?;
    let stable_baseline = previous
        .and_then(|previous| previous.stable_baseline.clone())
        .unwrap_or_else(|| immediate.clone());
    Ok(ConfigurationRestoration {
        ownership: Ownership::Managed,
        action: destination_action(file.action),
        immediate_before: Some(immediate),
        target: file.target.clone().map(OwnedBytes::new).transpose()?,
        stable_baseline: Some(stable_baseline),
        before_mode: optional_mode(&file.before).map(|mode| mode as u16),
        target_mode: file.target_mode.map(|mode| mode as u16),
        external_sha256: None,
    })
}

fn agents_restoration(
    before: &OptionalFile,
    current_managed: Option<Vec<u8>>,
    edit: &crate::agents_policy::AgentsEdit,
    document_was_absent: bool,
    previous: Option<&AgentsRestoration>,
) -> Result<AgentsRestoration, Reason> {
    let immediate = current_managed
        .map(OwnedBytes::new)
        .transpose()?
        .map(PriorManagedState::Bytes)
        .unwrap_or(PriorManagedState::Absent);
    let target_bytes = edit.target_bytes[edit.managed_span.clone()].to_vec();
    let action = if matches!(immediate, PriorManagedState::Absent) {
        DestinationAction::Create
    } else if matches!(&immediate, PriorManagedState::Bytes(bytes) if bytes.bytes == target_bytes) {
        DestinationAction::Unchanged
    } else {
        DestinationAction::Replace
    };
    let target_mode = optional_mode(before).unwrap_or(0o600) as u16;
    let stable_baseline = previous
        .map(|previous| previous.stable_baseline.clone())
        .unwrap_or(PriorManagedState::Absent);
    let original_document_was_absent = previous
        .map(|previous| previous.document_was_absent)
        .unwrap_or(document_was_absent);
    let inserted_separator = previous
        .map(|previous| previous.inserted_separator.clone())
        .unwrap_or_else(|| edit.inserted_separator.clone());
    Ok(AgentsRestoration {
        ownership: Ownership::Managed,
        action,
        document_was_absent: original_document_was_absent,
        immediate_before: immediate.clone(),
        target: Some(OwnedBytes::new(target_bytes)?),
        stable_baseline,
        before_mode: optional_mode(before).map(|mode| mode as u16),
        target_mode: Some(target_mode),
        managed_span: ManagedSpan {
            start: u32::try_from(edit.managed_span.start)
                .map_err(|_| Reason::ManagedFileConflict)?,
            end: u32::try_from(edit.managed_span.end).map_err(|_| Reason::ManagedFileConflict)?,
        },
        inserted_separator,
        prefix_sha256: digest_bytes(&edit.target_bytes[..edit.managed_span.start]),
        suffix_sha256: digest_bytes(&edit.target_bytes[edit.managed_span.end..]),
    })
}

fn validate_previous_agents(
    previous: Option<&RestorationManifest>,
    current: &AgentsDocumentState,
) -> Result<(), Reason> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let expected = previous
        .agents_policy
        .target
        .as_ref()
        .map(|target| target.bytes.as_slice())
        .ok_or(Reason::ManagedFileConflict)?;
    if managed_block_bytes(current).as_deref() != Some(expected) {
        return Err(Reason::ManagedFileConflict);
    }
    Ok(())
}

fn managed_block_bytes(state: &AgentsDocumentState) -> Option<Vec<u8>> {
    match state {
        AgentsDocumentState::OneBlock(block) => Some(block.managed_bytes().to_vec()),
        _ => None,
    }
}

fn parse_previous_manifest(file: &OptionalFile) -> Result<Option<RestorationManifest>, Reason> {
    let Some(bytes) = optional_bytes(file) else {
        return Ok(None);
    };
    let manifest = parse_manifest(bytes)?;
    if manifest.state == JournalState::Prepared {
        return Err(Reason::ManagedFileConflict);
    }
    Ok(Some(manifest))
}

fn without_previous(manifest: RestorationManifest) -> Result<RestorationManifest, Reason> {
    RestorationManifest::new(
        manifest.state,
        manifest.repository_sha256,
        manifest.configuration,
        manifest.agents_policy,
        None,
    )
}

fn operation_report(operation: &crate::repository::OperationSummary) -> ManagedOperationReport {
    ManagedOperationReport {
        operation_key: operation.operation_key.clone(),
        workspace_key: operation.workspace_key.clone(),
    }
}

fn file_report(identity: ManagedIdentity, file: &PlannedFile) -> ManagedFileReport {
    ManagedFileReport {
        identity,
        action: file.action,
        before_sha256: optional_digest(&file.before),
        target_sha256: file.target.as_deref().map(digest_bytes),
    }
}

fn render_plan_diff(
    command: ManagedCommand,
    configuration: &PlannedFile,
    agents_policy: &PlannedFile,
    journal: Option<&PlannedJournal>,
) -> Result<Option<String>, Reason> {
    let mut output = String::new();
    if configuration.ownership == Ownership::Managed && is_mutating_action(configuration.action) {
        append_unified_fragment(
            &mut output,
            CONFIGURATION_PATH,
            optional_bytes(&configuration.before).unwrap_or_default(),
            configuration.target.as_deref().unwrap_or_default(),
            optional_bytes(&configuration.before).is_some(),
            configuration.target.is_some(),
            1,
            1,
        )?;
    }

    if is_mutating_action(agents_policy.action) {
        let restoration = journal
            .map(|journal| &journal.prepared.agents_policy)
            .ok_or(Reason::InternalError)?;
        let immediate = match &restoration.immediate_before {
            PriorManagedState::Absent => &[][..],
            PriorManagedState::Bytes(bytes) => bytes.bytes.as_slice(),
        };
        let mut old_owned = Vec::new();
        let new_owned = match command {
            ManagedCommand::Init => {
                old_owned.extend_from_slice(immediate);
                restoration
                    .target
                    .as_ref()
                    .map(|target| target.bytes.as_slice())
                    .unwrap_or_default()
            }
            ManagedCommand::Restore => {
                old_owned.extend_from_slice(&restoration.inserted_separator);
                old_owned.extend_from_slice(immediate);
                &[][..]
            }
        };
        let owned_start = (restoration.managed_span.start as usize).saturating_sub(
            if command == ManagedCommand::Restore {
                restoration.inserted_separator.len()
            } else {
                0
            },
        );
        let document = optional_bytes(&agents_policy.before).unwrap_or_default();
        let line = 1 + document
            .get(..owned_start.min(document.len()))
            .unwrap_or_default()
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
        append_unified_fragment(
            &mut output,
            AGENTS_PATH,
            &old_owned,
            new_owned,
            optional_bytes(&agents_policy.before).is_some(),
            agents_policy.target.is_some(),
            line,
            line,
        )?;
    }
    Ok((!output.is_empty()).then_some(output))
}

#[allow(clippy::too_many_arguments)]
fn append_unified_fragment(
    output: &mut String,
    identity: &str,
    old: &[u8],
    new: &[u8],
    old_document_exists: bool,
    new_document_exists: bool,
    old_line: usize,
    new_line: usize,
) -> Result<(), Reason> {
    let old = std::str::from_utf8(old).map_err(|_| Reason::InternalError)?;
    let new = std::str::from_utf8(new).map_err(|_| Reason::InternalError)?;
    if !output.is_empty() {
        output.push('\n');
    }
    let old_label = if old_document_exists {
        format!("a/{identity}")
    } else {
        "/dev/null".to_owned()
    };
    let new_label = if new_document_exists {
        format!("b/{identity}")
    } else {
        "/dev/null".to_owned()
    };
    writeln!(output, "--- {old_label}").map_err(|_| Reason::InternalError)?;
    writeln!(output, "+++ {new_label}").map_err(|_| Reason::InternalError)?;
    writeln!(
        output,
        "@@ -{},{} +{},{} @@",
        if old.is_empty() { 0 } else { old_line },
        line_count(old),
        if new.is_empty() { 0 } else { new_line },
        line_count(new)
    )
    .map_err(|_| Reason::InternalError)?;
    append_diff_lines(output, '-', old);
    append_diff_lines(output, '+', new);
    Ok(())
}

fn line_count(value: &str) -> usize {
    value.bytes().filter(|byte| *byte == b'\n').count()
        + usize::from(!value.is_empty() && !value.ends_with('\n'))
}

fn append_diff_lines(output: &mut String, prefix: char, value: &str) {
    for line in value.split_inclusive('\n') {
        output.push(prefix);
        output.push_str(line);
    }
    if !value.is_empty() && !value.ends_with('\n') {
        output.push('\n');
        output.push_str("\\ No newline at end of file\n");
    }
}

fn destination_action(action: ManagedAction) -> DestinationAction {
    match action {
        ManagedAction::Create => DestinationAction::Create,
        ManagedAction::Replace => DestinationAction::Replace,
        ManagedAction::Remove => DestinationAction::Remove,
        ManagedAction::Unchanged => DestinationAction::Unchanged,
        ManagedAction::Preserve => DestinationAction::Preserve,
        ManagedAction::Conflict => DestinationAction::Preserve,
    }
}

fn prior_state(file: &OptionalFile) -> Result<PriorManagedState, Reason> {
    optional_bytes(file)
        .map(|bytes| OwnedBytes::new(bytes.to_vec()).map(PriorManagedState::Bytes))
        .transpose()
        .map(|state| state.unwrap_or(PriorManagedState::Absent))
}

fn optional_bytes(file: &OptionalFile) -> Option<&[u8]> {
    match file {
        OptionalFile::Absent => None,
        OptionalFile::Regular { bytes, .. } => Some(bytes),
    }
}

fn optional_mode(file: &OptionalFile) -> Option<u32> {
    match file {
        OptionalFile::Absent => None,
        OptionalFile::Regular { mode, .. } => Some(*mode),
    }
}

fn valid_private_journal(file: &OptionalFile) -> bool {
    match file {
        OptionalFile::Absent => true,
        OptionalFile::Regular {
            mode, owner_uid, ..
        } => *mode == 0o600 && *owner_uid == rustix::process::geteuid().as_raw(),
    }
}

fn optional_digest(file: &OptionalFile) -> Option<String> {
    match file {
        OptionalFile::Absent => None,
        OptionalFile::Regular { sha256, .. } => Some(hex_digest(sha256)),
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex_digest(&digest)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

fn planning_failure(
    request: InitRequest,
    reason: Reason,
    manifest_state: ManifestState,
) -> ManagedFilesFailure {
    let (code, outcome) = if reason == Reason::ManagedFileConflict {
        if request.dry_run && manifest_state == ManifestState::Prepared {
            (0, ManagedOutcome::RecoveryRequired)
        } else {
            (78, ManagedOutcome::Conflict)
        }
    } else {
        (failure_code(reason), ManagedOutcome::Failed)
    };
    let result = ManagedResult::new(code, reason).unwrap_or(ManagedResult {
        code: 70,
        reason: Reason::InternalError,
    });
    let report = ManagedFilesReport::new(
        ManagedCommand::Init,
        request.dry_run,
        outcome,
        result,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        manifest_state,
    )
    .expect("failure reports are internally valid");
    ManagedFilesFailure { reason, report }
}

fn restore_planning_failure(
    request: RestoreRequest,
    reason: Reason,
    manifest_state: ManifestState,
) -> ManagedFilesFailure {
    let (code, outcome) = if reason == Reason::ManagedFileConflict {
        if request.dry_run && manifest_state == ManifestState::Prepared {
            (0, ManagedOutcome::RecoveryRequired)
        } else {
            (78, ManagedOutcome::Conflict)
        }
    } else {
        (failure_code(reason), ManagedOutcome::Failed)
    };
    let result = ManagedResult::new(code, reason).unwrap_or(ManagedResult {
        code: 70,
        reason: Reason::InternalError,
    });
    let report = ManagedFilesReport::new(
        ManagedCommand::Restore,
        request.dry_run,
        outcome,
        result,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        manifest_state,
    )
    .expect("restore failure reports are internally valid");
    ManagedFilesFailure { reason, report }
}

fn restore_recovery_required_outcome(request: RestoreRequest) -> ManagedFilesOutcome {
    let failure = restore_planning_failure(
        request,
        Reason::ManagedFileConflict,
        ManifestState::Prepared,
    );
    ManagedFilesOutcome {
        report: failure.report,
        human_diff: None,
    }
}

fn restore_recovery_failure_outcome(
    request: RestoreRequest,
    reason: Reason,
    manifest_state: ManifestState,
) -> ManagedFilesOutcome {
    ManagedFilesOutcome {
        report: restore_planning_failure(request, reason, manifest_state).report,
        human_diff: None,
    }
}

fn recovery_required_outcome(request: InitRequest) -> ManagedFilesOutcome {
    let failure = planning_failure(
        request,
        Reason::ManagedFileConflict,
        ManifestState::Prepared,
    );
    ManagedFilesOutcome {
        report: failure.report,
        human_diff: None,
    }
}

fn recovery_failure_outcome(
    request: InitRequest,
    reason: Reason,
    manifest_state: ManifestState,
) -> ManagedFilesOutcome {
    let code = if reason == Reason::ManagedFileConflict {
        78
    } else {
        failure_code(reason)
    };
    let report = ManagedFilesReport::new(
        ManagedCommand::Init,
        request.dry_run,
        if reason == Reason::ManagedFileConflict {
            ManagedOutcome::Conflict
        } else {
            ManagedOutcome::Failed
        },
        ManagedResult::new(code, reason).unwrap_or(ManagedResult {
            code: 70,
            reason: Reason::InternalError,
        }),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        manifest_state,
    )
    .expect("recovery failure reports are internally valid");
    ManagedFilesOutcome {
        report,
        human_diff: None,
    }
}

fn inspect_manifest_state(start: &Path) -> ManifestState {
    let Ok(Some(repository)) = find_git_repository(start) else {
        return ManifestState::Absent;
    };
    let Ok(directory) = File::open(repository.metadata()) else {
        return ManifestState::Absent;
    };
    let Ok(file) = read_optional_bounded(&directory, JOURNAL_PATH, MAX_MANIFEST_BYTES) else {
        return ManifestState::Absent;
    };
    let Some(bytes) = optional_bytes(&file) else {
        return ManifestState::Absent;
    };
    match parse_manifest(bytes).map(|manifest| manifest.state) {
        Ok(JournalState::Prepared) => ManifestState::Prepared,
        Ok(JournalState::Applied) => ManifestState::Applied,
        Err(_) => ManifestState::Absent,
    }
}

fn failure_code(reason: Reason) -> i32 {
    match reason {
        Reason::InvalidCli | Reason::InvalidConfig => 2,
        Reason::LockHeld | Reason::NestedInvocation => 73,
        Reason::EvidenceChanged => 75,
        Reason::InternalError => 70,
        _ => 64,
    }
}

fn previous_applied_matches(plan: &ManagedFilesPlan) -> bool {
    let Some(journal) = plan.journal.as_ref() else {
        return false;
    };
    if plan.report.manifest_state != ManifestState::Applied
        || optional_mode(&journal.before) != Some(0o600)
        || !matches!(
            plan.configuration.action,
            ManagedAction::Unchanged | ManagedAction::Preserve
        )
        || plan.agents_policy.action != ManagedAction::Unchanged
    {
        return false;
    }
    let Ok(metadata) = HeldDirectory::open(plan.root.metadata(), None) else {
        return false;
    };
    if HeldDirectory::open_child(&metadata, "agent-lowmem", Some(0o700)).is_err() {
        return false;
    }
    let Some(bytes) = optional_bytes(&journal.before) else {
        return false;
    };
    let Ok(previous) = parse_manifest(bytes) else {
        return false;
    };
    let configuration_matches = match previous.configuration.ownership {
        Ownership::Managed => previous
            .configuration
            .target
            .as_ref()
            .is_some_and(|target| {
                Some(target.bytes.as_slice()) == plan.configuration.target.as_deref()
            }),
        Ownership::External => {
            previous.configuration.external_sha256 == optional_digest(&plan.configuration.before)
        }
    };
    let Some(applied) = &journal.applied else {
        return false;
    };
    let desired_agents = &applied.agents_policy;
    configuration_matches
        && previous.agents_policy.target == desired_agents.target
        && previous.agents_policy.managed_span == desired_agents.managed_span
        && previous.agents_policy.prefix_sha256 == desired_agents.prefix_sha256
        && previous.agents_policy.suffix_sha256 == desired_agents.suffix_sha256
        && previous.agents_policy.target_mode == desired_agents.target_mode
}

fn unchanged_outcome(plan: &ManagedFilesPlan) -> ManagedFilesOutcome {
    let mut report = plan.report.clone();
    report.outcome = ManagedOutcome::Unchanged;
    if let Some(journal) = report
        .files
        .iter_mut()
        .find(|file| file.identity == ManagedIdentity::RestorationManifest)
    {
        journal.action = ManagedAction::Unchanged;
        journal.target_sha256 = journal.before_sha256.clone();
    }
    ManagedFilesOutcome {
        report,
        human_diff: None,
    }
}

fn managed_failure_outcome(plan: &ManagedFilesPlan, reason: Reason) -> ManagedFilesOutcome {
    let mut report = plan.report.clone();
    let code = if reason == Reason::ManagedFileConflict {
        78
    } else {
        failure_code(reason)
    };
    report.outcome = if reason == Reason::ManagedFileConflict {
        ManagedOutcome::Conflict
    } else {
        ManagedOutcome::Failed
    };
    report.result = ManagedResult::new(code, reason).unwrap_or(ManagedResult {
        code: 70,
        reason: Reason::InternalError,
    });
    ManagedFilesOutcome {
        report,
        human_diff: None,
    }
}

fn unix_seconds_now() -> Result<u64, Reason> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Reason::InternalError)?
        .as_secs();
    (seconds > 0)
        .then_some(seconds)
        .ok_or(Reason::InternalError)
}

#[cfg(test)]
mod transaction_tests {
    use super::{
        FaultPoint, ManagedOutcome, TransactionFaults, execute_init_core, execute_restore_core,
    };
    use crate::{
        cli::{InitRequest, RestoreRequest},
        host::{HostReadError, HostSource},
        result::Reason,
    };
    use std::{
        collections::BTreeMap,
        fs,
        panic::{AssertUnwindSafe, catch_unwind},
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct OneFault(FaultPoint);

    impl TransactionFaults for OneFault {
        fn fail_at(&self, point: FaultPoint) -> bool {
            self.0 == point
        }
    }

    struct CrashAt(FaultPoint);

    impl TransactionFaults for CrashAt {
        fn fail_at(&self, point: FaultPoint) -> bool {
            assert_ne!(self.0, point, "simulated process crash at {point:?}");
            false
        }
    }

    #[test]
    fn every_durable_fault_rolls_back_the_first_init_completely() {
        for point in [
            FaultPoint::PreparedDurable,
            FaultPoint::ConfigurationWritten,
            FaultPoint::AgentsWritten,
            FaultPoint::TargetsVerified,
            FaultPoint::AppliedJournalDurable,
        ] {
            let fixture = Fixture::new();
            let outcome = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &InitRequest {
                    dry_run: false,
                    json: true,
                },
                || {},
                &OneFault(point),
            );

            assert_eq!(outcome.report.result.code, 70, "fault: {point:?}");
            assert_eq!(
                outcome.report.result.reason,
                Reason::InternalError,
                "fault: {point:?}"
            );
            assert!(!fixture.root.join(".agent-lowmem.json").exists());
            assert!(!fixture.root.join("AGENTS.md").exists());
            assert!(!fixture.root.join(".git/agent-lowmem").exists());
        }
    }

    #[test]
    fn every_durable_fault_restores_the_prior_applied_transaction() {
        for point in [
            FaultPoint::PreparedDurable,
            FaultPoint::ConfigurationWritten,
            FaultPoint::AgentsWritten,
            FaultPoint::TargetsVerified,
            FaultPoint::AppliedJournalDurable,
        ] {
            let fixture = Fixture::new();
            let request = InitRequest {
                dry_run: false,
                json: true,
            };
            let first = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request,
                || {},
                &OneFault(FaultPoint::Never),
            );
            assert_eq!(first.report.result.reason, Reason::Completed);
            fs::create_dir_all(fixture.root.join("node_modules/eslint")).unwrap();
            fs::write(
                fixture.root.join("node_modules/eslint/package.json"),
                "{\"name\":\"eslint\",\"version\":\"10.9.1\"}\n",
            )
            .unwrap();
            fs::write(
                fixture.root.join("package.json"),
                r#"{"name":"fixture","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","lint":"eslint ."}}"#,
            )
            .unwrap();
            let before = managed_bytes(&fixture.root);

            let outcome = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request,
                || {},
                &OneFault(point),
            );

            assert_eq!(outcome.report.result.code, 70, "fault: {point:?}");
            assert_eq!(managed_bytes(&fixture.root), before, "fault: {point:?}");
            let journal: serde_json::Value = serde_json::from_slice(&before.2).unwrap();
            assert_eq!(journal["state"], "applied");
        }
    }

    #[test]
    fn the_next_init_recovers_after_every_crash_boundary() {
        for point in [
            FaultPoint::PreparedDurable,
            FaultPoint::ConfigurationWritten,
            FaultPoint::AgentsWritten,
            FaultPoint::TargetsVerified,
            FaultPoint::AppliedJournalDurable,
        ] {
            let fixture = Fixture::new();
            let request = InitRequest {
                dry_run: false,
                json: true,
            };
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                execute_init_core(
                    &SupportedHost::reference(),
                    &fixture.root,
                    &fixture.runtime,
                    &request,
                    || {},
                    &CrashAt(point),
                )
            }));
            assert!(crashed.is_err(), "fault: {point:?}");

            let recovered = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request,
                || {},
                &OneFault(FaultPoint::Never),
            );

            assert_eq!(recovered.report.result.code, 0, "fault: {point:?}");
            assert_eq!(
                recovered.report.result.reason,
                Reason::Completed,
                "fault: {point:?}"
            );
            assert!(fixture.root.join(".agent-lowmem.json").is_file());
            assert!(fixture.root.join("AGENTS.md").is_file());
            let journal: serde_json::Value = serde_json::from_slice(
                &fs::read(fixture.root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(journal["state"], "applied", "fault: {point:?}");
        }
    }

    #[test]
    fn recovery_plan_b_drift_returns_75_without_overwriting_the_change() {
        let fixture = Fixture::new();
        let request = InitRequest {
            dry_run: false,
            json: true,
        };
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request,
                || {},
                &CrashAt(FaultPoint::PreparedDurable),
            )
        }));
        assert!(crashed.is_err());
        let root = fixture.root.clone();

        let outcome = execute_init_core(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &request,
            || {
                fs::write(
                    root.join(".agent-lowmem.json"),
                    "{\"version\":1,\"packageManager\":\"npm\",\"operations\":{}}\n",
                )
                .unwrap();
            },
            &OneFault(FaultPoint::Never),
        );

        assert_eq!(outcome.report.result.code, 75);
        assert_eq!(outcome.report.result.reason, Reason::EvidenceChanged);
        assert_eq!(
            fs::read_to_string(fixture.root.join(".agent-lowmem.json")).unwrap(),
            "{\"version\":1,\"packageManager\":\"npm\",\"operations\":{}}\n"
        );
        let journal: serde_json::Value = serde_json::from_slice(
            &fs::read(fixture.root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(journal["state"], "prepared");
    }

    #[test]
    fn every_restore_fault_rolls_back_to_the_applied_state() {
        for point in [
            FaultPoint::PreparedDurable,
            FaultPoint::ConfigurationWritten,
            FaultPoint::AgentsWritten,
            FaultPoint::TargetsVerified,
        ] {
            let fixture = Fixture::new();
            initialize(&fixture);
            let before = managed_bytes(&fixture.root);
            let outcome = execute_restore_core(
                &fixture.root,
                &fixture.runtime,
                &RestoreRequest {
                    dry_run: false,
                    force_managed_block: false,
                    json: true,
                },
                || {},
                &OneFault(point),
            );

            assert_eq!(outcome.report.result.code, 70, "fault: {point:?}");
            assert_eq!(managed_bytes(&fixture.root), before, "fault: {point:?}");
        }
    }

    #[test]
    fn the_next_restore_recovers_after_every_restore_crash_boundary() {
        for point in [
            FaultPoint::PreparedDurable,
            FaultPoint::ConfigurationWritten,
            FaultPoint::AgentsWritten,
            FaultPoint::TargetsVerified,
        ] {
            let fixture = Fixture::new();
            initialize(&fixture);
            let request = RestoreRequest {
                dry_run: false,
                force_managed_block: false,
                json: true,
            };
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                execute_restore_core(
                    &fixture.root,
                    &fixture.runtime,
                    &request,
                    || {},
                    &CrashAt(point),
                )
            }));
            assert!(crashed.is_err(), "fault: {point:?}");

            let recovered = execute_restore_core(
                &fixture.root,
                &fixture.runtime,
                &request,
                || {},
                &OneFault(FaultPoint::Never),
            );

            assert_eq!(
                recovered.report.outcome,
                ManagedOutcome::Restored,
                "fault: {point:?}, outcome: {recovered:?}"
            );
            assert!(!fixture.root.join(".agent-lowmem.json").exists());
            assert!(!fixture.root.join("AGENTS.md").exists());
            assert!(!fixture.root.join(".git/agent-lowmem").exists());
        }
    }

    #[test]
    fn post_lock_source_drift_returns_75_before_the_prepared_journal() {
        for identity in ["package", "lockfile", "tool"] {
            let fixture = Fixture::new();
            let root = fixture.root.clone();
            let outcome = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &InitRequest {
                    dry_run: false,
                    json: true,
                },
                || {
                    match identity {
                    "package" => fs::write(
                        root.join("package.json"),
                        r#"{"name":"changed","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
                    )
                    .unwrap(),
                    "lockfile" => {
                        fs::write(root.join("package-lock.json"), "{\"lockfileVersion\":3,\"changed\":true}\n")
                            .unwrap()
                    }
                    "tool" => fs::write(
                        root.join("node_modules/vitest/package.json"),
                        "{\"name\":\"vitest\",\"version\":\"4.1.12\"}\n",
                    )
                    .unwrap(),
                    _ => unreachable!(),
                }
                },
                &OneFault(FaultPoint::Never),
            );

            assert_eq!(outcome.report.result.code, 75, "identity: {identity}");
            assert_eq!(
                outcome.report.result.reason,
                Reason::EvidenceChanged,
                "identity: {identity}"
            );
            assert!(!fixture.root.join(".agent-lowmem.json").exists());
            assert!(!fixture.root.join("AGENTS.md").exists());
            assert!(!fixture.root.join(".git/agent-lowmem").exists());
        }
    }

    #[test]
    fn post_lock_destination_drift_returns_75_without_overwriting_it() {
        for identity in ["configuration", "agents", "journal"] {
            let fixture = Fixture::new();
            let root = fixture.root.clone();
            let outcome = execute_init_core(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &InitRequest {
                    dry_run: false,
                    json: true,
                },
                || {
                    match identity {
                    "configuration" => fs::write(
                        root.join(".agent-lowmem.json"),
                        r#"{"version":1,"packageManager":"npm","operations":{"checks":{"script":"test","timeoutSeconds":300}}}"#,
                    )
                    .unwrap(),
                    "agents" => fs::write(root.join("AGENTS.md"), "external policy\n").unwrap(),
                    "journal" => {
                        fs::create_dir_all(root.join(".git/agent-lowmem")).unwrap();
                        fs::write(
                            root.join(".git/agent-lowmem/restoration-v1.json"),
                            "external journal drift\n",
                        )
                        .unwrap();
                    }
                    _ => unreachable!(),
                }
                },
                &OneFault(FaultPoint::Never),
            );

            assert_eq!(outcome.report.result.code, 75, "identity: {identity}");
            assert_eq!(
                outcome.report.result.reason,
                Reason::EvidenceChanged,
                "identity: {identity}"
            );
            match identity {
                "configuration" => assert!(
                    fs::read_to_string(root.join(".agent-lowmem.json"))
                        .unwrap()
                        .contains("checks")
                ),
                "agents" => assert_eq!(
                    fs::read_to_string(root.join("AGENTS.md")).unwrap(),
                    "external policy\n"
                ),
                "journal" => assert_eq!(
                    fs::read_to_string(root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
                    "external journal drift\n"
                ),
                _ => unreachable!(),
            }
        }
    }

    struct Fixture {
        root: PathBuf,
        runtime: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let base = std::env::temp_dir().join(format!(
                "agent-lowmem-transaction-unit-{nanos}-{}-{serial}",
                std::process::id()
            ));
            let root = base.join("repository");
            fs::create_dir_all(root.join(".git")).unwrap();
            fs::create_dir_all(root.join("node_modules/vitest")).unwrap();
            fs::write(
                root.join("package.json"),
                r#"{"name":"fixture","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
            )
            .unwrap();
            fs::write(root.join("package-lock.json"), "{\"lockfileVersion\":3}\n").unwrap();
            fs::write(
                root.join("node_modules/vitest/package.json"),
                "{\"name\":\"vitest\",\"version\":\"4.1.11\"}\n",
            )
            .unwrap();
            Self {
                root: fs::canonicalize(root).unwrap(),
                runtime: base.join("runtime"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.root.parent().unwrap());
        }
    }

    fn managed_bytes(root: &std::path::Path) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        (
            fs::read(root.join(".agent-lowmem.json")).unwrap(),
            fs::read(root.join("AGENTS.md")).unwrap(),
            fs::read(root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
        )
    }

    fn initialize(fixture: &Fixture) {
        let outcome = execute_init_core(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &InitRequest {
                dry_run: false,
                json: true,
            },
            || {},
            &OneFault(FaultPoint::Never),
        );
        assert_eq!(outcome.report.result.reason, Reason::Completed);
    }

    struct SupportedHost {
        values: BTreeMap<&'static str, &'static str>,
    }

    impl SupportedHost {
        fn reference() -> Self {
            Self {
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
            "macos"
        }

        fn architecture(&self) -> &str {
            "aarch64"
        }

        fn read(&self, key: &'static str) -> Result<String, HostReadError> {
            self.values
                .get(key)
                .map(|value| (*value).to_owned())
                .ok_or(HostReadError::Missing(key))
        }
    }
}
