//! Core logic for 다시, 집중 (focus-return).
//!
//! This crate has no dependency on Tauri, any UI, the system clock or threads.
//! Every time-dependent call receives the current wall-clock time from the
//! caller as [`EpochMs`], which makes the timer fully deterministic and lets a
//! future Tauri layer decide how and when to observe time.
//!
//! * [`timer`]: the idle/running/paused/completed state machine.
//! * [`model`]: validated, versioned settings, drafts, distractions and session records.
//! * [`storage`]: crash-tolerant local JSON storage in a caller-provided directory.

#![forbid(unsafe_code)]

pub mod error;
pub mod model;
pub mod storage;
pub mod timer;

/// Milliseconds since the Unix epoch (wall clock), always supplied by the caller.
pub type EpochMs = i64;

pub use error::{StorageError, TextField, TimerError, ValidationError};
pub use model::{
    ActiveRun, AppData, DailySummary, Distraction, DistractionText, Minutes, NextAction,
    SessionOutcome, SessionRecord, Settings, TaskDraft, TaskName, SCHEMA_VERSION, UNTITLED_TASK,
};
pub use storage::{CorruptionKind, LoadReport, LoadSource, PreservedFile, SaveReport, Store};
pub use timer::{
    RunId, SessionKind, StartRequest, Step, Timer, TimerCommand, TimerSnapshot, TimerStatus,
};
