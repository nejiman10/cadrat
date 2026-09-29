//! `cadrat-tool`: the stand-alone configuration tool (spec tool/cli).
//!
//! [`run`] takes the arguments, the outside world ([`Env`]) and the standard
//! streams ([`Io`]), so the CLI tests run the whole program against the fake
//! transport of `cadrat-hidraw` (spec implementation §4). The binary only wires the real
//! implementations in.

// `expect` is used only where clap or this crate already guarantees the value.
#![allow(clippy::missing_panics_doc)]

mod cli;
mod cmd;
mod ctx;
mod exit;
mod render;

use std::ffi::OsString;

use clap::Parser;

pub use ctx::{Env, Interrupt, Io};
pub use exit::Exit;

use cli::{Cli, Command};

/// The command-line definition, for generating the manual page and shell
/// completions.
#[must_use]
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// Runs `cadrat-tool` and returns the exit code.
pub fn run(args: impl IntoIterator<Item = OsString>, env: &Env, io: Io) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            use clap::error::ErrorKind;
            let usage = !matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let rendered = error.render().to_string();
            let _ = if usage {
                write!(io.stderr, "{rendered}")
            } else {
                write!(io.stdout, "{rendered}")
            };
            if !usage {
                return Exit::Success.code();
            }
            if args.iter().any(|a| a == "--json") {
                let _ = writeln!(
                    io.stdout,
                    "{}",
                    serde_json::json!({
                        "format": 1, "command": null, "ok": false,
                        "exit_code": Exit::Usage.code(), "warnings": [],
                        "error": {"code": Exit::Usage.name(), "message": error.kind().to_string(), "details": null},
                    })
                );
            }
            return Exit::Usage.code();
        }
    };
    let name = match &cli.command {
        Command::List { .. } => "list",
        Command::Init { .. } => "init",
        Command::Get { .. } => "get",
        Command::Check => "check",
        Command::Set { .. } => "set",
        Command::Apply { .. } => "apply",
        Command::Receiver(cli::ReceiverCommand::Slots { .. }) => "receiver slots",
        Command::Receiver(cli::ReceiverCommand::Pair { .. }) => "receiver pair",
        Command::Receiver(cli::ReceiverCommand::Unpair { .. }) => "receiver unpair",
        Command::HoldOpen { .. } => "hold-open",
    };
    let mut ctx = ctx::Ctx::new(env, io, cli.global.clone());
    let result = match &cli.command {
        Command::List { nodes, redact } => cmd::list::run(&mut ctx, *nodes, *redact),
        Command::Init { preset, force } => cmd::config::init(&mut ctx, *preset, *force),
        Command::Get {
            keys,
            wire,
            values_only,
        } => cmd::config::get(&mut ctx, keys, *wire, *values_only),
        Command::Check => cmd::config::check(&mut ctx),
        Command::Set {
            assignments,
            dry_run,
            no_save,
        } => cmd::send::set(&mut ctx, assignments, *dry_run, *no_save),
        Command::Apply { dry_run } => cmd::send::apply(&mut ctx, *dry_run),
        Command::Receiver(command) => cmd::receiver::run(&mut ctx, command),
        Command::HoldOpen { poll_interval } => cmd::hold::run(&mut ctx, *poll_interval),
    };
    ctx.finish(name, &result)
}
