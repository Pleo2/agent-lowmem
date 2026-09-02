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
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/result-v1.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();

        assert_eq!(
            schema.pointer("/properties/reason/enum").unwrap(),
            &serde_json::to_value(Reason::ALL).unwrap()
        );
    }
}
