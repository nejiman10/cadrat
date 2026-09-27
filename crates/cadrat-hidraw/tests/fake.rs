//! Discovery, selection, sending and Receiver management against fake
//! sysfs, descriptors and ioctl replies (spec 04 §4).

#![allow(missing_docs)]

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use cadrat_hidraw::fake::{
    FakeClock, FakeNode, FakeSystem, Request, probe_response, slot_response,
};
use cadrat_hidraw::receiver::{self, PairResult, Polling, SetResult, UnpairResult};
use cadrat_hidraw::{
    Inventory, NodeStatus, Route, RouteState, SelectError, SendError, SendFailure, SlotMatch,
    SlotsRead, System, enumerate, select_management_node, select_mouse, select_node,
    select_receiver,
};
use cadrat_proto::{Report10Config, Slot};

// Synthetic device IDs; not taken from any device.
const A: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];
const B: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

const EACCES: i32 = 13;
const EPIPE: i32 = 32;
const ENODEV: i32 = 19;
const EIO: i32 = 5;

fn inventory(nodes: Vec<FakeNode>) -> Inventory {
    enumerate(&FakeSystem::new(nodes)).unwrap()
}

fn keys(inventory: &Inventory) -> Vec<String> {
    inventory.mice.iter().map(|m| m.key.to_string()).collect()
}

fn warnings(inventory: &Inventory) -> Vec<&'static str> {
    inventory
        .warnings
        .iter()
        .map(cadrat_hidraw::Warning::code)
        .collect()
}

fn wire() -> [u8; 32] {
    Report10Config::research_baseline().to_wire()
}

fn empty_slots() -> [&'static [Option<[u8; 6]>]; 5] {
    [&[None], &[None], &[None], &[None], &[None]]
}

/// A Receiver on `port` whose `MI_00` manages it and whose `MI_03` carries mouse A
/// in slot 3.
fn receiver_with_a(port: &str) -> Vec<FakeNode> {
    vec![
        FakeNode::receiver("hidraw6", port, 0, None, true).slots([
            &[None],
            &[None],
            &[None],
            &[Some(A)],
            &[None],
        ]),
        FakeNode::receiver("hidraw9", port, 3, Some(A), true),
    ]
}

// --- enumeration and selection ---

#[test]
fn no_devices() {
    let mut inv = inventory(vec![]);
    assert!(inv.mice.is_empty() && inv.receivers.is_empty());
    assert_eq!(
        select_mouse(&mut inv, None, None).unwrap_err(),
        SelectError::NoDevice
    );
}

#[test]
fn one_wired_mouse() {
    let mut inv = inventory(vec![FakeNode::wired("hidraw5", "3-2", A)]);
    assert_eq!(keys(&inv), ["c658:0a1b2c3d4e5f"]);
    let mouse = &inv.mice[0];
    assert_eq!(mouse.active_route().route, Route::Wired);
    assert_eq!(mouse.routes.len(), 1);
    let target = select_mouse(&mut inv, None, None).unwrap();
    assert_eq!(target.route.path, Path::new("/dev/hidraw5"));
    assert!(target.warnings.is_empty());
}

#[test]
fn several_mice_need_a_selector() {
    let nodes = || {
        vec![
            FakeNode::wired("hidraw5", "3-2", B),
            FakeNode::wired("hidraw7", "3-3", A),
        ]
    };
    let mut inv = inventory(nodes());
    // Sorted by key, whatever the sysfs order.
    assert_eq!(keys(&inv), ["c658:0a1b2c3d4e5f", "c658:112233445566"]);
    assert_eq!(
        select_mouse(&mut inv, None, None).unwrap_err(),
        SelectError::Ambiguous(keys(&inv))
    );
    let target = select_mouse(&mut inv, Some("2"), None).unwrap();
    assert_eq!(target.mouse.key.to_string(), "c658:112233445566");

    let mut inv = inventory(nodes());
    assert_eq!(
        select_mouse(&mut inv, Some("3"), None).unwrap_err(),
        SelectError::NoDevice
    );
    assert_eq!(
        select_mouse(&mut inv, Some("0"), None).unwrap_err(),
        SelectError::NoDevice
    );
    assert!(matches!(
        select_mouse(&mut inv, Some("c658:"), None).unwrap_err(),
        SelectError::Ambiguous(k) if k.len() == 2
    ));
    assert_eq!(
        select_mouse(&mut inv, Some("c658:ff"), None).unwrap_err(),
        SelectError::NoDevice
    );
    let target = select_mouse(&mut inv, Some("c658:0a"), None).unwrap();
    assert_eq!(target.route.path, Path::new("/dev/hidraw7"));

    let mut inv = inventory(nodes());
    let target = select_mouse(&mut inv, Some("c658:112233445566"), None).unwrap();
    assert_eq!(target.route.path, Path::new("/dev/hidraw5"));
}

#[test]
fn other_vendors_are_not_opened() {
    let mut other = FakeNode::wired("hidraw0", "1-1", B);
    other.info.hid_id = Some((3, 0x046d, 0xc077));
    other.open_error = Some(EACCES);
    let inv = inventory(vec![other, FakeNode::wired("hidraw5", "3-2", A)]);
    assert_eq!(inv.nodes.len(), 1);
    assert_eq!(keys(&inv), ["c658:0a1b2c3d4e5f"]);
}

#[test]
fn permission_denied_is_not_ignored() {
    let mut blocked = FakeNode::wired("hidraw5", "3-2", A);
    blocked.open_error = Some(EACCES);
    let mut inv = inventory(vec![blocked.clone()]);
    assert!(matches!(inv.nodes[0].status, NodeStatus::Inaccessible(_)));
    let err = select_mouse(&mut inv, None, None).unwrap_err();
    assert_eq!(
        err,
        SelectError::PermissionDenied(vec!["/dev/hidraw5".into()])
    );
    assert!(err.to_string().contains("udev/69-cadrat.rules"));

    // Another usable mouse does not make the choice safe.
    let mut inv = inventory(vec![blocked.clone(), FakeNode::wired("hidraw7", "3-3", B)]);
    assert!(matches!(
        select_mouse(&mut inv, None, None).unwrap_err(),
        SelectError::PermissionDenied(_)
    ));
    // An explicit key still works, and an unknown one reports the permission problem.
    let mut inv = inventory(vec![blocked.clone(), FakeNode::wired("hidraw7", "3-3", B)]);
    assert!(select_mouse(&mut inv, Some("c658:1122"), None).is_ok());
    let mut inv = inventory(vec![blocked, FakeNode::wired("hidraw7", "3-3", B)]);
    assert!(matches!(
        select_mouse(&mut inv, Some("c658:0a"), None).unwrap_err(),
        SelectError::PermissionDenied(_)
    ));
}

#[test]
fn rejected_nodes_carry_reasons() {
    let no_feature = FakeNode::new("hidraw4", 0xc658, "3-2", 0, &[(0x03, 2)]);
    let wrong_rawinfo = {
        let mut n = FakeNode::wired("hidraw6", "3-4", B);
        n.raw_info.product = 0xc652;
        n
    };
    let other_product = FakeNode::new("hidraw8", 0xc62e, "3-5", 0, &[(0x10, 32)]);
    let open_failed = {
        let mut n = FakeNode::wired("hidraw9", "3-6", B);
        n.open_error = Some(EIO);
        n
    };
    let incomplete = {
        let mut n = FakeNode::wired("hidraw10", "3-7", B);
        n.info.usb_port = None;
        n
    };
    let inv = inventory(vec![
        no_feature,
        wrong_rawinfo,
        other_product,
        open_failed,
        incomplete,
    ]);
    let reasons: Vec<String> = inv
        .nodes
        .iter()
        .map(|n| match &n.status {
            NodeStatus::Rejected(reason) => reason.to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        reasons,
        [
            "no-feature-0x10",
            "rawinfo-mismatch: bus 0003 256f:c652",
            "unsupported-product: 256f:c62e",
            "open-error: EIO",
            "sysfs-incomplete",
        ]
    );
    assert!(inv.mice.is_empty());
}

#[test]
fn wired_probe_failure_uses_port_key() {
    let mismatch = FakeNode::wired("hidraw5", "3-2", A).get(
        0x08,
        &[
            Ok(vec![0x08, 0x00, 1, 2, 3, 4, 5, 6]),
            Ok(probe_response(B)),
        ],
    );
    let mut nodes = vec![mismatch.clone()];
    nodes.extend(receiver_with_a("1-4"));
    let mut inv = inventory(nodes);
    // The port key is never merged with the Receiver route of the same mouse.
    assert_eq!(keys(&inv), ["c658-port:3-2/if1", "c658:0a1b2c3d4e5f"]);
    assert_eq!(warnings(&inv), ["W-NO-DEVICE-ID"]);
    let mut target = select_mouse(&mut inv, Some("c658-port"), None).unwrap();
    assert_eq!(target.expected_id(), None);
    // Without a device ID, the check before sending only requires 08 59.
    target.send(&wire()).unwrap();
    assert_eq!(mismatch.sets(), [wire().to_vec()]);
}

#[test]
fn receiver_probe_failure_rejects_node() {
    let setting_only = FakeNode::receiver("hidraw9", "1-4", 3, Some(A), false)
        .get(0x08, &[Ok(vec![0x08, 0x00, 0, 0, 0, 0, 0, 0])]);
    let errored = FakeNode::receiver("hidraw10", "1-4", 4, Some(B), false).get(0x08, &[Err(EPIPE)]);
    let inv = inventory(vec![setting_only, errored]);
    let reasons: Vec<String> = inv
        .nodes
        .iter()
        .map(|n| match &n.status {
            NodeStatus::Rejected(reason) => reason.to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(reasons, ["probe-mismatch: 08 00 …", "probe-error: EPIPE"]);
    assert!(inv.mice.is_empty());
}

#[test]
fn receiver_probe_failure_keeps_management_role() {
    let node = FakeNode::receiver("hidraw9", "1-4", 3, Some(A), true)
        .get(0x08, &[Err(EPIPE)])
        .slots(empty_slots());
    let inv = inventory(vec![node]);
    assert!(inv.mice.is_empty());
    assert!(matches!(
        &inv.nodes[0].status,
        NodeStatus::Candidate {
            setting: None,
            management: Some(_),
            setting_rejected: Some(_),
            ..
        }
    ));
    assert_eq!(inv.receivers[0].management, [0]);
}

#[test]
fn two_routes_become_one_mouse() {
    let mut nodes = vec![FakeNode::wired("hidraw5", "3-2", A)];
    nodes.extend(receiver_with_a("1-4"));
    let mut inv = inventory(nodes);
    assert_eq!(keys(&inv), ["c658:0a1b2c3d4e5f"]);
    let mouse = inv.mice[0].clone();
    assert_eq!(mouse.routes.len(), 2);
    assert_eq!(mouse.active_route().route, Route::Wired);
    let standby = mouse.route(Route::Receiver).unwrap();
    assert_eq!(standby.state, RouteState::Standby);
    assert_eq!(standby.slot, Some(SlotMatch::Slot(Slot::new(3).unwrap())));
    assert_eq!(
        standby.receiver.as_ref().unwrap().to_string(),
        "recv:port-1-4"
    );
    assert!(warnings(&inv).is_empty());

    let target = select_mouse(&mut inv, None, None).unwrap();
    assert_eq!(target.route.route, Route::Wired);

    let mut nodes = vec![FakeNode::wired("hidraw5", "3-2", A)];
    nodes.extend(receiver_with_a("1-4"));
    let mut inv = inventory(nodes);
    let target = select_mouse(&mut inv, None, Some(Route::Receiver)).unwrap();
    assert_eq!(target.route.path, Path::new("/dev/hidraw9"));
    let codes: Vec<&str> = target
        .warnings
        .iter()
        .map(cadrat_hidraw::Warning::code)
        .collect();
    assert_eq!(codes, ["W-INACTIVE-ROUTE"]);
}

#[test]
fn receiver_only_mouse_is_active_on_receiver() {
    let mut inv = inventory(receiver_with_a("1-4"));
    assert_eq!(inv.mice[0].active_route().route, Route::Receiver);
    assert_eq!(
        select_mouse(&mut inv, None, Some(Route::Wired)).unwrap_err(),
        SelectError::NoDevice
    );
    let target = select_mouse(&mut inv, None, Some(Route::Receiver)).unwrap();
    assert!(target.warnings.is_empty());
}

#[test]
fn slot_matching() {
    // Interface 2 but the identifier is in slot 3.
    let nodes = vec![
        FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[None],
            &[Some(A)],
            &[None],
        ]),
        FakeNode::receiver("hidraw8", "1-4", 2, Some(A), true),
    ];
    let inv = inventory(nodes);
    assert_eq!(warnings(&inv), ["W-SLOT-IF-MISMATCH"]);
    assert_eq!(
        inv.mice[0].routes[0].slot,
        Some(SlotMatch::Slot(Slot::new(3).unwrap()))
    );

    // No slot carries the ID.
    let nodes = vec![
        FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[None],
            &[Some(B)],
            &[None],
        ]),
        FakeNode::receiver("hidraw9", "1-4", 3, Some(A), true),
    ];
    let inv = inventory(nodes);
    assert_eq!(inv.mice[0].routes[0].slot, Some(SlotMatch::Unknown));
    assert!(warnings(&inv).is_empty());

    // Slots unreadable.
    let nodes = vec![
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots(empty_slots())
            .get(0x45, &[Ok(vec![0x44, 0, 0, 0, 0, 0, 0, 0])]),
        FakeNode::receiver("hidraw9", "1-4", 3, Some(A), true),
    ];
    let inv = inventory(nodes);
    assert_eq!(inv.mice[0].routes[0].slot, Some(SlotMatch::Unknown));
    assert!(matches!(&inv.receivers[0].slots, SlotsRead::Failed(m) if m.contains("slot 2")));
}

#[test]
fn ambiguous_node_refuses_to_send() {
    let second = {
        let mut n = FakeNode::wired("hidraw6", "3-2", A);
        n.info.interface = Some(2);
        n
    };
    let mut inv = inventory(vec![FakeNode::wired("hidraw5", "3-2", A), second]);
    assert_eq!(keys(&inv), ["c658:0a1b2c3d4e5f"]);
    assert!(inv.mice[0].is_ambiguous());
    assert!(matches!(
        select_mouse(&mut inv, None, None).unwrap_err(),
        SelectError::DeviceInvalid(m) if m.contains("ambiguous-node")
    ));
}

// --- sending ---

#[test]
fn send_rechecks_and_sends_once() {
    let node = FakeNode::wired("hidraw5", "3-2", A);
    let mut inv = inventory(vec![node.clone()]);
    let mut target = select_mouse(&mut inv, None, None).unwrap();
    target.send(&wire()).unwrap();
    assert_eq!(
        node.log(),
        [
            Request::Get(0x08),
            Request::Get(0x08),
            Request::Set(wire().to_vec())
        ]
    );
}

#[test]
fn send_failures() {
    for (reply, expected) in [
        (Ok(31), SendFailure::ShortWrite(31)),
        (
            Err(ENODEV),
            SendFailure::Errno(cadrat_hidraw::Errno(ENODEV)),
        ),
        (Err(EIO), SendFailure::Errno(cadrat_hidraw::Errno(EIO))),
    ] {
        let node = FakeNode::wired("hidraw5", "3-2", A).set(&[reply]);
        let mut inv = inventory(vec![node]);
        let err = select_mouse(&mut inv, None, None)
            .unwrap()
            .send(&wire())
            .unwrap_err();
        let SendError::Failed { failure, .. } = &err else {
            panic!("{err:?}");
        };
        assert_eq!(*failure, expected);
        assert_eq!(failure.is_device_gone(), matches!(reply, Err(ENODEV)));
    }
    let err = SendFailure::Errno(cadrat_hidraw::Errno(ENODEV));
    assert_eq!(err.to_string(), "device disconnected; errno: ENODEV");
}

#[test]
fn target_changed_before_send() {
    // Enumeration sees A; the check before sending sees B.
    let node = FakeNode::wired("hidraw5", "3-2", A)
        .get(0x08, &[Ok(probe_response(A)), Ok(probe_response(B))]);
    let mut inv = inventory(vec![node.clone()]);
    let err = select_mouse(&mut inv, None, None)
        .unwrap()
        .send(&wire())
        .unwrap_err();
    assert!(matches!(err, SendError::TargetChanged { .. }), "{err:?}");
    assert!(node.sets().is_empty());

    // An emptied slot answers without 08 59.
    let node = FakeNode::receiver("hidraw9", "1-4", 3, Some(A), false).get(
        0x08,
        &[Ok(probe_response(A)), Ok(vec![0x08, 0, 0, 0, 0, 0, 0, 0])],
    );
    let mut inv = inventory(vec![node.clone()]);
    let err = select_mouse(&mut inv, None, None)
        .unwrap()
        .send(&wire())
        .unwrap_err();
    assert!(matches!(err, SendError::TargetChanged { .. }), "{err:?}");
    assert!(node.sets().is_empty());

    // The device vanished before the check.
    let node =
        FakeNode::wired("hidraw5", "3-2", A).get(0x08, &[Ok(probe_response(A)), Err(ENODEV)]);
    let mut inv = inventory(vec![node.clone()]);
    let err = select_mouse(&mut inv, None, None)
        .unwrap()
        .send(&wire())
        .unwrap_err();
    assert!(matches!(
        err,
        SendError::Failed {
            failure: SendFailure::CheckErrno(_),
            ..
        }
    ));
    assert!(node.sets().is_empty());
}

#[test]
fn hidraw_option() {
    let system = FakeSystem::new(vec![
        FakeNode::wired("hidraw5", "3-2", A),
        FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(empty_slots()),
        {
            let mut n = FakeNode::wired("hidraw7", "3-3", B);
            n.open_error = Some(EACCES);
            n
        },
    ]);
    let mut target = select_node(&system, Path::new("/dev/hidraw5")).unwrap();
    assert_eq!(target.mouse.key.to_string(), "c658:0a1b2c3d4e5f");
    target.send(&wire()).unwrap();
    assert!(matches!(
        select_node(&system, Path::new("/dev/hidraw6")).unwrap_err(),
        SelectError::DeviceInvalid(m) if m.contains("not a setting node")
    ));
    assert!(matches!(
        select_node(&system, Path::new("/dev/hidraw7")).unwrap_err(),
        SelectError::PermissionDenied(_)
    ));
    assert!(matches!(
        select_node(&system, Path::new("/dev/hidraw99")).unwrap_err(),
        SelectError::DeviceInvalid(_)
    ));
    let target = select_management_node(&system, Path::new("/dev/hidraw6"), true).unwrap();
    assert_eq!(target.interface, 0);
    assert!(matches!(
        select_management_node(&system, Path::new("/dev/hidraw5"), true).unwrap_err(),
        SelectError::DeviceInvalid(m) if m.contains("not a C652")
    ));
}

// --- Receiver selection ---

#[test]
fn management_node_is_lowest_interface() {
    // Listed in reverse interface order; every interface is a candidate.
    let nodes: Vec<FakeNode> = (0u8..5)
        .rev()
        .map(|i| {
            FakeNode::receiver(&format!("hidraw{}", 10 + i), "1-4", i, None, true)
                .slots(empty_slots())
        })
        .collect();
    let mut inv = inventory(nodes);
    assert_eq!(inv.receivers.len(), 1);
    assert_eq!(inv.receivers[0].management.len(), 5);
    let target = select_receiver(&mut inv, None, true).unwrap();
    assert_eq!(target.interface, 0);
    assert_eq!(target.path, Path::new("/dev/hidraw10"));
}

#[test]
fn pairing_requirement() {
    // MI_00 declares slots but not 0x41; MI_01 declares both.
    let slots_only = FakeNode::new(
        "hidraw10",
        0xc652,
        "1-4",
        0,
        &[(0x43, 8), (0x44, 8), (0x45, 8), (0x46, 8), (0x47, 8)],
    )
    .slots(empty_slots());
    let full = FakeNode::receiver("hidraw11", "1-4", 1, None, true).slots(empty_slots());
    let mut inv = inventory(vec![slots_only.clone(), full.clone()]);
    assert_eq!(select_receiver(&mut inv, None, false).unwrap().interface, 0);
    let mut inv = inventory(vec![slots_only.clone(), full]);
    assert_eq!(select_receiver(&mut inv, None, true).unwrap().interface, 1);
    let mut inv = inventory(vec![slots_only]);
    assert!(matches!(
        select_receiver(&mut inv, None, true).unwrap_err(),
        SelectError::DeviceInvalid(_)
    ));
}

#[test]
fn two_receivers_need_a_key() {
    let nodes = || {
        vec![
            FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(empty_slots()),
            FakeNode::receiver("hidraw12", "1-5", 0, None, true).slots(empty_slots()),
        ]
    };
    let mut inv = inventory(nodes());
    assert!(matches!(
        select_receiver(&mut inv, None, true).unwrap_err(),
        SelectError::Ambiguous(k) if k == ["recv:port-1-4", "recv:port-1-5"]
    ));
    let target = select_receiver(&mut inv, Some("recv:port-1-5"), true).unwrap();
    assert_eq!(target.path, Path::new("/dev/hidraw12"));
    let mut inv = inventory(nodes());
    assert_eq!(
        select_receiver(&mut inv, Some("1"), true).unwrap_err(),
        SelectError::NoDevice
    );
    assert_eq!(inventory(vec![]).receivers.len(), 0);
    assert_eq!(
        select_receiver(&mut inventory(vec![]), None, true).unwrap_err(),
        SelectError::NoDevice
    );
}

// --- pair ---

const START: [u8; 5] = [0x41, 0x02, 0x02, 0x00, 0x00];
const STOP: [u8; 5] = [0x41, 0x02, 0x00, 0x00, 0x00];

fn management(node: FakeNode) -> (FakeNode, Box<dyn cadrat_hidraw::Device>) {
    let system = FakeSystem::new(vec![node.clone()]);
    let device = system.open(&node.info.path).unwrap();
    (node, device)
}

fn fast() -> Polling {
    Polling {
        timeout: Duration::from_secs(5),
        interval: Duration::from_secs(1),
    }
}

#[test]
fn pair_success() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[Some(B)],
            &[None, None, Some(A)],
            &[None],
        ]));
    let clock = FakeClock::default();
    let mut prompted = 0;
    let outcome = receiver::pair(
        device.as_mut(),
        &clock,
        fast(),
        &mut || prompted += 1,
        &|| false,
    )
    .unwrap();
    assert_eq!(
        outcome.result,
        PairResult::Paired(vec![Slot::new(3).unwrap()])
    );
    assert_eq!(prompted, 1);
    assert!(outcome.warnings.is_empty());
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    assert_eq!(clock_secs(&clock), 2);
    // The already occupied slot 2 is not "new".
    assert!(outcome.after.unwrap()[2].occupied());
}

fn clock_secs(clock: &FakeClock) -> u64 {
    use cadrat_hidraw::Clock;
    clock.now().as_secs()
}

#[test]
fn pair_timeout_still_stops() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(empty_slots()));
    let clock = FakeClock::default();
    let outcome = receiver::pair(device.as_mut(), &clock, fast(), &mut || {}, &|| false).unwrap();
    assert_eq!(outcome.result, PairResult::Timeout);
    assert!(outcome.stop_sent);
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    assert_eq!(clock_secs(&clock), 5);
}

#[test]
fn pair_interrupt_still_stops() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(empty_slots()));
    let clock = FakeClock::default();
    let interrupted = Rc::new(Cell::new(false));
    let flag = Rc::clone(&interrupted);
    clock.on_sleep(move |now| {
        if now >= Duration::from_secs(2) {
            flag.set(true);
        }
    });
    let outcome = receiver::pair(device.as_mut(), &clock, fast(), &mut || {}, &|| {
        interrupted.get()
    })
    .unwrap();
    assert_eq!(outcome.result, PairResult::Interrupted);
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    assert_eq!(clock_secs(&clock), 2);
}

#[test]
fn pair_stop_failure_wins() {
    let (_, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots([&[None], &[None], &[None], &[None, Some(A)], &[None]])
            .set(&[Ok(5), Err(EIO)]),
    );
    let outcome = receiver::pair(
        device.as_mut(),
        &FakeClock::default(),
        fast(),
        &mut || {},
        &|| false,
    )
    .unwrap();
    assert_eq!(
        outcome.result,
        PairResult::StopFailed(SetResult::Errno(cadrat_hidraw::Errno(EIO)))
    );
}

#[test]
fn pair_start_failure_still_stops() {
    let (node, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots(empty_slots())
            .set(&[Ok(3), Ok(5)]),
    );
    let mut prompted = false;
    let outcome = receiver::pair(
        device.as_mut(),
        &FakeClock::default(),
        fast(),
        &mut || prompted = true,
        &|| false,
    )
    .unwrap();
    assert_eq!(outcome.result, PairResult::StartFailed(SetResult::Short(3)));
    assert!(!prompted);
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
}

#[test]
fn pair_multiple_new_slots() {
    let (_, mut device) = management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
        &[None],
        &[None, Some(B)],
        &[None],
        &[None, Some(A)],
        &[None],
    ]));
    let outcome = receiver::pair(
        device.as_mut(),
        &FakeClock::default(),
        fast(),
        &mut || {},
        &|| false,
    )
    .unwrap();
    assert_eq!(
        outcome.result,
        PairResult::Paired(vec![Slot::new(1).unwrap(), Slot::new(3).unwrap()])
    );
    let codes: Vec<&str> = outcome
        .warnings
        .iter()
        .map(cadrat_hidraw::Warning::code)
        .collect();
    assert_eq!(codes, ["W-PAIR-MULTIPLE"]);
}

#[test]
fn pair_slot_read_failure_still_stops() {
    let (node, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots(empty_slots())
            .get(0x44, &[Ok(slot_response(1, None)), Err(EIO)]),
    );
    let outcome = receiver::pair(
        device.as_mut(),
        &FakeClock::default(),
        fast(),
        &mut || {},
        &|| false,
    )
    .unwrap();
    assert!(matches!(outcome.result, PairResult::SlotReadFailed(_)));
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
}

#[test]
fn pair_initial_read_failure_sends_nothing() {
    let (node, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots(empty_slots())
            .get(0x46, &[Ok(vec![0x46, 0])]),
    );
    let err = receiver::pair(
        device.as_mut(),
        &FakeClock::default(),
        fast(),
        &mut || {},
        &|| false,
    )
    .unwrap_err();
    assert!(matches!(err, receiver::SlotReadError::Protocol(_)));
    assert!(node.sets().is_empty());
}

// --- unpair ---

const UNPAIR_2: [u8; 5] = [0x41, 0x04, 0x02, 0x00, 0x00];

fn slot2() -> Slot {
    Slot::new(2).unwrap()
}

fn yes(_: &cadrat_proto::SlotReport) -> bool {
    true
}

#[test]
fn unpair_success() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[Some(A), Some(A), Some(A), None],
            &[None],
            &[None],
        ]));
    let clock = FakeClock::default();
    let mut shown = None;
    let outcome = receiver::unpair(device.as_mut(), slot2(), &clock, fast(), &mut |s| {
        shown = Some(*s);
        true
    })
    .unwrap();
    assert_eq!(outcome.result, UnpairResult::Unpaired);
    assert_eq!(shown.unwrap().id_candidate().0, A);
    assert_eq!(node.sets(), [UNPAIR_2.to_vec()]);
    assert!(!outcome.after.unwrap()[2].occupied());
    assert_eq!(clock_secs(&clock), 1);
}

#[test]
fn unpair_epipe_then_empty() {
    let (_, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots([
                &[None],
                &[None],
                &[Some(A), Some(A), None],
                &[None],
                &[None],
            ])
            .set(&[Err(EPIPE)]),
    );
    let outcome = receiver::unpair(
        device.as_mut(),
        slot2(),
        &FakeClock::default(),
        fast(),
        &mut yes,
    )
    .unwrap();
    assert_eq!(outcome.result, UnpairResult::Unpaired);
    let codes: Vec<&str> = outcome
        .warnings
        .iter()
        .map(cadrat_hidraw::Warning::code)
        .collect();
    assert_eq!(codes, ["W-UNPAIR-EPIPE"]);
}

#[test]
fn unpair_epipe_never_empty() {
    let (_, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots([&[None], &[None], &[Some(A)], &[None], &[None]])
            .set(&[Err(EPIPE)]),
    );
    let clock = FakeClock::default();
    let outcome = receiver::unpair(device.as_mut(), slot2(), &clock, fast(), &mut yes).unwrap();
    assert_eq!(outcome.result, UnpairResult::NotConfirmed);
    assert!(outcome.sent);
    assert_eq!(clock_secs(&clock), 5);
}

#[test]
fn unpair_other_errors_fail() {
    let (_, mut device) = management(
        FakeNode::receiver("hidraw6", "1-4", 0, None, true)
            .slots([&[None], &[None], &[Some(A)], &[None], &[None]])
            .set(&[Err(EIO)]),
    );
    let outcome = receiver::unpair(
        device.as_mut(),
        slot2(),
        &FakeClock::default(),
        fast(),
        &mut yes,
    )
    .unwrap();
    assert_eq!(
        outcome.result,
        UnpairResult::CommandFailed(SetResult::Errno(cadrat_hidraw::Errno(EIO)))
    );
}

#[test]
fn unpair_refuses_empty_or_changed_slot() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(empty_slots()));
    let mut asked = false;
    let outcome = receiver::unpair(
        device.as_mut(),
        slot2(),
        &FakeClock::default(),
        fast(),
        &mut |_| {
            asked = true;
            true
        },
    )
    .unwrap();
    assert_eq!(outcome.result, UnpairResult::SlotChanged);
    assert!(!asked && !outcome.sent && node.sets().is_empty());

    // Occupied when shown, a different device when re-read.
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[Some(A), Some(B)],
            &[None],
            &[None],
        ]));
    let outcome = receiver::unpair(
        device.as_mut(),
        slot2(),
        &FakeClock::default(),
        fast(),
        &mut yes,
    )
    .unwrap();
    assert_eq!(outcome.result, UnpairResult::SlotChanged);
    assert!(node.sets().is_empty());
}

#[test]
fn unpair_confirmation_refused() {
    let (node, mut device) =
        management(FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[Some(A)],
            &[None],
            &[None],
        ]));
    let outcome = receiver::unpair(
        device.as_mut(),
        slot2(),
        &FakeClock::default(),
        fast(),
        &mut |_| false,
    )
    .unwrap();
    assert_eq!(outcome.result, UnpairResult::Aborted);
    assert!(node.sets().is_empty());
    assert_eq!(node.log(), [Request::Get(0x45)]);
}
