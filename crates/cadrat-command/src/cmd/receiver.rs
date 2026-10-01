//! `receiver slots`, `receiver pair` and `receiver unpair` (spec receiver).

use cadrat_hidraw::receiver::{self, PairResult, Polling, SlotReadError, UnpairResult};
use cadrat_hidraw::{Inventory, ManagementTarget, Route, select_management_node, select_receiver};
use cadrat_proto::Slot;
use serde_json::json;

use crate::cmd::list::inventory;
use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::format::{self, Redactor};
use crate::render;

/// Spec receiver §5: receiver commands do not take a mouse.
fn no_mouse(ctx: &Ctx) -> Result<(), Failure> {
    if ctx.options.mouse.is_some() || ctx.options.route.is_some() {
        return Err(Failure::new(
            Exit::Usage,
            "receiver commands do not take --mouse or --route",
        ));
    }
    Ok(())
}

/// Enumerates and chooses the management node (spec receiver §1).
fn target(
    ctx: &mut Ctx,
    receiver: Option<&str>,
    require_pairing: bool,
) -> Result<(ManagementTarget, Inventory), Failure> {
    let mut inventory = inventory(ctx)?;
    let target = match &ctx.options.hidraw {
        Some(node) => select_management_node(ctx.env.system, node, require_pairing)?,
        None => select_receiver(&mut inventory, receiver, require_pairing)?,
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

/// Spec device §7.2: another process is writing through this Receiver.
fn busy(path: &std::path::Path, errno: cadrat_hidraw::Errno) -> Failure {
    Failure::new(
        Exit::Busy,
        format!(
            "another process is writing to {} ({errno}); nothing was done",
            path.display()
        ),
    )
}

fn protocol(error: &SlotReadError) -> Failure {
    Failure::new(Exit::ReceiverProtocolError, error.to_string())
}

pub fn slots(ctx: &mut Ctx, receiver: Option<&str>, redact: bool) -> Result<(), Failure> {
    no_mouse(ctx)?;
    let (mut target, inventory) = target(ctx, receiver, false)?;
    let slots = receiver::read_slots(target.device()).map_err(|e| protocol(&e))?;
    ctx.set(
        "slots",
        format::slots_json(&slots, &inventory, &mut Redactor::new(redact)),
    );
    Ok(())
}

pub fn pair(ctx: &mut Ctx, receiver: Option<&str>, polling: Polling) -> Result<(), Failure> {
    no_mouse(ctx)?;
    let _daemon = ctx.hold_daemon_lock("receiver pair")?;
    let (mut target, inventory) = target(ctx, receiver, true)?;
    let interrupt = ctx.env.interrupt;
    let clock = ctx.env.clock;
    let system = ctx.env.system;
    let frontend = &mut *ctx.frontend;
    interrupt.arm();
    let outcome = receiver::pair(
        &mut target.link(system, true),
        clock,
        polling,
        &mut || frontend.pairing_started(polling.timeout),
        &|| interrupt.is_set(),
    );
    interrupt.disarm();
    let outcome = outcome.map_err(|e| protocol(&e))?;
    let path = target.path.clone();

    let mut redactor = Redactor::new(false);
    ctx.set(
        "slots_before",
        format::slots_json(&outcome.before, &inventory, &mut redactor),
    );
    ctx.set(
        "slots_after",
        outcome
            .after
            .as_ref()
            .map(|s| format::slots_json(s, &inventory, &mut redactor)),
    );
    ctx.set("stop_sent", outcome.stop_sent);
    for warning in &outcome.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    let new_slots: Vec<u8> = match &outcome.result {
        PairResult::Paired(slots) => slots.iter().map(|s| s.get()).collect(),
        _ => Vec::new(),
    };
    ctx.set("new_slots", new_slots);
    match outcome.result {
        PairResult::Paired(_) => Ok(()),
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
        PairResult::Busy(errno) => Err(busy(&path, errno)),
    }
}

pub fn unpair(
    ctx: &mut Ctx,
    receiver: Option<&str>,
    slot: Slot,
    yes: bool,
    polling: Polling,
) -> Result<(), Failure> {
    no_mouse(ctx)?;
    if !yes && !ctx.frontend.can_confirm() {
        return Err(Failure::new(
            Exit::Usage,
            "unpair asks for confirmation on a terminal; pass --yes to skip it",
        ));
    }
    let _daemon = ctx.hold_daemon_lock(&format!("receiver unpair {slot}"))?;
    let (mut target, inventory) = target(ctx, receiver, true)?;
    let mut redactor = Redactor::new(false);
    let clock = ctx.env.clock;
    let system = ctx.env.system;
    let frontend = &mut *ctx.frontend;
    let mut shown = false;
    let mut management = target.link(system, true);
    let outcome = receiver::unpair(&mut management, slot, clock, polling, &mut |report| {
        let mouse = format::mouse_for_slot(&inventory, report);
        let mut lines = vec![render::slot_line(&format::slot_json(
            report,
            mouse,
            &mut redactor,
        ))];
        if mouse.is_some_and(|(_, m)| m.route(Route::Wired).is_none()) {
            lines.push(
                "this mouse is connected only through this Receiver; after unpairing it stops \
                 working until you pair it again, or connect it by cable or Bluetooth"
                    .to_owned(),
            );
        }
        frontend.unpair_target(&lines);
        shown = true;
        yes || frontend.confirm_unpair(slot)
    });
    let outcome = outcome.map_err(|e| protocol(&e))?;
    let path = target.path.clone();

    let mut redactor = Redactor::new(false);
    ctx.set(
        "target",
        format::slot_json(
            &outcome.target,
            format::mouse_for_slot(&inventory, &outcome.target),
            &mut redactor,
        ),
    );
    ctx.set("sent", outcome.sent);
    ctx.set(
        "slots_after",
        outcome
            .after
            .as_ref()
            .map(|s| format::slots_json(s, &inventory, &mut redactor)),
    );
    for warning in &outcome.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    match outcome.result {
        UnpairResult::Unpaired => Ok(()),
        UnpairResult::SlotChanged if !shown => Err(Failure::new(
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
        UnpairResult::Busy(errno) => Err(busy(&path, errno)),
    }
}
