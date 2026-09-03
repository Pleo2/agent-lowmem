use crate::{configuration::valid_key, result::Reason};
use rustix::{
    fs::{FlockOperation, Mode, OFlags, flock, openat},
    process::{Pid, geteuid, getpgid, test_kill_process_group},
};
use serde::{Deserialize, Serialize};
use std::{
    env, fmt, fs,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::Path,
};

const LOCK_FILE: &str = "operation.lock";
const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessIdentity {
    pid: i32,
    start_identity: u64,
}

impl ProcessIdentity {
    pub fn current() -> Result<Self, Reason> {
        Self::for_pid(std::process::id() as i32)
    }

    pub fn for_pid(pid: i32) -> Result<Self, Reason> {
        if pid <= 0 {
            return Err(Reason::InternalError);
        }
        Ok(Self {
            pid,
            start_identity: process_start_identity(pid)?,
        })
    }

    pub const fn start_identity(self) -> u64 {
        self.start_identity
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChildGroupIdentity {
    process_group_id: i32,
    leader_start_identity: u64,
}

impl ChildGroupIdentity {
    pub const fn new(process_group_id: i32, leader_start_identity: u64) -> Self {
        Self {
            process_group_id,
            leader_start_identity,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseRecord {
    schema_version: u8,
    owner: ProcessIdentity,
    repository_hash: String,
    operation_key: String,
    acquired_at_unix_seconds: u64,
    child_group: Option<ChildGroupIdentity>,
}

impl LeaseRecord {
    pub fn new(
        owner: ProcessIdentity,
        repository_hash: [u8; 32],
        operation_key: impl Into<String>,
        acquired_at_unix_seconds: u64,
    ) -> Result<Self, Reason> {
        let operation_key = operation_key.into();
        if !valid_key(&operation_key) || acquired_at_unix_seconds == 0 {
            return Err(Reason::InternalError);
        }
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            owner,
            repository_hash: hex_digest(&repository_hash),
            operation_key,
            acquired_at_unix_seconds,
            child_group: None,
        })
    }

    fn valid(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.owner.pid > 0
            && self.owner.start_identity > 0
            && self.repository_hash.len() == 64
            && self
                .repository_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && valid_key(&self.operation_key)
            && self.acquired_at_unix_seconds > 0
            && self
                .child_group
                .is_none_or(|group| group.process_group_id > 0 && group.leader_start_identity > 0)
    }
}

impl fmt::Debug for LeaseRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LeaseRecord")
            .field("schema_version", &self.schema_version)
            .field("operation_key", &self.operation_key)
            .field("child_group_present", &self.child_group.is_some())
            .finish()
    }
}

pub struct UserLease {
    file: File,
    record: LeaseRecord,
}

impl fmt::Debug for UserLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UserLease")
            .field("record", &self.record)
            .finish_non_exhaustive()
    }
}

impl UserLease {
    pub fn acquire(runtime: &Path, record: LeaseRecord) -> Result<Self, Reason> {
        if env::var_os("AGENT_LOWMEM_ACTIVE").as_deref() == Some(std::ffi::OsStr::new("1")) {
            return Err(Reason::NestedInvocation);
        }
        if !record.valid() {
            return Err(Reason::InternalError);
        }
        ensure_runtime_directory(runtime)?;
        let mut file = open_lock(runtime, true)?.ok_or(Reason::InternalError)?;
        flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|_| Reason::LockHeld)?;

        if let Some(previous) = read_record(&mut file)? {
            if !previous.valid() {
                return Err(Reason::LockHeld);
            }
            if previous.child_group.is_some_and(group_is_live) {
                return Err(Reason::LockHeld);
            }
        }
        write_record(&mut file, &record)?;
        Ok(Self { file, record })
    }

    pub fn set_child_group(&mut self, group: ChildGroupIdentity) -> Result<(), Reason> {
        if group.process_group_id <= 0 || group.leader_start_identity == 0 {
            return Err(Reason::InternalError);
        }
        self.record.child_group = Some(group);
        write_record(&mut self.file, &self.record)
    }

    pub fn clear_child_group(&mut self) -> Result<(), Reason> {
        self.record.child_group = None;
        write_record(&mut self.file, &self.record)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockStatus {
    Available,
    Held,
    OrphanRecovery,
    InvalidRecord,
}

#[derive(Debug, Clone, Copy)]
pub struct LockProbe;

impl LockProbe {
    pub fn probe(runtime: &Path) -> LockStatus {
        match fs::symlink_metadata(runtime) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return LockStatus::Available;
            }
            Err(_) => return LockStatus::InvalidRecord,
        }
        if validate_runtime_directory(runtime).is_err() {
            return LockStatus::InvalidRecord;
        }
        let mut file = match open_lock(runtime, false) {
            Ok(Some(file)) => file,
            Ok(None) => return LockStatus::Available,
            Err(_) => return LockStatus::InvalidRecord,
        };
        if flock(&file, FlockOperation::NonBlockingLockExclusive).is_err() {
            return LockStatus::Held;
        }
        match read_record(&mut file) {
            Ok(None) => LockStatus::Available,
            Ok(Some(record)) if !record.valid() => LockStatus::InvalidRecord,
            Ok(Some(record)) if record.child_group.is_some_and(group_is_live) => {
                LockStatus::OrphanRecovery
            }
            Ok(Some(_)) => LockStatus::Available,
            Err(_) => LockStatus::InvalidRecord,
        }
    }
}

fn ensure_runtime_directory(runtime: &Path) -> Result<(), Reason> {
    match fs::symlink_metadata(runtime) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(runtime) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(Reason::LockHeld),
            }
            fs::set_permissions(runtime, fs::Permissions::from_mode(0o700))
                .map_err(|_| Reason::LockHeld)?;
        }
        Err(_) => return Err(Reason::LockHeld),
    }
    validate_runtime_directory(runtime)
}

fn validate_runtime_directory(runtime: &Path) -> Result<(), Reason> {
    let metadata = fs::symlink_metadata(runtime).map_err(|_| Reason::LockHeld)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(Reason::LockHeld);
    }
    Ok(())
}

fn open_lock(runtime: &Path, create: bool) -> Result<Option<File>, Reason> {
    let directory = File::open(runtime).map_err(|_| Reason::LockHeld)?;
    let mut flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    if create {
        flags |= OFlags::CREATE;
    }
    let descriptor = match openat(&directory, LOCK_FILE, flags, Mode::RUSR | Mode::WUSR) {
        Ok(descriptor) => descriptor,
        Err(error) if !create && error == rustix::io::Errno::NOENT => return Ok(None),
        Err(_) => return Err(Reason::LockHeld),
    };
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(|_| Reason::LockHeld)?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(Reason::LockHeld);
    }
    Ok(Some(file))
}

fn read_record(file: &mut File) -> Result<Option<LeaseRecord>, Reason> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Reason::LockHeld)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| Reason::LockHeld)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Reason::LockHeld)
}

fn write_record(file: &mut File, record: &LeaseRecord) -> Result<(), Reason> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| Reason::InternalError)?;
    file.set_len(0).map_err(|_| Reason::InternalError)?;
    serde_json::to_writer(&mut *file, record).map_err(|_| Reason::InternalError)?;
    file.write_all(b"\n").map_err(|_| Reason::InternalError)?;
    file.flush().map_err(|_| Reason::InternalError)?;
    file.sync_all().map_err(|_| Reason::InternalError)
}

fn group_is_live(group: ChildGroupIdentity) -> bool {
    let Ok(identity) = ProcessIdentity::for_pid(group.process_group_id) else {
        return false;
    };
    if identity.start_identity != group.leader_start_identity {
        return false;
    }
    let Some(pid) = Pid::from_raw(group.process_group_id) else {
        return false;
    };
    getpgid(Some(pid)).is_ok_and(|actual| actual == pid) && test_kill_process_group(pid).is_ok()
}

#[cfg(target_os = "macos")]
fn process_start_identity(pid: i32) -> Result<u64, Reason> {
    use libproc::libproc::pid_rusage::{RUsageInfoV4, pidrusage};

    let usage = pidrusage::<RUsageInfoV4>(pid).map_err(|_| Reason::InternalError)?;
    (usage.ri_proc_start_abstime > 0)
        .then_some(usage.ri_proc_start_abstime)
        .ok_or(Reason::InternalError)
}

#[cfg(not(target_os = "macos"))]
fn process_start_identity(_pid: i32) -> Result<u64, Reason> {
    Err(Reason::HostUnsupported)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    use fmt::Write as _;

    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}
