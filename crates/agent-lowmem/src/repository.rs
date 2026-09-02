use crate::result::Reason;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<Reason>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RootPackageManifest {
    package_manager: Option<String>,
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
    if !package_path.is_file() {
        return unsupported_repository(true, false);
    }

    let manifest = match fs::read_to_string(&package_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<RootPackageManifest>(&contents).ok())
    {
        Some(manifest) => manifest,
        None => return unsupported_repository(true, true),
    };
    let package_manager = match manifest
        .package_manager
        .as_deref()
        .and_then(parse_package_manager)
    {
        Some(package_manager) => package_manager,
        None => return unsupported_package_manager(),
    };

    let npm_lock = root.as_path().join("package-lock.json").is_file();
    let pnpm_lock = root.as_path().join("pnpm-lock.yaml").is_file();
    let lockfile_matches = match package_manager.kind {
        PackageManagerKind::Npm => npm_lock && !pnpm_lock,
        PackageManagerKind::Pnpm => pnpm_lock && !npm_lock,
    };
    if !lockfile_matches {
        return unsupported_package_manager();
    }

    RepositoryReport {
        git_root_available: true,
        root_package_available: true,
        package_manager: Some(package_manager),
        failure_reason: None,
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
        failure_reason: Some(Reason::RepositoryUnsupported),
    }
}

fn unsupported_package_manager() -> RepositoryReport {
    RepositoryReport {
        git_root_available: true,
        root_package_available: true,
        package_manager: None,
        failure_reason: Some(Reason::PackageManagerUnsupported),
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
        fixture.write("package.json", r#"{"packageManager":"pnpm@10.33.0"}"#);
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert!(report.git_root_available);
        let manager = report.package_manager.as_ref().unwrap();
        assert_eq!(manager.kind, PackageManagerKind::Pnpm);
        assert_eq!(manager.version.to_string(), "10.33.0");
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains(fixture.path().to_str().unwrap())
        );
    }

    #[test]
    fn rejects_a_declared_manager_with_the_wrong_lockfile() {
        let fixture = Fixture::git_repo();
        fixture.write("package.json", r#"{"packageManager":"npm@11.11.0"}"#);
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
        fixture.write("package.json", r#"{"packageManager":"npm@11.11.0"}"#);
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
        fixture.write("package.json", r#"{"packageManager":"npm@11.11.0"}"#);
        fixture.write("package-lock.json", "{}\n");
        fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

        let report = inspect_repository(fixture.path());

        assert_eq!(
            report.failure_reason,
            Some(Reason::PackageManagerUnsupported)
        );
    }
}
