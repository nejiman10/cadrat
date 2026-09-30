//! Checks `cadrat-proto` against the research SDK's test vectors in
//! `vectors/` (spec implementation §3). See `vectors/README.md` for their origin.

#![allow(missing_docs)]

use std::path::PathBuf;

use cadrat_proto::report10::inspect;
use cadrat_proto::{
    Action, Buttons, DirectAction, Dpi, HostIndex, Lift, PollingRate, Report03Frame,
    Report10Config, ReportLengths, Slot, WheelMode, WireButton, receiver,
};
use serde_json::{Map, Value};

const SETS: [&str; 3] = ["research", "boundary", "real"];

fn load(set: &str, file: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vectors")
        .join(set)
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn output(set: &str) -> Value {
    let output = load(set, "output.json");
    assert_eq!(output["format_version"], 1, "{set}");
    output
}

fn array<'a>(value: &'a Value, key: &str) -> &'a Vec<Value> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is not an array"))
}

fn uint(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("{value} is not an unsigned integer"))
}

fn byte(value: &Value) -> u8 {
    u8::try_from(uint(value)).unwrap()
}

fn text(value: &Value) -> &str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{value} is not a string"))
}

fn unhex(value: &Value) -> Vec<u8> {
    text(value)
        .split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn direct(name: &str) -> DirectAction {
    match name {
        "HID_MOUSE_LEFT" => DirectAction::Left,
        "HID_MOUSE_RIGHT" => DirectAction::Right,
        "HID_MOUSE_MIDDLE_OR_WHEEL_BUTTON" => DirectAction::Middle,
        "HID_MOUSE_BACKWARD" => DirectAction::Backward,
        "HID_MOUSE_FORWARD" => DirectAction::Forward,
        "UNKNOWN_DIRECT_CODE_6" => DirectAction::Unknown6,
        _ => panic!("unknown direct action {name}"),
    }
}

/// The SDK's `InspectedButton.name` and `supported_for_encoding`.
fn sdk_button(button: WireButton) -> (String, bool) {
    match button {
        WireButton::Action(Action::Direct(action)) => {
            let name = match action {
                DirectAction::Left => "HID_MOUSE_LEFT",
                DirectAction::Right => "HID_MOUSE_RIGHT",
                DirectAction::Middle => "HID_MOUSE_MIDDLE_OR_WHEEL_BUTTON",
                DirectAction::Backward => "HID_MOUSE_BACKWARD",
                DirectAction::Forward => "HID_MOUSE_FORWARD",
                DirectAction::Unknown6 => "UNKNOWN_DIRECT_CODE_6",
            };
            (name.to_owned(), true)
        }
        WireButton::Action(Action::HostRouted(index)) => {
            (format!("HOST_ROUTED_INDEX_{}", index.get()), true)
        }
        WireButton::Action(Action::Raw(raw)) => (
            format!("UNKNOWN_UNREACHABLE_DIRECT_WIRE_0x{:02X}", raw.get()),
            false,
        ),
        WireButton::Invalid(wire) => (format!("UNKNOWN_INVALID_WIRE_0x{wire:02X}"), false),
    }
}

fn config(input: &Value) -> Report10Config {
    let buttons: Vec<Action> = array(input, "buttons")
        .iter()
        .map(
            |button| match (button.get("direct"), button.get("host_index")) {
                (Some(name), None) => Action::Direct(direct(text(name))),
                (None, Some(index)) => Action::HostRouted(HostIndex::new(byte(index)).unwrap()),
                _ => panic!("bad button {button}"),
            },
        )
        .collect();
    let dpi = u16::try_from(uint(&input["dpi"])).unwrap();
    Report10Config {
        // Vectors hold only valid DPI values; clamping is tested in Rust only.
        dpi: Dpi::new(dpi).unwrap_or_else(|| panic!("vector DPI {dpi} is not valid")),
        lift: match &input["lift_threshold"] {
            Value::Null => Lift::Disabled,
            threshold => Lift::Enabled {
                threshold: byte(threshold),
            },
        },
        wheel: match text(&input["wheel_mode"]) {
            "NORMAL" => WheelMode::Normal,
            "INERTIAL" => WheelMode::Inertial,
            other => panic!("unknown wheel mode {other}"),
        },
        buttons: Buttons(buttons.try_into().unwrap()),
        polling_rate: PollingRate::from_hz(i64::try_from(uint(&input["polling_hz"])).unwrap())
            .unwrap(),
    }
}

#[test]
fn report10_encode_and_inspect() {
    let mut count = 0;
    for set in SETS {
        for vector in array(&output(set), "wire") {
            let name = format!("{set}/{}", text(&vector["name"]));
            let config = config(&vector["input"]);
            let wire = config.to_wire();
            assert_eq!(hex(&wire), text(&vector["wire_hex"]), "{name}: wire");

            let parsed = &vector["parsed"];
            let inspected = inspect(&wire).unwrap();
            assert_eq!(
                inspected.dpi_encoded,
                byte(&parsed["dpi_encoded"]),
                "{name}"
            );
            assert_eq!(
                u64::from(inspected.nominal_dpi),
                uint(&parsed["nominal_dpi"]),
                "{name}"
            );
            let (lift_enabled, lift_threshold) = match inspected.lift {
                Lift::Disabled => (false, 0x1f),
                Lift::Enabled { threshold } => (true, threshold),
            };
            assert_eq!(Value::Bool(lift_enabled), parsed["lift_enabled"], "{name}");
            assert_eq!(lift_threshold, byte(&parsed["lift_threshold"]), "{name}");
            let wheel = match inspected.wheel {
                WheelMode::Normal => "NORMAL",
                WheelMode::Inertial => "INERTIAL",
            };
            assert_eq!(wheel, text(&parsed["wheel_mode"]), "{name}");
            let buttons = array(parsed, "buttons");
            assert_eq!(buttons.len(), 7, "{name}");
            for (button, expected) in inspected.buttons.iter().zip(buttons) {
                let (sdk_name, supported) = sdk_button(*button);
                assert_eq!(
                    format!("0x{:02x}", button.wire()),
                    text(&expected["wire_value"]),
                    "{name}"
                );
                assert_eq!(sdk_name, text(&expected["name"]), "{name}");
                assert_eq!(
                    Value::Bool(supported),
                    expected["supported_for_encoding"],
                    "{name}"
                );
            }
            assert_eq!(
                inspected.polling_rate.divider(),
                byte(&parsed["polling_divider"]),
                "{name}"
            );
            assert_eq!(
                Value::Bool(inspected.reserved_bytes_are_zero),
                parsed["reserved_bytes_are_zero"],
                "{name}"
            );
            assert_eq!(
                Value::Bool(inspected.fixed_field_is_valid),
                parsed["fixed_field_is_valid"],
                "{name}"
            );

            // Decoding gives back the configuration, except that an enabled
            // threshold of 0x1f encodes like disabled (W-LIFT-AMBIGUOUS).
            let mut expected = config;
            if expected.lift.is_ambiguous() {
                expected.lift = Lift::Disabled;
            }
            assert_eq!(inspected.to_config(), Some(expected), "{name}");
            count += 1;
        }
    }
    assert!(count > 0);
}

fn lengths(map: &Value) -> Vec<(u8, u64)> {
    let map: &Map<String, Value> = map.as_object().unwrap();
    map.iter()
        .map(|(id, len)| {
            let id = u8::from_str_radix(id.strip_prefix("0x").unwrap(), 16).unwrap();
            (id, uint(len))
        })
        .collect()
}

#[test]
fn descriptor_lengths() {
    let mut count = 0;
    for set in SETS {
        for vector in array(&output(set), "descriptors") {
            let name = format!("{set}/{}", text(&vector["name"]));
            let parsed = ReportLengths::parse(&unhex(&vector["input_hex"]))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            // Keys are sorted by Report ID on both sides. top_level_usages is
            // not part of cadrat-proto and is not compared.
            assert_eq!(
                parsed.features().collect::<Vec<_>>(),
                lengths(&vector["feature_wire_lengths"]),
                "{name}: feature"
            );
            assert_eq!(
                parsed.inputs().collect::<Vec<_>>(),
                lengths(&vector["input_wire_lengths"]),
                "{name}: input"
            );
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn report03_parse() {
    let mut count = 0;
    for set in SETS {
        for vector in array(&output(set), "report03") {
            let name = format!("{set}/{}", text(&vector["name"]));
            let frame = Report03Frame::parse(
                &unhex(&vector["input_hex"]),
                byte(&vector["previous_bitmap"]),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(frame.bitmap, byte(&vector["bitmap"]), "{name}");
            assert_eq!(frame.pressed, byte(&vector["pressed_mask"]), "{name}");
            assert_eq!(frame.released, byte(&vector["released_mask"]), "{name}");
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn receiver_packets() {
    let mut count = 0;
    for set in SETS {
        for vector in array(&output(set), "receiver") {
            let input = &vector["input"];
            let packet = match text(&input["operation"]) {
                "pair_start" => receiver::PAIR_START,
                "pair_stop" => receiver::PAIR_STOP,
                "unpair" => Slot::new(byte(&input["slot"])).unwrap().unpair_packet(),
                other => panic!("{set}: unknown operation {other}"),
            };
            assert_eq!(hex(&packet), text(&vector["packet_hex"]), "{set}: {input}");
            count += 1;
        }
    }
    assert!(count > 0);
}

/// `output.json` must be the export of the `input.json` beside it: same
/// entries in the same order, with the input carried over.
#[test]
fn outputs_match_inputs() {
    for set in SETS {
        let input = load(set, "input.json");
        let output = output(set);
        let names = |value: &Value, key: &str| -> Vec<String> {
            array(value, key)
                .iter()
                .map(|item| item["name"].to_string())
                .collect()
        };
        for key in ["wire", "descriptors", "report03"] {
            assert_eq!(names(&input, key), names(&output, key), "{set}/{key}");
        }
        for (i, o) in array(&input, "wire").iter().zip(array(&output, "wire")) {
            assert_eq!(i["config"], o["input"], "{set}");
        }
        for (i, o) in array(&input, "descriptors")
            .iter()
            .zip(array(&output, "descriptors"))
        {
            assert_eq!(unhex(&i["hex"]), unhex(&o["input_hex"]), "{set}");
        }
        for (i, o) in array(&input, "report03")
            .iter()
            .zip(array(&output, "report03"))
        {
            assert_eq!(unhex(&i["hex"]), unhex(&o["input_hex"]), "{set}");
            assert_eq!(i["previous_bitmap"], o["previous_bitmap"], "{set}");
        }
        let receiver_inputs: Vec<&Value> = array(&output, "receiver")
            .iter()
            .map(|o| &o["input"])
            .collect();
        assert_eq!(
            array(&input, "receiver").iter().collect::<Vec<_>>(),
            receiver_inputs,
            "{set}/receiver"
        );
    }
}
