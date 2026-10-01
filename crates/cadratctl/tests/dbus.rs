//! `cadratd` and `cadratctl` on a private `dbus-daemon`, against the fake
//! transport, compared with `cadrat-tool` in the same situations
//! (spec implementation §4, D-Bus layer).

#![allow(missing_docs)]

use std::any::Any;
use std::cell::Cell;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cadrat_command::DaemonLock;
use cadrat_config::{Preset, template};
use cadrat_hidraw::fake::{FakeClock, FakeNode, FakeSystem};
use cadrat_hidraw::{Clock, Device, NodeInfo, System, SystemClock};
use cadratctl::Bus;
use serde_json::Value;
use zbus::zvariant::Value as Variant;

// Synthetic device IDs; not taken from any device.
const A: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];
const B: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
const START: [u8; 5] = [0x41, 0x02, 0x02, 0x00, 0x00];
const STOP: [u8; 5] = [0x41, 0x02, 0x00, 0x00, 0x00];
const UNPAIR_3: [u8; 5] = [0x41, 0x04, 0x03, 0x00, 0x00];
const UID: u32 = 1000;

// --- a private bus ---

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Self {
        let mut child = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is needed for these tests");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().to_owned(),
        }
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// --- the outside world of cadratd ---

/// A fake system whose nodes a test replaces while cadratd runs.
#[derive(Default)]
struct Plug(Mutex<FakeSystem>);

impl Plug {
    fn set(&self, nodes: Vec<FakeNode>) {
        *self.0.lock().unwrap() = FakeSystem::new(nodes);
    }
    fn system(&self) -> FakeSystem {
        self.0.lock().unwrap().clone()
    }
}

impl System for Plug {
    fn nodes(&self) -> std::io::Result<Vec<NodeInfo>> {
        self.system().nodes()
    }
    fn node(&self, path: &Path) -> std::io::Result<NodeInfo> {
        self.system().node(path)
    }
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Device>> {
        self.system().open(path)
    }
}

struct TestStop {
    flag: AtomicBool,
    read: UnixStream,
    write: UnixStream,
}

impl TestStop {
    fn new() -> Self {
        let (read, write) = UnixStream::pair().unwrap();
        Self {
            flag: AtomicBool::new(false),
            read,
            write,
        }
    }
    fn raise(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _ = (&self.write).write_all(b"x");
    }
}

impl cadratd::Stop for TestStop {
    fn wake(&self) -> BorrowedFd<'_> {
        self.read.as_fd()
    }
    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
struct SharedLog(Arc<Mutex<Vec<u8>>>);

impl Write for SharedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedLog {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

// --- cadratctl's signals ---

#[derive(Default)]
struct TestSignals {
    handler: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl TestSignals {
    fn raise(&self) {
        if let Some(handler) = self.handler.lock().unwrap().as_ref() {
            handler();
        }
    }
}

impl cadratctl::Signals for TestSignals {
    fn catch(&self, on_signal: Box<dyn Fn() + Send + Sync>) -> Box<dyn Any> {
        *self.handler.lock().unwrap() = Some(on_signal);
        Box::new(())
    }
}

// --- cadrat-tool's interrupt ---

#[derive(Default)]
struct NoInterrupt(Cell<bool>);

impl cadrat_tool::Interrupt for NoInterrupt {
    fn arm(&self) {
        self.0.set(true);
    }
    fn disarm(&self) {
        self.0.set(false);
    }
    fn is_set(&self) -> bool {
        false
    }
}

#[derive(Debug)]
struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Output {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }
}

/// How cadratd starts.
struct Start {
    clock: Arc<dyn Clock + Send + Sync>,
    /// Wait until it is ready.
    wait: bool,
}

impl Default for Start {
    fn default() -> Self {
        Self {
            clock: Arc::new(FakeClock::default()),
            wait: true,
        }
    }
}

struct Harness {
    dir: tempfile::TempDir,
    bus: PrivateBus,
    plug: Arc<Plug>,
    stop: Arc<TestStop>,
    signals: Arc<TestSignals>,
    daemon: Option<JoinHandle<i32>>,
    log: SharedLog,
}

impl Harness {
    fn new() -> Self {
        Self::start(Start::default())
    }

    fn start(start: Start) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(format!("run/{UID}"))).unwrap();
        fs::create_dir_all(dir.path().join("dev")).unwrap();
        let mut h = Self {
            dir,
            bus: PrivateBus::start(),
            plug: Arc::default(),
            stop: Arc::new(TestStop::new()),
            signals: Arc::default(),
            daemon: None,
            log: SharedLog::default(),
        };
        h.daemon = Some(h.spawn_daemon(start.clock));
        if start.wait {
            h.wait_for(|h| h.property::<bool>("Ready") == Some(true));
        }
        h
    }

    fn daemon_lock(&self) -> DaemonLock {
        DaemonLock {
            run_user: self.dir.path().join("run"),
            uid: UID,
        }
    }

    fn spawn_daemon(&self, clock: Arc<dyn Clock + Send + Sync>) -> JoinHandle<i32> {
        let world = cadratd::World {
            system: Arc::clone(&self.plug) as Arc<dyn System + Send + Sync>,
            clock,
            xdg_config_home: Some(self.dir.path().into()),
            home: None,
            lock_timeout: Duration::from_millis(100),
            daemon_lock: self.daemon_lock(),
            dev: self.dir.path().join("dev"),
            bus: Bus::Address(self.bus.address.clone()),
            start_wait: Duration::from_secs(20),
            settle: Duration::from_millis(50),
        };
        let stop = Arc::clone(&self.stop);
        let log = self.log.clone();
        std::thread::spawn(move || cadratd::run(world, &*stop, Box::new(log)))
    }

    fn stop_daemon(&mut self) -> i32 {
        self.stop.raise();
        self.daemon.take().map_or(0, |d| d.join().unwrap())
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("cadrat/default.toml")
    }

    fn write_config(&self, text: &str) {
        fs::create_dir_all(self.config().parent().unwrap()).unwrap();
        fs::write(self.config(), text).unwrap();
    }

    fn read_config(&self) -> String {
        fs::read_to_string(self.config()).unwrap()
    }

    fn ctl_with(&self, args: &[&str], stdin: &str, terminal: bool) -> Output {
        ctl(
            &self.bus.address,
            self.dir.path(),
            &self.signals,
            args,
            stdin,
            terminal,
        )
    }

    fn ctl(&self, args: &[&str]) -> Output {
        self.ctl_with(args, "", false)
    }

    /// Runs `cadrat-tool` on its own fake devices, with the same
    /// configuration directory and without cadratd's lock.
    fn tool_with(
        &self,
        nodes: Vec<FakeNode>,
        args: &[&str],
        stdin: &str,
        terminal: bool,
    ) -> Output {
        let system = FakeSystem::new(nodes);
        let clock = FakeClock::default();
        let interrupt = NoInterrupt::default();
        let env = cadrat_tool::Env {
            system: &system,
            clock: &clock,
            interrupt: &interrupt,
            xdg_config_home: Some(self.dir.path().into()),
            home: None,
            lock_timeout: Duration::from_millis(100),
            daemon_lock: None,
        };
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut input = stdin.as_bytes();
        let io = cadrat_tool::Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
            stdin: &mut input,
            stdin_is_terminal: terminal,
        };
        let args = std::iter::once("cadrat-tool")
            .chain(args.iter().copied())
            .map(Into::into);
        let code = cadrat_tool::run(args, &env, io);
        Output {
            code,
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
        }
    }

    fn conn(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.bus.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }

    #[allow(clippy::unused_self)]
    fn proxy(&self, conn: &zbus::blocking::Connection) -> zbus::blocking::Proxy<'static> {
        zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(conn)
            .destination(cadrat_dbus::BUS_NAME)
            .unwrap()
            .path(cadrat_dbus::PATH)
            .unwrap()
            .interface(cadrat_dbus::INTERFACE)
            .unwrap()
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .unwrap()
    }

    fn property<T>(&self, name: &str) -> Option<T>
    where
        T: TryFrom<zbus::zvariant::OwnedValue>,
        T::Error: Into<zbus::Error>,
    {
        let conn = self.conn();
        self.proxy(&conn).get_property(name).ok()
    }

    /// Calls a method directly, as another client would.
    fn call(&self, method: &str, args: &[(&str, Variant<'static>)]) -> Result<Value, String> {
        let conn = self.conn();
        let args: HashMap<&str, Variant<'_>> = args
            .iter()
            .map(|(k, v)| (*k, v.try_clone().unwrap()))
            .collect();
        match self.proxy(&conn).call_method(method, &(args,)) {
            Ok(reply) => {
                Ok(serde_json::from_str(&reply.body().deserialize::<String>().unwrap()).unwrap())
            }
            Err(zbus::Error::MethodError(name, _, _)) => Err(name.as_str().to_owned()),
            Err(e) => panic!("{e}"),
        }
    }

    fn wait_for(&self, ready: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !ready(self) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "timed out; log:\n{}",
                self.log.text()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if self.daemon.is_some() {
            self.stop_daemon();
        }
    }
}

fn ctl(
    address: &str,
    dir: &Path,
    signals: &Arc<TestSignals>,
    args: &[&str],
    stdin: &str,
    terminal: bool,
) -> Output {
    let env = cadratctl::Env {
        bus: Bus::Address(address.to_owned()),
        xdg_config_home: Some(dir.into()),
        home: None,
        cwd: dir.to_path_buf(),
        signals: &**signals,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut input = stdin.as_bytes();
    let io = cadratctl::Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
        stdin: &mut input,
        stdin_is_terminal: terminal,
    };
    let args = std::iter::once("cadratctl")
        .chain(args.iter().copied())
        .map(Into::into);
    let code = cadratctl::run(args, &env, io);
    Output {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn baseline() -> String {
    template(Preset::ResearchBaseline)
}

fn wired() -> FakeNode {
    FakeNode::wired("hidraw5", "3-2", A)
}

fn management(slots: [&'static [Option<[u8; 6]>]; 5]) -> FakeNode {
    FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(slots)
}

fn two_routes() -> Vec<FakeNode> {
    vec![
        wired(),
        FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots([
            &[None],
            &[None],
            &[None],
            &[Some(A)],
            &[None],
        ]),
        FakeNode::receiver("hidraw9", "1-4", 3, Some(A), true),
    ]
}

/// Compares `cadratctl` with `cadrat-tool` in the same situation: the same
/// configuration file and equal fresh devices. Guidance names the program.
fn same(h: &Harness, nodes: fn() -> Vec<FakeNode>, config: Option<&str>, args: &[&str]) -> Output {
    same_with(h, nodes, nodes, config, args, "", false)
}

fn same_with(
    h: &Harness,
    tool_nodes: fn() -> Vec<FakeNode>,
    ctl_nodes: fn() -> Vec<FakeNode>,
    config: Option<&str>,
    args: &[&str],
    stdin: &str,
    terminal: bool,
) -> Output {
    let reset = || match config {
        Some(text) => h.write_config(text),
        None => {
            let _ = fs::remove_file(h.config());
        }
    };
    reset();
    let tool = h.tool_with(tool_nodes(), args, stdin, terminal);
    let tool_config = fs::read_to_string(h.config()).ok();
    reset();
    h.plug.set(ctl_nodes());
    let ctl = h.ctl_with(args, stdin, terminal);
    let ctl_config = fs::read_to_string(h.config()).ok();
    let context = format!("{args:?}\n--- tool\n{tool:?}\n--- ctl\n{ctl:?}");
    assert_eq!(ctl.code, tool.code, "{context}");
    assert_eq!(
        ctl.stdout,
        tool.stdout.replace("cadrat-tool", "cadratctl"),
        "{context}"
    );
    assert_eq!(
        ctl.stderr,
        tool.stderr.replace("cadrat-tool", "cadratctl"),
        "{context}"
    );
    assert_eq!(ctl_config, tool_config, "{context}");
    ctl
}

// --- the same results as cadrat-tool ---

#[test]
fn configuration_commands_match_cadrat_tool() {
    let h = Harness::new();
    let none = Vec::new;
    let base = baseline();
    for json in [false, true] {
        let extra: &[&str] = if json { &["--json"] } else { &[] };
        let args = |a: &[&'static str]| [a, extra].concat();
        same(&h, none, None, &args(&["init"]));
        same(
            &h,
            none,
            Some(&base),
            &args(&["init", "--preset=research-baseline"]),
        );
        same(
            &h,
            none,
            None,
            &args(&["init", "--preset=research-baseline", "--force"]),
        );
        same(&h, none, Some(&base), &args(&["get"]));
        same(&h, none, Some(&base), &args(&["get", "-n", "mouse.dpi"]));
        same(&h, none, Some(&base), &args(&["get", "--wire"]));
        same(&h, none, Some(&base), &args(&["get", "mouse.color"]));
        same(
            &h,
            none,
            Some("schema = 1\n[mouse]\ndpi = 800\n"),
            &args(&["get"]),
        );
        same(&h, none, None, &args(&["check"]));
        same(&h, none, Some(&base), &args(&["check"]));
        same(
            &h,
            none,
            Some(&base),
            &args(&["set", "mouse.dpi=800", "--dry-run"]),
        );
        same(&h, none, Some(&base), &args(&["set", "mouse.dpi=8000000"]));
        same(&h, none, Some(&base), &args(&["apply", "--dry-run", "-q"]));
    }
    // A relative --config is made absolute in cadratctl's directory.
    h.write_config(&base);
    let out = h.ctl(&["check", "--config=cadrat/default.toml"]);
    assert_eq!(out.code, 0, "{out:?}");
    assert_eq!(out.stdout, format!("ok: {}\n", h.config().display()));
}

#[test]
fn device_commands_match_cadrat_tool() {
    let h = Harness::new();
    let base = baseline();
    for json in [false, true] {
        let extra: &[&str] = if json { &["--json"] } else { &[] };
        let args = |a: &[&'static str]| [a, extra].concat();
        same(&h, two_routes, None, &args(&["list"]));
        same(&h, two_routes, None, &args(&["list", "--nodes"]));
        same(&h, two_routes, None, &args(&["list", "--redact"]));
        same(&h, Vec::new, None, &args(&["list"]));
        same(
            &h,
            two_routes,
            Some(&base),
            &args(&["set", "mouse.dpi=800"]),
        );
        same(
            &h,
            two_routes,
            Some(&base),
            &args(&["set", "mouse.dpi=800", "--no-save"]),
        );
        same(
            &h,
            two_routes,
            Some(&base),
            &args(&["apply", "--route=receiver"]),
        );
        same(&h, two_routes, Some(&base), &args(&["apply", "--mouse=9"]));
        same(&h, Vec::new, Some(&base), &args(&["apply"]));
        same(&h, two_routes, None, &args(&["receiver", "slots"]));
        same(
            &h,
            two_routes,
            None,
            &args(&["receiver", "slots", "--redact"]),
        );
        same(
            &h,
            two_routes,
            None,
            &args(&["receiver", "slots", "--mouse=1"]),
        );
        same(&h, Vec::new, None, &args(&["receiver", "slots"]));
    }
    // What was sent and saved.
    let ctl_nodes = two_routes();
    h.plug.set(ctl_nodes.clone());
    h.write_config(&base);
    assert_eq!(h.ctl(&["set", "mouse.dpi=900"]).code, 0);
    assert!(h.read_config().contains("dpi = 900"));
    assert_eq!(ctl_nodes[0].sets().len(), 1);
}

#[test]
fn verbose_output_matches_cadrat_tool() {
    let h = Harness::new();
    let base = baseline();
    same(&h, two_routes, None, &["list", "-v"]);
    same(&h, two_routes, None, &["list", "-v", "--json"]);
    same(&h, two_routes, None, &["list", "-v", "--nodes", "--json"]);
    same(&h, two_routes, Some(&base), &["apply", "-v", "--json"]);
    same(&h, two_routes, None, &["receiver", "slots", "-v", "--json"]);
}

fn pair_node() -> Vec<FakeNode> {
    vec![management([
        &[None],
        &[None],
        &[None, None, None, None, Some(B)],
        &[None],
        &[None],
    ])]
}

#[test]
fn pair_matches_cadrat_tool() {
    let h = Harness::new();
    let out = same(&h, pair_node, None, &["receiver", "pair"]);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("put the mouse in pairing mode"));
    assert!(out.stdout.contains("run `cadratctl list`"));
    same(&h, pair_node, None, &["receiver", "pair", "--json", "-q"]);
    let silent = || vec![management([&[None]; 5])];
    same(&h, silent, None, &["receiver", "pair", "--timeout=3"]);
    same(&h, silent, None, &["receiver", "pair", "--timeout=0"]);
}

/// Slot 3 holds A for `reads` slot reads, then empties.
fn slot3_for(reads: usize) -> Vec<FakeNode> {
    let occupants: &'static [Option<[u8; 6]>] = match reads {
        3 => &[Some(A), Some(A), Some(A), None],
        5 => &[Some(A), Some(A), Some(A), Some(A), Some(A), None],
        _ => unreachable!(),
    };
    vec![management([&[None], &[None], &[None], occupants, &[None]])]
}

// cadratctl reads the slots once more before asking (spec ctl/cli §2).
fn tool_slot3() -> Vec<FakeNode> {
    slot3_for(3)
}
fn ctl_slot3() -> Vec<FakeNode> {
    slot3_for(5)
}

#[test]
fn unpair_matches_cadrat_tool() {
    let h = Harness::new();
    let unpair = ["receiver", "unpair", "3"];
    let out = same_with(&h, tool_slot3, ctl_slot3, None, &unpair, "y\n", true);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("Unpair slot 3? [y/N]"));
    let out = same_with(&h, tool_slot3, ctl_slot3, None, &unpair, "n\n", true);
    assert_eq!(out.code, 18);
    same_with(
        &h,
        tool_slot3,
        ctl_slot3,
        None,
        &[&unpair[..], &["--json", "--yes"]].concat(),
        "",
        false,
    );
    same_with(&h, tool_slot3, ctl_slot3, None, &unpair, "", false);
    same_with(
        &h,
        tool_slot3,
        ctl_slot3,
        None,
        &[&unpair[..], &["--mouse=1"]].concat(),
        "",
        false,
    );
    let out = same(&h, two_routes, None, &["receiver", "unpair", "1", "--yes"]);
    assert_eq!(out.code, 16);
    same(&h, Vec::new, None, &["receiver", "unpair", "1", "--yes"]);
    // Only the unpair request was sent.
    let nodes = ctl_slot3();
    h.plug.set(nodes.clone());
    assert_eq!(h.ctl(&["receiver", "unpair", "3", "--yes"]).code, 0);
    assert_eq!(nodes[0].sets(), [UNPAIR_3.to_vec()]);
}

#[test]
fn unpair_checks_the_value_that_was_shown() {
    let h = Harness::new();
    let nodes = vec![management([&[None], &[None], &[None], &[Some(A)], &[None]])];
    h.plug.set(nodes.clone());
    let mut other = vec![0x46, 0x59];
    other.extend(B);
    let json = h
        .call(
            "ReceiverUnpair",
            &[
                ("slot", Variant::from(3u8)),
                ("expected", Variant::from(other)),
                ("receiver", Variant::from("recv:port-1-4")),
            ],
        )
        .unwrap();
    assert_eq!(json["exit_code"], 16, "{json}");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("changed after it was shown")
    );
    assert!(nodes[0].sets().is_empty());
}

#[test]
fn arguments_are_checked() {
    let h = Harness::new();
    let invalid = "org.freedesktop.DBus.Error.InvalidArgs";
    assert_eq!(
        h.call("Check", &[("config", Variant::from("rel.toml"))])
            .unwrap_err(),
        invalid
    );
    assert_eq!(
        h.call("List", &[("mouse", Variant::from("1"))])
            .unwrap_err(),
        invalid
    );
    assert_eq!(
        h.call("ReceiverUnpair", &[("slot", Variant::from(3u8))])
            .unwrap_err(),
        invalid
    );
    let json = h
        .call("Apply", &[("route", Variant::from("bluetooth"))])
        .unwrap();
    assert_eq!(json["exit_code"], 2);
    let json = h.call("Cancel", &[]).unwrap();
    assert_eq!(json["cancelled"], false);
}

// --- one write at a time, Cancel and callers that leave ---

fn real_clock() -> Harness {
    Harness::start(Start {
        clock: Arc::new(SystemClock::default()),
        wait: true,
    })
}

fn slow_pair(h: &Harness) -> (FakeNode, JoinHandle<Output>) {
    let node = management([&[None]; 5]);
    h.plug.set(vec![node.clone()]);
    let address = h.bus.address.clone();
    let dir = h.dir.path().to_path_buf();
    let signals = Arc::clone(&h.signals);
    let pair = std::thread::spawn(move || {
        ctl(
            &address,
            &dir,
            &signals,
            &["receiver", "pair", "--timeout=20", "--poll-interval=0.05"],
            "",
            false,
        )
    });
    h.wait_for(|h| h.property::<String>("Busy").as_deref() == Some("receiver pair"));
    (node, pair)
}

#[test]
fn one_write_at_a_time_and_cancel() {
    let h = real_clock();
    h.write_config(&baseline());
    let (node, pair) = slow_pair(&h);

    let out = h.ctl(&["apply"]);
    assert_eq!(out.code, 22, "{out:?}");
    assert!(
        out.stderr.contains("cadratd is running `receiver pair`"),
        "{out:?}"
    );
    let json = h.ctl(&["apply", "--json"]).json();
    assert_eq!(json["error"]["code"], "Busy");
    // Reading is not blocked.
    assert_eq!(h.ctl(&["list"]).code, 0);
    assert_eq!(h.ctl(&["receiver", "slots"]).code, 0);
    assert_eq!(h.ctl(&["set", "mouse.dpi=800", "--dry-run"]).code, 0);

    h.signals.raise();
    let out = pair.join().unwrap();
    assert_eq!(out.code, 12, "{out:?}");
    assert!(out.stderr.contains("put the mouse in pairing mode"));
    assert!(out.stderr.contains("interrupted; pairing mode was stopped"));
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    assert_eq!(h.property::<String>("Busy").as_deref(), Some(""));
}

#[test]
fn pair_stops_when_the_caller_leaves() {
    let h = real_clock();
    let node = management([&[None]; 5]);
    h.plug.set(vec![node.clone()]);
    let conn = h.conn();
    let caller = conn.clone();
    let proxy = h.proxy(&conn);
    let call = std::thread::spawn(move || {
        let args: HashMap<&str, Variant<'_>> = HashMap::from([
            ("timeout_ms", Variant::from(20_000u32)),
            ("poll_interval_ms", Variant::from(50u32)),
        ]);
        let _ = proxy.call_method("ReceiverPair", &(args,));
    });
    h.wait_for(|h| h.property::<String>("Busy").as_deref() == Some("receiver pair"));
    caller.close().unwrap();
    drop(conn);
    h.wait_for(|h| h.property::<String>("Busy").as_deref() == Some(""));
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    let _ = call.join();
}

#[test]
fn stopping_cadratd_stops_a_pair() {
    let mut h = real_clock();
    let (node, pair) = slow_pair(&h);
    assert_eq!(h.stop_daemon(), 0);
    let out = pair.join().unwrap();
    assert_eq!(out.code, 12, "{out:?}");
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
}

#[test]
fn pairing_started_goes_only_to_the_caller() {
    let h = real_clock();
    let watcher = h.conn();
    let signals = zbus::blocking::MessageIterator::for_match_rule(
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .member("PairingStarted")
            .unwrap()
            .build(),
        &watcher,
        None,
    )
    .unwrap();
    let (_, pair) = slow_pair(&h);
    h.signals.raise();
    let out = pair.join().unwrap();
    assert!(out.stderr.contains("put the mouse in pairing mode"));
    // Nothing reached the other client.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for message in signals {
            let _ = tx.send(message.is_ok());
        }
    });
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}

// --- starting, devices, versions and the log ---

#[test]
fn requests_wait_for_cadrat_tool_at_start() {
    let mut h = Harness::start(Start {
        wait: false,
        ..Start::default()
    });
    // Restart cadratd while cadratd.lock is held as cadrat-tool holds it
    // while it writes.
    h.stop_daemon();
    let (_, file) = h.daemon_lock().open().unwrap();
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockShared).unwrap();
    h.stop = Arc::new(TestStop::new());
    h.daemon = Some(h.spawn_daemon(Arc::new(FakeClock::default())));
    h.wait_for(|h| h.property::<bool>("Ready") == Some(false));
    h.write_config(&baseline());

    let out = h.ctl(&["apply"]);
    assert_eq!(out.code, 22, "{out:?}");
    assert!(out.stderr.contains("waits for cadrat-tool"), "{out:?}");
    assert_eq!(h.ctl(&["list"]).code, 22);
    assert_eq!(h.ctl(&["get", "mouse.dpi"]).code, 0);
    assert_eq!(h.ctl(&["apply", "--dry-run"]).code, 0);

    drop(file);
    h.wait_for(|h| h.property::<bool>("Ready") == Some(true));
    assert_eq!(h.ctl(&["list"]).code, 0);
}

#[test]
fn devices_follow_dev() {
    let h = Harness::new();
    let devices =
        || -> Value { serde_json::from_str(&h.property::<String>("Devices").unwrap()).unwrap() };
    assert_eq!(devices()["mice"], serde_json::json!([]));

    let conn = h.conn();
    let properties = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
        .destination(cadrat_dbus::BUS_NAME)
        .unwrap()
        .path(cadrat_dbus::PATH)
        .unwrap()
        .build()
        .unwrap();
    let changes = properties.receive_properties_changed().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for change in changes {
            let args = change.args().unwrap();
            if let Some(value) = args.changed_properties().get("Devices") {
                let text: String = value.try_clone().unwrap().try_into().unwrap();
                let _ = tx.send(text);
            }
        }
    });

    h.plug.set(vec![wired()]);
    fs::write(h.dir.path().join("dev/hidraw5"), "").unwrap();
    let text = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("PropertiesChanged");
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["mice"][0]["key"], "c658:0a1b2c3d4e5f");
    assert_eq!(devices(), value);

    // Unrelated files change nothing.
    fs::write(h.dir.path().join("dev/tty1"), "").unwrap();
    h.plug.set(vec![]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(devices(), value);
    fs::remove_file(h.dir.path().join("dev/hidraw5")).unwrap();
    let text = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("PropertiesChanged");
    assert!(text.contains("\"mice\":[]"));
}

#[test]
fn only_one_cadratd_per_bus() {
    let h = Harness::new();
    let log = SharedLog::default();
    let world = cadratd::World {
        system: Arc::new(Plug::default()),
        clock: Arc::new(FakeClock::default()),
        xdg_config_home: None,
        home: None,
        lock_timeout: Duration::from_millis(100),
        daemon_lock: h.daemon_lock(),
        dev: h.dir.path().join("dev"),
        bus: Bus::Address(h.bus.address.clone()),
        start_wait: Duration::from_millis(100),
        settle: Duration::from_millis(50),
    };
    let stop = TestStop::new();
    assert_eq!(cadratd::run(world, &stop, Box::new(log.clone())), 1);
    assert!(log.text().contains("already running"), "{}", log.text());
}

#[test]
fn the_log_hides_device_ids() {
    let mut h = Harness::new();
    h.plug.set(two_routes());
    h.write_config(&baseline());
    assert_eq!(h.ctl(&["set", "mouse.dpi=800"]).code, 0);
    assert_eq!(h.ctl(&["receiver", "slots"]).code, 0);
    assert_eq!(h.stop_daemon(), 0);
    let log = h.log.text();
    assert!(log.contains("set: Success, sent via wired, saved"), "{log}");
    assert!(log.contains("receiver slots: Success"), "{log}");
    assert!(log.contains("stopped"), "{log}");
    assert!(!log.contains("0a1b2c3d4e5f"), "{log}");
}

/// An older `cadratd` that knows none of today's methods.
struct Old;

#[zbus::interface(name = "cc.nejiman10.Cadrat1.Manager")]
impl Old {
    #[zbus(property)]
    #[allow(clippy::unused_self)]
    fn version(&self) -> String {
        "0.0.1".to_owned()
    }
}

#[test]
fn another_version_of_cadratd() {
    let bus = PrivateBus::start();
    let _old = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(cadrat_dbus::BUS_NAME)
        .unwrap()
        .serve_at(cadrat_dbus::PATH, Old)
        .unwrap()
        .build()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = ctl(
        &bus.address,
        dir.path(),
        &Arc::default(),
        &["list"],
        "",
        false,
    );
    assert_eq!(out.code, 21, "{out:?}");
    assert!(
        out.stderr
            .contains("warning: cadratd 0.0.1 is running, but cadratctl is")
    );
    assert!(
        out.stderr
            .contains("systemctl --user restart cadratd.service")
    );
    let json = ctl(
        &bus.address,
        dir.path(),
        &Arc::default(),
        &["list", "--json"],
        "",
        false,
    )
    .json();
    assert_eq!(json["error"]["code"], "DaemonUnavailable");
}

#[test]
fn no_cadratd_is_unavailable() {
    let bus = PrivateBus::start();
    let dir = tempfile::tempdir().unwrap();
    let out = ctl(
        &bus.address,
        dir.path(),
        &Arc::default(),
        &["list"],
        "",
        false,
    );
    assert_eq!(out.code, 21, "{out:?}");
    assert!(out.stderr.contains("cannot reach cadratd"), "{out:?}");
    // Usage errors are found before calling.
    let out = ctl(
        &bus.address,
        dir.path(),
        &Arc::default(),
        &["set", "mouse.color=red"],
        "",
        false,
    );
    assert_eq!(out.code, 2, "{out:?}");
}
