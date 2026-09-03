use crate::{
    configuration::valid_package_name,
    repository::{PackageManagerKind, PackageManagerReport},
    result::Reason,
};
use semver::Version;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const MATRIX_SCHEMA: &str = "https://agentlowmem.dev/schema/adapter-matrix-v1.json";
const MATRIX_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
    Controlled,
    Disclosed,
    Auxiliary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AdapterKind {
    Command,
    Runtime,
    Wrapper,
    Auxiliary,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterMatrix {
    #[serde(rename = "$schema")]
    schema: String,
    version: u8,
    #[serde(rename = "_evidence")]
    evidence: BTreeMap<String, String>,
    package_managers: Vec<PackageManagerRule>,
    adapters: Vec<AdapterRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageManagerRule {
    name: String,
    version: String,
    root_arguments: Vec<String>,
    workspace_arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterRule {
    pub package_name: String,
    pub executable: String,
    pub version: String,
    kind: AdapterKind,
    pub classification: Classification,
    pub required_prefix: Vec<String>,
    pub required_controls: Vec<String>,
    pub suffix: Vec<String>,
    denials: Vec<DenialRule>,
    pub disclosure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DenialRule {
    reason: DenialReason,
    exact: Vec<String>,
    key_value: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum DenialReason {
    #[serde(rename = "watch-denied")]
    Watch,
    #[serde(rename = "ui-denied")]
    Ui,
    #[serde(rename = "background-denied")]
    Background,
    #[serde(rename = "parallel-denied")]
    Parallel,
    #[serde(rename = "argument-denied")]
    Argument,
}

impl DenialReason {
    const fn reason(self) -> Reason {
        match self {
            Self::Watch => Reason::WatchDenied,
            Self::Ui => Reason::UiDenied,
            Self::Background => Reason::BackgroundDenied,
            Self::Parallel => Reason::ParallelDenied,
            Self::Argument => Reason::ArgumentDenied,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlDecision {
    AlreadyControlled,
    RequiresSuffix(Vec<String>),
    NoControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disclosure {
    pub identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterMatch<'a> {
    pub rule: &'a AdapterRule,
    pub classification: Classification,
    pub control: ControlDecision,
    pub disclosure: Option<Disclosure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub package_name: String,
    pub version: Version,
    pub evidence_file: String,
}

#[derive(Debug, Deserialize)]
struct InstalledManifest {
    name: String,
    version: String,
}

pub fn load_embedded_matrix() -> Result<AdapterMatrix, Reason> {
    let matrix: AdapterMatrix = serde_json::from_str(include_str!("../adapters/matrix-v1.json"))
        .map_err(|_| Reason::InternalError)?;
    validate_matrix(&matrix)?;
    Ok(matrix)
}

pub fn match_adapter<'a>(
    matrix: &'a AdapterMatrix,
    package_name: &str,
    version: &Version,
    arguments: &[String],
) -> Result<AdapterMatch<'a>, Reason> {
    let package_rules = matrix
        .adapters
        .iter()
        .filter(|rule| rule.package_name == package_name)
        .collect::<Vec<_>>();
    if package_rules.is_empty() {
        return Err(Reason::ToolUnsupported);
    }
    let version_string = version.to_string();
    let version_rules = package_rules
        .into_iter()
        .filter(|rule| rule.version == version_string)
        .collect::<Vec<_>>();
    if version_rules.is_empty() {
        return Err(Reason::ToolVersionUnsupported);
    }
    let rule = version_rules
        .into_iter()
        .find(|rule| command_form_matches(rule, arguments))
        .ok_or(Reason::ToolUnsupported)?;

    if rule.kind == AdapterKind::Auxiliary && !valid_rimraf_arguments(arguments) {
        return Err(Reason::ToolUnsupported);
    }
    for denial in &rule.denials {
        if arguments.iter().any(|argument| {
            !rule.required_controls.contains(argument)
                && (denial.exact.contains(argument)
                    || argument
                        .split_once('=')
                        .is_some_and(|(key, _)| denial.key_value.iter().any(|item| item == key)))
        }) {
            return Err(denial.reason.reason());
        }
    }

    let (control, disclosure) = match rule.classification {
        Classification::Controlled => {
            let missing = rule
                .suffix
                .iter()
                .filter(|control| !arguments.contains(control))
                .cloned()
                .collect::<Vec<_>>();
            let control = if missing.is_empty() {
                ControlDecision::AlreadyControlled
            } else {
                ControlDecision::RequiresSuffix(missing)
            };
            (control, None)
        }
        Classification::Disclosed => (
            ControlDecision::NoControl,
            Some(Disclosure {
                identifier: rule.disclosure.clone().ok_or(Reason::InternalError)?,
            }),
        ),
        Classification::Auxiliary => (ControlDecision::NoControl, None),
    };
    Ok(AdapterMatch {
        rule,
        classification: rule.classification,
        control,
        disclosure,
    })
}

pub fn match_package_manager(
    matrix: &AdapterMatrix,
    report: &PackageManagerReport,
) -> Result<(), Reason> {
    let name = match report.kind {
        PackageManagerKind::Npm => "npm",
        PackageManagerKind::Pnpm => "pnpm",
    };
    if matrix
        .package_managers
        .iter()
        .any(|rule| rule.name == name && rule.version == report.version)
    {
        Ok(())
    } else {
        Err(Reason::PackageManagerUnsupported)
    }
}

pub fn resolve_installed_package(
    git_root: &Path,
    selected_package: &Path,
    package_name: &str,
) -> Result<InstalledPackage, Reason> {
    let matrix = load_embedded_matrix()?;
    if !valid_package_name(package_name)
        || !matrix
            .adapters
            .iter()
            .any(|rule| rule.package_name == package_name)
    {
        return Err(Reason::ToolUnsupported);
    }
    let canonical_root = fs::canonicalize(git_root).map_err(|_| Reason::ToolUnsupported)?;
    let canonical_selected =
        fs::canonicalize(selected_package).map_err(|_| Reason::ToolUnsupported)?;
    if !canonical_selected.starts_with(&canonical_root) {
        return Err(Reason::ToolUnsupported);
    }

    match inspect_package_candidate(&canonical_root, &canonical_selected, package_name)? {
        CandidateResult::Found(package) => return Ok(package),
        CandidateResult::Missing => {}
    }
    if canonical_selected != canonical_root {
        match inspect_package_candidate(&canonical_root, &canonical_root, package_name)? {
            CandidateResult::Found(package) => return Ok(package),
            CandidateResult::Missing => {}
        }
    }
    Err(Reason::ToolUnsupported)
}

fn validate_matrix(matrix: &AdapterMatrix) -> Result<(), Reason> {
    if matrix.schema != MATRIX_SCHEMA
        || matrix.version != MATRIX_VERSION
        || matrix.evidence.is_empty()
        || matrix.package_managers.is_empty()
        || matrix.adapters.is_empty()
    {
        return Err(Reason::InternalError);
    }
    if matrix
        .evidence
        .iter()
        .any(|(key, url)| !stable_identifier(key) || !official_evidence_url(url))
    {
        return Err(Reason::InternalError);
    }

    let mut package_manager_tuples = BTreeSet::new();
    for rule in &matrix.package_managers {
        if !matches!(rule.name.as_str(), "npm" | "pnpm")
            || Version::parse(&rule.version).is_err()
            || rule.root_arguments.is_empty()
            || rule.workspace_arguments.is_empty()
            || !package_manager_tuples.insert((rule.name.as_str(), rule.version.as_str()))
        {
            return Err(Reason::InternalError);
        }
    }

    let mut adapter_tuples = BTreeSet::new();
    for rule in &matrix.adapters {
        if !valid_package_name(&rule.package_name)
            || !valid_executable(&rule.executable)
            || Version::parse(&rule.version).is_err()
            || !adapter_tuples.insert((
                rule.package_name.as_str(),
                rule.executable.as_str(),
                rule.version.as_str(),
            ))
            || rule
                .required_prefix
                .iter()
                .chain(&rule.required_controls)
                .chain(&rule.suffix)
                .any(|value| value.is_empty())
        {
            return Err(Reason::InternalError);
        }
        let valid_shape = match rule.classification {
            Classification::Controlled => {
                rule.disclosure.is_none() && rule.required_controls == rule.suffix
            }
            Classification::Disclosed => {
                rule.suffix.is_empty()
                    && rule.required_controls.is_empty()
                    && rule.disclosure.as_deref().is_some_and(stable_identifier)
            }
            Classification::Auxiliary => {
                rule.suffix.is_empty()
                    && rule.required_controls.is_empty()
                    && rule.disclosure.is_none()
            }
        };
        if !valid_shape
            || (rule.kind == AdapterKind::Wrapper
                && rule.classification != Classification::Auxiliary)
            || rule.denials.iter().any(|denial| {
                denial
                    .exact
                    .iter()
                    .chain(&denial.key_value)
                    .any(|token| token.is_empty())
            })
        {
            return Err(Reason::InternalError);
        }
    }
    Ok(())
}

fn command_form_matches(rule: &AdapterRule, arguments: &[String]) -> bool {
    arguments
        .first()
        .is_some_and(|value| value == &rule.executable)
        && arguments
            .get(1..1 + rule.required_prefix.len())
            .is_some_and(|prefix| prefix == rule.required_prefix)
}

fn valid_rimraf_arguments(arguments: &[String]) -> bool {
    arguments.len() > 1
        && arguments[1..].iter().all(|argument| {
            !argument.starts_with('-')
                && !argument.contains(['*', '?', '[', ']', '{', '}'])
                && crate::configuration::valid_relative_path(argument)
        })
}

fn stable_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_executable(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn official_evidence_url(url: &str) -> bool {
    [
        "https://docs.npmjs.com/",
        "https://pnpm.io/",
        "https://v4.vitest.dev/",
        "https://jestjs.io/",
        "https://nodejs.org/",
        "https://www.typescriptlang.org/",
        "https://eslint.org/",
        "https://nextjs.org/",
        "https://docs.nestjs.com/",
    ]
    .iter()
    .any(|prefix| url.starts_with(prefix))
}

enum CandidateResult {
    Missing,
    Found(InstalledPackage),
}

fn inspect_package_candidate(
    canonical_root: &Path,
    base: &Path,
    package_name: &str,
) -> Result<CandidateResult, Reason> {
    let mut package_directory = base.join("node_modules");
    if !admit_directory_component(&package_directory)? {
        return Ok(CandidateResult::Missing);
    }
    for component in package_name.split('/') {
        package_directory.push(component);
        if !admit_directory_component(&package_directory)? {
            return Ok(CandidateResult::Missing);
        }
    }
    let manifest_path = package_directory.join("package.json");
    let metadata = match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CandidateResult::Missing);
        }
        Err(_) => return Err(Reason::ToolUnsupported),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Reason::ToolUnsupported);
    }
    let canonical_manifest =
        fs::canonicalize(&manifest_path).map_err(|_| Reason::ToolUnsupported)?;
    if !canonical_manifest.starts_with(canonical_root) {
        return Err(Reason::ToolUnsupported);
    }
    let manifest: InstalledManifest = serde_json::from_slice(
        &fs::read(&canonical_manifest).map_err(|_| Reason::ToolUnsupported)?,
    )
    .map_err(|_| Reason::ToolUnsupported)?;
    if manifest.name != package_name {
        return Err(Reason::ToolUnsupported);
    }
    let version = Version::parse(&manifest.version).map_err(|_| Reason::ToolVersionUnsupported)?;
    let evidence_file = canonical_manifest
        .strip_prefix(canonical_root)
        .map_err(|_| Reason::ToolUnsupported)?
        .to_str()
        .ok_or(Reason::ToolUnsupported)?
        .replace(std::path::MAIN_SEPARATOR, "/");
    Ok(CandidateResult::Found(InstalledPackage {
        package_name: manifest.name,
        version,
        evidence_file,
    }))
}

fn admit_directory_component(path: &Path) -> Result<bool, Reason> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Reason::ToolUnsupported),
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(Reason::ToolUnsupported),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Reason::ToolUnsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Classification, ControlDecision, load_embedded_matrix, match_adapter,
        match_package_manager, resolve_installed_package,
    };
    use crate::{
        repository::{PackageManagerKind, PackageManagerReport},
        result::Reason,
    };
    use semver::Version;
    use std::{
        collections::BTreeSet,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    const SNAPSHOT: [(&str, &str, &str); 10] = [
        ("vitest", "vitest", "4.1.11"),
        ("jest", "jest", "30.5.1"),
        ("node", "node", "24.14.1"),
        ("typescript", "tsc", "7.0.2"),
        ("eslint", "eslint", "10.9.1"),
        ("next", "next", "16.3.4"),
        ("@nestjs/cli", "nest", "12.0.0"),
        ("cross-env", "cross-env", "10.1.0"),
        ("dotenv-cli", "dotenv", "11.0.0"),
        ("rimraf", "rimraf", "6.1.3"),
    ];

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "agent-lowmem-adapter-{nanos}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self {
                root: fs::canonicalize(root).unwrap(),
            }
        }

        fn package(&self, base: &str, name: &str, version: &str) {
            let directory = self.root.join(base).join("node_modules").join(name);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("package.json"),
                format!(r#"{{"name":"{name}","version":"{version}"}}"#),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn embedded_matrix_contains_only_the_exact_reviewed_snapshot() {
        let matrix = load_embedded_matrix().unwrap();
        assert_eq!(
            matrix
                .package_managers
                .iter()
                .map(|rule| (rule.name.as_str(), rule.version.as_str()))
                .collect::<Vec<_>>(),
            [("npm", "12.0.2"), ("pnpm", "11.25.0")]
        );
        assert_eq!(
            matrix
                .adapters
                .iter()
                .map(|rule| (
                    rule.package_name.as_str(),
                    rule.executable.as_str(),
                    rule.version.as_str(),
                ))
                .collect::<Vec<_>>(),
            SNAPSHOT
        );
        let raw = include_str!("../adapters/matrix-v1.json");
        for operator in ['^', '*', '>', '<'] {
            assert!(
                !raw.contains(operator),
                "range operator {operator} is forbidden"
            );
        }
        assert_eq!(
            matrix.package_managers[0].root_arguments,
            strings(&["--script-shell=/bin/sh", "run", "{script}"])
        );
        assert_eq!(
            matrix.package_managers[0].workspace_arguments,
            strings(&[
                "--script-shell=/bin/sh",
                "--workspace={packageName}",
                "run",
                "{script}",
            ])
        );
        assert_eq!(
            matrix.package_managers[1].root_arguments,
            strings(&[
                "--config.script-shell=/bin/sh",
                "--config.shell-emulator=false",
                "run",
                "{script}",
            ])
        );
        assert_eq!(
            matrix.package_managers[1].workspace_arguments,
            strings(&[
                "--config.script-shell=/bin/sh",
                "--config.shell-emulator=false",
                "--filter",
                "{packageName}",
                "--fail-if-no-match",
                "run",
                "{script}",
            ])
        );
    }

    #[test]
    fn matches_only_exact_package_manager_versions() {
        let matrix = load_embedded_matrix().unwrap();
        for (kind, version) in [
            (PackageManagerKind::Npm, "12.0.2"),
            (PackageManagerKind::Pnpm, "11.25.0"),
        ] {
            let exact = PackageManagerReport {
                kind,
                version: version.to_owned(),
            };
            assert_eq!(match_package_manager(&matrix, &exact), Ok(()));
            for other in ["0.0.0", "99.99.99"] {
                let unsupported = PackageManagerReport {
                    kind,
                    version: other.to_owned(),
                };
                assert_eq!(
                    match_package_manager(&matrix, &unsupported),
                    Err(Reason::PackageManagerUnsupported)
                );
            }
        }
    }

    #[test]
    fn matrix_schema_identity_matches_the_embedded_artifact() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schemas/adapter-matrix-v1.schema.json")).unwrap();
        let artifact: serde_json::Value =
            serde_json::from_str(include_str!("../adapters/matrix-v1.json")).unwrap();

        assert_eq!(schema["$id"], artifact["$schema"]);
        assert_eq!(
            schema["properties"]["version"]["const"],
            artifact["version"]
        );
    }

    #[test]
    fn matrix_tuples_and_official_evidence_are_valid_and_unique() {
        let matrix = load_embedded_matrix().unwrap();
        let mut tuples = BTreeSet::new();
        for rule in &matrix.adapters {
            assert!(tuples.insert((
                rule.package_name.as_str(),
                rule.executable.as_str(),
                rule.version.as_str(),
            )));
        }
        for url in matrix.evidence.values() {
            let allowed = [
                "https://docs.npmjs.com/",
                "https://pnpm.io/",
                "https://v4.vitest.dev/",
                "https://jestjs.io/",
                "https://nodejs.org/",
                "https://www.typescriptlang.org/",
                "https://eslint.org/",
                "https://nextjs.org/",
                "https://docs.nestjs.com/",
            ];
            assert!(allowed.iter().any(|prefix| url.starts_with(prefix)));
        }
    }

    #[test]
    fn committed_package_fixtures_have_one_to_one_matrix_parity() {
        let matrix = load_embedded_matrix().unwrap();
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/repositories/adapter-packages");
        let mut fixture_identities = BTreeSet::new();
        for entry in fs::read_dir(fixture_root).unwrap() {
            let manifest: serde_json::Value = serde_json::from_slice(
                &fs::read(entry.unwrap().path().join("package.json")).unwrap(),
            )
            .unwrap();
            fixture_identities.insert((
                manifest["name"].as_str().unwrap().to_owned(),
                manifest["version"].as_str().unwrap().to_owned(),
            ));
        }
        let matrix_identities = matrix
            .adapters
            .iter()
            .map(|rule| (rule.package_name.clone(), rule.version.clone()))
            .collect::<BTreeSet<_>>();
        assert_eq!(fixture_identities, matrix_identities);
    }

    #[test]
    fn exact_versions_match_while_adjacent_patches_do_not() {
        let matrix = load_embedded_matrix().unwrap();
        for (package, executable, version) in SNAPSHOT {
            let version = Version::parse(version).unwrap();
            let arguments = representative_arguments(executable);
            assert!(match_adapter(&matrix, package, &version, &arguments).is_ok());

            let mut higher = version.clone();
            higher.patch += 1;
            let mut adjacent_versions = vec![higher];
            if version.patch > 0 {
                let mut lower = version.clone();
                lower.patch -= 1;
                adjacent_versions.push(lower);
            }
            for adjacent in adjacent_versions {
                assert_eq!(
                    match_adapter(&matrix, package, &adjacent, &arguments).unwrap_err(),
                    Reason::ToolVersionUnsupported
                );
            }
        }
    }

    #[test]
    fn classifies_controls_disclosures_and_auxiliary_forms() {
        let matrix = load_embedded_matrix().unwrap();
        let vitest = match_adapter(
            &matrix,
            "vitest",
            &Version::parse("4.1.11").unwrap(),
            &strings(&["vitest", "run"]),
        )
        .unwrap();
        assert_eq!(vitest.classification, Classification::Controlled);
        assert_eq!(
            vitest.control,
            ControlDecision::RequiresSuffix(strings(&["--no-file-parallelism", "--maxWorkers=1"]))
        );
        let controlled = match_adapter(
            &matrix,
            "vitest",
            &Version::parse("4.1.11").unwrap(),
            &strings(&["vitest", "run", "--no-file-parallelism", "--maxWorkers=1"]),
        )
        .unwrap();
        assert_eq!(controlled.control, ControlDecision::AlreadyControlled);

        let next = match_adapter(
            &matrix,
            "next",
            &Version::parse("16.3.4").unwrap(),
            &strings(&["next", "build"]),
        )
        .unwrap();
        assert_eq!(next.classification, Classification::Disclosed);
        assert_eq!(
            next.disclosure.unwrap().identifier,
            "internal-fanout-uncontrolled"
        );

        let rimraf = match_adapter(
            &matrix,
            "rimraf",
            &Version::parse("6.1.3").unwrap(),
            &strings(&["rimraf", "dist", "coverage/tmp"]),
        )
        .unwrap();
        assert_eq!(rimraf.classification, Classification::Auxiliary);
        assert_eq!(rimraf.control, ControlDecision::NoControl);
    }

    #[test]
    fn returns_specific_denials_before_control_suffixes() {
        let matrix = load_embedded_matrix().unwrap();
        for (package, version, arguments, reason) in [
            (
                "vitest",
                "4.1.11",
                strings(&["vitest", "run", "--watch"]),
                Reason::WatchDenied,
            ),
            (
                "vitest",
                "4.1.11",
                strings(&["vitest", "run", "--ui"]),
                Reason::UiDenied,
            ),
            (
                "jest",
                "30.5.1",
                strings(&["jest", "--maxWorkers=4"]),
                Reason::ParallelDenied,
            ),
            (
                "node",
                "24.14.1",
                strings(&["node", "--test", "--test-concurrency=2"]),
                Reason::ParallelDenied,
            ),
            (
                "eslint",
                "10.9.1",
                strings(&["eslint", "--concurrency=auto"]),
                Reason::ParallelDenied,
            ),
            (
                "next",
                "16.3.4",
                strings(&["next", "build", "--experimental-build-mode=compile"]),
                Reason::ArgumentDenied,
            ),
        ] {
            assert_eq!(
                match_adapter(
                    &matrix,
                    package,
                    &Version::parse(version).unwrap(),
                    &arguments
                )
                .unwrap_err(),
                reason
            );
        }
    }

    #[test]
    fn rejects_unsupported_command_forms_and_unsafe_rimraf_arguments() {
        let matrix = load_embedded_matrix().unwrap();
        for (package, version, arguments) in [
            ("vitest", "4.1.11", strings(&["vitest", "watch"])),
            ("node", "24.14.1", strings(&["node", "app.js"])),
            ("next", "16.3.4", strings(&["next", "dev"])),
            ("@nestjs/cli", "12.0.0", strings(&["nest", "start"])),
            ("rimraf", "6.1.3", strings(&["rimraf"])),
            ("rimraf", "6.1.3", strings(&["rimraf", "--glob", "dist"])),
            ("rimraf", "6.1.3", strings(&["rimraf", "../outside"])),
            ("rimraf", "6.1.3", strings(&["rimraf", "dist/*"])),
        ] {
            assert_eq!(
                match_adapter(
                    &matrix,
                    package,
                    &Version::parse(version).unwrap(),
                    &arguments
                )
                .unwrap_err(),
                Reason::ToolUnsupported
            );
        }
    }

    #[test]
    fn resolves_selected_package_first_then_root_with_relative_evidence() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("apps/web")).unwrap();
        fixture.package("", "vitest", "4.1.10");
        fixture.package("apps/web", "vitest", "4.1.11");
        fixture.package("", "jest", "30.5.1");

        let selected =
            resolve_installed_package(&fixture.root, &fixture.root.join("apps/web"), "vitest")
                .unwrap();
        assert_eq!(selected.version, Version::parse("4.1.11").unwrap());
        assert_eq!(
            selected.evidence_file,
            "apps/web/node_modules/vitest/package.json"
        );

        let fallback =
            resolve_installed_package(&fixture.root, &fixture.root.join("apps/web"), "jest")
                .unwrap();
        assert_eq!(fallback.evidence_file, "node_modules/jest/package.json");
    }

    #[test]
    fn rejects_invalid_identity_malformed_manifest_and_canonical_escape() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("apps/web")).unwrap();
        assert_eq!(
            resolve_installed_package(&fixture.root, &fixture.root, "../../secret").unwrap_err(),
            Reason::ToolUnsupported
        );
        fixture.package("", "vitest", "not-semver");
        assert_eq!(
            resolve_installed_package(&fixture.root, &fixture.root, "vitest").unwrap_err(),
            Reason::ToolVersionUnsupported
        );
        fixture.package("", "jest", "30.5.1");
        fs::write(
            fixture.root.join("node_modules/jest/package.json"),
            r#"{"name":"other","version":"30.5.1"}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_installed_package(&fixture.root, &fixture.root, "jest").unwrap_err(),
            Reason::ToolUnsupported
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_package_evidence() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = Fixture::new();
        outside.package("", "vitest", "4.1.11");
        fs::create_dir_all(fixture.root.join("node_modules")).unwrap();
        symlink(
            outside.root.join("node_modules/vitest"),
            fixture.root.join("node_modules/vitest"),
        )
        .unwrap();

        assert_eq!(
            resolve_installed_package(&fixture.root, &fixture.root, "vitest").unwrap_err(),
            Reason::ToolUnsupported
        );
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn representative_arguments(executable: &str) -> Vec<String> {
        match executable {
            "vitest" => strings(&["vitest", "run"]),
            "node" => strings(&["node", "--test"]),
            "next" | "nest" => strings(&[executable, "build"]),
            "rimraf" => strings(&["rimraf", "dist"]),
            _ => strings(&[executable]),
        }
    }
}
