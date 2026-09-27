//! C652 Receiver management: pairing packets and slot reports (spec 05).
//!
//! `OBSERVED` (`evidence/receiver-repair-2026-09`): one cycle of unpairing
//! slot 2 and pairing into slot 3 with these packets.

use core::fmt;

use crate::device_id::DeviceId;

/// Report ID of the pairing control Feature report.
pub const PAIRING_REPORT_ID: u8 = 0x41;
/// Declared wire length of Feature `0x41`.
pub const PAIRING_REPORT_LEN: usize = 5;
/// Start pairing: `41 02 02 00 00`.
pub const PAIR_START: [u8; PAIRING_REPORT_LEN] = [PAIRING_REPORT_ID, 0x02, 0x02, 0x00, 0x00];
/// Stop pairing: `41 02 00 00 00`.
pub const PAIR_STOP: [u8; PAIRING_REPORT_LEN] = [PAIRING_REPORT_ID, 0x02, 0x00, 0x00, 0x00];
const UNPAIR_SUBCOMMAND: u8 = 0x04;

/// Report ID of the slot 0 report; slot N uses `0x43 + N`.
pub const SLOT_REPORT_BASE: u8 = 0x43;
/// Declared wire length of the slot reports.
pub const SLOT_REPORT_LEN: usize = 8;
/// Number of Receiver slots.
pub const SLOT_COUNT: u8 = 5;

/// Receiver slot, 0..=4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(u8);

impl Slot {
    /// All slots in order.
    pub const ALL: [Self; SLOT_COUNT as usize] = [Self(0), Self(1), Self(2), Self(3), Self(4)];

    /// Returns the slot if it is in 0..=4.
    #[must_use]
    pub const fn new(slot: u8) -> Option<Self> {
        if slot < SLOT_COUNT {
            Some(Self(slot))
        } else {
            None
        }
    }

    /// The slot number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Report ID of this slot's report, `0x43 + slot`.
    #[must_use]
    pub const fn report_id(self) -> u8 {
        SLOT_REPORT_BASE + self.0
    }

    /// Unpair packet for this slot: `41 04 <slot> 00 00`.
    #[must_use]
    pub const fn unpair_packet(self) -> [u8; PAIRING_REPORT_LEN] {
        [PAIRING_REPORT_ID, UNPAIR_SUBCOMMAND, self.0, 0x00, 0x00]
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A parsed slot report.
///
/// Equality compares the full raw response, which is what the unpair
/// procedure checks between confirmation and execution (spec 05 §4 step 4).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotReport {
    slot: Slot,
    raw: [u8; SLOT_REPORT_LEN],
}

impl SlotReport {
    /// Parses the response to GET Feature `0x43 + slot`.
    ///
    /// # Errors
    ///
    /// [`SlotError`] unless the response is 8 bytes starting with the slot's
    /// Report ID (`ReceiverProtocolError`, spec 05 §2).
    pub fn parse(slot: Slot, response: &[u8]) -> Result<Self, SlotError> {
        let raw: [u8; SLOT_REPORT_LEN] = response.try_into().map_err(|_| SlotError::Length {
            slot,
            len: response.len(),
        })?;
        if raw[0] != slot.report_id() {
            return Err(SlotError::ReportId { slot, got: raw[0] });
        }
        Ok(Self { slot, raw })
    }

    /// The slot this report describes.
    #[must_use]
    pub const fn slot(&self) -> Slot {
        self.slot
    }

    /// Byte 1, a device-type candidate (`HYPOTHESIS`). `0x59` was observed
    /// for a paired C658.
    #[must_use]
    pub const fn device_type(&self) -> u8 {
        self.raw[1]
    }

    /// Whether the slot is occupied: `byte1 != 0`.
    ///
    /// `HYPOTHESIS` (Q11): `0x00` was observed for empty slots and `0x59` for
    /// occupied ones; that every non-zero value means occupied is untested.
    #[must_use]
    pub const fn occupied(&self) -> bool {
        self.raw[1] != 0
    }

    /// Bytes 2..7, a per-device identifier candidate (`HYPOTHESIS`). Compared
    /// with the device ID of Receiver setting nodes (spec 02 §5).
    #[must_use]
    pub fn id_candidate(&self) -> DeviceId {
        let mut id = [0u8; 6];
        id.copy_from_slice(&self.raw[2..]);
        DeviceId(id)
    }

    /// The raw response.
    #[must_use]
    pub const fn raw(&self) -> &[u8; SLOT_REPORT_LEN] {
        &self.raw
    }
}

impl fmt::Debug for SlotReport {
    // The identifier candidate is shown through DeviceId's Debug, never as
    // part of an error.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlotReport")
            .field("slot", &self.slot.get())
            .field("device_type", &self.device_type())
            .field("id_candidate", &self.id_candidate())
            .finish_non_exhaustive()
    }
}

/// Slots that are occupied in `after` but were empty in `before`
/// (spec 05 §3 step 5).
///
/// Reports are matched by slot number; slots missing from `before` are
/// ignored.
pub fn newly_occupied<'a>(
    before: &'a [SlotReport],
    after: &'a [SlotReport],
) -> impl Iterator<Item = Slot> + 'a {
    after.iter().filter_map(move |now| {
        let was_empty = before
            .iter()
            .any(|then| then.slot == now.slot && !then.occupied());
        (was_empty && now.occupied()).then_some(now.slot)
    })
}

/// Why a slot response was rejected.
///
/// Carries only the length or Report ID, never the identifier bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotError {
    /// The response was not 8 bytes long.
    Length {
        /// Requested slot.
        slot: Slot,
        /// Response length.
        len: usize,
    },
    /// Byte 0 is not `0x43 + slot`.
    ReportId {
        /// Requested slot.
        slot: Slot,
        /// Byte 0 of the response.
        got: u8,
    },
}

impl fmt::Display for SlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { slot, len } => {
                write!(
                    f,
                    "slot {slot} response has {len} bytes, expected {SLOT_REPORT_LEN}"
                )
            }
            Self::ReportId { slot, got } => write!(
                f,
                "slot {slot} response starts with 0x{got:02x}, expected 0x{:02x}",
                slot.report_id()
            ),
        }
    }
}

impl core::error::Error for SlotError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(n: u8) -> Slot {
        Slot::new(n).unwrap()
    }

    // Synthetic identifier; not taken from any device.
    fn occupied(n: u8) -> SlotReport {
        SlotReport::parse(
            slot(n),
            &[0x43 + n, 0x59, 0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f],
        )
        .unwrap()
    }

    fn empty(n: u8) -> SlotReport {
        SlotReport::parse(slot(n), &[0x43 + n, 0, 0, 0, 0, 0, 0, 0]).unwrap()
    }

    #[test]
    fn packets() {
        assert_eq!(PAIR_START, [0x41, 0x02, 0x02, 0x00, 0x00]);
        assert_eq!(PAIR_STOP, [0x41, 0x02, 0x00, 0x00, 0x00]);
        assert_eq!(slot(0).unpair_packet(), [0x41, 0x04, 0x00, 0x00, 0x00]);
        assert_eq!(slot(2).unpair_packet(), [0x41, 0x04, 0x02, 0x00, 0x00]);
        assert_eq!(slot(4).unpair_packet(), [0x41, 0x04, 0x04, 0x00, 0x00]);
    }

    #[test]
    fn slot_bounds() {
        let ids: Vec<u8> = Slot::ALL.iter().map(|s| s.report_id()).collect();
        assert_eq!(ids, [0x43, 0x44, 0x45, 0x46, 0x47]);
        assert_eq!(Slot::new(5), None);
    }

    #[test]
    fn parse_slots() {
        let report = empty(2);
        assert!(!report.occupied());
        assert_eq!(report.device_type(), 0);

        let report = occupied(3);
        assert!(report.occupied());
        assert_eq!(report.device_type(), 0x59);
        assert_eq!(report.id_candidate().to_string(), "0a1b2c3d4e5f");
        assert_eq!(report.slot(), slot(3));
    }

    #[test]
    fn any_nonzero_type_counts_as_occupied() {
        let report = SlotReport::parse(slot(0), &[0x43, 0x01, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(report.occupied());
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            SlotReport::parse(slot(1), &[0x44, 0x00]),
            Err(SlotError::Length {
                slot: slot(1),
                len: 2
            })
        );
        assert_eq!(
            SlotReport::parse(slot(1), &[0x43, 0, 0, 0, 0, 0, 0, 0]),
            Err(SlotError::ReportId {
                slot: slot(1),
                got: 0x43
            })
        );
    }

    #[test]
    fn raw_equality_detects_any_change() {
        let a = occupied(2);
        let mut raw = *a.raw();
        raw[7] ^= 1;
        let b = SlotReport::parse(slot(2), &raw).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, occupied(2));
    }

    #[test]
    fn newly_occupied_slots() {
        let before = [empty(0), empty(1), occupied(2), empty(3), empty(4)];
        let after = [empty(0), empty(1), occupied(2), occupied(3), occupied(4)];
        let new: Vec<Slot> = newly_occupied(&before, &after).collect();
        assert_eq!(new, [slot(3), slot(4)]);
        assert_eq!(newly_occupied(&before, &before).count(), 0);
        // A slot that was occupied and still is does not count, even if its
        // identifier changed.
        let after = [empty(0), empty(1), occupied(2), empty(3), empty(4)];
        assert_eq!(newly_occupied(&before, &after).count(), 0);
    }
}
