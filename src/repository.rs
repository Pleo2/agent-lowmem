use crate::{
    adapter::{
        AdapterMatrix, load_embedded_matrix, match_package_manager, package_for_executable,
        resolve_installed_package,
    },
    configuration::{AgentLowmemConfig, OperationConfig, parse_config},
    package_manager::{
        NodeVersionEvidence, inspect_node_version, inspect_npmrc, inspect_pnpm_settings,
    },
    policy::{PolicyInput, PolicyTarget, build_operation_policy},
    result::Reason,
    script::{
        graph::expand_script_graph,
        wrapper::{WrapperIdentity, unwrap_segment},
    },
    workspace::{
        PnpmWorkspaceDocument, WorkspaceCandidate, expand_workspace_patterns, parse_npm_workspaces,
        parse_pnpm_workspace, resolve_configured_workspace,
    },
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRoot(PathBuf);

impl GitRoot {
    fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryError {
    Io(io::ErrorKind),
    InvalidGitPointer,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageManagerKind {
    Npm,
    Pnpm,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageManagerReport {
    pub kind: PackageManagerKind,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReport {
    pub git_root_available: bool,
    pub root_package_available: bool,
    pub package_manager: Option<PackageManagerReport>,
    pub operations: Vec<OperationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<Reason>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
    pub operation_key: String,
    pub status: OperationStatus,
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStatus {
    Runnable,
    Rejected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootPackageManifest {
    package_manager: Option<String>,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

pub fn find_git_root(start: &Path) -> Result<Option<GitRoot>, RepositoryError> {
    let canonical_start = fs::canonicalize(start).map_err(repository_io_error)?;
    let first_directory = if canonical_start.is_dir() {
        canonical_start
    } else {
        canonical_start
            .parent()
            .ok_or(RepositoryError::InvalidGitPointer)?
            .to_path_buf()
    };

    for candidate in first_directory.ancestors() {
        let marker = candidate.join(".git");
        let metadata = match fs::symlink_metadata(&marker) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(repository_io_error(source)),
        };

        if metadata.is_dir() {
            return Ok(Some(GitRoot(candidate.to_path_buf())));
        }

        if metadata.is_file() && valid_git_pointer(candidate, &marker)? {
            return Ok(Some(GitRoot(candidate.to_path_buf())));
        }

        return Err(RepositoryError::InvalidGitPointer);
    }

    Ok(None)
}

pub fn inspect_repository(start: &Path) -> RepositoryReport {
    let root = match find_git_root(start) {
        Ok(Some(root)) => root,
        Ok(None) | Err(_) => return unsupported_repository(false, false),
    };
    let package_path = root.as_path().join("package.json");
    match regular_file_state(&package_path) {
        Some(true) => {}
        Some(false) => return unsupported_repository(true, false),
        None => return unsupported_repository(true, true),
    }

    let package_bytes = match fs::read(&package_path) {
        Ok(bytes) => bytes,
        Err(_) => return unsupported_repository(true, true),
    };
    let manifest: RootPackageManifest = match serde_json::from_slice(&package_bytes) {
        Ok(manifest) => manifest,
        Err(_) => return unsupported_repository(true, true),
    };
    let package_manager = match manifest
        .package_manager
        .as_deref()
        .and_then(parse_package_manager)
    {
        Some(package_manager) => package_manager,
        None => return unsupported_package_manager(),
    };

    let npm_lock = match regular_file_state(&root.as_path().join("package-lock.json")) {
        Some(present) => present,
        None => return unsupported_repository(true, true),
    };
    let pnpm_lock = match regular_file_state(&root.as_path().join("pnpm-lock.yaml")) {
        Some(present) => present,
        None => return unsupported_repository(true, true),
    };
    let lockfile_matches = match package_manager.kind {
        PackageManagerKind::Npm => npm_lock && !pnpm_lock,
        PackageManagerKind::Pnpm => pnpm_lock && !npm_lock,
    };
    if !lockfile_matches {
        return unsupported_package_manager();
    }

    let matrix = match load_embedded_matrix() {
        Ok(matrix) => matrix,
        Err(reason) => return failed_repository(Some(package_manager), reason),
    };
    if let Err(reason) = match_package_manager(&matrix, &package_manager) {
        return failed_repository(Some(package_manager), reason);
    }

    let pnpm_document = match inspect_repository_shell(root.as_path(), package_manager.kind) {
        Ok(document) => document,
        Err(reason) => return failed_repository(Some(package_manager), reason),
    };
    let patterns = match package_manager.kind {
        PackageManagerKind::Npm => parse_npm_workspaces(&package_bytes),
        PackageManagerKind::Pnpm => Ok(pnpm_document
            .as_ref()
            .map(|document| document.patterns.clone())
            .unwrap_or_default()),
    };
    let patterns = match patterns {
        Ok(patterns) => patterns,
        Err(error) => return failed_repository(Some(package_manager), error.reason()),
    };
    let candidates = match expand_workspace_patterns(root.as_path(), &patterns) {
        Ok(candidates) => candidates,
        Err(error) => return failed_repository(Some(package_manager), error.reason()),
    };

    let config = match read_optional_file(root.as_path(), ".agent-lowmem.json") {
        Ok(Some(bytes)) => match parse_config(&bytes) {
            Ok(config) => Some(config),
            Err(error) => {
                return failed_repository(Some(package_manager), error.reason());
            }
        },
        Ok(None) => None,
        Err(reason) => return failed_repository(Some(package_manager), reason),
    };
    if config
        .as_ref()
        .is_some_and(|config| config.package_manager != package_manager.kind)
    {
        return failed_repository(Some(package_manager), Reason::InvalidConfig);
    }

    let node_evidence = inspect_repository_node_version(root.as_path());
    let operations = collect_operation_summaries(OperationCollectionInput {
        root: root.as_path(),
        root_scripts: &manifest.scripts,
        package_manager: &package_manager,
        matrix: &matrix,
        config: config.as_ref(),
        candidates: &candidates,
        node_evidence: &node_evidence,
    });

    RepositoryReport {
        git_root_available: true,
        root_package_available: true,
        package_manager: Some(package_manager),
        operations,
        failure_reason: None,
    }
}

struct RepositoryNodeEvidence {
    version: NodeVersionEvidence,
    files: Vec<String>,
}

struct OperationCollectionInput<'a> {
    root: &'a Path,
    root_scripts: &'a BTreeMap<String, String>,
    package_manager: &'a PackageManagerReport,
    matrix: &'a AdapterMatrix,
    config: Option<&'a AgentLowmemConfig>,
    candidates: &'a [WorkspaceCandidate],
    node_evidence: &'a Result<Option<RepositoryNodeEvidence>, Reason>,
}

struct OperationAnalysisInput<'a> {
    root: &'a Path,
    selected_package: &'a Path,
    target: PolicyTarget,
    operation_key: &'a str,
    operation: &'a OperationConfig,
    scripts: &'a BTreeMap<String, String>,
    package_manager: &'a PackageManagerReport,
    matrix: &'a AdapterMatrix,
    node_evidence: &'a Result<Option<RepositoryNodeEvidence>, Reason>,
    configured: bool,
    evidence_files: Vec<String>,
}

fn inspect_repository_shell(
    root: &Path,
    package_manager: PackageManagerKind,
) -> Result<Option<PnpmWorkspaceDocument>, Reason> {
    match package_manager {
        PackageManagerKind::Npm => {
            let npmrc = read_optional_evidence(root, ".npmrc", Reason::ScriptShellUnsupported)?;
            inspect_npmrc(npmrc.as_deref())?;
            Ok(None)
        }
        PackageManagerKind::Pnpm => {
            let workspace =
                read_optional_evidence(root, "pnpm-workspace.yaml", Reason::WorkspaceUnsupported)?;
            let Some(workspace) = workspace else {
                return Ok(None);
            };
            let document = parse_pnpm_workspace(&workspace).map_err(|error| error.reason())?;
            inspect_pnpm_settings(&document)?;
            Ok(Some(document))
        }
    }
}

fn inspect_repository_node_version(root: &Path) -> Result<Option<RepositoryNodeEvidence>, Reason> {
    let node_version =
        read_optional_evidence(root, ".node-version", Reason::ToolVersionUnsupported)?;
    let nvmrc = read_optional_evidence(root, ".nvmrc", Reason::ToolVersionUnsupported)?;
    let Some(version) = inspect_node_version(node_version.as_deref(), nvmrc.as_deref())? else {
        return Ok(None);
    };
    let mut files = Vec::new();
    if node_version.is_some() {
        files.push(".node-version".to_owned());
    }
    if nvmrc.is_some() {
        files.push(".nvmrc".to_owned());
    }
    Ok(Some(RepositoryNodeEvidence { version, files }))
}

fn collect_operation_summaries(input: OperationCollectionInput<'_>) -> Vec<OperationSummary> {
    let mut summaries = Vec::new();
    match input.config {
        Some(config) => {
            for (operation_key, operation) in &config.operations {
                summaries.push(analyze_operation(OperationAnalysisInput {
                    root: input.root,
                    selected_package: input.root,
                    target: PolicyTarget::Root,
                    operation_key,
                    operation,
                    scripts: input.root_scripts,
                    package_manager: input.package_manager,
                    matrix: input.matrix,
                    node_evidence: input.node_evidence,
                    configured: true,
                    evidence_files: base_evidence_files(
                        input.package_manager.kind,
                        true,
                        input.root,
                    ),
                }));
            }
            for (workspace_key, workspace) in &config.workspaces {
                let candidate = resolve_configured_workspace(workspace, input.candidates);
                let candidate = match candidate {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        for operation_key in workspace.operations.keys() {
                            summaries.push(rejected_operation(
                                Some(workspace_key),
                                operation_key,
                                true,
                                error.reason(),
                            ));
                        }
                        continue;
                    }
                };
                let selected_package = input.root.join(&candidate.relative_path);
                let scripts = match read_package_scripts(&selected_package) {
                    Ok(scripts) => scripts,
                    Err(reason) => {
                        for operation_key in workspace.operations.keys() {
                            summaries.push(rejected_operation(
                                Some(workspace_key),
                                operation_key,
                                true,
                                reason,
                            ));
                        }
                        continue;
                    }
                };
                for (operation_key, operation) in &workspace.operations {
                    let mut evidence =
                        base_evidence_files(input.package_manager.kind, true, input.root);
                    evidence.push(format!("{}/package.json", candidate.relative_path));
                    summaries.push(analyze_operation(OperationAnalysisInput {
                        root: input.root,
                        selected_package: &selected_package,
                        target: PolicyTarget::Workspace {
                            key: workspace_key.clone(),
                            package_name: candidate.package_name.clone(),
                        },
                        operation_key,
                        operation,
                        scripts: &scripts,
                        package_manager: input.package_manager,
                        matrix: input.matrix,
                        node_evidence: input.node_evidence,
                        configured: true,
                        evidence_files: evidence,
                    }));
                }
            }
        }
        None => {
            for operation_key in ["build", "lint", "test", "typecheck"] {
                if input.root_scripts.contains_key(operation_key) {
                    let operation = OperationConfig {
                        script: operation_key.to_owned(),
                        timeout_seconds: 900,
                    };
                    summaries.push(analyze_operation(OperationAnalysisInput {
                        root: input.root,
                        selected_package: input.root,
                        target: PolicyTarget::Root,
                        operation_key,
                        operation: &operation,
                        scripts: input.root_scripts,
                        package_manager: input.package_manager,
                        matrix: input.matrix,
                        node_evidence: input.node_evidence,
                        configured: false,
                        evidence_files: base_evidence_files(
                            input.package_manager.kind,
                            false,
                            input.root,
                        ),
                    }));
                }
            }
        }
    }
    summaries.sort_by(|left, right| {
        left.workspace_key
            .cmp(&right.workspace_key)
            .then_with(|| left.operation_key.cmp(&right.operation_key))
    });
    summaries
}

fn analyze_operation(input: OperationAnalysisInput<'_>) -> OperationSummary {
    match try_analyze_operation(&input) {
        Ok(disclosures) => OperationSummary {
            workspace_key: workspace_key(&input.target).map(str::to_owned),
            operation_key: input.operation_key.to_owned(),
            status: OperationStatus::Runnable,
            configured: input.configured,
            reason: None,
            disclosures,
        },
        Err(reason) => rejected_operation(
            workspace_key(&input.target),
            input.operation_key,
            input.configured,
            reason,
        ),
    }
}

fn try_analyze_operation(input: &OperationAnalysisInput<'_>) -> Result<Vec<String>, Reason> {
    let graph = expand_script_graph(&input.operation.script, input.scripts)?;
    let mut installed_versions = BTreeMap::new();
    let mut evidence_files = input.evidence_files.clone();

    for leaf in &graph.leaves {
        let initial_executable = leaf
            .segment
            .arguments()
            .first()
            .ok_or(Reason::ToolUnsupported)?;
        let wrapper_identity = if matches!(initial_executable.as_str(), "cross-env" | "dotenv") {
            let package_name = package_for_executable(input.matrix, initial_executable)
                .map_err(|_| Reason::WrapperUnsupported)?;
            record_installed_package(
                input.root,
                input.selected_package,
                package_name,
                &mut installed_versions,
                &mut evidence_files,
            )?;
            Some(WrapperIdentity::new(
                package_name,
                installed_versions[package_name].clone(),
            ))
        } else {
            None
        };
        let unwrapped = unwrap_segment(&leaf.segment, wrapper_identity.as_ref())?;
        let executable = unwrapped
            .arguments()
            .first()
            .ok_or(Reason::ToolUnsupported)?;
        let package_name = package_for_executable(input.matrix, executable)?;
        if package_name == "node" {
            let node = input
                .node_evidence
                .as_ref()
                .map_err(|reason| *reason)?
                .as_ref()
                .ok_or(Reason::ToolVersionUnsupported)?;
            installed_versions.insert("node".to_owned(), node.version.0.clone());
            evidence_files.extend(node.files.iter().cloned());
        } else {
            record_installed_package(
                input.root,
                input.selected_package,
                package_name,
                &mut installed_versions,
                &mut evidence_files,
            )?;
        }
    }

    let package_manager_version = Version::parse(&input.package_manager.version)
        .map_err(|_| Reason::PackageManagerUnsupported)?;
    let policy = build_operation_policy(PolicyInput {
        target: input.target.clone(),
        operation_key: input.operation_key,
        operation: input.operation,
        graph: &graph,
        matrix: input.matrix,
        package_manager: input.package_manager.kind,
        package_manager_version: &package_manager_version,
        installed_versions: &installed_versions,
        forwarded_arguments: &[],
        evidence_files: &evidence_files,
    })?;
    Ok(policy.disclosures)
}

fn record_installed_package(
    root: &Path,
    selected_package: &Path,
    package_name: &str,
    versions: &mut BTreeMap<String, Version>,
    evidence_files: &mut Vec<String>,
) -> Result<(), Reason> {
    if versions.contains_key(package_name) {
        return Ok(());
    }
    let package = resolve_installed_package(root, selected_package, package_name)?;
    evidence_files.push(package.evidence_file);
    versions.insert(package.package_name, package.version);
    Ok(())
}

fn read_package_scripts(directory: &Path) -> Result<BTreeMap<String, String>, Reason> {
    let bytes =
        fs::read(directory.join("package.json")).map_err(|_| Reason::WorkspaceUnsupported)?;
    serde_json::from_slice::<RootPackageManifest>(&bytes)
        .map(|manifest| manifest.scripts)
        .map_err(|_| Reason::WorkspaceUnsupported)
}

fn base_evidence_files(
    package_manager: PackageManagerKind,
    configured: bool,
    root: &Path,
) -> Vec<String> {
    let mut files = vec![
        "package.json".to_owned(),
        match package_manager {
            PackageManagerKind::Npm => "package-lock.json".to_owned(),
            PackageManagerKind::Pnpm => "pnpm-lock.yaml".to_owned(),
        },
    ];
    if configured {
        files.push(".agent-lowmem.json".to_owned());
    }
    if root.join(".npmrc").is_file() {
        files.push(".npmrc".to_owned());
    }
    if root.join("pnpm-workspace.yaml").is_file() {
        files.push("pnpm-workspace.yaml".to_owned());
    }
    files
}

fn workspace_key(target: &PolicyTarget) -> Option<&str> {
    match target {
        PolicyTarget::Root => None,
        PolicyTarget::Workspace { key, .. } => Some(key),
    }
}

fn rejected_operation(
    workspace_key: Option<&str>,
    operation_key: &str,
    configured: bool,
    reason: Reason,
) -> OperationSummary {
    OperationSummary {
        workspace_key: workspace_key.map(str::to_owned),
        operation_key: operation_key.to_owned(),
        status: OperationStatus::Rejected,
        configured,
        reason: Some(reason),
        disclosures: Vec::new(),
    }
}

fn read_optional_file(root: &Path, name: &str) -> Result<Option<Vec<u8>>, Reason> {
    read_optional_evidence(root, name, Reason::RepositoryUnsupported)
}

fn read_optional_evidence(
    root: &Path,
    name: &str,
    failure: Reason,
) -> Result<Option<Vec<u8>>, Reason> {
    let path = root.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(failure);
    }
    fs::read(path).map(Some).map_err(|_| failure)
}

fn regular_file_state(path: &Path) -> Option<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => None,
        Ok(metadata) => Some(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}

fn valid_git_pointer(root: &Path, marker: &Path) -> Result<bool, RepositoryError> {
    let contents = fs::read_to_string(marker).map_err(repository_io_error)?;
    let mut lines = contents.lines();
    let Some(gitdir) = lines.next().and_then(|line| line.strip_prefix("gitdir: ")) else {
        return Ok(false);
    };
    if gitdir.is_empty() || lines.next().is_some() {
        return Ok(false);
    }

    let gitdir_path = Path::new(gitdir);
    let resolved = if gitdir_path.is_absolute() {
        gitdir_path.to_path_buf()
    } else {
        root.join(gitdir_path)
    };
    Ok(resolved.is_dir())
}

fn parse_package_manager(declaration: &str) -> Option<PackageManagerReport> {
    let (name, version) = declaration.split_once('@')?;
    let kind = match name {
        "npm" => PackageManagerKind::Npm,
        "pnpm" => PackageManagerKind::Pnpm,
        _ => return None,
    };
    let version = Version::parse(version).ok()?.to_string();

    Some(PackageManagerReport { kind, version })
}

fn unsupported_repository(
    git_root_available: bool,
    root_package_available: bool,
) -> RepositoryReport {
    RepositoryReport {
        git_root_available,
        root_package_available,
        package_manager: None,
        operations: Vec::new(),
        failure_reason: Some(Reason::RepositoryUnsupported),
    }
}

fn unsupported_package_manager() -> RepositoryReport {
    RepositoryReport {
        git_root_available: true,
        root_package_available: true,
        package_manager: None,
        operations: Vec::new(),
        failure_reason: Some(Reason::PackageManagerUnsupported),
    }
}

fn failed_repository(
    package_manager: Option<PackageManagerReport>,
    reason: Reason,
) -> RepositoryReport {
    RepositoryReport {
        git_root_available: true,
        root_package_available: true,
        package_manager,
        operations: Vec::new(),
        failure_reason: Some(reason),
    }
}

fn repository_io_error(source: io::Error) -> RepositoryError {
    RepositoryError::Io(source.kind())
}

#[cfg(test)]
mod tests {
    use super::{PackageManagerKind, find_git_root, inspect_repository};
    use crate::result::Reason;
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn empty() -> Self {
            let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("agent-lowmem-repository-test-{timestamp}-{id}"));
            fs::create_dir_all(&root).unwrap();
            Self {
                root: fs::canonicalize(root).unwrap(),
            }
        }

        fn git_repo() -> Self {
            let fixture = Self::empty();
            fs::create_dir(fixture.root.join(".git")).unwrap();
            fixture
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn mkdir(&self, relative: &str) {
            fs::create_dir_all(self.root.join(relative)).unwrap();
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn detects_pnpm_from_manifest_and_matching_lockfile_without_exposing_root() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"pnpm@11.25.0"}"#);
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert!(report.git_root_available);
        let manager = report.package_manager.as_ref().unwrap();
        assert_eq!(manager.kind, PackageManagerKind::Pnpm);
        assert_eq!(manager.version.to_string(), "11.25.0");
        assert_eq!(report.failure_reason, None);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains(fixture.path().to_str().unwrap())
        );
    }

    #[test]
    fn rejects_a_declared_manager_with_the_wrong_lockfile() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"npm@12.0.2"}"#);
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert_eq!(
            report.failure_reason,
            Some(Reason::PackageManagerUnsupported)
        );
    }

    #[test]
    fn reports_when_no_git_root_exists() {
        let fixture = Fixture::empty();

        let report = inspect_repository(fixture.path());

        assert!(!report.git_root_available);
        assert!(!report.root_package_available);
        assert_eq!(report.failure_reason, Some(Reason::RepositoryUnsupported));
    }

    #[test]
    fn walks_parents_to_find_a_git_directory() {
        let fixture = Fixture::git_repo();
        fixture.mkdir("packages/web/src");

        let root = find_git_root(&fixture.path().join("packages/web/src"))
            .unwrap()
            .unwrap();

        assert_eq!(root.as_path(), fixture.path());
    }

    #[test]
    fn accepts_a_valid_worktree_git_pointer() {
        let fixture = Fixture::empty();
        fixture.mkdir("git-data");
        fixture.write(".git", "gitdir: git-data\n");
        fixture.write("package.json", r#"{"packageManager":"npm@12.0.2"}"#);
        fixture.write("package-lock.json", "{}\n");

        let report = inspect_repository(fixture.path());

        assert!(report.git_root_available);
        assert_eq!(
            report.package_manager.unwrap().kind,
            PackageManagerKind::Npm
        );
    }

    #[test]
    fn rejects_malformed_root_package_json() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", "{not-json}");

        let report = inspect_repository(fixture.path());

        assert!(report.root_package_available);
        assert_eq!(report.failure_reason, Some(Reason::RepositoryUnsupported));
    }

    #[test]
    fn rejects_a_missing_root_package() {
        let fixture = Fixture::git_repo();

        let report = inspect_repository(fixture.path());

        assert!(!report.root_package_available);
        assert_eq!(report.failure_reason, Some(Reason::RepositoryUnsupported));
    }

    #[test]
    fn rejects_a_package_manager_without_a_version() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"pnpm"}"#);
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert_eq!(
            report.failure_reason,
            Some(Reason::PackageManagerUnsupported)
        );
    }

    #[test]
    fn rejects_ambiguous_npm_and_pnpm_lockfiles() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"npm@12.0.2"}"#);
        fixture.write("package-lock.json", "{}\n");
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert_eq!(
            report.failure_reason,
            Some(Reason::PackageManagerUnsupported)
        );
    }
}
