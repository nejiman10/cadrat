//! The `cadrat-hold-open` binary: real hidraw, signals and streams.

use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cadrat_hidraw::LinuxSystem;
use cadrat_hold_open::{Stop, run};

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
        // Never drained: once readable, the flag is set and the wait ends.
        self.wake.as_fd()
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

fn main() {
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(e) => {
            eprintln!("error: cannot install signal handlers: {e}");
            std::process::exit(cadrat_command::Exit::Internal.code());
        }
    };
    let code = run(
        std::env::args_os(),
        &LinuxSystem::default(),
        &signals,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code);
}
