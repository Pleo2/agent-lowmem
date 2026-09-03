use crate::{
    process::{
        GroupController, GroupPhase, GroupStatus, ManagedChild, ManagedSignal, SignalSource,
    },
    result::{ExitResult, Origin, Reason},
};
use std::{
    os::unix::process::ExitStatusExt,
    time::{Duration, Instant},
};

const ORDINARY_POLL_INTERVAL: Duration = Duration::from_secs(1);
const GRACE_PERIOD: Duration = Duration::from_secs(10);
const POST_KILL_OBSERVATION: Duration = Duration::from_secs(10);
const CLEANUP_OBSERVATION_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildOutcome {
    Code(i32),
    Signal(i32),
}

pub trait ChildController {
    fn try_wait(&mut self) -> Result<Option<ChildOutcome>, Reason>;
}

impl ChildController for ManagedChild {
    fn try_wait(&mut self) -> Result<Option<ChildOutcome>, Reason> {
        self.try_wait()
            .map_err(|_| Reason::InternalError)?
            .map(outcome_from_status)
            .transpose()
    }
}

fn outcome_from_status(status: std::process::ExitStatus) -> Result<ChildOutcome, Reason> {
    if let Some(code) = status.code() {
        return Ok(ChildOutcome::Code(code));
    }
    status
        .signal()
        .map(ChildOutcome::Signal)
        .ok_or(Reason::InternalError)
}

pub trait Clock {
    fn now(&self) -> Duration;
}

#[derive(Debug)]
pub struct InstantClock {
    started: Instant,
}

impl InstantClock {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Clock for InstantClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

pub trait SupervisionOutput {
    fn timeout_warning(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupAction {
    None,
    Terminate,
    ForwardInterrupt,
    ForwardTerminate,
    ForwardHangup,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisionReport {
    pub result: ExitResult,
    pub external_signal: Option<ManagedSignal>,
    pub warning_emitted: bool,
    pub elapsed_millis: u64,
    pub cleanup_action: CleanupAction,
    pub cleanup_complete: bool,
}

pub fn supervise<C, G, S, K, O>(
    child: &mut C,
    group: &G,
    signals: &mut S,
    clock: &K,
    output: &mut O,
    timeout: Duration,
) -> SupervisionReport
where
    C: ChildController,
    G: GroupController,
    S: SignalSource,
    K: Clock,
    O: SupervisionOutput,
{
    let started = clock.now();
    let warning_deadline = started.saturating_add(timeout * 4 / 5);
    let final_deadline = started.saturating_add(timeout);
    let mut warning_emitted = false;
    let mut delivered_signal = None;

    loop {
        match child.try_wait() {
            Ok(Some(outcome)) => {
                return finish(
                    result_from_child(outcome),
                    None,
                    true,
                    child,
                    group,
                    signals,
                    clock,
                    started,
                    warning_emitted,
                );
            }
            Err(_) => {
                return finish(
                    internal_result(),
                    None,
                    false,
                    child,
                    group,
                    signals,
                    clock,
                    started,
                    warning_emitted,
                );
            }
            Ok(None) => {}
        }

        if let Some(signal) = delivered_signal
            .take()
            .or_else(|| signals.try_recv().and_then(ManagedSignal::from_raw))
        {
            return finish(
                external_result(signal),
                Some(signal),
                false,
                child,
                group,
                signals,
                clock,
                started,
                warning_emitted,
            );
        }

        let now = clock.now();
        if !warning_emitted && now >= warning_deadline {
            output.timeout_warning();
            warning_emitted = true;
        }
        if now >= final_deadline {
            match child.try_wait() {
                Ok(Some(outcome)) => {
                    return finish(
                        result_from_child(outcome),
                        None,
                        true,
                        child,
                        group,
                        signals,
                        clock,
                        started,
                        warning_emitted,
                    );
                }
                Err(_) => {
                    return finish(
                        internal_result(),
                        None,
                        false,
                        child,
                        group,
                        signals,
                        clock,
                        started,
                        warning_emitted,
                    );
                }
                Ok(None) => {
                    return finish(
                        ExitResult::new(Origin::SupervisorTimeout, 124, Reason::DeadlineExceeded),
                        None,
                        false,
                        child,
                        group,
                        signals,
                        clock,
                        started,
                        warning_emitted,
                    );
                }
            }
        }

        let next_check = now.saturating_add(ORDINARY_POLL_INTERVAL);
        let wake_at = if warning_emitted {
            next_check.min(final_deadline)
        } else {
            next_check.min(warning_deadline).min(final_deadline)
        };
        let wait = wake_at.saturating_sub(now);
        match signals.recv_timeout(wait) {
            Ok(Some(signal)) => delivered_signal = ManagedSignal::from_raw(signal),
            Ok(None) => {}
            Err(_) => {
                return finish(
                    internal_result(),
                    None,
                    false,
                    child,
                    group,
                    signals,
                    clock,
                    started,
                    warning_emitted,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finish<C, G, S, K>(
    result: ExitResult,
    external_signal: Option<ManagedSignal>,
    direct_reaped: bool,
    child: &mut C,
    group: &G,
    signals: &mut S,
    clock: &K,
    started: Duration,
    warning_emitted: bool,
) -> SupervisionReport
where
    C: ChildController,
    G: GroupController,
    S: SignalSource,
    K: Clock,
{
    let (cleanup_action, cleanup_complete) =
        cleanup(child, group, signals, clock, direct_reaped, external_signal);
    let elapsed_millis = clock
        .now()
        .saturating_sub(started)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    SupervisionReport {
        result,
        external_signal,
        warning_emitted,
        elapsed_millis,
        cleanup_action,
        cleanup_complete,
    }
}

fn cleanup<C, G, S, K>(
    child: &mut C,
    group: &G,
    signals: &mut S,
    clock: &K,
    mut direct_reaped: bool,
    external_signal: Option<ManagedSignal>,
) -> (CleanupAction, bool)
where
    C: ChildController,
    G: GroupController,
    S: SignalSource,
    K: Clock,
{
    match group.status(group_phase(direct_reaped)) {
        Ok(GroupStatus::Absent) => {
            let complete = direct_reaped || observe_child_reap(child, signals, clock);
            return (CleanupAction::None, complete);
        }
        Err(_) => return (CleanupAction::None, false),
        Ok(GroupStatus::Live) => {}
    }

    let first_signal = external_signal.unwrap_or(ManagedSignal::Terminate);
    let mut action = match external_signal {
        Some(signal) => forwarded_action_for(signal),
        None => CleanupAction::Terminate,
    };
    if group
        .send(first_signal, group_phase(direct_reaped))
        .is_err()
    {
        return (action, false);
    }
    let grace_deadline = clock.now().saturating_add(GRACE_PERIOD);

    loop {
        if !direct_reaped {
            match child.try_wait() {
                Ok(Some(_)) => direct_reaped = true,
                Ok(None) => {}
                Err(_) => return (action, false),
            }
        }
        match group.status(group_phase(direct_reaped)) {
            Ok(GroupStatus::Absent) => {
                let complete = direct_reaped || observe_child_reap(child, signals, clock);
                return (action, complete);
            }
            Err(_) => return (action, false),
            Ok(GroupStatus::Live) => {}
        }

        if signals
            .try_recv()
            .and_then(ManagedSignal::from_raw)
            .is_some()
        {
            if group
                .send(ManagedSignal::Kill, group_phase(direct_reaped))
                .is_err()
            {
                return (CleanupAction::Kill, false);
            }
            action = CleanupAction::Kill;
            break;
        }
        let now = clock.now();
        if now >= grace_deadline {
            if group
                .send(ManagedSignal::Kill, group_phase(direct_reaped))
                .is_err()
            {
                return (CleanupAction::Kill, false);
            }
            action = CleanupAction::Kill;
            break;
        }
        let wait = grace_deadline
            .saturating_sub(now)
            .min(ORDINARY_POLL_INTERVAL);
        match signals.recv_timeout(wait) {
            Ok(Some(signal)) if ManagedSignal::from_raw(signal).is_some() => {
                if group
                    .send(ManagedSignal::Kill, group_phase(direct_reaped))
                    .is_err()
                {
                    return (CleanupAction::Kill, false);
                }
                action = CleanupAction::Kill;
                break;
            }
            Ok(_) => {}
            Err(_) => return (action, false),
        }
    }

    let observation_deadline = clock.now().saturating_add(POST_KILL_OBSERVATION);
    loop {
        if !direct_reaped {
            match child.try_wait() {
                Ok(Some(_)) => direct_reaped = true,
                Ok(None) => {}
                Err(_) => return (action, false),
            }
        }
        match group.status(GroupPhase::AfterKill) {
            Ok(GroupStatus::Absent) if direct_reaped => return (action, true),
            Ok(GroupStatus::Absent | GroupStatus::Live) => {}
            Err(_) => return (action, false),
        }
        let now = clock.now();
        if now >= observation_deadline {
            return (action, false);
        }
        let wait = observation_deadline
            .saturating_sub(now)
            .min(CLEANUP_OBSERVATION_INTERVAL);
        if signals.recv_timeout(wait).is_err() {
            return (action, false);
        }
    }
}

fn observe_child_reap<C, S, K>(child: &mut C, signals: &mut S, clock: &K) -> bool
where
    C: ChildController,
    S: SignalSource,
    K: Clock,
{
    let deadline = clock.now().saturating_add(POST_KILL_OBSERVATION);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Err(_) => return false,
            Ok(None) => {}
        }
        let now = clock.now();
        if now >= deadline {
            return false;
        }
        let wait = deadline
            .saturating_sub(now)
            .min(CLEANUP_OBSERVATION_INTERVAL);
        if signals.recv_timeout(wait).is_err() {
            return false;
        }
    }
}

fn group_phase(direct_reaped: bool) -> GroupPhase {
    if direct_reaped {
        GroupPhase::LeaderReaped
    } else {
        GroupPhase::LeaderExpected
    }
}

fn forwarded_action_for(signal: ManagedSignal) -> CleanupAction {
    match signal {
        ManagedSignal::Interrupt => CleanupAction::ForwardInterrupt,
        ManagedSignal::Terminate => CleanupAction::ForwardTerminate,
        ManagedSignal::Hangup => CleanupAction::ForwardHangup,
        ManagedSignal::Kill => CleanupAction::Kill,
    }
}

fn result_from_child(outcome: ChildOutcome) -> ExitResult {
    match outcome {
        ChildOutcome::Code(0) => ExitResult::new(Origin::Child, 0, Reason::Completed),
        ChildOutcome::Code(code @ 1..=255) => {
            ExitResult::new(Origin::Child, code, Reason::ChildExit)
        }
        ChildOutcome::Signal(signal @ 1..=127) => {
            ExitResult::new(Origin::Child, 128 + signal, Reason::ChildSignal)
        }
        ChildOutcome::Code(_) | ChildOutcome::Signal(_) => internal_result(),
    }
}

fn external_result(signal: ManagedSignal) -> ExitResult {
    ExitResult::new(
        Origin::ExternalSignal,
        128 + signal.number(),
        Reason::ExternalSignal,
    )
}

fn internal_result() -> ExitResult {
    ExitResult::new(Origin::Internal, 70, Reason::InternalError)
}

#[cfg(test)]
mod tests {
    use super::{
        ChildController, ChildOutcome, CleanupAction, Clock, SupervisionOutput, supervise,
    };
    use crate::{
        process::{GroupController, GroupPhase, GroupStatus, ManagedSignal, SignalSource},
        result::{ExitResult, Origin, Reason},
    };
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
        time::Duration,
    };

    #[derive(Clone)]
    struct FakeClock(Rc<Cell<Duration>>);

    impl FakeClock {
        fn new() -> Self {
            Self(Rc::new(Cell::new(Duration::ZERO)))
        }

        fn advance_to(&self, target: Duration) {
            self.0.set(target.max(self.0.get()));
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }

    struct FakeChild {
        clock: FakeClock,
        terminal_at: Duration,
        outcome: ChildOutcome,
        reaped: bool,
    }

    impl FakeChild {
        fn at(clock: &FakeClock, seconds: u64, outcome: ChildOutcome) -> Self {
            Self {
                clock: clock.clone(),
                terminal_at: Duration::from_secs(seconds),
                outcome,
                reaped: false,
            }
        }

        fn never_reaped(clock: &FakeClock) -> Self {
            Self {
                clock: clock.clone(),
                terminal_at: Duration::MAX,
                outcome: ChildOutcome::Code(0),
                reaped: false,
            }
        }
    }

    impl ChildController for FakeChild {
        fn try_wait(&mut self) -> Result<Option<ChildOutcome>, Reason> {
            if self.reaped || self.clock.now() < self.terminal_at {
                return Ok(None);
            }
            self.reaped = true;
            Ok(Some(self.outcome))
        }
    }

    struct FakeSignals {
        clock: FakeClock,
        pending: VecDeque<(Duration, i32)>,
        waits: Rc<RefCell<Vec<Duration>>>,
    }

    impl FakeSignals {
        fn new(clock: &FakeClock, events: &[(u64, i32)]) -> Self {
            Self {
                clock: clock.clone(),
                pending: events
                    .iter()
                    .map(|(second, signal)| (Duration::from_secs(*second), *signal))
                    .collect(),
                waits: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl SignalSource for FakeSignals {
        fn try_recv(&mut self) -> Option<i32> {
            if self
                .pending
                .front()
                .is_some_and(|(at, _)| *at <= self.clock.now())
            {
                return self.pending.pop_front().map(|(_, signal)| signal);
            }
            None
        }

        fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<i32>, Reason> {
            self.waits.borrow_mut().push(timeout);
            let deadline = self.clock.now().saturating_add(timeout);
            if self.pending.front().is_some_and(|(at, _)| *at <= deadline) {
                let (at, signal) = self.pending.pop_front().unwrap();
                self.clock.advance_to(at);
                return Ok(Some(signal));
            }
            self.clock.advance_to(deadline);
            Ok(None)
        }

        fn shutdown(&mut self) -> Result<(), Reason> {
            Ok(())
        }
    }

    struct FakeGroup {
        clock: FakeClock,
        live: Cell<bool>,
        disappear_after_term: Option<Duration>,
        term_at: Cell<Option<Duration>>,
        sent: RefCell<Vec<ManagedSignal>>,
        phases: RefCell<Vec<GroupPhase>>,
        fail_status: bool,
    }

    impl FakeGroup {
        fn absent(clock: &FakeClock) -> Self {
            Self {
                clock: clock.clone(),
                live: Cell::new(false),
                disappear_after_term: None,
                term_at: Cell::new(None),
                sent: RefCell::new(Vec::new()),
                phases: RefCell::new(Vec::new()),
                fail_status: false,
            }
        }

        fn live(clock: &FakeClock, disappear_after_term: Option<Duration>) -> Self {
            Self {
                clock: clock.clone(),
                live: Cell::new(true),
                disappear_after_term,
                term_at: Cell::new(None),
                sent: RefCell::new(Vec::new()),
                phases: RefCell::new(Vec::new()),
                fail_status: false,
            }
        }

        fn failing(clock: &FakeClock) -> Self {
            Self {
                fail_status: true,
                ..Self::absent(clock)
            }
        }
    }

    impl GroupController for FakeGroup {
        fn status(&self, phase: GroupPhase) -> Result<GroupStatus, Reason> {
            self.phases.borrow_mut().push(phase);
            if self.fail_status {
                return Err(Reason::InternalError);
            }
            if let (Some(term_at), Some(delay)) = (self.term_at.get(), self.disappear_after_term) {
                if self.clock.now() >= term_at.saturating_add(delay) {
                    self.live.set(false);
                }
            }
            Ok(if self.live.get() {
                GroupStatus::Live
            } else {
                GroupStatus::Absent
            })
        }

        fn send(&self, signal: ManagedSignal, phase: GroupPhase) -> Result<(), Reason> {
            if self.status(phase)? == GroupStatus::Absent {
                return Ok(());
            }
            self.sent.borrow_mut().push(signal);
            if signal != ManagedSignal::Kill {
                self.term_at.set(Some(self.clock.now()));
            }
            if signal == ManagedSignal::Kill {
                self.live.set(false);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeOutput {
        warnings: usize,
    }

    impl SupervisionOutput for FakeOutput {
        fn timeout_warning(&mut self) {
            self.warnings += 1;
        }
    }

    fn run(
        terminal_at: u64,
        child_outcome: ChildOutcome,
        timeout: u64,
        signal_events: &[(u64, i32)],
        group: impl FnOnce(&FakeClock) -> FakeGroup,
    ) -> (super::SupervisionReport, FakeGroup, FakeSignals, FakeOutput) {
        let clock = FakeClock::new();
        let mut child = FakeChild::at(&clock, terminal_at, child_outcome);
        let group = group(&clock);
        let mut signals = FakeSignals::new(&clock, signal_events);
        let mut output = FakeOutput::default();
        let report = supervise(
            &mut child,
            &group,
            &mut signals,
            &clock,
            &mut output,
            Duration::from_secs(timeout),
        );
        (report, group, signals, output)
    }

    #[test]
    fn maps_normal_and_signaled_child_completion_exactly() {
        let cases = [
            (
                ChildOutcome::Code(0),
                ExitResult::new(Origin::Child, 0, Reason::Completed),
            ),
            (
                ChildOutcome::Code(17),
                ExitResult::new(Origin::Child, 17, Reason::ChildExit),
            ),
            (
                ChildOutcome::Signal(9),
                ExitResult::new(Origin::Child, 137, Reason::ChildSignal),
            ),
        ];

        for (outcome, expected) in cases {
            let (report, _, _, _) = run(2, outcome, 60, &[], FakeGroup::absent);
            assert_eq!(report.result, expected);
            assert_eq!(report.cleanup_action, CleanupAction::None);
            assert!(report.cleanup_complete);
            assert_eq!(report.elapsed_millis, 2_000);
        }
    }

    #[test]
    fn child_status_wins_at_initial_signal_and_final_deadline_boundaries() {
        let (at_signal, _, _, _) = run(
            0,
            ChildOutcome::Code(0),
            60,
            &[(0, signal_hook::consts::SIGINT)],
            FakeGroup::absent,
        );
        assert_eq!(at_signal.result.reason, Reason::Completed);

        let (at_deadline, _, _, _) = run(60, ChildOutcome::Code(0), 60, &[], FakeGroup::absent);
        assert_eq!(at_deadline.result.reason, Reason::Completed);
    }

    #[test]
    fn delivered_signal_interrupts_before_the_next_child_tick() {
        let (report, group, _, _) = run(
            1,
            ChildOutcome::Code(0),
            60,
            &[(0, signal_hook::consts::SIGINT)],
            |clock| FakeGroup::live(clock, Some(Duration::ZERO)),
        );

        assert_eq!(
            report.result,
            ExitResult::new(Origin::ExternalSignal, 130, Reason::ExternalSignal)
        );
        assert_eq!(group.sent.borrow().as_slice(), &[ManagedSignal::Interrupt]);
        assert_eq!(report.cleanup_action, CleanupAction::ForwardInterrupt);
        assert_eq!(report.external_signal, Some(ManagedSignal::Interrupt));
    }

    #[test]
    fn emits_the_eighty_percent_warning_once() {
        let (report, _, _, output) = run(55, ChildOutcome::Code(0), 60, &[], FakeGroup::absent);

        assert!(report.warning_emitted);
        assert_eq!(output.warnings, 1);
    }

    #[test]
    fn timeout_terminates_then_escalates_after_exactly_ten_seconds() {
        let (report, group, _, _) = run(70, ChildOutcome::Signal(9), 60, &[], |clock| {
            FakeGroup::live(clock, None)
        });

        assert_eq!(
            report.result,
            ExitResult::new(Origin::SupervisorTimeout, 124, Reason::DeadlineExceeded)
        );
        assert_eq!(
            group.sent.borrow().as_slice(),
            &[ManagedSignal::Terminate, ManagedSignal::Kill]
        );
        assert_eq!(report.cleanup_action, CleanupAction::Kill);
        assert_eq!(report.elapsed_millis, 70_000);
        assert!(report.cleanup_complete);
        assert_eq!(group.phases.borrow().last(), Some(&GroupPhase::AfterKill));
    }

    #[test]
    fn post_kill_reap_is_bounded_by_the_observation_deadline() {
        let clock = FakeClock::new();
        let mut child = FakeChild::never_reaped(&clock);
        let group = FakeGroup::live(&clock, None);
        let mut signals = FakeSignals::new(&clock, &[]);
        let mut output = FakeOutput::default();
        let report = supervise(
            &mut child,
            &group,
            &mut signals,
            &clock,
            &mut output,
            Duration::from_secs(60),
        );

        assert!(!report.cleanup_complete);
        assert_eq!(report.elapsed_millis, 80_000);
    }

    #[test]
    fn second_external_signal_skips_the_remaining_grace_period() {
        let (report, group, _, _) = run(
            6,
            ChildOutcome::Signal(9),
            60,
            &[
                (5, signal_hook::consts::SIGTERM),
                (6, signal_hook::consts::SIGINT),
            ],
            |clock| FakeGroup::live(clock, None),
        );

        assert_eq!(report.result.origin, Origin::ExternalSignal);
        assert_eq!(
            group.sent.borrow().as_slice(),
            &[ManagedSignal::Terminate, ManagedSignal::Kill]
        );
        assert_eq!(report.elapsed_millis, 6_000);
    }

    #[test]
    fn normal_child_completion_cleans_a_surviving_descendant_group() {
        let (report, group, _, _) = run(2, ChildOutcome::Code(0), 60, &[], |clock| {
            FakeGroup::live(clock, Some(Duration::from_secs(2)))
        });

        assert_eq!(report.result.reason, Reason::Completed);
        assert_eq!(group.sent.borrow().as_slice(), &[ManagedSignal::Terminate]);
        assert_eq!(report.cleanup_action, CleanupAction::Terminate);
        assert_eq!(report.elapsed_millis, 4_000);
        assert!(report.cleanup_complete);
    }

    #[test]
    fn preserves_primary_result_when_cleanup_cannot_prove_group_state() {
        let (report, _, _, _) = run(2, ChildOutcome::Code(23), 60, &[], FakeGroup::failing);

        assert_eq!(report.result.reason, Reason::ChildExit);
        assert!(!report.cleanup_complete);
    }

    #[test]
    fn steady_state_never_waits_longer_than_one_second() {
        let (report, _, signals, _) =
            run(1_800, ChildOutcome::Code(0), 3_600, &[], FakeGroup::absent);

        assert_eq!(report.result.reason, Reason::Completed);
        assert!(signals.waits.borrow().len() <= 1_800);
        assert!(
            signals
                .waits
                .borrow()
                .iter()
                .all(|wait| *wait <= Duration::from_secs(1))
        );
    }
}
