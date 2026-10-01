//! The `cadrat-hold-open` binary: real hidraw, signals and streams.

use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cadrat_hidraw::LinuxSystem;
use cadrat_hold_open::{ErrorLevel, Exit, Stop, run};

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

/// Whether standard error is the journal's stream: `JOURNAL_STREAM` names
/// its device and inode (systemd.exec(5)).
fn stderr_is_journal() -> bool {
    let Some(stream) = std::env::var_os("JOURNAL_STREAM") else {
        return false;
    };
    rustix::fs::fstat(std::io::stderr())
        .is_ok_and(|stat| stream.to_str() == Some(&format!("{}:{}", stat.st_dev, stat.st_ino)))
}

fn main() {
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(e) => {
            eprintln!("error: cannot install signal handlers: {e}");
            std::process::exit(Exit::Internal.code());
        }
    };
    let mut stderr: Box<dyn std::io::Write> = if stderr_is_journal() {
        Box::new(ErrorLevel::new(std::io::stderr().lock()))
    } else {
        Box::new(std::io::stderr().lock())
    };
    let code = run(
        std::env::args_os(),
        &LinuxSystem::default(),
        &signals,
        &mut std::io::stdout().lock(),
        &mut stderr,
    );
    drop(stderr);
    std::process::exit(code);
}
