//! The `cadratctl` binary: the session bus, signals and streams.

use std::any::Any;
use std::io::IsTerminal;

use cadratctl::{Env, Io, Signals, run};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals as SignalIterator};

/// Catches SIGINT/SIGTERM on a thread while a guard lives.
struct Caught;

struct Guard(Handle);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Signals for Caught {
    fn catch(&self, on_signal: Box<dyn Fn() + Send + Sync>) -> Box<dyn Any> {
        let Ok(mut signals) = SignalIterator::new([SIGINT, SIGTERM]) else {
            return Box::new(());
        };
        let handle = signals.handle();
        std::thread::spawn(move || {
            for _ in signals.forever() {
                on_signal();
            }
        });
        Box::new(Guard(handle))
    }
}

fn main() {
    let env = Env {
        bus: cadratctl::Bus::Session,
        xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
        home: std::env::var_os("HOME"),
        cwd: std::env::current_dir().unwrap_or_default(),
        signals: &Caught,
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
