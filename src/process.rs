use crate::{
    lock::{ChildGroupIdentity, ProcessIdentity, UserLease},
    repository::RunPlan,
    result::Reason,
};
use rustix::process::{Pid, Signal, getpgid, kill_process_group, test_kill_process_group};
use signal_hook::{
    consts::signal::{SIGHUP, SIGINT, SIGTERM},
    iterator::{Handle, Signals},
};
use std::{
    fmt, io,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct ManagedChild {
    child: Child,
    group: OwnedProcessGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnError {
    pub reason: Reason,
    pub child_started: bool,
    pub cleanup_complete: bool,
}

impl ManagedChild {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub const fn group(&self) -> &OwnedProcessGroup {
        &self.group
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }
}

impl fmt::Debug for ManagedChild {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedChild")
            .field("group", &self.group)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OwnedProcessGroup {
    id: Pid,
    leader_start_identity: u64,
}

impl OwnedProcessGroup {
    pub fn id(self) -> i32 {
        self.id.as_raw_nonzero().get()
    }

    pub fn is_live(self) -> bool {
        self.status(GroupPhase::LeaderExpected)
            .is_ok_and(|status| status == GroupStatus::Live)
    }
}

impl fmt::Debug for OwnedProcessGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OwnedProcessGroup { redacted: true }")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSignal {
    Interrupt,
    Terminate,
    Hangup,
    Kill,
}

impl ManagedSignal {
    pub const fn from_raw(signal: i32) -> Option<Self> {
        match signal {
            SIGINT => Some(Self::Interrupt),
            SIGTERM => Some(Self::Terminate),
            SIGHUP => Some(Self::Hangup),
            _ => None,
        }
    }

    pub const fn number(self) -> i32 {
        match self {
            Self::Interrupt => SIGINT,
            Self::Terminate => SIGTERM,
            Self::Hangup => SIGHUP,
            Self::Kill => signal_hook::consts::signal::SIGKILL,
        }
    }

    const fn native(self) -> Signal {
        match self {
            Self::Interrupt => Signal::INT,
            Self::Terminate => Signal::TERM,
            Self::Hangup => Signal::HUP,
            Self::Kill => Signal::KILL,
        }
    }
}

pub fn reraise_signal(signal: ManagedSignal) -> Result<(), Reason> {
    signal_hook::low_level::emulate_default_handler(signal.number())
        .map_err(|_| Reason::InternalError)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupStatus {
    Live,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPhase {
    LeaderExpected,
    LeaderReaped,
    AfterKill,
}

pub trait GroupController {
    fn status(&self, phase: GroupPhase) -> Result<GroupStatus, Reason>;
    fn send(&self, signal: ManagedSignal, phase: GroupPhase) -> Result<(), Reason>;
}

impl GroupController for OwnedProcessGroup {
    fn status(&self, phase: GroupPhase) -> Result<GroupStatus, Reason> {
        match test_kill_process_group(self.id) {
            Ok(()) => {}
            Err(error) if error == rustix::io::Errno::SRCH => return Ok(GroupStatus::Absent),
            Err(error) if error == rustix::io::Errno::PERM && phase == GroupPhase::AfterKill => {
                return match getpgid(Some(self.id)) {
                    Err(leader_error) if leader_error == rustix::io::Errno::SRCH => {
                        Ok(GroupStatus::Live)
                    }
                    Ok(_) | Err(_) => Err(Reason::InternalError),
                };
            }
            Err(_) => return Err(Reason::InternalError),
        }

        match getpgid(Some(self.id)) {
            Ok(actual) if actual == self.id => match ProcessIdentity::for_pid(self.id()) {
                Ok(identity) if identity.start_identity() == self.leader_start_identity => {
                    Ok(GroupStatus::Live)
                }
                Ok(_) => Err(Reason::InternalError),
                Err(_) => Err(Reason::InternalError),
            },
            Err(error)
                if error == rustix::io::Errno::SRCH && phase != GroupPhase::LeaderExpected =>
            {
                Ok(GroupStatus::Live)
            }
            Ok(_) | Err(_) => Err(Reason::InternalError),
        }
    }

    fn send(&self, signal: ManagedSignal, phase: GroupPhase) -> Result<(), Reason> {
        if self.status(phase)? == GroupStatus::Absent {
            return Ok(());
        }
        match kill_process_group(self.id, signal.native()) {
            Ok(()) => Ok(()),
            Err(error) if error == rustix::io::Errno::SRCH => Ok(()),
            Err(_) => Err(Reason::InternalError),
        }
    }
}

pub fn spawn_managed(
    plan: &RunPlan,
    lease: &mut UserLease,
) -> Result<(ManagedChild, NativeSignalSource), SpawnError> {
    let signals = NativeSignalSource::install().map_err(|reason| SpawnError {
        reason,
        child_started: false,
        cleanup_complete: true,
    })?;
    let launch = &plan.policy().launch;
    let mut command = Command::new(&launch.executable);
    command
        .args(&launch.arguments)
        .current_dir(plan.root())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env("AGENT_LOWMEM_ACTIVE", "1")
        .process_group(0);
    let mut child = command.spawn().map_err(|_| SpawnError {
        reason: Reason::InternalError,
        child_started: false,
        cleanup_complete: true,
    })?;
    let Some(group_id) = Pid::from_raw(child.id() as i32) else {
        let cleanup_complete = cleanup_failed_spawn(None, &mut child);
        return Err(SpawnError {
            reason: Reason::InternalError,
            child_started: true,
            cleanup_complete,
        });
    };
    let leader = match ProcessIdentity::for_pid(child.id() as i32) {
        Ok(identity) => identity,
        Err(reason) => {
            let cleanup_complete = cleanup_failed_spawn(Some(group_id), &mut child);
            return Err(SpawnError {
                reason,
                child_started: true,
                cleanup_complete,
            });
        }
    };
    let group = OwnedProcessGroup {
        id: group_id,
        leader_start_identity: leader.start_identity(),
    };
    if !group.is_live() {
        let cleanup_complete = cleanup_failed_spawn(Some(group_id), &mut child);
        return Err(SpawnError {
            reason: Reason::InternalError,
            child_started: true,
            cleanup_complete,
        });
    }
    if let Err(reason) =
        lease.set_child_group(ChildGroupIdentity::new(group.id(), leader.start_identity()))
    {
        let cleanup_complete = cleanup_failed_spawn(Some(group_id), &mut child);
        return Err(SpawnError {
            reason,
            child_started: true,
            cleanup_complete,
        });
    }
    Ok((ManagedChild { child, group }, signals))
}

fn cleanup_failed_spawn(group_id: Option<Pid>, child: &mut Child) -> bool {
    let signal_ok = match group_id {
        Some(group_id) => match kill_process_group(group_id, Signal::KILL) {
            Ok(()) => true,
            Err(error) if error == rustix::io::Errno::SRCH => true,
            Err(_) => false,
        },
        None => child.kill().is_ok(),
    };
    if !signal_ok {
        return false;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut child_reaped = false;
    loop {
        if !child_reaped {
            match child.try_wait() {
                Ok(Some(_)) => child_reaped = true,
                Ok(None) => {}
                Err(_) => return false,
            }
        }
        let group_absent = group_id.is_none_or(|group_id| {
            test_kill_process_group(group_id).is_err_and(|error| error == rustix::io::Errno::SRCH)
        });
        if child_reaped && group_absent {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub trait SignalSource {
    fn try_recv(&mut self) -> Option<i32>;
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<i32>, Reason>;
    fn shutdown(&mut self) -> Result<(), Reason>;
}

pub struct NativeSignalSource {
    receiver: Receiver<i32>,
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

impl NativeSignalSource {
    pub fn install() -> Result<Self, Reason> {
        let mut signals =
            Signals::new([SIGINT, SIGTERM, SIGHUP]).map_err(|_| Reason::InternalError)?;
        let handle = signals.handle();
        let (sender, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("agent-lowmem-signals".to_owned())
            .spawn(move || {
                for signal in &mut signals {
                    if sender.send(signal).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| Reason::InternalError)?;
        Ok(Self {
            receiver,
            handle,
            thread: Some(thread),
        })
    }
}

impl SignalSource for NativeSignalSource {
    fn try_recv(&mut self) -> Option<i32> {
        self.receiver.try_recv().ok()
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<i32>, Reason> {
        match self.receiver.recv_timeout(timeout) {
            Ok(signal) => Ok(Some(signal)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(Reason::InternalError),
        }
    }

    fn shutdown(&mut self) -> Result<(), Reason> {
        self.handle.close();
        match self.thread.take() {
            Some(thread) => thread.join().map_err(|_| Reason::InternalError),
            None => Ok(()),
        }
    }
}

impl Drop for NativeSignalSource {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for NativeSignalSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NativeSignalSource { active: true }")
    }
}
