//! `hold-open` (spec 03 §2.9, spec 02 §9).

use std::time::Duration;

use cadrat_hidraw::{HoldEvent, HoldOpen};

use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::render;

pub fn run(ctx: &mut Ctx, poll_interval: f64) -> Result<(), Failure> {
    let global = &ctx.global;
    if global.mouse.is_some() || global.route.is_some() || global.hidraw.is_some() {
        return Err(Failure::new(
            Exit::Usage,
            "hold-open does not take --mouse, --route or --hidraw",
        ));
    }
    if global.json {
        return Err(Failure::new(
            Exit::Usage,
            "hold-open does not support --json",
        ));
    }
    let interval = Duration::from_secs_f64(poll_interval);
    let (system, clock, interrupt) = (ctx.env.system, ctx.env.clock, ctx.env.interrupt);
    ctx.note("keeping the wired C658's hidraw nodes open; stop with Ctrl-C");

    interrupt.arm();
    let mut hold = HoldOpen::default();
    loop {
        for event in hold.reconcile(system) {
            report(ctx, &event);
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
        report(ctx, &event);
    }
    interrupt.disarm();
    Ok(())
}

/// Prints one event. Warnings go straight to stderr: this command runs for
/// days, so they are not collected for a final report.
fn report(ctx: &mut Ctx, event: &HoldEvent) {
    match event {
        HoldEvent::Held { path, interface } => {
            let interface = interface.map_or_else(|| "MI_??".to_owned(), render::interface);
            ctx.out(format!("held      {} ({interface})", path.display()));
        }
        HoldEvent::Released { path } => ctx.out(format!("released  {}", path.display())),
        HoldEvent::OpenFailed { path, errno } => {
            let hint = if errno.is_permission() {
                "; check that the udev rule is installed"
            } else {
                ""
            };
            let _ = writeln!(
                ctx.io.stderr,
                "warning: W-HOLD-OPEN-FAILED: cannot open {} ({errno}){hint}",
                path.display()
            );
        }
        HoldEvent::EnumerateFailed { errno } => {
            let _ = writeln!(
                ctx.io.stderr,
                "warning: W-HOLD-ENUMERATE-FAILED: cannot list hidraw nodes ({errno}); retrying"
            );
        }
    }
}
