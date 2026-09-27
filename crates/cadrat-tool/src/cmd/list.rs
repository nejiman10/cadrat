//! `list` (spec 03 §3).

use cadrat_hidraw::{Inventory, NodeStatus, SlotsRead, enumerate};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::render::{self, Redactor};

/// Enumerates devices, reporting unreadable nodes with the udev hint.
pub fn inventory(ctx: &mut Ctx) -> Result<Inventory, Failure> {
    let inventory = enumerate(ctx.env.system)
        .map_err(|e| Failure::new(Exit::IoError, format!("cannot list hidraw nodes: {e}")))?;
    for node in &inventory.nodes {
        let status = match &node.status {
            NodeStatus::Candidate { .. } => "candidate".to_owned(),
            NodeStatus::Rejected(reason) => format!("rejected ({reason})"),
            NodeStatus::Inaccessible(errno) => format!("inaccessible ({errno})"),
        };
        ctx.verbose(format!("node {}: {status}", node.info.path.display()));
    }
    for warning in &inventory.warnings {
        ctx.warn(warning.code(), warning.to_string());
    }
    let blocked: Vec<String> = inventory
        .inaccessible()
        .map(|n| n.info.path.display().to_string())
        .collect();
    if !blocked.is_empty() {
        ctx.warn(
            "W-INACCESSIBLE",
            format!(
                "cannot open {}; install udev/69-cadrat.rules and reconnect the device",
                blocked.join(", ")
            ),
        );
    }
    Ok(inventory)
}

pub fn run(ctx: &mut Ctx, nodes: bool, redact: bool) -> Result<(), Failure> {
    let inventory = inventory(ctx)?;
    let mut redactor = Redactor::new(redact);

    let mice: Vec<Value> = inventory
        .mice
        .iter()
        .enumerate()
        .map(|(i, m)| render::mouse_json(m, Some(i + 1), &mut redactor))
        .collect();
    ctx.set("mice", mice);
    let receivers: Vec<Value> = inventory
        .receivers
        .iter()
        .map(|r| render::receiver_json(r, &inventory, &mut redactor))
        .collect();
    ctx.set("receivers", receivers);

    if inventory.mice.is_empty() {
        ctx.out("no mice found");
    } else {
        let width = inventory
            .mice
            .iter()
            .map(|m| redactor.key(m).len())
            .max()
            .unwrap_or(0)
            .max("MOUSE".len());
        ctx.out(format!("#  {:<width$}  ACTIVE    ROUTES", "MOUSE"));
        for (i, mouse) in inventory.mice.iter().enumerate() {
            let key = redactor.key(mouse);
            let active = mouse.active_route().route.to_string();
            for (j, route) in mouse.routes.iter().enumerate() {
                let route = render::route(route);
                if j == 0 {
                    ctx.out(format!("{:<2} {key:<width$}  {active:<8}  {route}", i + 1));
                } else {
                    ctx.out(format!("{:<2} {:<width$}  {:<8}  {route}", "", "", ""));
                }
            }
        }
    }
    if !inventory.receivers.is_empty() {
        ctx.out("");
        ctx.out("RECEIVER        SLOTS  MANAGEMENT");
        for receiver in &inventory.receivers {
            let slots = match &receiver.slots {
                SlotsRead::Read(slots) => {
                    format!("{}/5", slots.iter().filter(|s| s.occupied()).count())
                }
                _ => "?".to_owned(),
            };
            let management = receiver.management.first().map_or_else(
                || "none".to_owned(),
                |&i| {
                    let node = &inventory.nodes[i];
                    format!(
                        "{} ({})",
                        node.info.path.display(),
                        render::interface(node.info.interface.unwrap_or_default())
                    )
                },
            );
            ctx.out(format!(
                "{:<15} {slots:<6} {management}",
                receiver.key.to_string()
            ));
        }
    }

    if nodes {
        let mut rows = Vec::new();
        ctx.out("");
        ctx.out("NODE            IF     PRODUCT    STATUS");
        for node in &inventory.nodes {
            let (status, detail) = node_status(&node.status, &mut redactor);
            let interface = node
                .info
                .interface
                .map_or_else(|| "?".to_owned(), render::interface);
            let product = node
                .product()
                .map_or_else(|| "?".to_owned(), |p| format!("256f:{p:04x}"));
            let line = if detail.is_empty() {
                status.to_owned()
            } else {
                format!("{status} ({detail})")
            };
            ctx.out(format!(
                "{:<15} {interface:<6} {product:<10} {line}",
                node.info.path.display().to_string()
            ));
            rows.push(json!({
                "node": node.info.path.display().to_string(),
                "interface": node.info.interface,
                "product": node.product().map(|p| format!("256f:{p:04x}")),
                "usb_port": node.info.usb_port,
                "status": status,
                "detail": detail,
            }));
        }
        ctx.set("nodes", rows);
    }
    Ok(())
}

fn node_status(status: &NodeStatus, redactor: &mut Redactor) -> (&'static str, String) {
    match status {
        NodeStatus::Candidate {
            setting,
            management,
            device_id,
            setting_rejected,
        } => {
            let mut roles = Vec::new();
            match setting {
                Some(cadrat_hidraw::model::SettingRole::Wired) => {
                    roles.push("wired setting".to_owned());
                }
                Some(cadrat_hidraw::model::SettingRole::Receiver) => {
                    roles.push("receiver setting".to_owned());
                }
                None => {}
            }
            if let Some(caps) = management {
                roles.push(if caps.pairing {
                    "management".to_owned()
                } else {
                    "management (read only)".to_owned()
                });
            }
            if let Some(id) = device_id {
                roles.push(format!("id {}", redactor.id(*id)));
            }
            if let Some(reason) = setting_rejected {
                roles.push(format!("setting rejected: {reason}"));
            }
            ("candidate", roles.join(", "))
        }
        NodeStatus::Rejected(reason) => ("rejected", reason.to_string()),
        NodeStatus::Inaccessible(errno) => ("inaccessible", errno.to_string()),
    }
}
