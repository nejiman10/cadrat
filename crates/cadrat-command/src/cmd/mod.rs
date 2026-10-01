//! One module per command group. Commands only record results; the
//! human-readable lines come from [`crate::render`].

pub mod config;
pub mod list;
pub mod receiver;
pub mod send;
