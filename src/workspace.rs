use crate::{
    configuration::{WorkspaceConfig, valid_package_name, valid_relative_path},
    result::Reason,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const MAX_PATTERN_SEGMENTS: usize = 32;
const MAX_PATTERN_BYTES: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternSegment {
    Literal(String),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePattern {
    segments: Vec<PatternSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceCandidate {
    pub relative_path: String,
    pub package_name: String,
}

impl WorkspaceCandidate {
    pub fn new(relative_path: impl Into<String>, package_name: impl Into<String>) -> Self {
        Self {
            relative_path: relative_path.into(),
            package_name: package_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnpmWorkspaceDocument {
    pub patterns: Vec<WorkspacePattern>,
    pub script_shell: Option<String>,
    pub shell_emulator: Option<bool>,
    pub enable_pre_post_scripts: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceError(Reason);

impl WorkspaceError {
    pub const fn reason(self) -> Reason {
        self.0
    }
}

#[derive(Debug, Deserialize)]
struct NpmRootManifest {
    workspaces: Option<NpmWorkspaces>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NpmWorkspaces {
    Array(Vec<String>),
    Object(NpmWorkspaceObject),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NpmWorkspaceObject {
    packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspacePackageManifest {
    name: String,
}

pub fn parse_npm_workspaces(bytes: &[u8]) -> Result<Vec<WorkspacePattern>, WorkspaceError> {
    let manifest: NpmRootManifest =
        serde_json::from_slice(bytes).map_err(|_| workspace_unsupported())?;
    let declarations = match manifest.workspaces {
        None => return Ok(Vec::new()),
        Some(NpmWorkspaces::Array(patterns)) => patterns,
        Some(NpmWorkspaces::Object(object)) => object.packages,
    };
    declarations
        .into_iter()
        .map(|pattern| parse_pattern(&pattern))
        .collect()
}

pub fn parse_pnpm_workspace(bytes: &[u8]) -> Result<PnpmWorkspaceDocument, WorkspaceError> {
    let contents = std::str::from_utf8(bytes).map_err(|_| workspace_unsupported())?;
    if contents.contains(['\r', '\t']) {
        return Err(workspace_unsupported());
    }

    let mut saw_packages = false;
    let mut in_packages = false;
    let mut patterns = Vec::new();
    let mut script_shell = None;
    let mut shell_emulator = None;
    let mut enable_pre_post_scripts = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line.starts_with(' ') {
            if !in_packages || !line.starts_with("  - ") || line[4..].starts_with(' ') {
                return Err(workspace_unsupported());
            }
            patterns.push(parse_pattern(&parse_scalar(&line[4..])?)?);
            continue;
        }

        let (key, raw_value) = line.split_once(':').ok_or_else(workspace_unsupported)?;
        match key {
            "packages" => {
                if saw_packages || !strip_unquoted_comment(raw_value).trim().is_empty() {
                    return Err(workspace_unsupported());
                }
                saw_packages = true;
                in_packages = true;
            }
            "scriptShell" => {
                in_packages = false;
                if script_shell.is_some() {
                    return Err(workspace_unsupported());
                }
                script_shell = Some(parse_scalar(raw_value)?);
            }
            "shellEmulator" => {
                in_packages = false;
                if shell_emulator.is_some() {
                    return Err(workspace_unsupported());
                }
                shell_emulator = Some(parse_bool(raw_value)?);
            }
            "enablePrePostScripts" => {
                in_packages = false;
                if enable_pre_post_scripts.is_some() {
                    return Err(workspace_unsupported());
                }
                enable_pre_post_scripts = Some(parse_bool(raw_value)?);
            }
            _ => return Err(workspace_unsupported()),
        }
    }

    if !saw_packages || patterns.is_empty() {
        return Err(workspace_unsupported());
    }

    Ok(PnpmWorkspaceDocument {
        patterns,
        script_shell,
        shell_emulator,
        enable_pre_post_scripts,
    })
}

pub fn expand_workspace_patterns(
    root: &Path,
    patterns: &[WorkspacePattern],
) -> Result<Vec<WorkspaceCandidate>, WorkspaceError> {
    let canonical_root = fs::canonicalize(root).map_err(|_| workspace_unsupported())?;
    let mut candidates_by_path = BTreeMap::new();

    for pattern in patterns {
        let mut directories = vec![canonical_root.clone()];
        for segment in &pattern.segments {
            let mut next = Vec::new();
            for directory in directories {
                match segment {
                    PatternSegment::Literal(value) => {
                        let candidate = directory.join(value);
                        match fs::symlink_metadata(&candidate) {
                            Ok(metadata) if metadata.file_type().is_symlink() => {
                                return Err(workspace_unsupported());
                            }
                            Ok(metadata) if metadata.is_dir() => next.push(candidate),
                            Ok(_) => return Err(workspace_unsupported()),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => return Err(workspace_unsupported()),
                        }
                    }
                    PatternSegment::Wildcard => {
                        let entries = match fs::read_dir(&directory) {
                            Ok(entries) => entries,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(_) => return Err(workspace_unsupported()),
                        };
                        let mut paths = entries
                            .map(|entry| entry.map(|entry| entry.path()))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| workspace_unsupported())?;
                        paths.sort();
                        for path in paths {
                            let metadata =
                                fs::symlink_metadata(&path).map_err(|_| workspace_unsupported())?;
                            if metadata.file_type().is_symlink() {
                                return Err(workspace_unsupported());
                            }
                            if metadata.is_dir() {
                                next.push(path);
                            }
                        }
                    }
                }
            }
            directories = next;
        }

        for directory in directories {
            admit_candidate(&canonical_root, &directory, &mut candidates_by_path)?;
        }
    }

    let candidates = candidates_by_path.into_values().collect::<Vec<_>>();
    let mut package_names = BTreeSet::new();
    if candidates
        .iter()
        .any(|candidate| !package_names.insert(candidate.package_name.as_str()))
    {
        return Err(WorkspaceError(Reason::WorkspaceCardinality));
    }
    Ok(candidates)
}

pub fn resolve_configured_workspace<'a>(
    configured: &WorkspaceConfig,
    candidates: &'a [WorkspaceCandidate],
) -> Result<&'a WorkspaceCandidate, WorkspaceError> {
    let mut matches = candidates.iter().filter(|candidate| {
        candidate.relative_path == configured.path
            && candidate.package_name == configured.package_name
    });
    let exact = matches
        .next()
        .ok_or(WorkspaceError(Reason::WorkspaceCardinality))?;
    if matches.next().is_some() {
        return Err(WorkspaceError(Reason::WorkspaceCardinality));
    }
    Ok(exact)
}

fn parse_pattern(value: &str) -> Result<WorkspacePattern, WorkspaceError> {
    if value.is_empty()
        || value.len() > MAX_PATTERN_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains(['\\', '\0'])
    {
        return Err(workspace_unsupported());
    }
    let raw_segments = value.split('/').collect::<Vec<_>>();
    if raw_segments.is_empty() || raw_segments.len() > MAX_PATTERN_SEGMENTS {
        return Err(workspace_unsupported());
    }
    let segments = raw_segments
        .into_iter()
        .map(|segment| {
            if segment == "*" {
                return Ok(PatternSegment::Wildcard);
            }
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || !segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'-'
                        )
                })
            {
                return Err(workspace_unsupported());
            }
            Ok(PatternSegment::Literal(segment.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WorkspacePattern { segments })
}

fn parse_scalar(value: &str) -> Result<String, WorkspaceError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(workspace_unsupported());
    }
    if let Some(rest) = trimmed.strip_prefix('\'') {
        let closing = rest.find('\'').ok_or_else(workspace_unsupported)?;
        let decoded = &rest[..closing];
        if decoded.is_empty() || !only_comment_or_empty(&rest[closing + 1..]) {
            return Err(workspace_unsupported());
        }
        return Ok(decoded.to_owned());
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        return parse_double_quoted_scalar(rest);
    }

    let decoded = strip_unquoted_comment(trimmed).trim_end();
    if decoded.is_empty()
        || decoded.starts_with(['&', '*', '!', '|', '>', '{', '['])
        || decoded.contains(['$', '`'])
    {
        return Err(workspace_unsupported());
    }
    Ok(decoded.to_owned())
}

fn parse_double_quoted_scalar(rest: &str) -> Result<String, WorkspaceError> {
    let mut decoded = String::new();
    let mut characters = rest.char_indices();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => {
                if decoded.is_empty() || !only_comment_or_empty(&rest[index + 1..]) {
                    return Err(workspace_unsupported());
                }
                return Ok(decoded);
            }
            '\\' => match characters.next() {
                Some((_, '"')) => decoded.push('"'),
                Some((_, '\\')) => decoded.push('\\'),
                _ => return Err(workspace_unsupported()),
            },
            '$' | '`' => return Err(workspace_unsupported()),
            _ => decoded.push(character),
        }
    }
    Err(workspace_unsupported())
}

fn only_comment_or_empty(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn strip_unquoted_comment(value: &str) -> &str {
    value.split_once('#').map_or(value, |(before, _)| before)
}

fn parse_bool(value: &str) -> Result<bool, WorkspaceError> {
    match strip_unquoted_comment(value).trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(workspace_unsupported()),
    }
}

fn admit_candidate(
    canonical_root: &Path,
    directory: &Path,
    candidates: &mut BTreeMap<String, WorkspaceCandidate>,
) -> Result<(), WorkspaceError> {
    let package_path = directory.join("package.json");
    let metadata = match fs::symlink_metadata(&package_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(workspace_unsupported()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(workspace_unsupported());
    }

    let canonical_directory = fs::canonicalize(directory).map_err(|_| workspace_unsupported())?;
    if !canonical_directory.starts_with(canonical_root) {
        return Err(workspace_unsupported());
    }
    let relative = canonical_directory
        .strip_prefix(canonical_root)
        .map_err(|_| workspace_unsupported())?
        .to_str()
        .ok_or_else(workspace_unsupported)?
        .replace(std::path::MAIN_SEPARATOR, "/");
    if !valid_relative_path(&relative) {
        return Err(workspace_unsupported());
    }

    let manifest: WorkspacePackageManifest =
        serde_json::from_slice(&fs::read(package_path).map_err(|_| workspace_unsupported())?)
            .map_err(|_| workspace_unsupported())?;
    if !valid_package_name(&manifest.name) {
        return Err(workspace_unsupported());
    }
    candidates
        .entry(relative.clone())
        .or_insert_with(|| WorkspaceCandidate::new(relative, manifest.name));
    Ok(())
}

const fn workspace_unsupported() -> WorkspaceError {
    WorkspaceError(Reason::WorkspaceUnsupported)
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceCandidate, expand_workspace_patterns, parse_npm_workspaces, parse_pnpm_workspace,
        resolve_configured_workspace,
    };
    use crate::{configuration::WorkspaceConfig, result::Reason};
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

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
                "agent-lowmem-workspace-{nanos}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self {
                root: fs::canonicalize(root).unwrap(),
            }
        }

        fn package(&self, relative: &str, name: &str) {
            let directory = self.root.join(relative);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("package.json"),
                format!(r#"{{"name":"{name}"}}"#),
            )
            .unwrap();
        }

        fn root(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn parses_npm_array_and_packages_object_declarations() {
        let array = parse_npm_workspaces(br#"{"workspaces":["apps/*","packages/api"]}"#).unwrap();
        let object =
            parse_npm_workspaces(br#"{"workspaces":{"packages":["apps/*","packages/api"]}}"#)
                .unwrap();

        assert_eq!(array, object);
        assert_eq!(array.len(), 2);
    }

    #[test]
    fn parses_the_supported_pnpm_document_without_using_lifecycle_policy() {
        let document = parse_pnpm_workspace(
            br#"
# repository workspace policy
packages:
  - 'apps/*'
  - "packages/api"
scriptShell: /bin/sh
shellEmulator: false
enablePrePostScripts: false
"#,
        )
        .unwrap();

        assert_eq!(document.patterns.len(), 2);
        assert_eq!(document.script_shell.as_deref(), Some("/bin/sh"));
        assert_eq!(document.shell_emulator, Some(false));
        assert_eq!(document.enable_pre_post_scripts, Some(false));
    }

    #[test]
    fn rejects_unsupported_workspace_and_yaml_syntax() {
        for pattern in ["apps/**", "apps/pkg-*", "!apps/skip", "{apps,packages}/*"] {
            let npm = format!(r#"{{"workspaces":["{pattern}"]}}"#);
            assert_eq!(
                parse_npm_workspaces(npm.as_bytes()).unwrap_err().reason(),
                Reason::WorkspaceUnsupported
            );
        }

        for yaml in [
            "packages: ['apps/*']\n",
            "packages:\n\t- apps/*\n",
            "packages:\n  - &apps apps/*\n",
            "packages:\n  - |\n    apps/*\n",
            "packages:\n  - apps/*\ncatalog: {}\n",
            "scriptShell: /bin/sh\n",
            "packages:\n - apps/*\n",
            "packages:\nscriptShell: /bin/sh\n  - apps/*\n",
        ] {
            assert_eq!(
                parse_pnpm_workspace(yaml.as_bytes()).unwrap_err().reason(),
                Reason::WorkspaceUnsupported,
                "yaml should be rejected: {yaml:?}"
            );
        }
    }

    #[test]
    fn expands_literal_and_single_segment_wildcards_in_sorted_order() {
        let fixture = Fixture::new();
        fixture.package("apps/web", "@acme/web");
        fixture.package("apps/api", "@acme/api");
        fixture.package("packages/shared", "@acme/shared");
        let patterns =
            parse_npm_workspaces(br#"{"workspaces":["apps/*","packages/shared","apps/api"]}"#)
                .unwrap();

        assert_eq!(
            expand_workspace_patterns(fixture.root(), &patterns).unwrap(),
            vec![
                WorkspaceCandidate::new("apps/api", "@acme/api"),
                WorkspaceCandidate::new("apps/web", "@acme/web"),
                WorkspaceCandidate::new("packages/shared", "@acme/shared"),
            ]
        );
    }

    #[test]
    fn rejects_duplicate_package_names_and_invalid_manifests() {
        let duplicate = Fixture::new();
        duplicate.package("apps/a", "@acme/same");
        duplicate.package("apps/b", "@acme/same");
        let patterns = parse_npm_workspaces(br#"{"workspaces":["apps/*"]}"#).unwrap();
        assert_eq!(
            expand_workspace_patterns(duplicate.root(), &patterns)
                .unwrap_err()
                .reason(),
            Reason::WorkspaceCardinality
        );

        let malformed = Fixture::new();
        fs::create_dir_all(malformed.root().join("apps/bad")).unwrap();
        fs::write(
            malformed.root().join("apps/bad/package.json"),
            br#"{"private":true}"#,
        )
        .unwrap();
        assert_eq!(
            expand_workspace_patterns(malformed.root(), &patterns)
                .unwrap_err()
                .reason(),
            Reason::WorkspaceUnsupported
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_workspace_symlinks() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = Fixture::new();
        outside.package("package", "@acme/outside");
        fs::create_dir_all(fixture.root().join("apps")).unwrap();
        symlink(
            outside.root().join("package"),
            fixture.root().join("apps/escaped"),
        )
        .unwrap();
        let patterns = parse_npm_workspaces(br#"{"workspaces":["apps/*"]}"#).unwrap();

        assert_eq!(
            expand_workspace_patterns(fixture.root(), &patterns)
                .unwrap_err()
                .reason(),
            Reason::WorkspaceUnsupported
        );
    }

    #[test]
    fn resolves_only_an_exact_configured_workspace() {
        let candidates = vec![
            WorkspaceCandidate::new("apps/api", "@acme/api"),
            WorkspaceCandidate::new("apps/web", "@acme/web"),
        ];
        let exact = WorkspaceConfig {
            path: "apps/web".into(),
            package_name: "@acme/web".into(),
            operations: BTreeMap::new(),
        };
        assert_eq!(
            resolve_configured_workspace(&exact, &candidates).unwrap(),
            &candidates[1]
        );

        for (path, package_name) in [
            ("apps/missing", "@acme/web"),
            ("apps/web", "@acme/api"),
            ("apps/missing", "@acme/missing"),
        ] {
            let configured = WorkspaceConfig {
                path: path.into(),
                package_name: package_name.into(),
                operations: BTreeMap::new(),
            };
            assert_eq!(
                resolve_configured_workspace(&configured, &candidates)
                    .unwrap_err()
                    .reason(),
                Reason::WorkspaceCardinality
            );
        }
    }
}
