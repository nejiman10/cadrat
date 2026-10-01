//! I/O-free protocol core for the C658 mouse and the C652 Universal
//! Receiver.
//!
//! This crate builds and inspects Feature Report `0x10`, measures report
//! lengths from HID report descriptors, parses Input Report `0x03`, builds the
//! Receiver management packets and parses slot and device-ID responses.
//!
//! It performs no I/O and depends only on `core`, so `cadrat-tool` and the
//! later daemon share exactly the same encoding rules.
//!
//! Protocol facts follow `SPEC.md` of `nejiman10/3dx-hid-research` at the
//! commit cited in `docs/spec/README.md`. Evidence labels (`CONFIRMED`,
//! `OBSERVED`, `HYPOTHESIS`, `UNKNOWN`) are kept in the item documentation.

#![cfg_attr(not(test), no_std)]

pub mod action;
pub mod descriptor;
pub mod device_id;
mod int;
pub mod receiver;
pub mod report03;
pub mod report10;
mod route;

pub use action::{Action, DirectAction, HostIndex, ParseActionError, RawWire};
pub use descriptor::{DescriptorError, ReportLengths};
pub use device_id::{DeviceId, DeviceIdError};
pub use int::parse_u32;
pub use receiver::{Polling, Slot, SlotError, SlotReport};
pub use report03::{Report03Error, Report03Frame};
pub use report10::{
    ButtonName, Buttons, Dpi, InspectError, InspectedReport10, Lift, PollingRate, Report10Config,
    WheelMode, WireButton,
};
pub use route::Route;
