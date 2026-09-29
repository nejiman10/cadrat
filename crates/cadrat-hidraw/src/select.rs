//! Choosing the mouse or Receiver to act on, and sending (spec device §6, §7,
//! spec receiver §1).

use std::path::{Path, PathBuf};

use cadrat_proto::DeviceId;
use cadrat_proto::report10::WIRE_LEN;

use crate::enumerate::{classify, probe};
use crate::model::{
    Inventory, Mouse, MouseKey, MouseRoute, NodeIndex, NodeStatus, Receiver, RejectReason, Route,
    RouteState, SettingRole, Warning,
};
use crate::sys::{Device, Errno, PRODUCT_C652, System, VENDOR};

/// Why no single target could be chosen. Each maps to an exit code
/// (spec tool/cli §6).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SelectError {
    /// `NoDevice` (4).
    #[error("no matching device is connected")]
    NoDevice,
    /// `AmbiguousTarget` (5): the candidates' keys.
    #[error("more than one device matches: {}", .0.join(", "))]
    Ambiguous(Vec<String>),
    /// `PermissionDenied` (6): nodes that could not be opened.
    #[error(
        "cannot open {}; install udev/69-cadrat.rules and reconnect the device",
        .0.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    )]
    PermissionDenied(Vec<PathBuf>),
    /// `DeviceInvalid` (7).
    #[error("{0}")]
    DeviceInvalid(String),
}

/// A mouse route chosen for sending, with its open descriptor.
#[derive(Debug)]
pub struct Target {
    /// The mouse. For `--hidraw`, a mouse with only the given route.
    pub mouse: Mouse,
    /// The route to send over.
    pub route: MouseRoute,
    /// Warnings from selection.
    pub warnings: Vec<Warning>,
    device: Box<dyn Device>,
}

fn inaccessible(inventory: &Inventory, product: Option<u16>) -> Vec<PathBuf> {
    inventory
        .inaccessible()
        .filter(|n| product.is_none() || n.product() == product)
        .map(|n| n.info.path.clone())
        .collect()
}

/// Resolves a selector against a list of keys (spec device §6): a 1-based
/// number, a key, or a unique key prefix.
fn resolve(
    keys: &[String],
    selector: Option<&str>,
    blocked: &[PathBuf],
) -> Result<usize, SelectError> {
    let denied = || SelectError::PermissionDenied(blocked.to_vec());
    let Some(selector) = selector else {
        // Never guess (P4): a node we cannot open may be another device.
        return if keys.len() > 1 {
            Err(SelectError::Ambiguous(keys.to_vec()))
        } else if !blocked.is_empty() {
            Err(denied())
        } else if keys.is_empty() {
            Err(SelectError::NoDevice)
        } else {
            Ok(0)
        };
    };
    if !selector.is_empty() && selector.bytes().all(|b| b.is_ascii_digit()) {
        return selector
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=keys.len()).contains(n))
            .map(|n| n - 1)
            .ok_or(SelectError::NoDevice);
    }
    if let Some(i) = keys.iter().position(|k| k == selector) {
        return Ok(i);
    }
    let matches: Vec<usize> = (0..keys.len())
        .filter(|&i| keys[i].starts_with(selector))
        .collect();
    match matches.as_slice() {
        [i] => Ok(*i),
        [] if !blocked.is_empty() => Err(denied()),
        [] => Err(SelectError::NoDevice),
        _ => Err(SelectError::Ambiguous(
            matches.iter().map(|&i| keys[i].clone()).collect(),
        )),
    }
}

/// Chooses the mouse and route for a send (`--mouse`, `--route`).
///
/// # Errors
///
/// See [`SelectError`].
pub fn select_mouse(
    inventory: &mut Inventory,
    selector: Option<&str>,
    route: Option<Route>,
) -> Result<Target, SelectError> {
    let keys: Vec<String> = inventory.mice.iter().map(|m| m.key.to_string()).collect();
    let index = resolve(&keys, selector, &inaccessible(inventory, None))?;
    let mouse = inventory.mice[index].clone();
    let mut warnings = Vec::new();
    let chosen = match route {
        None => mouse.active_route().clone(),
        Some(route) => {
            let chosen = mouse.route(route).ok_or(SelectError::NoDevice)?.clone();
            if chosen.state == RouteState::Standby {
                warnings.push(Warning::InactiveRoute(route));
            }
            chosen
        }
    };
    if chosen.ambiguous {
        return Err(SelectError::DeviceInvalid(format!(
            "{} has more than one {} setting node (ambiguous-node)",
            mouse.key, chosen.route
        )));
    }
    let device = inventory
        .take_device(chosen.node)
        .expect("setting nodes stay open");
    Ok(Target {
        mouse,
        route: chosen,
        warnings,
        device,
    })
}

/// Uses one node directly (`--hidraw`). Classification and the ID probe
/// still apply (spec device §6).
///
/// # Errors
///
/// `PermissionDenied` if the node cannot be opened, `DeviceInvalid` if it
/// is not a setting node.
pub fn select_node(system: &dyn System, path: &Path) -> Result<Target, SelectError> {
    let info = system
        .node(path)
        .map_err(|e| SelectError::DeviceInvalid(format!("{}: {e}", path.display())))?;
    if !matches!(info.hid_id, Some((_, VENDOR, _))) {
        return Err(SelectError::DeviceInvalid(format!(
            "{}: not a 3Dconnexion device",
            path.display()
        )));
    }
    let mut warnings = Vec::new();
    let mut entry = classify(system, info, &mut warnings);
    let invalid = |reason: &dyn std::fmt::Display| {
        SelectError::DeviceInvalid(format!("{}: {reason}", path.display()))
    };
    let (role, device_id) = match &entry.status {
        NodeStatus::Inaccessible(_) => {
            return Err(SelectError::PermissionDenied(vec![path.to_owned()]));
        }
        NodeStatus::Candidate {
            setting: Some(role),
            device_id,
            ..
        } => (*role, *device_id),
        NodeStatus::Rejected(reason)
        | NodeStatus::Candidate {
            setting_rejected: Some(reason),
            ..
        } => return Err(invalid(reason)),
        NodeStatus::Candidate { .. } => return Err(invalid(&"not a setting node")),
    };
    let port = entry.info.usb_port.clone().unwrap_or_default();
    let interface = entry.info.interface.unwrap_or_default();
    let key = device_id.map_or_else(
        || MouseKey::Port {
            port: port.clone(),
            interface,
        },
        MouseKey::DeviceId,
    );
    let route = MouseRoute {
        route: match role {
            SettingRole::Wired => Route::Wired,
            SettingRole::Receiver => Route::Receiver,
        },
        state: RouteState::Active,
        node: 0,
        path: path.to_owned(),
        interface,
        receiver: (role == SettingRole::Receiver).then_some(crate::model::ReceiverKey(port)),
        slot: None,
        ambiguous: false,
    };
    Ok(Target {
        mouse: Mouse {
            key,
            device_id,
            routes: vec![route.clone()],
        },
        route,
        warnings,
        device: entry.device.take().expect("candidates are open"),
    })
}

/// Why a send did not complete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// `TargetChanged` (19): the node no longer answers as the chosen mouse.
    /// Nothing was sent.
    #[error("the device at {path} is no longer the selected mouse ({reason}); nothing was sent")]
    TargetChanged {
        /// The node.
        path: PathBuf,
        /// What differed.
        reason: String,
    },
    /// `SendFailed` (8): the check or the send failed.
    #[error("sending to {path} failed: {failure}")]
    Failed {
        /// The node.
        path: PathBuf,
        /// The failure.
        failure: SendFailure,
    },
}

/// How a send failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailure {
    /// The ID check before sending failed with an errno; nothing was sent.
    CheckErrno(Errno),
    /// `short-write: <n>`.
    ShortWrite(usize),
    /// `errno: <name>`.
    Errno(Errno),
}

impl SendFailure {
    /// Whether the failure means the device was disconnected.
    #[must_use]
    pub fn is_device_gone(self) -> bool {
        matches!(self, Self::CheckErrno(e) | Self::Errno(e) if e.is_device_gone())
    }
}

impl std::fmt::Display for SendFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_device_gone() {
            f.write_str("device disconnected; ")?;
        }
        match self {
            Self::CheckErrno(errno) => write!(f, "device check before sending: errno: {errno}"),
            Self::ShortWrite(n) => write!(f, "short-write: {n}"),
            Self::Errno(errno) => write!(f, "errno: {errno}"),
        }
    }
}

impl std::error::Error for SendFailure {}

impl Target {
    /// The device ID the pre-send check expects, if the key has one.
    #[must_use]
    pub fn expected_id(&self) -> Option<DeviceId> {
        self.mouse.device_id
    }

    /// Sends one wire report (spec device §7): re-probes on the same descriptor,
    /// then sends exactly once. Success means the host completed the ioctl,
    /// not that the mouse applied it (P6).
    ///
    /// # Errors
    ///
    /// See [`SendError`].
    pub fn send(&mut self, wire: &[u8; WIRE_LEN]) -> Result<(), SendError> {
        let path = self.route.path.clone();
        match probe(self.device.as_mut()) {
            Ok(id) => {
                if let Some(expected) = self.expected_id()
                    && id != expected
                {
                    return Err(SendError::TargetChanged {
                        path,
                        reason: "device ID differs".to_owned(),
                    });
                }
            }
            Err(RejectReason::ProbeError(errno)) => {
                return Err(SendError::Failed {
                    path,
                    failure: SendFailure::CheckErrno(errno),
                });
            }
            Err(reason) => {
                return Err(SendError::TargetChanged {
                    path,
                    reason: reason.to_string(),
                });
            }
        }
        match self.device.set_feature(wire) {
            Ok(WIRE_LEN) => Ok(()),
            Ok(n) => Err(SendError::Failed {
                path,
                failure: SendFailure::ShortWrite(n),
            }),
            Err(e) => Err(SendError::Failed {
                path,
                failure: SendFailure::Errno(Errno::of(&e)),
            }),
        }
    }
}

/// A Receiver management node chosen for `receiver` commands, with its open
/// descriptor.
#[derive(Debug)]
pub struct ManagementTarget {
    /// The Receiver. For `--hidraw`, only its key is meaningful.
    pub receiver: Receiver,
    /// The management node's path.
    pub path: PathBuf,
    /// The management node's interface.
    pub interface: u8,
    device: Box<dyn Device>,
}

impl ManagementTarget {
    /// The open management node.
    pub fn device(&mut self) -> &mut dyn Device {
        self.device.as_mut()
    }

    /// A [`Link`](crate::receiver::Link) that reopens this Receiver's
    /// management node when it disappears: it enumerates again and chooses
    /// by the same rules (spec receiver §1), keyed by the Receiver's USB port.
    pub fn link<'a>(
        &'a mut self,
        system: &'a dyn System,
        require_pairing: bool,
    ) -> ManagementLink<'a> {
        ManagementLink {
            target: self,
            system,
            require_pairing,
        }
    }
}

/// See [`ManagementTarget::link`].
pub struct ManagementLink<'a> {
    target: &'a mut ManagementTarget,
    system: &'a dyn System,
    require_pairing: bool,
}

impl crate::receiver::Link for ManagementLink<'_> {
    fn device(&mut self) -> &mut dyn Device {
        self.target.device.as_mut()
    }

    fn reopen(&mut self) -> Result<(), String> {
        let mut inventory = crate::enumerate(self.system).map_err(|e| e.to_string())?;
        let key = self.target.receiver.key.to_string();
        let fresh = select_receiver(&mut inventory, Some(&key), self.require_pairing)
            .map_err(|e| e.to_string())?;
        if fresh.receiver.key != self.target.receiver.key {
            return Err(format!("{key} is no longer connected"));
        }
        self.target.path = fresh.path;
        self.target.interface = fresh.interface;
        self.target.device = fresh.device;
        Ok(())
    }
}

/// Chooses the Receiver (`--receiver`) and its management node: the
/// candidate with the lowest interface number (spec receiver §1, Q10).
///
/// `require_pairing` also requires Feature `0x41` (pair and unpair).
///
/// # Errors
///
/// See [`SelectError`]; `DeviceInvalid` if the Receiver has no qualifying
/// management node.
pub fn select_receiver(
    inventory: &mut Inventory,
    selector: Option<&str>,
    require_pairing: bool,
) -> Result<ManagementTarget, SelectError> {
    let keys: Vec<String> = inventory
        .receivers
        .iter()
        .map(|r| r.key.to_string())
        .collect();
    let blocked = inaccessible(inventory, Some(PRODUCT_C652));
    // Receiver selectors are keys or prefixes; `list` does not number them.
    if selector.is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit())) {
        return Err(SelectError::NoDevice);
    }
    let index = resolve(&keys, selector, &blocked)?;
    let receiver = inventory.receivers[index].clone();
    let node: NodeIndex = receiver
        .management
        .iter()
        .copied()
        .find(|&i| {
            matches!(
                inventory.nodes[i].status,
                NodeStatus::Candidate { management: Some(caps), .. } if caps.pairing || !require_pairing
            )
        })
        .ok_or_else(|| {
            SelectError::DeviceInvalid(format!("{} has no management node", receiver.key))
        })?;
    let path = inventory.nodes[node].info.path.clone();
    let interface = inventory.nodes[node].info.interface.unwrap_or_default();
    let device = inventory
        .take_device(node)
        .expect("management nodes stay open");
    Ok(ManagementTarget {
        receiver,
        path,
        interface,
        device,
    })
}

/// Uses one management node directly (`--hidraw`), checked against the
/// same conditions (spec receiver §1).
///
/// # Errors
///
/// `PermissionDenied` or `DeviceInvalid`.
pub fn select_management_node(
    system: &dyn System,
    path: &Path,
    require_pairing: bool,
) -> Result<ManagementTarget, SelectError> {
    let info = system
        .node(path)
        .map_err(|e| SelectError::DeviceInvalid(format!("{}: {e}", path.display())))?;
    if !matches!(info.hid_id, Some((_, VENDOR, PRODUCT_C652))) {
        return Err(SelectError::DeviceInvalid(format!(
            "{}: not a C652 Receiver",
            path.display()
        )));
    }
    let mut entry = classify(system, info, &mut Vec::new());
    let invalid =
        |reason: &str| SelectError::DeviceInvalid(format!("{}: {reason}", path.display()));
    match &entry.status {
        NodeStatus::Inaccessible(_) => {
            return Err(SelectError::PermissionDenied(vec![path.to_owned()]));
        }
        NodeStatus::Rejected(reason) => return Err(invalid(&reason.to_string())),
        NodeStatus::Candidate {
            management: Some(caps),
            ..
        } if caps.pairing || !require_pairing => {}
        NodeStatus::Candidate { .. } => return Err(invalid("not a management node")),
    }
    let port = entry.info.usb_port.clone().unwrap_or_default();
    Ok(ManagementTarget {
        receiver: Receiver {
            key: crate::model::ReceiverKey(port),
            management: Vec::new(),
            slots: crate::model::SlotsRead::NoManagementNode,
        },
        path: path.to_owned(),
        interface: entry.info.interface.unwrap_or_default(),
        device: entry.device.take().expect("candidates are open"),
    })
}
