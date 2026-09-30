//! Exit codes (spec tool/cli §6).

/// Every exit code `cadrat-tool` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// 0: success, warnings included.
    Success,
    /// 1: unexpected internal error.
    Internal,
    /// 2: invalid arguments.
    Usage,
    /// 3: the TOML file is missing, malformed, invalid or incomplete.
    ConfigError,
    /// 4: no matching mouse or Receiver.
    NoDevice,
    /// 5: more than one candidate.
    AmbiguousTarget,
    /// 6: a candidate node cannot be opened.
    PermissionDenied,
    /// 7: `--hidraw` node rejected, or `ambiguous-node`.
    DeviceInvalid,
    /// 8: the send failed; the TOML file is unchanged.
    SendFailed,
    /// 9: sent, but the TOML file was not saved.
    SentNotSaved,
    /// 10: other file I/O failure.
    IoError,
    /// 11: the configuration lock is held by another process.
    ConfigLocked,
    /// 12: no new slot before the timeout, or interrupted (stop succeeded).
    PairTimeout,
    /// 13: stopping pairing mode failed.
    PairStopFailed,
    /// 14: the pair-start or unpair request failed.
    ReceiverCommandFailed,
    /// 15: the slot did not empty before the timeout.
    UnpairNotConfirmed,
    /// 16: the unpair target is empty or changed; nothing was done.
    SlotChanged,
    /// 17: a slot read failed or returned a malformed report.
    ReceiverProtocolError,
    /// 18: the confirmation was refused.
    Aborted,
    /// 19: the pre-send check found a different device; nothing was sent.
    TargetChanged,
}

impl Exit {
    /// The process exit code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Internal => 1,
            Self::Usage => 2,
            Self::ConfigError => 3,
            Self::NoDevice => 4,
            Self::AmbiguousTarget => 5,
            Self::PermissionDenied => 6,
            Self::DeviceInvalid => 7,
            Self::SendFailed => 8,
            Self::SentNotSaved => 9,
            Self::IoError => 10,
            Self::ConfigLocked => 11,
            Self::PairTimeout => 12,
            Self::PairStopFailed => 13,
            Self::ReceiverCommandFailed => 14,
            Self::UnpairNotConfirmed => 15,
            Self::SlotChanged => 16,
            Self::ReceiverProtocolError => 17,
            Self::Aborted => 18,
            Self::TargetChanged => 19,
        }
    }

    /// The name used in JSON `error.code`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Internal => "Internal",
            Self::Usage => "Usage",
            Self::ConfigError => "ConfigError",
            Self::NoDevice => "NoDevice",
            Self::AmbiguousTarget => "AmbiguousTarget",
            Self::PermissionDenied => "PermissionDenied",
            Self::DeviceInvalid => "DeviceInvalid",
            Self::SendFailed => "SendFailed",
            Self::SentNotSaved => "SentNotSaved",
            Self::IoError => "IoError",
            Self::ConfigLocked => "ConfigLocked",
            Self::PairTimeout => "PairTimeout",
            Self::PairStopFailed => "PairStopFailed",
            Self::ReceiverCommandFailed => "ReceiverCommandFailed",
            Self::UnpairNotConfirmed => "UnpairNotConfirmed",
            Self::SlotChanged => "SlotChanged",
            Self::ReceiverProtocolError => "ReceiverProtocolError",
            Self::Aborted => "Aborted",
            Self::TargetChanged => "TargetChanged",
        }
    }
}

/// A command failure: the exit code, a message and optional details.
#[derive(Debug, Clone)]
pub struct Failure {
    /// Exit code.
    pub exit: Exit,
    /// One-line message for stderr and JSON.
    pub message: String,
    /// Extra lines printed after the message.
    pub hints: Vec<String>,
    /// Machine-readable details for JSON.
    pub details: serde_json::Value,
}

impl Failure {
    /// A failure without details.
    pub fn new(exit: Exit, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
            hints: Vec::new(),
            details: serde_json::Value::Null,
        }
    }

    /// Adds a hint line.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }

    /// Sets the JSON details.
    #[must_use]
    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}

impl From<cadrat_hidraw::SelectError> for Failure {
    fn from(error: cadrat_hidraw::SelectError) -> Self {
        use cadrat_hidraw::SelectError as E;
        let exit = match &error {
            E::NoDevice => Exit::NoDevice,
            E::Ambiguous(_) => Exit::AmbiguousTarget,
            E::PermissionDenied(_) => Exit::PermissionDenied,
            E::DeviceInvalid(_) => Exit::DeviceInvalid,
        };
        let failure = Self::new(exit, error.to_string());
        match error {
            E::Ambiguous(keys) => failure
                .hint("choose one with --mouse=<number or key> (or --receiver=<key>)")
                .details(serde_json::json!({ "candidates": keys })),
            E::PermissionDenied(paths) => failure.details(serde_json::json!({
                "nodes": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
            })),
            _ => failure,
        }
    }
}

impl From<cadrat_config::FileError> for Failure {
    fn from(error: cadrat_config::FileError) -> Self {
        use cadrat_config::FileError as E;
        let exit = match &error {
            E::NotFound(_) | E::NotUtf8(_) => Exit::ConfigError,
            E::Locked(_) => Exit::ConfigLocked,
            E::Changed(_) => Exit::SentNotSaved,
            E::AlreadyExists(_) | E::Io { .. } => Exit::IoError,
        };
        Self::new(exit, error.to_string())
    }
}
