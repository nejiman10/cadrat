//! The `cadratd` binary: real hidraw, clock, signals and the session bus.

use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cadrat_command::DaemonLock;
use cadrat_hidraw::{LinuxSystem, SystemClock};
use cadratd::{Bus, Stop, World, run};

/// SIGINT/SIGTERM: a flag, and a self-pipe that wakes `poll`.
struct Signals {
    flag: Arc<AtomicBool>,
    wake: UnixStream,
}

impl Signals {
    fn install() -> std::io::Result<Self> {
        let (wake, write) = UnixStream::pair()?;
        let flag = Arc::new(AtomicBool::new(false));
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            signal_hook::flag::register(signal, Arc::clone(&flag))?;
            signal_hook::low_level::pipe::register(signal, write.try_clone()?)?;
        }
        Ok(Self { flag, wake })
    }
}

impl Stop for Signals {
    fn wake(&self) -> BorrowedFd<'_> {
        // Never drained: once readable, the flag is set and the loop ends.
        self.wake.as_fd()
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

fn main() {
    if let Err(error) = cadratd::command().try_get_matches() {
        let _ = error.print();
        std::process::exit(error.exit_code());
    }
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(e) => {
            eprintln!("error: cannot install signal handlers: {e}");
            std::process::exit(1);
        }
    };
    let world = World {
        system: Arc::new(LinuxSystem::default()),
        clock: Arc::new(SystemClock::default()),
        xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        home: std::env::var_os("HOME"),
        lock_timeout: cadrat_config::LOCK_TIMEOUT,
        daemon_lock: DaemonLock::system(),
        dev: PathBuf::from("/dev"),
        bus: Bus::Session,
        start_wait: Duration::from_secs(90),
        settle: Duration::from_millis(250),
    };
    let code = run(world, &signals, Box::new(std::io::stderr()));
    std::process::exit(code);
}
