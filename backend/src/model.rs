//! Versioned, validated data models shared by the timer and storage.
//!
//! Naming note: `timer_run_ms` is how long the countdown was running (pauses
//! excluded). It is the timer's run time, not a measurement of real focus.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{TextField, ValidationError};
use crate::timer::{RunId, SessionKind, Step, Timer, TimerStatus};
use crate::EpochMs;

/// Current on-disk schema version of [`AppData`].
pub const SCHEMA_VERSION: u32 = 1;
pub const MIN_MINUTES: u32 = 1;
pub const MAX_MINUTES: u32 = 120;
pub const DEFAULT_MINUTES: u32 = 30;
pub const PRESET_MINUTES: [u32; 3] = [10, 30, 45];
/// "남은 세션을 끝내고 5분 새로 시작" — separate from the presets.
pub const QUICK_RESTART_MINUTES: u32 = 5;
pub const BREAK_MINUTES: u32 = 3;
/// Name used when the task is left blank.
pub const UNTITLED_TASK: &str = "무제";
pub const MAX_TASK_CHARS: usize = 200;
pub const MAX_NEXT_ACTION_CHARS: usize = 200;
pub const MAX_DISTRACTION_CHARS: usize = 500;
pub const MAX_SESSIONS: usize = 100_000;
pub const MAX_DISTRACTIONS: usize = 10_000;

const MS_PER_MINUTE: u64 = 60_000;

fn invalid(detail: impl Into<String>) -> ValidationError {
    ValidationError::InvalidData {
        detail: detail.into(),
    }
}

/// Trims input; blank becomes `None`. Rejects control characters and overlong text.
fn normalize_line(
    input: &str,
    field: TextField,
    max_chars: usize,
) -> Result<Option<String>, ValidationError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ValidationError::TextControlCharacter { field });
    }
    let actual_chars = trimmed.chars().count();
    if actual_chars > max_chars {
        return Err(ValidationError::TextTooLong {
            field,
            max_chars,
            actual_chars,
        });
    }
    Ok(Some(trimmed.to_owned()))
}

/// Stored text must already be in normalized, non-empty form.
fn from_stored(value: String, field: TextField, max_chars: usize) -> Result<String, ValidationError> {
    match normalize_line(&value, field, max_chars)? {
        Some(normalized) if normalized == value => Ok(value),
        _ => Err(ValidationError::TextNotNormalized { field }),
    }
}

/// Timer length in whole minutes, 1..=120.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Minutes(u32);

impl Minutes {
    pub const DEFAULT: Minutes = Minutes(DEFAULT_MINUTES);
    pub const PRESETS: [Minutes; 3] = [
        Minutes(PRESET_MINUTES[0]),
        Minutes(PRESET_MINUTES[1]),
        Minutes(PRESET_MINUTES[2]),
    ];
    pub const QUICK_RESTART: Minutes = Minutes(QUICK_RESTART_MINUTES);
    pub const BREAK: Minutes = Minutes(BREAK_MINUTES);

    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if (MIN_MINUTES..=MAX_MINUTES).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ValidationError::MinutesOutOfRange {
                value: i64::from(value),
            })
        }
    }

    /// Parses the custom-duration text field ("45", " 45 "). Fractions are rejected.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let value: i64 = input
            .trim()
            .parse()
            .map_err(|_| ValidationError::MinutesNotInteger)?;
        let value = u32::try_from(value).map_err(|_| ValidationError::MinutesOutOfRange { value })?;
        Self::new(value)
    }

    /// Accepts only durations that are a whole, in-range number of minutes.
    pub fn from_ms(ms: u64) -> Result<Self, ValidationError> {
        if ms % MS_PER_MINUTE != 0 {
            return Err(invalid("duration is not a whole number of minutes"));
        }
        let minutes = u32::try_from(ms / MS_PER_MINUTE)
            .map_err(|_| invalid("duration is out of range"))?;
        Self::new(minutes)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn as_ms(self) -> u64 {
        u64::from(self.0) * MS_PER_MINUTE
    }
}

impl Default for Minutes {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<u32> for Minutes {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Minutes> for u32 {
    fn from(value: Minutes) -> Self {
        value.0
    }
}

/// Task name: trimmed, single line, at most 200 characters. Blank input becomes `무제`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TaskName(String);

impl TaskName {
    /// Input from the optional task field. `None`, empty or whitespace gives `무제`.
    pub fn from_input(input: Option<&str>) -> Result<Self, ValidationError> {
        Ok(Self::parse_optional(input.unwrap_or(""))?.unwrap_or_else(Self::untitled))
    }

    /// Like [`TaskName::from_input`] but keeps "blank" distinguishable (for drafts).
    pub fn parse_optional(input: &str) -> Result<Option<Self>, ValidationError> {
        Ok(normalize_line(input, TextField::Task, MAX_TASK_CHARS)?.map(Self))
    }

    pub fn untitled() -> Self {
        Self(UNTITLED_TASK.to_owned())
    }

    pub fn is_untitled(&self) -> bool {
        self.0 == UNTITLED_TASK
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TaskName {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        from_stored(value, TextField::Task, MAX_TASK_CHARS).map(Self)
    }
}

impl From<TaskName> for String {
    fn from(value: TaskName) -> Self {
        value.0
    }
}

impl fmt::Display for TaskName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Optional "first thing to do when coming back". Blank means none.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NextAction(String);

impl NextAction {
    pub fn parse(input: &str) -> Result<Option<Self>, ValidationError> {
        Ok(normalize_line(input, TextField::NextAction, MAX_NEXT_ACTION_CHARS)?.map(Self))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NextAction {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        from_stored(value, TextField::NextAction, MAX_NEXT_ACTION_CHARS).map(Self)
    }
}

impl From<NextAction> for String {
    fn from(value: NextAction) -> Self {
        value.0
    }
}

/// One-line note captured with "딴생각 맡기기". Blank means the user skipped it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DistractionText(String);

impl DistractionText {
    pub fn parse(input: &str) -> Result<Option<Self>, ValidationError> {
        Ok(normalize_line(input, TextField::Distraction, MAX_DISTRACTION_CHARS)?.map(Self))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DistractionText {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        from_stored(value, TextField::Distraction, MAX_DISTRACTION_CHARS).map(Self)
    }
}

impl From<DistractionText> for String {
    fn from(value: DistractionText) -> Self {
        value.0
    }
}

/// User settings. The notification/sound defaults are provisional until the
/// product owner confirms them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_minutes: Minutes,
    pub notify_on_complete: bool,
    pub sound_on_complete: bool,
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_minutes: Minutes::DEFAULT,
            notify_on_complete: true,
            sound_on_complete: true,
            always_on_top: false,
        }
    }
}

/// Unstarted input on the start screen, kept so a restart does not lose it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDraft {
    /// `None` when the field is blank; starting then uses `무제`.
    pub task: Option<TaskName>,
    pub next_action: Option<NextAction>,
    pub minutes: Minutes,
}

impl TaskDraft {
    pub fn from_input(task: &str, next_action: &str, minutes: Minutes) -> Result<Self, ValidationError> {
        Ok(Self {
            task: TaskName::parse_optional(task)?,
            next_action: NextAction::parse(next_action)?,
            minutes,
        })
    }

    pub fn task_for_start(&self) -> TaskName {
        self.task.clone().unwrap_or_else(TaskName::untitled)
    }
}

impl Default for TaskDraft {
    fn default() -> Self {
        Self {
            task: None,
            next_action: None,
            minutes: Minutes::DEFAULT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Distraction {
    pub id: u64,
    pub session_id: Option<RunId>,
    pub text: DistractionText,
    pub captured_at: EpochMs,
    pub resolved_at: Option<EpochMs>,
}

/// How a run ended. Only `Completed` counts as a completed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionOutcome {
    /// The countdown reached zero.
    Completed,
    /// The user ended the run before zero (e.g. before a 5-minute restart).
    EndedEarly,
    /// The user reset a running or paused run.
    Reset,
    /// The app stopped while the run was active; recovered on next launch.
    Interrupted,
}

impl SessionOutcome {
    pub fn counts_as_completed(self) -> bool {
        matches!(self, SessionOutcome::Completed)
    }
}

/// Persisted record of one ended run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: RunId,
    pub kind: SessionKind,
    pub task: TaskName,
    pub next_action: Option<NextAction>,
    pub started_at: EpochMs,
    pub ended_at: EpochMs,
    pub duration_ms: u64,
    /// Countdown run time with pauses excluded. Not "focus time".
    pub timer_run_ms: u64,
    pub outcome: SessionOutcome,
    /// Times the user pressed the return button. Not a score.
    pub return_count: u32,
}

impl SessionRecord {
    pub fn validate(&self) -> Result<(), ValidationError> {
        Minutes::from_ms(self.duration_ms)?;
        if self.ended_at < self.started_at {
            return Err(invalid("endedAt is before startedAt"));
        }
        if self.timer_run_ms > self.duration_ms {
            return Err(invalid("timerRunMs exceeds durationMs"));
        }
        if self.outcome == SessionOutcome::Completed && self.timer_run_ms != self.duration_ms {
            return Err(invalid("a completed session must have run its full duration"));
        }
        Ok(())
    }
}

/// Recovery marker for a run that was active at the last save.
///
/// Contract: on the next launch the app must not resume it. It is converted with
/// [`ActiveRun::into_interrupted_record`] (see [`AppData::recover_interrupted_run`])
/// and the UI may offer to start again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub run_id: RunId,
    pub kind: SessionKind,
    pub task: TaskName,
    pub next_action: Option<NextAction>,
    pub started_at: EpochMs,
    pub duration_ms: u64,
    /// `running` or `paused`.
    pub status: TimerStatus,
    /// Remaining time as of `saved_at`.
    pub remaining_ms: u64,
    /// Present only when `status` is `running`.
    pub target_end_at: Option<EpochMs>,
    /// Last moment the app is known to have been alive with this run.
    pub saved_at: EpochMs,
    pub return_count: u32,
}

impl ActiveRun {
    pub fn validate(&self) -> Result<(), ValidationError> {
        Minutes::from_ms(self.duration_ms)?;
        if self.remaining_ms > self.duration_ms {
            return Err(invalid("activeRun.remainingMs exceeds durationMs"));
        }
        match (self.status, self.target_end_at) {
            (TimerStatus::Running, Some(_)) | (TimerStatus::Paused, None) => Ok(()),
            (TimerStatus::Running, None) => Err(invalid("running activeRun needs targetEndAt")),
            (TimerStatus::Paused, Some(_)) => Err(invalid("paused activeRun has targetEndAt")),
            _ => Err(invalid("activeRun status must be running or paused")),
        }
    }

    /// Turns the marker into an `Interrupted` record. Run time counts only up to
    /// `saved_at`; the run is never reported as completed, even if its end time passed.
    pub fn into_interrupted_record(self) -> SessionRecord {
        SessionRecord {
            id: self.run_id,
            kind: self.kind,
            task: self.task,
            next_action: self.next_action,
            started_at: self.started_at,
            ended_at: self.saved_at.max(self.started_at),
            duration_ms: self.duration_ms,
            timer_run_ms: self.duration_ms.saturating_sub(self.remaining_ms),
            outcome: SessionOutcome::Interrupted,
            return_count: self.return_count,
        }
    }
}

/// Counts for the "오늘 기록" view. No scores or targets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailySummary {
    pub completed_sessions: u32,
    pub timer_run_ms: u64,
    pub return_count: u32,
}

/// Everything persisted in the local data file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppData {
    pub schema_version: u32,
    pub settings: Settings,
    pub draft: TaskDraft,
    pub distractions: Vec<Distraction>,
    pub sessions: Vec<SessionRecord>,
    pub active_run: Option<ActiveRun>,
    /// Highest run id ever issued; feeds [`Timer::new`].
    pub last_run_id: u64,
    pub last_distraction_id: u64,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            settings: Settings::default(),
            draft: TaskDraft::default(),
            distractions: Vec::new(),
            sessions: Vec::new(),
            active_run: None,
            last_run_id: 0,
            last_distraction_id: 0,
        }
    }
}

impl AppData {
    /// Cross-record checks. Field-level checks already happen during deserialization.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid(format!(
                "schemaVersion {} is not {SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.sessions.len() > MAX_SESSIONS {
            return Err(invalid(format!("more than {MAX_SESSIONS} sessions")));
        }
        if self.distractions.len() > MAX_DISTRACTIONS {
            return Err(invalid(format!("more than {MAX_DISTRACTIONS} distractions")));
        }

        let mut session_ids = HashSet::with_capacity(self.sessions.len());
        for (index, session) in self.sessions.iter().enumerate() {
            session
                .validate()
                .map_err(|err| invalid(format!("sessions[{index}]: {err}")))?;
            if session.id.0 == 0 || session.id.0 > self.last_run_id {
                return Err(invalid(format!("sessions[{index}]: id is outside issued range")));
            }
            if !session_ids.insert(session.id) {
                return Err(invalid(format!("sessions[{index}]: duplicate id")));
            }
        }

        let mut distraction_ids = HashSet::with_capacity(self.distractions.len());
        for (index, distraction) in self.distractions.iter().enumerate() {
            if distraction.id == 0 || distraction.id > self.last_distraction_id {
                return Err(invalid(format!(
                    "distractions[{index}]: id is outside issued range"
                )));
            }
            if !distraction_ids.insert(distraction.id) {
                return Err(invalid(format!("distractions[{index}]: duplicate id")));
            }
        }

        if let Some(active) = &self.active_run {
            active
                .validate()
                .map_err(|err| invalid(format!("activeRun: {err}")))?;
            if active.run_id.0 == 0 || active.run_id.0 > self.last_run_id {
                return Err(invalid("activeRun: id is outside issued range"));
            }
            if session_ids.contains(&active.run_id) {
                return Err(invalid("activeRun: run already has a session record"));
            }
        }
        Ok(())
    }

    /// Stores a distraction note. Blank text means "skipped" and returns `Ok(None)`.
    pub fn capture_distraction(
        &mut self,
        session_id: Option<RunId>,
        text: &str,
        now: EpochMs,
    ) -> Result<Option<u64>, ValidationError> {
        let text = match DistractionText::parse(text)? {
            Some(text) => text,
            None => return Ok(None),
        };
        if self.distractions.len() >= MAX_DISTRACTIONS {
            return Err(invalid(format!("more than {MAX_DISTRACTIONS} distractions")));
        }
        let id = self
            .last_distraction_id
            .checked_add(1)
            .ok_or_else(|| invalid("distraction ids exhausted"))?;
        self.last_distraction_id = id;
        self.distractions.push(Distraction {
            id,
            session_id,
            text,
            captured_at: now,
            resolved_at: None,
        });
        Ok(Some(id))
    }

    /// Marks a distraction as done. Returns `false` if it does not exist.
    pub fn resolve_distraction(&mut self, id: u64, now: EpochMs) -> bool {
        match self.distractions.iter_mut().find(|d| d.id == id) {
            Some(distraction) => {
                if distraction.resolved_at.is_none() {
                    distraction.resolved_at = Some(now);
                }
                true
            }
            None => false,
        }
    }

    pub fn delete_distraction(&mut self, id: u64) -> bool {
        let before = self.distractions.len();
        self.distractions.retain(|d| d.id != id);
        self.distractions.len() != before
    }

    /// Adds an ended run. Idempotent: a record whose id is already stored is ignored
    /// and `Ok(false)` is returned, so a re-delivered completion is never double counted.
    pub fn record_session(&mut self, record: SessionRecord) -> Result<bool, ValidationError> {
        record.validate()?;
        if record.id.0 == 0 {
            return Err(invalid("session id must be positive"));
        }
        if self.sessions.iter().any(|s| s.id == record.id) {
            return Ok(false);
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(invalid(format!("more than {MAX_SESSIONS} sessions")));
        }
        self.last_run_id = self.last_run_id.max(record.id.0);
        if self.active_run.as_ref().map(|a| a.run_id) == Some(record.id) {
            self.active_run = None;
        }
        self.sessions.push(record);
        Ok(true)
    }

    /// Applies a timer step: records an ended run and refreshes the recovery marker.
    /// Safe to call more than once with the same step.
    pub fn apply_timer_step(&mut self, timer: &Timer, step: &Step) -> Result<bool, ValidationError> {
        let recorded = match &step.ended {
            Some(record) => self.record_session(record.clone())?,
            None => false,
        };
        self.last_run_id = self.last_run_id.max(timer.last_run_id());
        self.active_run = timer.active_run();
        Ok(recorded)
    }

    /// Call once at startup, before creating the [`Timer`]. Converts a leftover
    /// active run into an `Interrupted` record and returns it so the UI can tell the
    /// user and offer a restart. Nothing is resumed.
    pub fn recover_interrupted_run(&mut self) -> Result<Option<SessionRecord>, ValidationError> {
        let active = match self.active_run.take() {
            Some(active) => active,
            None => return Ok(None),
        };
        let record = active.into_interrupted_record();
        if self.record_session(record.clone())? {
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// "전체 기록 삭제": removes sessions and distractions; settings, the draft and
    /// id counters stay so ids are never reused.
    pub fn clear_history(&mut self) {
        self.sessions.clear();
        self.distractions.clear();
    }

    /// Focus sessions that ended in `[from, until)`. The caller supplies local-day bounds.
    pub fn summary_between(&self, from: EpochMs, until: EpochMs) -> DailySummary {
        let mut summary = DailySummary::default();
        for session in self
            .sessions
            .iter()
            .filter(|s| s.kind == SessionKind::Focus && s.ended_at >= from && s.ended_at < until)
        {
            if session.outcome.counts_as_completed() {
                summary.completed_sessions = summary.completed_sessions.saturating_add(1);
            }
            summary.timer_run_ms = summary.timer_run_ms.saturating_add(session.timer_run_ms);
            summary.return_count = summary.return_count.saturating_add(session.return_count);
        }
        summary
    }

    /// Distinct named tasks, newest first, for "최근 작업 다시 선택".
    pub fn recent_tasks(&self, limit: usize) -> Vec<TaskName> {
        let mut seen = HashSet::new();
        let mut tasks = Vec::new();
        for session in self.sessions.iter().rev() {
            if tasks.len() >= limit {
                break;
            }
            if session.kind == SessionKind::Focus
                && !session.task.is_untitled()
                && seen.insert(session.task.clone())
            {
                tasks.push(session.task.clone());
            }
        }
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::StartRequest;

    const MINUTE: u64 = 60_000;
    const T0: EpochMs = 1_800_000_000_000;

    fn task(name: &str) -> TaskName {
        TaskName::from_input(Some(name)).unwrap()
    }

    fn record(id: u64, outcome: SessionOutcome, run_minutes: u64) -> SessionRecord {
        SessionRecord {
            id: RunId(id),
            kind: SessionKind::Focus,
            task: task("보고서 첫 문단"),
            next_action: None,
            started_at: T0,
            ended_at: T0 + (run_minutes * MINUTE) as EpochMs,
            duration_ms: 30 * MINUTE,
            timer_run_ms: run_minutes * MINUTE,
            outcome,
            return_count: 0,
        }
    }

    #[test]
    fn blank_task_becomes_untitled() {
        for input in [None, Some(""), Some("   "), Some("\u{3000}\t \n")] {
            let name = TaskName::from_input(input).unwrap();
            assert_eq!(name.as_str(), "무제");
            assert!(name.is_untitled());
        }
    }

    #[test]
    fn task_is_trimmed() {
        assert_eq!(task("  첫 슬라이드 제목 쓰기 \u{3000}").as_str(), "첫 슬라이드 제목 쓰기");
    }

    #[test]
    fn task_length_and_control_characters_are_limited() {
        assert!(TaskName::from_input(Some(&"가".repeat(MAX_TASK_CHARS))).is_ok());
        assert_eq!(
            TaskName::from_input(Some(&"가".repeat(MAX_TASK_CHARS + 1))),
            Err(ValidationError::TextTooLong {
                field: TextField::Task,
                max_chars: MAX_TASK_CHARS,
                actual_chars: MAX_TASK_CHARS + 1,
            })
        );
        assert_eq!(
            TaskName::from_input(Some("첫 줄\n둘째 줄")),
            Err(ValidationError::TextControlCharacter {
                field: TextField::Task
            })
        );
    }

    #[test]
    fn minutes_accept_only_whole_minutes_in_range() {
        assert_eq!(Minutes::default().get(), 30);
        assert_eq!(Minutes::PRESETS.map(Minutes::get), [10, 30, 45]);
        assert_eq!(Minutes::new(1).unwrap().get(), 1);
        assert_eq!(Minutes::new(120).unwrap().get(), 120);
        assert_eq!(Minutes::new(0), Err(ValidationError::MinutesOutOfRange { value: 0 }));
        assert_eq!(Minutes::new(121), Err(ValidationError::MinutesOutOfRange { value: 121 }));

        assert_eq!(Minutes::parse(" 45 ").unwrap().get(), 45);
        assert_eq!(Minutes::parse("-3"), Err(ValidationError::MinutesOutOfRange { value: -3 }));
        for bad in ["", "abc", "1.5", "30분"] {
            assert_eq!(Minutes::parse(bad), Err(ValidationError::MinutesNotInteger), "{bad:?}");
        }
        assert_eq!(Minutes::new(30).unwrap().as_ms(), 30 * MINUTE);
        assert!(Minutes::from_ms(30 * MINUTE + 1).is_err());
    }

    #[test]
    fn deserialization_rejects_unnormalized_or_out_of_range_values() {
        assert!(serde_json::from_str::<TaskName>("\"  padded  \"").is_err());
        assert!(serde_json::from_str::<TaskName>("\"\"").is_err());
        assert!(serde_json::from_str::<TaskName>("\"무제\"").is_ok());
        assert!(serde_json::from_str::<Minutes>("121").is_err());
        assert!(serde_json::from_str::<Minutes>("0").is_err());
        assert_eq!(serde_json::from_str::<Minutes>("45").unwrap().get(), 45);
    }

    #[test]
    fn draft_keeps_blank_task_until_start() {
        let draft = TaskDraft::from_input("  ", " 메일 답장 ", Minutes::DEFAULT).unwrap();
        assert_eq!(draft.task, None);
        assert_eq!(draft.next_action.as_ref().map(NextAction::as_str), Some("메일 답장"));
        assert!(draft.task_for_start().is_untitled());
    }

    #[test]
    fn skipped_distraction_is_not_stored() {
        let mut data = AppData::default();
        assert_eq!(data.capture_distraction(Some(RunId(1)), "   ", T0), Ok(None));
        assert!(data.distractions.is_empty());
        assert_eq!(data.last_distraction_id, 0);
    }

    #[test]
    fn distractions_get_unique_ids_and_can_be_resolved_or_deleted() {
        let mut data = AppData::default();
        let first = data.capture_distraction(None, " 택배 확인 ", T0).unwrap().unwrap();
        let second = data.capture_distraction(None, "엄마 전화", T0 + 1).unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(data.distractions[0].text.as_str(), "택배 확인");

        assert!(data.resolve_distraction(first, T0 + 5));
        assert_eq!(data.distractions[0].resolved_at, Some(T0 + 5));
        assert!(data.delete_distraction(second));
        assert!(!data.delete_distraction(second));
        assert!(!data.resolve_distraction(999, T0));

        // Ids are not reused after deletion.
        let third = data.capture_distraction(None, "물 마시기", T0 + 9).unwrap().unwrap();
        assert!(third > second);
        assert!(data.validate().is_ok());
    }

    #[test]
    fn recording_the_same_session_twice_is_ignored() {
        let mut data = AppData::default();
        assert_eq!(data.record_session(record(1, SessionOutcome::Completed, 30)), Ok(true));
        assert_eq!(data.record_session(record(1, SessionOutcome::Completed, 30)), Ok(false));
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.last_run_id, 1);
        assert!(data.validate().is_ok());
    }

    #[test]
    fn inconsistent_session_records_are_rejected() {
        let mut data = AppData::default();
        let mut too_long = record(1, SessionOutcome::EndedEarly, 10);
        too_long.timer_run_ms = 31 * MINUTE;
        assert!(data.record_session(too_long).is_err());

        let partial_completion = record(2, SessionOutcome::Completed, 10);
        assert!(data.record_session(partial_completion).is_err());

        let mut backwards = record(3, SessionOutcome::Reset, 5);
        backwards.ended_at = T0 - 1;
        assert!(data.record_session(backwards).is_err());
        assert!(data.sessions.is_empty());
    }

    #[test]
    fn validate_detects_structural_problems() {
        let mut data = AppData::default();
        data.sessions.push(record(1, SessionOutcome::Completed, 30));
        assert!(data.validate().is_err(), "id beyond lastRunId");

        data.last_run_id = 2;
        data.sessions.push(record(1, SessionOutcome::Completed, 30));
        assert!(data.validate().is_err(), "duplicate id");

        data.sessions.pop();
        assert!(data.validate().is_ok());
    }

    #[test]
    fn interrupted_run_is_recorded_once_and_not_resumed() {
        let mut timer = Timer::new(0);
        let mut data = AppData::default();
        let step = timer
            .start(T0, StartRequest::focus_from_input(None, None, 30).unwrap())
            .unwrap();
        data.apply_timer_step(&timer, &step).unwrap();
        let step = timer.tick(T0 + (7 * MINUTE) as EpochMs);
        data.apply_timer_step(&timer, &step).unwrap();
        assert!(data.active_run.is_some());
        assert!(data.validate().is_ok());

        // Simulated restart: the old timer is gone; only `data` survives.
        let recovered = data.recover_interrupted_run().unwrap().expect("interrupted run");
        assert_eq!(recovered.outcome, SessionOutcome::Interrupted);
        assert_eq!(recovered.timer_run_ms, 7 * MINUTE);
        assert!(recovered.task.is_untitled());
        assert!(data.active_run.is_none());
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.recover_interrupted_run(), Ok(None));
        assert_eq!(Timer::new(data.last_run_id).status(), TimerStatus::Idle);
    }

    #[test]
    fn applying_steps_records_completion_once_and_clears_marker() {
        let mut timer = Timer::new(0);
        let mut data = AppData::default();
        let step = timer
            .start(T0, StartRequest::focus_from_input(Some("코드 리뷰"), None, 10).unwrap())
            .unwrap();
        assert_eq!(data.apply_timer_step(&timer, &step), Ok(false));

        let done = timer.tick(T0 + (10 * MINUTE) as EpochMs);
        assert_eq!(data.apply_timer_step(&timer, &done), Ok(true));
        assert_eq!(data.apply_timer_step(&timer, &done), Ok(false));
        assert_eq!(data.sessions.len(), 1);
        assert!(data.active_run.is_none());
        assert_eq!(data.last_run_id, 1);
        assert!(data.validate().is_ok());
    }

    #[test]
    fn summary_counts_only_completed_focus_sessions_as_completed() {
        let mut data = AppData::default();
        data.record_session(record(1, SessionOutcome::Completed, 30)).unwrap();
        data.record_session(record(2, SessionOutcome::EndedEarly, 12)).unwrap();
        data.record_session(record(3, SessionOutcome::Reset, 3)).unwrap();
        let mut rest = record(4, SessionOutcome::Completed, 30);
        rest.kind = SessionKind::Break;
        data.record_session(rest).unwrap();
        data.sessions[1].return_count = 2;

        let summary = data.summary_between(T0, T0 + (24 * 60 * MINUTE) as EpochMs);
        assert_eq!(summary.completed_sessions, 1);
        assert_eq!(summary.timer_run_ms, 45 * MINUTE);
        assert_eq!(summary.return_count, 2);
        assert_eq!(data.summary_between(0, T0), DailySummary::default());
    }

    #[test]
    fn recent_tasks_are_distinct_newest_first_and_skip_untitled() {
        let mut data = AppData::default();
        for (id, name) in [(1, "메일"), (2, "보고서"), (3, ""), (4, "메일")] {
            let mut session = record(id, SessionOutcome::Completed, 30);
            session.task = TaskName::from_input(Some(name)).unwrap();
            data.record_session(session).unwrap();
        }
        let names: Vec<String> = data.recent_tasks(5).into_iter().map(String::from).collect();
        assert_eq!(names, ["메일", "보고서"]);
        assert_eq!(data.recent_tasks(1).len(), 1);
    }

    #[test]
    fn clear_history_keeps_settings_and_id_counters() {
        let mut data = AppData::default();
        data.settings.always_on_top = true;
        data.record_session(record(1, SessionOutcome::Completed, 30)).unwrap();
        data.capture_distraction(None, "메모", T0).unwrap();
        data.clear_history();
        assert!(data.sessions.is_empty() && data.distractions.is_empty());
        assert!(data.settings.always_on_top);
        assert_eq!((data.last_run_id, data.last_distraction_id), (1, 1));
        assert!(data.validate().is_ok());
    }

    #[test]
    fn json_round_trip_uses_versioned_camel_case_schema() {
        let mut data = AppData::default();
        data.record_session(record(1, SessionOutcome::Completed, 30)).unwrap();
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["sessions"][0]["timerRunMs"], 30 * MINUTE);
        assert_eq!(json["sessions"][0]["outcome"], "completed");
        assert_eq!(json["settings"]["defaultMinutes"], 30);
        let back: AppData = serde_json::from_value(json).unwrap();
        assert_eq!(back, data);
    }
}
