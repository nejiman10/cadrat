//! HID report descriptor parsing for report-length discovery.
//!
//! Follows the research SDK's `hid_descriptor.py` (spec device §3): global
//! PUSH/POP, Report Size, Report ID and Report Count are tracked, long items
//! are skipped, and multiple main items for the same Report ID are summed.
//! The wire length is the payload rounded up to bytes, plus one byte for the
//! Report ID when it is non-zero.

use core::fmt;

/// Maximum PUSH depth. Real descriptors nest a few levels at most; the limit
/// keeps the parser allocation-free.
pub const MAX_PUSH_DEPTH: usize = 16;

const TYPE_MAIN: u8 = 0;
const TYPE_GLOBAL: u8 = 1;
const MAIN_INPUT: u8 = 8;
const MAIN_FEATURE: u8 = 11;
const GLOBAL_REPORT_SIZE: u8 = 7;
const GLOBAL_REPORT_ID: u8 = 8;
const GLOBAL_REPORT_COUNT: u8 = 9;
const GLOBAL_PUSH: u8 = 10;
const GLOBAL_POP: u8 = 11;
const LONG_ITEM: u8 = 0xfe;

/// Why a descriptor could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorError {
    /// A long item header runs past the end.
    TruncatedLongItemHeader,
    /// A long item's data runs past the end.
    TruncatedLongItem,
    /// A short item's data runs past the end.
    TruncatedShortItem,
    /// Report ID 0 is reserved.
    ReportIdZero,
    /// Report ID above 255.
    ReportIdTooLarge(u32),
    /// POP without a matching PUSH.
    PopWithoutPush,
    /// More than [`MAX_PUSH_DEPTH`] nested PUSH items.
    PushTooDeep,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedLongItemHeader => f.write_str("truncated HID long-item header"),
            Self::TruncatedLongItem => f.write_str("truncated HID long item"),
            Self::TruncatedShortItem => f.write_str("truncated HID short item"),
            Self::ReportIdZero => f.write_str("Report ID zero is invalid"),
            Self::ReportIdTooLarge(id) => write!(f, "Report ID {id} does not fit in one byte"),
            Self::PopWithoutPush => f.write_str("global POP without PUSH"),
            Self::PushTooDeep => write!(f, "global PUSH nested deeper than {MAX_PUSH_DEPTH}"),
        }
    }
}

impl core::error::Error for DescriptorError {}

#[derive(Debug, Clone, Copy, Default)]
struct Globals {
    size: u32,
    count: u32,
    id: u8,
}

/// Per-Report-ID total bits. Index is the Report ID; 0 means "no ID".
#[derive(Clone, PartialEq, Eq)]
struct BitTable([Option<u64>; 256]);

impl BitTable {
    const EMPTY: Self = Self([None; 256]);

    fn add(&mut self, id: u8, bits: u64) {
        let entry = &mut self.0[usize::from(id)];
        *entry = Some(entry.unwrap_or(0).saturating_add(bits));
    }

    fn wire_len(&self, id: u8) -> Option<u64> {
        self.0[usize::from(id)].map(|bits| bits.div_ceil(8) + u64::from(id != 0))
    }

    fn iter(&self) -> impl Iterator<Item = (u8, u64)> + '_ {
        (0..=u8::MAX).filter_map(|id| self.wire_len(id).map(|len| (id, len)))
    }
}

/// Input and Feature report wire lengths, keyed by Report ID.
///
/// Report ID 0 stands for reports declared before any Report ID item.
#[derive(Clone, PartialEq, Eq)]
pub struct ReportLengths {
    input: BitTable,
    feature: BitTable,
}

impl ReportLengths {
    /// Parses a report descriptor.
    ///
    /// # Errors
    ///
    /// See [`DescriptorError`].
    pub fn parse(descriptor: &[u8]) -> Result<Self, DescriptorError> {
        let mut lengths = Self {
            input: BitTable::EMPTY,
            feature: BitTable::EMPTY,
        };
        let mut current = Globals::default();
        let mut stack = [Globals::default(); MAX_PUSH_DEPTH];
        let mut depth = 0;
        let mut rest = descriptor;

        while let Some((&prefix, tail)) = rest.split_first() {
            rest = tail;
            if prefix == LONG_ITEM {
                let [size, _tag, tail @ ..] = rest else {
                    return Err(DescriptorError::TruncatedLongItemHeader);
                };
                rest = tail
                    .get(usize::from(*size)..)
                    .ok_or(DescriptorError::TruncatedLongItem)?;
                continue;
            }
            let size = match prefix & 0x03 {
                3 => 4,
                n => usize::from(n),
            };
            if rest.len() < size {
                return Err(DescriptorError::TruncatedShortItem);
            }
            let (data, tail) = rest.split_at(size);
            rest = tail;
            let value = data
                .iter()
                .rev()
                .fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
            let item_type = (prefix >> 2) & 0x03;
            let tag = prefix >> 4;

            match (item_type, tag) {
                (TYPE_GLOBAL, GLOBAL_REPORT_SIZE) => current.size = value,
                (TYPE_GLOBAL, GLOBAL_REPORT_ID) => {
                    current.id = match u8::try_from(value) {
                        Ok(0) => return Err(DescriptorError::ReportIdZero),
                        Ok(id) => id,
                        Err(_) => return Err(DescriptorError::ReportIdTooLarge(value)),
                    };
                }
                (TYPE_GLOBAL, GLOBAL_REPORT_COUNT) => current.count = value,
                (TYPE_GLOBAL, GLOBAL_PUSH) => {
                    *stack.get_mut(depth).ok_or(DescriptorError::PushTooDeep)? = current;
                    depth += 1;
                }
                (TYPE_GLOBAL, GLOBAL_POP) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or(DescriptorError::PopWithoutPush)?;
                    current = stack[depth];
                }
                (TYPE_MAIN, MAIN_INPUT | MAIN_FEATURE) => {
                    let bits = u64::from(current.size) * u64::from(current.count);
                    let table = if tag == MAIN_INPUT {
                        &mut lengths.input
                    } else {
                        &mut lengths.feature
                    };
                    table.add(current.id, bits);
                }
                _ => {}
            }
        }
        Ok(lengths)
    }

    /// Wire length of a Feature report, if declared.
    #[must_use]
    pub fn feature(&self, report_id: u8) -> Option<u64> {
        self.feature.wire_len(report_id)
    }

    /// Wire length of an Input report, if declared.
    #[must_use]
    pub fn input(&self, report_id: u8) -> Option<u64> {
        self.input.wire_len(report_id)
    }

    /// All declared Feature reports as `(report_id, wire_len)`, by ID.
    pub fn features(&self) -> impl Iterator<Item = (u8, u64)> + '_ {
        self.feature.iter()
    }

    /// All declared Input reports as `(report_id, wire_len)`, by ID.
    pub fn inputs(&self) -> impl Iterator<Item = (u8, u64)> + '_ {
        self.input.iter()
    }
}

impl fmt::Debug for ReportLengths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Map<'a>(&'a BitTable);
        impl fmt::Debug for Map<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_map().entries(self.0.iter()).finish()
            }
        }
        f.debug_struct("ReportLengths")
            .field("input", &Map(&self.input))
            .field("feature", &Map(&self.feature))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features(lengths: &ReportLengths) -> Vec<(u8, u64)> {
        lengths.features().collect()
    }

    fn inputs(lengths: &ReportLengths) -> Vec<(u8, u64)> {
        lengths.inputs().collect()
    }

    /// A synthetic vendor collection shaped like the reports cadrat uses:
    /// Feature 0x10 (31 bytes), Feature 0x08 (7 bytes), Input 0x03 (1 byte).
    const SYNTHETIC: &[u8] = &[
        0x06, 0x00, 0xff, // Usage Page (vendor)
        0x09, 0x01, // Usage
        0xa1, 0x01, // Collection (Application)
        0x75, 0x08, // Report Size 8
        0x85, 0x10, // Report ID 0x10
        0x95, 0x1f, // Report Count 31
        0x09, 0x01, // Usage
        0xb1, 0x02, // Feature
        0x85, 0x08, // Report ID 0x08
        0x95, 0x07, // Report Count 7
        0x09, 0x02, // Usage
        0xb1, 0x02, // Feature
        0x85, 0x03, // Report ID 0x03
        0x95, 0x01, // Report Count 1
        0x09, 0x03, // Usage
        0x81, 0x02, // Input
        0xc0, // End Collection
    ];

    #[test]
    fn synthetic_lengths() {
        let lengths = ReportLengths::parse(SYNTHETIC).unwrap();
        assert_eq!(features(&lengths), [(0x08, 8), (0x10, 32)]);
        assert_eq!(inputs(&lengths), [(0x03, 2)]);
        assert_eq!(lengths.feature(0x10), Some(32));
        assert_eq!(lengths.feature(0x11), None);
    }

    #[test]
    fn empty_descriptor() {
        let lengths = ReportLengths::parse(&[]).unwrap();
        assert!(features(&lengths).is_empty());
        assert!(inputs(&lengths).is_empty());
    }

    #[test]
    fn feature_items_are_summed_and_rounded_up() {
        let desc = [
            0x85, 0x41, // Report ID 0x41
            0x75, 0x08, 0x95, 0x02, 0xb1, 0x02, // 16 bits
            0x75, 0x01, 0x95, 0x0a, 0xb1, 0x02, // 10 bits
        ];
        let lengths = ReportLengths::parse(&desc).unwrap();
        // 26 bits -> 4 bytes, plus the Report ID.
        assert_eq!(features(&lengths), [(0x41, 5)]);
    }

    #[test]
    fn no_report_id_has_no_id_byte() {
        let desc = [0x75, 0x08, 0x95, 0x04, 0x81, 0x02];
        let lengths = ReportLengths::parse(&desc).unwrap();
        assert_eq!(inputs(&lengths), [(0, 4)]);
    }

    #[test]
    fn push_and_pop_restore_globals() {
        let desc = [
            0x85, 0x01, 0x75, 0x08, 0x95, 0x02, // ID 1, 8x2
            0xa4, // PUSH
            0x85, 0x02, 0x95, 0x04, 0xb1, 0x02, // ID 2, 8x4 feature
            0xb4, // POP
            0xb1, 0x02, // ID 1, 8x2 feature
        ];
        let lengths = ReportLengths::parse(&desc).unwrap();
        assert_eq!(features(&lengths), [(1, 3), (2, 5)]);
    }

    #[test]
    fn long_items_are_skipped() {
        let desc = [
            0xfe, 0x03, 0x10, 0xaa, 0xbb, 0xcc, // long item, 3 data bytes
            0x85, 0x05, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02,
        ];
        let lengths = ReportLengths::parse(&desc).unwrap();
        assert_eq!(features(&lengths), [(5, 2)]);
    }

    #[test]
    fn four_byte_items() {
        // Report Count with a 4-byte value (size code 3).
        let desc = [
            0x85, 0x07, 0x75, 0x08, 0x97, 0x03, 0x00, 0x00, 0x00, 0xb1, 0x02,
        ];
        let lengths = ReportLengths::parse(&desc).unwrap();
        assert_eq!(features(&lengths), [(7, 4)]);
    }

    #[test]
    fn other_main_items_do_not_count() {
        // Output (0x91) and Collection/End Collection are ignored.
        let desc = [
            0x85, 0x02, 0x75, 0x08, 0x95, 0x04, 0x91, 0x02, 0xa1, 0x01, 0xc0,
        ];
        let lengths = ReportLengths::parse(&desc).unwrap();
        assert!(features(&lengths).is_empty());
        assert!(inputs(&lengths).is_empty());
    }

    #[test]
    fn errors() {
        assert_eq!(
            ReportLengths::parse(&[0xfe, 0x01]),
            Err(DescriptorError::TruncatedLongItemHeader)
        );
        assert_eq!(
            ReportLengths::parse(&[0xfe, 0x02, 0x10, 0xaa]),
            Err(DescriptorError::TruncatedLongItem)
        );
        assert_eq!(
            ReportLengths::parse(&[0x06, 0x00]),
            Err(DescriptorError::TruncatedShortItem)
        );
        assert_eq!(
            ReportLengths::parse(&[0x85, 0x00]),
            Err(DescriptorError::ReportIdZero)
        );
        assert_eq!(
            ReportLengths::parse(&[0x86, 0x00, 0x01]),
            Err(DescriptorError::ReportIdTooLarge(0x100))
        );
        assert_eq!(
            ReportLengths::parse(&[0xb4]),
            Err(DescriptorError::PopWithoutPush)
        );
        assert_eq!(
            ReportLengths::parse(&[0xa4; MAX_PUSH_DEPTH + 1]),
            Err(DescriptorError::PushTooDeep)
        );
        assert!(ReportLengths::parse(&[0xa4; MAX_PUSH_DEPTH]).is_ok());
    }
}
