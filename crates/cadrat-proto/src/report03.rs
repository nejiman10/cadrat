//! Input Report `0x03`: host-routed button bitmap.
//!
//! `CONFIRMED`: the static descriptor defines masks `0x01..=0x40` and the
//! parser treats the payload as a little-endian bitmap. `OBSERVED`: host
//! indices 1..=7 produce bits 0..=6 on both routes. Follows the research
//! SDK's `parse_report03`.

use core::fmt;

/// Report ID of the host-routed button report.
pub const REPORT_ID: u8 = 0x03;
/// Bits that carry host indices 1..=7.
pub const MASK: u8 = 0x7f;

/// One decoded Report `0x03` frame and its transition from the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Report03Frame {
    /// Current bitmap, masked to bits 0..=6.
    pub bitmap: u8,
    /// Previous bitmap, masked to bits 0..=6.
    pub previous: u8,
    /// Bits that changed from 0 to 1.
    pub pressed: u8,
    /// Bits that changed from 1 to 0.
    pub released: u8,
}

impl Report03Frame {
    /// Parses a raw input packet given the previous bitmap.
    ///
    /// Up to four payload bytes are read as a little-endian integer and
    /// masked to bits 0..=6, as the research SDK does.
    ///
    /// # Errors
    ///
    /// See [`Report03Error`].
    pub fn parse(packet: &[u8], previous: u8) -> Result<Self, Report03Error> {
        match packet {
            [] | [_] => Err(Report03Error::TooShort(packet.len())),
            [REPORT_ID, payload @ ..] => {
                // Only the low byte matters after masking to 0x7f.
                let bitmap = payload[0] & MASK;
                let previous = previous & MASK;
                let changed = bitmap ^ previous;
                Ok(Self {
                    bitmap,
                    previous,
                    pressed: changed & bitmap,
                    released: changed & previous,
                })
            }
            [id, ..] => Err(Report03Error::ReportId(*id)),
        }
    }
}

/// Why a packet is not a Report `0x03` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report03Error {
    /// Fewer than two bytes (Report ID and bitmap).
    TooShort(usize),
    /// The first byte is not `0x03`.
    ReportId(u8),
}

impl fmt::Display for Report03Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(len) => {
                write!(f, "Report 0x03 needs an ID and a bitmap, got {len} bytes")
            }
            Self::ReportId(id) => write!(f, "expected Report ID 0x03, got 0x{id:02x}"),
        }
    }
}

impl core::error::Error for Report03Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_and_release() {
        let press = Report03Frame::parse(&[0x03, 0x40], 0).unwrap();
        assert_eq!(
            press,
            Report03Frame {
                bitmap: 0x40,
                previous: 0,
                pressed: 0x40,
                released: 0
            }
        );
        let release = Report03Frame::parse(&[0x03, 0x00], press.bitmap).unwrap();
        assert_eq!(release.pressed, 0);
        assert_eq!(release.released, 0x40);
    }

    #[test]
    fn overlapping_transitions() {
        let frame = Report03Frame::parse(&[0x03, 0b0000_0110], 0b0000_0011).unwrap();
        assert_eq!(frame.pressed, 0b0000_0100);
        assert_eq!(frame.released, 0b0000_0001);
    }

    #[test]
    fn masks_high_bits_and_extra_bytes() {
        let frame = Report03Frame::parse(&[0x03, 0xff, 0xff, 0xff, 0xff, 0xff], 0x80).unwrap();
        assert_eq!(frame.bitmap, 0x7f);
        assert_eq!(frame.previous, 0);
        assert_eq!(frame.pressed, 0x7f);
        assert_eq!(frame.released, 0);
    }

    #[test]
    fn errors() {
        assert_eq!(
            Report03Frame::parse(&[], 0),
            Err(Report03Error::TooShort(0))
        );
        assert_eq!(
            Report03Frame::parse(&[0x03], 0),
            Err(Report03Error::TooShort(1))
        );
        assert_eq!(
            Report03Frame::parse(&[0x17, 0x64], 0),
            Err(Report03Error::ReportId(0x17))
        );
    }
}
