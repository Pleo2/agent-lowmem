use crate::{
    adapter::{
        AdapterMatrix, Classification, ControlDecision, match_adapter, match_package_manager,
        package_for_executable,
    },
    configuration::{OperationConfig, valid_package_name, valid_relative_path},
    package_manager::{LaunchArray, build_launch_array},
    repository::{PackageManagerKind, PackageManagerReport},
    result::Reason,
    script::{
        graph::{ScriptGraph, ScriptPhase},
        wrapper::{WrapperEvidence, WrapperIdentity, unwrap_segment},
    },
};
use semver::Version;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq, Eq)]
pub struct OperationPolicy {
    pub target: PolicyTarget,
    pub operation_key: String,
    pub script_key: String,
    pub timeout_seconds: u16,
    pub graph_depth: u8,
    pub leaves: Vec<PolicyLeaf>,
    pub launch: LaunchArray,
    pub disclosures: Vec<String>,
    pub evidence_files: Vec<String>,
}

impl std::fmt::Debug for OperationPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.redacted_summary().fmt(formatter)
    }
}

impl OperationPolicy {
    pub fn redacted_summary(&self) -> RedactedPolicySummary<'_> {
        RedactedPolicySummary {
            target: &self.target,
            operation_key: &self.operation_key,
            script_key: &self.script_key,
            timeout_seconds: self.timeout_seconds,
            graph_depth: self.graph_depth,
            leaves: &self.leaves,
            disclosures: &self.disclosures,
            evidence_files: &self.evidence_files,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PolicyTarget {
    Root,
    Workspace { key: String, package_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyLeaf {
    pub script_key: String,
    pub phase: ScriptPhase,
    pub classification: Classification,
    pub potential_lifecycle: bool,
    pub wrapper: Option<WrapperEvidence>,
    pub control: ControlDecision,
}

pub struct PolicyInput<'a> {
    pub target: PolicyTarget,
    pub operation_key: &'a str,
    pub operation: &'a OperationConfig,
    pub graph: &'a ScriptGraph,
    pub matrix: &'a AdapterMatrix,
    pub package_manager: PackageManagerKind,
    pub package_manager_version: &'a Version,
    pub installed_versions: &'a BTreeMap<String, Version>,
    pub forwarded_arguments: &'a [String],
    pub evidence_files: &'a [String],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedPolicySummary<'a> {
    target: &'a PolicyTarget,
    operation_key: &'a str,
    script_key: &'a str,
    timeout_seconds: u16,
    graph_depth: u8,
    leaves: &'a [PolicyLeaf],
    disclosures: &'a [String],
    evidence_files: &'a [String],
}

pub fn build_operation_policy(input: PolicyInput<'_>) -> Result<OperationPolicy, Reason> {
    let package_manager_report = PackageManagerReport {
        kind: input.package_manager,
        version: input.package_manager_version.to_string(),
    };
    match_package_manager(input.matrix, &package_manager_report)?;
    let workspace_package_name = match &input.target {
        PolicyTarget::Root => None,
        PolicyTarget::Workspace { package_name, .. } => {
            if !valid_package_name(package_name) {
                return Err(Reason::InvalidConfig);
            }
            Some(package_name.as_str())
        }
    };

    let evidence_files = normalize_evidence_files(input.evidence_files)?;
    let mut leaves = Vec::with_capacity(input.graph.leaves.len());
    let mut disclosures = Vec::new();
    let mut injection_arguments = Vec::new();
    let mut missing_nonfinal_control = false;
    let mut final_recipient_seen = false;
    let mut target_work_seen = false;

    for occurrence in &input.graph.leaves {
        let (arguments, wrapper) =
            unwrap_policy_segment(input.matrix, &occurrence.segment, input.installed_versions)?;
        let executable = arguments.first().ok_or(Reason::ToolUnsupported)?;
        let package_name = package_for_executable(input.matrix, executable)?;
        let version = input
            .installed_versions
            .get(package_name)
            .ok_or(Reason::ToolUnsupported)?;

        let is_final_recipient = occurrence.final_top_level;
        let mut classified_arguments = arguments;
        if is_final_recipient {
            final_recipient_seen = true;
            classified_arguments.extend_from_slice(input.forwarded_arguments);
        }
        let adapter = match_adapter(input.matrix, package_name, version, &classified_arguments)?;
        if is_final_recipient
            && !input.forwarded_arguments.is_empty()
            && !adapter.rule.supports_forwarded_arguments
        {
            return Err(Reason::ArgumentDenied);
        }

        if !occurrence.potential_lifecycle
            && matches!(
                adapter.classification,
                Classification::Controlled | Classification::Disclosed
            )
        {
            target_work_seen = true;
        }
        if let Some(disclosure) = &adapter.disclosure {
            if !disclosures.contains(&disclosure.identifier) {
                disclosures.push(disclosure.identifier.clone());
            }
        }
        if let ControlDecision::RequiresSuffix(suffix) = &adapter.control {
            if is_final_recipient {
                injection_arguments.extend(suffix.iter().cloned());
            } else {
                missing_nonfinal_control = true;
            }
        }
        leaves.push(PolicyLeaf {
            script_key: occurrence.script_key.clone(),
            phase: occurrence.phase,
            classification: adapter.classification,
            potential_lifecycle: occurrence.potential_lifecycle,
            wrapper,
            control: adapter.control,
        });
    }

    if !input.forwarded_arguments.is_empty() && !final_recipient_seen {
        return Err(Reason::NonfinalInjectionRequired);
    }
    if missing_nonfinal_control {
        return Err(Reason::NonfinalInjectionRequired);
    }
    if !target_work_seen {
        return Err(Reason::OperationUnsupported);
    }
    injection_arguments.extend_from_slice(input.forwarded_arguments);
    let launch = build_launch_array(
        input.package_manager,
        &input.operation.script,
        workspace_package_name,
        &injection_arguments,
    )?;

    Ok(OperationPolicy {
        target: input.target,
        operation_key: input.operation_key.to_owned(),
        script_key: input.operation.script.clone(),
        timeout_seconds: input.operation.timeout_seconds,
        graph_depth: input
            .graph
            .leaves
            .iter()
            .map(|leaf| leaf.depth)
            .max()
            .unwrap_or(0),
        leaves,
        launch,
        disclosures,
        evidence_files,
    })
}

fn unwrap_policy_segment(
    matrix: &AdapterMatrix,
    segment: &crate::script::tokenizer::CommandSegment,
    installed_versions: &BTreeMap<String, Version>,
) -> Result<(Vec<String>, Option<WrapperEvidence>), Reason> {
    let arguments = segment.arguments();
    let executable = arguments.first().ok_or(Reason::ToolUnsupported)?;
    let identity = if matches!(executable.as_str(), "cross-env" | "dotenv") {
        let package_name =
            package_for_executable(matrix, executable).map_err(|_| Reason::WrapperUnsupported)?;
        let version = installed_versions
            .get(package_name)
            .ok_or(Reason::WrapperUnsupported)?;
        Some(WrapperIdentity::new(package_name, version.clone()))
    } else {
        None
    };
    let unwrapped = unwrap_segment(segment, identity.as_ref())?;
    Ok((unwrapped.arguments().to_vec(), unwrapped.evidence()))
}

fn normalize_evidence_files(files: &[String]) -> Result<Vec<String>, Reason> {
    let mut normalized = BTreeSet::new();
    for file in files {
        if !valid_relative_path(file) {
            return Err(Reason::InvalidConfig);
        }
        normalized.insert(file.clone());
    }
    Ok(normalized.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::{OperationPolicy, PolicyInput, PolicyTarget, build_operation_policy};
    use crate::{
        adapter::{Classification, ControlDecision, load_embedded_matrix},
        configuration::OperationConfig,
        package_manager::LaunchArray,
        repository::PackageManagerKind,
        result::Reason,
        script::{graph::expand_script_graph, wrapper::WrapperKind},
    };
    use semver::Version;
    use std::collections::BTreeMap;

    fn scripts(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn versions(entries: &[(&str, &str)]) -> BTreeMap<String, Version> {
        entries
            .iter()
            .map(|(name, version)| ((*name).to_owned(), Version::parse(version).unwrap()))
            .collect()
    }

    fn build(
        target: PolicyTarget,
        operation: &OperationConfig,
        scripts: &BTreeMap<String, String>,
        package_manager: PackageManagerKind,
        installed_versions: &BTreeMap<String, Version>,
        forwarded_arguments: &[String],
        evidence_files: &[String],
    ) -> Result<OperationPolicy, Reason> {
        let graph = expand_script_graph(&operation.script, scripts).unwrap();
        let matrix = load_embedded_matrix().unwrap();
        let package_manager_version = match package_manager {
            PackageManagerKind::Npm => Version::parse("12.0.2").unwrap(),
            PackageManagerKind::Pnpm => Version::parse("11.25.0").unwrap(),
        };
        build_operation_policy(PolicyInput {
            target,
            operation_key: &operation.script,
            operation,
            graph: &graph,
            matrix: &matrix,
            package_manager,
            package_manager_version: &package_manager_version,
            installed_versions,
            forwarded_arguments,
            evidence_files,
        })
    }

    #[test]
    fn assembles_controlled_disclosed_and_auxiliary_leaves() {
        let operation = OperationConfig {
            script: "build".into(),
            timeout_seconds: 900,
        };
        let policy = build(
            PolicyTarget::Root,
            &operation,
            &scripts(&[(
                "build",
                "rimraf dist && vitest run --no-file-parallelism --maxWorkers=1 && next build",
            )]),
            PackageManagerKind::Npm,
            &versions(&[
                ("rimraf", "6.1.3"),
                ("vitest", "4.1.11"),
                ("next", "16.3.4"),
            ]),
            &[],
            &["package.json".into()],
        )
        .unwrap();

        assert_eq!(
            policy
                .leaves
                .iter()
                .map(|leaf| leaf.classification)
                .collect::<Vec<_>>(),
            [
                Classification::Auxiliary,
                Classification::Controlled,
                Classification::Disclosed,
            ]
        );
        assert_eq!(policy.disclosures, ["internal-fanout-uncontrolled"]);
    }

    #[test]
    fn rejects_missing_controls_outside_the_final_top_level_leaf() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 900,
        };
        for script_map in [
            scripts(&[("test", "vitest run && next build")]),
            scripts(&[("pretest", "vitest run"), ("test", "next build")]),
            scripts(&[("test", "npm run nested"), ("nested", "vitest run")]),
        ] {
            assert_eq!(
                build(
                    PolicyTarget::Root,
                    &operation,
                    &script_map,
                    PackageManagerKind::Npm,
                    &versions(&[("vitest", "4.1.11"), ("next", "16.3.4")]),
                    &[],
                    &[],
                )
                .unwrap_err(),
                Reason::NonfinalInjectionRequired
            );
        }
    }

    #[test]
    fn injects_only_into_the_exact_workspace_final_leaf() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 600,
        };
        let policy = build(
            PolicyTarget::Workspace {
                key: "web".into(),
                package_name: "@acme/web".into(),
            },
            &operation,
            &scripts(&[("test", "rimraf dist && vitest run")]),
            PackageManagerKind::Pnpm,
            &versions(&[("rimraf", "6.1.3"), ("vitest", "4.1.11")]),
            &["src/unit.test.ts".into()],
            &["apps/web/package.json".into()],
        )
        .unwrap();

        assert_eq!(
            policy.launch,
            LaunchArray::new(
                "pnpm",
                [
                    "--config.script-shell=/bin/sh",
                    "--config.shell-emulator=false",
                    "--filter",
                    "@acme/web",
                    "--fail-if-no-match",
                    "run",
                    "test",
                    "--",
                    "--no-file-parallelism",
                    "--maxWorkers=1",
                    "src/unit.test.ts",
                ],
            )
        );
        assert_eq!(
            policy.leaves[1].control,
            ControlDecision::RequiresSuffix(vec![
                "--no-file-parallelism".into(),
                "--maxWorkers=1".into(),
            ])
        );
    }

    #[test]
    fn preserves_already_controlled_final_leaves_idempotently() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 600,
        };
        let policy = build(
            PolicyTarget::Root,
            &operation,
            &scripts(&[("test", "vitest run --no-file-parallelism --maxWorkers=1")]),
            PackageManagerKind::Npm,
            &versions(&[("vitest", "4.1.11")]),
            &[],
            &[],
        )
        .unwrap();

        assert_eq!(policy.leaves[0].control, ControlDecision::AlreadyControlled);
        assert_eq!(
            policy.launch,
            LaunchArray::new("npm", ["--script-shell=/bin/sh", "run", "test"])
        );
    }

    #[test]
    fn denial_precedes_a_possible_final_suffix() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 600,
        };
        assert_eq!(
            build(
                PolicyTarget::Root,
                &operation,
                &scripts(&[("test", "vitest run")]),
                PackageManagerKind::Npm,
                &versions(&[("vitest", "4.1.11")]),
                &["--watch".into()],
                &[],
            )
            .unwrap_err(),
            Reason::WatchDenied
        );
    }

    #[test]
    fn does_not_forward_arguments_when_the_target_ends_in_a_reference() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 600,
        };
        assert_eq!(
            build(
                PolicyTarget::Root,
                &operation,
                &scripts(&[
                    (
                        "test",
                        "vitest run --no-file-parallelism --maxWorkers=1 && npm run nested",
                    ),
                    ("nested", "next build"),
                ]),
                PackageManagerKind::Npm,
                &versions(&[("vitest", "4.1.11"), ("next", "16.3.4")]),
                &["secret-forwarded-value".into()],
                &[],
            )
            .unwrap_err(),
            Reason::NonfinalInjectionRequired
        );
    }

    #[test]
    fn rejects_auxiliary_only_targets_and_forwarding_to_auxiliary() {
        let operation = OperationConfig {
            script: "build".into(),
            timeout_seconds: 600,
        };
        assert_eq!(
            build(
                PolicyTarget::Root,
                &operation,
                &scripts(&[("build", "rimraf dist")]),
                PackageManagerKind::Npm,
                &versions(&[("rimraf", "6.1.3")]),
                &[],
                &[],
            )
            .unwrap_err(),
            Reason::OperationUnsupported
        );

        assert_eq!(
            build(
                PolicyTarget::Root,
                &operation,
                &scripts(&[(
                    "build",
                    "vitest run --no-file-parallelism --maxWorkers=1 && rimraf dist",
                )]),
                PackageManagerKind::Npm,
                &versions(&[("vitest", "4.1.11"), ("rimraf", "6.1.3")]),
                &["extra".into()],
                &[],
            )
            .unwrap_err(),
            Reason::ArgumentDenied
        );
    }

    #[test]
    fn redacted_summary_omits_paths_scripts_assignments_and_dotenv_files() {
        let operation = OperationConfig {
            script: "test".into(),
            timeout_seconds: 600,
        };
        let raw_script = "dotenv -e .env.private -- vitest run";
        let policy = build(
            PolicyTarget::Root,
            &operation,
            &scripts(&[("test", raw_script)]),
            PackageManagerKind::Npm,
            &versions(&[("dotenv-cli", "11.0.0"), ("vitest", "4.1.11")]),
            &[],
            &[
                "package.json".into(),
                "node_modules/vitest/package.json".into(),
            ],
        )
        .unwrap();

        assert_eq!(policy.leaves[0].wrapper.unwrap().kind, WrapperKind::Dotenv);
        let json = serde_json::to_string(&policy.redacted_summary()).unwrap();
        let debug = format!("{policy:?}");
        for forbidden in [
            "/Users/example/private/repository",
            raw_script,
            "SECRET=value",
            ".env.private",
        ] {
            assert!(!json.contains(forbidden));
            assert!(!debug.contains(forbidden));
        }
    }
}
