//! `set` and `apply`, following the order in spec tool/cli §4.

use cadrat_config::{ConfigLock, parse_assignments};
use cadrat_hidraw::{SendError, Target, select_mouse, select_node};
use serde_json::json;

use crate::cmd::config::{load, read, warn_config};
use crate::cmd::list::inventory;
use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::format;

pub fn set(
    ctx: &mut Ctx,
    assignments: &[String],
    dry_run: bool,
    no_save: bool,
) -> Result<(), Failure> {
    run(ctx, Some(assignments), dry_run, no_save)
}

pub fn apply(ctx: &mut Ctx, dry_run: bool) -> Result<(), Failure> {
    run(ctx, None, dry_run, true)
}

#[allow(clippy::too_many_lines)]
fn run(
    ctx: &mut Ctx,
    assignments: Option<&[String]>,
    dry_run: bool,
    no_save: bool,
) -> Result<(), Failure> {
    // 1. Arguments.
    let given = assignments;
    let assignments = match assignments {
        Some(args) => {
            parse_assignments(args).map_err(|e| Failure::new(Exit::Usage, e.to_string()))?
        }
        None => Vec::new(),
    };
    let saving = !assignments.is_empty() && !no_save;

    // Spec tool/cli §8: not while cadratd runs, checked before the TOML lock.
    let _daemon = if dry_run {
        None
    } else {
        let instead = match given {
            Some(args) => format!("set {}", args.join(" ")),
            None => "apply".to_owned(),
        };
        ctx.hold_daemon_lock(&instead)?
    };

    // 2. Lock. A missing file is a configuration error, not a lock error.
    let path = ctx.config_path()?;
    if !path.exists() {
        read(&path, ctx.program)?;
    }
    let _lock = ConfigLock::acquire(&path, ctx.env.lock_timeout)?;

    // 3–5. Read, validate, change.
    let (loaded, mut document, config) = load(&path, ctx.program)?;
    let (config, changes) = config
        .apply(&assignments)
        .map_err(|e| Failure::new(Exit::Internal, e.to_string()))?;

    // 6. Wire report and warnings.
    warn_config(ctx, &config);
    let wire = config.to_report().to_wire();
    let wire_hex = format::hex(&wire);
    ctx.set("path", loaded.path.display().to_string());
    ctx.set("wire_hex", wire_hex.replace(' ', ""));
    let change_json: Vec<_> = changes
        .iter()
        .map(|c| {
            json!({
                "key": c.key.to_string(),
                "from": format::value_json(c.from),
                "to": format::value_json(c.to),
            })
        })
        .collect();
    if assignments_given(&changes) {
        ctx.set("changes", change_json);
    }
    ctx.set("sent", false);
    ctx.set("saved", false);
    if dry_run {
        ctx.set("wire_layout", format::wire_layout_json(&config, &wire));
        return Ok(());
    }

    // 7. Choose the mouse.
    let (mut target, number) = if let Some(node) = &ctx.options.hidraw {
        (select_node(ctx.env.system, node)?, None)
    } else {
        let mut inventory = inventory(ctx)?;
        let route = ctx.options.route;
        let target = select_mouse(&mut inventory, ctx.options.mouse.as_deref(), route)?;
        let number = inventory
            .mice
            .iter()
            .position(|m| m.key == target.mouse.key)
            .map(|i| i + 1);
        (target, number)
    };
    for warning in &target.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    describe_target(ctx, &target, number);

    // 8. Send.
    let program = ctx.program;
    target.send(&wire).map_err(|e| match &e {
        SendError::TargetChanged { .. } => Failure::new(Exit::TargetChanged, e.to_string())
            .hint(format!("run `{program} list` to find the mouse again")),
        SendError::Failed { .. } => Failure::new(Exit::SendFailed, e.to_string())
            .hint("the configuration file was not changed"),
        SendError::Busy { .. } => {
            Failure::new(Exit::Busy, e.to_string()).hint("the configuration file was not changed")
        }
    })?;
    ctx.set("sent", true);

    if saving {
        // 9–10. Save only if nobody else changed the file.
        let saved = document
            .update(&changes)
            .map_err(|e| Failure::new(Exit::Internal, e.to_string()))
            .and_then(|()| loaded.save(&document.to_string()).map_err(Failure::from));
        if let Err(failure) = saved {
            let pending: Vec<String> = changes
                .iter()
                .filter(|c| c.is_effective())
                .map(|c| format!("{}={}", c.key, c.to))
                .collect();
            return Err(Failure::new(
                Exit::SentNotSaved,
                format!("the settings were sent but not saved: {}", failure.message),
            )
            .hint(format!("sent wire report: {wire_hex}"))
            .hint(format!(
                "changes to record in the file: {}",
                pending.join(" ")
            ))
            .hint(format!(
                "edit the file by hand, or run `{program} set` again"
            ))
            .details(json!({ "pending_changes": pending })));
        }
        ctx.set("saved", true);
    } else if assignments_given(&changes) {
        ctx.warn(
            "W-NOT-SAVED",
            format!(
                "the mouse and the TOML file now differ; run `{program} apply` to resend the file"
            ),
        );
    }
    Ok(())
}

fn assignments_given(changes: &[cadrat_config::Change]) -> bool {
    !changes.is_empty()
}

fn describe_target(ctx: &mut Ctx, target: &Target, number: Option<usize>) {
    let route = &target.route;
    ctx.set(
        "mouse",
        json!({
            "number": number,
            "key": target.mouse.key.to_string(),
            "active_route": target.mouse.active_route().route.to_string(),
            "sent_via": {
                "route": route.route.to_string(),
                "node": route.path.display().to_string(),
                "interface": route.interface,
            },
            "routes": target.mouse.routes.iter().map(format::route_json).collect::<Vec<_>>(),
        }),
    );
}
