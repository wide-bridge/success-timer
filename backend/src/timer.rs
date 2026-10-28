//! Deterministic countdown state machine.
//!
//! The timer never reads the system clock and never spawns threads. Each call
//! receives `now` from the caller, reconciles the countdown against its target
//! end time and returns a [`Step`]. The caller may observe time as rarely as it
//! likes (e.g. one wake-up at [`Timer::next_deadline`]) because the remaining
//! time is always derived from the end time, never from counting ticks.
//!
//! Clock policy (see README): forward jumps, including sleep/hibernate, count as
//! elapsed time; a backward jump while running shifts the target end time back by
//! the same amount so remaining time never grows.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{TimerError, ValidationError};
use crate::model::{ActiveRun, Minutes, NextAction, SessionOutcome, SessionRecord, TaskName};
use crate::EpochMs;

/// Identifier of one timer run. Never reused while `last_run_id` is persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub u64);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerStatus {
    Idle,
    Running,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    Focus,
    Break,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimerCommand {
    Start,
    Pause,
    Resume,
    FinishEarly,
    Reset,
    NoteReturn,
}

/// Validated parameters for a new run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest {
    pub kind: SessionKind,
    pub task: TaskName,
    pub next_action: Option<NextAction>,
    pub minutes: Minutes,
}

impl StartRequest {
    pub fn focus(task: TaskName, next_action: Option<NextAction>, minutes: Minutes) -> Self {
        Self {
            kind: SessionKind::Focus,
            task,
            next_action,
            minutes,
        }
    }

    /// Builds a focus request from raw UI input. A blank task becomes `무제`.
    pub fn focus_from_input(
        task: Option<&str>,
        next_action: Option<&str>,
        minutes: u32,
    ) -> Result<Self, ValidationError> {
        let next_action = match next_action {
            Some(text) => NextAction::parse(text)?,
            None => None,
        };
        Ok(Self::focus(
            TaskName::from_input(task)?,
            next_action,
            Minutes::new(minutes)?,
        ))
    }

    /// Optional 3-minute break. Never started automatically by this crate.
    pub fn rest(task: TaskName) -> Self {
        Self {
            kind: SessionKind::Break,
            task,
            next_action: None,
            minutes: Minutes::BREAK,
        }
    }
}

/// State sent to the UI. Apply only if `revision` is greater than the last one applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSnapshot {
    pub revision: u64,
    pub run_id: Option<RunId>,
    pub status: TimerStatus,
    pub kind: Option<SessionKind>,
    pub task: Option<TaskName>,
    pub next_action: Option<NextAction>,
    pub duration_ms: u64,
    pub remaining_ms: u64,
    /// Only set while running.
    pub target_end_at: Option<EpochMs>,
    /// Only set when completed.
    pub ended_at: Option<EpochMs>,
    /// Only set when completed.
    pub outcome: Option<SessionOutcome>,
    pub return_count: u32,
    /// The `now` of the latest observation.
    pub updated_at: Option<EpochMs>,
}

/// Result of every successful timer call.
#[must_use = "a Step may carry the only report of a finished run"]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub snapshot: TimerSnapshot,
    /// A run that ended (completed, ended early or reset). Each run is reported exactly once.
    pub ended: Option<SessionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running {
        target_end_at: EpochMs,
    },
    Paused {
        remaining_ms: u64,
    },
    Completed {
        ended_at: EpochMs,
        remaining_ms: u64,
        outcome: SessionOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    id: RunId,
    kind: SessionKind,
    task: TaskName,
    next_action: Option<NextAction>,
    duration_ms: u64,
    started_at: EpochMs,
    return_count: u32,
    phase: Phase,
}

impl Run {
    fn status(&self) -> TimerStatus {
        match self.phase {
            Phase::Running { .. } => TimerStatus::Running,
            Phase::Paused { .. } => TimerStatus::Paused,
            Phase::Completed { .. } => TimerStatus::Completed,
        }
    }

    fn remaining_at(&self, now: EpochMs) -> u64 {
        match self.phase {
            Phase::Running { target_end_at } => {
                remaining_until(now, target_end_at, self.duration_ms)
            }
            Phase::Paused { remaining_ms } | Phase::Completed { remaining_ms, .. } => remaining_ms,
        }
    }

    fn record(&self, ended_at: EpochMs, remaining_ms: u64, outcome: SessionOutcome) -> SessionRecord {
        SessionRecord {
            id: self.id,
            kind: self.kind,
            task: self.task.clone(),
            next_action: self.next_action.clone(),
            started_at: self.started_at,
            ended_at: ended_at.max(self.started_at),
            duration_ms: self.duration_ms,
            timer_run_ms: self.duration_ms.saturating_sub(remaining_ms),
            outcome,
            return_count: self.return_count,
        }
    }
}

/// The timer. Holds at most one run; a new run requires idle or completed state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    revision: u64,
    last_run_id: u64,
    observed_at: Option<EpochMs>,
    run: Option<Run>,
    /// A finished run not yet handed to the caller. Drained by the next successful call.
    pending_end: Option<SessionRecord>,
}

impl Default for Timer {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Timer {
    /// Creates an idle timer. Pass the persisted `AppData::last_run_id` so run ids
    /// stay unique across restarts. A previous run is never resumed automatically.
    pub fn new(last_run_id: u64) -> Self {
        Self {
            revision: 0,
            last_run_id,
            observed_at: None,
            run: None,
            pending_end: None,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn last_run_id(&self) -> u64 {
        self.last_run_id
    }

    pub fn status(&self) -> TimerStatus {
        self.run.as_ref().map_or(TimerStatus::Idle, Run::status)
    }

    pub fn current_run_id(&self) -> Option<RunId> {
        self.run.as_ref().map(|run| run.id)
    }

    /// When the caller should next call [`Timer::tick`]. `None` means nothing will
    /// change on its own, so no wake-up or polling is needed.
    pub fn next_deadline(&self) -> Option<EpochMs> {
        match self.run.as_ref()?.phase {
            Phase::Running { target_end_at } => Some(target_end_at),
            _ => None,
        }
    }

    /// Snapshot as of the latest observation, without observing time.
    pub fn snapshot(&self) -> TimerSnapshot {
        let updated_at = self.observed_at;
        let run = match &self.run {
            Some(run) => run,
            None => {
                return TimerSnapshot {
                    revision: self.revision,
                    run_id: None,
                    status: TimerStatus::Idle,
                    kind: None,
                    task: None,
                    next_action: None,
                    duration_ms: 0,
                    remaining_ms: 0,
                    target_end_at: None,
                    ended_at: None,
                    outcome: None,
                    return_count: 0,
                    updated_at,
                }
            }
        };
        let (target_end_at, ended_at, outcome) = match run.phase {
            Phase::Running { target_end_at } => (Some(target_end_at), None, None),
            Phase::Paused { .. } => (None, None, None),
            Phase::Completed {
                ended_at, outcome, ..
            } => (None, Some(ended_at), Some(outcome)),
        };
        TimerSnapshot {
            revision: self.revision,
            run_id: Some(run.id),
            status: run.status(),
            kind: Some(run.kind),
            task: Some(run.task.clone()),
            next_action: run.next_action.clone(),
            duration_ms: run.duration_ms,
            remaining_ms: run.remaining_at(updated_at.unwrap_or(run.started_at)),
            target_end_at,
            ended_at,
            outcome,
            return_count: run.return_count,
            updated_at,
        }
    }

    /// Recovery marker for the running/paused run, to be persisted. After a restart
    /// it becomes an `Interrupted` record instead of being resumed.
    pub fn active_run(&self) -> Option<ActiveRun> {
        let run = self.run.as_ref()?;
        let saved_at = self.observed_at?;
        let (status, target_end_at) = match run.phase {
            Phase::Running { target_end_at } => (TimerStatus::Running, Some(target_end_at)),
            Phase::Paused { .. } => (TimerStatus::Paused, None),
            Phase::Completed { .. } => return None,
        };
        Some(ActiveRun {
            run_id: run.id,
            kind: run.kind,
            task: run.task.clone(),
            next_action: run.next_action.clone(),
            started_at: run.started_at,
            duration_ms: run.duration_ms,
            status,
            remaining_ms: run.remaining_at(saved_at),
            target_end_at,
            saved_at,
            return_count: run.return_count,
        })
    }

    /// Recomputes the countdown at `now`; completes an expired run (tick, wake from sleep).
    pub fn tick(&mut self, now: EpochMs) -> Step {
        self.observe(now);
        self.step()
    }

    /// Starts a new run from idle or completed.
    pub fn start(&mut self, now: EpochMs, request: StartRequest) -> Result<Step, TimerError> {
        self.observe(now);
        let status = self.status();
        if !matches!(status, TimerStatus::Idle | TimerStatus::Completed) {
            return Err(invalid_transition(TimerCommand::Start, status));
        }
        self.last_run_id = self.last_run_id.saturating_add(1);
        let duration_ms = request.minutes.as_ms();
        self.run = Some(Run {
            id: RunId(self.last_run_id),
            kind: request.kind,
            task: request.task,
            next_action: request.next_action,
            duration_ms,
            started_at: now,
            return_count: 0,
            phase: Phase::Running {
                target_end_at: add_ms(now, duration_ms),
            },
        });
        Ok(self.step())
    }

    pub fn pause(&mut self, now: EpochMs, run_id: RunId) -> Result<Step, TimerError> {
        self.observe(now);
        let run = self.run_for(TimerCommand::Pause, run_id)?;
        let target_end_at = match run.phase {
            Phase::Running { target_end_at } => target_end_at,
            _ => return Err(invalid_transition(TimerCommand::Pause, run.status())),
        };
        run.phase = Phase::Paused {
            remaining_ms: remaining_until(now, target_end_at, run.duration_ms),
        };
        Ok(self.step())
    }

    pub fn resume(&mut self, now: EpochMs, run_id: RunId) -> Result<Step, TimerError> {
        self.observe(now);
        let run = self.run_for(TimerCommand::Resume, run_id)?;
        let remaining_ms = match run.phase {
            Phase::Paused { remaining_ms } => remaining_ms,
            _ => return Err(invalid_transition(TimerCommand::Resume, run.status())),
        };
        run.phase = Phase::Running {
            target_end_at: add_ms(now, remaining_ms),
        };
        Ok(self.step())
    }

    /// Ends a running or paused run now. Recorded as `EndedEarly`, not as completed.
    pub fn finish_early(&mut self, now: EpochMs, run_id: RunId) -> Result<Step, TimerError> {
        self.observe(now);
        let run = self.run_for(TimerCommand::FinishEarly, run_id)?;
        if run.status() == TimerStatus::Completed {
            return Err(invalid_transition(TimerCommand::FinishEarly, run.status()));
        }
        let remaining_ms = run.remaining_at(now);
        let ended_at = now.max(run.started_at);
        run.phase = Phase::Completed {
            ended_at,
            remaining_ms,
            outcome: SessionOutcome::EndedEarly,
        };
        let record = run.record(ended_at, remaining_ms, SessionOutcome::EndedEarly);
        self.set_pending(record);
        Ok(self.step())
    }

    /// Returns to idle and invalidates the run. An unfinished run is reported as `Reset`.
    pub fn reset(&mut self, now: EpochMs, run_id: RunId) -> Result<Step, TimerError> {
        self.observe(now);
        let run = self.run_for(TimerCommand::Reset, run_id)?;
        let ended = match run.phase {
            Phase::Running { .. } | Phase::Paused { .. } => {
                let remaining_ms = run.remaining_at(now);
                Some(run.record(now, remaining_ms, SessionOutcome::Reset))
            }
            Phase::Completed { .. } => None,
        };
        self.run = None;
        if let Some(record) = ended {
            self.set_pending(record);
        }
        Ok(self.step())
    }

    /// Counts a user-initiated "return to task". The countdown is not affected.
    pub fn note_return(&mut self, now: EpochMs, run_id: RunId) -> Result<Step, TimerError> {
        self.observe(now);
        let run = self.run_for(TimerCommand::NoteReturn, run_id)?;
        if run.status() == TimerStatus::Completed {
            return Err(invalid_transition(TimerCommand::NoteReturn, run.status()));
        }
        run.return_count = run.return_count.saturating_add(1);
        Ok(self.step())
    }

    /// Advances the revision, applies the clock policy and completes an expired run.
    /// Runs before every command, so even a rejected command never loses a completion.
    fn observe(&mut self, now: EpochMs) {
        self.revision = self.revision.saturating_add(1);
        let previous = self.observed_at.replace(now);
        let mut expired = None;
        if let Some(run) = self.run.as_mut() {
            if let Phase::Running { target_end_at } = run.phase {
                let mut target = target_end_at;
                if let Some(previous) = previous {
                    if now < previous {
                        // Clock moved backwards: keep the remaining time unchanged.
                        target = target.saturating_sub(previous.saturating_sub(now));
                        run.phase = Phase::Running {
                            target_end_at: target,
                        };
                    }
                }
                if now >= target {
                    let ended_at = target.max(run.started_at);
                    run.phase = Phase::Completed {
                        ended_at,
                        remaining_ms: 0,
                        outcome: SessionOutcome::Completed,
                    };
                    expired = Some(run.record(ended_at, 0, SessionOutcome::Completed));
                }
            }
        }
        if let Some(record) = expired {
            self.set_pending(record);
        }
    }

    fn run_for(&mut self, command: TimerCommand, run_id: RunId) -> Result<&mut Run, TimerError> {
        match self.run.as_mut() {
            Some(run) if run.id == run_id => Ok(run),
            other => Err(TimerError::StaleRun {
                command,
                current: other.map(|run| run.id),
                requested: run_id,
            }),
        }
    }

    fn set_pending(&mut self, record: SessionRecord) {
        // A run can only end once, and starting the next run drains this slot.
        debug_assert!(self.pending_end.is_none(), "unreported run end overwritten");
        self.pending_end = Some(record);
    }

    fn step(&mut self) -> Step {
        Step {
            snapshot: self.snapshot(),
            ended: self.pending_end.take(),
        }
    }
}

fn invalid_transition(command: TimerCommand, status: TimerStatus) -> TimerError {
    TimerError::InvalidTransition { command, status }
}

fn add_ms(at: EpochMs, ms: u64) -> EpochMs {
    at.saturating_add(i64::try_from(ms).unwrap_or(i64::MAX))
}

fn remaining_until(now: EpochMs, target_end_at: EpochMs, duration_ms: u64) -> u64 {
    u64::try_from(target_end_at.saturating_sub(now))
        .unwrap_or(0)
        .min(duration_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;
    const T0: EpochMs = 1_800_000_000_000;

    fn ms(minutes: u64) -> EpochMs {
        (minutes * MINUTE) as EpochMs
    }

    fn focus(minutes: u32) -> StartRequest {
        StartRequest::focus_from_input(Some("첫 슬라이드 제목 쓰기"), Some("제목 후보 3개"), minutes)
            .unwrap()
    }

    fn started(minutes: u32) -> (Timer, RunId) {
        let mut timer = Timer::new(0);
        let step = timer.start(T0, focus(minutes)).unwrap();
        (timer, step.snapshot.run_id.unwrap())
    }

    #[test]
    fn start_uses_injected_time_for_target_end() {
        let mut timer = Timer::new(0);
        assert_eq!(timer.status(), TimerStatus::Idle);
        assert_eq!(timer.next_deadline(), None);

        let step = timer.start(T0, focus(30)).unwrap();
        let snapshot = &step.snapshot;
        assert_eq!(snapshot.status, TimerStatus::Running);
        assert_eq!(snapshot.run_id, Some(RunId(1)));
        assert_eq!(snapshot.kind, Some(SessionKind::Focus));
        assert_eq!(snapshot.duration_ms, 30 * MINUTE);
        assert_eq!(snapshot.remaining_ms, 30 * MINUTE);
        assert_eq!(snapshot.target_end_at, Some(T0 + ms(30)));
        assert_eq!(snapshot.updated_at, Some(T0));
        assert!(step.ended.is_none());
        assert_eq!(timer.next_deadline(), Some(T0 + ms(30)));
    }

    #[test]
    fn run_ids_continue_from_persisted_value() {
        let mut timer = Timer::new(41);
        let step = timer.start(T0, focus(10)).unwrap();
        assert_eq!(step.snapshot.run_id, Some(RunId(42)));
        assert_eq!(timer.last_run_id(), 42);
    }

    #[test]
    fn pause_and_resume_preserve_remaining_time() {
        let (mut timer, run) = started(10);

        let paused = timer.pause(T0 + ms(4), run).unwrap();
        assert_eq!(paused.snapshot.status, TimerStatus::Paused);
        assert_eq!(paused.snapshot.remaining_ms, 6 * MINUTE);
        assert_eq!(paused.snapshot.target_end_at, None);
        assert_eq!(timer.next_deadline(), None);

        // Time passing while paused (including sleep) changes nothing.
        let later = timer.tick(T0 + ms(50));
        assert_eq!(later.snapshot.status, TimerStatus::Paused);
        assert_eq!(later.snapshot.remaining_ms, 6 * MINUTE);

        let resumed = timer.resume(T0 + ms(50), run).unwrap();
        assert_eq!(resumed.snapshot.status, TimerStatus::Running);
        assert_eq!(resumed.snapshot.target_end_at, Some(T0 + ms(56)));

        let done = timer.tick(T0 + ms(56));
        assert_eq!(done.snapshot.status, TimerStatus::Completed);
        let ended = done.ended.expect("completion reported");
        assert_eq!(ended.outcome, SessionOutcome::Completed);
        assert_eq!(ended.timer_run_ms, 10 * MINUTE);
        assert_eq!(ended.ended_at, T0 + ms(56));
    }

    #[test]
    fn invalid_transitions_are_rejected_and_state_is_kept() {
        let mut timer = Timer::new(0);
        assert_eq!(
            timer.pause(T0, RunId(1)),
            Err(TimerError::StaleRun {
                command: TimerCommand::Pause,
                current: None,
                requested: RunId(1),
            })
        );

        let run = timer.start(T0, focus(10)).unwrap().snapshot.run_id.unwrap();
        assert_eq!(
            timer.start(T0 + 1, focus(10)),
            Err(invalid_transition(TimerCommand::Start, TimerStatus::Running))
        );
        assert_eq!(
            timer.resume(T0 + 2, run),
            Err(invalid_transition(TimerCommand::Resume, TimerStatus::Running))
        );

        let _ = timer.pause(T0 + 3, run).unwrap();
        assert_eq!(
            timer.pause(T0 + 4, run),
            Err(invalid_transition(TimerCommand::Pause, TimerStatus::Paused))
        );
        assert_eq!(
            timer.start(T0 + 5, focus(10)),
            Err(invalid_transition(TimerCommand::Start, TimerStatus::Paused))
        );
        assert_eq!(timer.status(), TimerStatus::Paused);
        assert_eq!(timer.current_run_id(), Some(run));
    }

    #[test]
    fn late_and_irregular_ticks_follow_the_end_time() {
        let (mut timer, _) = started(1);
        assert_eq!(timer.tick(T0 + 17_300).snapshot.remaining_ms, MINUTE - 17_300);
        assert_eq!(timer.tick(T0 + 59_999).snapshot.remaining_ms, 1);

        // The tick arrives 1.7s late; the run still ended at its target time.
        let step = timer.tick(T0 + 61_700);
        assert_eq!(step.snapshot.status, TimerStatus::Completed);
        assert_eq!(step.snapshot.remaining_ms, 0);
        assert_eq!(step.snapshot.ended_at, Some(T0 + ms(1)));
        let ended = step.ended.expect("completion reported");
        assert_eq!(ended.ended_at, T0 + ms(1));
        assert_eq!(ended.timer_run_ms, MINUTE);
    }

    #[test]
    fn completion_is_reported_once_per_run() {
        let (mut timer, run) = started(1);
        assert!(timer.tick(T0 + ms(1)).ended.is_some());
        for minute in 2..6 {
            assert!(timer.tick(T0 + ms(minute)).ended.is_none());
        }
        assert!(timer.pause(T0 + ms(7), run).is_err());
        assert!(timer.finish_early(T0 + ms(7), run).is_err());
        assert!(timer.tick(T0 + ms(8)).ended.is_none());
        // Leaving the completed screen does not report the run again.
        assert!(timer.reset(T0 + ms(9), run).unwrap().ended.is_none());
    }

    #[test]
    fn completion_found_by_rejected_command_is_delivered_next() {
        let (mut timer, run) = started(1);
        assert_eq!(
            timer.pause(T0 + ms(2), run),
            Err(invalid_transition(TimerCommand::Pause, TimerStatus::Completed))
        );
        let step = timer.tick(T0 + ms(2));
        assert_eq!(step.ended.map(|r| r.outcome), Some(SessionOutcome::Completed));
        assert!(timer.tick(T0 + ms(3)).ended.is_none());
    }

    #[test]
    fn wake_from_sleep_completes_once_at_original_end_time() {
        let (mut timer, _) = started(30);
        assert!(timer.tick(T0 + ms(5)).ended.is_none());

        // The PC slept for two hours; this is the first observation after wake.
        let step = timer.tick(T0 + ms(125));
        assert_eq!(step.snapshot.status, TimerStatus::Completed);
        let ended = step.ended.expect("completion reported after wake");
        assert_eq!(ended.ended_at, T0 + ms(30));
        assert_eq!(ended.timer_run_ms, 30 * MINUTE);
        assert!(timer.tick(T0 + ms(126)).ended.is_none());
    }

    #[test]
    fn reset_invalidates_the_previous_run() {
        let (mut timer, first) = started(5);
        let reset = timer.reset(T0 + ms(3), first).unwrap();
        assert_eq!(reset.snapshot.status, TimerStatus::Idle);
        assert_eq!(reset.snapshot.run_id, None);
        let ended = reset.ended.expect("reset run reported");
        assert_eq!(ended.outcome, SessionOutcome::Reset);
        assert_eq!(ended.timer_run_ms, 3 * MINUTE);
        assert!(!ended.outcome.counts_as_completed());

        let second = timer.start(T0 + ms(4), focus(10)).unwrap().snapshot.run_id.unwrap();
        assert_ne!(first, second);

        // Late commands aimed at the old run must not touch the new one.
        for result in [
            timer.pause(T0 + ms(5), first),
            timer.finish_early(T0 + ms(5), first),
            timer.reset(T0 + ms(5), first),
        ] {
            assert!(matches!(
                result,
                Err(TimerError::StaleRun { current: Some(current), requested, .. })
                    if current == second && requested == first
            ));
        }

        // The old run's end time (T0+5) passes without completing anything.
        let step = timer.tick(T0 + ms(6));
        assert_eq!(step.snapshot.status, TimerStatus::Running);
        assert_eq!(step.snapshot.run_id, Some(second));
        assert_eq!(step.snapshot.remaining_ms, 8 * MINUTE);
        assert!(step.ended.is_none());
    }

    #[test]
    fn finish_early_records_run_time_but_not_completion() {
        let (mut timer, run) = started(45);
        let _ = timer.pause(T0 + ms(10), run).unwrap();
        let _ = timer.resume(T0 + ms(20), run).unwrap();
        let step = timer.finish_early(T0 + ms(25), run).unwrap();

        assert_eq!(step.snapshot.status, TimerStatus::Completed);
        assert_eq!(step.snapshot.outcome, Some(SessionOutcome::EndedEarly));
        let ended = step.ended.expect("early end reported");
        assert_eq!(ended.outcome, SessionOutcome::EndedEarly);
        assert_eq!(ended.timer_run_ms, 15 * MINUTE);
        assert_eq!(ended.started_at, T0);
        assert_eq!(ended.ended_at, T0 + ms(25));
        assert!(!ended.outcome.counts_as_completed());
        assert!(timer.tick(T0 + ms(90)).ended.is_none());
    }

    #[test]
    fn completed_timer_can_start_a_new_run() {
        let (mut timer, first) = started(1);
        let _ = timer.tick(T0 + ms(1));
        let step = timer.start(T0 + ms(2), focus(5)).unwrap();
        assert_eq!(step.snapshot.status, TimerStatus::Running);
        assert_ne!(step.snapshot.run_id, Some(first));
        assert!(step.ended.is_none());
    }

    #[test]
    fn clock_moved_backwards_does_not_add_time() {
        let (mut timer, _) = started(30);
        assert_eq!(timer.tick(T0 + ms(10)).snapshot.remaining_ms, 20 * MINUTE);

        // The user sets the clock back by an hour.
        let back = timer.tick(T0 - ms(50));
        assert_eq!(back.snapshot.status, TimerStatus::Running);
        assert_eq!(back.snapshot.remaining_ms, 20 * MINUTE);
        assert_eq!(back.snapshot.target_end_at, Some(T0 - ms(30)));

        let ended = timer.tick(T0 - ms(30)).ended.expect("completes on shifted target");
        assert_eq!(ended.timer_run_ms, 30 * MINUTE);
        assert!(ended.ended_at >= ended.started_at);
    }

    #[test]
    fn revision_increases_on_every_call_including_rejections() {
        let mut timer = Timer::new(0);
        let mut last = timer.revision();
        let run = timer.start(T0, focus(10)).unwrap().snapshot.run_id.unwrap();
        assert!(timer.revision() > last);
        last = timer.revision();

        assert!(timer.resume(T0 + 1, run).is_err());
        assert!(timer.revision() > last);
        last = timer.revision();

        let step = timer.tick(T0 + 2);
        assert!(step.snapshot.revision > last);
        assert_eq!(step.snapshot.revision, timer.revision());
    }

    #[test]
    fn returns_are_counted_without_touching_the_countdown() {
        let (mut timer, run) = started(10);
        let step = timer.note_return(T0 + ms(2), run).unwrap();
        assert_eq!(step.snapshot.return_count, 1);
        assert_eq!(step.snapshot.status, TimerStatus::Running);
        assert_eq!(step.snapshot.target_end_at, Some(T0 + ms(10)));

        let ended = timer.tick(T0 + ms(10)).ended.unwrap();
        assert_eq!(ended.return_count, 1);
        assert_eq!(
            timer.note_return(T0 + ms(11), run),
            Err(invalid_transition(TimerCommand::NoteReturn, TimerStatus::Completed))
        );
    }

    #[test]
    fn active_run_marker_recovers_as_interrupted_not_resumed() {
        let (mut timer, run) = started(30);
        let _ = timer.note_return(T0 + ms(4), run).unwrap();
        let _ = timer.tick(T0 + ms(12));

        let marker = timer.active_run().expect("running run has a marker");
        assert_eq!(marker.status, TimerStatus::Running);
        assert_eq!(marker.remaining_ms, 18 * MINUTE);
        assert_eq!(marker.target_end_at, Some(T0 + ms(30)));
        assert_eq!(marker.saved_at, T0 + ms(12));

        let record = marker.into_interrupted_record();
        assert_eq!(record.outcome, SessionOutcome::Interrupted);
        assert_eq!(record.timer_run_ms, 12 * MINUTE);
        assert_eq!(record.ended_at, T0 + ms(12));
        assert_eq!(record.return_count, 1);
        assert!(!record.outcome.counts_as_completed());

        // A restarted process begins idle; nothing resumes on its own.
        let restarted = Timer::new(timer.last_run_id());
        assert_eq!(restarted.status(), TimerStatus::Idle);
        assert_eq!(restarted.next_deadline(), None);
    }

    #[test]
    fn completed_and_idle_timers_have_no_active_marker() {
        let (mut timer, _) = started(1);
        let _ = timer.tick(T0 + ms(1));
        assert!(timer.active_run().is_none());
        assert!(Timer::new(0).active_run().is_none());
    }

    #[test]
    fn break_runs_last_three_minutes() {
        let mut timer = Timer::new(0);
        let step = timer.start(T0, StartRequest::rest(TaskName::untitled())).unwrap();
        assert_eq!(step.snapshot.kind, Some(SessionKind::Break));
        assert_eq!(step.snapshot.duration_ms, 3 * MINUTE);
    }

    #[test]
    fn snapshot_serializes_with_camel_case_fields() {
        let (timer, _) = started(10);
        let json = serde_json::to_value(timer.snapshot()).unwrap();
        assert_eq!(json["status"], "running");
        assert_eq!(json["runId"], 1);
        assert_eq!(json["targetEndAt"], T0 + ms(10));
        assert_eq!(json["task"], "첫 슬라이드 제목 쓰기");
    }
}
