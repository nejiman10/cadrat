//! Command-line syntax (spec tool/cli §1, §3). The commands and options
//! shared with `cadratctl` are in `cadrat_command::cli`.

use std::path::PathBuf;

use cadrat_command::cli::{Common, Shared, VERSION, seconds};
use cadrat_command::{Command as Request, Options};
use clap::{Args, Parser, Subcommand};

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
#[derive(Debug, Clone, Default, Args)]
pub struct Global {
    #[command(flatten)]
    pub common: Common,
    /// Use this hidraw node directly (for development; checks still apply)
    #[arg(long, global = true, value_name = "PATH", conflicts_with_all = ["mouse", "route"])]
    pub hidraw: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(flatten)]
    Shared(Shared),
    /// Keep the wired C658's hidraw nodes open until stopped
    ///
    /// Without this, a wired C658 was seen to stop sending input a few
    /// seconds after it was plugged in. Runs in the foreground, for testing;
    /// the cadrat-hold-open system service keeps the nodes open day to day.
    HoldOpen {
        /// Seconds between checks for plugged and unplugged nodes
        #[arg(long, default_value = "1.0", value_parser = seconds)]
        poll_interval: f64,
    },
}

impl Global {
    /// The options shared with `cadratd` (`--json` only changes the output).
    pub fn options(&self) -> Options {
        Options {
            hidraw: self.hidraw.clone(),
            ..self.common.options()
        }
    }
}

impl Command {
    /// The shared command, or `None` for `hold-open`, which only `cadrat-tool` has.
    pub fn request(&self) -> Option<Request> {
        match self {
            Self::Shared(shared) => Some(shared.request()),
            Self::HoldOpen { .. } => None,
        }
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
