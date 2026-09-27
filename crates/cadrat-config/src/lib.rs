//! Schema 1 TOML configuration for cadrat (spec 01).
//!
//! The TOML file is the only record of the mouse settings (P1). This crate
//! reads and validates it, applies `key=value` changes, rewrites only the
//! changed values so comments and notation survive, saves atomically, and
//! serializes readers and writers with a lock file. `cadrat-tool` and the
//! later daemon use it so that both treat the file the same way.

pub mod config;
pub mod document;
pub mod file;
pub mod key;
pub mod template;

pub use config::{Change, Config, Warning, WrongValueKind};
pub use document::{Checked, ConfigError, Document, Problem, SyntaxError, UpdateError};
pub use file::{ConfigLock, FileError, LOCK_TIMEOUT, Loaded};
pub use key::{AssignmentError, Key, UnknownKey, Value, parse_assignments};
pub use template::{Preset, template};
