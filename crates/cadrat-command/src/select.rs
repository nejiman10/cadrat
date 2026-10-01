//! The `Failure` of a selection error (spec tool/cli §6).

use cadrat_cli::{Exit, Failure};
use cadrat_hidraw::SelectError;

/// The exit code, hints and details of a [`SelectError`].
pub(crate) fn failure(error: SelectError) -> Failure {
    let exit = match &error {
        SelectError::NoDevice => Exit::NoDevice,
        SelectError::Ambiguous(_) => Exit::AmbiguousTarget,
        SelectError::PermissionDenied(_) => Exit::PermissionDenied,
        SelectError::DeviceInvalid(_) => Exit::DeviceInvalid,
    };
    let failure = Failure::new(exit, error.to_string());
    match error {
        SelectError::Ambiguous(keys) => failure
            .hint("choose one with --mouse=<number or key> (or --receiver=<key>)")
            .details(serde_json::json!({ "candidates": keys })),
        SelectError::PermissionDenied(paths) => failure.details(serde_json::json!({
            "nodes": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
        })),
        _ => failure,
    }
}
