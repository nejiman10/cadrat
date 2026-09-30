//! Keeping the wired C658's hidraw nodes open (spec device §9).
//!
//! On the tested host, a wired C658 stopped sending input a few seconds
//! after it was plugged in unless some process held its hidraw nodes open.
//! [`HoldOpen`] is the workaround the research repository validated: open
//! every wired C658 node, keep it open, and follow reconnects by polling.
//! It claims no root cause.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::sys::{BUS_USB, Device, Errno, NodeInfo, PRODUCT_C658, System, VENDOR};

/// What changed in one [`HoldOpen::reconcile`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldEvent {
    /// A node was opened and is now held.
    Held {
        /// Device path.
        path: PathBuf,
        /// Interface number, if sysfs knows it.
        interface: Option<u8>,
    },
    /// A held node was closed because it disappeared or went stale.
    Released {
        /// Device path.
        path: PathBuf,
    },
    /// A node could not be opened. Reported once per path and errno.
    OpenFailed {
        /// Device path.
        path: PathBuf,
        /// The `open` error.
        errno: Errno,
    },
    /// sysfs could not be listed. Reported once until it recovers.
    EnumerateFailed {
        /// The error.
        errno: Errno,
    },
}

/// The set of held wired C658 nodes.
#[derive(Debug, Default)]
pub struct HoldOpen {
    held: BTreeMap<PathBuf, Box<dyn Device>>,
    /// Open errors already reported, to keep a service log quiet while a
    /// node stays inaccessible.
    reported: BTreeMap<PathBuf, Errno>,
    enumerate_failed: bool,
}

/// Whether sysfs describes a wired C658 node.
fn is_target(info: &NodeInfo) -> bool {
    info.hid_id == Some((BUS_USB, VENDOR, PRODUCT_C658))
}

impl HoldOpen {
    /// Paths currently held, in order.
    pub fn held(&self) -> impl Iterator<Item = &PathBuf> {
        self.held.keys()
    }

    /// Brings the held set in line with sysfs: closes nodes that are gone
    /// or no longer answer, and opens new ones.
    pub fn reconcile(&mut self, system: &dyn System) -> Vec<HoldEvent> {
        let nodes = match system.nodes() {
            Ok(nodes) => {
                self.enumerate_failed = false;
                nodes
            }
            Err(e) => return self.enumerate_error(&e),
        };
        let targets: BTreeMap<PathBuf, NodeInfo> = nodes
            .into_iter()
            .filter(is_target)
            .map(|info| (info.path.clone(), info))
            .collect();

        let mut events = Vec::new();
        // A path can be reused by a new device after a quick reconnect; the
        // old descriptor then fails HIDIOCGRAWINFO, so reopen it.
        let stale: Vec<PathBuf> = self
            .held
            .iter_mut()
            .filter_map(|(path, device)| {
                (!targets.contains_key(path) || device.raw_info().is_err()).then(|| path.clone())
            })
            .collect();
        for path in stale {
            self.held.remove(&path);
            events.push(HoldEvent::Released { path });
        }
        self.reported.retain(|path, _| targets.contains_key(path));

        for (path, info) in targets {
            if self.held.contains_key(&path) {
                continue;
            }
            match system.open(&path) {
                Ok(device) => {
                    self.held.insert(path.clone(), device);
                    self.reported.remove(&path);
                    events.push(HoldEvent::Held {
                        path,
                        interface: info.interface,
                    });
                }
                Err(e) => {
                    let errno = Errno::of(&e);
                    if self.reported.insert(path.clone(), errno) != Some(errno) {
                        events.push(HoldEvent::OpenFailed { path, errno });
                    }
                }
            }
        }
        events
    }

    fn enumerate_error(&mut self, error: &io::Error) -> Vec<HoldEvent> {
        if std::mem::replace(&mut self.enumerate_failed, true) {
            Vec::new()
        } else {
            vec![HoldEvent::EnumerateFailed {
                errno: Errno::of(error),
            }]
        }
    }

    /// Closes every held node.
    pub fn release_all(&mut self) -> Vec<HoldEvent> {
        std::mem::take(&mut self.held)
            .into_keys()
            .map(|path| HoldEvent::Released { path })
            .collect()
    }
}
