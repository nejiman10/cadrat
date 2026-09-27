//! A complete, validated schema 1 configuration.

use std::fmt;

use cadrat_proto::{
    Action, ButtonName, Buttons, DirectAction, Dpi, Lift, PollingRate, Report10Config, WheelMode,
};

use crate::key::{Key, Value};

/// Every schema 1 setting, validated.
///
/// Unlike [`Report10Config`], the lift threshold is kept while lift is
/// disabled, because the TOML file always holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    /// `mouse.dpi`
    pub dpi: Dpi,
    /// `mouse.polling_rate`
    pub polling_rate: PollingRate,
    /// `mouse.wheel`
    pub wheel: WheelMode,
    /// `mouse.lift.enabled`
    pub lift_enabled: bool,
    /// `mouse.lift.threshold`
    pub lift_threshold: u8,
    /// `buttons.*`
    pub buttons: Buttons,
}

impl Config {
    /// The values of `init --preset=research-baseline` (spec 01 §8).
    #[must_use]
    pub fn research_baseline() -> Self {
        let report = Report10Config::research_baseline();
        Self {
            dpi: report.dpi,
            polling_rate: report.polling_rate,
            wheel: report.wheel,
            lift_enabled: false,
            lift_threshold: cadrat_proto::report10::LIFT_DISABLED,
            buttons: report.buttons,
        }
    }

    /// Builds a configuration from one value per key, in [`Key::ALL`] order.
    ///
    /// Returns `None` if a value is missing or has the wrong kind for its key.
    #[must_use]
    pub fn from_values(values: &[Option<Value>; 12]) -> Option<Self> {
        let get = |key: Key| values[key.index()];
        let mut buttons = [Action::Direct(DirectAction::Left); 7];
        for name in ButtonName::ALL {
            let Some(Value::Action(action)) = get(Key::Button(name)) else {
                return None;
            };
            buttons[name.index()] = action;
        }
        Some(Self {
            dpi: match get(Key::Dpi)? {
                Value::Dpi(dpi) => dpi,
                _ => return None,
            },
            polling_rate: match get(Key::PollingRate)? {
                Value::PollingRate(rate) => rate,
                _ => return None,
            },
            wheel: match get(Key::Wheel)? {
                Value::Wheel(wheel) => wheel,
                _ => return None,
            },
            lift_enabled: match get(Key::LiftEnabled)? {
                Value::Bool(enabled) => enabled,
                _ => return None,
            },
            lift_threshold: match get(Key::LiftThreshold)? {
                Value::Threshold(threshold) => threshold,
                _ => return None,
            },
            buttons: Buttons(buttons),
        })
    }

    /// The value of one key.
    #[must_use]
    pub fn get(&self, key: Key) -> Value {
        match key {
            Key::Dpi => Value::Dpi(self.dpi),
            Key::PollingRate => Value::PollingRate(self.polling_rate),
            Key::Wheel => Value::Wheel(self.wheel),
            Key::LiftEnabled => Value::Bool(self.lift_enabled),
            Key::LiftThreshold => Value::Threshold(self.lift_threshold),
            Key::Button(name) => Value::Action(self.buttons.get(name)),
        }
    }

    /// Replaces the value of one key.
    ///
    /// # Errors
    ///
    /// [`WrongValueKind`] if the value does not belong to the key.
    pub fn set(&mut self, key: Key, value: Value) -> Result<(), WrongValueKind> {
        match (key, value) {
            (Key::Dpi, Value::Dpi(dpi)) => self.dpi = dpi,
            (Key::PollingRate, Value::PollingRate(rate)) => self.polling_rate = rate,
            (Key::Wheel, Value::Wheel(wheel)) => self.wheel = wheel,
            (Key::LiftEnabled, Value::Bool(enabled)) => self.lift_enabled = enabled,
            (Key::LiftThreshold, Value::Threshold(threshold)) => self.lift_threshold = threshold,
            (Key::Button(name), Value::Action(action)) => self.buttons.set(name, action),
            _ => return Err(WrongValueKind(key)),
        }
        Ok(())
    }

    /// Applies assignments in order and lists every assignment with its old
    /// value (spec 03 §4 step 5).
    ///
    /// # Errors
    ///
    /// [`WrongValueKind`] if a value does not belong to its key.
    pub fn apply(
        &self,
        assignments: &[(Key, Value)],
    ) -> Result<(Self, Vec<Change>), WrongValueKind> {
        let mut next = *self;
        let mut changes = Vec::with_capacity(assignments.len());
        for &(key, to) in assignments {
            let from = next.get(key);
            next.set(key, to)?;
            changes.push(Change { key, from, to });
        }
        Ok((next, changes))
    }

    /// The Report `0x10` configuration this file describes.
    #[must_use]
    pub fn to_report(&self) -> Report10Config {
        Report10Config {
            dpi: self.dpi,
            lift: if self.lift_enabled {
                Lift::Enabled {
                    threshold: self.lift_threshold,
                }
            } else {
                Lift::Disabled
            },
            wheel: self.wheel,
            buttons: self.buttons,
            polling_rate: self.polling_rate,
        }
    }

    /// Warnings for sending this configuration (spec 01 §4.4, §5).
    #[must_use]
    pub fn warnings(&self) -> Vec<Warning> {
        let mut warnings = Vec::new();
        if self.lift_enabled {
            warnings.push(Warning::LiftExperimental);
            if self.to_report().lift.is_ambiguous() {
                warnings.push(Warning::LiftAmbiguous);
            }
        }
        for (name, action) in self.buttons.iter() {
            match action {
                Action::Direct(DirectAction::Unknown6) => warnings.push(Warning::Unknown6(name)),
                Action::HostRouted(index) if index.report03_mask().is_none() => {
                    warnings.push(Warning::HostUnobservable(name, index.get()));
                }
                Action::Raw(raw) => warnings.push(Warning::Raw(name, raw.get())),
                _ => {}
            }
        }
        warnings
    }
}

/// A value was given for a key it does not belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("value does not belong to {0}")]
pub struct WrongValueKind(pub Key);

/// One assignment and the value it replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// The key.
    pub key: Key,
    /// The value before.
    pub from: Value,
    /// The value after.
    pub to: Value,
}

impl Change {
    /// Whether the value actually changes.
    #[must_use]
    pub fn is_effective(&self) -> bool {
        self.from != self.to
    }
}

/// Warnings derived from the configuration (spec 03 §7). They never stop a
/// send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// `W-LIFT-EXPERIMENTAL`: lift enabled is sent.
    LiftExperimental,
    /// `W-LIFT-AMBIGUOUS`: enabled with threshold 31.
    LiftAmbiguous,
    /// `W-UNKNOWN-6`: `unknown:6` is sent.
    Unknown6(ButtonName),
    /// `W-HOST-UNOBSERVABLE`: `host:N` with N outside 1..7.
    HostUnobservable(ButtonName, u8),
    /// `W-RAW`: `raw:` is sent.
    Raw(ButtonName, u8),
}

impl Warning {
    /// The warning code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::LiftExperimental => "W-LIFT-EXPERIMENTAL",
            Self::LiftAmbiguous => "W-LIFT-AMBIGUOUS",
            Self::Unknown6(_) => "W-UNKNOWN-6",
            Self::HostUnobservable(..) => "W-HOST-UNOBSERVABLE",
            Self::Raw(..) => "W-RAW",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LiftExperimental => f.write_str(
                "lift detection is experimental on C658; the vendor UI does not offer it for this model",
            ),
            Self::LiftAmbiguous => f.write_str(
                "mouse.lift.threshold = 31 (0x1f) encodes the same byte as lift disabled",
            ),
            Self::Unknown6(name) => write!(
                f,
                "buttons.{name} = unknown:6 has no known meaning; its effect is unverified"
            ),
            Self::HostUnobservable(name, index) => write!(
                f,
                "buttons.{name} = host:{index} has no bit in Input Report 0x03; only host:1..7 can be observed"
            ),
            Self::Raw(name, wire) => write!(
                f,
                "buttons.{name} = raw:0x{wire:02x} is not produced by the normal configuration path"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrat_proto::HostIndex;

    fn host(index: u8) -> Action {
        Action::HostRouted(HostIndex::new(index).unwrap())
    }

    #[test]
    fn baseline_matches_proto() {
        let config = Config::research_baseline();
        assert_eq!(config.to_report(), Report10Config::research_baseline());
        assert!(config.warnings().is_empty());
    }

    #[test]
    fn values_round_trip() {
        let config = Config::research_baseline();
        let values = Key::ALL.map(|key| Some(config.get(key)));
        assert_eq!(Config::from_values(&values), Some(config));
        let mut missing = values;
        missing[Key::Wheel.index()] = None;
        assert_eq!(Config::from_values(&missing), None);
        let mut wrong = values;
        wrong[Key::Wheel.index()] = Some(Value::Bool(true));
        assert_eq!(Config::from_values(&wrong), None);
    }

    #[test]
    fn disabled_lift_keeps_threshold_but_sends_0x1f() {
        let mut config = Config::research_baseline();
        config.lift_threshold = 10;
        assert_eq!(config.to_report().to_blob()[2], 0x1f);
        config.lift_enabled = true;
        assert_eq!(config.to_report().to_blob()[2], 10);
    }

    #[test]
    fn apply_lists_changes() {
        let config = Config::research_baseline();
        let (next, changes) = config
            .apply(&[
                (Key::Dpi, Value::Dpi(Dpi::new(1000).unwrap())),
                (Key::Button(ButtonName::Radial), Value::Action(host(1))),
                (Key::Wheel, Value::Wheel(WheelMode::Normal)),
            ])
            .unwrap();
        assert_eq!(next.dpi.get(), 1000);
        assert_eq!(next.buttons.get(ButtonName::Radial), host(1));
        let effective: Vec<String> = changes
            .iter()
            .filter(|c| c.is_effective())
            .map(|c| format!("{}={} -> {}", c.key, c.from, c.to))
            .collect();
        assert_eq!(
            effective,
            [
                "mouse.dpi=1400 -> 1000",
                "buttons.radial=mouse:middle -> host:1"
            ]
        );
        assert_eq!(changes.len(), 3);
        assert_eq!(
            config.apply(&[(Key::Dpi, Value::Bool(true))]),
            Err(WrongValueKind(Key::Dpi))
        );
    }

    #[test]
    fn warnings() {
        let mut config = Config::research_baseline();
        config.lift_enabled = true;
        config.lift_threshold = 31;
        config
            .buttons
            .set(ButtonName::Left, Action::Direct(DirectAction::Unknown6));
        config.buttons.set(ButtonName::Right, host(0));
        config.buttons.set(ButtonName::Middle, host(1));
        config.buttons.set(ButtonName::Wheel, host(7));
        config.buttons.set(ButtonName::Forward, host(8));
        config
            .buttons
            .set(ButtonName::Back, "raw:0x10".parse().unwrap());
        let codes: Vec<&str> = config.warnings().iter().map(Warning::code).collect();
        assert_eq!(
            codes,
            [
                "W-LIFT-EXPERIMENTAL",
                "W-LIFT-AMBIGUOUS",
                "W-UNKNOWN-6",
                "W-HOST-UNOBSERVABLE",
                "W-HOST-UNOBSERVABLE",
                "W-RAW",
            ]
        );
        assert_eq!(
            config.warnings()[3].to_string(),
            "buttons.right = host:0 has no bit in Input Report 0x03; only host:1..7 can be observed"
        );

        config.lift_threshold = 32;
        let codes: Vec<&str> = config.warnings().iter().map(Warning::code).collect();
        assert!(!codes.contains(&"W-LIFT-AMBIGUOUS"));
    }
}
