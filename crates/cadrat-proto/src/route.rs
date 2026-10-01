//! The two ways a C658 is reached (spec device §2).
//!
//! The device model lives in `cadrat-hidraw`; this name is here, with no
//! I/O, so that front ends that never touch devices (`cadratctl`) can name
//! a route without depending on the transport.

use core::fmt;

/// `wired` (C658 directly) or `receiver` (through C652).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Route {
    /// C658 connected by cable.
    Wired,
    /// C658 through a C652 Receiver.
    Receiver,
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wired => "wired",
            Self::Receiver => "receiver",
        })
    }
}
