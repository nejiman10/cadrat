//! Holding the wired C658 open against fake sysfs (spec device §9).

#![allow(missing_docs)]

use std::io;
use std::path::{Path, PathBuf};

use cadrat_hidraw::fake::{FakeNode, FakeSystem, SETTING_FEATURES};
use cadrat_hidraw::sys::{PRODUCT_C652, PRODUCT_C658};
use cadrat_hidraw::{Device, Errno, HoldEvent, HoldOpen, NodeInfo, System};

const EACCES: i32 = 13;
const EPERM: i32 = 1;

fn c658(name: &str, interface: u8) -> FakeNode {
    FakeNode::new(name, PRODUCT_C658, "3-2", interface, &SETTING_FEATURES)
}

fn held(path: &str, interface: u8) -> HoldEvent {
    HoldEvent::Held {
        path: PathBuf::from(path),
        interface: Some(interface),
    }
}

fn released(path: &str) -> HoldEvent {
    HoldEvent::Released {
        path: PathBuf::from(path),
    }
}

fn paths(hold: &HoldOpen) -> Vec<&Path> {
    hold.held().map(PathBuf::as_path).collect()
}

#[test]
fn holds_every_wired_c658_node_and_nothing_else() {
    let mi00 = c658("hidraw4", 0);
    let mi01 = c658("hidraw5", 1);
    let receiver = FakeNode::new("hidraw6", PRODUCT_C652, "1-4", 0, &SETTING_FEATURES);
    let mut bluetooth = c658("hidraw7", 0);
    bluetooth.info.hid_id = Some((0x05, 0x256f, PRODUCT_C658));
    let system = FakeSystem::new(vec![
        mi00.clone(),
        mi01.clone(),
        receiver.clone(),
        bluetooth.clone(),
    ]);

    let mut hold = HoldOpen::default();
    assert_eq!(
        hold.reconcile(&system),
        [held("/dev/hidraw4", 0), held("/dev/hidraw5", 1)]
    );
    assert_eq!(
        paths(&hold),
        [Path::new("/dev/hidraw4"), Path::new("/dev/hidraw5")]
    );
    assert_eq!(
        [
            mi00.open_count(),
            mi01.open_count(),
            receiver.open_count(),
            bluetooth.open_count()
        ],
        [1, 1, 0, 0]
    );
    // Nothing is sent to the mouse.
    assert!(mi00.log().is_empty() && mi01.log().is_empty());

    // Polling again changes nothing and opens nothing twice.
    assert_eq!(hold.reconcile(&system), []);
    assert_eq!([mi00.open_count(), mi01.open_count()], [1, 1]);
}

#[test]
fn follows_unplug_and_reconnect() {
    let old = [c658("hidraw4", 0), c658("hidraw5", 1)];
    let mut hold = HoldOpen::default();
    hold.reconcile(&FakeSystem::new(old.to_vec()));

    // Unplugged: the nodes leave sysfs and are closed.
    for node in &old {
        node.unplug();
    }
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![])),
        [released("/dev/hidraw4"), released("/dev/hidraw5")]
    );
    assert_eq!([old[0].open_count(), old[1].open_count()], [0, 0]);

    // Plugged in again under new node numbers.
    let new = [c658("hidraw8", 0), c658("hidraw9", 1)];
    assert_eq!(
        hold.reconcile(&FakeSystem::new(new.to_vec())),
        [held("/dev/hidraw8", 0), held("/dev/hidraw9", 1)]
    );
    assert_eq!([new[0].open_count(), new[1].open_count()], [1, 1]);
}

#[test]
fn reopens_a_path_reused_between_polls() {
    // Unplugged and plugged in again within one poll, on the same paths:
    // sysfs looks unchanged, but the old descriptors are dead.
    let old = c658("hidraw4", 0);
    let mut hold = HoldOpen::default();
    hold.reconcile(&FakeSystem::new(vec![old.clone()]));
    old.unplug();

    let new = c658("hidraw4", 0);
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![new.clone()])),
        [released("/dev/hidraw4"), held("/dev/hidraw4", 0)]
    );
    assert_eq!([old.open_count(), new.open_count()], [0, 1]);
}

#[test]
fn reports_an_open_error_once_until_it_changes() {
    let mut node = c658("hidraw4", 0);
    node.open_error = Some(EACCES);
    let mut hold = HoldOpen::default();
    let failed = |errno| HoldEvent::OpenFailed {
        path: PathBuf::from("/dev/hidraw4"),
        errno: Errno(errno),
    };

    let system = FakeSystem::new(vec![node.clone()]);
    assert_eq!(hold.reconcile(&system), [failed(EACCES)]);
    assert_eq!(hold.reconcile(&system), []);

    node.open_error = Some(EPERM);
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![node.clone()])),
        [failed(EPERM)]
    );

    // The udev rule took effect.
    node.open_error = None;
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![node.clone()])),
        [held("/dev/hidraw4", 0)]
    );

    // A node that went away and came back is reported afresh.
    let mut hold = HoldOpen::default();
    node.open_error = Some(EACCES);
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![node.clone()])),
        [failed(EACCES)]
    );
    assert_eq!(hold.reconcile(&FakeSystem::new(vec![])), []);
    assert_eq!(
        hold.reconcile(&FakeSystem::new(vec![node])),
        [failed(EACCES)]
    );
}

/// A system whose sysfs cannot be listed.
struct NoSysfs;

impl System for NoSysfs {
    fn nodes(&self) -> io::Result<Vec<NodeInfo>> {
        Err(io::Error::from_raw_os_error(EACCES))
    }
    fn node(&self, _: &Path) -> io::Result<NodeInfo> {
        unreachable!()
    }
    fn open(&self, _: &Path) -> io::Result<Box<dyn Device>> {
        unreachable!()
    }
}

#[test]
fn reports_a_sysfs_failure_once_and_keeps_what_it_holds() {
    let node = c658("hidraw4", 0);
    let mut hold = HoldOpen::default();
    hold.reconcile(&FakeSystem::new(vec![node.clone()]));

    let failed = HoldEvent::EnumerateFailed {
        errno: Errno(EACCES),
    };
    assert_eq!(hold.reconcile(&NoSysfs), std::slice::from_ref(&failed));
    assert_eq!(hold.reconcile(&NoSysfs), []);
    assert_eq!(node.open_count(), 1);

    assert_eq!(hold.reconcile(&FakeSystem::new(vec![node.clone()])), []);
    assert_eq!(hold.reconcile(&NoSysfs), [failed]);
}

#[test]
fn release_all_closes_everything() {
    let nodes = [c658("hidraw4", 0), c658("hidraw5", 1)];
    let mut hold = HoldOpen::default();
    hold.reconcile(&FakeSystem::new(nodes.to_vec()));
    assert_eq!(
        hold.release_all(),
        [released("/dev/hidraw4"), released("/dev/hidraw5")]
    );
    assert_eq!([nodes[0].open_count(), nodes[1].open_count()], [0, 0]);
    assert_eq!(hold.held().count(), 0);
}
