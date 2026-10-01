//! `cadrat-hold-open` against the fake transport (spec hold-open/cli §3).

#![allow(missing_docs)]

use std::cell::Cell;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;

use cadrat_hidraw::fake::{FakeNode, FakeSystem};
use cadrat_hold_open::{Stop, run};

// Synthetic device ID; not taken from any device.
const A: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];

/// A stop signal that arrives after `after` waits (never with `None`).
struct FakeStop {
    wake: UnixStream,
    after: Option<u32>,
    waits: Cell<u32>,
}

impl FakeStop {
    fn new(after: Option<u32>) -> Self {
        Self {
            wake: UnixStream::pair().unwrap().0,
            after,
            waits: Cell::new(0),
        }
    }
}

impl Stop for FakeStop {
    fn wake(&self) -> BorrowedFd<'_> {
        self.waits.set(self.waits.get() + 1);
        self.wake.as_fd()
    }
    fn is_set(&self) -> bool {
        self.after.is_some_and(|n| self.waits.get() >= n)
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_with(nodes: Vec<FakeNode>, args: &[&str], stop: &FakeStop) -> Output {
    let system = FakeSystem::new(nodes);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let args = std::iter::once("cadrat-hold-open")
        .chain(args.iter().copied())
        .map(Into::into);
    let code = run(args, &system, stop, &mut stdout, &mut stderr);
    Output {
        code,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

fn wired() -> FakeNode {
    FakeNode::wired("hidraw5", "3-2", A)
}

#[test]
fn holds_until_stopped() {
    let node = wired();
    let stop = FakeStop::new(Some(3));
    let out = run_with(vec![node.clone()], &["/dev/hidraw5"], &stop);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "held      /dev/hidraw5 (MI_01)\nreleased  /dev/hidraw5\n"
    );
    assert_eq!(node.open_count(), 0, "the descriptor stayed open");
    // Holding never sends or reads a report.
    assert!(node.log().is_empty());
}

/// Unplugs the node the first time the program waits.
struct Unplug<'a>(&'a FakeNode, FakeStop);
impl Stop for Unplug<'_> {
    fn wake(&self) -> BorrowedFd<'_> {
        self.0.unplug();
        self.1.wake()
    }
    fn is_set(&self) -> bool {
        false
    }
}

#[test]
fn releases_when_the_node_disappears() {
    let node = wired();
    node.unplug();
    let out = run_with(vec![node.clone()], &["/dev/hidraw5"], &FakeStop::new(None));
    // raw_info answers ENODEV for an unplugged node before holding.
    assert_eq!(out.code, 4, "{}", out.stderr);

    // Unplugged while held.
    let node = wired();
    let probe = node.clone();
    let stop = Unplug(&probe, FakeStop::new(None));
    let system = FakeSystem::new(vec![node.clone()]);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let args = ["cadrat-hold-open", "/dev/hidraw5"].map(Into::into);
    let code = run(args, &system, &stop, &mut stdout, &mut stderr);
    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "held      /dev/hidraw5 (MI_01)\nreleased  /dev/hidraw5\n"
    );
    assert_eq!(node.open_count(), 0);
}

#[test]
fn exit_codes() {
    let stop = FakeStop::new(Some(1));
    // No such node.
    assert_eq!(run_with(vec![], &["/dev/hidraw5"], &stop).code, 4);
    // Cannot open.
    let mut blocked = wired();
    blocked.open_error = Some(13);
    let out = run_with(vec![blocked], &["/dev/hidraw5"], &stop);
    assert_eq!(out.code, 6);
    assert!(out.stderr.contains("EACCES"), "{}", out.stderr);
    // Not a wired C658: a Receiver node.
    let receiver = FakeNode::receiver("hidraw5", "1-4", 0, None, true);
    let out = run_with(vec![receiver.clone()], &["/dev/hidraw5"], &stop);
    assert_eq!(out.code, 7, "{}", out.stderr);
    assert_eq!(receiver.open_count(), 0);
    // Arguments.
    assert_eq!(run_with(vec![], &[], &stop).code, 2);
    assert_eq!(
        run_with(vec![], &["/dev/hidraw5", "/dev/hidraw6"], &stop).code,
        2
    );
    let out = run_with(vec![], &["--version"], &stop);
    assert_eq!(out.code, 0);
    assert!(out.stdout.starts_with("cadrat-hold-open "));
    assert_eq!(run_with(vec![], &["--help"], &stop).code, 0);
}

#[test]
fn interface_is_left_out_when_unknown() {
    let mut node = wired();
    node.info.interface = None;
    let out = run_with(vec![node], &["/dev/hidraw5"], &FakeStop::new(Some(1)));
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "held      /dev/hidraw5\nreleased  /dev/hidraw5\n"
    );
}

/// The codes are those of the shared table (spec hold-open/cli §3.1), which
/// this program does not link.
#[test]
fn exit_codes_match_the_shared_table() {
    use cadrat_command::Exit as Shared;
    use cadrat_hold_open::Exit;
    let shared = [
        Shared::Success,
        Shared::Internal,
        Shared::Usage,
        Shared::NoDevice,
        Shared::PermissionDenied,
        Shared::DeviceInvalid,
    ];
    for (own, shared) in Exit::ALL.into_iter().zip(shared) {
        assert_eq!((own.code(), own.name()), (shared.code(), shared.name()));
    }
}

#[test]
fn error_lines_get_their_journal_priority() {
    use std::io::Write;
    let mut out = cadrat_hold_open::ErrorLevel::new(Vec::new());
    write!(out, "error: cannot open").unwrap();
    writeln!(out, " /dev/hidraw5: ENOENT").unwrap();
    writeln!(out, "error: second").unwrap();
    assert_eq!(
        String::from_utf8(out.into_inner()).unwrap(),
        "<3>error: cannot open /dev/hidraw5: ENOENT\n<3>error: second\n"
    );
}
