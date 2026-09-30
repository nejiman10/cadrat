//! Discovery: classify each node and assemble mice and Receivers
//! (spec device §3–§5, spec receiver §1).
//!
//! Enumeration only reads: the ID probe (GET `0x08`) on setting nodes and the
//! slot reports (GET `0x43..0x47`) on one management node per Receiver.

use std::collections::BTreeMap;
use std::io;

use cadrat_proto::report10::WIRE_LEN;
use cadrat_proto::{DeviceId, ReportLengths};

use crate::model::{
    Inventory, ManagementCaps, Mouse, MouseKey, MouseRoute, NodeEntry, NodeIndex, NodeStatus,
    Receiver, ReceiverKey, RejectReason, Route, RouteState, SettingRole, SlotMatch, SlotsRead,
    Warning,
};
use crate::receiver::read_slots;
use crate::sys::{BUS_USB, Device, Errno, NodeInfo, PRODUCT_C652, PRODUCT_C658, System, VENDOR};

const DEVICE_ID_REPORT: u8 = cadrat_proto::device_id::REPORT_ID;
const DEVICE_ID_LEN: usize = cadrat_proto::device_id::RESPONSE_LEN;

/// Finds every mouse and Receiver.
///
/// Nodes whose sysfs vendor is not `256f` are skipped without opening them.
///
/// # Errors
///
/// Only if sysfs cannot be listed; per-node failures are recorded in
/// [`Inventory::nodes`].
pub fn enumerate(system: &dyn System) -> io::Result<Inventory> {
    let mut warnings = Vec::new();
    let nodes: Vec<NodeEntry> = system
        .nodes()?
        .into_iter()
        .filter(|info| matches!(info.hid_id, Some((_, VENDOR, _))))
        .map(|info| classify(system, info, &mut warnings))
        .collect();
    Ok(assemble(nodes, warnings))
}

/// Classifies one node (spec device §4, spec receiver §1).
pub(crate) fn classify(
    system: &dyn System,
    info: NodeInfo,
    warnings: &mut Vec<Warning>,
) -> NodeEntry {
    let reject = |info: NodeInfo, reason| NodeEntry {
        info,
        status: NodeStatus::Rejected(reason),
        device: None,
    };
    let (Some((_, _, product)), Some(_), Some(_)) = (info.hid_id, info.interface, &info.usb_port)
    else {
        return reject(info, RejectReason::SysfsIncomplete);
    };
    let mut device = match system.open(&info.path) {
        Ok(device) => device,
        Err(e) if Errno::of(&e).is_permission() => {
            return NodeEntry {
                info,
                status: NodeStatus::Inaccessible(Errno::of(&e)),
                device: None,
            };
        }
        Err(e) => return reject(info, RejectReason::OpenError(Errno::of(&e))),
    };
    match device.raw_info() {
        Ok(raw) if raw.bus == BUS_USB && raw.vendor == VENDOR && raw.product == product => {}
        Ok(raw) => return reject(info, RejectReason::RawInfoMismatch(raw)),
        Err(e) => return reject(info, RejectReason::RawInfoError(Errno::of(&e))),
    }
    let lengths = match device.descriptor() {
        Ok(descriptor) => match ReportLengths::parse(&descriptor) {
            Ok(lengths) => lengths,
            Err(e) => return reject(info, RejectReason::DescriptorInvalid(e)),
        },
        Err(e) => return reject(info, RejectReason::DescriptorError(Errno::of(&e))),
    };

    let declares_settings = lengths.feature(cadrat_proto::report10::REPORT_ID)
        == Some(WIRE_LEN as u64)
        && lengths.feature(DEVICE_ID_REPORT) == Some(DEVICE_ID_LEN as u64);
    let (setting, management) = match product {
        PRODUCT_C658 => (declares_settings.then_some(SettingRole::Wired), None),
        PRODUCT_C652 => {
            let slots = cadrat_proto::Slot::ALL.iter().all(|slot| {
                lengths.feature(slot.report_id())
                    == Some(cadrat_proto::receiver::SLOT_REPORT_LEN as u64)
            });
            let pairing = lengths.feature(cadrat_proto::receiver::PAIRING_REPORT_ID)
                == Some(cadrat_proto::receiver::PAIRING_REPORT_LEN as u64);
            (
                declares_settings.then_some(SettingRole::Receiver),
                slots.then_some(ManagementCaps { pairing }),
            )
        }
        other => return reject(info, RejectReason::UnsupportedProduct(other)),
    };
    if setting.is_none() && management.is_none() {
        return reject(info, RejectReason::NoFeature10);
    }

    let mut status = NodeStatus::Candidate {
        setting,
        management,
        device_id: None,
        setting_rejected: None,
    };
    if let Some(role) = setting {
        let probe = probe(device.as_mut());
        if let NodeStatus::Candidate {
            setting,
            device_id,
            setting_rejected,
            ..
        } = &mut status
        {
            match (probe, role) {
                (Ok(id), _) => *device_id = Some(id),
                (Err(_), SettingRole::Wired) => {
                    warnings.push(Warning::NoDeviceId(info.path.clone()));
                }
                (Err(reason), SettingRole::Receiver) => {
                    if management.is_none() {
                        return reject(info, reason);
                    }
                    *setting = None;
                    *setting_rejected = Some(reason);
                }
            }
        }
    }
    NodeEntry {
        info,
        status,
        device: Some(device),
    }
}

/// The ID probe: GET Feature `0x08` with length 8 (spec device §4).
pub(crate) fn probe(device: &mut dyn Device) -> Result<DeviceId, RejectReason> {
    let response = device
        .get_feature(DEVICE_ID_REPORT, DEVICE_ID_LEN)
        .map_err(|e| RejectReason::ProbeError(Errno::of(&e)))?;
    DeviceId::from_probe(&response).map_err(RejectReason::ProbeMismatch)
}

fn assemble(mut nodes: Vec<NodeEntry>, mut warnings: Vec<Warning>) -> Inventory {
    let receivers = assemble_receivers(&mut nodes);
    let mice = assemble_mice(&nodes, &receivers, &mut warnings);
    let mut receivers: Vec<Receiver> = receivers.into_values().collect();
    receivers.sort_by_key(|r| r.key.to_string());
    // Drop descriptors nothing will use.
    let used: Vec<NodeIndex> = mice
        .iter()
        .flat_map(|m| m.routes.iter().map(|r| r.node))
        .chain(receivers.iter().flat_map(|r| r.management.iter().copied()))
        .collect();
    for (index, node) in nodes.iter_mut().enumerate() {
        if !used.contains(&index) {
            node.device = None;
        }
    }
    Inventory {
        nodes,
        mice,
        receivers,
        warnings,
    }
}

/// One Receiver per USB port with a usable C652 node; slots are read
/// through the management candidate with the lowest interface.
fn assemble_receivers(nodes: &mut [NodeEntry]) -> BTreeMap<String, Receiver> {
    let mut receivers: BTreeMap<String, Receiver> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let NodeStatus::Candidate { management, .. } = &node.status else {
            continue;
        };
        if node.product() != Some(PRODUCT_C652) {
            continue;
        }
        let port = node
            .info
            .usb_port
            .clone()
            .expect("classified nodes have a port");
        let receiver = receivers.entry(port.clone()).or_insert_with(|| Receiver {
            key: ReceiverKey(port),
            management: Vec::new(),
            slots: SlotsRead::NoManagementNode,
        });
        if management.is_some() {
            receiver.management.push(index);
        }
    }
    for receiver in receivers.values_mut() {
        receiver
            .management
            .sort_by_key(|&i| nodes[i].info.interface);
        if let Some(&first) = receiver.management.first() {
            let device = nodes[first].device.as_mut().expect("candidates are open");
            receiver.slots = match read_slots(device.as_mut()) {
                Ok(slots) => SlotsRead::Read(slots),
                Err(e) => SlotsRead::Failed(e.to_string()),
            };
        }
    }
    receivers
}

/// Setting nodes grouped into mice by key (spec device §2, §5).
fn assemble_mice(
    nodes: &[NodeEntry],
    receivers: &BTreeMap<String, Receiver>,
    warnings: &mut Vec<Warning>,
) -> Vec<Mouse> {
    // Routes, grouped by mouse key.
    let mut routes: BTreeMap<String, (MouseKey, Option<DeviceId>, Vec<MouseRoute>)> =
        BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let NodeStatus::Candidate {
            setting: Some(role),
            device_id,
            ..
        } = &node.status
        else {
            continue;
        };
        let port = node
            .info
            .usb_port
            .clone()
            .expect("classified nodes have a port");
        let interface = node
            .info
            .interface
            .expect("classified nodes have an interface");
        let key = match device_id {
            Some(id) => MouseKey::DeviceId(*id),
            None => MouseKey::Port {
                port: port.clone(),
                interface,
            },
        };
        let (route, receiver, slot) = match role {
            SettingRole::Wired => (Route::Wired, None, None),
            SettingRole::Receiver => {
                let id = device_id.expect("Receiver setting nodes have a device ID");
                let slot = match &receivers[&port].slots {
                    SlotsRead::Read(slots) => slots
                        .iter()
                        .find(|s| s.occupied() && s.id_candidate() == id)
                        .map_or(SlotMatch::Unknown, |s| SlotMatch::Slot(s.slot())),
                    _ => SlotMatch::Unknown,
                };
                if let SlotMatch::Slot(slot) = slot
                    && slot.get() != interface
                {
                    warnings.push(Warning::SlotInterfaceMismatch {
                        mouse: key.clone(),
                        slot,
                        interface,
                    });
                }
                (Route::Receiver, Some(ReceiverKey(port.clone())), Some(slot))
            }
        };
        routes
            .entry(key.to_string())
            .or_insert_with(|| (key, *device_id, Vec::new()))
            .2
            .push(MouseRoute {
                route,
                state: RouteState::Standby,
                node: index,
                path: node.info.path.clone(),
                interface,
                receiver,
                slot,
                ambiguous: false,
            });
    }

    // ambiguous-node: two wired setting nodes on one USB device, or two
    // routes of the same kind for one mouse (spec device §4).
    let wired_ports: Vec<&str> = routes
        .values()
        .flat_map(|(_, _, r)| r)
        .filter(|r| r.route == Route::Wired)
        .map(|r| nodes[r.node].info.usb_port.as_deref().unwrap_or_default())
        .collect();
    let mut mice: Vec<Mouse> = routes
        .into_values()
        .map(|(key, device_id, mut routes)| {
            routes.sort_by_key(|r| (r.route, r.interface));
            for i in 0..routes.len() {
                let same_kind = routes.iter().filter(|r| r.route == routes[i].route).count();
                let port = nodes[routes[i].node]
                    .info
                    .usb_port
                    .as_deref()
                    .unwrap_or_default();
                let shared_port = routes[i].route == Route::Wired
                    && wired_ports.iter().filter(|p| **p == port).count() > 1;
                routes[i].ambiguous = same_kind > 1 || shared_port;
            }
            routes[0].state = RouteState::Active;
            Mouse {
                key,
                device_id,
                routes,
            }
        })
        .collect();
    mice.sort_by_key(|m| m.key.to_string());
    mice
}
