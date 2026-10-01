//! `list` (spec tool/cli §3).

use cadrat_hidraw::{Inventory, NodeStatus, enumerate};
use serde_json::{Value, json};

use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::format::{self, Redactor};

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
        .map(|(i, m)| format::mouse_json(m, Some(i + 1), &mut redactor))
        .collect();
    ctx.set("mice", mice);
    let receivers: Vec<Value> = inventory
        .receivers
        .iter()
        .map(|r| format::receiver_json(r, &inventory, &mut redactor))
        .collect();
    ctx.set("receivers", receivers);

    if nodes {
        let rows: Vec<Value> = inventory
            .nodes
            .iter()
            .map(|node| {
                let (status, detail) = node_status(&node.status, &mut redactor);
                json!({
                    "node": node.info.path.display().to_string(),
                    "interface": node.info.interface,
                    "product": node.product().map(|p| format!("256f:{p:04x}")),
                    "usb_port": node.info.usb_port,
                    "status": status,
                    "detail": detail,
                })
            })
            .collect();
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
