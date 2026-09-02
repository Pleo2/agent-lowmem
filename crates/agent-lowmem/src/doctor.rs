use crate::{
    host::{HostReport, HostSource, inspect_host},
    repository::{RepositoryReport, inspect_repository},
};
use serde::Serialize;
use std::path::Path;

const PHASE: &str = "native-foundation";
const NEXT_ACTION: &str = "implement repository policy before enabling managed runs";

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
    format!(
        "Agent Lowmem doctor\n\
         Runtime supported: {}\n\
         Performance validated: {}\n\
         Repository available: {}\n\
         Phase: {}\n\
         Managed runs: unavailable in Phase 1\n\
         Next action: {}",
        yes_no(report.host.runtime_supported),
        yes_no(report.host.performance_validated),
        yes_no(report.repository.git_root_available),
        report.phase,
        report.next_action,
    )
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
            failure_reason: Some(Reason::RepositoryUnsupported),
        }
    }

    #[test]
    fn assembles_the_private_native_foundation_checkpoint() {
        let report = assemble_doctor_report(reference_host(), outside_repository());
        let json = serde_json::to_value(&report).unwrap();

        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["phase"], "native-foundation");
        assert_eq!(
            json["nextAction"],
            "implement repository policy before enabling managed runs"
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
        assert!(output.contains("Managed runs: unavailable in Phase 1"));
        assert!(!output.contains('/'));
    }
}
