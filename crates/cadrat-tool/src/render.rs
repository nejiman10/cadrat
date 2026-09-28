//! Shared formatting of devices, slots and wire reports.

use std::collections::HashMap;
use std::fmt::Write as _;

use cadrat_config::{Config, Key};
use cadrat_hidraw::{Inventory, Mouse, MouseRoute, Receiver, RouteState, SlotMatch, SlotsRead};
use cadrat_proto::{ButtonName, DeviceId, SlotReport};
use serde_json::{Value, json};

/// Space-separated lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hides device IDs behind stable placeholders (`--redact`).
#[derive(Debug, Default)]
pub struct Redactor {
    enabled: bool,
    seen: HashMap<DeviceId, usize>,
}

impl Redactor {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            seen: HashMap::new(),
        }
    }

    /// The ID, or `id-N` where equal IDs get the same N.
    pub fn id(&mut self, id: DeviceId) -> String {
        if !self.enabled {
            return id.to_string();
        }
        let next = self.seen.len() + 1;
        format!("id-{}", self.seen.entry(id).or_insert(next))
    }

    /// A mouse key with its ID redacted.
    pub fn key(&mut self, mouse: &Mouse) -> String {
        match mouse.device_id {
            Some(id) => format!("c658:{}", self.id(id)),
            None => mouse.key.to_string(),
        }
    }
}

/// `MI_01`.
pub fn interface(n: u8) -> String {
    format!("MI_{n:02}")
}

/// `wired (/dev/hidraw5, MI_01)` or
/// `receiver standby (recv:port-3-2 slot 3, /dev/hidraw9, MI_03)`.
pub fn route(route: &MouseRoute) -> String {
    let state = match route.state {
        RouteState::Active => String::new(),
        RouteState::Standby => " standby".to_owned(),
    };
    let receiver = match (&route.receiver, route.slot) {
        (Some(key), Some(SlotMatch::Slot(slot))) => format!("{key} slot {slot}, "),
        (Some(key), _) => format!("{key} slot ?, "),
        (None, _) => String::new(),
    };
    let ambiguous = if route.ambiguous {
        " ambiguous-node"
    } else {
        ""
    };
    format!(
        "{}{state} ({receiver}{}, {}){ambiguous}",
        route.route,
        route.path.display(),
        interface(route.interface)
    )
}

pub fn route_json(route: &MouseRoute) -> Value {
    json!({
        "route": route.route.to_string(),
        "state": route.state.to_string(),
        "node": route.path.display().to_string(),
        "interface": route.interface,
        "receiver": route.receiver.as_ref().map(ToString::to_string),
        "slot": match route.slot {
            Some(SlotMatch::Slot(slot)) => json!(slot.get()),
            _ => Value::Null,
        },
        "ambiguous_node": route.ambiguous,
    })
}

pub fn mouse_json(mouse: &Mouse, number: Option<usize>, redactor: &mut Redactor) -> Value {
    json!({
        "number": number,
        "key": redactor.key(mouse),
        "device_id": mouse.device_id.map(|id| redactor.id(id)),
        "active_route": mouse.active_route().route.to_string(),
        "routes": mouse.routes.iter().map(route_json).collect::<Vec<_>>(),
    })
}

/// The `list` number of the mouse whose device ID equals a slot identifier.
pub fn mouse_for_slot<'a>(
    inventory: &'a Inventory,
    slot: &SlotReport,
) -> Option<(usize, &'a Mouse)> {
    if !slot.occupied() {
        return None;
    }
    inventory
        .mice
        .iter()
        .enumerate()
        .find(|(_, m)| m.device_id == Some(slot.id_candidate()))
        .map(|(i, m)| (i + 1, m))
}

/// One `receiver slots` line, e.g.
/// `slot 2  occupied  type 0x59  id 0a1b2c3d4e5f   → mouse 1 (c658:0a1b2c3d4e5f)`.
pub fn slot_line(
    slot: &SlotReport,
    mouse: Option<(usize, &Mouse)>,
    redactor: &mut Redactor,
) -> String {
    if !slot.occupied() {
        return format!("slot {}  empty", slot.slot());
    }
    let mut line = format!(
        "slot {}  occupied  type 0x{:02x}  id {}",
        slot.slot(),
        slot.device_type(),
        redactor.id(slot.id_candidate())
    );
    if let Some((number, mouse)) = mouse {
        let key = redactor.key(mouse);
        let _ = write!(line, "   → mouse {number} ({key})");
    }
    line
}

pub fn slot_json(
    slot: &SlotReport,
    mouse: Option<(usize, &Mouse)>,
    redactor: &mut Redactor,
) -> Value {
    json!({
        "slot": slot.slot().get(),
        "occupied": slot.occupied(),
        "device_type": format!("0x{:02x}", slot.device_type()),
        "id": slot.occupied().then(|| redactor.id(slot.id_candidate())),
        "mouse": mouse.map(|(number, m)| json!({"number": number, "key": redactor.key(m)})),
    })
}

pub fn slots_json(slots: &[SlotReport], inventory: &Inventory, redactor: &mut Redactor) -> Value {
    Value::Array(
        slots
            .iter()
            .map(|s| slot_json(s, mouse_for_slot(inventory, s), redactor))
            .collect(),
    )
}

pub fn receiver_json(receiver: &Receiver, inventory: &Inventory, redactor: &mut Redactor) -> Value {
    let management = receiver.management.first().map(|&i| {
        let node = &inventory.nodes[i];
        json!({
            "node": node.info.path.display().to_string(),
            "interface": node.info.interface,
        })
    });
    let (slots, slots_error) = match &receiver.slots {
        SlotsRead::Read(slots) => (slots_json(slots, inventory, redactor), Value::Null),
        SlotsRead::NoManagementNode => (Value::Null, json!("no management node")),
        SlotsRead::Failed(message) => (Value::Null, json!(message)),
    };
    json!({
        "key": receiver.key.to_string(),
        "management": management,
        "slots": slots,
        "slots_error": slots_error,
    })
}

/// The wire report laid out by field, as `(wire offsets, bytes, meaning)`.
pub fn wire_layout(config: &Config, wire: &[u8; 32]) -> Vec<(String, String, String)> {
    let mut rows = vec![
        ("0".to_owned(), hex(&wire[0..1]), "report ID".to_owned()),
        ("1".to_owned(), hex(&wire[1..2]), "reserved".to_owned()),
        (
            "2".to_owned(),
            hex(&wire[2..3]),
            format!("{}={}", Key::Dpi, config.dpi),
        ),
        (
            "3".to_owned(),
            hex(&wire[3..4]),
            if config.lift_enabled {
                format!("{}={}", Key::LiftThreshold, config.lift_threshold)
            } else {
                "mouse.lift.enabled=false".to_owned()
            },
        ),
        (
            "4..7".to_owned(),
            hex(&wire[4..8]),
            format!("{}={}", Key::Wheel, config.wheel),
        ),
        ("8..18".to_owned(), hex(&wire[8..19]), "reserved".to_owned()),
    ];
    for name in ButtonName::ALL {
        let offset = name.blob_offset() + 1;
        rows.push((
            offset.to_string(),
            hex(&wire[offset..=offset]),
            format!("{}={}", Key::Button(name), config.buttons.get(name)),
        ));
    }
    rows.extend([
        ("26".to_owned(), hex(&wire[26..27]), "reserved".to_owned()),
        ("27".to_owned(), hex(&wire[27..28]), "fixed".to_owned()),
        (
            "28..30".to_owned(),
            hex(&wire[28..31]),
            "reserved".to_owned(),
        ),
        (
            "31".to_owned(),
            hex(&wire[31..32]),
            format!("{}={}", Key::PollingRate, config.polling_rate),
        ),
    ]);
    rows
}

/// Human lines for [`wire_layout`].
pub fn wire_lines(config: &Config, wire: &[u8; 32]) -> Vec<String> {
    let mut lines = vec![format!("wire    {}", hex(wire))];
    for (offset, bytes, meaning) in wire_layout(config, wire) {
        lines.push(format!("        {offset:<7} {bytes:<33}  {meaning}"));
    }
    lines
}

/// JSON value of a setting: integers as numbers, the rest as strings.
pub fn value_json(value: cadrat_config::Value) -> Value {
    use cadrat_config::Value as V;
    match value {
        V::Dpi(dpi) => json!(dpi.get()),
        V::PollingRate(rate) => json!(rate.hz()),
        V::Threshold(t) => json!(t),
        V::Bool(b) => json!(b),
        V::Wheel(_) | V::Action(_) => json!(value.to_string()),
    }
}
