//! What a front end asks for: the common options and one command (spec tool/cli §1, §3).

use std::path::PathBuf;

use cadrat_config::Preset;
use cadrat_proto::{Polling, Route, Slot};

/// Options every command accepts (spec tool/cli §1).
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--config`; `None` uses the default path (spec config §1).
    pub config: Option<PathBuf>,
    /// `--mouse`.
    pub mouse: Option<String>,
    /// `--route`.
    pub route: Option<Route>,
    /// `--hidraw` (`cadrat-tool` only).
    pub hidraw: Option<PathBuf>,
    /// `-v`: per-node detail through `cadrat_command::Frontend::verbose`.
    pub verbose: bool,
    /// The per-node detail of `-v` also goes into the JSON `nodes`
    /// (`cadratd`'s `verbose` key, spec dbus §3). [`crate::render::verbose_lines`]
    /// turns it back into the `-v` lines.
    pub verbose_json: bool,
    /// `-q`: no informational lines.
    pub quiet: bool,
}

/// One command and its own arguments.
#[derive(Debug, Clone)]
pub enum Command {
    /// `list`.
    List {
        /// `--nodes`.
        nodes: bool,
        /// `--redact`.
        redact: bool,
    },
    /// `init`.
    Init {
        /// `--preset`; [`Preset::Empty`] without it.
        preset: Preset,
        /// `--force`.
        force: bool,
    },
    /// `get`.
    Get {
        /// Keys as given; empty means all.
        keys: Vec<String>,
        /// `--wire`.
        wire: bool,
        /// `-n`: only changes the human-readable output.
        values_only: bool,
    },
    /// `check`.
    Check,
    /// `set`.
    Set {
        /// `key=value` arguments as given.
        assignments: Vec<String>,
        /// `--dry-run`.
        dry_run: bool,
        /// `--no-save`.
        no_save: bool,
    },
    /// `apply`.
    Apply {
        /// `--dry-run`.
        dry_run: bool,
    },
    /// `receiver slots`.
    ReceiverSlots {
        /// `--receiver`.
        receiver: Option<String>,
        /// `--redact`.
        redact: bool,
    },
    /// `receiver pair`.
    ReceiverPair {
        /// `--receiver`.
        receiver: Option<String>,
        /// `--timeout` and `--poll-interval`.
        polling: Polling,
    },
    /// `receiver unpair`.
    ReceiverUnpair {
        /// `--receiver`.
        receiver: Option<String>,
        /// The slot to unpair.
        slot: Slot,
        /// `--yes`.
        yes: bool,
        /// The raw slot response the caller confirmed (`cadratd`'s
        /// `expected`, spec dbus §6): no confirmation is asked, and a slot
        /// that no longer reads the same is `SlotChanged`.
        expected: Option<[u8; 8]>,
        /// `--timeout` and `--poll-interval`.
        polling: Polling,
    },
}

impl Command {
    /// The JSON `command` value.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::List { .. } => "list",
            Self::Init { .. } => "init",
            Self::Get { .. } => "get",
            Self::Check => "check",
            Self::Set { .. } => "set",
            Self::Apply { .. } => "apply",
            Self::ReceiverSlots { .. } => "receiver slots",
            Self::ReceiverPair { .. } => "receiver pair",
            Self::ReceiverUnpair { .. } => "receiver unpair",
        }
    }
}
