//! The human-readable output, built only from a command's `--json` object
//! (spec tool/cli §5, P11).
//!
//! `cadrat-tool` and `cadratctl` both print what [`human`] returns, so they
//! show the same lines for the same result. What has to be shown while a
//! command runs (warnings, `-v` detail, prompts) is not here; it goes
//! through [`crate::Frontend`].

use std::fmt::Write as _;

use cadrat_config::Key;
use serde_json::Value;

use crate::request::{Command, Options};

/// The lines a command prints after it finished.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Human {
    /// Standard output: results, and informational lines unless `-q`.
    pub stdout: Vec<String>,
    /// Standard error: notes (unless `-q`), then the error and its hints.
    pub stderr: Vec<String>,
}

impl Human {
    fn out(&mut self, line: impl Into<String>) {
        self.stdout.push(line.into());
    }

    fn note(&mut self, options: &Options, message: &str) {
        if !options.quiet {
            self.stderr.push(format!("note: {message}"));
        }
    }
}

/// Builds the human-readable output of `command` from its `--json` object.
///
/// `program` is the command name used in guidance (`cadrat-tool` or
/// `cadratctl`). Fields are shown when present, so a failed command shows
/// what it had done before it failed, as it did when it printed directly.
#[must_use]
pub fn human(program: &str, options: &Options, command: &Command, result: &Value) -> Human {
    let mut human = Human::default();
    let mut info = Vec::new();
    let ok = result["ok"].as_bool().unwrap_or(false);
    match command {
        Command::List { nodes, .. } => list(&mut human, result, *nodes),
        Command::Init { .. } => {
            if ok {
                info.push(format!("created {}", text(&result["path"])));
                let note = if result["preset"].is_null() {
                    "every value is commented out; edit the file before sending"
                } else {
                    "these values are a research baseline, not values read from the mouse"
                };
                human.note(options, note);
            }
        }
        Command::Get {
            keys, values_only, ..
        } => get(&mut human, result, keys, *values_only),
        Command::Check => {
            if ok {
                info.push(format!("ok: {}", text(&result["path"])));
            }
        }
        Command::Set { dry_run, .. } | Command::Apply { dry_run } => {
            if *dry_run {
                dry_run_lines(&mut human, &mut info, result);
            } else {
                send_lines(&mut info, result, ok, program);
            }
        }
        Command::ReceiverSlots { .. } => {
            if let Some(slots) = result["slots"].as_array() {
                receiver_header(&mut human, result);
                human.stdout.extend(slots.iter().map(slot_line));
            }
        }
        Command::ReceiverPair { .. } => {
            receiver_header(&mut human, result);
            if ok {
                let slots: Vec<String> = array(&result["new_slots"]).iter().map(text).collect();
                human.out(format!("paired  slot {}", slots.join(", ")));
                info.push(format!(
                    "next    run `{program} list`; the new mouse is expected on the interface \
                     with the same number as its slot"
                ));
                info.push(format!(
                    "        then send your settings with `{program} apply --mouse=<number or key>`"
                ));
                info.push(
                    "        the first send after pairing is sometimes lost; if nothing changes, \
                     send it again"
                        .to_owned(),
                );
            }
        }
        Command::ReceiverUnpair { slot, .. } => {
            receiver_header(&mut human, result);
            if ok {
                human.out(format!("unpaired slot {slot}"));
                human
                    .stdout
                    .extend(array(&result["slots_after"]).iter().map(slot_line));
            }
        }
    }
    if !options.quiet {
        human.stdout.extend(info);
    }
    if let Some(error) = result["error"].as_object() {
        human
            .stderr
            .push(format!("error: {}", text(&error["message"])));
        for hint in array(&error["hints"]) {
            human.stderr.push(format!("hint: {}", text(hint)));
        }
    }
    human
}

/// One slot as `slot 2  occupied  type 0x59  id 0a1b2c3d4e5f   → mouse 1 (c658:…)`.
pub(crate) fn slot_line(slot: &Value) -> String {
    if slot["occupied"].as_bool() != Some(true) {
        return format!("slot {}  empty", text(&slot["slot"]));
    }
    let mut line = format!(
        "slot {}  occupied  type {}  id {}",
        text(&slot["slot"]),
        text(&slot["device_type"]),
        text(&slot["id"])
    );
    if slot["mouse"].is_object() {
        let mouse = &slot["mouse"];
        let _ = write!(
            line,
            "   → mouse {} ({})",
            text(&mouse["number"]),
            text(&mouse["key"])
        );
    }
    line
}

/// A JSON scalar as plain text (strings without quotes).
fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

/// `MI_01`, or `MI_00` when unknown.
fn interface(value: &Value) -> String {
    format!("MI_{:02}", value.as_u64().unwrap_or(0))
}

/// `wired (/dev/hidraw5, MI_01)` or
/// `receiver standby (recv:port-3-2 slot 3, /dev/hidraw9, MI_03)`.
fn route(route: &Value) -> String {
    let state = if route["state"] == "standby" {
        " standby"
    } else {
        ""
    };
    let receiver = match (&route["receiver"], &route["slot"]) {
        (Value::Null, _) => String::new(),
        (key, Value::Null) => format!("{} slot ?, ", text(key)),
        (key, slot) => format!("{} slot {}, ", text(key), text(slot)),
    };
    let ambiguous = if route["ambiguous_node"] == true {
        " ambiguous-node"
    } else {
        ""
    };
    format!(
        "{}{state} ({receiver}{}, {}){ambiguous}",
        text(&route["route"]),
        text(&route["node"]),
        interface(&route["interface"])
    )
}

fn list(human: &mut Human, result: &Value, nodes: bool) {
    let Some(mice) = result["mice"].as_array() else {
        return;
    };
    if mice.is_empty() {
        human.out("no mice found");
    } else {
        let width = mice
            .iter()
            .map(|m| text(&m["key"]).len())
            .max()
            .unwrap_or(0)
            .max("MOUSE".len());
        human.out(format!("#  {:<width$}  ACTIVE    ROUTES", "MOUSE"));
        for mouse in mice {
            let number = text(&mouse["number"]);
            let key = text(&mouse["key"]);
            let active = text(&mouse["active_route"]);
            for (j, r) in array(&mouse["routes"]).iter().enumerate() {
                let r = route(r);
                if j == 0 {
                    human.out(format!("{number:<2} {key:<width$}  {active:<8}  {r}"));
                } else {
                    human.out(format!("{:<2} {:<width$}  {:<8}  {r}", "", "", ""));
                }
            }
        }
    }
    let receivers = array(&result["receivers"]);
    if !receivers.is_empty() {
        human.out("");
        let width = receivers
            .iter()
            .map(|r| text(&r["key"]).len())
            .max()
            .unwrap_or(0)
            .max("RECEIVER".len());
        human.out(format!("{:<width$}  SLOTS  MANAGEMENT", "RECEIVER"));
        for receiver in receivers {
            let slots = receiver["slots"].as_array().map_or_else(
                || "?".to_owned(),
                |slots| {
                    let occupied = slots.iter().filter(|s| s["occupied"] == true).count();
                    format!("{occupied}/5")
                },
            );
            let management = &receiver["management"];
            let management = if management.is_object() {
                format!(
                    "{} ({})",
                    text(&management["node"]),
                    interface(&management["interface"])
                )
            } else {
                "none".to_owned()
            };
            human.out(format!(
                "{:<width$}  {slots:<5}  {management}",
                text(&receiver["key"])
            ));
        }
    }
    if nodes {
        let rows = array(&result["nodes"]);
        human.out("");
        let width = rows
            .iter()
            .map(|n| text(&n["node"]).len())
            .max()
            .unwrap_or(0)
            .max("NODE".len());
        human.out(format!("{:<width$}  IF     PRODUCT    STATUS", "NODE"));
        for row in rows {
            let interface = if row["interface"].is_null() {
                "?".to_owned()
            } else {
                interface(&row["interface"])
            };
            let product = if row["product"].is_null() {
                "?".to_owned()
            } else {
                text(&row["product"])
            };
            let detail = text(&row["detail"]);
            let status = if detail.is_empty() {
                text(&row["status"])
            } else {
                format!("{} ({detail})", text(&row["status"]))
            };
            human.out(format!(
                "{:<width$}  {interface:<6} {product:<10} {status}",
                text(&row["node"])
            ));
        }
    }
}

fn get(human: &mut Human, result: &Value, keys: &[String], values_only: bool) {
    let Some(values) = result["values"].as_object() else {
        return;
    };
    let keys: Vec<Key> = if keys.is_empty() {
        Key::ALL.to_vec()
    } else {
        keys.iter().filter_map(|k| k.parse().ok()).collect()
    };
    for key in keys {
        let key = key.to_string();
        if let Some(value) = values.get(&key) {
            human.out(if values_only {
                text(value)
            } else {
                format!("{key}={}", text(value))
            });
        }
    }
    wire_lines(human, result);
}

/// `00 11 22 …` from the unspaced `wire_hex`.
fn spaced(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn wire_lines(human: &mut Human, result: &Value) {
    let Some(layout) = result["wire_layout"].as_array() else {
        return;
    };
    human.out(format!("wire    {}", spaced(&text(&result["wire_hex"]))));
    for row in layout {
        human.out(format!(
            "        {:<7} {:<33}  {}",
            text(&row["offset"]),
            text(&row["bytes"]),
            text(&row["meaning"])
        ));
    }
}

fn change_line(change: &Value) -> String {
    format!(
        "change  {}={} → {}",
        text(&change["key"]),
        text(&change["from"]),
        text(&change["to"])
    )
}

fn dry_run_lines(human: &mut Human, info: &mut Vec<String>, result: &Value) {
    if result["wire_layout"].is_null() {
        return;
    }
    for change in array(&result["changes"]) {
        let mut line = change_line(change);
        if change["from"] == change["to"] {
            line.push_str(" (unchanged)");
        }
        human.out(line);
    }
    wire_lines(human, result);
    info.push("dry run: nothing was sent or saved".to_owned());
}

fn send_lines(info: &mut Vec<String>, result: &Value, ok: bool, program: &str) {
    let mouse = &result["mouse"];
    if !mouse.is_object() {
        return;
    }
    let number = if mouse["number"].is_null() {
        "-".to_owned()
    } else {
        text(&mouse["number"])
    };
    let via = &mouse["sent_via"];
    info.push(format!(
        "mouse   {number}  {}  via {} ({})",
        text(&mouse["key"]),
        text(&via["route"]),
        interface(&via["interface"])
    ));
    if result["sent"] == true {
        for change in array(&result["changes"]) {
            if change["from"] != change["to"] {
                info.push(change_line(change));
            }
        }
        info.push(format!("sent    {}", spaced(&text(&result["wire_hex"]))));
    }
    if result["saved"] == true {
        info.push(format!("saved   {}", text(&result["path"])));
    }
    if !ok {
        return;
    }
    if let Some(standby) = array(&mouse["routes"])
        .iter()
        .find(|r| r["state"] == "standby")
    {
        info.push(format!(
            "note    the {} route is on standby; run `{program} apply` after switching modes",
            text(&standby["route"])
        ));
    }
    if via["route"] == "receiver" {
        // Spec device §7 step 7 (Q7): a send through the Receiver is sometimes
        // lost, and nothing can read the setting back to tell.
        info.push(
            "note    a send through the Receiver can take about 30 s to show and is sometimes lost;"
                .to_owned(),
        );
        info.push("        if nothing changes, run the same command again".to_owned());
    }
}

fn receiver_header(human: &mut Human, result: &Value) {
    let receiver = &result["receiver"];
    if receiver.is_object() {
        human.out(format!(
            "receiver {}  ({}, {})",
            text(&receiver["key"]),
            text(&receiver["node"]),
            interface(&receiver["interface"])
        ));
    }
}
