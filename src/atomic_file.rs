use crate::{configuration::valid_relative_path, result::Reason};
use rustix::{
    fs::{
        AtFlags, FileType, Mode, OFlags, fchmod, mkdirat, open, openat, renameat, statat, unlinkat,
    },
    io::Errno,
};
use sha2::{Digest, Sha256};
use std::{
    collections::hash_map::RandomState,
    fmt::{self, Write as _},
    fs::File,
    hash::{BuildHasher, Hash, Hasher},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum OptionalFile {
    Absent,
    Regular {
        bytes: Vec<u8>,
        sha256: [u8; 32],
        mode: u32,
        owner_uid: u32,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ExpectedState {
    Absent,
    Regular {
        byte_length: usize,
        sha256: [u8; 32],
        mode: u32,
        owner_uid: u32,
    },
}

impl fmt::Debug for ExpectedState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("ExpectedState::Absent"),
            Self::Regular {
                byte_length,
                sha256,
                mode,
                owner_uid,
            } => formatter
                .debug_struct("ExpectedState::Regular")
                .field("byte_length", byte_length)
                .field("sha256", &hex_digest(sha256))
                .field("mode", mode)
                .field("owner_uid", owner_uid)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePrecondition {
    pub state: ExpectedState,
}

impl From<&OptionalFile> for FilePrecondition {
    fn from(value: &OptionalFile) -> Self {
        let state = match value {
            OptionalFile::Absent => ExpectedState::Absent,
            OptionalFile::Regular {
                bytes,
                sha256,
                mode,
                owner_uid,
            } => ExpectedState::Regular {
                byte_length: bytes.len(),
                sha256: *sha256,
                mode: *mode,
                owner_uid: *owner_uid,
            },
        };
        Self { state }
    }
}

pub(crate) struct HeldDirectory {
    descriptor: File,
}

impl fmt::Debug for HeldDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeldDirectory")
            .finish_non_exhaustive()
    }
}

impl HeldDirectory {
    pub fn open(path: &Path, expected_mode: Option<u32>) -> Result<Self, Reason> {
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| Reason::ManagedFileConflict)?;
        let directory = Self {
            descriptor: File::from(descriptor),
        };
        directory.validate(expected_mode)?;
        Ok(directory)
    }

    // Task 9 wires this Task 7 primitive into the managed transaction applier.
    #[allow(dead_code)]
    pub fn open_or_create_private(
        parent: &HeldDirectory,
        name: &str,
        mode: u32,
    ) -> Result<Self, Reason> {
        if !valid_component(name) || mode != 0o700 {
            return Err(Reason::ManagedFileConflict);
        }
        let requested = Mode::from_raw_mode(mode as _);
        let created = match mkdirat(&parent.descriptor, name, requested) {
            Ok(()) => true,
            Err(Errno::EXIST) => false,
            Err(_) => return Err(Reason::InternalError),
        };
        let opened = Self::open_child(parent, name, None);
        let directory = match opened {
            Ok(directory) => directory,
            Err(error) => {
                if created {
                    let _ = unlinkat(&parent.descriptor, name, AtFlags::REMOVEDIR);
                }
                return Err(error);
            }
        };

        if created {
            if fchmod(&directory.descriptor, requested).is_err()
                || directory.validate(Some(mode)).is_err()
                || directory.sync().is_err()
                || parent.sync().is_err()
            {
                drop(directory);
                let _ = unlinkat(&parent.descriptor, name, AtFlags::REMOVEDIR);
                return Err(Reason::InternalError);
            }
        } else {
            directory.validate(Some(mode))?;
        }
        Ok(directory)
    }

    pub(crate) fn open_child(
        parent: &HeldDirectory,
        name: &str,
        expected_mode: Option<u32>,
    ) -> Result<Self, Reason> {
        if !valid_component(name) {
            return Err(Reason::ManagedFileConflict);
        }
        let descriptor = openat(
            &parent.descriptor,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| Reason::ManagedFileConflict)?;
        let directory = Self {
            descriptor: File::from(descriptor),
        };
        directory.validate(expected_mode)?;
        Ok(directory)
    }

    // Task 8 consumes bounded snapshots through the held repository directory.
    #[allow(dead_code)]
    pub fn read_optional(&self, name: &str, limit: usize) -> Result<OptionalFile, Reason> {
        if !valid_component(name) {
            return Err(Reason::ManagedFileConflict);
        }
        read_optional_bounded(&self.descriptor, name, limit)
    }

    pub(crate) fn precondition(&self, name: &str) -> Result<FilePrecondition, Reason> {
        if !valid_component(name) {
            return Err(Reason::ManagedFileConflict);
        }
        Ok(FilePrecondition {
            state: self.inspect_expected_state(name)?,
        })
    }

    pub(crate) fn ensure_replaceable(&self, name: &str) -> Result<(), Reason> {
        if !valid_component(name) {
            return Err(Reason::ManagedFileConflict);
        }
        match statat(&self.descriptor, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile => Ok(()),
            Ok(_) => Err(Reason::ManagedFileConflict),
            Err(Errno::NOENT) => Ok(()),
            Err(_) => Err(Reason::ManagedFileConflict),
        }
    }

    pub fn replace_atomic(
        &self,
        name: &str,
        expected: &FilePrecondition,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), Reason> {
        self.replace_atomic_with_hook(name, expected, bytes, mode, || {})
    }

    fn replace_atomic_with_hook(
        &self,
        name: &str,
        expected: &FilePrecondition,
        bytes: &[u8],
        mode: u32,
        before_revalidate: impl FnOnce(),
    ) -> Result<(), Reason> {
        if !valid_component(name) || !valid_regular_mode(mode) {
            return Err(Reason::ManagedFileConflict);
        }
        self.require_exact(name, expected)?;
        let (temporary_name, mut temporary) = self.create_temporary(mode)?;
        let result = (|| {
            temporary
                .write_all(bytes)
                .map_err(|_| Reason::InternalError)?;
            temporary.sync_all().map_err(|_| Reason::InternalError)?;
            before_revalidate();
            self.require_exact(name, expected)?;
            renameat(
                &self.descriptor,
                temporary_name.as_str(),
                &self.descriptor,
                name,
            )
            .map_err(|_| Reason::InternalError)?;
            self.sync()
        })();
        if result.is_err() {
            let _ = unlinkat(&self.descriptor, temporary_name.as_str(), AtFlags::empty());
        }
        result
    }

    // Task 9 wires exact deletion into rollback and restore transactions.
    #[allow(dead_code)]
    pub fn remove_exact(&self, name: &str, expected: &FilePrecondition) -> Result<(), Reason> {
        if !valid_component(name) {
            return Err(Reason::ManagedFileConflict);
        }
        self.require_exact(name, expected)?;
        if matches!(expected.state, ExpectedState::Absent) {
            return Ok(());
        }
        unlinkat(&self.descriptor, name, AtFlags::empty()).map_err(|error| {
            if error == Errno::NOENT {
                Reason::ManagedFileConflict
            } else {
                Reason::InternalError
            }
        })?;
        self.sync()
    }

    pub fn sync(&self) -> Result<(), Reason> {
        self.descriptor
            .sync_all()
            .map_err(|_| Reason::InternalError)
    }

    fn validate(&self, expected_mode: Option<u32>) -> Result<(), Reason> {
        let metadata = self
            .descriptor
            .metadata()
            .map_err(|_| Reason::ManagedFileConflict)?;
        if !metadata.is_dir() {
            return Err(Reason::ManagedFileConflict);
        }
        if let Some(mode) = expected_mode {
            if mode > 0o777
                || metadata.mode() & 0o777 != mode
                || metadata.uid() != rustix::process::geteuid().as_raw()
            {
                return Err(Reason::ManagedFileConflict);
            }
        }
        Ok(())
    }

    fn require_exact(&self, name: &str, expected: &FilePrecondition) -> Result<(), Reason> {
        let current = self.inspect_expected_state(name)?;
        if expected.state == current {
            Ok(())
        } else {
            Err(Reason::ManagedFileConflict)
        }
    }

    fn inspect_expected_state(&self, name: &str) -> Result<ExpectedState, Reason> {
        let descriptor = match openat(
            &self.descriptor,
            name,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(Errno::NOENT) => return Ok(ExpectedState::Absent),
            Err(_) => return Err(Reason::ManagedFileConflict),
        };
        let mut file = File::from(descriptor);
        let before = file.metadata().map_err(|_| Reason::ManagedFileConflict)?;
        if !before.is_file() {
            return Err(Reason::ManagedFileConflict);
        }

        let mut sha256 = Sha256::new();
        let mut byte_length = 0usize;
        let mut buffer = [0u8; 8_192];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| Reason::ManagedFileConflict)?;
            if read == 0 {
                break;
            }
            byte_length = byte_length
                .checked_add(read)
                .ok_or(Reason::ManagedFileConflict)?;
            sha256.update(&buffer[..read]);
        }
        let after = file.metadata().map_err(|_| Reason::ManagedFileConflict)?;
        if !after.is_file()
            || before.len() != after.len()
            || before.mode() != after.mode()
            || before.uid() != after.uid()
            || after.len() != byte_length as u64
        {
            return Err(Reason::ManagedFileConflict);
        }
        Ok(ExpectedState::Regular {
            byte_length,
            sha256: sha256.finalize().into(),
            mode: after.mode() & 0o7777,
            owner_uid: after.uid(),
        })
    }

    fn create_temporary(&self, mode: u32) -> Result<(String, File), Reason> {
        for _ in 0..16 {
            let name = unpredictable_temporary_name();
            match openat(
                &self.descriptor,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(mode as _),
            ) {
                Ok(descriptor) => {
                    let file = File::from(descriptor);
                    if fchmod(&file, Mode::from_raw_mode(mode as _)).is_err() {
                        let _ = unlinkat(&self.descriptor, name.as_str(), AtFlags::empty());
                        return Err(Reason::InternalError);
                    }
                    return Ok((name, file));
                }
                Err(Errno::EXIST) => continue,
                Err(_) => return Err(Reason::InternalError),
            }
        }
        Err(Reason::InternalError)
    }
}

impl fmt::Debug for OptionalFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => formatter.write_str("OptionalFile::Absent"),
            Self::Regular {
                bytes,
                sha256,
                mode,
                ..
            } => formatter
                .debug_struct("OptionalFile::Regular")
                .field("byte_length", &bytes.len())
                .field("sha256", &hex_digest(sha256))
                .field("mode", mode)
                .finish_non_exhaustive(),
        }
    }
}

pub(crate) fn read_optional_bounded(
    directory: &File,
    identity: &str,
    max_bytes: usize,
) -> Result<OptionalFile, Reason> {
    if !valid_relative_path(identity) {
        return Err(Reason::ManagedFileConflict);
    }
    let components = identity.split('/').collect::<Vec<_>>();
    let mut parent = directory.try_clone().map_err(|_| Reason::InternalError)?;
    for component in &components[..components.len() - 1] {
        let descriptor = match openat(
            &parent,
            *component,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(error) if error == rustix::io::Errno::NOENT => return Ok(OptionalFile::Absent),
            Err(_) => return Err(Reason::ManagedFileConflict),
        };
        parent = File::from(descriptor);
        if !parent
            .metadata()
            .map_err(|_| Reason::InternalError)?
            .is_dir()
        {
            return Err(Reason::ManagedFileConflict);
        }
    }

    let file_name = components.last().expect("validated non-empty identity");
    let descriptor = match openat(
        &parent,
        *file_name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(descriptor) => descriptor,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(OptionalFile::Absent),
        Err(_) => return Err(Reason::ManagedFileConflict),
    };
    let mut file = File::from(descriptor);
    let metadata = file.metadata().map_err(|_| Reason::InternalError)?;
    if !metadata.is_file() || metadata.len() > max_bytes as u64 {
        return Err(Reason::ManagedFileConflict);
    }

    let capacity = usize::try_from(metadata.len()).map_err(|_| Reason::ManagedFileConflict)?;
    let read_limit = u64::try_from(max_bytes)
        .ok()
        .and_then(|limit| limit.checked_add(1))
        .ok_or(Reason::InternalError)?;
    let mut bytes = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|_| Reason::InternalError)?;
    if bytes.len() > max_bytes {
        return Err(Reason::ManagedFileConflict);
    }

    let sha256 = Sha256::digest(&bytes).into();
    Ok(OptionalFile::Regular {
        bytes,
        sha256,
        mode: metadata.mode() & 0o7777,
        owner_uid: metadata.uid(),
    })
}

fn hex_digest(digest: &[u8; 32]) -> String {
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

fn valid_component(name: &str) -> bool {
    valid_relative_path(name) && !name.contains('/')
}

fn valid_regular_mode(mode: u32) -> bool {
    mode <= 0o666 && mode & 0o111 == 0
}

fn unpredictable_temporary_name() -> String {
    let serial = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut hasher = RandomState::new().build_hasher();
    std::process::id().hash(&mut hasher);
    serial.hash(&mut hasher);
    nanos.hash(&mut hasher);
    format!(".agent-lowmem-{:016x}-{serial:016x}.tmp", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{FilePrecondition, HeldDirectory, OptionalFile, read_optional_bounded};
    use crate::result::Reason;
    use std::{
        fs::{self, File},
        os::unix::fs::PermissionsExt,
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
                "agent-lowmem-atomic-file-{nanos}-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }

        fn directory(&self) -> File {
            File::open(&self.0).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_absent_and_exact_bounded_regular_file_state() {
        let fixture = Fixture::new();
        assert!(matches!(
            read_optional_bounded(&fixture.directory(), "absent.json", 16).unwrap(),
            OptionalFile::Absent
        ));
        fs::create_dir(fixture.0.join("private")).unwrap();
        fs::write(fixture.0.join("private/state.json"), b"abc").unwrap();
        fs::set_permissions(
            fixture.0.join("private/state.json"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();

        let state = read_optional_bounded(&fixture.directory(), "private/state.json", 3).unwrap();
        let OptionalFile::Regular {
            bytes,
            sha256,
            mode,
            owner_uid,
        } = state
        else {
            panic!("expected a regular file");
        };

        assert_eq!(bytes, b"abc");
        assert_eq!(
            sha256,
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
        assert_eq!(mode, 0o640);
        assert_eq!(owner_uid, rustix::process::geteuid().as_raw());
    }

    #[test]
    fn rejects_files_larger_than_the_requested_bound() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("state.json"), b"abcd").unwrap();

        assert_eq!(
            read_optional_bounded(&fixture.directory(), "state.json", 3).unwrap_err(),
            Reason::ManagedFileConflict
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_parent_final_and_special_file_identities() {
        use std::os::unix::{fs::symlink, net::UnixListener};

        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::write(outside.0.join("secret"), b"secret-value").unwrap();
        symlink(&outside.0, fixture.0.join("linked-parent")).unwrap();
        symlink(outside.0.join("secret"), fixture.0.join("linked-file")).unwrap();
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let short_directory =
            PathBuf::from(format!("/tmp/al-socket-{}-{serial}", std::process::id()));
        fs::create_dir(&short_directory).unwrap();
        let short_socket = short_directory.join("s");
        let _socket = UnixListener::bind(&short_socket).unwrap();
        fs::rename(short_socket, fixture.0.join("socket")).unwrap();
        fs::remove_dir(short_directory).unwrap();

        for identity in ["linked-parent/secret", "linked-file", "socket"] {
            assert_eq!(
                read_optional_bounded(&fixture.directory(), identity, 64).unwrap_err(),
                Reason::ManagedFileConflict
            );
        }
    }

    #[test]
    fn rejects_non_relative_or_structurally_invalid_identities() {
        let fixture = Fixture::new();

        for identity in ["", "/absolute", "../outside", "a/../b", "a//b", "a\\b"] {
            assert_eq!(
                read_optional_bounded(&fixture.directory(), identity, 16).unwrap_err(),
                Reason::ManagedFileConflict
            );
        }
    }

    #[test]
    fn debug_output_redacts_regular_file_bytes() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("state.json"), b"private-secret-value").unwrap();

        let state = read_optional_bounded(&fixture.directory(), "state.json", 64).unwrap();
        let debug = format!("{state:?}");

        assert!(debug.contains("byte_length"));
        assert!(debug.contains("sha256"));
        assert!(!debug.contains("private-secret-value"));
    }

    #[test]
    fn creates_private_directories_and_atomically_replaces_exact_state() {
        let fixture = Fixture::new();
        let root = HeldDirectory::open(&fixture.0, None).unwrap();
        let private = HeldDirectory::open_or_create_private(&root, "agent-lowmem", 0o700).unwrap();

        fs::create_dir(fixture.0.join("wrong-mode")).unwrap();
        fs::set_permissions(
            fixture.0.join("wrong-mode"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(
            HeldDirectory::open_or_create_private(&root, "wrong-mode", 0o700).unwrap_err(),
            Reason::ManagedFileConflict
        );

        assert_eq!(
            fs::metadata(fixture.0.join("agent-lowmem"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        let absent = private.read_optional("state.json", 64).unwrap();
        private
            .replace_atomic(
                "state.json",
                &FilePrecondition::from(&absent),
                b"prepared\n",
                0o600,
            )
            .unwrap();
        assert_eq!(
            fs::read(fixture.0.join("agent-lowmem/state.json")).unwrap(),
            b"prepared\n"
        );
        assert_eq!(
            fs::metadata(fixture.0.join("agent-lowmem/state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let prepared = private.read_optional("state.json", 64).unwrap();
        private
            .replace_atomic(
                "state.json",
                &FilePrecondition::from(&prepared),
                b"applied\n",
                0o600,
            )
            .unwrap();
        let applied = private.read_optional("state.json", 64).unwrap();
        fs::write(
            fixture.0.join("agent-lowmem/state.json"),
            b"external-change\n",
        )
        .unwrap();
        assert_eq!(
            private.remove_exact("state.json", &FilePrecondition::from(&applied)),
            Err(Reason::ManagedFileConflict)
        );
        let changed = private.read_optional("state.json", 64).unwrap();
        private
            .remove_exact("state.json", &FilePrecondition::from(&changed))
            .unwrap();
        assert!(!fixture.0.join("agent-lowmem/state.json").exists());
    }

    #[test]
    fn preserves_requested_non_executable_mode_and_rejects_drift() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("AGENTS.md"), b"before").unwrap();
        fs::set_permissions(
            fixture.0.join("AGENTS.md"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        let root = HeldDirectory::open(&fixture.0, None).unwrap();
        let before = root.read_optional("AGENTS.md", 64).unwrap();
        let expected = FilePrecondition::from(&before);

        fs::write(fixture.0.join("AGENTS.md"), b"drifted").unwrap();
        assert_eq!(
            root.replace_atomic("AGENTS.md", &expected, b"managed", 0o640),
            Err(Reason::ManagedFileConflict)
        );
        assert_eq!(fs::read(fixture.0.join("AGENTS.md")).unwrap(), b"drifted");
        assert!(temporary_entries(&fixture).is_empty());

        let drifted = root.read_optional("AGENTS.md", 64).unwrap();
        root.replace_atomic(
            "AGENTS.md",
            &FilePrecondition::from(&drifted),
            b"managed",
            0o640,
        )
        .unwrap();
        let mode = fs::metadata(fixture.0.join("AGENTS.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o640);
        assert_eq!(mode & 0o111, 0);
    }

    #[test]
    fn removes_the_temporary_when_the_precondition_changes_before_rename() {
        let fixture = Fixture::new();
        let target = fixture.0.join("state.json");
        fs::write(&target, b"before").unwrap();
        let root = HeldDirectory::open(&fixture.0, None).unwrap();
        let before = root.read_optional("state.json", 64).unwrap();

        assert_eq!(
            root.replace_atomic_with_hook(
                "state.json",
                &FilePrecondition::from(&before),
                b"target",
                0o600,
                || fs::write(&target, b"changed-before-rename").unwrap(),
            ),
            Err(Reason::ManagedFileConflict)
        );
        assert_eq!(fs::read(target).unwrap(), b"changed-before-rename");
        assert!(temporary_entries(&fixture).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn held_parent_survives_path_swap_and_final_symlinks_and_special_files_fail_closed() {
        use std::os::unix::{fs::symlink, net::UnixListener};

        let fixture = Fixture::new();
        let outside = Fixture::new();
        fs::create_dir(fixture.0.join("held")).unwrap();
        let held = HeldDirectory::open(&fixture.0.join("held"), None).unwrap();
        fs::rename(fixture.0.join("held"), fixture.0.join("moved")).unwrap();
        symlink(&outside.0, fixture.0.join("held")).unwrap();

        let absent = held.read_optional("state.json", 64).unwrap();
        held.replace_atomic(
            "state.json",
            &FilePrecondition::from(&absent),
            b"inside-held-directory",
            0o600,
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.0.join("moved/state.json")).unwrap(),
            b"inside-held-directory"
        );
        assert!(!outside.0.join("state.json").exists());

        let absent_link = held.read_optional("link", 64).unwrap();
        symlink(outside.0.join("target"), fixture.0.join("moved/link")).unwrap();
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let short_directory =
            PathBuf::from(format!("/tmp/al-held-{}-{serial}", std::process::id()));
        fs::create_dir(&short_directory).unwrap();
        let short_socket = short_directory.join("s");
        let socket = UnixListener::bind(&short_socket).unwrap();
        fs::rename(short_socket, fixture.0.join("moved/socket")).unwrap();
        fs::remove_dir(short_directory).unwrap();
        assert_eq!(
            held.replace_atomic(
                "link",
                &FilePrecondition::from(&absent_link),
                b"must-not-escape",
                0o600,
            ),
            Err(Reason::ManagedFileConflict)
        );
        for name in ["link", "socket"] {
            assert_eq!(
                held.read_optional(name, 64),
                Err(Reason::ManagedFileConflict)
            );
        }
        drop(socket);
        assert!(temporary_entries_at(&fixture.0.join("moved")).is_empty());
    }

    fn temporary_entries(fixture: &Fixture) -> Vec<String> {
        temporary_entries_at(&fixture.0)
    }

    fn temporary_entries_at(path: &std::path::Path) -> Vec<String> {
        fs::read_dir(path)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".agent-lowmem-") && name.ends_with(".tmp"))
            .collect()
    }
}
