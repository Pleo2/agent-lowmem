use crate::{configuration::valid_relative_path, result::Reason};
use rustix::fs::{Mode, OFlags, openat};
use sha2::{Digest, Sha256};
use std::{
    fmt::{self, Write as _},
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
};

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
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
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
    file.by_ref()
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
        mode: metadata.mode() & 0o777,
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

#[cfg(test)]
mod tests {
    use super::{OptionalFile, read_optional_bounded};
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
}
