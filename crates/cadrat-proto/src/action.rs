//! Button actions: the one-byte wire value of each Report `0x10` button entry.
//!
//! The string forms (`mouse:left`, `unknown:6`, `host:1`, `raw:0x10`) are the
//! same in TOML and JSON (spec 01 §5, spec 04 §2).

use core::fmt;
use core::str::FromStr;

use crate::int::parse_u32;

/// Direct actions reachable in the normal C658 configuration path.
///
/// `CONFIRMED`: action codes 1..5 map to wire `0x0a..0x0e`; code 6 maps to
/// wire `0x0f` and is written by the C658 default initializer. The formal name
/// of code 6 is `UNKNOWN`, so it is called [`DirectAction::Unknown6`] and never
/// given a descriptive name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectAction {
    /// `mouse:left`, code 1, wire `0x0a`.
    Left,
    /// `mouse:right`, code 2, wire `0x0b`.
    Right,
    /// `mouse:middle`, code 3, wire `0x0c` (middle or wheel button).
    Middle,
    /// `mouse:backward`, code 4, wire `0x0d`.
    Backward,
    /// `mouse:forward`, code 5, wire `0x0e`.
    Forward,
    /// `unknown:6`, code 6, wire `0x0f`. Meaning `UNKNOWN`.
    Unknown6,
}

impl DirectAction {
    /// All direct actions in code order.
    pub const ALL: [Self; 6] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Backward,
        Self::Forward,
        Self::Unknown6,
    ];

    /// Static action code (1..6).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 3,
            Self::Backward => 4,
            Self::Forward => 5,
            Self::Unknown6 => 6,
        }
    }

    /// Wire value, `0x09 + code` (`CONFIRMED`).
    #[must_use]
    pub const fn wire(self) -> u8 {
        0x09 + self.code()
    }

    /// Looks up a direct action by its wire value (`0x0a..=0x0f`).
    #[must_use]
    pub const fn from_wire(wire: u8) -> Option<Self> {
        match wire {
            0x0a => Some(Self::Left),
            0x0b => Some(Self::Right),
            0x0c => Some(Self::Middle),
            0x0d => Some(Self::Backward),
            0x0e => Some(Self::Forward),
            0x0f => Some(Self::Unknown6),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "mouse:left",
            Self::Right => "mouse:right",
            Self::Middle => "mouse:middle",
            Self::Backward => "mouse:backward",
            Self::Forward => "mouse:forward",
            Self::Unknown6 => "unknown:6",
        }
    }
}

/// Host-routed action index, 0..=215.
///
/// `CONFIRMED`: encoded as wire `0x28 + index`. The native generator uses an
/// 8-bit result; indices above 215 would wrap and are not representable.
/// Only indices 1..=7 have a bit in the Input Report `0x03` bitmap (`OBSERVED`
/// on both routes); see [`HostIndex::report03_mask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostIndex(u8);

impl HostIndex {
    /// Largest valid index.
    pub const MAX: u8 = 0xd7;

    /// Returns the index if it is in 0..=215.
    #[must_use]
    pub const fn new(index: u8) -> Option<Self> {
        if index <= Self::MAX {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The index value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Wire value, `0x28 + index`.
    #[must_use]
    pub const fn wire(self) -> u8 {
        0x28 + self.0
    }

    /// The Input Report `0x03` bit this index sets when pressed, if any.
    ///
    /// Indices 1..=7 map to bits 0..=6 (`OBSERVED`). Index 0 and 8 and above
    /// have no position in the bitmap.
    #[must_use]
    pub const fn report03_mask(self) -> Option<u8> {
        match self.0 {
            1..=7 => Some(1 << (self.0 - 1)),
            _ => None,
        }
    }
}

/// Raw wire value in `0x10..=0x27`.
///
/// These values are not produced by the normal configuration path and have no
/// named form. Values that do have a named form (`0x0a..=0x0f`, `0x28..`) are
/// not representable here, so each wire value has exactly one spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawWire(u8);

impl RawWire {
    /// Smallest accepted raw value.
    pub const MIN: u8 = 0x10;
    /// Largest accepted raw value.
    pub const MAX: u8 = 0x27;

    /// Returns the value if it is in `0x10..=0x27`.
    #[must_use]
    pub const fn new(wire: u8) -> Option<Self> {
        if wire >= Self::MIN && wire <= Self::MAX {
            Some(Self(wire))
        } else {
            None
        }
    }

    /// The wire value.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Action assigned to one physical button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// A direct mouse action (`mouse:*`, `unknown:6`).
    Direct(DirectAction),
    /// A host-routed action (`host:N`).
    HostRouted(HostIndex),
    /// A raw wire value without a named form (`raw:0xNN`).
    Raw(RawWire),
}

impl Action {
    /// Wire value written to the blob.
    #[must_use]
    pub const fn wire(self) -> u8 {
        match self {
            Self::Direct(action) => action.wire(),
            Self::HostRouted(index) => index.wire(),
            Self::Raw(raw) => raw.get(),
        }
    }

    /// Decodes a wire value. Returns `None` for `0x00..=0x09`, which no
    /// action produces.
    #[must_use]
    pub const fn from_wire(wire: u8) -> Option<Self> {
        if let Some(action) = DirectAction::from_wire(wire) {
            return Some(Self::Direct(action));
        }
        if let Some(raw) = RawWire::new(wire) {
            return Some(Self::Raw(raw));
        }
        if wire >= 0x28 {
            return Some(Self::HostRouted(HostIndex(wire - 0x28)));
        }
        None
    }
}

impl From<DirectAction> for Action {
    fn from(action: DirectAction) -> Self {
        Self::Direct(action)
    }
}

impl From<HostIndex> for Action {
    fn from(index: HostIndex) -> Self {
        Self::HostRouted(index)
    }
}

impl From<RawWire> for Action {
    fn from(raw: RawWire) -> Self {
        Self::Raw(raw)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct(action) => f.write_str(action.as_str()),
            Self::HostRouted(index) => write!(f, "host:{}", index.get()),
            Self::Raw(raw) => write!(f, "raw:0x{:02x}", raw.get()),
        }
    }
}

/// Why an action string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseActionError {
    /// Not of the form `<kind>:<argument>` with a known kind and argument.
    Syntax,
    /// `host:N` with N outside 0..=215.
    HostIndexOutOfRange,
    /// `raw:0xNN` with a value that has a named form; use that form instead.
    RawHasName(Action),
    /// `raw:0xNN` with a value that is not a button action (`0x00..=0x09`
    /// or above `0xff`).
    RawOutOfRange,
}

impl fmt::Display for ParseActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax => f.write_str(
                "expected mouse:left|right|middle|backward|forward, unknown:6, host:<0..215> or raw:<0x10..0x27>",
            ),
            Self::HostIndexOutOfRange => f.write_str("host index must be in range 0..215"),
            Self::RawHasName(action) => write!(f, "this raw value has a named form; write {action}"),
            Self::RawOutOfRange => f.write_str("raw value must be in range 0x10..0x27"),
        }
    }
}

impl core::error::Error for ParseActionError {}

impl FromStr for Action {
    type Err = ParseActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(action) = DirectAction::ALL.into_iter().find(|a| a.as_str() == s) {
            return Ok(Self::Direct(action));
        }
        let (kind, arg) = s.split_once(':').ok_or(ParseActionError::Syntax)?;
        match kind {
            "host" => {
                // Decimal only: host indices are written as `host:1`.
                if arg.starts_with("0x") {
                    return Err(ParseActionError::Syntax);
                }
                let index = parse_u32(arg).ok_or(ParseActionError::Syntax)?;
                u8::try_from(index)
                    .ok()
                    .and_then(HostIndex::new)
                    .map(Self::HostRouted)
                    .ok_or(ParseActionError::HostIndexOutOfRange)
            }
            "raw" => {
                // Hexadecimal only: raw values are wire bytes.
                if !arg.starts_with("0x") {
                    return Err(ParseActionError::Syntax);
                }
                let wire = parse_u32(arg).ok_or(ParseActionError::Syntax)?;
                let wire = u8::try_from(wire).map_err(|_| ParseActionError::RawOutOfRange)?;
                match Self::from_wire(wire) {
                    Some(Self::Raw(raw)) => Ok(Self::Raw(raw)),
                    Some(named) => Err(ParseActionError::RawHasName(named)),
                    None => Err(ParseActionError::RawOutOfRange),
                }
            }
            _ => Err(ParseActionError::Syntax),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Action, ParseActionError> {
        s.parse()
    }

    #[test]
    fn direct_wire_values() {
        let wires: Vec<u8> = DirectAction::ALL.iter().map(|a| a.wire()).collect();
        assert_eq!(wires, [0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]);
        for action in DirectAction::ALL {
            assert_eq!(DirectAction::from_wire(action.wire()), Some(action));
        }
    }

    #[test]
    fn host_bounds() {
        assert_eq!(HostIndex::new(0).map(HostIndex::wire), Some(0x28));
        assert_eq!(HostIndex::new(7).map(HostIndex::wire), Some(0x2f));
        assert_eq!(HostIndex::new(215).map(HostIndex::wire), Some(0xff));
        assert_eq!(HostIndex::new(216), None);
    }

    #[test]
    fn host_report03_mask() {
        let masks: Vec<Option<u8>> = (0..=8)
            .map(|i| HostIndex::new(i).unwrap().report03_mask())
            .collect();
        assert_eq!(
            masks,
            [
                None,
                Some(0x01),
                Some(0x02),
                Some(0x04),
                Some(0x08),
                Some(0x10),
                Some(0x20),
                Some(0x40),
                None
            ]
        );
    }

    #[test]
    fn from_wire_covers_every_byte() {
        for wire in 0..=u8::MAX {
            match Action::from_wire(wire) {
                None => assert!(wire < 0x0a),
                Some(action) => assert_eq!(action.wire(), wire),
            }
        }
    }

    #[test]
    fn string_round_trip_for_every_wire_value() {
        for wire in 0x0a..=u8::MAX {
            let action = Action::from_wire(wire).unwrap();
            let text = action.to_string();
            assert_eq!(parse(&text), Ok(action), "{text}");
        }
    }

    #[test]
    fn spellings() {
        assert_eq!(parse("mouse:left").unwrap().wire(), 0x0a);
        assert_eq!(parse("mouse:backward").unwrap().wire(), 0x0d);
        assert_eq!(parse("unknown:6").unwrap().wire(), 0x0f);
        assert_eq!(parse("host:0").unwrap().wire(), 0x28);
        assert_eq!(parse("host:1").unwrap().wire(), 0x29);
        assert_eq!(parse("host:215").unwrap().wire(), 0xff);
        assert_eq!(parse("raw:0x10").unwrap().wire(), 0x10);
        assert_eq!(parse("raw:0x27").unwrap().wire(), 0x27);
        assert_eq!(parse("raw:0x1A").unwrap().to_string(), "raw:0x1a");
    }

    #[test]
    fn rejects_bad_spellings() {
        for s in [
            "",
            "left",
            "Mouse:left",
            "mouse:LEFT",
            "mouse:back",
            "mouse:wheel",
            "unknown:7",
            "direct:6",
            "host:",
            "host:01",
            "host:0x01",
            "host:-1",
            "host: 1",
            "raw:16",
            "raw:0x",
            "radial",
        ] {
            assert_eq!(parse(s), Err(ParseActionError::Syntax), "{s:?}");
        }
    }

    #[test]
    fn host_out_of_range() {
        assert_eq!(
            parse("host:216"),
            Err(ParseActionError::HostIndexOutOfRange)
        );
        assert_eq!(parse("host:99999999999"), Err(ParseActionError::Syntax));
    }

    #[test]
    fn raw_with_named_form_is_rejected() {
        assert_eq!(
            parse("raw:0x0a"),
            Err(ParseActionError::RawHasName(Action::Direct(
                DirectAction::Left
            )))
        );
        assert_eq!(
            parse("raw:0x0f"),
            Err(ParseActionError::RawHasName(Action::Direct(
                DirectAction::Unknown6
            )))
        );
        assert_eq!(
            parse("raw:0x29"),
            Err(ParseActionError::RawHasName(Action::HostRouted(
                HostIndex::new(1).unwrap()
            )))
        );
        assert_eq!(
            ParseActionError::RawHasName(Action::HostRouted(HostIndex::new(1).unwrap()))
                .to_string(),
            "this raw value has a named form; write host:1"
        );
    }

    #[test]
    fn raw_out_of_range() {
        assert_eq!(parse("raw:0x00"), Err(ParseActionError::RawOutOfRange));
        assert_eq!(parse("raw:0x09"), Err(ParseActionError::RawOutOfRange));
        assert_eq!(parse("raw:0x100"), Err(ParseActionError::RawOutOfRange));
    }
}
