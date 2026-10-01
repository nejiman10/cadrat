//! In-memory [`System`], [`Device`] and [`Clock`] for tests (spec implementation §4).
//!
//! Each node answers GET requests from a per-Report-ID queue whose last
//! entry repeats, answers SET requests from a queue whose last entry
//! repeats, and records every request.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::sys::{
    BUS_USB, Clock, Device, NodeInfo, PRODUCT_C652, PRODUCT_C658, RawInfo, System, VENDOR,
};

/// A request a fake node received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// GET Feature with this Report ID.
    Get(u8),
    /// SET Feature with these bytes.
    Set(Vec<u8>),
}

type Reply<T> = Result<T, i32>;

type SetHook = Box<dyn FnMut(&[u8])>;

#[derive(Default)]
struct State {
    gets: HashMap<u8, VecDeque<Reply<Vec<u8>>>>,
    sets: VecDeque<Reply<usize>>,
    log: Vec<Request>,
    on_set: Option<SetHook>,
    /// Unplugged: every open descriptor answers `ENODEV`.
    gone: bool,
    /// Descriptors currently open.
    open: usize,
    /// Another process holds the write lock (spec device §7.2).
    locked_elsewhere: bool,
    /// Descriptors of this process holding the write lock.
    locks: usize,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("gets", &self.gets)
            .field("sets", &self.sets)
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}

fn next<T: Clone>(queue: &mut VecDeque<Reply<T>>) -> Option<Reply<T>> {
    if queue.len() > 1 {
        queue.pop_front()
    } else {
        queue.front().cloned()
    }
}

/// One fake hidraw node. Clones share their state.
#[derive(Debug, Clone)]
pub struct FakeNode {
    /// sysfs information.
    pub info: NodeInfo,
    /// `open` fails with this errno.
    pub open_error: Option<i32>,
    /// `HIDIOCGRAWINFO`.
    pub raw_info: RawInfo,
    /// Report descriptor.
    pub descriptor: Vec<u8>,
    state: Rc<RefCell<State>>,
}

/// A report descriptor declaring these Feature reports as
/// `(report_id, wire_len)`.
#[must_use]
pub fn descriptor(features: &[(u8, u8)]) -> Vec<u8> {
    let mut d = vec![0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x75, 0x08];
    for &(id, len) in features {
        d.extend([0x85, id, 0x95, len - 1, 0x09, 0x01, 0xb1, 0x02]);
    }
    d.push(0xc0);
    d
}

/// Features of a setting node: `0x10` (32) and `0x08` (8).
pub const SETTING_FEATURES: [(u8, u8); 2] = [(0x10, 32), (0x08, 8)];
/// Features of a management node: `0x41` (5) and `0x43..0x47` (8).
pub const MANAGEMENT_FEATURES: [(u8, u8); 6] = [
    (0x41, 5),
    (0x43, 8),
    (0x44, 8),
    (0x45, 8),
    (0x46, 8),
    (0x47, 8),
];

/// An ID probe response for a device ID.
#[must_use]
pub fn probe_response(id: [u8; 6]) -> Vec<u8> {
    let mut r = vec![0x08, 0x59];
    r.extend(id);
    r
}

/// A slot report: empty if `id` is `None`.
#[must_use]
pub fn slot_response(slot: u8, id: Option<[u8; 6]>) -> Vec<u8> {
    let mut r = vec![0x43 + slot];
    match id {
        Some(id) => {
            r.push(0x59);
            r.extend(id);
        }
        None => r.extend([0; 7]),
    }
    r
}

impl FakeNode {
    /// A node with the given identity and features; every GET fails with
    /// `EPIPE` and every SET succeeds with the full length until scripted.
    #[must_use]
    pub fn new(name: &str, product: u16, port: &str, interface: u8, features: &[(u8, u8)]) -> Self {
        Self {
            info: NodeInfo {
                path: PathBuf::from("/dev").join(name),
                hid_id: Some((BUS_USB, VENDOR, product)),
                hid_name: Some("Fake".to_owned()),
                interface: Some(interface),
                usb_port: Some(port.to_owned()),
            },
            open_error: None,
            raw_info: RawInfo {
                bus: BUS_USB,
                vendor: VENDOR,
                product,
            },
            descriptor: descriptor(features),
            state: Rc::default(),
        }
    }

    /// A wired C658 setting node answering the probe with `id`.
    #[must_use]
    pub fn wired(name: &str, port: &str, id: [u8; 6]) -> Self {
        Self::new(name, PRODUCT_C658, port, 1, &SETTING_FEATURES)
            .get(0x08, &[Ok(probe_response(id))])
    }

    /// A C652 node on `interface`, with setting features if `id` is given
    /// and management features if `management` is set.
    #[must_use]
    pub fn receiver(
        name: &str,
        port: &str,
        interface: u8,
        id: Option<[u8; 6]>,
        management: bool,
    ) -> Self {
        let mut features = Vec::new();
        if id.is_some() {
            features.extend(SETTING_FEATURES);
        }
        if management {
            features.extend(MANAGEMENT_FEATURES);
        }
        let node = Self::new(name, PRODUCT_C652, port, interface, &features);
        match id {
            Some(id) => node.get(0x08, &[Ok(probe_response(id))]),
            None => node,
        }
    }

    /// Scripts the replies to GET `report_id`; the last one repeats.
    #[must_use]
    pub fn get(self, report_id: u8, replies: &[Reply<Vec<u8>>]) -> Self {
        self.state
            .borrow_mut()
            .gets
            .insert(report_id, replies.iter().cloned().collect());
        self
    }

    /// Scripts the slots: `slots[n]` is the sequence of occupants of slot n.
    #[must_use]
    pub fn slots(mut self, slots: [&[Option<[u8; 6]>]; 5]) -> Self {
        for (n, occupants) in (0u8..).zip(slots) {
            let replies: Vec<_> = occupants
                .iter()
                .map(|id| Ok(slot_response(n, *id)))
                .collect();
            self = self.get(0x43 + n, &replies);
        }
        self
    }

    /// Scripts the replies to SET requests; the last one repeats.
    #[must_use]
    pub fn set(self, replies: &[Reply<usize>]) -> Self {
        self.state.borrow_mut().sets = replies.iter().copied().collect();
        self
    }

    /// Runs `hook` with the data of every SET request, before it is
    /// answered (e.g. to edit a file while a send is in progress).
    #[must_use]
    pub fn on_set(self, hook: impl FnMut(&[u8]) + 'static) -> Self {
        self.state.borrow_mut().on_set = Some(Box::new(hook));
        self
    }

    /// Every request received so far.
    #[must_use]
    pub fn log(&self) -> Vec<Request> {
        self.state.borrow().log.clone()
    }

    /// Unplugs the device: descriptors already open answer `ENODEV` from
    /// now on. Remove the node from the [`FakeSystem`] as well, or replace
    /// it with a new node on the same path to model a reconnect.
    pub fn unplug(&self) {
        self.state.borrow_mut().gone = true;
    }

    /// Another process takes the write lock on this node (`flock(LOCK_EX)`)
    /// until [`FakeNode::unlock_elsewhere`]. A lock this process already
    /// holds is not affected.
    pub fn lock_elsewhere(&self) {
        self.state.borrow_mut().locked_elsewhere = true;
    }

    /// The other process releases the write lock.
    pub fn unlock_elsewhere(&self) {
        self.state.borrow_mut().locked_elsewhere = false;
    }

    /// Whether a descriptor opened through the [`FakeSystem`] holds the
    /// write lock.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.state.borrow().locks > 0
    }

    /// How many descriptors of this node are open.
    #[must_use]
    pub fn open_count(&self) -> usize {
        self.state.borrow().open
    }

    /// Only the SET requests received so far.
    #[must_use]
    pub fn sets(&self) -> Vec<Vec<u8>> {
        self.log()
            .into_iter()
            .filter_map(|r| match r {
                Request::Set(data) => Some(data),
                Request::Get(_) => None,
            })
            .collect()
    }
}

/// Like a real hidraw descriptor, a fake device that once answered `ENODEV`
/// stays dead; opening the node again gives a working descriptor.
#[derive(Debug)]
struct FakeDevice {
    node: FakeNode,
    dead: bool,
    locked: bool,
}

const ENODEV: i32 = 19;

impl FakeDevice {
    fn new(node: FakeNode) -> Self {
        node.state.borrow_mut().open += 1;
        Self {
            node,
            dead: false,
            locked: false,
        }
    }

    fn gone(&self) -> bool {
        self.dead || self.node.state.borrow().gone
    }

    fn check<T>(&mut self, reply: Reply<T>) -> io::Result<T> {
        if reply.as_ref().is_err_and(|&errno| errno == ENODEV) {
            self.dead = true;
        }
        reply.map_err(os_error)
    }
}

fn os_error(errno: i32) -> io::Error {
    io::Error::from_raw_os_error(errno)
}

impl Drop for FakeDevice {
    fn drop(&mut self) {
        let mut state = self.node.state.borrow_mut();
        state.open -= 1;
        if self.locked {
            state.locks -= 1;
        }
    }
}

impl Device for FakeDevice {
    fn raw_info(&mut self) -> io::Result<RawInfo> {
        if self.node.state.borrow().gone {
            return Err(os_error(ENODEV));
        }
        Ok(self.node.raw_info)
    }

    fn descriptor(&mut self) -> io::Result<Vec<u8>> {
        Ok(self.node.descriptor.clone())
    }

    fn get_feature(&mut self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        if self.gone() {
            return Err(os_error(ENODEV));
        }
        let node = self.node.clone();
        let mut state = node.state.borrow_mut();
        state.log.push(Request::Get(report_id));
        let epipe = rustix::io::Errno::PIPE.raw_os_error();
        let reply = state
            .gets
            .get_mut(&report_id)
            .and_then(next)
            .unwrap_or(Err(epipe));
        drop(state);
        self.check(reply).map(|mut data| {
            data.truncate(len);
            data
        })
    }

    fn set_feature(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.gone() {
            return Err(os_error(ENODEV));
        }
        let node = self.node.clone();
        let mut state = node.state.borrow_mut();
        state.log.push(Request::Set(data.to_vec()));
        if let Some(hook) = state.on_set.as_mut() {
            hook(data);
        }
        let reply = next(&mut state.sets).unwrap_or(Ok(data.len()));
        drop(state);
        self.check(reply)
    }

    fn lock(&mut self) -> io::Result<()> {
        if self.locked {
            return Ok(());
        }
        let mut state = self.node.state.borrow_mut();
        // Like flock, a second descriptor conflicts even in the same process.
        if state.locked_elsewhere || state.locks > 0 {
            return Err(os_error(rustix::io::Errno::WOULDBLOCK.raw_os_error()));
        }
        state.locks += 1;
        self.locked = true;
        Ok(())
    }
}

/// A fake system made of [`FakeNode`]s.
#[derive(Debug, Clone, Default)]
pub struct FakeSystem {
    /// The nodes, in sysfs order.
    pub nodes: Vec<FakeNode>,
}

impl FakeSystem {
    /// A system with these nodes.
    #[must_use]
    pub fn new(nodes: Vec<FakeNode>) -> Self {
        Self { nodes }
    }

    fn find(&self, path: &Path) -> io::Result<&FakeNode> {
        self.nodes
            .iter()
            .find(|n| n.info.path == path)
            .ok_or_else(|| os_error(rustix::io::Errno::NOENT.raw_os_error()))
    }
}

impl System for FakeSystem {
    fn nodes(&self) -> io::Result<Vec<NodeInfo>> {
        Ok(self.nodes.iter().map(|n| n.info.clone()).collect())
    }

    fn node(&self, path: &Path) -> io::Result<NodeInfo> {
        self.find(path).map(|n| n.info.clone())
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Device>> {
        let node = self.find(path)?;
        if let Some(errno) = node.open_error {
            return Err(os_error(errno));
        }
        Ok(Box::new(FakeDevice::new(node.clone())))
    }
}

/// A clock that only moves when slept on.
#[derive(Default)]
pub struct FakeClock {
    now: Cell<Duration>,
    /// Called after every sleep, e.g. to raise an interrupt.
    #[allow(clippy::type_complexity)]
    on_sleep: RefCell<Option<Box<dyn FnMut(Duration)>>>,
}

impl std::fmt::Debug for FakeClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeClock")
            .field("now", &self.now.get())
            .finish_non_exhaustive()
    }
}

impl FakeClock {
    /// Runs `hook` with the new time after every sleep.
    pub fn on_sleep(&self, hook: impl FnMut(Duration) + 'static) {
        *self.on_sleep.borrow_mut() = Some(Box::new(hook));
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.now.get()
    }

    fn sleep(&self, duration: Duration) {
        self.now.set(self.now.get() + duration);
        if let Some(hook) = self.on_sleep.borrow_mut().as_mut() {
            hook(self.now.get());
        }
    }
}
