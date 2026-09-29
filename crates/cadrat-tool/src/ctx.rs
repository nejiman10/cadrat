//! What a command runs against, and how it reports (spec tool/cli §1, §5).

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use cadrat_hidraw::{Clock, System};
use serde_json::{Map, Value};

use crate::cli::Global;
use crate::exit::{Exit, Failure};

/// A SIGINT/SIGTERM flag that is only active while armed, so Ctrl-C keeps
/// its normal meaning outside `receiver pair`.
pub trait Interrupt {
    /// Starts catching the signals.
    fn arm(&self);
    /// Stops catching the signals.
    fn disarm(&self);
    /// Whether a signal arrived while armed.
    fn is_set(&self) -> bool;
}

/// The outside world: devices, time, signals and the environment.
pub struct Env<'a> {
    /// hidraw access.
    pub system: &'a dyn System,
    /// Time.
    pub clock: &'a dyn Clock,
    /// Signals.
    pub interrupt: &'a dyn Interrupt,
    /// `XDG_CONFIG_HOME`.
    pub xdg_config_home: Option<OsString>,
    /// `HOME`.
    pub home: Option<OsString>,
    /// How long to wait for the configuration lock (5 s in the binary).
    pub lock_timeout: std::time::Duration,
}

/// Standard streams.
pub struct Io<'a> {
    /// Results.
    pub stdout: &'a mut dyn Write,
    /// Warnings, errors and prompts.
    pub stderr: &'a mut dyn Write,
    /// Answers to the unpair confirmation.
    pub stdin: &'a mut dyn BufRead,
    /// Whether stdin is a terminal.
    pub stdin_is_terminal: bool,
}

/// A running command.
pub struct Ctx<'e, 'io> {
    pub env: &'e Env<'e>,
    pub io: Io<'io>,
    pub global: Global,
    /// JSON fields of the result.
    pub fields: Map<String, Value>,
    warnings: Vec<Value>,
}

// Output is best effort: a closed stdout or stderr must not turn a
// completed send into a failure.
impl<'e, 'io> Ctx<'e, 'io> {
    pub fn new(env: &'e Env<'e>, io: Io<'io>, global: Global) -> Self {
        Self {
            env,
            io,
            global,
            fields: Map::new(),
            warnings: Vec::new(),
        }
    }

    /// Prints a result line (suppressed with `--json`).
    pub fn out(&mut self, line: impl AsRef<str>) {
        if !self.global.json {
            let _ = writeln!(self.io.stdout, "{}", line.as_ref());
        }
    }

    /// Prints an informational line (suppressed with `--json` and `-q`).
    pub fn info(&mut self, line: impl AsRef<str>) {
        if !self.global.quiet {
            self.out(line);
        }
    }

    /// Prints a detail line to stderr with `-v`.
    pub fn verbose(&mut self, line: impl AsRef<str>) {
        if self.global.verbose {
            let _ = writeln!(self.io.stderr, "{}", line.as_ref());
        }
    }

    /// Prints a warning to stderr and records it for JSON.
    pub fn warn(&mut self, code: &str, message: impl AsRef<str>) {
        let message = message.as_ref();
        let _ = writeln!(self.io.stderr, "warning: {code}: {message}");
        self.warnings
            .push(serde_json::json!({ "code": code, "message": message }));
    }

    /// Prints a note to stderr (suppressed with `-q`).
    pub fn note(&mut self, message: impl AsRef<str>) {
        if !self.global.quiet {
            let _ = writeln!(self.io.stderr, "note: {}", message.as_ref());
        }
    }

    /// Sets a JSON result field.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.fields.insert(key.to_owned(), value.into());
    }

    /// The configuration file path (spec config §1).
    pub fn config_path(&self) -> Result<PathBuf, Failure> {
        if let Some(path) = &self.global.config {
            return Ok(path.clone());
        }
        cadrat_config::file::default_path(
            self.env.xdg_config_home.as_deref(),
            self.env.home.as_deref(),
        )
        .ok_or_else(|| {
            Failure::new(
                Exit::ConfigError,
                "cannot find the configuration directory: set HOME or use --config",
            )
        })
    }

    /// Prints the outcome and returns the exit code.
    pub fn finish(mut self, command: &str, result: &Result<(), Failure>) -> i32 {
        let (exit, error) = match result {
            Ok(()) => (Exit::Success, Value::Null),
            Err(failure) => {
                let _ = writeln!(self.io.stderr, "error: {}", failure.message);
                for hint in &failure.hints {
                    let _ = writeln!(self.io.stderr, "hint: {hint}");
                }
                (
                    failure.exit,
                    serde_json::json!({
                        "code": failure.exit.name(),
                        "message": failure.message,
                        "details": failure.details,
                    }),
                )
            }
        };
        if self.global.json {
            let mut object = Map::new();
            object.insert("format".into(), 1.into());
            object.insert("command".into(), command.into());
            object.insert("ok".into(), (exit == Exit::Success).into());
            object.insert("exit_code".into(), exit.code().into());
            object.append(&mut self.fields);
            object.insert(
                "warnings".into(),
                Value::Array(std::mem::take(&mut self.warnings)),
            );
            object.insert("error".into(), error);
            let _ = writeln!(self.io.stdout, "{}", Value::Object(object));
        }
        exit.code()
    }
}
