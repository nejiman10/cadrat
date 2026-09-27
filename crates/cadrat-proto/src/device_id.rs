//! Device ID: bytes 2..7 of the Feature `0x08` response (spec 02 §2.1, §4).
//!
//! `OBSERVED`: the same six bytes came back from the wired node, the Receiver
//! setting node and the occupied Receiver slot of one mouse. Their formal
//! meaning and uniqueness across devices are unverified (Q17), so the value is
//! used only for equality.
//!
//! The value identifies a physical device. It is shown locally, but must not
//! appear in committed files, fixtures or public logs; [`DeviceIdError`] never
//! includes it.

use core::fmt;
use core::str::FromStr;

/// Report ID of the ID probe.
pub const REPORT_ID: u8 = 0x08;
/// Wire length of the ID probe response.
pub const RESPONSE_LEN: usize = 8;
/// Byte 1 of a matching probe response.
///
/// Also observed as byte 1 of occupied slot reports and presumed to indicate
/// C658, but its meaning is not established; it is used only for matching.
pub const C658_TYPE: u8 = 0x59;

/// Six-byte device ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub [u8; 6]);

impl DeviceId {
    /// Parses an ID probe response (the bytes the GET returned).
    ///
    /// Success requires exactly 8 bytes, byte 0 `0x08` and byte 1 `0x59`.
    ///
    /// # Errors
    ///
    /// See [`DeviceIdError`].
    pub fn from_probe(response: &[u8]) -> Result<Self, DeviceIdError> {
        let response: &[u8; RESPONSE_LEN] = response
            .try_into()
            .map_err(|_| DeviceIdError::Length(response.len()))?;
        if response[0] != REPORT_ID || response[1] != C658_TYPE {
            return Err(DeviceIdError::Mismatch {
                report_id: response[0],
                device_type: response[1],
            });
        }
        let mut id = [0u8; 6];
        id.copy_from_slice(&response[2..]);
        Ok(Self(id))
    }
}

impl fmt::Display for DeviceId {
    /// Twelve lowercase hex digits, as used in the mouse key `c658:<id>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({self})")
    }
}

/// A string that is not twelve lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseDeviceIdError;

impl fmt::Display for ParseDeviceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("device ID must be 12 lowercase hex digits")
    }
}

impl core::error::Error for ParseDeviceIdError {}

impl FromStr for DeviceId {
    type Err = ParseDeviceIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s.as_bytes();
        if digits.len() != 12
            || !digits
                .iter()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(ParseDeviceIdError);
        }
        let mut id = [0u8; 6];
        for (byte, pair) in id.iter_mut().zip(digits.chunks_exact(2)) {
            // Validated above, so the pair is ASCII hex.
            let pair = core::str::from_utf8(pair).map_err(|_| ParseDeviceIdError)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| ParseDeviceIdError)?;
        }
        Ok(Self(id))
    }
}

/// Why an ID probe response did not match.
///
/// Carries only bytes 0 and 1, never the ID bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceIdError {
    /// The response was not 8 bytes long.
    Length(usize),
    /// Byte 0 is not `0x08` or byte 1 is not `0x59`.
    Mismatch {
        /// Byte 0.
        report_id: u8,
        /// Byte 1.
        device_type: u8,
    },
}

impl fmt::Display for DeviceIdError {
    /// The rejection reason shown by `list --nodes` (spec 02 §4).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(len) => write!(f, "probe-length: {len}"),
            Self::Mismatch {
                report_id,
                device_type,
            } => write!(f, "probe-mismatch: {report_id:02x} {device_type:02x} …"),
        }
    }
}

impl core::error::Error for DeviceIdError {}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic ID; not taken from any device.
    const ID: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];

    fn response(id0: u8, id1: u8) -> [u8; 8] {
        let mut r = [id0, id1, 0, 0, 0, 0, 0, 0];
        r[2..].copy_from_slice(&ID);
        r
    }

    #[test]
    fn matching_probe() {
        let id = DeviceId::from_probe(&response(0x08, 0x59)).unwrap();
        assert_eq!(id, DeviceId(ID));
        assert_eq!(id.to_string(), "0a1b2c3d4e5f");
        assert_eq!("0a1b2c3d4e5f".parse(), Ok(id));
    }

    #[test]
    fn mismatches_hide_the_id() {
        let err = DeviceId::from_probe(&response(0x08, 0x00)).unwrap_err();
        assert_eq!(
            err,
            DeviceIdError::Mismatch {
                report_id: 0x08,
                device_type: 0x00
            }
        );
        assert_eq!(err.to_string(), "probe-mismatch: 08 00 …");
        assert!(DeviceId::from_probe(&response(0x09, 0x59)).is_err());
    }

    #[test]
    fn wrong_length() {
        let full = response(0x08, 0x59);
        assert_eq!(
            DeviceId::from_probe(&full[..2]),
            Err(DeviceIdError::Length(2))
        );
        assert_eq!(DeviceId::from_probe(&[]), Err(DeviceIdError::Length(0)));
        assert_eq!(DeviceId::from_probe(&[0; 9]), Err(DeviceIdError::Length(9)));
    }

    #[test]
    fn parse_rejects_other_forms() {
        for s in [
            "",
            "0a1b2c3d4e5",
            "0a1b2c3d4e5f0",
            "0A1B2C3D4E5F",
            "0a1b2c3d4e5g",
            "c658:0a1b2c3d4e5f",
        ] {
            assert_eq!(s.parse::<DeviceId>(), Err(ParseDeviceIdError), "{s:?}");
        }
    }
}
