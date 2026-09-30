//! Feature Report `0x10`: the complete 31-byte configuration snapshot.
//!
//! `CONFIRMED`: the generator zero-fills the blob and sets offset 1 (DPI),
//! 2 (lift threshold), 3..6 (wheel), 18..24 (seven buttons), 26 (fixed
//! `0x1e`) and 30 (polling divider). The wire report is Report ID `0x10`
//! followed by the blob, 32 bytes in total. It is always a full snapshot,
//! never a patch (spec config §3).

use core::fmt;
use core::str::FromStr;

use crate::action::{Action, DirectAction};
use crate::int::parse_u32;

/// Report ID of the configuration Feature Report.
pub const REPORT_ID: u8 = 0x10;
/// Length of the blob.
pub const BLOB_LEN: usize = 31;
/// Length of the wire report (Report ID plus blob).
pub const WIRE_LEN: usize = 32;
/// Blob offset of the first button entry.
pub const BUTTON_OFFSET: usize = 18;
/// Fixed value at blob offset 26.
pub const FIXED_26: u8 = 0x1e;
/// Lift byte that means "disabled".
pub const LIFT_DISABLED: u8 = 0x1f;

/// Blob offsets that the generator always leaves zero.
const RESERVED: [usize; 16] = [0, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 25, 27, 28, 29];

/// DPI: 50..=8200 in steps of 50 (spec config §4.1).
///
/// Unlike the recovered generator, which clamps and floors, out-of-range and
/// non-multiple values are not representable. How values in range behave on
/// the mouse is `UNKNOWN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dpi(u16);

impl Dpi {
    /// Smallest DPI.
    pub const MIN: u16 = 50;
    /// Largest DPI.
    pub const MAX: u16 = 8200;
    /// Step between DPI values.
    pub const STEP: u16 = 50;

    /// Returns the DPI if it is in range and a multiple of 50.
    #[must_use]
    pub const fn new(dpi: u16) -> Option<Self> {
        if dpi >= Self::MIN && dpi <= Self::MAX && dpi.is_multiple_of(Self::STEP) {
            Some(Self(dpi))
        } else {
            None
        }
    }

    /// Decodes blob offset 1. Returns `None` for 0 and values above 164.
    #[must_use]
    pub const fn from_encoded(byte: u8) -> Option<Self> {
        // byte <= 164, so the product fits in u16.
        Self::new(byte as u16 * Self::STEP)
    }

    /// The DPI value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Blob offset 1: `dpi / 50`, in 1..=164.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn encoded(self) -> u8 {
        (self.0 / Self::STEP) as u8
    }
}

impl fmt::Display for Dpi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a DPI string was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseDpiError {
    /// Not an integer.
    Syntax,
    /// Outside 50..=8200.
    OutOfRange,
    /// Not a multiple of 50.
    NotMultiple,
}

impl fmt::Display for ParseDpiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "dpi must be an integer",
            Self::OutOfRange => "dpi must be in range 50..8200",
            Self::NotMultiple => "dpi must be a multiple of 50",
        })
    }
}

impl core::error::Error for ParseDpiError {}

impl Dpi {
    /// Validates an integer, distinguishing the reason for rejection.
    ///
    /// # Errors
    ///
    /// [`ParseDpiError::OutOfRange`] or [`ParseDpiError::NotMultiple`].
    pub fn try_from_i64(dpi: i64) -> Result<Self, ParseDpiError> {
        let dpi = u16::try_from(dpi).map_err(|_| ParseDpiError::OutOfRange)?;
        if !(Self::MIN..=Self::MAX).contains(&dpi) {
            return Err(ParseDpiError::OutOfRange);
        }
        Self::new(dpi).ok_or(ParseDpiError::NotMultiple)
    }
}

impl FromStr for Dpi {
    type Err = ParseDpiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let dpi = parse_u32(s).ok_or(ParseDpiError::Syntax)?;
        Self::try_from_i64(i64::from(dpi))
    }
}

/// Lift detection (blob offset 2). Experimental on C658 (spec config §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lift {
    /// Encoded as `0x1f`.
    Disabled,
    /// Encoded as the threshold. A threshold of `0x1f` encodes the same byte
    /// as [`Lift::Disabled`]; see [`Lift::is_ambiguous`].
    Enabled {
        /// Raw threshold, 0..=255.
        threshold: u8,
    },
}

impl Lift {
    /// Blob offset 2.
    #[must_use]
    pub const fn encoded(self) -> u8 {
        match self {
            Self::Disabled => LIFT_DISABLED,
            Self::Enabled { threshold } => threshold,
        }
    }

    /// Decodes blob offset 2. `0x1f` decodes as [`Lift::Disabled`].
    #[must_use]
    pub const fn from_encoded(byte: u8) -> Self {
        if byte == LIFT_DISABLED {
            Self::Disabled
        } else {
            Self::Enabled { threshold: byte }
        }
    }

    /// True when enabled with a threshold that encodes like disabled
    /// (`W-LIFT-AMBIGUOUS`).
    #[must_use]
    pub const fn is_ambiguous(self) -> bool {
        matches!(
            self,
            Self::Enabled {
                threshold: LIFT_DISABLED
            }
        )
    }
}

/// Wheel mode (blob offsets 3..6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelMode {
    /// `normal`: `01 ff 00 00`.
    Normal,
    /// `inertial`: `00 00 00 01`.
    Inertial,
}

impl WheelMode {
    /// Blob offsets 3..6.
    #[must_use]
    pub const fn encoded(self) -> [u8; 4] {
        match self {
            Self::Normal => [0x01, 0xff, 0x00, 0x00],
            Self::Inertial => [0x00, 0x00, 0x00, 0x01],
        }
    }

    /// Decodes blob offsets 3..6.
    #[must_use]
    pub fn from_encoded(bytes: [u8; 4]) -> Option<Self> {
        [Self::Normal, Self::Inertial]
            .into_iter()
            .find(|mode| mode.encoded() == bytes)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Inertial => "inertial",
        }
    }
}

impl fmt::Display for WheelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A wheel mode string other than `normal` or `inertial`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseWheelModeError;

impl fmt::Display for ParseWheelModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("wheel must be \"normal\" or \"inertial\"")
    }
}

impl core::error::Error for ParseWheelModeError {}

impl FromStr for WheelMode {
    type Err = ParseWheelModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [Self::Normal, Self::Inertial]
            .into_iter()
            .find(|mode| mode.as_str() == s)
            .ok_or(ParseWheelModeError)
    }
}

/// Polling rate (blob offset 30, as a divider of 1000 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PollingRate {
    /// 1000 Hz, divider 1.
    Hz1000,
    /// 500 Hz, divider 2.
    Hz500,
    /// 250 Hz, divider 4.
    Hz250,
    /// 125 Hz, divider 8.
    Hz125,
}

impl PollingRate {
    const ALL: [Self; 4] = [Self::Hz1000, Self::Hz500, Self::Hz250, Self::Hz125];

    /// Rate in Hz.
    #[must_use]
    pub const fn hz(self) -> u16 {
        match self {
            Self::Hz1000 => 1000,
            Self::Hz500 => 500,
            Self::Hz250 => 250,
            Self::Hz125 => 125,
        }
    }

    /// Blob offset 30.
    #[must_use]
    pub const fn divider(self) -> u8 {
        match self {
            Self::Hz1000 => 1,
            Self::Hz500 => 2,
            Self::Hz250 => 4,
            Self::Hz125 => 8,
        }
    }

    /// Looks up a rate in Hz.
    #[must_use]
    pub fn from_hz(hz: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|rate| i64::from(rate.hz()) == hz)
    }

    /// Decodes blob offset 30.
    #[must_use]
    pub fn from_divider(divider: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|rate| rate.divider() == divider)
    }
}

impl fmt::Display for PollingRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hz())
    }
}

/// A polling rate other than 125, 250, 500 or 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsePollingRateError;

impl fmt::Display for ParsePollingRateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("polling_rate must be 125, 250, 500 or 1000")
    }
}

impl core::error::Error for ParsePollingRateError {}

impl FromStr for PollingRate {
    type Err = ParsePollingRateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_u32(s)
            .and_then(|hz| Self::from_hz(i64::from(hz)))
            .ok_or(ParsePollingRateError)
    }
}

/// Physical button names for blob offsets 18..24 (spec config §6).
///
/// `OBSERVED`: on both routes, changing each offset to `host:1` made the
/// corresponding physical button report bitmap `0x01`, for all seven entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonName {
    /// Offset 18.
    Left,
    /// Offset 19.
    Right,
    /// Offset 20.
    Middle,
    /// Offset 21, wheel click.
    Wheel,
    /// Offset 22.
    Forward,
    /// Offset 23.
    Back,
    /// Offset 24.
    Radial,
}

impl ButtonName {
    /// All buttons in blob order.
    pub const ALL: [Self; 7] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Wheel,
        Self::Forward,
        Self::Back,
        Self::Radial,
    ];

    /// Index into the seven-entry table, 0..=6.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Blob offset, 18..=24.
    #[must_use]
    pub const fn blob_offset(self) -> usize {
        BUTTON_OFFSET + self.index()
    }

    /// TOML key under `[buttons]`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Wheel => "wheel",
            Self::Forward => "forward",
            Self::Back => "back",
            Self::Radial => "radial",
        }
    }
}

impl fmt::Display for ButtonName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string that is not a physical button name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseButtonNameError;

impl fmt::Display for ParseButtonNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("button must be one of left, right, middle, wheel, forward, back, radial")
    }
}

impl core::error::Error for ParseButtonNameError {}

impl FromStr for ButtonName {
    type Err = ParseButtonNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|name| name.as_str() == s)
            .ok_or(ParseButtonNameError)
    }
}

/// The seven button entries, indexed by [`ButtonName`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Buttons(pub [Action; 7]);

impl Buttons {
    /// Action of one button.
    #[must_use]
    pub const fn get(&self, name: ButtonName) -> Action {
        self.0[name.index()]
    }

    /// Replaces the action of one button.
    pub const fn set(&mut self, name: ButtonName, action: Action) {
        self.0[name.index()] = action;
    }

    /// Iterates over `(name, action)` in blob order.
    pub fn iter(&self) -> impl Iterator<Item = (ButtonName, Action)> + '_ {
        ButtonName::ALL.into_iter().zip(self.0)
    }
}

/// A complete, validated Report `0x10` configuration.
///
/// Every field is a validated type, so encoding cannot fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Report10Config {
    /// Blob offset 1.
    pub dpi: Dpi,
    /// Blob offset 2.
    pub lift: Lift,
    /// Blob offsets 3..6.
    pub wheel: WheelMode,
    /// Blob offsets 18..24.
    pub buttons: Buttons,
    /// Blob offset 30.
    pub polling_rate: PollingRate,
}

impl Report10Config {
    /// The research SDK's `latest_software_baseline()`.
    ///
    /// A static-analysis-derived test starting point: neither a value read
    /// from a device nor a factory default. It is used only for
    /// `init --preset=research-baseline` and must never fill missing values
    /// implicitly (P3).
    #[must_use]
    pub const fn research_baseline() -> Self {
        use DirectAction::{Backward, Forward, Left, Middle, Right};
        Self {
            dpi: Dpi(1400),
            lift: Lift::Disabled,
            wheel: WheelMode::Normal,
            buttons: Buttons([
                Action::Direct(Left),
                Action::Direct(Right),
                Action::Direct(Middle),
                Action::Direct(Middle),
                Action::Direct(Forward),
                Action::Direct(Backward),
                Action::Direct(Middle),
            ]),
            polling_rate: PollingRate::Hz1000,
        }
    }

    /// The complete 31-byte blob, with all reserved bytes zero.
    #[must_use]
    pub fn to_blob(&self) -> [u8; BLOB_LEN] {
        let mut blob = [0u8; BLOB_LEN];
        blob[1] = self.dpi.encoded();
        blob[2] = self.lift.encoded();
        blob[3..7].copy_from_slice(&self.wheel.encoded());
        for (name, action) in self.buttons.iter() {
            blob[name.blob_offset()] = action.wire();
        }
        blob[26] = FIXED_26;
        blob[30] = self.polling_rate.divider();
        blob
    }

    /// The 32-byte wire report: Report ID `0x10` followed by the blob.
    #[must_use]
    pub fn to_wire(&self) -> [u8; WIRE_LEN] {
        let mut wire = [0u8; WIRE_LEN];
        wire[0] = REPORT_ID;
        wire[1..].copy_from_slice(&self.to_blob());
        wire
    }
}

/// One decoded button entry of an inspected report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireButton {
    /// A value some action produces.
    Action(Action),
    /// `0x00..=0x09`: no action produces this value.
    Invalid(u8),
}

impl WireButton {
    /// Decodes one wire byte.
    #[must_use]
    pub const fn from_wire(wire: u8) -> Self {
        match Action::from_wire(wire) {
            Some(action) => Self::Action(action),
            None => Self::Invalid(wire),
        }
    }

    /// The raw wire byte.
    #[must_use]
    pub const fn wire(self) -> u8 {
        match self {
            Self::Action(action) => action.wire(),
            Self::Invalid(wire) => wire,
        }
    }
}

/// A decoded wire report. Produced from captures and test vectors; the
/// device itself cannot be read back (spec device §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InspectedReport10 {
    /// Raw blob offset 1.
    pub dpi_encoded: u8,
    /// Blob offset 1 times 50, without range checks.
    pub nominal_dpi: u16,
    /// Blob offset 2.
    pub lift: Lift,
    /// Blob offsets 3..6.
    pub wheel: WheelMode,
    /// Blob offsets 18..24 in [`ButtonName`] order.
    pub buttons: [WireButton; 7],
    /// Blob offset 30.
    pub polling_rate: PollingRate,
    /// Whether every reserved byte is zero.
    pub reserved_bytes_are_zero: bool,
    /// Whether blob offset 26 is `0x1e`.
    pub fixed_field_is_valid: bool,
}

impl InspectedReport10 {
    /// The configuration this report encodes, if it is exactly what
    /// [`Report10Config::to_wire`] produces for some configuration.
    #[must_use]
    pub fn to_config(&self) -> Option<Report10Config> {
        if !self.reserved_bytes_are_zero || !self.fixed_field_is_valid {
            return None;
        }
        let mut actions = [Action::Direct(DirectAction::Left); 7];
        for (slot, button) in actions.iter_mut().zip(self.buttons) {
            match button {
                WireButton::Action(action) => *slot = action,
                WireButton::Invalid(_) => return None,
            }
        }
        Some(Report10Config {
            dpi: Dpi::from_encoded(self.dpi_encoded)?,
            lift: self.lift,
            wheel: self.wheel,
            buttons: Buttons(actions),
            polling_rate: self.polling_rate,
        })
    }
}

/// Why a wire report could not be inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectError {
    /// Not exactly 32 bytes.
    Length(usize),
    /// The first byte is not `0x10`.
    ReportId(u8),
    /// Blob offsets 3..6 match no wheel mode.
    UnknownWheelMode([u8; 4]),
    /// Blob offset 30 matches no polling rate.
    UnknownPollingDivider(u8),
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(len) => write!(f, "Report 0x10 must be exactly 32 bytes, got {len}"),
            Self::ReportId(id) => write!(f, "expected Report ID 0x10, got 0x{id:02x}"),
            Self::UnknownWheelMode(bytes) => {
                f.write_str("unknown wheel mode bytes:")?;
                bytes.iter().try_for_each(|b| write!(f, " {b:02x}"))
            }
            Self::UnknownPollingDivider(divider) => {
                write!(f, "unknown polling divider: {divider}")
            }
        }
    }
}

impl core::error::Error for InspectError {}

/// Decodes a 32-byte wire report.
///
/// Follows the research SDK's `inspect_wire_report`: the wheel bytes and
/// polling divider must be known values; reserved bytes and offset 26 are
/// reported but not rejected.
///
/// # Errors
///
/// See [`InspectError`].
pub fn inspect(wire: &[u8]) -> Result<InspectedReport10, InspectError> {
    let wire: &[u8; WIRE_LEN] = wire
        .try_into()
        .map_err(|_| InspectError::Length(wire.len()))?;
    if wire[0] != REPORT_ID {
        return Err(InspectError::ReportId(wire[0]));
    }
    let blob = &wire[1..];
    let wheel_bytes = [blob[3], blob[4], blob[5], blob[6]];
    let wheel =
        WheelMode::from_encoded(wheel_bytes).ok_or(InspectError::UnknownWheelMode(wheel_bytes))?;
    let polling_rate =
        PollingRate::from_divider(blob[30]).ok_or(InspectError::UnknownPollingDivider(blob[30]))?;
    let mut buttons = [WireButton::Invalid(0); 7];
    for (button, &wire) in buttons
        .iter_mut()
        .zip(&blob[BUTTON_OFFSET..BUTTON_OFFSET + 7])
    {
        *button = WireButton::from_wire(wire);
    }
    Ok(InspectedReport10 {
        dpi_encoded: blob[1],
        nominal_dpi: u16::from(blob[1]) * Dpi::STEP,
        lift: Lift::from_encoded(blob[2]),
        wheel,
        buttons,
        polling_rate,
        reserved_bytes_are_zero: RESERVED.iter().all(|&i| blob[i] == 0),
        fixed_field_is_valid: blob[26] == FIXED_26,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HostIndex;

    fn hex(bytes: &[u8]) -> String {
        use std::fmt::Write;
        bytes.iter().fold(String::new(), |mut s, b| {
            write!(s, "{b:02x}").unwrap();
            s
        })
    }

    /// The research baseline wire report, byte by byte from spec config §3.
    const BASELINE_WIRE: &str = concat!(
        "10",                     // Report ID
        "00",                     // 0
        "1c",                     // 1: 1400 / 50
        "1f",                     // 2: lift disabled
        "01ff0000",               // 3..6: normal
        "0000000000000000000000", // 7..17
        "0a0b0c0c0e0d0c",         // 18..24
        "00",                     // 25
        "1e",                     // 26
        "000000",                 // 27..29
        "01",                     // 30: 1000 Hz
    );

    #[test]
    fn baseline_wire() {
        let wire = Report10Config::research_baseline().to_wire();
        assert_eq!(hex(&wire), BASELINE_WIRE);
        assert_eq!(&wire[1..], &Report10Config::research_baseline().to_blob());
    }

    #[test]
    fn dpi_bounds() {
        assert_eq!(Dpi::new(50).map(Dpi::encoded), Some(1));
        assert_eq!(Dpi::new(8200).map(Dpi::encoded), Some(164));
        assert_eq!(Dpi::new(0), None);
        assert_eq!(Dpi::new(49), None);
        assert_eq!(Dpi::new(8250), None);
        assert_eq!(Dpi::new(1425), None);
        assert_eq!(Dpi::from_encoded(0), None);
        assert_eq!(Dpi::from_encoded(164).map(Dpi::get), Some(8200));
        assert_eq!(Dpi::from_encoded(165), None);
    }

    #[test]
    fn dpi_rejects_instead_of_clamping() {
        assert_eq!(Dpi::try_from_i64(0), Err(ParseDpiError::OutOfRange));
        assert_eq!(Dpi::try_from_i64(-50), Err(ParseDpiError::OutOfRange));
        assert_eq!(Dpi::try_from_i64(8250), Err(ParseDpiError::OutOfRange));
        assert_eq!(Dpi::try_from_i64(100_000), Err(ParseDpiError::OutOfRange));
        assert_eq!(Dpi::try_from_i64(1601), Err(ParseDpiError::NotMultiple));
        assert_eq!(Dpi::try_from_i64(75), Err(ParseDpiError::NotMultiple));
        assert_eq!("1600".parse::<Dpi>().map(Dpi::get), Ok(1600));
        assert_eq!("0x640".parse::<Dpi>().map(Dpi::get), Ok(1600));
        assert_eq!("1600.0".parse::<Dpi>(), Err(ParseDpiError::Syntax));
        assert_eq!("1650".parse::<Dpi>().unwrap().to_string(), "1650");
    }

    #[test]
    fn every_dpi_round_trips() {
        for dpi in (50..=8200).step_by(50) {
            let dpi = Dpi::new(dpi).unwrap();
            assert_eq!(Dpi::from_encoded(dpi.encoded()), Some(dpi));
        }
    }

    #[test]
    fn lift() {
        assert_eq!(Lift::Disabled.encoded(), 0x1f);
        assert_eq!(Lift::Enabled { threshold: 0 }.encoded(), 0);
        assert_eq!(Lift::Enabled { threshold: 255 }.encoded(), 255);
        assert!(Lift::Enabled { threshold: 0x1f }.is_ambiguous());
        assert!(!Lift::Enabled { threshold: 0x20 }.is_ambiguous());
        assert!(!Lift::Disabled.is_ambiguous());
        assert_eq!(Lift::from_encoded(0x1f), Lift::Disabled);
        assert_eq!(Lift::from_encoded(0x20), Lift::Enabled { threshold: 0x20 });
    }

    #[test]
    fn wheel_and_polling_strings() {
        assert_eq!("normal".parse(), Ok(WheelMode::Normal));
        assert_eq!("inertial".parse(), Ok(WheelMode::Inertial));
        assert_eq!("Normal".parse::<WheelMode>(), Err(ParseWheelModeError));
        assert_eq!(WheelMode::Inertial.to_string(), "inertial");
        for (text, rate, divider) in [
            ("1000", PollingRate::Hz1000, 1),
            ("500", PollingRate::Hz500, 2),
            ("250", PollingRate::Hz250, 4),
            ("125", PollingRate::Hz125, 8),
        ] {
            assert_eq!(text.parse(), Ok(rate));
            assert_eq!(rate.to_string(), text);
            assert_eq!(rate.divider(), divider);
            assert_eq!(PollingRate::from_divider(divider), Some(rate));
        }
        assert_eq!("100".parse::<PollingRate>(), Err(ParsePollingRateError));
        assert_eq!(PollingRate::from_divider(3), None);
    }

    #[test]
    fn button_names_and_offsets() {
        let offsets: Vec<usize> = ButtonName::ALL.iter().map(|b| b.blob_offset()).collect();
        assert_eq!(offsets, [18, 19, 20, 21, 22, 23, 24]);
        for name in ButtonName::ALL {
            assert_eq!(name.to_string().parse(), Ok(name));
        }
        assert_eq!("backward".parse::<ButtonName>(), Err(ParseButtonNameError));
    }

    #[test]
    fn each_button_lands_on_its_offset() {
        let host1 = Action::HostRouted(HostIndex::new(1).unwrap());
        for name in ButtonName::ALL {
            let mut config = Report10Config::research_baseline();
            config.buttons.set(name, host1);
            let blob = config.to_blob();
            let baseline = Report10Config::research_baseline().to_blob();
            for offset in 0..BLOB_LEN {
                if offset == name.blob_offset() {
                    assert_eq!(blob[offset], 0x29);
                } else {
                    assert_eq!(blob[offset], baseline[offset], "{name} offset {offset}");
                }
            }
        }
    }

    #[test]
    fn inspect_round_trips_config() {
        let mut config = Report10Config::research_baseline();
        config.dpi = Dpi::new(8200).unwrap();
        config.lift = Lift::Enabled { threshold: 0 };
        config.wheel = WheelMode::Inertial;
        config.polling_rate = PollingRate::Hz125;
        config.buttons.set(
            ButtonName::Radial,
            Action::HostRouted(HostIndex::new(215).unwrap()),
        );
        config
            .buttons
            .set(ButtonName::Back, "raw:0x10".parse().unwrap());
        config
            .buttons
            .set(ButtonName::Wheel, "unknown:6".parse().unwrap());
        let inspected = inspect(&config.to_wire()).unwrap();
        assert!(inspected.reserved_bytes_are_zero);
        assert!(inspected.fixed_field_is_valid);
        assert_eq!(inspected.nominal_dpi, 8200);
        assert_eq!(inspected.to_config(), Some(config));
    }

    #[test]
    fn inspect_reports_reserved_and_fixed_bytes() {
        let wire = Report10Config::research_baseline().to_wire();
        for offset in RESERVED {
            let mut bad = wire;
            bad[1 + offset] = 0x01;
            let inspected = inspect(&bad).unwrap();
            assert!(!inspected.reserved_bytes_are_zero, "offset {offset}");
            assert!(inspected.fixed_field_is_valid);
            assert_eq!(inspected.to_config(), None);
        }
        let mut bad = wire;
        bad[1 + 26] = 0x00;
        let inspected = inspect(&bad).unwrap();
        assert!(inspected.reserved_bytes_are_zero);
        assert!(!inspected.fixed_field_is_valid);
        assert_eq!(inspected.to_config(), None);
    }

    #[test]
    fn inspect_keeps_undecodable_fields_visible() {
        let mut wire = Report10Config::research_baseline().to_wire();
        wire[1 + 1] = 0; // dpi 0
        wire[1 + 18] = 0x09; // no action
        let inspected = inspect(&wire).unwrap();
        assert_eq!(inspected.nominal_dpi, 0);
        assert_eq!(inspected.buttons[0], WireButton::Invalid(0x09));
        assert_eq!(inspected.buttons[0].wire(), 0x09);
        assert_eq!(inspected.to_config(), None);
    }

    #[test]
    fn inspect_errors() {
        let wire = Report10Config::research_baseline().to_wire();
        assert_eq!(inspect(&wire[..31]), Err(InspectError::Length(31)));
        assert_eq!(inspect(&[0u8; 33]), Err(InspectError::Length(33)));
        let mut bad = wire;
        bad[0] = 0x11;
        assert_eq!(inspect(&bad), Err(InspectError::ReportId(0x11)));
        let mut bad = wire;
        bad[1 + 6] = 0x01;
        assert_eq!(
            inspect(&bad),
            Err(InspectError::UnknownWheelMode([0x01, 0xff, 0x00, 0x01]))
        );
        assert!(
            inspect(&bad)
                .unwrap_err()
                .to_string()
                .starts_with("unknown wheel mode")
        );
        let mut bad = wire;
        bad[1 + 30] = 3;
        assert_eq!(inspect(&bad), Err(InspectError::UnknownPollingDivider(3)));
    }
}
