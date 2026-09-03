use crate::{
    process::run_captured,
    repository::find_git_repository,
    result::{ExitResult, Origin, Reason},
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const MAX_GIT_CONFIG_BYTES: u64 = 262_144;
const MAX_API_RESPONSE_BYTES: usize = 262_144;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubInspectReport {
    pub schema_version: u8,
    pub repository: String,
    pub workflow_count: usize,
    pub inspected_workflow_count: usize,
    pub active_workflow_count: usize,
    pub recommendations: Vec<&'static str>,
    pub result: ExitResult,
}

#[derive(Debug, Deserialize)]
struct WorkflowsResponse {
    total_count: usize,
    workflows: Vec<Workflow>,
}

#[derive(Debug, Deserialize)]
struct Workflow {
    state: String,
}

pub fn inspect(start: &Path) -> Result<GithubInspectReport, ExitResult> {
    let repository = find_git_repository(start)
        .map_err(|_| unsupported(Reason::RepositoryUnsupported))?
        .ok_or_else(|| unsupported(Reason::RepositoryUnsupported))?;
    let config_path = repository.metadata().join("config");
    let metadata =
        fs::metadata(&config_path).map_err(|_| unsupported(Reason::RepositoryUnsupported))?;
    if !metadata.is_file() || metadata.len() > MAX_GIT_CONFIG_BYTES {
        return Err(unsupported(Reason::RepositoryUnsupported));
    }
    let config =
        fs::read_to_string(config_path).map_err(|_| unsupported(Reason::RepositoryUnsupported))?;
    let identity =
        origin_identity(&config).ok_or_else(|| unsupported(Reason::RepositoryUnsupported))?;
    let endpoint = format!("repos/{identity}/actions/workflows?per_page=100");
    let arguments = vec![
        "api".to_owned(),
        "--method".to_owned(),
        "GET".to_owned(),
        endpoint,
        "--jq".to_owned(),
        "{total_count: .total_count, workflows: [.workflows[] | {state: .state}]}".to_owned(),
    ];
    let output = run_captured("gh", &arguments, repository.root()).map_err(command_failure)?;
    if !output.status.success() {
        let code = output.status.code().filter(|code| *code > 0).unwrap_or(1);
        return Err(ExitResult::new(Origin::Child, code, Reason::ChildExit));
    }
    if output.stdout.len() > MAX_API_RESPONSE_BYTES {
        return Err(ExitResult::new(Origin::Internal, 70, Reason::InternalError));
    }
    let response: WorkflowsResponse = serde_json::from_slice(&output.stdout)
        .map_err(|_| ExitResult::new(Origin::Internal, 70, Reason::InternalError))?;
    let inspected_workflow_count = response.workflows.len();
    let active_workflow_count = response
        .workflows
        .iter()
        .filter(|workflow| workflow.state == "active")
        .count();
    Ok(GithubInspectReport {
        schema_version: 1,
        repository: identity,
        workflow_count: response.total_count,
        inspected_workflow_count,
        active_workflow_count,
        recommendations: vec![
            "run focused checks locally through agent-lowmem",
            "keep broad build and test workflows on GitHub-hosted runners",
        ],
        result: ExitResult::new(Origin::Child, 0, Reason::Completed),
    })
}

pub fn render_human(report: &GithubInspectReport) -> String {
    format!(
        "GitHub repository: {}\nWorkflows: {} total, {} inspected, {} active\nRecommendations:\n- {}\n- {}",
        report.repository,
        report.workflow_count,
        report.inspected_workflow_count,
        report.active_workflow_count,
        report.recommendations[0],
        report.recommendations[1],
    )
}

fn unsupported(reason: Reason) -> ExitResult {
    ExitResult::new(Origin::Preflight, 64, reason)
}

fn command_failure(reason: Reason) -> ExitResult {
    match reason {
        Reason::ToolUnsupported => unsupported(reason),
        Reason::DeadlineExceeded => ExitResult::new(Origin::SupervisorTimeout, 124, reason),
        _ => ExitResult::new(Origin::Internal, 70, Reason::InternalError),
    }
}

fn origin_identity(config: &str) -> Option<String> {
    let mut in_origin = false;
    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            in_origin = line == r#"[remote "origin"]"#;
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some(value) = line.strip_prefix("url") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        return github_identity(value.trim());
    }
    None
}

fn github_identity(remote: &str) -> Option<String> {
    let path = remote
        .strip_prefix("git@github.com:")
        .or_else(|| remote.strip_prefix("https://github.com/"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repository) = path.split_once('/')?;
    if owner.is_empty()
        || repository.is_empty()
        || repository.contains('/')
        || !owner.chars().all(valid_identity_character)
        || !repository.chars().all(valid_identity_character)
    {
        return None;
    }
    Some(format!("{owner}/{repository}"))
}

fn valid_identity_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use super::{command_failure, origin_identity};
    use crate::result::{ExitResult, Origin, Reason};

    #[test]
    fn origin_url_is_found_even_when_other_remote_keys_come_first() {
        let config = concat!(
            "[remote \"origin\"]\n",
            "\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            "\turl = https://github.com/Pleo2/agent-lowmem.git\n",
        );

        assert_eq!(
            origin_identity(config).as_deref(),
            Some("Pleo2/agent-lowmem")
        );
    }

    #[test]
    fn command_failures_keep_the_closed_result_mapping() {
        assert_eq!(
            command_failure(Reason::ToolUnsupported),
            ExitResult::new(Origin::Preflight, 64, Reason::ToolUnsupported)
        );
        assert_eq!(
            command_failure(Reason::DeadlineExceeded),
            ExitResult::new(Origin::SupervisorTimeout, 124, Reason::DeadlineExceeded)
        );
        assert_eq!(
            command_failure(Reason::InternalError),
            ExitResult::new(Origin::Internal, 70, Reason::InternalError)
        );
    }
}
