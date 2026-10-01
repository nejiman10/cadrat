//! What `cadrat-tool`, `cadratctl` and `cadratd` agree on without touching
//! devices: the command-line syntax ([`cli`]), a request ([`Command`],
//! [`Options`]), the exit codes ([`Exit`], [`Failure`]), the `--json` frame
//! ([`envelope`]) and the human-readable output built from it ([`render`])
//! (spec tool/cli §1, §5, §6, ctl/cli).
//!
//! It does not depend on `cadrat-hidraw`: `cadratctl` uses only this crate
//! and never opens hidraw, and the build keeps it that way (AGENTS.md). The
//! command procedures are in `cadrat-command`.

// `expect` is used only where clap or this crate already guarantees the value.
#![allow(clippy::missing_panics_doc)]

pub mod cli;
mod exit;
pub mod render;
mod request;

pub use exit::{Exit, Failure};
pub use request::{Command, Options};

use serde_json::{Map, Value};

/// The common `--json` frame around a command's fields (spec tool/cli §5).
/// `command` is `None` when the arguments could not be parsed.
#[must_use]
pub fn envelope(
    command: Option<&str>,
    mut fields: Map<String, Value>,
    warnings: Vec<Value>,
    result: &Result<(), Failure>,
) -> Value {
    let (exit, error) = match result {
        Ok(()) => (Exit::Success, Value::Null),
        Err(failure) => (
            failure.exit,
            serde_json::json!({
                "code": failure.exit.name(),
                "message": failure.message,
                "hints": failure.hints,
                "details": failure.details,
            }),
        ),
    };
    let mut object = Map::new();
    object.insert("format".into(), 1.into());
    object.insert("command".into(), command.into());
    object.insert("ok".into(), (exit == Exit::Success).into());
    object.insert("exit_code".into(), exit.code().into());
    object.append(&mut fields);
    object.insert("warnings".into(), Value::Array(warnings));
    object.insert("error".into(), error);
    Value::Object(object)
}

/// Spec receiver §5: receiver commands do not take a mouse.
///
/// # Errors
///
/// `Usage` with `--mouse` or `--route`.
pub fn no_mouse(options: &Options) -> Result<(), Failure> {
    if options.mouse.is_some() || options.route.is_some() {
        return Err(Failure::new(
            Exit::Usage,
            "receiver commands do not take --mouse or --route",
        ));
    }
    Ok(())
}

/// Spec receiver §4 step 3: without `--yes`, unpair needs someone to ask.
///
/// # Errors
///
/// `Usage` with `--mouse` or `--route`, or when nobody can be asked.
pub fn unpair_arguments(options: &Options, yes: bool, can_confirm: bool) -> Result<(), Failure> {
    no_mouse(options)?;
    if !yes && !can_confirm {
        return Err(Failure::new(
            Exit::Usage,
            "unpair asks for confirmation on a terminal; pass --yes to skip it",
        ));
    }
    Ok(())
}

/// The refused confirmation (`Aborted`, 18).
#[must_use]
pub fn not_confirmed() -> Failure {
    Failure::new(Exit::Aborted, "not confirmed; nothing was done")
}
