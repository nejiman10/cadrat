//! The `cadrat-tool` binary: real hidraw, clock, signals and streams.

use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cadrat_hidraw::{LinuxSystem, SystemClock};
use cadrat_tool::{DaemonLock, Env, Interrupt, Io, run};

/// SIGINT/SIGTERM caught only while armed.
#[derive(Default)]
struct Signals {
    flag: Arc<AtomicBool>,
    ids: Mutex<Vec<signal_hook::SigId>>,
}

impl Interrupt for Signals {
    fn arm(&self) {
        let mut ids = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            if let Ok(id) = signal_hook::flag::register(signal, Arc::clone(&self.flag)) {
                ids.push(id);
            }
        }
    }

    fn disarm(&self) {
        let mut ids = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for id in ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

fn main() {
    let system = LinuxSystem::default();
    let clock = SystemClock::default();
    let signals = Signals::default();
    let env = Env {
        system: &system,
        clock: &clock,
        interrupt: &signals,
        xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        home: std::env::var_os("HOME"),
        lock_timeout: cadrat_config::LOCK_TIMEOUT,
        daemon_lock: Some(DaemonLock::system()),
    };
    let stdin = std::io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    let io = Io {
        stdout: &mut std::io::stdout().lock(),
        stderr: &mut std::io::stderr().lock(),
        stdin: &mut stdin.lock(),
        stdin_is_terminal,
    };
    let code = run(std::env::args_os(), &env, io);
    std::process::exit(code);
}
