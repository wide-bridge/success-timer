//! Error types. Messages describe structure only and never echo user text,
//! so they are safe to show or log without leaking task or distraction content.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::timer::{RunId, TimerCommand, TimerStatus};

/// Which user-entered text field failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    Task,
    NextAction,
    Distraction,
}

impl fmt::Display for TextField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TextField::Task => "task",
            TextField::NextAction => "next action",
            TextField::Distraction => "distraction",
        })
    }
}

/// Input or stored data that violates the model rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Duration is outside 1..=120 whole minutes.
    MinutesOutOfRange { value: i64 },
    /// Duration input was not a whole number.
    MinutesNotInteger,
    TextTooLong {
        field: TextField,
        max_chars: usize,
        actual_chars: usize,
    },
    /// Text must be a single line without control characters.
    TextControlCharacter { field: TextField },
    /// Stored text was not trimmed/normalized (only produced when loading data).
    TextNotNormalized { field: TextField },
    /// Structural problem in records or collections.
    InvalidData { detail: String },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::MinutesOutOfRange { value } => {
                write!(f, "minutes must be a whole number from 1 to 120 (got {value})")
            }
            ValidationError::MinutesNotInteger => f.write_str("minutes must be a whole number"),
            ValidationError::TextTooLong {
                field,
                max_chars,
                actual_chars,
            } => write!(
                f,
                "{field} is too long ({actual_chars} characters, max {max_chars})"
            ),
            ValidationError::TextControlCharacter { field } => {
                write!(f, "{field} must be a single line without control characters")
            }
            ValidationError::TextNotNormalized { field } => {
                write!(f, "stored {field} is not trimmed or is empty")
            }
            ValidationError::InvalidData { detail } => write!(f, "invalid data: {detail}"),
        }
    }
}

impl Error for ValidationError {}

/// A timer command that is not allowed in the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerError {
    InvalidTransition {
        command: TimerCommand,
        status: TimerStatus,
    },
    /// The command referenced a run that is no longer current (e.g. after reset).
    StaleRun {
        command: TimerCommand,
        current: Option<RunId>,
        requested: RunId,
    },
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimerError::InvalidTransition { command, status } => {
                write!(f, "cannot {command:?} while the timer is {status:?}")
            }
            TimerError::StaleRun {
                command,
                current,
                requested,
            } => match current {
                Some(current) => write!(
                    f,
                    "{command:?} targets run {requested}, but the current run is {current}"
                ),
                None => write!(f, "{command:?} targets run {requested}, but no run is active"),
            },
        }
    }
}

impl Error for TimerError {}

/// Local storage failure. The previous data file is never destroyed by a failed save.
#[derive(Debug)]
pub enum StorageError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Serialize(serde_json::Error),
    /// The file was written by a newer app version; it is left untouched and saving is refused.
    NewerSchema {
        path: PathBuf,
        found: u64,
        supported: u32,
    },
    /// The caller tried to save data that does not pass validation.
    InvalidData(ValidationError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io {
                operation,
                path,
                source,
            } => write!(f, "failed to {operation} ({}): {source}", path.display()),
            StorageError::Serialize(err) => write!(f, "failed to serialize data: {err}"),
            StorageError::NewerSchema {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} uses schema version {found}, newer than supported version {supported}",
                path.display()
            ),
            StorageError::InvalidData(err) => write!(f, "refusing to save: {err}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StorageError::Io { source, .. } => Some(source),
            StorageError::Serialize(err) => Some(err),
            StorageError::InvalidData(err) => Some(err),
            StorageError::NewerSchema { .. } => None,
        }
    }
}
