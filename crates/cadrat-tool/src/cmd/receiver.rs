//! `receiver slots`, `receiver pair` and `receiver unpair` (spec receiver).

use std::io::Write;
use std::time::Duration;

use cadrat_hidraw::receiver::{self, PairResult, Polling, SlotReadError, UnpairResult};
use cadrat_hidraw::{Inventory, ManagementTarget, Route, select_management_node, select_receiver};
use cadrat_proto::{Slot, SlotReport};
use serde_json::json;

use crate::cli::{ReceiverArg, ReceiverCommand};
use crate::cmd::list::inventory;
use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::render::{self, Redactor};

pub fn run(ctx: &mut Ctx, command: &ReceiverCommand) -> Result<(), Failure> {
    // Spec receiver §5: receiver commands do not take a mouse.
    if ctx.global.mouse.is_some() || ctx.global.route.is_some() {
        return Err(Failure::new(
            Exit::Usage,
            "receiver commands do not take --mouse or --route",
        ));
    }
    match command {
        ReceiverCommand::Slots { receiver, redact } => slots(ctx, receiver, *redact),
        ReceiverCommand::Pair {
            receiver,
            timeout,
            poll_interval,
        } => pair(ctx, receiver, polling(*timeout, *poll_interval)),
        ReceiverCommand::Unpair {
            slot,
            receiver,
            yes,
            timeout,
            poll_interval,
        } => unpair(
            ctx,
            receiver,
            Slot::new(*slot).expect("clap checks the range"),
            *yes,
            polling(*timeout, *poll_interval),
        ),
    }
}

fn polling(timeout: f64, interval: f64) -> Polling {
    Polling {
        timeout: Duration::from_secs_f64(timeout),
        interval: Duration::from_secs_f64(interval),
    }
}

/// Enumerates and chooses the management node (spec receiver §1).
fn target(
    ctx: &mut Ctx,
    receiver: &ReceiverArg,
    require_pairing: bool,
) -> Result<(ManagementTarget, Inventory), Failure> {
    let mut inventory = inventory(ctx)?;
    let target = match &ctx.global.hidraw {
        Some(node) => select_management_node(ctx.env.system, node, require_pairing)?,
        None => select_receiver(
            &mut inventory,
            receiver.receiver.as_deref(),
            require_pairing,
        )?,
    };
    ctx.set(
        "receiver",
        json!({
            "key": target.receiver.key.to_string(),
            "node": target.path.display().to_string(),
            "interface": target.interface,
        }),
    );
    Ok((target, inventory))
}

fn header(ctx: &mut Ctx, target: &ManagementTarget) {
    ctx.out(format!(
        "receiver {}  ({}, {})",
        target.receiver.key,
        target.path.display(),
        render::interface(target.interface)
    ));
}

fn protocol(error: &SlotReadError) -> Failure {
    Failure::new(Exit::ReceiverProtocolError, error.to_string())
}

fn show_slots(
    ctx: &mut Ctx,
    field: &str,
    slots: &[SlotReport],
    inventory: &Inventory,
    redactor: &mut Redactor,
) {
    ctx.set(field, render::slots_json(slots, inventory, redactor));
    for slot in slots {
        let line = render::slot_line(slot, render::mouse_for_slot(inventory, slot), redactor);
        ctx.out(line);
    }
}

fn slots(ctx: &mut Ctx, receiver: &ReceiverArg, redact: bool) -> Result<(), Failure> {
    let (mut target, inventory) = target(ctx, receiver, false)?;
    let slots = receiver::read_slots(target.device()).map_err(|e| protocol(&e))?;
    header(ctx, &target);
    show_slots(ctx, "slots", &slots, &inventory, &mut Redactor::new(redact));
    Ok(())
}

fn pair(ctx: &mut Ctx, receiver: &ReceiverArg, polling: Polling) -> Result<(), Failure> {
    let (mut target, inventory) = target(ctx, receiver, true)?;
    header(ctx, &target);
    let interrupt = ctx.env.interrupt;
    let clock = ctx.env.clock;
    let stderr = &mut ctx.io.stderr;
    let quiet = ctx.global.quiet;
    interrupt.arm();
    let system = ctx.env.system;
    let outcome = receiver::pair(
        &mut target.link(system, true),
        clock,
        polling,
        &mut || {
            if !quiet {
                let _ = writeln!(
                    stderr,
                    "put the mouse in pairing mode now (waiting up to {} s; Ctrl-C stops pairing)",
                    polling.timeout.as_secs_f64()
                );
            }
        },
        &|| interrupt.is_set(),
    );
    interrupt.disarm();
    let outcome = outcome.map_err(|e| protocol(&e))?;

    let mut redactor = Redactor::new(false);
    ctx.set(
        "slots_before",
        render::slots_json(&outcome.before, &inventory, &mut redactor),
    );
    ctx.set(
        "slots_after",
        outcome
            .after
            .as_ref()
            .map(|s| render::slots_json(s, &inventory, &mut redactor)),
    );
    ctx.set("stop_sent", outcome.stop_sent);
    for warning in &outcome.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    let new_slots: Vec<u8> = match &outcome.result {
        PairResult::Paired(slots) => slots.iter().map(|s| s.get()).collect(),
        _ => Vec::new(),
    };
    ctx.set("new_slots", new_slots.clone());
    match outcome.result {
        PairResult::Paired(_) => {
            let slots: Vec<String> = new_slots.iter().map(ToString::to_string).collect();
            ctx.out(format!("paired  slot {}", slots.join(", ")));
            ctx.info("next    run `cadrat-tool list`; the new mouse is expected on the interface with the same number as its slot");
            ctx.info(
                "        then send your settings with `cadrat-tool apply --mouse=<number or key>`",
            );
            ctx.info("        the first send after pairing is sometimes lost; if nothing changes, send it again");
            Ok(())
        }
        PairResult::Timeout => Err(Failure::new(
            Exit::PairTimeout,
            "no new slot became occupied before the timeout; pairing mode was stopped",
        )),
        PairResult::Interrupted => Err(Failure::new(
            Exit::PairTimeout,
            "interrupted; pairing mode was stopped",
        )),
        PairResult::StartFailed(result) => Err(Failure::new(
            Exit::ReceiverCommandFailed,
            format!("starting pairing mode failed ({result}); pairing mode was stopped"),
        )),
        PairResult::SlotReadFailed(e) => Err(Failure::new(
            Exit::ReceiverProtocolError,
            format!("{e}; pairing mode was stopped"),
        )),
        PairResult::StopFailed(result) => Err(Failure::new(
            Exit::PairStopFailed,
            format!(
                "stopping pairing mode failed ({result}); the Receiver may still be in pairing mode"
            ),
        )
        .hint("unplug the Receiver and plug it in again to leave pairing mode")),
    }
}

fn unpair(
    ctx: &mut Ctx,
    receiver: &ReceiverArg,
    slot: Slot,
    yes: bool,
    polling: Polling,
) -> Result<(), Failure> {
    if !yes && (ctx.global.json || !ctx.io.stdin_is_terminal) {
        return Err(Failure::new(
            Exit::Usage,
            "unpair asks for confirmation on a terminal; pass --yes to skip it",
        ));
    }
    let (mut target, inventory) = target(ctx, receiver, true)?;
    header(ctx, &target);
    let mut redactor = Redactor::new(false);
    let clock = ctx.env.clock;
    let io = &mut ctx.io;
    let mut shown = None;
    let system = ctx.env.system;
    let mut management = target.link(system, true);
    let outcome = receiver::unpair(&mut management, slot, clock, polling, &mut |report| {
        let mouse = render::mouse_for_slot(&inventory, report);
        let line = render::slot_line(report, mouse, &mut redactor);
        let _ = writeln!(io.stderr, "{line}");
        if mouse.is_some_and(|(_, m)| m.route(Route::Wired).is_none()) {
            let _ = writeln!(
                io.stderr,
                "this mouse is connected only through this Receiver; after unpairing it stops \
                 working until you pair it again, or connect it by cable or Bluetooth"
            );
        }
        shown = Some(line);
        if yes {
            return true;
        }
        let _ = write!(io.stderr, "Unpair slot {slot}? [y/N] ");
        let _ = io.stderr.flush();
        let mut answer = String::new();
        if io.stdin.read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
    });
    let outcome = outcome.map_err(|e| protocol(&e))?;

    let mut redactor = Redactor::new(false);
    ctx.set(
        "target",
        render::slot_json(
            &outcome.target,
            render::mouse_for_slot(&inventory, &outcome.target),
            &mut redactor,
        ),
    );
    ctx.set("sent", outcome.sent);
    ctx.set(
        "slots_after",
        outcome
            .after
            .as_ref()
            .map(|s| render::slots_json(s, &inventory, &mut redactor)),
    );
    for warning in &outcome.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    match outcome.result {
        UnpairResult::Unpaired => {
            ctx.out(format!("unpaired slot {slot}"));
            if let Some(after) = &outcome.after {
                show_slots(ctx, "slots_after", after, &inventory, &mut redactor);
            }
            Ok(())
        }
        UnpairResult::SlotChanged if shown.is_none() => Err(Failure::new(
            Exit::SlotChanged,
            format!("slot {slot} is empty; nothing was done"),
        )),
        UnpairResult::SlotChanged => Err(Failure::new(
            Exit::SlotChanged,
            format!("slot {slot} changed after it was shown; nothing was done"),
        )),
        UnpairResult::Aborted => Err(Failure::new(
            Exit::Aborted,
            "not confirmed; nothing was done",
        )),
        UnpairResult::CommandFailed(result) => Err(Failure::new(
            Exit::ReceiverCommandFailed,
            format!("the unpair request failed ({result})"),
        )),
        UnpairResult::NotConfirmed => Err(Failure::new(
            Exit::UnpairNotConfirmed,
            format!("slot {slot} did not become empty before the timeout"),
        )),
        UnpairResult::SlotReadFailed(e) => Err(protocol(&e)),
    }
}
