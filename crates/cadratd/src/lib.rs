//! `cadratd`: the per-user daemon (spec daemon/daemon.md, daemon/dbus.md).
//!
//! [`run`] takes the outside world ([`World`]), a stop signal ([`Stop`]) and
//! the log stream, so the D-Bus tests run the whole daemon against the fake
//! transport of `cadrat-hidraw` and a private `dbus-daemon` (spec
//! implementation §4). The binary only wires the real implementations in.
//! Each request runs the same command as `cadrat-tool` through
//! `cadrat-command`.

mod log;
mod service;

use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::fd::BorrowedFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cadrat_command::DaemonLock;
pub use cadrat_dbus::Bus;
use cadrat_dbus::{BUS_NAME, PATH};
use cadrat_hidraw::{Clock, DevWatch, System};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{FlockOperation, flock};
use zbus::fdo::{RequestNameFlags, RequestNameReply};

use log::Log;
use service::{Manager, Shared};

/// The version reported by the `Version` property and `--version`.
pub use cadrat_command::cli::VERSION;

/// The command-line definition: `cadratd` takes no arguments besides
/// `--help` and `--version`; systemd starts it (spec daemon §2).
#[must_use]
pub fn command() -> clap::Command {
    clap::Command::new("cadratd")
        .version(VERSION)
        .about("Per-user daemon that configures the C658 mouse and the C652 Receiver over D-Bus")
        .long_about(
            "Per-user daemon that configures the C658 mouse and the C652 Receiver over D-Bus.\n\n\
             It runs as the systemd user service cadratd.service and takes the name \
             cc.nejiman10.Cadrat1 on the session bus. Use cadratctl to talk to it. Its D-Bus \
             interface is not stable yet: other programs must not depend on it.",
        )
}

/// The outside world.
pub struct World {
    /// hidraw access.
    pub system: Arc<dyn System + Send + Sync>,
    /// Time.
    pub clock: Arc<dyn Clock + Send + Sync>,
    /// `XDG_CONFIG_HOME`, for requests without `config`.
    pub xdg_config_home: Option<OsString>,
    /// `HOME`.
    pub home: Option<OsString>,
    /// How long to wait for the configuration lock (5 s in the binary).
    pub lock_timeout: Duration,
    /// Where `cadratd.lock` is (spec daemon §4).
    pub daemon_lock: DaemonLock,
    /// The directory to watch for hidraw nodes: `/dev`.
    pub dev: PathBuf,
    /// The bus.
    pub bus: Bus,
    /// How long to wait for `cadrat-tool` to release `cadratd.lock` at
    /// start (90 s in the binary, spec daemon §3).
    pub start_wait: Duration,
    /// How long to let `/dev` settle before enumerating again (250 ms).
    pub settle: Duration,
}

/// The request to stop (SIGTERM or SIGINT in the binary).
pub trait Stop {
    /// A descriptor that becomes readable when a stop is requested.
    fn wake(&self) -> BorrowedFd<'_>;
    /// Whether a stop was requested.
    fn is_set(&self) -> bool;
}

/// Runs `cadratd` until stopped. Returns the exit code: 0 when stopped by
/// [`Stop`], 1 when starting failed (spec daemon §3).
pub fn run(world: World, stop: &dyn Stop, log: Box<dyn Write + Send>) -> i32 {
    let log = Arc::new(Log::new(log));
    match serve(world, stop, &log) {
        Ok(()) => 0,
        Err(message) => {
            log.line(&format!("error: {message}"));
            1
        }
    }
}

fn serve(world: World, stop: &dyn Stop, log: &Arc<Log>) -> Result<(), String> {
    log.line(&format!("cadratd {VERSION} starting"));
    // 1. The lock file.
    let (lock_path, lock) = world
        .daemon_lock
        .open()
        .map_err(|(path, e)| format!("cannot open {}: {e}", path.display()))?;
    // 2. Change detection.
    let mut watch = DevWatch::new(&world.dev)
        .map_err(|e| format!("cannot watch {}: {e}", world.dev.display()))?;
    let settle = world.settle;
    let start_wait = world.start_wait;
    let shared = Arc::new(Shared::new(world, Arc::clone(log)));
    // 3. The bus name. Requests that touch devices get `Starting` until 5.
    let conn = connect(&shared)?;
    // 4. The write lock.
    if !wait_for_lock(&lock, &lock_path, start_wait, stop, log)? {
        log.line("stopped while starting");
        return Ok(());
    }
    log.line(&format!("holding {}", lock_path.display()));
    // 5–6. The first enumeration, then every request.
    let iface = conn
        .object_server()
        .interface::<_, Manager>(PATH)
        .map_err(|e| format!("cannot find the D-Bus object: {e}"))?;
    let publish = || {
        if shared.enumerate() {
            let _ = zbus::block_on(iface.get().devices_changed(iface.signal_emitter()));
        }
    };
    publish();
    shared.set_ready();
    let _ = zbus::block_on(iface.get().ready_changed(iface.signal_emitter()));
    log.line("ready");

    let mut pending: Option<Instant> = None;
    while !stop.is_set() {
        let timeout = pending.map(|at| {
            let left = at.saturating_duration_since(Instant::now());
            Timespec::try_from(left).unwrap_or_default()
        });
        let mut fds = [
            PollFd::from_borrowed_fd(watch.fd(), PollFlags::IN),
            PollFd::from_borrowed_fd(stop.wake(), PollFlags::IN),
        ];
        match poll(&mut fds, timeout.as_ref()) {
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(format!("poll failed: {e}")),
        }
        let changed = fds[0].revents().contains(PollFlags::IN);
        if changed {
            match watch.drain() {
                Ok(true) => pending = Some(Instant::now() + settle),
                Ok(false) => {}
                Err(e) => return Err(format!("cannot read the /dev watch: {e}")),
            }
        }
        if pending.is_some_and(|at| Instant::now() >= at) {
            pending = None;
            publish();
        }
    }

    log.line("stopping");
    shared.stop_and_wait();
    // Let the replies of the requests that just ended reach the bus.
    std::thread::sleep(Duration::from_millis(200));
    let _ = conn.release_name(BUS_NAME);
    drop(iface);
    drop(conn);
    drop(lock);
    log.line("stopped");
    Ok(())
}

fn connect(shared: &Arc<Shared>) -> Result<zbus::blocking::Connection, String> {
    let builder = match &shared.world.bus {
        Bus::Session => zbus::blocking::connection::Builder::session(),
        Bus::Address(address) => zbus::blocking::connection::Builder::address(address.as_str()),
    };
    let conn = builder
        .and_then(|b| b.serve_at(PATH, Manager::new(Arc::clone(shared))))
        .and_then(zbus::blocking::connection::Builder::build)
        .map_err(|e| format!("cannot connect to the session bus: {e}"))?;
    match conn.request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(conn),
        Ok(_) | Err(zbus::Error::NameTaken) => {
            Err(format!("{BUS_NAME} is taken: cadratd is already running"))
        }
        Err(e) => Err(format!("cannot take {BUS_NAME}: {e}")),
    }
}

/// Takes the exclusive lock on `cadratd.lock`, waiting up to `limit` while
/// `cadrat-tool` holds it (spec daemon §3 step 4). Returns `false` when
/// stopped meanwhile.
fn wait_for_lock(
    lock: &File,
    path: &std::path::Path,
    limit: Duration,
    stop: &dyn Stop,
    log: &Log,
) -> Result<bool, String> {
    let start = Instant::now();
    let mut said = false;
    loop {
        match flock(lock, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(true),
            Err(rustix::io::Errno::WOULDBLOCK) => {}
            Err(e) => return Err(format!("cannot lock {}: {e}", path.display())),
        }
        if !said {
            log.line("waiting for cadrat-tool to finish writing to a device");
            said = true;
        }
        if start.elapsed() >= limit {
            return Err(format!(
                "cadrat-tool kept writing to devices for {} s; giving up",
                limit.as_secs()
            ));
        }
        let mut fds = [PollFd::from_borrowed_fd(stop.wake(), PollFlags::IN)];
        let _ = poll(
            &mut fds,
            Some(&Timespec::try_from(Duration::from_millis(100)).unwrap_or_default()),
        );
        if stop.is_set() {
            return Ok(false);
        }
    }
}
