//! A tiny logging sink so diagnostics (warnings about dropped packs, refused overlays, skipped files…) are observable
//! in tests and can be forwarded to any logger by the host application.

use std::fmt;
use std::sync::{Arc, Mutex};

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Informational.
    Info,
    /// Something was skipped or repaired.
    Warn,
    /// Something failed.
    Error,
}

/// One recorded diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Severity.
    pub level: Level,
    /// Message text.
    pub message: String,
}

/// Receiver for diagnostics.
pub trait LogSink: Send + Sync + fmt::Debug {
    /// Records one diagnostic.
    fn log(&self, level: Level, message: String);
}

/// Shared handle to a sink.
pub type SharedLog = Arc<dyn LogSink>;

/// Convenience helpers on any sink.
pub trait LogExt {
    /// Logs at [`Level::Info`].
    fn info(&self, message: impl Into<String>);
    /// Logs at [`Level::Warn`].
    fn warn(&self, message: impl Into<String>);
    /// Logs at [`Level::Error`].
    fn error(&self, message: impl Into<String>);
}

impl<T: LogSink + ?Sized> LogExt for T {
    fn info(&self, message: impl Into<String>) {
        self.log(Level::Info, message.into());
    }
    fn warn(&self, message: impl Into<String>) {
        self.log(Level::Warn, message.into());
    }
    fn error(&self, message: impl Into<String>) {
        self.log(Level::Error, message.into());
    }
}

/// Discards everything.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullLog;

impl LogSink for NullLog {
    fn log(&self, _level: Level, _message: String) {}
}

/// Collects diagnostics in memory.
#[derive(Debug, Default)]
pub struct MemoryLog {
    entries: Mutex<Vec<LogEntry>>,
}

impl MemoryLog {
    /// A new, shareable, empty log.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A copy of everything recorded so far.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether any entry at `level` contains `needle`.
    pub fn contains(&self, level: Level, needle: &str) -> bool {
        self.entries()
            .iter()
            .any(|e| e.level == level && e.message.contains(needle))
    }

    /// Whether any entry at all contains `needle`.
    pub fn contains_any(&self, needle: &str) -> bool {
        self.entries().iter().any(|e| e.message.contains(needle))
    }
}

impl LogSink for MemoryLog {
    fn log(&self, level: Level, message: String) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(LogEntry { level, message });
    }
}

/// A shared [`NullLog`].
pub fn null_log() -> SharedLog {
    Arc::new(NullLog)
}
