use crate::{
    host::{HostReport, HostSource, inspect_host},
    repository::{RepositoryReport, inspect_repository},
    result::Reason,
};
use serde::Serialize;
use std::path::Path;

const PHASE: &str = "repository-policy";
const NEXT_ACTION: &str = "design the managed runner from verified operation policies";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub schema_version: u8,
    pub phase: &'static str,
    pub host: HostReport,
    pub repository: RepositoryReport,
    pub next_action: &'static str,
}

pub fn inspect_doctor(source: &impl HostSource, start: &Path) -> DoctorReport {
    assemble_doctor_report(inspect_host(source), inspect_repository(start))
}

pub fn assemble_doctor_report(host: HostReport, repository: RepositoryReport) -> DoctorReport {
    DoctorReport {
        schema_version: 1,
        phase: PHASE,
        host,
        repository,
        next_action: NEXT_ACTION,
    }
}

pub fn render_human(report: &DoctorReport) -> String {
    let mut output = format!(
        "Agent Lowmem doctor\n\
         Runtime supported: {}\n\
         Performance validated: {}\n\
         Repository available: {}\n\
         Phase: {}\n\
         Managed runs: unavailable in Phase 2\n\
         Next action: {}",
        yes_no(report.host.runtime_supported),
        yes_no(report.host.performance_validated),
        yes_no(report.repository.git_root_available),
        report.phase,
        report.next_action,
    );
    if report.repository.operations.is_empty() {
        output.push_str("\nRepository operations: none");
    } else {
        output.push_str("\nRepository operations:");
        for operation in &report.repository.operations {
            let target = operation.workspace_key.as_deref().unwrap_or("root");
            let reason = operation.reason.map(Reason::as_str).unwrap_or("compatible");
            let mode = if operation.configured {
                "configured"
            } else {
                "candidate"
            };
            output.push_str(&format!(
                "\n- {target}:{} [{}] {} ({reason})",
                operation.operation_key,
                mode,
                match operation.status {
                    crate::repository::OperationStatus::Runnable => "runnable",
                    crate::repository::OperationStatus::Rejected => "rejected",
                }
            ));
            if !operation.disclosures.is_empty() {
                output.push_str(" disclosures=");
                output.push_str(&operation.disclosures.join(","));
            }
        }
    }
    output
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::{assemble_doctor_report, render_human};
    use crate::{host::HostReport, repository::RepositoryReport, result::Reason};

    fn reference_host() -> HostReport {
        HostReport {
            operating_system: "darwin".to_owned(),
            architecture: "arm64".to_owned(),
            macos_version: Some("26.6.2".to_owned()),
            hardware_model: Some("Mac14,15".to_owned()),
            cpu_brand: Some("Apple M2".to_owned()),
            physical_memory_bytes: Some(8_589_934_592),
            page_size_bytes: Some(16_384),
            runtime_supported: true,
            performance_validated: true,
            mismatched_profile_fields: Vec::new(),
            failure_reason: None,
        }
    }

    fn outside_repository() -> RepositoryReport {
        RepositoryReport {
            git_root_available: false,
            root_package_available: false,
            package_manager: None,
            operations: Vec::new(),
            failure_reason: Some(Reason::RepositoryUnsupported),
        }
    }

    #[test]
    fn assembles_the_private_repository_policy_checkpoint() {
        let report = assemble_doctor_report(reference_host(), outside_repository());
        let json = serde_json::to_value(&report).unwrap();

        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["phase"], "repository-policy");
        assert_eq!(
            json["nextAction"],
            "design the managed runner from verified operation policies"
        );
        assert!(json.get("timestamp").is_none());
    }

    #[test]
    fn human_report_states_support_and_phase_limit_without_a_path() {
        let report = assemble_doctor_report(reference_host(), outside_repository());
        let output = render_human(&report);

        assert!(output.contains("Agent Lowmem doctor"));
        assert!(output.contains("Runtime supported: yes"));
        assert!(output.contains("Performance validated: yes"));
        assert!(output.contains("Repository available: no"));
        assert!(output.contains("Managed runs: unavailable in Phase 2"));
        assert!(!output.contains('/'));
    }
}
