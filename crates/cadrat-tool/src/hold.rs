//! `hold-open` (spec tool/cli §2.9, spec device §9).
//!
//! Only `cadrat-tool` has this command, so it prints directly instead of
//! going through `cadrat-command`.

use std::time::Duration;

use cadrat_command::{Env, Exit, Failure};
use cadrat_hidraw::{HoldEvent, HoldOpen};

use crate::Io;
use crate::cli::Global;

pub fn run(env: &Env, mut io: Io, global: &Global, poll_interval: f64) -> i32 {
    let usage = if global.common.mouse.is_some()
        || global.common.route.is_some()
        || global.hidraw.is_some()
    {
        Some("hold-open does not take --mouse, --route or --hidraw")
    } else if global.common.json {
        Some("hold-open does not support --json")
    } else {
        None
    };
    if let Some(message) = usage {
        let _ = writeln!(io.stderr, "error: {message}");
        if global.common.json {
            let failure = Failure::new(Exit::Usage, message);
            let json = cadrat_command::envelope(
                Some("hold-open"),
                serde_json::Map::new(),
                Vec::new(),
                &Err(failure),
            );
            let _ = writeln!(io.stdout, "{json}");
        }
        return Exit::Usage.code();
    }
    let interval = Duration::from_secs_f64(poll_interval);
    let (system, clock, interrupt) = (env.system, env.clock, env.interrupt);
    if !global.common.quiet {
        let _ = writeln!(
            io.stderr,
            "note: keeping the wired C658's hidraw nodes open; stop with Ctrl-C"
        );
    }

    interrupt.arm();
    let mut hold = HoldOpen::default();
    loop {
        for event in hold.reconcile(system) {
            report(&mut io, &event);
        }
        if interrupt.is_set() {
            break;
        }
        clock.sleep(interval);
        if interrupt.is_set() {
            break;
        }
    }
    for event in hold.release_all() {
        report(&mut io, &event);
    }
    interrupt.disarm();
    Exit::Success.code()
}

/// Prints one event. Warnings go straight to stderr: this command runs for
/// days, so they are not collected for a final report.
fn report(io: &mut Io, event: &HoldEvent) {
    match event {
        HoldEvent::Held { path, interface } => {
            let interface = interface.map_or_else(|| "MI_??".to_owned(), |n| format!("MI_{n:02}"));
            let _ = writeln!(io.stdout, "held      {} ({interface})", path.display());
        }
        HoldEvent::Released { path } => {
            let _ = writeln!(io.stdout, "released  {}", path.display());
        }
        HoldEvent::OpenFailed { path, errno } => {
            let hint = if errno.is_permission() {
                "; check that the udev rule is installed"
            } else {
                ""
            };
            let _ = writeln!(
                io.stderr,
                "warning: W-HOLD-OPEN-FAILED: cannot open {} ({errno}){hint}",
                path.display()
            );
        }
        HoldEvent::EnumerateFailed { errno } => {
            let _ = writeln!(
                io.stderr,
                "warning: W-HOLD-ENUMERATE-FAILED: cannot list hidraw nodes ({errno}); retrying"
            );
        }
    }
}
