//! Templates written by `init` (spec 01 §8).

use std::fmt::Write as _;

use cadrat_proto::ButtonName;

use crate::config::Config;
use crate::key::Key;

/// What `init` fills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Every value commented out. The file stays incomplete until the user
    /// writes each value.
    Empty,
    /// `--preset=research-baseline`: the research SDK's
    /// `latest_software_baseline()`.
    ResearchBaseline,
}

const HEADER: &str = "\
# cadrat configuration (schema 1)
# Every key is required. Ranges and meanings:
# https://github.com/nejiman10/cadrat/blob/main/docs/spec/01-config.md
";

const EMPTY_NOTE: &str = "\
# Choose each value and uncomment its line. Nothing is sent while a value is missing.
";

const BASELINE_NOTE: &str = "\
# Values from the research SDK's latest_software_baseline() (--preset=research-baseline).
# They were not read from a device and are not factory defaults.
# Sending them overwrites the mouse's current settings.
";

fn comment(key: Key) -> Option<&'static str> {
    match key {
        Key::Dpi => Some("50..8200, in steps of 50"),
        Key::PollingRate => Some("125 | 250 | 500 | 1000"),
        Key::Wheel => Some("\"normal\" | \"inertial\""),
        Key::LiftThreshold => Some("0..255, used only when enabled = true"),
        Key::LiftEnabled | Key::Button(_) => None,
    }
}

/// The template text. Both presets parse as schema 1; the empty one lists
/// every key as missing.
#[must_use]
pub fn template(preset: Preset) -> String {
    let config = Config::research_baseline();
    let prefix = match preset {
        Preset::Empty => "# ",
        Preset::ResearchBaseline => "",
    };
    let mut out = String::from(HEADER);
    out.push_str(match preset {
        Preset::Empty => EMPTY_NOTE,
        Preset::ResearchBaseline => BASELINE_NOTE,
    });
    out.push_str("\nschema = 1\n");
    let line = |out: &mut String, key: Key, width: usize| {
        let value = config.get(key).to_string();
        let value = match key {
            Key::Dpi | Key::PollingRate | Key::LiftEnabled | Key::LiftThreshold => value,
            _ => format!("\"{value}\""),
        };
        let name = key.path().1;
        let assignment = format!("{prefix}{name:width$} = {value}");
        // Writing to a String cannot fail.
        let _ = match comment(key) {
            Some(comment) => writeln!(out, "{assignment:<24}# {comment}"),
            None => writeln!(out, "{assignment}"),
        };
    };

    out.push_str("\n[mouse]\n");
    for key in [Key::Dpi, Key::PollingRate, Key::Wheel] {
        line(&mut out, key, 0);
    }
    out.push_str("\n[mouse.lift]            # experimental on C658; sending it warns\n");
    for key in [Key::LiftEnabled, Key::LiftThreshold] {
        line(&mut out, key, 0);
    }
    out.push_str(
        "\n[buttons]\n\
         # actions: mouse:left | mouse:right | mouse:middle | mouse:backward | mouse:forward\n\
         #          unknown:6 | host:<0..215> | raw:<0x10..0x27>\n",
    );
    for name in ButtonName::ALL {
        line(&mut out, Key::Button(name), 7);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ConfigError, Document};

    #[test]
    fn baseline_template() {
        let text = template(Preset::ResearchBaseline);
        let config = Document::parse(&text).unwrap().config().unwrap();
        assert_eq!(config, Config::research_baseline());
        assert!(text.contains("are not factory defaults"));
        assert!(text.contains("forward = \"mouse:forward\"\nback    = \"mouse:backward\"\n"));
        assert!(text.contains("dpi = 1400              # 50..8200, in steps of 50\n"));
    }

    #[test]
    fn empty_template_is_incomplete() {
        let text = template(Preset::Empty);
        let err = Document::parse(&text).unwrap().config().unwrap_err();
        assert_eq!(err, ConfigError::Incomplete(Key::ALL.to_vec()));
    }

    #[test]
    fn uncommenting_the_empty_template_gives_the_baseline() {
        let uncommented: String = template(Preset::Empty)
            .lines()
            .map(|l| {
                let is_value = l
                    .strip_prefix("# ")
                    .and_then(|rest| rest.split_whitespace().next())
                    .is_some_and(|word| Key::ALL.iter().any(|k| k.path().1 == word));
                if is_value { &l[2..] } else { l }
            })
            .flat_map(|l| [l, "\n"])
            .collect();
        let config = Document::parse(&uncommented).unwrap().config().unwrap();
        assert_eq!(config, Config::research_baseline());
    }
}
