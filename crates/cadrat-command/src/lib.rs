//! The commands shared by `cadrat-tool` and `cadratd` (spec implementation §1, P11).
//!
//! [`execute`] runs one command against the outside world ([`Env`]) and returns
//! its result as the `--json` object (spec tool/cli §5). Nothing is printed on
//! standard output while it runs: [`render::human`] builds the human-readable
//! lines from that object, so a front end that only has the JSON (`cadratctl`)
//! shows exactly what `cadrat-tool` shows. What has to happen while the
//! command runs (warnings, `-v` detail, the pairing prompt, the unpair
//! confirmation) goes through [`Frontend`].

// `expect` is used only where the caller or this crate already guarantees the value.
#![allow(clippy::missing_panics_doc)]

mod cmd;
mod ctx;
mod daemon;
mod format;
mod select;

pub use cadrat_cli::{
    Command, Exit, Failure, Options, cli, envelope, no_mouse, not_confirmed, render,
    unpair_arguments,
};
pub use ctx::{Env, Frontend, Interrupt};
pub use daemon::DaemonLock;

use serde_json::Value;

/// Runs one command and returns its `--json` object.
///
/// `program` is the command name used in guidance (`cadrat-tool` or
/// `cadratctl`, spec ctl/cli §3).
pub fn execute(
    env: &Env,
    frontend: &mut dyn Frontend,
    options: &Options,
    command: &Command,
    program: &str,
) -> Value {
    let mut ctx = ctx::Ctx::new(env, frontend, options, program);
    let result = match command {
        Command::List { nodes, redact } => cmd::list::run(&mut ctx, *nodes, *redact),
        Command::Init { preset, force } => cmd::config::init(&mut ctx, *preset, *force),
        Command::Get { keys, wire, .. } => cmd::config::get(&mut ctx, keys, *wire),
        Command::Check => cmd::config::check(&mut ctx),
        Command::Set {
            assignments,
            dry_run,
            no_save,
        } => cmd::send::set(&mut ctx, assignments, *dry_run, *no_save),
        Command::Apply { dry_run } => cmd::send::apply(&mut ctx, *dry_run),
        Command::ReceiverSlots { receiver, redact } => {
            cmd::receiver::slots(&mut ctx, receiver.as_deref(), *redact)
        }
        Command::ReceiverPair { receiver, polling } => {
            cmd::receiver::pair(&mut ctx, receiver.as_deref(), *polling)
        }
        Command::ReceiverUnpair {
            receiver,
            slot,
            yes,
            expected,
            polling,
        } => cmd::receiver::unpair(
            &mut ctx,
            receiver.as_deref(),
            *slot,
            *yes,
            *expected,
            *polling,
        ),
    };
    ctx.finish(command.name(), &result)
}
