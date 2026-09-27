//! Integer parsing shared by the `FromStr` implementations.

/// Parses a non-negative integer written in decimal or as `0x` hexadecimal.
///
/// These are the integer forms accepted in command-line values (spec 03 §2):
/// decimal without sign or leading zeros, or a lowercase `0x` prefix followed
/// by hexadecimal digits. The TOML file itself accepts every TOML notation. Returns `None` for anything else, including values
/// that do not fit in `u32`.
#[must_use]
pub fn parse_u32(s: &str) -> Option<u32> {
    if let Some(hex) = s.strip_prefix("0x") {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return u32::from_str_radix(hex, 16).ok();
    }
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_u32;

    #[test]
    fn accepts_decimal_and_hex() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("1600"), Some(1600));
        assert_eq!(parse_u32("0x1f"), Some(31));
        assert_eq!(parse_u32("0x1F"), Some(31));
    }

    #[test]
    fn rejects_other_forms() {
        for s in [
            "", "+1", "-1", "01", "0X1f", "0x", "1_000", " 1", "1.0", "0o7", "0b1",
        ] {
            assert_eq!(parse_u32(s), None, "{s:?}");
        }
        assert_eq!(parse_u32("4294967296"), None);
    }
}
