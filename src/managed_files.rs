use crate::{
    configuration::{valid_key, valid_package_name, valid_relative_path},
    result::Reason,
};
use serde::Serialize;

pub const MANAGED_FILES_SCHEMA_VERSION: u8 = 1;
pub const MAX_MANUAL_CANDIDATES: usize = 256;

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
