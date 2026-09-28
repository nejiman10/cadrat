//! Linux hidraw access for cadrat: discovery, selection, sending and
//! Receiver management (spec 02, spec 05).
//!
//! All I/O and time go through the [`System`], [`Device`] and [`Clock`]
//! traits. [`LinuxSystem`] is the real implementation; with the `fake`
//! feature, the `fake` module provides in-memory ones.

// `expect` is used only for invariants established inside this crate (an
// enumerated candidate is open, a mouse has an active route), never for
// input from devices or callers.
#![allow(clippy::missing_panics_doc)]

mod enumerate;
pub mod hold;
pub mod linux;
pub mod model;
pub mod receiver;
pub mod select;
pub mod sys;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

pub use enumerate::enumerate;
pub use hold::{HoldEvent, HoldOpen};
pub use linux::LinuxSystem;
pub use model::{
    Inventory, Mouse, MouseKey, MouseRoute, NodeEntry, NodeStatus, Receiver, ReceiverKey,
    RejectReason, Route, RouteState, SlotMatch, SlotsRead, Warning,
};
pub use select::{
    ManagementLink, ManagementTarget, SelectError, SendError, SendFailure, Target,
    select_management_node, select_mouse, select_node, select_receiver,
};
pub use sys::{Clock, Device, Errno, NodeInfo, RawInfo, System, SystemClock};
