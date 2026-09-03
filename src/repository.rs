use crate::{
    adapter::{
        AdapterMatrix, load_embedded_matrix, match_package_manager, package_for_executable,
        resolve_installed_package,
    },
    atomic_file::{OptionalFile, read_optional_bounded},
    configuration::{
        AgentLowmemConfig, OperationConfig, parse_config, select_operation, valid_key,
    },
    evidence::{EvidenceDigest, EvidenceReader, EvidenceSnapshot},
    package_manager::{
        NodeVersionEvidence, inspect_node_version, inspect_npmrc, inspect_pnpm_settings,
    },
    policy::{OperationPolicy, PolicyInput, PolicyTarget, build_operation_policy},
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
use rustix::fs::{Mode, OFlags, openat};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

const MAX_GIT_PATH_FILE_BYTES: usize = 4_096;
const MAX_MANAGED_CONFIGURATION_BYTES: usize = 262_144;

#[derive(Clone, PartialEq, Eq)]
pub struct GitRepository {
    root: PathBuf,
    metadata: PathBuf,
}

impl GitRepository {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn metadata(&self) -> &Path {
        &self.metadata
    }
}

impl fmt::Debug for GitRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitRepository")
            .field("root_resolved", &self.root().is_absolute())
            .field("metadata_resolved", &self.metadata().is_absolute())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RunSelection {
    pub operation_key: String,
    pub workspace_key: Option<String>,
    pub forwarded_arguments: Vec<String>,
}

impl RunSelection {
    pub fn root(operation_key: impl Into<String>, forwarded_arguments: Vec<String>) -> Self {
        Self {
            operation_key: operation_key.into(),
            workspace_key: None,
            forwarded_arguments,
        }
    }

    pub fn workspace(
        workspace_key: impl Into<String>,
        operation_key: impl Into<String>,
        forwarded_arguments: Vec<String>,
    ) -> Self {
        Self {
            operation_key: operation_key.into(),
            workspace_key: Some(workspace_key.into()),
            forwarded_arguments,
        }
    }
}

impl fmt::Debug for RunSelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation_key = valid_key(&self.operation_key).then_some(self.operation_key.as_str());
        let workspace_key = self.workspace_key.as_deref().filter(|key| valid_key(key));
        formatter
            .debug_struct("RunSelection")
            .field("operation_key", &operation_key.unwrap_or("invalid"))
            .field("workspace_key", &workspace_key)
            .field("forwarded_argument_count", &self.forwarded_arguments.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RunPlan {
    root: GitRepository,
    policy: OperationPolicy,
    evidence: EvidenceSnapshot,
    repository_hash: [u8; 32],
    package_manager: PackageManagerReport,
    forwarded_argument_count: usize,
}

impl RunPlan {
    pub fn policy(&self) -> &OperationPolicy {
        &self.policy
    }

    pub fn evidence(&self) -> &EvidenceSnapshot {
        &self.evidence
    }

    pub fn package_manager(&self) -> &PackageManagerReport {
        &self.package_manager
    }

    pub fn forwarded_argument_count(&self) -> usize {
        self.forwarded_argument_count
    }

    pub(crate) const fn repository_hash(&self) -> [u8; 32] {
        self.repository_hash
    }

    pub(crate) fn root(&self) -> &Path {
        self.root.root()
    }

    pub fn redacted(&self) -> RedactedRunPlan<'_> {
        RedactedRunPlan {
            package_manager: &self.package_manager,
            policy: self.policy.redacted_summary(),
            evidence: self
                .evidence
                .files()
                .iter()
                .map(|file| RedactedEvidenceDigest {
                    relative_path: file.relative_path(),
                    sha256: file.hex(),
                })
                .collect(),
            forwarded_argument_count: self.forwarded_argument_count,
        }
    }
}

impl fmt::Debug for RunPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.redacted().fmt(formatter)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedRunPlan<'a> {
    package_manager: &'a PackageManagerReport,
    policy: crate::policy::RedactedPolicySummary<'a>,
    evidence: Vec<RedactedEvidenceDigest<'a>>,
    forwarded_argument_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RedactedEvidenceDigest<'a> {
    relative_path: &'a str,
    sha256: String,
}

pub fn plans_match(before: &RunPlan, after: &RunPlan) -> bool {
    before.evidence == after.evidence
        && before.policy == after.policy
        && before.repository_hash == after.repository_hash
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

pub fn find_git_repository(start: &Path) -> Result<Option<GitRepository>, RepositoryError> {
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
        let directory = File::open(candidate).map_err(repository_io_error)?;
        let marker = match openat(
            &directory,
            ".git",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(marker) => File::from(marker),
            Err(source) if source == rustix::io::Errno::NOENT => continue,
            Err(source) if source == rustix::io::Errno::LOOP => {
                return Err(RepositoryError::InvalidGitPointer);
            }
            Err(source) => return Err(repository_errno(source)),
        };
        let marker_metadata = marker.metadata().map_err(repository_io_error)?;

        if marker_metadata.is_dir() {
            return Ok(Some(GitRepository {
                root: candidate.to_path_buf(),
                metadata: candidate.join(".git"),
            }));
        }

        if marker_metadata.is_file() {
            let metadata = resolve_git_pointer(candidate, marker)?;
            return Ok(Some(GitRepository {
                root: candidate.to_path_buf(),
                metadata,
            }));
        }

        return Err(RepositoryError::InvalidGitPointer);
    }

    Ok(None)
}

pub fn inspect_repository(start: &Path) -> RepositoryReport {
    let root = match find_git_repository(start) {
        Ok(Some(root)) => root,
        Ok(None) | Err(_) => return unsupported_repository(false, false),
    };
    let package_path = root.root().join("package.json");
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

    let npm_lock = match regular_file_state(&root.root().join("package-lock.json")) {
        Some(present) => present,
        None => return unsupported_repository(true, true),
    };
    let pnpm_lock = match regular_file_state(&root.root().join("pnpm-lock.yaml")) {
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

    let pnpm_document = match inspect_repository_shell(root.root(), package_manager.kind) {
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
    let candidates = match expand_workspace_patterns(root.root(), &patterns) {
        Ok(candidates) => candidates,
        Err(error) => return failed_repository(Some(package_manager), error.reason()),
    };

    let config = match read_optional_file(root.root(), ".agent-lowmem.json") {
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

    let node_evidence = inspect_repository_node_version(root.root());
    let operations = collect_operation_summaries(OperationCollectionInput {
        root: root.root(),
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

pub fn plan_run(start: &Path, selection: &RunSelection) -> Result<RunPlan, Reason> {
    if !valid_key(&selection.operation_key)
        || selection
            .workspace_key
            .as_deref()
            .is_some_and(|key| !valid_key(key))
    {
        return Err(Reason::InvalidConfig);
    }
    let root = find_git_repository(start)
        .map_err(|_| Reason::RepositoryUnsupported)?
        .ok_or(Reason::RepositoryUnsupported)?;
    let reader = EvidenceReader::new(root.root())?;
    let mut evidence = PlanningEvidence::new(reader, root.root());

    let package_file = evidence.read("package.json", Reason::RepositoryUnsupported)?;
    let manifest: RootPackageManifest =
        serde_json::from_slice(&package_file).map_err(|_| Reason::RepositoryUnsupported)?;
    let package_manager = manifest
        .package_manager
        .as_deref()
        .and_then(parse_package_manager)
        .ok_or(Reason::PackageManagerUnsupported)?;
    let (selected_lock, other_lock) = match package_manager.kind {
        PackageManagerKind::Npm => ("package-lock.json", "pnpm-lock.yaml"),
        PackageManagerKind::Pnpm => ("pnpm-lock.yaml", "package-lock.json"),
    };
    evidence.read(selected_lock, Reason::PackageManagerUnsupported)?;
    if regular_file_state(&root.root().join(other_lock)) != Some(false) {
        return Err(Reason::PackageManagerUnsupported);
    }

    let matrix = load_embedded_matrix()?;
    match_package_manager(&matrix, &package_manager)?;

    let pnpm_document = match package_manager.kind {
        PackageManagerKind::Npm => {
            let npmrc = evidence.read_optional(".npmrc", Reason::ScriptShellUnsupported)?;
            inspect_npmrc(npmrc.as_deref())?;
            None
        }
        PackageManagerKind::Pnpm => {
            let bytes =
                evidence.read_optional("pnpm-workspace.yaml", Reason::WorkspaceUnsupported)?;
            match bytes {
                Some(bytes) => {
                    let document = parse_pnpm_workspace(&bytes).map_err(|error| error.reason())?;
                    inspect_pnpm_settings(&document)?;
                    Some(document)
                }
                None => None,
            }
        }
    };
    let patterns = match package_manager.kind {
        PackageManagerKind::Npm => {
            parse_npm_workspaces(&package_file).map_err(|error| error.reason())?
        }
        PackageManagerKind::Pnpm => pnpm_document
            .as_ref()
            .map(|document| document.patterns.clone())
            .unwrap_or_default(),
    };
    let candidates =
        expand_workspace_patterns(root.root(), &patterns).map_err(|error| error.reason())?;
    let mut workspace_manifests = BTreeMap::new();
    for candidate in &candidates {
        let relative_manifest = format!("{}/package.json", candidate.relative_path);
        let workspace_bytes = evidence.read(&relative_manifest, Reason::WorkspaceUnsupported)?;
        let workspace_manifest: WorkspaceRunManifest =
            serde_json::from_slice(&workspace_bytes).map_err(|_| Reason::WorkspaceUnsupported)?;
        if workspace_manifest.name != candidate.package_name {
            return Err(Reason::WorkspaceCardinality);
        }
        workspace_manifests.insert(candidate.relative_path.clone(), workspace_manifest);
    }

    let config_bytes = evidence.read(".agent-lowmem.json", Reason::InvalidConfig)?;
    let config = parse_config(&config_bytes).map_err(|error| error.reason())?;
    if config.package_manager != package_manager.kind {
        return Err(Reason::InvalidConfig);
    }
    let operation = select_operation(
        &config,
        selection.workspace_key.as_deref(),
        &selection.operation_key,
    )
    .map_err(|error| error.reason())?;

    let (selected_package, target, scripts) = match selection.workspace_key.as_deref() {
        None => (
            root.root().to_path_buf(),
            PolicyTarget::Root,
            manifest.scripts,
        ),
        Some(workspace_key) => {
            let configured = config
                .workspaces
                .get(workspace_key)
                .ok_or(Reason::WorkspaceCardinality)?;
            let candidate = resolve_configured_workspace(configured, &candidates)
                .map_err(|error| error.reason())?;
            let workspace_manifest = workspace_manifests
                .get(&candidate.relative_path)
                .ok_or(Reason::WorkspaceCardinality)?;
            (
                root.root().join(&candidate.relative_path),
                PolicyTarget::Workspace {
                    key: workspace_key.to_owned(),
                    package_name: candidate.package_name.clone(),
                },
                workspace_manifest.scripts.clone(),
            )
        }
    };

    let node_evidence = read_node_evidence(&mut evidence)?;
    let policy = build_run_policy(RunPolicyInput {
        root: root.root(),
        selected_package: &selected_package,
        target,
        operation_key: &selection.operation_key,
        operation,
        scripts: &scripts,
        package_manager: &package_manager,
        matrix: &matrix,
        node_evidence: node_evidence.as_ref(),
        forwarded_arguments: &selection.forwarded_arguments,
        evidence: &mut evidence,
    })?;

    let snapshot = EvidenceSnapshot::new(evidence.digests)?;
    Ok(RunPlan {
        repository_hash: repository_hash(root.root()),
        root,
        policy,
        evidence: snapshot,
        package_manager,
        forwarded_argument_count: selection.forwarded_arguments.len(),
    })
}

struct PlanningEvidence {
    reader: EvidenceReader,
    root: PathBuf,
    digests: Vec<EvidenceDigest>,
}

impl PlanningEvidence {
    fn new(reader: EvidenceReader, root: &Path) -> Self {
        Self {
            reader,
            root: root.to_path_buf(),
            digests: Vec::new(),
        }
    }

    fn read(&mut self, relative_path: &str, failure: Reason) -> Result<Vec<u8>, Reason> {
        let file = self.reader.read(relative_path).map_err(|_| failure)?;
        self.digests.push(file.digest());
        Ok(file.bytes().to_vec())
    }

    fn read_optional(
        &mut self,
        relative_path: &str,
        failure: Reason,
    ) -> Result<Option<Vec<u8>>, Reason> {
        let absolute = self.root.join(relative_path);
        match fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(failure)
            }
            Ok(_) => self.read(relative_path, failure).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(failure),
        }
    }
}

#[derive(Deserialize)]
struct WorkspaceRunManifest {
    name: String,
    #[serde(default)]
    scripts: BTreeMap<String, String>,
}

struct RunPolicyInput<'a> {
    root: &'a Path,
    selected_package: &'a Path,
    target: PolicyTarget,
    operation_key: &'a str,
    operation: &'a OperationConfig,
    scripts: &'a BTreeMap<String, String>,
    package_manager: &'a PackageManagerReport,
    matrix: &'a AdapterMatrix,
    node_evidence: Option<&'a RepositoryNodeEvidence>,
    forwarded_arguments: &'a [String],
    evidence: &'a mut PlanningEvidence,
}

#[derive(Deserialize)]
struct InstalledRunManifest {
    name: String,
    version: String,
}

fn read_node_evidence(
    evidence: &mut PlanningEvidence,
) -> Result<Option<RepositoryNodeEvidence>, Reason> {
    let node_version = evidence.read_optional(".node-version", Reason::ToolVersionUnsupported)?;
    let nvmrc = evidence.read_optional(".nvmrc", Reason::ToolVersionUnsupported)?;
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

fn build_run_policy(input: RunPolicyInput<'_>) -> Result<OperationPolicy, Reason> {
    let graph = expand_script_graph(&input.operation.script, input.scripts)?;
    let mut installed_versions = BTreeMap::new();

    for leaf in &graph.leaves {
        let initial_executable = leaf
            .segment
            .arguments()
            .first()
            .ok_or(Reason::ToolUnsupported)?;
        let wrapper_identity = if matches!(initial_executable.as_str(), "cross-env" | "dotenv") {
            let package_name = package_for_executable(input.matrix, initial_executable)
                .map_err(|_| Reason::WrapperUnsupported)?;
            record_installed_package_for_plan(
                input.root,
                input.selected_package,
                package_name,
                &mut installed_versions,
                input.evidence,
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
            let node = input.node_evidence.ok_or(Reason::ToolVersionUnsupported)?;
            installed_versions.insert("node".to_owned(), node.version.0.clone());
        } else {
            record_installed_package_for_plan(
                input.root,
                input.selected_package,
                package_name,
                &mut installed_versions,
                input.evidence,
            )?;
        }
    }

    let evidence_files = input
        .evidence
        .digests
        .iter()
        .map(|digest| digest.relative_path().to_owned())
        .collect::<Vec<_>>();
    let package_manager_version = Version::parse(&input.package_manager.version)
        .map_err(|_| Reason::PackageManagerUnsupported)?;
    build_operation_policy(PolicyInput {
        target: input.target,
        operation_key: input.operation_key,
        operation: input.operation,
        graph: &graph,
        matrix: input.matrix,
        package_manager: input.package_manager.kind,
        package_manager_version: &package_manager_version,
        installed_versions: &installed_versions,
        forwarded_arguments: input.forwarded_arguments,
        evidence_files: &evidence_files,
    })
}

fn record_installed_package_for_plan(
    root: &Path,
    selected_package: &Path,
    package_name: &str,
    versions: &mut BTreeMap<String, Version>,
    evidence: &mut PlanningEvidence,
) -> Result<(), Reason> {
    if versions.contains_key(package_name) {
        return Ok(());
    }
    let located = resolve_installed_package(root, selected_package, package_name)?;
    let bytes = evidence.read(&located.evidence_file, Reason::ToolUnsupported)?;
    let manifest: InstalledRunManifest =
        serde_json::from_slice(&bytes).map_err(|_| Reason::ToolUnsupported)?;
    if manifest.name != package_name {
        return Err(Reason::ToolUnsupported);
    }
    let version = Version::parse(&manifest.version).map_err(|_| Reason::ToolVersionUnsupported)?;
    if version != located.version {
        return Err(Reason::ToolVersionUnsupported);
    }
    versions.insert(manifest.name, version);
    Ok(())
}

fn repository_hash(root: &Path) -> [u8; 32] {
    #[cfg(unix)]
    let bytes = root.as_os_str().as_bytes();
    #[cfg(not(unix))]
    let bytes = root.as_os_str().to_string_lossy().as_bytes();
    Sha256::digest(bytes).into()
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
    let directory = File::open(root).map_err(|_| Reason::RepositoryUnsupported)?;
    match read_optional_bounded(&directory, name, MAX_MANAGED_CONFIGURATION_BYTES)? {
        OptionalFile::Absent => Ok(None),
        OptionalFile::Regular { bytes, .. } => Ok(Some(bytes)),
    }
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

fn resolve_git_pointer(root: &Path, marker: File) -> Result<PathBuf, RepositoryError> {
    let contents = read_git_path_file(marker)?;
    let gitdir = parse_git_path_line(&contents)
        .and_then(|line| line.strip_prefix("gitdir: "))
        .filter(|path| !path.is_empty())
        .ok_or(RepositoryError::InvalidGitPointer)?;
    let metadata = resolve_git_path(root, gitdir)?;
    let metadata_state =
        fs::symlink_metadata(&metadata).map_err(|_| RepositoryError::InvalidGitPointer)?;
    if metadata_state.file_type().is_symlink() || !metadata_state.is_dir() {
        return Err(RepositoryError::InvalidGitPointer);
    }
    let metadata = fs::canonicalize(metadata).map_err(|_| RepositoryError::InvalidGitPointer)?;
    validate_worktree_backlink(root, &metadata)?;
    Ok(metadata)
}

fn validate_worktree_backlink(root: &Path, metadata: &Path) -> Result<(), RepositoryError> {
    let directory = File::open(metadata).map_err(repository_io_error)?;
    let backlink = openat(
        &directory,
        "gitdir",
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|_| RepositoryError::InvalidGitPointer)?;
    let contents = read_git_path_file(backlink)?;
    let backlink = parse_git_path_line(&contents)
        .filter(|path| !path.is_empty())
        .ok_or(RepositoryError::InvalidGitPointer)?;
    let resolved = resolve_git_path(metadata, backlink)?;
    let resolved = fs::canonicalize(resolved).map_err(|_| RepositoryError::InvalidGitPointer)?;
    if resolved != root.join(".git") {
        return Err(RepositoryError::InvalidGitPointer);
    }
    Ok(())
}

fn resolve_git_path(base: &Path, value: &str) -> Result<PathBuf, RepositoryError> {
    if value.contains('\0') {
        return Err(RepositoryError::InvalidGitPointer);
    }
    let value = Path::new(value);
    Ok(if value.is_absolute() {
        value.to_path_buf()
    } else {
        base.join(value)
    })
}

fn read_git_path_file(mut file: File) -> Result<Vec<u8>, RepositoryError> {
    let metadata = file.metadata().map_err(repository_io_error)?;
    if !metadata.is_file() || metadata.len() > MAX_GIT_PATH_FILE_BYTES as u64 {
        return Err(RepositoryError::InvalidGitPointer);
    }
    let mut contents = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_GIT_PATH_FILE_BYTES as u64 + 1)
        .read_to_end(&mut contents)
        .map_err(repository_io_error)?;
    if contents.len() > MAX_GIT_PATH_FILE_BYTES {
        return Err(RepositoryError::InvalidGitPointer);
    }
    Ok(contents)
}

fn parse_git_path_line(contents: &[u8]) -> Option<&str> {
    let line = std::str::from_utf8(contents).ok()?;
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    (!line.is_empty() && !line.contains(['\r', '\n', '\0'])).then_some(line)
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

fn repository_errno(source: rustix::io::Errno) -> RepositoryError {
    repository_io_error(source.into())
}

#[cfg(test)]
mod tests {
    use super::{
        PackageManagerKind, RunSelection, find_git_repository, inspect_repository, plan_run,
        plans_match,
    };
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

        fn configured_npm() -> Self {
            let fixture = Self::git_repo();
            fixture.write(
                "package.json",
                r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
            );
            fixture.write("package-lock.json", "{}\n");
            fixture.write(
                ".agent-lowmem.json",
                r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":300}}}"#,
            );
            fixture.write(
                "node_modules/vitest/package.json",
                r#"{"name":"vitest","version":"4.1.11"}"#,
            );
            fixture
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
    fn plans_configured_run_with_exact_evidence() {
        let fixture = Fixture::configured_npm();

        let plan = plan_run(fixture.path(), &RunSelection::root("test", Vec::new())).unwrap();

        assert_eq!(plan.policy().operation_key, "test");
        assert!(
            plan.evidence()
                .files()
                .iter()
                .any(|file| { file.relative_path() == ".agent-lowmem.json" })
        );
        assert!(
            plan.evidence()
                .files()
                .iter()
                .any(|file| { file.relative_path() == "node_modules/vitest/package.json" })
        );
    }

    #[test]
    fn redacted_run_plan_hides_root_scripts_and_forwarded_arguments() {
        let fixture = Fixture::configured_npm();
        let secret = "customer-token-123";

        let plan = plan_run(
            fixture.path(),
            &RunSelection::root("test", vec![secret.to_owned()]),
        )
        .unwrap();
        let debug = format!("{:?}", plan.redacted());

        assert!(!debug.contains(fixture.path().to_str().unwrap()));
        assert!(!debug.contains("vitest run"));
        assert!(!debug.contains(secret));
        assert!(debug.contains("forwarded_argument_count: 1"));
    }

    #[test]
    fn rejects_unconfigured_operations_and_denied_forwarded_arguments() {
        let fixture = Fixture::configured_npm();

        assert_eq!(
            plan_run(fixture.path(), &RunSelection::root("build", Vec::new())).unwrap_err(),
            Reason::OperationUnsupported
        );
        assert_eq!(
            plan_run(
                fixture.path(),
                &RunSelection::root("test", vec!["--watch".to_owned()]),
            )
            .unwrap_err(),
            Reason::WatchDenied
        );
    }

    #[test]
    fn plan_comparison_detects_changed_evidence() {
        let fixture = Fixture::configured_npm();
        let selection = RunSelection::root("test", Vec::new());
        let before = plan_run(fixture.path(), &selection).unwrap();
        fixture.write("package-lock.json", "{\"lockfileVersion\":3}\n");
        let after = plan_run(fixture.path(), &selection).unwrap();

        assert!(!plans_match(&before, &after));
    }

    #[test]
    fn plans_an_exact_configured_workspace() {
        let fixture = Fixture::git_repo();
        fixture.write(
            "package.json",
            r#"{"name":"root","packageManager":"npm@12.0.2","workspaces":["packages/*"]}"#,
        );
        fixture.write("package-lock.json", "{}\n");
        fixture.write(
            ".agent-lowmem.json",
            r#"{"version":1,"packageManager":"npm","workspaces":{"web":{"path":"packages/web","packageName":"@acme/web","operations":{"test":{"script":"test","timeoutSeconds":300}}}}}"#,
        );
        fixture.write(
            "packages/web/package.json",
            r#"{"name":"@acme/web","scripts":{"test":"vitest run"}}"#,
        );
        fixture.write(
            "node_modules/vitest/package.json",
            r#"{"name":"vitest","version":"4.1.11"}"#,
        );

        let plan = plan_run(
            fixture.path(),
            &RunSelection::workspace("web", "test", Vec::new()),
        )
        .unwrap();

        assert!(matches!(
            &plan.policy().target,
            crate::policy::PolicyTarget::Workspace { key, package_name }
                if key == "web" && package_name == "@acme/web"
        ));
        assert!(
            plan.evidence()
                .files()
                .iter()
                .any(|file| { file.relative_path() == "packages/web/package.json" })
        );
    }

    #[test]
    fn rejects_duplicate_workspace_package_identity() {
        let fixture = Fixture::git_repo();
        fixture.write(
            "package.json",
            r#"{"name":"root","packageManager":"npm@12.0.2","workspaces":["packages/*"]}"#,
        );
        fixture.write("package-lock.json", "{}\n");
        fixture.write(
            ".agent-lowmem.json",
            r#"{"version":1,"packageManager":"npm","workspaces":{"web":{"path":"packages/web","packageName":"@acme/web","operations":{"test":{"script":"test","timeoutSeconds":300}}}}}"#,
        );
        for path in ["packages/web/package.json", "packages/copy/package.json"] {
            fixture.write(
                path,
                r#"{"name":"@acme/web","scripts":{"test":"vitest run"}}"#,
            );
        }

        assert_eq!(
            plan_run(
                fixture.path(),
                &RunSelection::workspace("web", "test", Vec::new()),
            )
            .unwrap_err(),
            Reason::WorkspaceCardinality
        );
    }

    #[test]
    fn captures_lifecycle_and_wrapper_evidence_without_leaking_wrapper_values() {
        let fixture = Fixture::configured_npm();
        fixture.write(
            "package.json",
            r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"pretest":"rimraf cache","test":"cross-env PRIVATE_TOKEN=value vitest run"}}"#,
        );
        fixture.write(
            "node_modules/rimraf/package.json",
            r#"{"name":"rimraf","version":"6.1.3"}"#,
        );
        fixture.write(
            "node_modules/cross-env/package.json",
            r#"{"name":"cross-env","version":"10.1.0"}"#,
        );

        let plan = plan_run(fixture.path(), &RunSelection::root("test", Vec::new())).unwrap();
        let paths = plan
            .evidence()
            .files()
            .iter()
            .map(|file| file.relative_path())
            .collect::<Vec<_>>();
        let debug = format!("{plan:?}");

        assert!(paths.contains(&"node_modules/rimraf/package.json"));
        assert!(paths.contains(&"node_modules/cross-env/package.json"));
        assert!(!debug.contains("PRIVATE_TOKEN"));
        assert!(!debug.contains("value"));
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

        let root = find_git_repository(&fixture.path().join("packages/web/src"))
            .unwrap()
            .unwrap();

        assert_eq!(root.root(), fixture.path());
        assert_eq!(root.metadata(), fixture.path().join(".git"));
    }

    #[test]
    fn resolves_git_directory_metadata_without_debug_path_exposure() {
        let fixture = Fixture::git_repo();
        fixture.mkdir("packages/web/src");

        let repository = find_git_repository(&fixture.path().join("packages/web/src"))
            .unwrap()
            .unwrap();

        assert_eq!(repository.root(), fixture.path());
        assert_eq!(repository.metadata(), fixture.path().join(".git"));
        let debug = format!("{repository:?}");
        assert!(debug.contains("root_resolved"));
        assert!(debug.contains("metadata_resolved"));
        assert!(!debug.contains(fixture.path().to_str().unwrap()));
    }

    #[test]
    fn resolves_git_relative_and_absolute_worktree_pointers_with_exact_backlinks() {
        for absolute in [false, true] {
            let fixture = Fixture::empty();
            fixture.mkdir("metadata");
            let marker = fixture.path().join(".git");
            fixture.write(
                "metadata/gitdir",
                &format!("{}\n", marker.to_string_lossy()),
            );
            let target = if absolute {
                fixture
                    .path()
                    .join("metadata")
                    .to_string_lossy()
                    .into_owned()
            } else {
                "metadata".to_owned()
            };
            fixture.write(".git", &format!("gitdir: {target}\n"));

            let repository = find_git_repository(fixture.path()).unwrap().unwrap();

            assert_eq!(repository.root(), fixture.path());
            assert_eq!(repository.metadata(), fixture.path().join("metadata"));
        }
    }

    #[test]
    fn resolves_git_pointer_only_when_metadata_belongs_to_the_checkout() {
        let missing_backlink = Fixture::empty();
        missing_backlink.mkdir("metadata");
        missing_backlink.write(".git", "gitdir: metadata\n");

        assert_eq!(
            find_git_repository(missing_backlink.path()).unwrap_err(),
            super::RepositoryError::InvalidGitPointer
        );

        let wrong_backlink = Fixture::empty();
        wrong_backlink.mkdir("metadata");
        wrong_backlink.write("metadata/gitdir", "/tmp/unrelated-checkout/.git\n");
        wrong_backlink.write(".git", "gitdir: metadata\n");

        assert_eq!(
            find_git_repository(wrong_backlink.path()).unwrap_err(),
            super::RepositoryError::InvalidGitPointer
        );
    }

    #[test]
    fn resolves_git_pointer_with_bounded_single_line_grammar() {
        for pointer in [
            "gitdir: \n".to_owned(),
            "gitdir: metadata\nsecond-line\n".to_owned(),
            format!("gitdir: {}\n", "a".repeat(4_096)),
        ] {
            let fixture = Fixture::empty();
            fixture.mkdir("metadata");
            fixture.write(".git", &pointer);

            assert_eq!(
                find_git_repository(fixture.path()).unwrap_err(),
                super::RepositoryError::InvalidGitPointer
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolves_git_pointer_without_following_a_marker_symlink() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::empty();
        fixture.mkdir("metadata");
        symlink("metadata", fixture.path().join(".git")).unwrap();

        assert_eq!(
            find_git_repository(fixture.path()).unwrap_err(),
            super::RepositoryError::InvalidGitPointer
        );
    }

    #[test]
    fn accepts_a_valid_worktree_git_pointer() {
        let fixture = Fixture::empty();
        fixture.mkdir("git-data");
        fixture.write(
            "git-data/gitdir",
            &format!("{}\n", fixture.path().join(".git").to_string_lossy()),
        );
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
    fn rejects_oversized_managed_configuration_during_repository_inspection() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"npm@12.0.2"}"#);
        fixture.write("package-lock.json", "{}\n");
        let mut configuration = r#"{"version":1,"packageManager":"npm"}"#.to_owned();
        configuration.push_str(&" ".repeat(262_145 - configuration.len()));
        fixture.write(".agent-lowmem.json", &configuration);

        let report = inspect_repository(fixture.path());

        assert_eq!(report.failure_reason, Some(Reason::ManagedFileConflict));
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
