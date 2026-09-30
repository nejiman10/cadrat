//! Setting keys and values, as written in TOML paths and `key=value`
//! arguments (spec tool/cli §2).

use std::fmt;
use std::str::FromStr;

use cadrat_proto::{Action, ButtonName, Dpi, PollingRate, WheelMode};

/// One of the twelve settings in schema 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// `mouse.dpi`
    Dpi,
    /// `mouse.polling_rate`
    PollingRate,
    /// `mouse.wheel`
    Wheel,
    /// `mouse.lift.enabled`
    LiftEnabled,
    /// `mouse.lift.threshold`
    LiftThreshold,
    /// `buttons.<name>`
    Button(ButtonName),
}

impl Key {
    /// All keys in display order (the order `get` prints them).
    pub const ALL: [Self; 12] = [
        Self::Dpi,
        Self::PollingRate,
        Self::Wheel,
        Self::LiftEnabled,
        Self::LiftThreshold,
        Self::Button(ButtonName::Left),
        Self::Button(ButtonName::Right),
        Self::Button(ButtonName::Middle),
        Self::Button(ButtonName::Wheel),
        Self::Button(ButtonName::Forward),
        Self::Button(ButtonName::Back),
        Self::Button(ButtonName::Radial),
    ];

    /// Position in [`Key::ALL`].
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Dpi => 0,
            Self::PollingRate => 1,
            Self::Wheel => 2,
            Self::LiftEnabled => 3,
            Self::LiftThreshold => 4,
            Self::Button(name) => 5 + name.index(),
        }
    }

    /// TOML table path and the key inside it, e.g. `(["mouse", "lift"], "enabled")`.
    #[must_use]
    pub fn path(self) -> (&'static [&'static str], &'static str) {
        match self {
            Self::Dpi => (&["mouse"], "dpi"),
            Self::PollingRate => (&["mouse"], "polling_rate"),
            Self::Wheel => (&["mouse"], "wheel"),
            Self::LiftEnabled => (&["mouse", "lift"], "enabled"),
            Self::LiftThreshold => (&["mouse", "lift"], "threshold"),
            Self::Button(name) => (&["buttons"], name.as_str()),
        }
    }

    /// Parses a value written on the command line for this key.
    ///
    /// Strings are written without quotes; integers in decimal or `0x` hex.
    ///
    /// # Errors
    ///
    /// A message describing the accepted values.
    pub fn parse_value(self, text: &str) -> Result<Value, String> {
        match self {
            Self::Dpi => text.parse().map(Value::Dpi).map_err(|e| e.to_string()),
            Self::PollingRate => text
                .parse()
                .map(Value::PollingRate)
                .map_err(|e| e.to_string()),
            Self::Wheel => text.parse().map(Value::Wheel).map_err(|e| e.to_string()),
            Self::LiftEnabled => match text {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err("mouse.lift.enabled must be true or false".to_owned()),
            },
            Self::LiftThreshold => parse_threshold(text).map(Value::Threshold),
            Self::Button(_) => text.parse().map(Value::Action).map_err(|e| e.to_string()),
        }
    }
}

fn parse_threshold(text: &str) -> Result<u8, String> {
    cadrat_proto::parse_u32(text)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| "mouse.lift.threshold must be an integer in range 0..255".to_owned())
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (tables, key) = self.path();
        for table in tables {
            write!(f, "{table}.")?;
        }
        f.write_str(key)
    }
}

/// A string that names no setting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown key {0:?}")]
pub struct UnknownKey(pub String);

impl FromStr for Key {
    type Err = UnknownKey;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|key| key.to_string() == s)
            .ok_or_else(|| UnknownKey(s.to_owned()))
    }
}

/// A validated setting value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    /// `mouse.dpi`
    Dpi(Dpi),
    /// `mouse.polling_rate`
    PollingRate(PollingRate),
    /// `mouse.wheel`
    Wheel(WheelMode),
    /// `mouse.lift.enabled`
    Bool(bool),
    /// `mouse.lift.threshold`
    Threshold(u8),
    /// `buttons.*`
    Action(Action),
}

impl fmt::Display for Value {
    /// The `get` form: integers in decimal, strings without quotes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dpi(dpi) => dpi.fmt(f),
            Self::PollingRate(rate) => rate.fmt(f),
            Self::Wheel(wheel) => wheel.fmt(f),
            Self::Bool(enabled) => enabled.fmt(f),
            Self::Threshold(threshold) => threshold.fmt(f),
            Self::Action(action) => action.fmt(f),
        }
    }
}

/// Why a `key=value` argument list was rejected (a usage error, spec tool/cli §2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssignmentError {
    /// No `=`.
    #[error("expected key=value, got {0:?}")]
    Syntax(String),
    /// The key names no setting.
    #[error(transparent)]
    UnknownKey(#[from] UnknownKey),
    /// The value is not valid for the key.
    #[error("{key}: {message}")]
    Value {
        /// The key.
        key: Key,
        /// What the key accepts.
        message: String,
    },
    /// The same key appears twice.
    #[error("{0} is given more than once")]
    Duplicate(Key),
}

/// Parses `key=value` arguments, splitting at the first `=`.
///
/// # Errors
///
/// See [`AssignmentError`].
pub fn parse_assignments<S: AsRef<str>>(args: &[S]) -> Result<Vec<(Key, Value)>, AssignmentError> {
    let mut assignments: Vec<(Key, Value)> = Vec::with_capacity(args.len());
    for arg in args {
        let arg = arg.as_ref();
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| AssignmentError::Syntax(arg.to_owned()))?;
        let key: Key = key.parse()?;
        if assignments.iter().any(|(seen, _)| *seen == key) {
            return Err(AssignmentError::Duplicate(key));
        }
        let value = key
            .parse_value(value)
            .map_err(|message| AssignmentError::Value { key, message })?;
        assignments.push((key, value));
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip() {
        let names: Vec<String> = Key::ALL.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "mouse.dpi",
                "mouse.polling_rate",
                "mouse.wheel",
                "mouse.lift.enabled",
                "mouse.lift.threshold",
                "buttons.left",
                "buttons.right",
                "buttons.middle",
                "buttons.wheel",
                "buttons.forward",
                "buttons.back",
                "buttons.radial",
            ]
        );
        for (i, key) in Key::ALL.into_iter().enumerate() {
            assert_eq!(key.to_string().parse(), Ok(key));
            assert_eq!(key.index(), i);
        }
        assert!("mouse.lift".parse::<Key>().is_err());
        assert!("buttons.backward".parse::<Key>().is_err());
        assert!("Mouse.dpi".parse::<Key>().is_err());
    }

    #[test]
    fn values_from_arguments() {
        let parsed = parse_assignments(&[
            "mouse.dpi=1000",
            "mouse.wheel=inertial",
            "mouse.lift.enabled=true",
            "mouse.lift.threshold=0x20",
            "buttons.radial=host:1",
        ])
        .unwrap();
        let shown: Vec<String> = parsed.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            shown,
            [
                "mouse.dpi=1000",
                "mouse.wheel=inertial",
                "mouse.lift.enabled=true",
                "mouse.lift.threshold=32",
                "buttons.radial=host:1",
            ]
        );
    }

    #[test]
    fn argument_errors() {
        assert_eq!(
            parse_assignments(&["mouse.dpi"]),
            Err(AssignmentError::Syntax("mouse.dpi".into()))
        );
        assert_eq!(
            parse_assignments(&["mouse.dpi =1000"]),
            Err(AssignmentError::UnknownKey(UnknownKey("mouse.dpi ".into())))
        );
        assert_eq!(
            parse_assignments(&["mouse.dpi=1000", "mouse.dpi=1200"]),
            Err(AssignmentError::Duplicate(Key::Dpi))
        );
        for (arg, key) in [
            ("mouse.dpi=1025", Key::Dpi),
            ("mouse.dpi=9000", Key::Dpi),
            ("mouse.polling_rate=100", Key::PollingRate),
            ("mouse.wheel=Normal", Key::Wheel),
            ("mouse.lift.enabled=yes", Key::LiftEnabled),
            ("mouse.lift.threshold=256", Key::LiftThreshold),
            ("mouse.lift.threshold=-1", Key::LiftThreshold),
            ("mouse.lift.threshold=", Key::LiftThreshold),
            ("buttons.left=raw:0x0a", Key::Button(ButtonName::Left)),
            ("buttons.left=host:216", Key::Button(ButtonName::Left)),
        ] {
            assert!(
                matches!(parse_assignments(&[arg]), Err(AssignmentError::Value { key: k, .. }) if k == key),
                "{arg}"
            );
        }
    }

    #[test]
    fn value_splits_at_first_equals() {
        assert!(matches!(
            parse_assignments(&["buttons.left=mouse:left=x"]),
            Err(AssignmentError::Value { .. })
        ));
    }
}
