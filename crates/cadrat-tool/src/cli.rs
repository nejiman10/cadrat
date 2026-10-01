//! Command-line syntax (spec tool/cli §1, §3).

use std::path::PathBuf;
use std::time::Duration;

use cadrat_command::{Command as Request, Options};
use cadrat_hidraw::receiver::Polling;
use cadrat_proto::Slot;
use clap::{Args, Parser, Subcommand, ValueEnum};

/// The version shown by `--version`: `CADRAT_VERSION` at build time (set by
/// packaging/build-deb.sh, e.g. `0.1.0~test1+gabc1234`), else the crate
/// version.
const VERSION: &str = match option_env!("CADRAT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// Configure the C658 mouse and the C652 Receiver without a daemon.
#[derive(Debug, Parser)]
#[command(name = "cadrat-tool", version = VERSION, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Command,
}

/// Options every command accepts.
#[allow(clippy::doc_markdown)]
#[derive(Debug, Clone, Default, Args)]
pub struct Global {
    /// Configuration file [default: $XDG_CONFIG_HOME/cadrat/default.toml]
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Target mouse: a number from `list`, a key, or a unique key prefix
    #[arg(long, global = true, value_name = "SELECTOR")]
    pub mouse: Option<String>,
    /// Send over this route instead of the active one
    #[arg(long, global = true, value_enum)]
    pub route: Option<RouteArg>,
    /// Use this hidraw node directly (for development; checks still apply)
    #[arg(long, global = true, value_name = "PATH", conflicts_with_all = ["mouse", "route"])]
    pub hidraw: Option<PathBuf>,
    /// Print one JSON object on stdout
    #[arg(long, global = true)]
    pub json: bool,
    /// Show more detail
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    pub verbose: bool,
    /// Print only results and warnings
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

/// `--route` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RouteArg {
    /// C658 connected by cable
    Wired,
    /// C658 through the C652 Receiver
    Receiver,
}

impl From<RouteArg> for cadrat_hidraw::Route {
    fn from(route: RouteArg) -> Self {
        match route {
            RouteArg::Wired => Self::Wired,
            RouteArg::Receiver => Self::Receiver,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List connected mice and Receivers (reads only)
    List {
        /// Also show every hidraw node and how it was classified
        #[arg(long)]
        nodes: bool,
        /// Hide device IDs and slot identifiers
        #[arg(long)]
        redact: bool,
    },
    /// Create the configuration file
    Init {
        /// Fill in values instead of leaving them commented out
        #[arg(long, value_enum)]
        preset: Option<Preset>,
        /// Overwrite an existing file
        #[arg(long)]
        force: bool,
    },
    /// Show values from the configuration file
    Get {
        /// Keys to show [default: all]
        keys: Vec<String>,
        /// Also show the wire report and its field layout
        #[arg(long)]
        wire: bool,
        /// Print values only
        #[arg(short = 'n')]
        values_only: bool,
    },
    /// Validate the configuration file
    Check,
    /// Change values, send the complete settings, then save
    Set {
        /// key=value assignments
        #[arg(required = true, value_name = "KEY=VALUE")]
        assignments: Vec<String>,
        /// Show the result without sending or saving
        #[arg(long, conflicts_with = "no_save")]
        dry_run: bool,
        /// Send without saving
        #[arg(long)]
        no_save: bool,
    },
    /// Send the configuration file as it is
    Apply {
        /// Show the wire report without sending
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage the C652 Receiver
    #[command(subcommand)]
    Receiver(ReceiverCommand),
    /// Keep the wired C658's hidraw nodes open until stopped
    ///
    /// Without this, a wired C658 was seen to stop sending input a few
    /// seconds after it was plugged in. Runs in the foreground; the
    /// cadrat-hold-open.service user unit runs it in the background.
    HoldOpen {
        /// Seconds between checks for plugged and unplugged nodes
        #[arg(long, default_value = "1.0", value_parser = seconds)]
        poll_interval: f64,
    },
}

/// `init --preset` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Preset {
    /// The research SDK's static-analysis baseline (not a device or factory value)
    ResearchBaseline,
}

#[derive(Debug, Subcommand)]
pub enum ReceiverCommand {
    /// Show slots 0..4 (reads only)
    Slots {
        #[command(flatten)]
        receiver: ReceiverArg,
        /// Hide slot identifiers
        #[arg(long)]
        redact: bool,
    },
    /// Pair a new device into a free slot
    Pair {
        #[command(flatten)]
        receiver: ReceiverArg,
        /// Seconds to wait for a new slot
        #[arg(long, default_value = "60", value_parser = seconds)]
        timeout: f64,
        /// Seconds between slot reads
        #[arg(long, default_value = "1.0", value_parser = seconds)]
        poll_interval: f64,
    },
    /// Unpair the device in one slot
    Unpair {
        /// Slot number, 0..4
        #[arg(value_parser = clap::value_parser!(u8).range(0..=4))]
        slot: u8,
        #[command(flatten)]
        receiver: ReceiverArg,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
        /// Seconds to wait for the slot to empty
        #[arg(long, default_value = "15", value_parser = seconds)]
        timeout: f64,
        /// Seconds between slot reads
        #[arg(long, default_value = "0.5", value_parser = seconds)]
        poll_interval: f64,
    },
}

/// `--receiver`.
#[derive(Debug, Clone, Args)]
pub struct ReceiverArg {
    /// Target Receiver: a key or a unique key prefix
    #[arg(long, value_name = "KEY")]
    pub receiver: Option<String>,
}

fn config_preset(preset: Option<Preset>) -> cadrat_config::Preset {
    match preset {
        None => cadrat_config::Preset::Empty,
        Some(Preset::ResearchBaseline) => cadrat_config::Preset::ResearchBaseline,
    }
}

impl Global {
    /// The options shared with `cadratd` (`--json` only changes the output).
    pub fn options(&self) -> Options {
        Options {
            config: self.config.clone(),
            mouse: self.mouse.clone(),
            route: self.route.map(Into::into),
            hidraw: self.hidraw.clone(),
            verbose: self.verbose,
            quiet: self.quiet,
        }
    }
}

impl Command {
    /// The shared command, or `None` for `hold-open`, which only `cadrat-tool` has.
    pub fn request(&self) -> Option<Request> {
        Some(match self {
            Self::List { nodes, redact } => Request::List {
                nodes: *nodes,
                redact: *redact,
            },
            Self::Init { preset, force } => Request::Init {
                preset: config_preset(*preset),
                force: *force,
            },
            Self::Get {
                keys,
                wire,
                values_only,
            } => Request::Get {
                keys: keys.clone(),
                wire: *wire,
                values_only: *values_only,
            },
            Self::Check => Request::Check,
            Self::Set {
                assignments,
                dry_run,
                no_save,
            } => Request::Set {
                assignments: assignments.clone(),
                dry_run: *dry_run,
                no_save: *no_save,
            },
            Self::Apply { dry_run } => Request::Apply { dry_run: *dry_run },
            Self::Receiver(ReceiverCommand::Slots { receiver, redact }) => Request::ReceiverSlots {
                receiver: receiver.receiver.clone(),
                redact: *redact,
            },
            Self::Receiver(ReceiverCommand::Pair {
                receiver,
                timeout,
                poll_interval,
            }) => Request::ReceiverPair {
                receiver: receiver.receiver.clone(),
                polling: polling(*timeout, *poll_interval),
            },
            Self::Receiver(ReceiverCommand::Unpair {
                slot,
                receiver,
                yes,
                timeout,
                poll_interval,
            }) => Request::ReceiverUnpair {
                receiver: receiver.receiver.clone(),
                slot: Slot::new(*slot).expect("clap checks the range"),
                yes: *yes,
                polling: polling(*timeout, *poll_interval),
            },
            Self::HoldOpen { .. } => return None,
        })
    }
}

fn polling(timeout: f64, interval: f64) -> Polling {
    Polling {
        timeout: Duration::from_secs_f64(timeout),
        interval: Duration::from_secs_f64(interval),
    }
}

fn seconds(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() && value > 0.0 && value <= 86_400.0 => Ok(value),
        _ => Err("expected a number of seconds greater than 0".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
