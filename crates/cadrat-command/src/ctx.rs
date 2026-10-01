//! What a command runs against, and how its result is collected (spec tool/cli §5).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use cadrat_hidraw::{Clock, System};
use serde_json::{Map, Value};

use crate::Options;
use crate::daemon::{DaemonGuard, DaemonLock};
use cadrat_cli::{Exit, Failure, envelope};

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
    /// How long to wait for the configuration lock (5 s in the binaries).
    pub lock_timeout: Duration,
    /// `cadratd`'s lock, checked before writing to a device (`cadrat-tool`);
    /// `None` in `cadratd` itself.
    pub daemon_lock: Option<DaemonLock>,
}

/// What has to reach the user while a command runs. Everything else is in
/// the returned JSON.
pub trait Frontend {
    /// A `-v` detail line (only called with [`Options::verbose`]).
    fn verbose(&mut self, line: &str);
    /// A warning as it happens. It is also in the JSON `warnings`.
    fn warning(&mut self, code: &str, message: &str);
    /// Pairing mode has started on the Receiver with this key: tell the
    /// user to put the mouse in pairing mode.
    fn pairing_started(&mut self, receiver: &str, timeout: Duration);
    /// Whether [`Frontend::confirm_unpair`] can ask anyone. Without `--yes`,
    /// unpair is a usage error otherwise (spec receiver §4 step 3).
    fn can_confirm(&self) -> bool;
    /// Shows the unpair target (its slot line and any caution).
    fn unpair_target(&mut self, lines: &[String]);
    /// Asks whether to unpair the slot just shown.
    fn confirm_unpair(&mut self, slot: cadrat_proto::Slot) -> bool;
}

/// A running command.
pub(crate) struct Ctx<'e, 'f> {
    pub env: &'e Env<'e>,
    pub frontend: &'f mut dyn Frontend,
    pub options: &'e Options,
    /// `cadrat-tool` or `cadratctl`, for guidance.
    pub program: &'e str,
    fields: Map<String, Value>,
    warnings: Vec<Value>,
}

impl<'e, 'f> Ctx<'e, 'f> {
    pub fn new(
        env: &'e Env<'e>,
        frontend: &'f mut dyn Frontend,
        options: &'e Options,
        program: &'e str,
    ) -> Self {
        Self {
            env,
            frontend,
            options,
            program,
            fields: Map::new(),
            warnings: Vec::new(),
        }
    }

    /// Sends a detail line with `-v`.
    pub fn verbose(&mut self, line: impl AsRef<str>) {
        if self.options.verbose {
            self.frontend.verbose(line.as_ref());
        }
    }

    /// Reports a warning and records it for JSON.
    pub fn warn(&mut self, code: &str, message: impl AsRef<str>) {
        let message = message.as_ref();
        self.frontend.warning(code, message);
        self.warnings
            .push(serde_json::json!({ "code": code, "message": message }));
    }

    /// Sets a JSON result field.
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.fields.insert(key.to_owned(), value.into());
    }

    /// Checks that `cadratd` is not running before writing to a device
    /// (spec tool/cli §8). Keep the guard until the command ends.
    /// `instead` is the same operation as a `cadratctl` command.
    pub fn hold_daemon_lock(&self, instead: &str) -> Result<Option<DaemonGuard>, Failure> {
        self.env
            .daemon_lock
            .as_ref()
            .map(|lock| lock.hold(instead))
            .transpose()
    }

    /// The configuration file path (spec config §1).
    pub fn config_path(&self) -> Result<PathBuf, Failure> {
        if let Some(path) = &self.options.config {
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

    /// The `--json` object.
    pub fn finish(self, command: &str, result: &Result<(), Failure>) -> Value {
        envelope(Some(command), self.fields, self.warnings, result)
    }
}
