//! The device model: nodes, mice with their routes, and Receivers
//! (spec 02 §2).

use std::fmt;
use std::path::PathBuf;

use cadrat_proto::{DeviceId, Slot, SlotReport};

use crate::sys::{Device, Errno, NodeInfo, RawInfo};

/// Index of a node in [`Inventory::nodes`].
pub type NodeIndex = usize;

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

/// Whether a route is the one settings go to by default (spec 02 §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteState {
    /// The active route.
    Active,
    /// Present but not active.
    Standby,
}

impl fmt::Display for RouteState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Active => "active",
            Self::Standby => "standby",
        })
    }
}

/// Why a node is not usable (shown by `list --nodes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// sysfs lacks the HID ID, interface number or USB port.
    SysfsIncomplete,
    /// `open` failed with something other than a permission error.
    OpenError(Errno),
    /// `HIDIOCGRAWINFO` failed.
    RawInfoError(Errno),
    /// `HIDIOCGRAWINFO` disagrees with sysfs, or is not USB `256f`.
    RawInfoMismatch(RawInfo),
    /// The descriptor could not be read.
    DescriptorError(Errno),
    /// The descriptor could not be parsed.
    DescriptorInvalid(cadrat_proto::DescriptorError),
    /// A product other than C658 or C652.
    UnsupportedProduct(u16),
    /// Neither a setting node nor a management node.
    NoFeature10,
    /// The ID probe response did not match (Receiver setting nodes only).
    ProbeMismatch(cadrat_proto::DeviceIdError),
    /// The ID probe failed (Receiver setting nodes only).
    ProbeError(Errno),
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SysfsIncomplete => f.write_str("sysfs-incomplete"),
            Self::OpenError(errno) => write!(f, "open-error: {errno}"),
            Self::RawInfoError(errno) => write!(f, "rawinfo-error: {errno}"),
            Self::RawInfoMismatch(info) => write!(
                f,
                "rawinfo-mismatch: bus {:04x} {:04x}:{:04x}",
                info.bus, info.vendor, info.product
            ),
            Self::DescriptorError(errno) => write!(f, "descriptor-error: {errno}"),
            Self::DescriptorInvalid(error) => write!(f, "descriptor-invalid: {error}"),
            Self::UnsupportedProduct(product) => {
                write!(f, "unsupported-product: 256f:{product:04x}")
            }
            Self::NoFeature10 => f.write_str("no-feature-0x10"),
            // DeviceIdError already reads "probe-mismatch: 08 00 …".
            Self::ProbeMismatch(error) => error.fmt(f),
            Self::ProbeError(errno) => write!(f, "probe-error: {errno}"),
        }
    }
}

/// The setting role of a node (spec 02 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRole {
    /// Wired C658 setting node.
    Wired,
    /// C652 setting node for one paired mouse.
    Receiver,
}

/// The result of classifying one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    /// Usable in at least one role.
    Candidate {
        /// Setting role, if the node carries Report `0x10`.
        setting: Option<SettingRole>,
        /// Whether the node qualifies as a Receiver management node.
        management: Option<ManagementCaps>,
        /// Device ID from the probe, for setting nodes.
        device_id: Option<DeviceId>,
        /// Why the setting role was dropped while management remained.
        setting_rejected: Option<RejectReason>,
    },
    /// Not usable.
    Rejected(RejectReason),
    /// Could not be opened for lack of permission.
    Inaccessible(Errno),
}

/// What a management node declares (spec 05 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagementCaps {
    /// Feature `0x41` is declared with 5 bytes, so it can pair and unpair.
    pub pairing: bool,
}

/// One hidraw node and what was learned about it.
#[derive(Debug)]
pub struct NodeEntry {
    /// sysfs information.
    pub info: NodeInfo,
    /// Classification.
    pub status: NodeStatus,
    /// The open descriptor, kept for the send (spec 02 §7 step 1).
    pub(crate) device: Option<Box<dyn Device>>,
}

impl NodeEntry {
    /// The node's product ID from sysfs.
    #[must_use]
    pub fn product(&self) -> Option<u16> {
        self.info.hid_id.map(|(_, _, product)| product)
    }
}

/// Stable key of a mouse (spec 02 §2.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseKey {
    /// `c658:<device ID>`.
    DeviceId(DeviceId),
    /// `c658-port:<USB port>/if<N>`, when the wired probe failed.
    Port {
        /// USB port path.
        port: String,
        /// Interface number.
        interface: u8,
    },
}

impl fmt::Display for MouseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceId(id) => write!(f, "c658:{id}"),
            Self::Port { port, interface } => write!(f, "c658-port:{port}/if{interface}"),
        }
    }
}

/// Stable key of a Receiver: `recv:port-<USB port>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReceiverKey(pub String);

impl fmt::Display for ReceiverKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "recv:port-{}", self.0)
    }
}

/// How a Receiver route was matched to a slot (spec 02 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotMatch {
    /// The slot whose identifier equals the device ID.
    Slot(Slot),
    /// No slot matched, or the slots could not be read.
    Unknown,
}

/// One route of a mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseRoute {
    /// Wired or Receiver.
    pub route: Route,
    /// Active or standby.
    pub state: RouteState,
    /// The setting node.
    pub node: NodeIndex,
    /// Device path of the setting node.
    pub path: PathBuf,
    /// Interface number of the setting node.
    pub interface: u8,
    /// The Receiver, for Receiver routes.
    pub receiver: Option<ReceiverKey>,
    /// The slot, for Receiver routes.
    pub slot: Option<SlotMatch>,
    /// Another setting node claims the same mouse on this route, so sending
    /// is refused (`ambiguous-node`).
    pub ambiguous: bool,
}

/// A mouse, seen over one or two routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mouse {
    /// Stable key.
    pub key: MouseKey,
    /// Device ID, absent for port keys.
    pub device_id: Option<DeviceId>,
    /// Routes, wired first.
    pub routes: Vec<MouseRoute>,
}

impl Mouse {
    /// The active route.
    #[must_use]
    pub fn active_route(&self) -> &MouseRoute {
        self.routes
            .iter()
            .find(|r| r.state == RouteState::Active)
            .expect("every mouse has an active route")
    }

    /// The route of the given kind, if present.
    #[must_use]
    pub fn route(&self, route: Route) -> Option<&MouseRoute> {
        self.routes.iter().find(|r| r.route == route)
    }

    /// Whether any route is ambiguous.
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.routes.iter().any(|r| r.ambiguous)
    }
}

/// Slots of a Receiver as read during enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotsRead {
    /// All five slots.
    Read([SlotReport; 5]),
    /// No management node declares the slot reports.
    NoManagementNode,
    /// A GET failed or returned a malformed report.
    Failed(String),
}

/// A C652 Receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receiver {
    /// Stable key.
    pub key: ReceiverKey,
    /// Management candidates, by ascending interface number.
    pub management: Vec<NodeIndex>,
    /// Slots read through the first management candidate.
    pub slots: SlotsRead,
}

/// Warnings found while enumerating or selecting (spec 03 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// `W-NO-DEVICE-ID`: a wired setting node without a device ID.
    NoDeviceId(PathBuf),
    /// `W-SLOT-IF-MISMATCH`: a Receiver setting node on an interface other
    /// than its slot number.
    SlotInterfaceMismatch {
        /// The mouse.
        mouse: MouseKey,
        /// Its slot.
        slot: Slot,
        /// The setting node's interface.
        interface: u8,
    },
    /// `W-INACTIVE-ROUTE`: `--route` chose a standby route.
    InactiveRoute(Route),
    /// `W-PAIR-MULTIPLE`: several slots became occupied while pairing.
    PairMultiple(Vec<Slot>),
    /// `W-UNPAIR-EPIPE`: the unpair SET returned `EPIPE`.
    UnpairEpipe,
    /// `W-MANAGEMENT-REOPENED`: the management node disappeared while
    /// waiting and was opened again.
    ManagementReopened {
        /// How many times it was reopened.
        count: usize,
        /// The last failed attempt, if any.
        last_failure: Option<String>,
    },
    /// `W-SLOT-READ-RETRY`: slot reads failed while waiting and were retried.
    SlotReadRetried {
        /// How many reads failed.
        count: usize,
        /// The last failure.
        last: String,
    },
}

impl Warning {
    /// The warning code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoDeviceId(_) => "W-NO-DEVICE-ID",
            Self::SlotInterfaceMismatch { .. } => "W-SLOT-IF-MISMATCH",
            Self::InactiveRoute(_) => "W-INACTIVE-ROUTE",
            Self::PairMultiple(_) => "W-PAIR-MULTIPLE",
            Self::UnpairEpipe => "W-UNPAIR-EPIPE",
            Self::SlotReadRetried { .. } => "W-SLOT-READ-RETRY",
            Self::ManagementReopened { .. } => "W-MANAGEMENT-REOPENED",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDeviceId(path) => write!(
                f,
                "{} did not return a device ID; using its USB port as the key",
                path.display()
            ),
            Self::SlotInterfaceMismatch {
                mouse,
                slot,
                interface,
            } => write!(
                f,
                "{mouse} is in slot {slot} but its setting node is interface {interface}; this was never observed"
            ),
            Self::InactiveRoute(route) => write!(
                f,
                "the {route} route is on standby; the effect of sending to it is unverified"
            ),
            Self::PairMultiple(slots) => {
                let slots: Vec<String> = slots.iter().map(ToString::to_string).collect();
                write!(f, "several slots became occupied: {}", slots.join(", "))
            }
            Self::UnpairEpipe => {
                f.write_str("the unpair request returned EPIPE; checking whether the slot empties")
            }
            Self::ManagementReopened {
                count,
                last_failure,
            } => {
                write!(
                    f,
                    "the management node disappeared while waiting; reopened it {count} time(s)"
                )?;
                match last_failure {
                    Some(failure) => write!(f, " (last failed attempt: {failure})"),
                    None => Ok(()),
                }
            }
            Self::SlotReadRetried { count, last } => write!(
                f,
                "{count} slot read(s) failed while waiting and were retried (last: {last})"
            ),
        }
    }
}

/// Everything found by [`crate::enumerate`].
#[derive(Debug)]
pub struct Inventory {
    /// Every node with vendor `256f`, in sysfs order.
    pub nodes: Vec<NodeEntry>,
    /// Mice, sorted by key; `list` numbers them from 1 in this order.
    pub mice: Vec<Mouse>,
    /// Receivers, sorted by key.
    pub receivers: Vec<Receiver>,
    /// Warnings from enumeration.
    pub warnings: Vec<Warning>,
}

impl Inventory {
    /// Nodes that could not be opened for lack of permission.
    pub fn inaccessible(&self) -> impl Iterator<Item = &NodeEntry> {
        self.nodes
            .iter()
            .filter(|n| matches!(n.status, NodeStatus::Inaccessible(_)))
    }

    /// Takes the open device of a node out of the inventory.
    pub(crate) fn take_device(&mut self, node: NodeIndex) -> Option<Box<dyn Device>> {
        self.nodes.get_mut(node)?.device.take()
    }
}
