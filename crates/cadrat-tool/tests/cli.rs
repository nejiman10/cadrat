//! The whole CLI against the fake transport, in a temporary configuration
//! directory (spec implementation §4, CLI layer).

#![allow(missing_docs)]

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cadrat_config::{ConfigLock, Preset, template};
use cadrat_hidraw::Clock;
use cadrat_hidraw::fake::{FakeClock, FakeNode, FakeSystem, probe_response};
use cadrat_tool::{DaemonLock, Env, Interrupt, Io, run};
use serde_json::Value;

// Synthetic device IDs; not taken from any device.
const A: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];
const B: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
const EIO: i32 = 5;
const EPIPE: i32 = 32;
const EACCES: i32 = 13;

#[derive(Default)]
struct FakeInterrupt {
    armed: Cell<bool>,
    set: Arc<AtomicBool>,
}

impl Interrupt for FakeInterrupt {
    fn arm(&self) {
        self.armed.set(true);
    }
    fn disarm(&self) {
        self.armed.set(false);
    }
    fn is_set(&self) -> bool {
        self.armed.get() && self.set.load(Ordering::SeqCst)
    }
}

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

struct Harness {
    dir: tempfile::TempDir,
    system: FakeSystem,
    clock: FakeClock,
    interrupt: FakeInterrupt,
    uid: Cell<u32>,
}

impl Harness {
    fn new(nodes: Vec<FakeNode>) -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            system: FakeSystem::new(nodes),
            clock: FakeClock::default(),
            interrupt: FakeInterrupt::default(),
            uid: Cell::new(1000),
        }
    }

    /// `/run/user` lives in the temporary directory; it does not exist
    /// until a test creates it, so cadratd is not running by default.
    fn daemon_lock(&self) -> DaemonLock {
        DaemonLock {
            run_user: self.dir.path().join("run"),
            uid: self.uid.get(),
        }
    }

    /// The default configuration path under the fake `XDG_CONFIG_HOME`.
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

    fn run_with(&self, args: &[&str], stdin: &str, terminal: bool) -> Output {
        let env = Env {
            system: &self.system,
            clock: &self.clock,
            interrupt: &self.interrupt,
            xdg_config_home: Some(self.dir.path().into()),
            home: None,
            lock_timeout: Duration::from_millis(100),
            daemon_lock: Some(self.daemon_lock()),
        };
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut input = stdin.as_bytes();
        let io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
            stdin: &mut input,
            stdin_is_terminal: terminal,
        };
        let args = std::iter::once("cadrat-tool")
            .chain(args.iter().copied())
            .map(Into::into);
        let code = run(args, &env, io);
        Output {
            code,
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, "", false)
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        write!(s, "{b:02x}").unwrap();
        s
    })
}

fn baseline() -> String {
    template(Preset::ResearchBaseline)
}

fn wired() -> FakeNode {
    FakeNode::wired("hidraw5", "3-2", A)
}

fn wire_hex(dpi: u16, radial: &str) -> String {
    let mut config = cadrat_config::Config::research_baseline();
    config.dpi = cadrat_proto::Dpi::new(dpi).unwrap();
    config
        .buttons
        .set(cadrat_proto::ButtonName::Radial, radial.parse().unwrap());
    hex(&config.to_report().to_wire())
}

// --- init, get, check ---

#[test]
fn init_creates_and_protects_the_file() {
    let h = Harness::new(vec![]);
    let out = h.run(&["init"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.starts_with("created "));
    assert_eq!(h.read_config(), template(Preset::Empty));

    let out = h.run(&["init", "--preset", "research-baseline"]);
    assert_eq!(out.code, 10);
    assert!(out.stderr.contains("--force"));
    assert_eq!(h.read_config(), template(Preset::Empty));

    let out = h.run(&["init", "--preset=research-baseline", "--force"]);
    assert_eq!(out.code, 0);
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn get_values() {
    let h = Harness::new(vec![]);
    h.write_config(&baseline());
    let out = h.run(&["get"]);
    assert_eq!(out.code, 0);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 12);
    assert_eq!(lines[0], "mouse.dpi=1400");
    assert_eq!(lines[11], "buttons.radial=mouse:middle");

    let out = h.run(&["get", "mouse.dpi", "buttons.back"]);
    assert_eq!(out.stdout, "mouse.dpi=1400\nbuttons.back=mouse:backward\n");
    let out = h.run(&["get", "-n", "mouse.dpi"]);
    assert_eq!(out.stdout, "1400\n");

    let out = h.run(&["get", "--wire"]);
    assert!(out.stdout.contains(&format!(
        "wire    {}",
        wire_hex(1400, "mouse:middle")
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(" ")
    )));
    assert!(out.stdout.contains("buttons.radial=mouse:middle"));

    let out = h.run(&["get", "mouse.color"]);
    assert_eq!(out.code, 2);
}

#[test]
fn get_incomplete_shows_what_it_can() {
    let h = Harness::new(vec![]);
    h.write_config("schema = 1\n[mouse]\ndpi = 800\n");
    let out = h.run(&["get"]);
    assert_eq!(out.code, 3);
    assert_eq!(out.stdout, "mouse.dpi=800\n");
    assert!(out.stderr.contains("hint: mouse.wheel is missing"));
    assert!(out.stderr.contains("hint: buttons.radial is missing"));

    let json = h.run(&["get", "--json"]).json();
    assert_eq!(json["exit_code"], 3);
    assert_eq!(json["values"]["mouse.dpi"], 800);
    assert_eq!(json["error"]["details"]["kind"], "ConfigIncomplete");
}

#[test]
fn check_reports_problems_and_warnings() {
    let h = Harness::new(vec![]);
    let out = h.run(&["check"]);
    assert_eq!(out.code, 3);
    assert!(out.stderr.contains("cadrat-tool init"));

    h.write_config(&baseline().replace("dpi = 1400", "dpi = 1425"));
    let out = h.run(&["check"]);
    assert_eq!(out.code, 3);
    assert!(out.stderr.contains("multiple of 50"));

    h.write_config(&baseline().replace("radial  = \"mouse:middle\"", "radial  = \"host:9\""));
    let out = h.run(&["check"]);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("warning: W-HOST-UNOBSERVABLE"));

    let other = h.dir.path().join("none.toml");
    let out = h.run(&["check", "--config", other.to_str().unwrap()]);
    assert_eq!(out.code, 3);
}

// --- set and apply ---

#[test]
fn set_sends_then_saves_only_changed_values() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000", "buttons.radial=host:1"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout
            .contains("mouse   1  c658:0a1b2c3d4e5f  via wired (MI_01)")
    );
    assert!(out.stdout.contains("change  mouse.dpi=1400 → 1000"));
    assert!(out.stdout.contains("saved   "));

    let expected = wire_hex(1000, "host:1");
    let sent: Vec<String> = node.sets().iter().map(|s| hex(s)).collect();
    assert_eq!(sent, [expected]);
    assert_eq!(
        h.read_config(),
        baseline()
            .replacen("dpi = 1400 ", "dpi = 1000 ", 1)
            .replacen("radial  = \"mouse:middle\"", "radial  = \"host:1\"", 1)
    );
}

#[test]
fn set_json() {
    let h = Harness::new(vec![wired()]);
    h.write_config(&baseline());
    let json = h.run(&["set", "mouse.dpi=1000", "--json"]).json();
    assert_eq!(json["format"], 1);
    assert_eq!(json["command"], "set");
    assert_eq!(json["ok"], true);
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["sent"], true);
    assert_eq!(json["saved"], true);
    assert_eq!(json["changes"][0]["from"], 1400);
    assert_eq!(json["changes"][0]["to"], 1000);
    assert_eq!(json["mouse"]["sent_via"]["route"], "wired");
    assert_eq!(json["wire_hex"], wire_hex(1000, "mouse:middle"));
    assert_eq!(json["error"], Value::Null);
}

#[test]
fn set_dry_run_touches_nothing() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    let out = h.run(&["set", "--dry-run", "mouse.dpi=1000"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("change  mouse.dpi=1400 → 1000"));
    assert!(out.stdout.contains("wire    10 00 14"));
    assert!(node.log().is_empty(), "no device I/O at all");
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn set_no_save() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    let out = h.run(&["set", "--no-save", "mouse.dpi=1000"]);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("warning: W-NOT-SAVED"));
    assert_eq!(node.sets().len(), 1);
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn send_failure_leaves_the_file() {
    let h = Harness::new(vec![wired().set(&[Err(EIO)])]);
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 8);
    assert!(out.stderr.contains("errno: EIO"));
    assert_eq!(h.read_config(), baseline());

    let h = Harness::new(vec![wired().set(&[Ok(12)])]);
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 8);
    assert!(out.stderr.contains("short-write: 12"));
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn concurrent_edit_is_not_overwritten() {
    let h = Harness::new(vec![]);
    let path = h.config();
    let edited = baseline() + "# edited in an editor\n";
    let for_hook = edited.clone();
    let node = wired().on_set(move |_| fs::write(&path, &for_hook).unwrap());
    let h = Harness {
        system: FakeSystem::new(vec![node]),
        ..h
    };
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 9, "{}", out.stderr);
    assert!(out.stderr.contains("sent but not saved"));
    assert!(
        out.stderr
            .contains("hint: changes to record in the file: mouse.dpi=1000")
    );
    assert!(out.stderr.contains("hint: sent wire report: 10 00 14"));
    assert_eq!(h.read_config(), edited);
}

#[test]
fn lock_contention() {
    let h = Harness::new(vec![wired()]);
    h.write_config(&baseline());
    let _lock = ConfigLock::acquire(&h.config(), Duration::ZERO).unwrap();
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 11);
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn target_changed() {
    let node = FakeNode::wired("hidraw5", "3-2", A)
        .get(0x08, &[Ok(probe_response(A)), Ok(probe_response(B))]);
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 19);
    assert!(node.sets().is_empty());
    assert_eq!(h.read_config(), baseline());
}

#[test]
fn selection_errors() {
    let run = |nodes: Vec<FakeNode>, args: &[&str]| {
        let h = Harness::new(nodes);
        h.write_config(&baseline());
        let out = h.run(args);
        assert_eq!(h.read_config(), baseline());
        out
    };
    assert_eq!(run(vec![], &["apply"]).code, 4);
    let two = || vec![wired(), FakeNode::wired("hidraw7", "3-3", B)];
    let out = run(two(), &["apply"]);
    assert_eq!(out.code, 5);
    assert!(out.stderr.contains("c658:0a1b2c3d4e5f, c658:112233445566"));
    assert_eq!(run(two(), &["apply", "--mouse=2"]).code, 0);
    assert_eq!(run(two(), &["apply", "--mouse", "c658:0a"]).code, 0);
    let mut blocked = wired();
    blocked.open_error = Some(EACCES);
    let out = run(vec![blocked], &["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 6);
    assert!(out.stderr.contains("69-cadrat.rules"));
    let mut second = FakeNode::wired("hidraw6", "3-2", A);
    second.info.interface = Some(2);
    assert_eq!(run(vec![wired(), second], &["apply"]).code, 7);
    assert_eq!(
        run(vec![wired()], &["apply", "--hidraw", "/dev/hidraw9"]).code,
        7
    );
    assert_eq!(
        run(vec![wired()], &["apply", "--hidraw", "/dev/hidraw5"]).code,
        0
    );
    assert_eq!(
        run(vec![wired()], &["apply", "--route", "receiver"]).code,
        4
    );
}

#[test]
fn usage_errors() {
    let h = Harness::new(vec![wired()]);
    h.write_config(&baseline());
    for args in [
        &["set", "mouse.dpi"][..],
        &["set", "mouse.dpi=1000", "mouse.dpi=1200"],
        &["set", "mouse.dpi=1025"],
        &["set", "mouse.colour=1"],
        &["set", "--dry-run", "--no-save", "mouse.dpi=1000"],
        &["set"],
        &["apply", "--mouse=1", "--hidraw=/dev/hidraw5"],
        &["list", "-q", "-v"],
        &["frobnicate"],
    ] {
        let out = h.run(args);
        assert_eq!(out.code, 2, "{args:?}: {}", out.stderr);
    }
    assert_eq!(h.read_config(), baseline());
    let json = h.run(&["set", "--json", "mouse.dpi"]).json();
    assert_eq!(json["exit_code"], 2);
    assert_eq!(json["error"]["code"], "Usage");
}

#[test]
fn apply_sends_the_file_as_is() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(
        &baseline()
            .replace("dpi = 1400", "dpi = 1000")
            .replace("radial  = \"mouse:middle\"", "radial  = \"host:1\""),
    );
    let before = h.read_config();
    let out = h.run(&["apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(!out.stderr.contains("W-NOT-SAVED"));
    assert_eq!(node.sets().len(), 1);
    assert_eq!(h.read_config(), before);

    let out = h.run(&["apply", "--dry-run"]);
    assert_eq!(out.code, 0);
    assert_eq!(node.sets().len(), 1);

    let h = Harness::new(vec![wired()]);
    assert_eq!(h.run(&["apply"]).code, 3);
    h.write_config("schema = 1\n");
    assert_eq!(h.run(&["apply"]).code, 3);
}

/// Mouse A connected only through a Receiver.
fn receiver_only() -> Vec<FakeNode> {
    let mut nodes = two_routes();
    nodes.remove(0);
    nodes
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

#[test]
fn standby_route_note() {
    let h = Harness::new(two_routes());
    h.write_config(&baseline());
    let out = h.run(&["apply"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains(
        "note    the receiver route is on standby; run `cadrat-tool apply` after switching modes"
    ));
    let out = h.run(&["apply", "-q"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");

    let out = h.run(&["apply", "--route=receiver"]);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("warning: W-INACTIVE-ROUTE"));
    assert!(out.stdout.contains("via receiver (MI_03)"));
    assert!(
        out.stdout
            .contains("if nothing changes, run the same command again")
    );
}

#[test]
fn resend_note_only_for_receiver_sends() {
    let h = Harness::new(two_routes());
    h.write_config(&baseline());
    let out = h.run(&["apply"]);
    assert_eq!(out.code, 0);
    assert!(!out.stdout.contains("through the Receiver"));

    let h = Harness::new(receiver_only());
    h.write_config(&baseline());
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains(
        "note    a send through the Receiver can take about 30 s to show and is sometimes lost;\n        \
         if nothing changes, run the same command again\n"
    ));
    let out = h.run(&["apply", "-q"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");
}

// --- list ---

#[test]
fn list_shows_one_mouse_over_two_routes() {
    let h = Harness::new(two_routes());
    let out = h.run(&["list"]);
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stdout,
        "\
#  MOUSE              ACTIVE    ROUTES
1  c658:0a1b2c3d4e5f  wired     wired (/dev/hidraw5, MI_01)
                                receiver standby (recv:port-1-4 slot 3, /dev/hidraw9, MI_03)

RECEIVER       SLOTS  MANAGEMENT
recv:port-1-4  1/5    /dev/hidraw6 (MI_00)
"
    );
    let out = h.run(&["list", "--redact", "--nodes"]);
    assert!(!out.stdout.contains("0a1b2c3d4e5f"), "{}", out.stdout);
    assert!(out.stdout.contains("c658:id-1"));
    assert!(
        out.stdout
            .contains("/dev/hidraw6  MI_00  256f:c652  candidate (management)")
    );

    let json = h.run(&["list", "--json"]).json();
    assert_eq!(json["mice"][0]["key"], "c658:0a1b2c3d4e5f");
    assert_eq!(json["mice"][0]["routes"][1]["state"], "standby");
    assert_eq!(json["receivers"][0]["slots"][3]["mouse"]["number"], 1);
    assert!(json.get("nodes").is_none());
}

#[test]
fn list_columns_fit_long_keys() {
    let nodes = vec![FakeNode::receiver("hidraw9", "9-1.3.4", 0, None, true).slots([&[None]; 5])];
    let out = Harness::new(nodes).run(&["list"]);
    assert!(
        out.stdout.contains(
            "RECEIVER           SLOTS  MANAGEMENT\nrecv:port-9-1.3.4  0/5    /dev/hidraw9 (MI_00)\n"
        ),
        "{}",
        out.stdout
    );
}

#[test]
fn list_reports_inaccessible_nodes() {
    let mut blocked = wired();
    blocked.open_error = Some(EACCES);
    let h = Harness::new(vec![blocked]);
    let out = h.run(&["list", "--nodes"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("inaccessible (EACCES)"));
    assert!(out.stderr.contains("69-cadrat.rules"));
}

// --- receiver ---

fn management(slots: [&'static [Option<[u8; 6]>]; 5]) -> FakeNode {
    FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(slots)
}

const START: [u8; 5] = [0x41, 0x02, 0x02, 0x00, 0x00];
const STOP: [u8; 5] = [0x41, 0x02, 0x00, 0x00, 0x00];

#[test]
fn receiver_slots() {
    let h = Harness::new(two_routes());
    let out = h.run(&["receiver", "slots"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout
            .starts_with("receiver recv:port-1-4  (/dev/hidraw6, MI_00)\nslot 0  empty\n")
    );
    assert!(
        out.stdout.contains(
            "slot 3  occupied  type 0x59  id 0a1b2c3d4e5f   → mouse 1 (c658:0a1b2c3d4e5f)"
        )
    );
    let out = h.run(&["receiver", "slots", "--redact"]);
    assert!(!out.stdout.contains("0a1b2c3d4e5f"));
    assert_eq!(h.run(&["receiver", "slots", "--mouse=1"]).code, 2);
    assert_eq!(Harness::new(vec![]).run(&["receiver", "slots"]).code, 4);

    let bad = management([&[None], &[None], &[None], &[None], &[None]]).get(
        0x44,
        &[
            Ok(vec![0x44, 0, 0, 0, 0, 0, 0, 0]),
            Ok(vec![0x43, 0, 0, 0, 0, 0, 0, 0]),
        ],
    );
    assert_eq!(Harness::new(vec![bad]).run(&["receiver", "slots"]).code, 17);
}

#[test]
fn receiver_pair() {
    // Enumeration reads the slots once, pair reads them before starting,
    // then slot 2 becomes occupied on the second poll.
    let node = management([
        &[None],
        &[None],
        &[None, None, None, None, Some(B)],
        &[None],
        &[None],
    ]);
    let h = Harness::new(vec![node.clone()]);
    let out = h.run(&["receiver", "pair"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("paired  slot 2"));
    assert!(
        out.stdout
            .contains("the first send after pairing is sometimes lost")
    );
    assert!(out.stderr.contains("put the mouse in pairing mode"));
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);

    let node = management([&[None]; 5]);
    let h = Harness::new(vec![node.clone()]);
    let json = h.run(&["receiver", "pair", "--timeout=3", "--json"]).json();
    assert_eq!(json["exit_code"], 12);
    assert_eq!(json["stop_sent"], true);
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);

    let node = management([&[None]; 5]).set(&[Ok(5), Err(EIO)]);
    let out = Harness::new(vec![node]).run(&["receiver", "pair", "--timeout=2"]);
    assert_eq!(out.code, 13);
    assert!(out.stderr.contains("unplug the Receiver"));

    let node = management([&[None]; 5]).set(&[Err(EIO), Ok(5)]);
    assert_eq!(Harness::new(vec![node]).run(&["receiver", "pair"]).code, 14);
    assert_eq!(
        Harness::new(vec![management([&[None]; 5])])
            .run(&["receiver", "pair", "--timeout=0"])
            .code,
        2
    );
}

#[test]
fn receiver_pair_interrupted() {
    let node = management([&[None]; 5]);
    let h = Harness::new(vec![node.clone()]);
    let flag = Arc::clone(&h.interrupt.set);
    h.clock.on_sleep(move |now| {
        if now >= Duration::from_secs(3) {
            flag.store(true, Ordering::SeqCst);
        }
    });
    let out = h.run(&["receiver", "pair"]);
    assert_eq!(out.code, 12);
    assert!(out.stderr.contains("interrupted"));
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
    assert!(!h.interrupt.armed.get());
}

const UNPAIR_3: [u8; 5] = [0x41, 0x04, 0x03, 0x00, 0x00];

/// Slot 3 holds A: read by enumeration, then by unpair twice, then empty.
fn occupied_slot3() -> FakeNode {
    management([
        &[None],
        &[None],
        &[None],
        &[Some(A), Some(A), Some(A), None],
        &[None],
    ])
}

#[test]
fn unpair_with_confirmation() {
    let node = occupied_slot3();
    let h = Harness::new(vec![node.clone()]);
    let out = h.run_with(&["receiver", "unpair", "3"], "y\n", true);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stderr
            .contains("slot 3  occupied  type 0x59  id 0a1b2c3d4e5f")
    );
    assert!(out.stderr.contains("Unpair slot 3? [y/N]"));
    assert!(out.stdout.contains("unpaired slot 3"));
    assert_eq!(node.sets(), [UNPAIR_3.to_vec()]);

    let node = occupied_slot3();
    let h = Harness::new(vec![node.clone()]);
    let out = h.run_with(&["receiver", "unpair", "3"], "n\n", true);
    assert_eq!(out.code, 18);
    assert!(node.sets().is_empty());

    let node = occupied_slot3();
    let h = Harness::new(vec![node.clone()]);
    let out = h.run_with(&["receiver", "unpair", "3"], "", true);
    assert_eq!(out.code, 18);
    assert!(node.sets().is_empty());
}

#[test]
fn unpair_without_terminal_needs_yes() {
    let node = occupied_slot3();
    let h = Harness::new(vec![node.clone()]);
    assert_eq!(h.run(&["receiver", "unpair", "3"]).code, 2);
    assert_eq!(
        h.run_with(&["receiver", "unpair", "3", "--json"], "y\n", true)
            .code,
        2
    );
    assert!(node.log().is_empty());
    let out = h.run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(node.sets(), [UNPAIR_3.to_vec()]);
}

#[test]
fn unpair_outcomes() {
    let node = occupied_slot3().set(&[Err(EPIPE)]);
    let out = Harness::new(vec![node]).run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("warning: W-UNPAIR-EPIPE"));

    let node = management([&[None], &[None], &[None], &[Some(A)], &[None]]).set(&[Err(EPIPE)]);
    let out = Harness::new(vec![node]).run(&["receiver", "unpair", "3", "--yes", "--timeout=2"]);
    assert_eq!(out.code, 15);

    let node = management([&[None], &[None], &[None], &[Some(A)], &[None]]).set(&[Err(EIO)]);
    assert_eq!(
        Harness::new(vec![node])
            .run(&["receiver", "unpair", "3", "--yes"])
            .code,
        14
    );

    let node = management([&[None]; 5]);
    let out = Harness::new(vec![node.clone()]).run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 16);
    assert!(node.sets().is_empty());

    let node = management([
        &[None],
        &[None],
        &[None],
        &[Some(A), Some(A), Some(B)],
        &[None],
    ]);
    let out = Harness::new(vec![node.clone()]).run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 16);
    assert!(out.stderr.contains("changed after it was shown"));
    assert!(node.sets().is_empty());

    assert_eq!(
        Harness::new(vec![])
            .run(&["receiver", "unpair", "5", "--yes"])
            .code,
        2
    );
}

#[test]
fn unpair_warns_when_receiver_is_the_only_route() {
    let nodes = vec![
        occupied_slot3(),
        FakeNode::receiver("hidraw9", "1-4", 3, Some(A), false),
    ];
    let out = Harness::new(nodes).run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stderr.contains("connected only through this Receiver"));
    assert!(out.stderr.contains("→ mouse 1 (c658:0a1b2c3d4e5f)"));
}

#[test]
fn two_receivers_need_a_key() {
    let nodes = vec![
        management([&[None]; 5]),
        FakeNode::receiver("hidraw12", "1-5", 0, None, true).slots([&[None]; 5]),
    ];
    let h = Harness::new(nodes);
    let out = h.run(&["receiver", "slots"]);
    assert_eq!(out.code, 5);
    let out = h.run(&["receiver", "slots", "--receiver=recv:port-1-5"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("/dev/hidraw12"));
}

#[test]
fn help_and_version() {
    let h = Harness::new(vec![]);
    let out = h.run(&["--help"]);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("Usage: cadrat-tool"));
    assert_eq!(h.run(&["--version"]).code, 0);
}

/// Hardware test run 1, F6′: after the unpair request the management node
/// answered ENODEV until it was opened again.
#[test]
fn unpair_survives_a_vanished_management_node() {
    let node = management([
        &[None],
        &[None],
        &[None],
        &[Some(A), Some(A), Some(A)],
        &[None],
    ])
    .get(
        0x46,
        &[
            Ok(cadrat_hidraw::fake::slot_response(3, Some(A))),
            Ok(cadrat_hidraw::fake::slot_response(3, Some(A))),
            Ok(cadrat_hidraw::fake::slot_response(3, Some(A))),
            Err(19),
            Ok(cadrat_hidraw::fake::slot_response(3, None)),
        ],
    );
    let out = Harness::new(vec![node.clone()]).run(&["receiver", "unpair", "3", "--yes"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stderr.contains("warning: W-MANAGEMENT-REOPENED"),
        "{}",
        out.stderr
    );
    assert!(out.stdout.contains("unpaired slot 3"));
    assert_eq!(node.sets(), [UNPAIR_3.to_vec()]);
}

// --- hold-open ---

/// Raises the interrupt after `polls` sleeps.
fn interrupt_after(h: &Harness, polls: u32) {
    let set = Arc::clone(&h.interrupt.set);
    let mut count = 0;
    h.clock.on_sleep(move |_| {
        count += 1;
        if count == polls {
            set.store(true, Ordering::SeqCst);
        }
    });
}

#[test]
fn hold_open_holds_until_interrupted() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    interrupt_after(&h, 3);
    let out = h.run(&["hold-open", "--poll-interval=2"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "held      /dev/hidraw5 (MI_01)\nreleased  /dev/hidraw5\n"
    );
    assert!(
        out.stderr
            .contains("keeping the wired C658's hidraw nodes open")
    );
    assert_eq!(h.clock.now(), Duration::from_secs(6));
    assert_eq!(node.open_count(), 0);
    assert!(node.log().is_empty(), "hold-open must not send anything");

    // -q drops the note but keeps the log lines.
    let h = Harness::new(vec![wired()]);
    interrupt_after(&h, 1);
    let out = h.run(&["hold-open", "-q"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stderr, "");
    assert!(out.stdout.starts_with("held "));
}

#[test]
fn hold_open_warns_once_about_an_inaccessible_node() {
    let mut node = wired();
    node.open_error = Some(EACCES);
    let h = Harness::new(vec![node]);
    interrupt_after(&h, 3);
    let out = h.run(&["hold-open"]);
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr.matches("warning: W-HOLD-OPEN-FAILED: cannot open /dev/hidraw5 (EACCES); check that the udev rule is installed").count(),
        1,
        "{}",
        out.stderr
    );
}

#[test]
fn hold_open_rejects_target_and_json_options() {
    let h = Harness::new(vec![wired()]);
    for args in [
        &["hold-open", "--json"][..],
        &["hold-open", "--mouse=1"],
        &["hold-open", "--hidraw=/dev/hidraw5"],
    ] {
        let out = h.run(args);
        assert_eq!(out.code, 2, "{args:?}");
    }
    assert_eq!(h.run(&["hold-open", "--poll-interval=0"]).code, 2);
}

// --- cadratd lock (spec tool/cli §8) and node write locks (spec device §7.2) ---

/// Takes `cadratd`'s exclusive lock for user 1000, as a running `cadratd`
/// would. The returned file holds it until dropped.
fn run_cadratd(h: &Harness) -> fs::File {
    let dir = h.dir.path().join("run/1000/cadrat");
    fs::create_dir_all(&dir).unwrap();
    let file = fs::File::create(dir.join("cadratd.lock")).unwrap();
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    file
}

/// Whether a `cadratd` starting now could take its lock.
fn cadratd_could_start(path: &std::path::Path) -> bool {
    let file = fs::File::open(path).unwrap();
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_ok()
}

#[test]
fn writes_stop_while_cadratd_runs() {
    let wired = wired();
    let receiver = occupied_slot3();
    let h = Harness::new(vec![wired.clone(), receiver.clone()]);
    h.write_config(&baseline());
    let _cadratd = run_cadratd(&h);

    for (args, instead) in [
        (
            &["set", "mouse.dpi=1000"][..],
            "cadratctl set mouse.dpi=1000",
        ),
        (&["apply"][..], "cadratctl apply"),
        (&["apply", "--hidraw=/dev/hidraw5"][..], "cadratctl apply"),
        (&["receiver", "pair"][..], "cadratctl receiver pair"),
        (
            &["receiver", "unpair", "3", "--yes"][..],
            "cadratctl receiver unpair 3",
        ),
    ] {
        let out = h.run(args);
        assert_eq!(out.code, 20, "{args:?}: {}", out.stderr);
        assert!(
            out.stderr
                .contains(&format!("cadratd is running; use `{instead}` instead")),
            "{}",
            out.stderr
        );
    }
    let json = h.run(&["apply", "--json"]).json();
    assert_eq!(json["error"]["code"], "DaemonRunning");
    assert!(wired.sets().is_empty());
    assert!(receiver.sets().is_empty());
    assert_eq!(h.read_config(), baseline());

    // Commands that do not write to a device still work.
    for args in [
        &["list"][..],
        &["get"],
        &["check"],
        &["set", "--dry-run", "mouse.dpi=1000"],
        &["apply", "--dry-run"],
        &["receiver", "slots"],
    ] {
        assert_eq!(h.run(args).code, 0, "{args:?}");
    }
}

#[test]
fn cadratd_waits_while_a_write_runs() {
    let h = Harness::new(vec![]);
    fs::create_dir_all(h.dir.path().join("run/1000")).unwrap();
    let lock = h.dir.path().join("run/1000/cadrat/cadratd.lock");
    let seen = Arc::new(Mutex::new(None));
    let during = Arc::clone(&seen);
    let path = lock.clone();
    let node = wired().on_set(move |_| *during.lock().unwrap() = Some(cadratd_could_start(&path)));
    let mut h = h;
    h.system = FakeSystem::new(vec![node.clone()]);
    h.write_config(&baseline());

    let out = h.run(&["apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        *seen.lock().unwrap(),
        Some(false),
        "cadratd started during the send"
    );
    assert!(cadratd_could_start(&lock), "the lock outlived the command");

    // The directory is private to the user.
    let permissions = fs::metadata(lock.parent().unwrap()).unwrap().permissions();
    let mode = std::os::unix::fs::PermissionsExt::mode(&permissions);
    assert_eq!(mode & 0o777, 0o700);
}

#[test]
fn no_runtime_directory_means_no_cadratd() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    assert_eq!(h.run(&["apply"]).code, 0);
    assert_eq!(node.sets().len(), 1);
    assert!(
        !h.dir.path().join("run").exists(),
        "/run/user/<uid> was created"
    );
}

#[test]
fn root_checks_every_users_cadratd() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    h.uid.set(0);
    // Another user without a lock file: nothing is created for them.
    fs::create_dir_all(h.dir.path().join("run/1001")).unwrap();
    assert_eq!(h.run(&["apply"]).code, 0);
    assert!(!h.dir.path().join("run/1001/cadrat").exists());

    let cadratd = run_cadratd(&h);
    let out = h.run(&["apply"]);
    assert_eq!(out.code, 20, "{}", out.stderr);
    assert!(out.stderr.contains("run/1000/cadrat/cadratd.lock"));
    assert_eq!(node.sets().len(), 1);

    drop(cadratd);
    assert_eq!(h.run(&["apply"]).code, 0);
}

#[test]
fn a_locked_node_is_busy() {
    let node = wired();
    let h = Harness::new(vec![node.clone()]);
    h.write_config(&baseline());
    node.lock_elsewhere();
    for args in [&["set", "mouse.dpi=1000"][..], &["apply"]] {
        let out = h.run(args);
        assert_eq!(out.code, 22, "{args:?}: {}", out.stderr);
        assert!(
            out.stderr
                .contains("another process is writing to /dev/hidraw5")
        );
    }
    assert!(node.sets().is_empty());
    assert_eq!(h.read_config(), baseline());
    assert_eq!(h.run(&["list"]).code, 0);

    node.unlock_elsewhere();
    assert_eq!(h.run(&["apply"]).code, 0);
    assert!(!node.is_locked(), "the lock outlived the command");
}

#[test]
fn a_locked_management_node_blocks_receiver_writes() {
    let nodes = receiver_only();
    let (management, setting) = (nodes[0].clone(), nodes[1].clone());
    let h = Harness::new(nodes);
    h.write_config(&baseline());
    management.lock_elsewhere();

    // A send through the Receiver locks its management node first.
    let out = h.run(&["set", "mouse.dpi=1000"]);
    assert_eq!(out.code, 22, "{}", out.stderr);
    assert!(out.stderr.contains("/dev/hidraw6"), "{}", out.stderr);
    assert!(setting.sets().is_empty());
    assert!(!setting.is_locked());
    assert_eq!(h.read_config(), baseline());
    let out = h.run(&["apply", "--hidraw=/dev/hidraw9"]);
    assert_eq!(out.code, 22, "{}", out.stderr);
    assert!(setting.sets().is_empty());

    assert_eq!(h.run(&["receiver", "pair"]).code, 22);
    assert_eq!(h.run(&["receiver", "unpair", "3", "--yes"]).code, 22);
    assert!(management.sets().is_empty());
    assert_eq!(h.run(&["receiver", "slots"]).code, 0);
    assert_eq!(h.run(&["list"]).code, 0);
}

/// Answers the unpair prompt, recording whether the node was locked then.
/// With `steal`, another process takes the node's lock at the prompt.
struct Answer {
    node: FakeNode,
    locked: Rc<Cell<Option<bool>>>,
    steal: bool,
    text: &'static [u8],
}

impl std::io::Read for Answer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.text.len().min(buf.len());
        buf[..n].copy_from_slice(&self.text[..n]);
        self.text = &self.text[n..];
        Ok(n)
    }
}

impl std::io::BufRead for Answer {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.locked.get().is_none() {
            self.locked.set(Some(self.node.is_locked()));
            if self.steal {
                self.node.lock_elsewhere();
            }
        }
        Ok(self.text)
    }
    fn consume(&mut self, n: usize) {
        self.text = &self.text[n..];
    }
}

fn unpair_answering(steal: bool) -> (i32, Option<bool>, FakeNode) {
    let node = occupied_slot3();
    let h = Harness::new(vec![node.clone()]);
    let locked = Rc::new(Cell::new(None));
    let mut answer = Answer {
        node: node.clone(),
        locked: Rc::clone(&locked),
        steal,
        text: b"y\n",
    };
    let env = Env {
        system: &h.system,
        clock: &h.clock,
        interrupt: &h.interrupt,
        xdg_config_home: Some(h.dir.path().into()),
        home: None,
        lock_timeout: Duration::from_millis(100),
        daemon_lock: None,
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
        stdin: &mut answer,
        stdin_is_terminal: true,
    };
    let args = ["cadrat-tool", "receiver", "unpair", "3"].map(Into::into);
    (run(args, &env, io), locked.get(), node)
}

#[test]
fn unpair_locks_only_after_confirmation() {
    // Not locked while the prompt waits.
    let (code, locked, node) = unpair_answering(false);
    assert_eq!(code, 0);
    assert_eq!(locked, Some(false));
    assert_eq!(node.sets(), [UNPAIR_3.to_vec()]);
    // The lock is taken after the answer: another writer that started
    // meanwhile makes it Busy, and nothing is sent.
    let (code, _, node) = unpair_answering(true);
    assert_eq!(code, 22);
    assert!(node.sets().is_empty());
}

#[test]
fn pair_stops_even_if_the_reopened_node_cannot_be_locked() {
    // Slot 0 answers ENODEV after pairing starts: the node is reopened,
    // but another process has taken its lock by then.
    let node = management([&[None]; 5]).get(
        0x43,
        &[
            Ok(cadrat_hidraw::fake::slot_response(0, None)),
            Ok(cadrat_hidraw::fake::slot_response(0, None)),
            Err(19),
            Ok(cadrat_hidraw::fake::slot_response(0, None)),
        ],
    );
    let h = Harness::new(vec![node.clone()]);
    let other = node.clone();
    h.clock.on_sleep(move |_| other.lock_elsewhere());
    let out = h.run(&["receiver", "pair", "--timeout=3"]);
    assert_eq!(out.code, 12, "{}", out.stderr);
    assert!(
        out.stderr.contains("W-MANAGEMENT-REOPENED"),
        "{}",
        out.stderr
    );
    assert_eq!(node.sets(), [START.to_vec(), STOP.to_vec()]);
}
