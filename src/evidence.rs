use crate::{configuration::valid_relative_path, result::Reason};
use rustix::fs::{Mode, OFlags, openat};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, fs::File, io::Read, path::Path};

#[derive(Debug)]
pub struct EvidenceReader {
    root: File,
}

impl EvidenceReader {
    pub fn new(root: &Path) -> Result<Self, Reason> {
        let root = File::open(root).map_err(|_| Reason::RepositoryUnsupported)?;
        if !root
            .metadata()
            .map_err(|_| Reason::RepositoryUnsupported)?
            .is_dir()
        {
            return Err(Reason::RepositoryUnsupported);
        }
        Ok(Self { root })
    }

    pub fn read(&self, relative_path: &str) -> Result<EvidenceFile, Reason> {
        if !valid_relative_path(relative_path) {
            return Err(Reason::RepositoryUnsupported);
        }

        let components = relative_path.split('/').collect::<Vec<_>>();
        let mut current = self
            .root
            .try_clone()
            .map_err(|_| Reason::RepositoryUnsupported)?;
        for (index, component) in components.iter().enumerate() {
            let final_component = index + 1 == components.len();
            let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW;
            if !final_component {
                flags |= OFlags::DIRECTORY;
            }
            let descriptor = openat(&current, *component, flags, Mode::empty())
                .map_err(|_| Reason::RepositoryUnsupported)?;
            let next = File::from(descriptor);
            let metadata = next.metadata().map_err(|_| Reason::RepositoryUnsupported)?;
            if (final_component && !metadata.is_file()) || (!final_component && !metadata.is_dir())
            {
                return Err(Reason::RepositoryUnsupported);
            }
            current = next;
        }

        let mut bytes = Vec::new();
        current
            .read_to_end(&mut bytes)
            .map_err(|_| Reason::RepositoryUnsupported)?;
        let sha256 = Sha256::digest(&bytes).into();
        Ok(EvidenceFile {
            relative_path: relative_path.to_owned(),
            bytes,
            sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceFile {
    relative_path: String,
    bytes: Vec<u8>,
    sha256: [u8; 32],
}

impl EvidenceFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn digest(&self) -> EvidenceDigest {
        EvidenceDigest {
            relative_path: self.relative_path.clone(),
            sha256: self.sha256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceDigest {
    relative_path: String,
    sha256: [u8; 32],
}

impl EvidenceDigest {
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub fn hex(&self) -> String {
        self.sha256
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                write!(output, "{byte:02x}").expect("writing to a String cannot fail");
                output
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSnapshot {
    files: Vec<EvidenceDigest>,
}

impl EvidenceSnapshot {
    pub fn new(mut files: Vec<EvidenceDigest>) -> Result<Self, Reason> {
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut unique: Vec<EvidenceDigest> = Vec::with_capacity(files.len());
        for file in files {
            match unique.last() {
                Some(previous) if previous.relative_path == file.relative_path => {
                    if previous.sha256 != file.sha256 {
                        return Err(Reason::RepositoryUnsupported);
                    }
                }
                _ => unique.push(file),
            }
        }
        Ok(Self { files: unique })
    }

    pub fn files(&self) -> &[EvidenceDigest] {
        &self.files
    }
}

#[cfg(test)]
mod tests {
    use super::{EvidenceReader, EvidenceSnapshot};
    use crate::result::Reason;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "agent-lowmem-evidence-{nanos}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn hashes_the_exact_bytes_that_are_returned() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("package.json"), b"abc").unwrap();
        let reader = EvidenceReader::new(&fixture.0).unwrap();

        let file = reader.read("package.json").unwrap();

        assert_eq!(file.bytes(), b"abc");
        assert_eq!(
            file.digest().hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn snapshots_sort_evidence_by_relative_identity() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("z.json"), b"z").unwrap();
        fs::write(fixture.0.join("a.json"), b"a").unwrap();
        let reader = EvidenceReader::new(&fixture.0).unwrap();

        let snapshot = EvidenceSnapshot::new(vec![
            reader.read("z.json").unwrap().digest(),
            reader.read("a.json").unwrap().digest(),
        ])
        .unwrap();

        assert_eq!(snapshot.files()[0].relative_path(), "a.json");
        assert_eq!(snapshot.files()[1].relative_path(), "z.json");
    }

    #[test]
    fn snapshots_collapse_identical_duplicates_and_reject_conflicts() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("package.json"), b"first").unwrap();
        let reader = EvidenceReader::new(&fixture.0).unwrap();
        let first = reader.read("package.json").unwrap().digest();

        let identical = EvidenceSnapshot::new(vec![first.clone(), first.clone()]).unwrap();
        assert_eq!(identical.files().len(), 1);

        fs::write(fixture.0.join("package.json"), b"second").unwrap();
        let second = reader.read("package.json").unwrap().digest();
        assert_eq!(
            EvidenceSnapshot::new(vec![first, second]).unwrap_err(),
            Reason::RepositoryUnsupported
        );
    }

    #[test]
    fn rejects_paths_outside_the_canonical_root() {
        let fixture = Fixture::new();
        let reader = EvidenceReader::new(&fixture.0).unwrap();

        assert_eq!(
            reader.read("../outside").unwrap_err(),
            Reason::RepositoryUnsupported
        );
        assert_eq!(
            reader.read("/tmp/outside").unwrap_err(),
            Reason::RepositoryUnsupported
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_evidence() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("secret.json"), b"secret").unwrap();
        symlink(outside.0.join("secret.json"), fixture.0.join("linked.json")).unwrap();
        let reader = EvidenceReader::new(&fixture.0).unwrap();

        assert_eq!(
            reader.read("linked.json").unwrap_err(),
            Reason::RepositoryUnsupported
        );
    }
}
