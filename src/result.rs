use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Preflight,
    Child,
    SupervisorTimeout,
    ExternalSignal,
    Internal,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Child => "child",
            Self::SupervisorTimeout => "supervisor-timeout",
            Self::ExternalSignal => "external-signal",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Completed,
    InvalidCli,
    InvalidConfig,
    HostUnsupported,
    RepositoryUnsupported,
    PackageManagerUnsupported,
    WorkspaceUnsupported,
    WorkspaceCardinality,
    OperationUnsupported,
    ScriptSyntaxUnsupported,
    ScriptShellUnsupported,
    ScriptReferenceUnsupported,
    ScriptGraphTooLarge,
    WrapperUnsupported,
    ToolUnsupported,
    ToolVersionUnsupported,
    WatchDenied,
    UiDenied,
    BackgroundDenied,
    ParallelDenied,
    ArgumentDenied,
    NonfinalInjectionRequired,
    LockHeld,
    NestedInvocation,
    EvidenceChanged,
    ManagedFileConflict,
    ChildExit,
    ChildSignal,
    DeadlineExceeded,
    ExternalSignal,
    InternalError,
}

impl Reason {
    pub const ALL: [Self; 31] = [
        Self::Completed,
        Self::InvalidCli,
        Self::InvalidConfig,
        Self::HostUnsupported,
        Self::RepositoryUnsupported,
        Self::PackageManagerUnsupported,
        Self::WorkspaceUnsupported,
        Self::WorkspaceCardinality,
        Self::OperationUnsupported,
        Self::ScriptSyntaxUnsupported,
        Self::ScriptShellUnsupported,
        Self::ScriptReferenceUnsupported,
        Self::ScriptGraphTooLarge,
        Self::WrapperUnsupported,
        Self::ToolUnsupported,
        Self::ToolVersionUnsupported,
        Self::WatchDenied,
        Self::UiDenied,
        Self::BackgroundDenied,
        Self::ParallelDenied,
        Self::ArgumentDenied,
        Self::NonfinalInjectionRequired,
        Self::LockHeld,
        Self::NestedInvocation,
        Self::EvidenceChanged,
        Self::ManagedFileConflict,
        Self::ChildExit,
        Self::ChildSignal,
        Self::DeadlineExceeded,
        Self::ExternalSignal,
        Self::InternalError,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::InvalidCli => "invalid-cli",
            Self::InvalidConfig => "invalid-config",
            Self::HostUnsupported => "host-unsupported",
            Self::RepositoryUnsupported => "repository-unsupported",
            Self::PackageManagerUnsupported => "package-manager-unsupported",
            Self::WorkspaceUnsupported => "workspace-unsupported",
            Self::WorkspaceCardinality => "workspace-cardinality",
            Self::OperationUnsupported => "operation-unsupported",
            Self::ScriptSyntaxUnsupported => "script-syntax-unsupported",
            Self::ScriptShellUnsupported => "script-shell-unsupported",
            Self::ScriptReferenceUnsupported => "script-reference-unsupported",
            Self::ScriptGraphTooLarge => "script-graph-too-large",
            Self::WrapperUnsupported => "wrapper-unsupported",
            Self::ToolUnsupported => "tool-unsupported",
            Self::ToolVersionUnsupported => "tool-version-unsupported",
            Self::WatchDenied => "watch-denied",
            Self::UiDenied => "ui-denied",
            Self::BackgroundDenied => "background-denied",
            Self::ParallelDenied => "parallel-denied",
            Self::ArgumentDenied => "argument-denied",
            Self::NonfinalInjectionRequired => "nonfinal-injection-required",
            Self::LockHeld => "lock-held",
            Self::NestedInvocation => "nested-invocation",
            Self::EvidenceChanged => "evidence-changed",
            Self::ManagedFileConflict => "managed-file-conflict",
            Self::ChildExit => "child-exit",
            Self::ChildSignal => "child-signal",
            Self::DeadlineExceeded => "deadline-exceeded",
            Self::ExternalSignal => "external-signal",
            Self::InternalError => "internal-error",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::Completed => "managed operation completed",
            Self::ChildExit => "managed operation exited unsuccessfully",
            Self::ChildSignal => "managed operation ended from a signal",
            Self::DeadlineExceeded => "managed operation exceeded its deadline",
            Self::ExternalSignal => "managed operation was interrupted",
            Self::InternalError => "the managed runner encountered an internal error",
            Self::LockHeld => "another managed operation is active",
            Self::NestedInvocation => "nested managed execution is not allowed",
            Self::EvidenceChanged => "repository evidence changed before launch",
            Self::ManagedFileConflict => "the managed file destination is unsafe",
            Self::InvalidCli => "the run request is invalid",
            Self::InvalidConfig => "the repository configuration is invalid",
            _ => "the managed operation is not supported by the current policy",
        }
    }

    pub const fn next_action(self) -> &'static str {
        match self {
            Self::Completed => "none",
            Self::ChildExit | Self::ChildSignal => "inspect the inherited child output",
            Self::DeadlineExceeded => "narrow the operation or use CI for the broad workload",
            Self::ExternalSignal => "rerun the operation when ready",
            Self::InternalError => "run agent-lowmem doctor and inspect the result reason",
            Self::LockHeld => "wait for the active managed operation to finish",
            Self::NestedInvocation => "invoke agent-lowmem only from the outer agent task",
            Self::EvidenceChanged => "review repository changes and rerun explicitly",
            Self::ManagedFileConflict => "choose a safe repository-relative regular-file path",
            Self::InvalidCli => "correct the command arguments and rerun",
            Self::InvalidConfig => "correct .agent-lowmem.json and rerun",
            _ => "run agent-lowmem doctor and choose a supported operation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExitResult {
    pub origin: Origin,
    pub code: i32,
    pub reason: Reason,
}

impl ExitResult {
    pub const fn new(origin: Origin, code: i32, reason: Reason) -> Self {
        Self {
            origin,
            code,
            reason,
        }
    }

    pub const fn is_valid(self) -> bool {
        match self.reason {
            Reason::Completed => matches!((self.origin, self.code), (Origin::Child, 0)),
            Reason::InvalidCli | Reason::InvalidConfig => {
                matches!((self.origin, self.code), (Origin::Preflight, 2))
            }
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
            | Reason::NonfinalInjectionRequired => {
                matches!((self.origin, self.code), (Origin::Preflight, 64))
            }
            Reason::LockHeld | Reason::NestedInvocation => {
                matches!((self.origin, self.code), (Origin::Preflight, 73))
            }
            Reason::EvidenceChanged => {
                matches!((self.origin, self.code), (Origin::Preflight, 75))
            }
            Reason::ManagedFileConflict => {
                matches!((self.origin, self.code), (Origin::Preflight, 78))
            }
            Reason::ChildExit => {
                matches!(self.origin, Origin::Child) && self.code >= 1 && self.code <= 255
            }
            Reason::ChildSignal => {
                matches!(self.origin, Origin::Child) && self.code >= 129 && self.code <= 255
            }
            Reason::DeadlineExceeded => {
                matches!((self.origin, self.code), (Origin::SupervisorTimeout, 124))
            }
            Reason::ExternalSignal => {
                matches!(self.origin, Origin::ExternalSignal)
                    && self.code >= 129
                    && self.code <= 255
            }
            Reason::InternalError => {
                matches!((self.origin, self.code), (Origin::Internal, 70))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ExitResult, Origin, Reason};
    use std::{fs, path::Path};

    #[test]
    fn serializes_stable_kebab_case_tokens() {
        assert_eq!(
            serde_json::to_string(&Reason::ScriptGraphTooLarge).unwrap(),
            "\"script-graph-too-large\""
        );
        assert_eq!(
            serde_json::to_string(&Origin::SupervisorTimeout).unwrap(),
            "\"supervisor-timeout\""
        );
    }

    #[test]
    fn rejects_invalid_origin_code_reason_combinations() {
        assert!(ExitResult::new(Origin::Preflight, 64, Reason::HostUnsupported).is_valid());
        assert!(ExitResult::new(Origin::Child, 0, Reason::Completed).is_valid());
        assert!(!ExitResult::new(Origin::Child, 0, Reason::InternalError).is_valid());
        assert!(!ExitResult::new(Origin::Preflight, 75, Reason::ChildExit).is_valid());
    }

    #[test]
    fn schema_reason_vocabulary_matches_rust_in_order() {
        let schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/result-v1.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();

        assert_eq!(
            schema.pointer("/properties/reason/enum").unwrap(),
            &serde_json::to_value(Reason::ALL).unwrap()
        );
    }

    #[test]
    fn stable_result_line_tokens_cover_the_closed_contract() {
        let reason_tokens = Reason::ALL.map(Reason::as_str);
        assert_eq!(
            reason_tokens,
            [
                "completed",
                "invalid-cli",
                "invalid-config",
                "host-unsupported",
                "repository-unsupported",
                "package-manager-unsupported",
                "workspace-unsupported",
                "workspace-cardinality",
                "operation-unsupported",
                "script-syntax-unsupported",
                "script-shell-unsupported",
                "script-reference-unsupported",
                "script-graph-too-large",
                "wrapper-unsupported",
                "tool-unsupported",
                "tool-version-unsupported",
                "watch-denied",
                "ui-denied",
                "background-denied",
                "parallel-denied",
                "argument-denied",
                "nonfinal-injection-required",
                "lock-held",
                "nested-invocation",
                "evidence-changed",
                "managed-file-conflict",
                "child-exit",
                "child-signal",
                "deadline-exceeded",
                "external-signal",
                "internal-error",
            ]
        );
        assert_eq!(Origin::Preflight.as_str(), "preflight");
        assert_eq!(Origin::Child.as_str(), "child");
        assert_eq!(Origin::SupervisorTimeout.as_str(), "supervisor-timeout");
        assert_eq!(Origin::ExternalSignal.as_str(), "external-signal");
        assert_eq!(Origin::Internal.as_str(), "internal");
    }
}
