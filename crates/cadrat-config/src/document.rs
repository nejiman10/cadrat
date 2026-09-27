//! Reading, validating and updating the TOML file in place (spec 01 §2, §7).

use std::fmt;

use cadrat_proto::{ButtonName, Dpi, PollingRate};
use toml_edit::{DocumentMut, Item, TableLike};

use crate::config::{Change, Config};
use crate::key::{Key, Value};

/// The only schema version this crate reads.
pub const SCHEMA: i64 = 1;

/// A parsed TOML file that remembers its formatting.
#[derive(Debug, Clone)]
pub struct Document {
    doc: DocumentMut,
}

/// The file is not valid TOML.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct SyntaxError(String);

/// One problem found while validating (spec 01 §2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No `schema` key.
    SchemaMissing,
    /// `schema` is not an integer.
    SchemaType(&'static str),
    /// `schema` is an integer other than 1.
    SchemaUnsupported(i64),
    /// A required key is absent.
    Missing(Key),
    /// A key that schema 1 does not define, as a dotted path.
    Unknown(String),
    /// A path that must be a table is something else.
    NotTable(String),
    /// A value of the wrong TOML type.
    Type {
        /// The key.
        key: Key,
        /// The TOML type the key takes.
        expected: &'static str,
        /// The TOML type found.
        found: &'static str,
    },
    /// A value of the right type that is out of range or unknown.
    Value {
        /// The key.
        key: Key,
        /// What the key accepts.
        message: String,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaMissing => f.write_str("schema is missing; this version reads schema = 1"),
            Self::SchemaType(found) => write!(f, "schema must be an integer, found {found}"),
            Self::SchemaUnsupported(n) if *n > SCHEMA => write!(
                f,
                "schema = {n} is newer than this version of cadrat supports (schema = 1); update cadrat"
            ),
            Self::SchemaUnsupported(n) => {
                write!(
                    f,
                    "schema = {n} is not supported; this version reads schema = 1"
                )
            }
            Self::Missing(key) => write!(f, "{key} is missing"),
            Self::Unknown(path) => write!(f, "unknown key {path}"),
            Self::NotTable(path) => write!(f, "{path} must be a table"),
            Self::Type {
                key,
                expected,
                found,
            } => write!(f, "{key} must be {expected}, found {found}"),
            Self::Value { key, message } => write!(f, "{key}: {message}"),
        }
    }
}

/// Why a document is not a complete, valid configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// `ConfigIncomplete`: only required keys are missing.
    #[error("configuration is incomplete: missing {}", join_keys(.0))]
    Incomplete(Vec<Key>),
    /// `ConfigInvalid`: at least one problem other than a missing key.
    #[error("configuration is invalid: {}", join_problems(.0))]
    Invalid(Vec<Problem>),
}

impl ConfigError {
    /// Every problem, missing keys included.
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        match self {
            Self::Incomplete(keys) => keys.iter().copied().map(Problem::Missing).collect(),
            Self::Invalid(problems) => problems.clone(),
        }
    }
}

fn join_keys(keys: &[Key]) -> String {
    keys.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_problems(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The result of validating a document: whatever values could be read, and
/// every problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// One entry per key, in [`Key::ALL`] order; `None` if missing or invalid.
    pub values: [Option<Value>; 12],
    /// Every problem found, in file order within each table.
    pub problems: Vec<Problem>,
}

impl Checked {
    /// The configuration, or the reason there is none.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Incomplete`] if the only problems are missing keys,
    /// otherwise [`ConfigError::Invalid`].
    pub fn into_config(self) -> Result<Config, ConfigError> {
        if self.problems.is_empty()
            && let Some(config) = Config::from_values(&self.values)
        {
            return Ok(config);
        }
        let missing: Vec<Key> = self
            .problems
            .iter()
            .filter_map(|p| match p {
                Problem::Missing(key) => Some(*key),
                _ => None,
            })
            .collect();
        if missing.len() == self.problems.len() {
            Err(ConfigError::Incomplete(missing))
        } else {
            Err(ConfigError::Invalid(self.problems))
        }
    }
}

/// A key could not be rewritten because it is not in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not in the file")]
pub struct UpdateError(pub Key);

impl Document {
    /// Parses TOML text.
    ///
    /// # Errors
    ///
    /// [`SyntaxError`] if the text is not TOML.
    pub fn parse(text: &str) -> Result<Self, SyntaxError> {
        text.parse::<DocumentMut>()
            .map(|doc| Self { doc })
            .map_err(|e| SyntaxError(e.to_string().trim_end().to_owned()))
    }

    /// Validates the document against schema 1 and reads every value it can.
    ///
    /// If `schema` is missing or unsupported, only that problem is reported,
    /// since the other keys may follow a different schema.
    #[must_use]
    pub fn check(&self) -> Checked {
        let mut checked = Checked {
            values: [None; 12],
            problems: Vec::new(),
        };
        let root = self.doc.as_table();
        match root.get("schema").map(|item| item.as_value()) {
            None => checked.problems.push(Problem::SchemaMissing),
            Some(Some(toml_edit::Value::Integer(n))) if *n.value() == SCHEMA => {}
            Some(Some(toml_edit::Value::Integer(n))) => {
                checked
                    .problems
                    .push(Problem::SchemaUnsupported(*n.value()));
            }
            Some(other) => checked.problems.push(Problem::SchemaType(
                other.map_or("table", toml_edit::Value::type_name),
            )),
        }
        if !checked.problems.is_empty() {
            return checked;
        }

        unknown_keys(
            root,
            "",
            &["schema", "mouse", "buttons"],
            &mut checked.problems,
        );
        let mouse = table(Parent::Table(root), "mouse", &mut checked.problems);
        let lift = table(mouse, "mouse.lift", &mut checked.problems);
        let buttons = table(Parent::Table(root), "buttons", &mut checked.problems);
        if let Parent::Table(mouse) = mouse {
            unknown_keys(
                mouse,
                "mouse.",
                &["dpi", "polling_rate", "wheel", "lift"],
                &mut checked.problems,
            );
        }
        if let Parent::Table(lift) = lift {
            unknown_keys(
                lift,
                "mouse.lift.",
                &["enabled", "threshold"],
                &mut checked.problems,
            );
        }
        if let Parent::Table(buttons) = buttons {
            let names = ButtonName::ALL.map(ButtonName::as_str);
            unknown_keys(buttons, "buttons.", &names, &mut checked.problems);
        }

        for key in Key::ALL {
            let parent = match key {
                Key::Dpi | Key::PollingRate | Key::Wheel => mouse,
                Key::LiftEnabled | Key::LiftThreshold => lift,
                Key::Button(_) => buttons,
            };
            match parent {
                // Already reported as NotTable.
                Parent::Broken => {}
                Parent::Absent => checked.problems.push(Problem::Missing(key)),
                Parent::Table(parent) => match parent.get(key.path().1) {
                    None => checked.problems.push(Problem::Missing(key)),
                    Some(item) => match read_value(key, item) {
                        Ok(value) => checked.values[key.index()] = Some(value),
                        Err(problem) => checked.problems.push(problem),
                    },
                },
            }
        }
        checked
    }

    /// Validates the document into a configuration.
    ///
    /// # Errors
    ///
    /// See [`Checked::into_config`].
    pub fn config(&self) -> Result<Config, ConfigError> {
        self.check().into_config()
    }

    /// Rewrites the value of each effective change, leaving everything else
    /// untouched: comments, key order, blank lines and the notation of other
    /// values. A replaced integer written in `0x` hex stays hex, and the
    /// replaced value keeps its surrounding whitespace and trailing comment.
    ///
    /// # Errors
    ///
    /// [`UpdateError`] if a changed key is not in the file. Validate the
    /// document first.
    pub fn update(&mut self, changes: &[Change]) -> Result<(), UpdateError> {
        for change in changes.iter().filter(|c| c.is_effective()) {
            let key = change.key;
            let (tables, name) = key.path();
            let mut parent: &mut dyn TableLike = self.doc.as_table_mut();
            for table in tables {
                parent = parent
                    .get_mut(table)
                    .and_then(Item::as_table_like_mut)
                    .ok_or(UpdateError(key))?;
            }
            let old = parent
                .get_mut(name)
                .and_then(Item::as_value_mut)
                .ok_or(UpdateError(key))?;
            let mut new = toml_value(change.to, old);
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        Ok(())
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.doc.fmt(f)
    }
}

/// A table on the way to a key.
#[derive(Clone, Copy)]
enum Parent<'a> {
    /// The table is not in the file, so its keys are missing.
    Absent,
    /// The path holds something other than a table (already reported).
    Broken,
    Table(&'a dyn TableLike),
}

fn table<'a>(parent: Parent<'a>, path: &str, problems: &mut Vec<Problem>) -> Parent<'a> {
    let Parent::Table(parent) = parent else {
        return parent;
    };
    let name = path.rsplit('.').next().unwrap_or(path);
    let Some(item) = parent.get(name) else {
        return Parent::Absent;
    };
    if let Some(table) = item.as_table_like() {
        Parent::Table(table)
    } else {
        problems.push(Problem::NotTable(path.to_owned()));
        Parent::Broken
    }
}

fn unknown_keys(table: &dyn TableLike, prefix: &str, known: &[&str], problems: &mut Vec<Problem>) {
    for (name, _) in table.iter() {
        if !known.contains(&name) {
            problems.push(Problem::Unknown(format!("{prefix}{name}")));
        }
    }
}

fn type_name(item: &Item) -> &'static str {
    match item {
        Item::Value(value) => value.type_name(),
        Item::Table(_) => "table",
        Item::ArrayOfTables(_) => "array of tables",
        Item::None => "nothing",
    }
}

fn read_value(key: Key, item: &Item) -> Result<Value, Problem> {
    let expected = match key {
        Key::Dpi | Key::PollingRate | Key::LiftThreshold => "integer",
        Key::Wheel | Key::Button(_) => "string",
        Key::LiftEnabled => "boolean",
    };
    let wrong_type = || Problem::Type {
        key,
        expected,
        found: type_name(item),
    };
    let invalid = |message: String| Problem::Value { key, message };
    match key {
        Key::Dpi => {
            let n = item.as_integer().ok_or_else(wrong_type)?;
            Dpi::try_from_i64(n)
                .map(Value::Dpi)
                .map_err(|e| invalid(e.to_string()))
        }
        Key::PollingRate => {
            let n = item.as_integer().ok_or_else(wrong_type)?;
            PollingRate::from_hz(n)
                .map(Value::PollingRate)
                .ok_or_else(|| invalid("polling_rate must be 125, 250, 500 or 1000".to_owned()))
        }
        Key::LiftThreshold => {
            let n = item.as_integer().ok_or_else(wrong_type)?;
            u8::try_from(n)
                .map(Value::Threshold)
                .map_err(|_| invalid("threshold must be in range 0..255".to_owned()))
        }
        Key::LiftEnabled => item.as_bool().map(Value::Bool).ok_or_else(wrong_type),
        Key::Wheel | Key::Button(_) => {
            let text = item.as_str().ok_or_else(wrong_type)?;
            key.parse_value(text).map_err(invalid)
        }
    }
}

/// Builds the TOML value for `value`, matching the notation of `old` where
/// that notation can carry the new value.
fn toml_value(value: Value, old: &toml_edit::Value) -> toml_edit::Value {
    let integer = |n: i64| -> toml_edit::Value {
        let hex = matches!(old, toml_edit::Value::Integer(i) if i.display_repr().starts_with("0x"));
        if hex {
            format!("0x{n:x}")
                .parse()
                .expect("hex integer literal is valid TOML")
        } else {
            toml_edit::Value::from(n)
        }
    };
    match value {
        Value::Dpi(dpi) => integer(i64::from(dpi.get())),
        Value::PollingRate(rate) => integer(i64::from(rate.hz())),
        Value::Threshold(threshold) => integer(i64::from(threshold)),
        Value::Bool(enabled) => toml_edit::Value::from(enabled),
        Value::Wheel(_) | Value::Action(_) => toml_edit::Value::from(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::parse_assignments;
    use cadrat_proto::{Action, HostIndex};

    const FULL: &str = r#"schema = 1

[mouse]
dpi = 1600              # 50..8200、50刻み
polling_rate = 1000     # 125 | 250 | 500 | 1000
wheel = "normal"        # "normal" | "inertial"

[mouse.lift]            # C658では実験的（§4.4）
enabled = false
threshold = 31          # 0..255。enabled = true のときだけ使う

[buttons]               # 7 entryすべて必須
left    = "mouse:left"
right   = "mouse:right"
middle  = "mouse:middle"
wheel   = "mouse:middle"
forward = "mouse:forward"
back    = "mouse:backward"
radial  = "host:1"
"#;

    fn problems(text: &str) -> Vec<String> {
        Document::parse(text)
            .unwrap()
            .check()
            .problems
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn spec_example_is_valid() {
        let config = Document::parse(FULL).unwrap().config().unwrap();
        assert_eq!(config.dpi.get(), 1600);
        assert!(!config.lift_enabled);
        assert_eq!(config.lift_threshold, 31);
        assert_eq!(
            config.buttons.get(ButtonName::Radial),
            Action::HostRouted(HostIndex::new(1).unwrap())
        );
    }

    #[test]
    fn syntax_error() {
        assert!(Document::parse("schema = ").is_err());
        assert!(Document::parse("[mouse]\ndpi = 1\ndpi = 2\n").is_err());
    }

    #[test]
    fn schema_rules() {
        let without = FULL.replacen("schema = 1\n", "", 1);
        assert_eq!(
            problems(&without),
            ["schema is missing; this version reads schema = 1"]
        );
        assert_eq!(
            problems(&FULL.replacen("schema = 1", "schema = 2", 1)),
            [
                "schema = 2 is newer than this version of cadrat supports (schema = 1); update cadrat"
            ]
        );
        assert_eq!(
            problems(&FULL.replacen("schema = 1", "schema = 0", 1)),
            ["schema = 0 is not supported; this version reads schema = 1"]
        );
        assert_eq!(
            problems(&FULL.replacen("schema = 1", "schema = \"1\"", 1)),
            ["schema must be an integer, found string"]
        );
        assert_eq!(
            problems(&FULL.replacen("schema = 1", "schema = 0x1", 1)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn missing_keys_are_all_listed() {
        let text = "schema = 1\n[mouse]\ndpi = 1600\n[buttons]\nleft = \"mouse:left\"\n";
        let err = Document::parse(text).unwrap().config().unwrap_err();
        let ConfigError::Incomplete(missing) = &err else {
            panic!("{err:?}");
        };
        let names: Vec<String> = missing.iter().map(ToString::to_string).collect();
        assert_eq!(
            names,
            [
                "mouse.polling_rate",
                "mouse.wheel",
                "mouse.lift.enabled",
                "mouse.lift.threshold",
                "buttons.right",
                "buttons.middle",
                "buttons.wheel",
                "buttons.forward",
                "buttons.back",
                "buttons.radial",
            ]
        );
        // Partial values stay available for `get`.
        let checked = Document::parse(text).unwrap().check();
        assert_eq!(
            checked.values[Key::Dpi.index()],
            Some(Value::Dpi(Dpi::new(1600).unwrap()))
        );
        assert_eq!(
            Document::parse("schema = 1\n")
                .unwrap()
                .check()
                .problems
                .len(),
            12
        );
    }

    #[test]
    fn unknown_keys_are_errors() {
        let text = FULL
            .replacen("[mouse]\n", "[mouse]\ncolor = 1\n", 1)
            .replacen("[buttons]", "extra = true\n\n[buttons]", 1)
            .replacen(
                "radial  = \"host:1\"",
                "radial  = \"host:1\"\nthumb = \"host:2\"",
                1,
            )
            + "\n[profile]\nname = \"x\"\n";
        assert_eq!(
            problems(&text),
            [
                "unknown key profile",
                "unknown key mouse.color",
                "unknown key mouse.lift.extra",
                "unknown key buttons.thumb",
            ]
        );
        assert!(matches!(
            Document::parse(&text).unwrap().config(),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn missing_and_invalid_together_are_invalid() {
        let text = FULL.replacen("dpi = 1600", "dpi = 1601", 1).replacen(
            "wheel = \"normal\"        # \"normal\" | \"inertial\"\n",
            "",
            1,
        );
        let err = Document::parse(&text).unwrap().config().unwrap_err();
        assert_eq!(
            err.problems()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            [
                "mouse.dpi: dpi must be a multiple of 50",
                "mouse.wheel is missing"
            ]
        );
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn value_and_type_errors() {
        for (from, to, expected) in [
            (
                "dpi = 1600",
                "dpi = 8250",
                "mouse.dpi: dpi must be in range 50..8200",
            ),
            (
                "dpi = 1600",
                "dpi = 0",
                "mouse.dpi: dpi must be in range 50..8200",
            ),
            (
                "dpi = 1600",
                "dpi = 1625",
                "mouse.dpi: dpi must be a multiple of 50",
            ),
            (
                "dpi = 1600",
                "dpi = 1600.0",
                "mouse.dpi must be integer, found float",
            ),
            (
                "dpi = 1600",
                "dpi = \"1600\"",
                "mouse.dpi must be integer, found string",
            ),
            (
                "polling_rate = 1000",
                "polling_rate = 2000",
                "mouse.polling_rate: polling_rate must be 125, 250, 500 or 1000",
            ),
            (
                "wheel = \"normal\"",
                "wheel = \"NORMAL\"",
                "mouse.wheel: wheel must be \"normal\" or \"inertial\"",
            ),
            (
                "enabled = false",
                "enabled = 0",
                "mouse.lift.enabled must be boolean, found integer",
            ),
            (
                "enabled = false",
                "enabled = \"false\"",
                "mouse.lift.enabled must be boolean, found string",
            ),
            (
                "threshold = 31",
                "threshold = 256",
                "mouse.lift.threshold: threshold must be in range 0..255",
            ),
            (
                "threshold = 31",
                "threshold = -1",
                "mouse.lift.threshold: threshold must be in range 0..255",
            ),
            (
                "radial  = \"host:1\"",
                "radial  = \"host:216\"",
                "buttons.radial: host index must be in range 0..215",
            ),
            (
                "radial  = \"host:1\"",
                "radial  = \"raw:0x29\"",
                "buttons.radial: this raw value has a named form; write host:1",
            ),
            (
                "radial  = \"host:1\"",
                "radial  = \"Host:1\"",
                "buttons.radial: expected mouse:left|right|middle|backward|forward, unknown:6, host:<0..215> or raw:<0x10..0x27>",
            ),
            (
                "radial  = \"host:1\"",
                "radial  = 41",
                "buttons.radial must be string, found integer",
            ),
        ] {
            let text = FULL.replacen(from, to, 1);
            assert_ne!(text, FULL, "{from}");
            assert_eq!(problems(&text), [expected], "{to}");
        }
    }

    #[test]
    fn integers_accept_toml_notations() {
        let text = FULL
            .replacen("dpi = 1600", "dpi = 0x640", 1)
            .replacen("threshold = 31", "threshold = 0x1f", 1)
            .replacen("polling_rate = 1000", "polling_rate = 1_000", 1);
        let config = Document::parse(&text).unwrap().config().unwrap();
        assert_eq!(config.dpi.get(), 1600);
        assert_eq!(config.lift_threshold, 31);
        assert_eq!(config.polling_rate, PollingRate::Hz1000);
    }

    #[test]
    fn tables_in_other_forms() {
        let dotted = "schema = 1\nmouse.dpi = 1600\nmouse.polling_rate = 1000\nmouse.wheel = \"normal\"\n\
            mouse.lift = { enabled = false, threshold = 31 }\n\
            buttons = { left = \"mouse:left\", right = \"mouse:right\", middle = \"mouse:middle\", wheel = \"mouse:middle\", forward = \"mouse:forward\", back = \"mouse:backward\", radial = \"host:1\" }\n";
        let doc = Document::parse(dotted).unwrap();
        assert_eq!(
            doc.config().unwrap(),
            Document::parse(FULL).unwrap().config().unwrap()
        );

        assert_eq!(
            problems("schema = 1\nmouse = 1\n[buttons]\n"),
            [
                "mouse must be a table",
                "buttons.left is missing",
                "buttons.right is missing",
                "buttons.middle is missing",
                "buttons.wheel is missing",
                "buttons.forward is missing",
                "buttons.back is missing",
                "buttons.radial is missing",
            ]
        );
    }

    fn updated(text: &str, args: &[&str]) -> String {
        let mut doc = Document::parse(text).unwrap();
        let config = doc.config().unwrap();
        let (_, changes) = config.apply(&parse_assignments(args).unwrap()).unwrap();
        doc.update(&changes).unwrap();
        doc.to_string()
    }

    #[test]
    fn update_keeps_everything_else() {
        let out = updated(
            FULL,
            &[
                "mouse.dpi=1000",
                "buttons.radial=mouse:middle",
                "mouse.lift.enabled=true",
            ],
        );
        let expected = FULL
            .replacen("dpi = 1600              #", "dpi = 1000              #", 1)
            .replacen("radial  = \"host:1\"", "radial  = \"mouse:middle\"", 1)
            .replacen("enabled = false", "enabled = true", 1);
        assert_eq!(out, expected);
        assert_eq!(
            Document::parse(&out).unwrap().config().unwrap().dpi.get(),
            1000
        );
    }

    #[test]
    fn unchanged_values_are_not_rewritten() {
        let text = FULL.replacen("dpi = 1600", "dpi = 1_600", 1);
        assert_eq!(updated(&text, &["mouse.dpi=1600"]), text);
        assert_eq!(updated(FULL, &[]), FULL);
    }

    #[test]
    fn hex_stays_hex() {
        let text = FULL.replacen("threshold = 31", "threshold = 0x1f", 1);
        let out = updated(&text, &["mouse.lift.threshold=40"]);
        assert_eq!(
            out,
            text.replacen("threshold = 0x1f", "threshold = 0x28", 1)
        );
        let out = updated(FULL, &["mouse.lift.threshold=0x28"]);
        assert_eq!(out, FULL.replacen("threshold = 31", "threshold = 40", 1));
    }

    #[test]
    fn update_inline_and_dotted_tables() {
        let text = "schema = 1\nmouse.dpi = 1600 # c\nmouse.polling_rate = 1000\nmouse.wheel = \"normal\"\n\
            mouse.lift = { enabled = false, threshold = 31 }\n\
            buttons = { left = \"mouse:left\", right = \"mouse:right\", middle = \"mouse:middle\", wheel = \"mouse:middle\", forward = \"mouse:forward\", back = \"mouse:backward\", radial = \"host:1\" }\n";
        let out = updated(
            text,
            &[
                "mouse.dpi=800",
                "mouse.lift.threshold=5",
                "buttons.left=host:2",
            ],
        );
        let expected = text
            .replacen("mouse.dpi = 1600 # c", "mouse.dpi = 800 # c", 1)
            .replacen("threshold = 31", "threshold = 5", 1)
            .replacen("left = \"mouse:left\"", "left = \"host:2\"", 1);
        assert_eq!(out, expected);
    }

    #[test]
    fn update_of_missing_key_fails() {
        let mut doc = Document::parse("schema = 1\n").unwrap();
        let change = Change {
            key: Key::Dpi,
            from: Value::Dpi(Dpi::new(1600).unwrap()),
            to: Value::Dpi(Dpi::new(800).unwrap()),
        };
        assert_eq!(doc.update(&[change]), Err(UpdateError(Key::Dpi)));
    }
}
