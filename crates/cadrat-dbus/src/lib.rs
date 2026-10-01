//! The D-Bus contract between `cadratd` and `cadratctl` (spec daemon/dbus.md):
//! names, the argument dictionaries of each method, and the errors.
//!
//! Both programs use this crate, so a key or an error name cannot differ
//! between the two sides. The interface is not stable in Phase 2a
//! (spec dbus §1): only `cadratctl` of the same version calls it.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use cadrat_cli::{Command, Exit, Failure, Options};
use cadrat_proto::{Polling, Route, Slot};
use zbus::message::{Header, Message};
use zbus::names::ErrorName;
use zbus::zvariant::{OwnedValue, Value};

/// The well-known name on the session bus.
pub const BUS_NAME: &str = "cc.nejiman10.Cadrat1";
/// The object path.
pub const PATH: &str = "/cc/nejiman10/Cadrat1";
/// The interface.
pub const INTERFACE: &str = "cc.nejiman10.Cadrat1.Manager";
/// The prefix of this interface's own error names.
pub const ERROR_PREFIX: &str = "cc.nejiman10.Cadrat1.Error.";

/// The extra time a caller waits for `ReceiverPair` and `ReceiverUnpair`
/// beyond the requested timeout (spec dbus §6, ctl/cli §5).
pub const CALL_MARGIN: Duration = Duration::from_secs(30);
/// How long a caller waits for the other methods (spec ctl/cli §5).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Which bus `cadratd` serves on and `cadratctl` calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The user's session bus.
    Session,
    /// A bus at this address (tests).
    Address(String),
}

/// The methods (spec dbus §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `list`.
    List,
    /// `init`.
    Init,
    /// `get`.
    Get,
    /// `check`.
    Check,
    /// `set`.
    Set,
    /// `apply`.
    Apply,
    /// `receiver slots`.
    ReceiverSlots,
    /// `receiver pair`.
    ReceiverPair,
    /// `receiver unpair`.
    ReceiverUnpair,
    /// Stops a running `ReceiverPair`.
    Cancel,
}

/// How a request uses devices (the "デバイス" column of spec dbus §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceUse {
    /// Does not touch devices.
    None,
    /// Reads only.
    Read,
    /// Writes; one at a time.
    Write,
}

impl Method {
    /// The D-Bus member name.
    #[must_use]
    pub fn member(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Init => "Init",
            Self::Get => "Get",
            Self::Check => "Check",
            Self::Set => "Set",
            Self::Apply => "Apply",
            Self::ReceiverSlots => "ReceiverSlots",
            Self::ReceiverPair => "ReceiverPair",
            Self::ReceiverUnpair => "ReceiverUnpair",
            Self::Cancel => "Cancel",
        }
    }

    /// The method of a command.
    #[must_use]
    pub fn of(command: &Command) -> Self {
        match command {
            Command::List { .. } => Self::List,
            Command::Init { .. } => Self::Init,
            Command::Get { .. } => Self::Get,
            Command::Check => Self::Check,
            Command::Set { .. } => Self::Set,
            Command::Apply { .. } => Self::Apply,
            Command::ReceiverSlots { .. } => Self::ReceiverSlots,
            Command::ReceiverPair { .. } => Self::ReceiverPair,
            Command::ReceiverUnpair { .. } => Self::ReceiverUnpair,
        }
    }

    /// The JSON `command` of the result (`cancel` for [`Method::Cancel`]).
    #[must_use]
    pub fn command_name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Init => "init",
            Self::Get => "get",
            Self::Check => "check",
            Self::Set => "set",
            Self::Apply => "apply",
            Self::ReceiverSlots => "receiver slots",
            Self::ReceiverPair => "receiver pair",
            Self::ReceiverUnpair => "receiver unpair",
            Self::Cancel => "cancel",
        }
    }

    /// The keys this method accepts (spec dbus §3, §6).
    #[must_use]
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::List => &["verbose", "redact", "nodes"],
            Self::Init => &["config", "preset", "force"],
            Self::Get => &["config", "keys", "wire"],
            Self::Check => &["config"],
            Self::Set => &[
                "config",
                "mouse",
                "route",
                "verbose",
                "assignments",
                "dry_run",
                "no_save",
            ],
            Self::Apply => &["config", "mouse", "route", "verbose", "dry_run"],
            Self::ReceiverSlots => &["receiver", "redact", "verbose"],
            Self::ReceiverPair => &["receiver", "verbose", "timeout_ms", "poll_interval_ms"],
            Self::ReceiverUnpair => &[
                "receiver",
                "verbose",
                "slot",
                "expected",
                "timeout_ms",
                "poll_interval_ms",
            ],
            Self::Cancel => &[],
        }
    }
}

/// How `command` uses devices.
#[must_use]
pub fn device_use(command: &Command) -> DeviceUse {
    match command {
        Command::Init { .. }
        | Command::Get { .. }
        | Command::Check
        | Command::Set { dry_run: true, .. }
        | Command::Apply { dry_run: true } => DeviceUse::None,
        Command::Set { .. }
        | Command::Apply { .. }
        | Command::ReceiverPair { .. }
        | Command::ReceiverUnpair { .. } => DeviceUse::Write,
        Command::List { .. } | Command::ReceiverSlots { .. } => DeviceUse::Read,
    }
}

/// The argument dictionary of a request.
pub type Args = HashMap<String, OwnedValue>;

/// The argument dictionary for `command` (the caller's side).
///
/// `options.config` must already be absolute (spec dbus §3) and UTF-8:
/// `cadratctl` refuses other paths, so the lossy conversion never applies. `-q` and
/// `--json` only change the output and are not sent.
#[must_use]
pub fn encode(options: &Options, command: &Command) -> HashMap<&'static str, Value<'static>> {
    let mut args: HashMap<&'static str, Value<'static>> = HashMap::new();
    let method = Method::of(command);
    let keys = method.keys();
    let mut put = |key: &'static str, value: Value<'static>| {
        if keys.contains(&key) {
            args.insert(key, value);
        }
    };
    if let Some(config) = &options.config {
        put("config", Value::from(config.to_string_lossy().into_owned()));
    }
    if let Some(mouse) = &options.mouse {
        put("mouse", Value::from(mouse.clone()));
    }
    if let Some(route) = options.route {
        put("route", Value::from(route.to_string()));
    }
    if options.verbose {
        put("verbose", Value::from(true));
    }
    match command {
        Command::List { nodes, redact } => {
            put("nodes", Value::from(*nodes));
            put("redact", Value::from(*redact));
        }
        Command::Init { preset, force } => {
            if *preset == cadrat_config::Preset::ResearchBaseline {
                put("preset", Value::from("research-baseline"));
            }
            put("force", Value::from(*force));
        }
        Command::Get { keys, wire, .. } => {
            put("keys", Value::from(keys.clone()));
            put("wire", Value::from(*wire));
        }
        Command::Check => {}
        Command::Set {
            assignments,
            dry_run,
            no_save,
        } => {
            put("assignments", Value::from(assignments.clone()));
            put("dry_run", Value::from(*dry_run));
            put("no_save", Value::from(*no_save));
        }
        Command::Apply { dry_run } => put("dry_run", Value::from(*dry_run)),
        Command::ReceiverSlots { receiver, redact } => {
            if let Some(receiver) = receiver {
                put("receiver", Value::from(receiver.clone()));
            }
            put("redact", Value::from(*redact));
        }
        Command::ReceiverPair { receiver, polling } => {
            if let Some(receiver) = receiver {
                put("receiver", Value::from(receiver.clone()));
            }
            put_polling(&mut put, *polling);
        }
        Command::ReceiverUnpair {
            receiver,
            slot,
            expected,
            polling,
            ..
        } => {
            if let Some(receiver) = receiver {
                put("receiver", Value::from(receiver.clone()));
            }
            put("slot", Value::from(slot.get()));
            if let Some(expected) = expected {
                put("expected", Value::from(expected.to_vec()));
            }
            put_polling(&mut put, *polling);
        }
    }
    args
}

fn put_polling(put: &mut impl FnMut(&'static str, Value<'static>), polling: Polling) {
    put("timeout_ms", Value::from(millis(polling.timeout)));
    put("poll_interval_ms", Value::from(millis(polling.interval)));
}

fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// The longest timeout `cadrat-tool` accepts, in milliseconds (one day).
const MAX_MS: u32 = 86_400_000;

/// A request as `cadratd` runs it.
#[derive(Debug, Clone)]
pub struct Request {
    /// The common options.
    pub options: Options,
    /// The command.
    pub command: Command,
}

/// Reads a request (`cadratd`'s side).
///
/// The outer error is a D-Bus `InvalidArgs` (unknown key, wrong type,
/// missing key); the inner one is a value `cadrat-tool` would reject as a
/// usage error, returned as the command's JSON (spec dbus §2).
///
/// # Errors
///
/// [`Error::InvalidArgs`] as described.
#[allow(clippy::too_many_lines)]
pub fn decode(method: Method, args: &Args) -> Result<Result<Request, Failure>, Error> {
    let mut reader = Reader { method, args };
    reader.check_keys()?;
    let mut options = Options {
        verbose_json: reader.bool("verbose")?,
        ..Options::default()
    };
    if let Some(config) = reader.string("config")? {
        let config = PathBuf::from(config);
        if !config.is_absolute() {
            return Err(Error::InvalidArgs(
                "config must be an absolute path".to_owned(),
            ));
        }
        options.config = Some(config);
    }
    options.mouse = reader.string("mouse")?;
    let route = reader.string("route")?;
    let receiver = reader.string("receiver")?;
    let command = match method {
        Method::List => Command::List {
            nodes: reader.bool("nodes")?,
            redact: reader.bool("redact")?,
        },
        Method::Init => {
            let preset = match reader.string("preset")?.as_deref() {
                None => cadrat_config::Preset::Empty,
                Some("research-baseline") => cadrat_config::Preset::ResearchBaseline,
                Some(other) => return Ok(Err(usage(format!("unknown preset {other:?}")))),
            };
            Command::Init {
                preset,
                force: reader.bool("force")?,
            }
        }
        Method::Get => Command::Get {
            keys: reader.strings("keys")?.unwrap_or_default(),
            wire: reader.bool("wire")?,
            values_only: false,
        },
        Method::Check => Command::Check,
        Method::Set => {
            let Some(assignments) = reader.strings("assignments")? else {
                return Err(Error::InvalidArgs("assignments is required".to_owned()));
            };
            let (dry_run, no_save) = (reader.bool("dry_run")?, reader.bool("no_save")?);
            if dry_run && no_save {
                return Ok(Err(usage("dry_run and no_save cannot both be true")));
            }
            if assignments.is_empty() {
                return Ok(Err(usage("set needs at least one key=value")));
            }
            Command::Set {
                assignments,
                dry_run,
                no_save,
            }
        }
        Method::Apply => Command::Apply {
            dry_run: reader.bool("dry_run")?,
        },
        Method::ReceiverSlots => Command::ReceiverSlots {
            receiver,
            redact: reader.bool("redact")?,
        },
        Method::ReceiverPair => {
            let polling = match reader.polling(Duration::from_secs(60), Duration::from_secs(1))? {
                Ok(polling) => polling,
                Err(failure) => return Ok(Err(failure)),
            };
            Command::ReceiverPair { receiver, polling }
        }
        Method::ReceiverUnpair => {
            let Some(slot) = reader.byte("slot")? else {
                return Err(Error::InvalidArgs("slot is required".to_owned()));
            };
            let Some(expected) = reader.bytes("expected")? else {
                return Err(Error::InvalidArgs("expected is required".to_owned()));
            };
            let polling =
                match reader.polling(Duration::from_secs(15), Duration::from_millis(500))? {
                    Ok(polling) => polling,
                    Err(failure) => return Ok(Err(failure)),
                };
            let Some(slot) = Slot::new(slot) else {
                return Ok(Err(usage(format!("slot {slot} is not in 0..4"))));
            };
            let Ok(expected) = <[u8; 8]>::try_from(expected.as_slice()) else {
                return Ok(Err(usage(
                    "expected must be the 8 bytes of a slot response",
                )));
            };
            Command::ReceiverUnpair {
                receiver,
                slot,
                yes: true,
                expected: Some(expected),
                polling,
            }
        }
        Method::Cancel => {
            return Err(Error::Internal("Cancel is not a command".to_owned()));
        }
    };
    if let Some(route) = route {
        options.route = Some(match route.as_str() {
            "wired" => Route::Wired,
            "receiver" => Route::Receiver,
            other => {
                return Ok(Err(usage(format!(
                    "route must be wired or receiver, not {other:?}"
                ))));
            }
        });
    }
    Ok(Ok(Request { options, command }))
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::new(Exit::Usage, message)
}

/// Checks that a `Cancel` request carries no keys.
///
/// # Errors
///
/// [`Error::InvalidArgs`] for any key.
pub fn decode_cancel(args: &Args) -> Result<(), Error> {
    Reader {
        method: Method::Cancel,
        args,
    }
    .check_keys()
}

struct Reader<'a> {
    method: Method,
    args: &'a Args,
}

impl Reader<'_> {
    fn check_keys(&mut self) -> Result<(), Error> {
        let mut keys: Vec<&String> = self.args.keys().collect();
        keys.sort();
        match keys
            .into_iter()
            .find(|key| !self.method.keys().contains(&key.as_str()))
        {
            Some(key) => Err(Error::InvalidArgs(format!(
                "{} does not take {key:?}",
                self.method.member()
            ))),
            None => Ok(()),
        }
    }

    fn get(&self, key: &str) -> Option<&Value<'static>> {
        self.args.get(key).map(|v| {
            let value: &Value<'static> = v;
            value
        })
    }

    fn wrong_type(key: &str, expected: &str) -> Error {
        Error::InvalidArgs(format!("{key} must have the type {expected}"))
    }

    fn bool(&self, key: &str) -> Result<bool, Error> {
        match self.get(key) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(Self::wrong_type(key, "b")),
        }
    }

    fn string(&self, key: &str) -> Result<Option<String>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Str(s)) => Ok(Some(s.as_str().to_owned())),
            Some(_) => Err(Self::wrong_type(key, "s")),
        }
    }

    fn byte(&self, key: &str) -> Result<Option<u8>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::U8(b)) => Ok(Some(*b)),
            Some(_) => Err(Self::wrong_type(key, "y")),
        }
    }

    fn u32(&self, key: &str) -> Result<Option<u32>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::U32(n)) => Ok(Some(*n)),
            Some(_) => Err(Self::wrong_type(key, "u")),
        }
    }

    fn strings(&self, key: &str) -> Result<Option<Vec<String>>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Array(array)) if array.element_signature().to_string() == "s" => array
                .inner()
                .iter()
                .map(|item| match item {
                    Value::Str(s) => Ok(s.as_str().to_owned()),
                    _ => Err(Self::wrong_type(key, "as")),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(Self::wrong_type(key, "as")),
        }
    }

    fn bytes(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Array(array)) if array.element_signature().to_string() == "y" => array
                .inner()
                .iter()
                .map(|item| match item {
                    Value::U8(b) => Ok(*b),
                    _ => Err(Self::wrong_type(key, "ay")),
                })
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(Self::wrong_type(key, "ay")),
        }
    }

    /// `timeout_ms` and `poll_interval_ms`, with `cadrat-tool`'s defaults.
    fn polling(
        &self,
        timeout: Duration,
        interval: Duration,
    ) -> Result<Result<Polling, Failure>, Error> {
        let mut polling = Polling { timeout, interval };
        for (key, field) in [
            ("timeout_ms", &mut polling.timeout),
            ("poll_interval_ms", &mut polling.interval),
        ] {
            if let Some(ms) = self.u32(key)? {
                if ms == 0 || ms > MAX_MS {
                    return Ok(Err(usage(format!("{key} must be between 1 and {MAX_MS}"))));
                }
                *field = Duration::from_millis(u64::from(ms));
            }
        }
        Ok(Ok(polling))
    }
}

/// The D-Bus errors (spec dbus §5). Every other failure is a command result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Another device-writing request is running (its command name in the
    /// message). Nothing was done.
    Busy(String),
    /// `cadratd` is still starting. Nothing was done.
    Starting(String),
    /// The argument dictionary does not match the method.
    InvalidArgs(String),
    /// The caller is another user.
    AccessDenied(String),
    /// Not even the result JSON could be made.
    Internal(String),
}

impl Error {
    /// The D-Bus error name.
    #[must_use]
    pub fn error_name(&self) -> &'static str {
        match self {
            Self::Busy(_) => "cc.nejiman10.Cadrat1.Error.Busy",
            Self::Starting(_) => "cc.nejiman10.Cadrat1.Error.Starting",
            Self::InvalidArgs(_) => "org.freedesktop.DBus.Error.InvalidArgs",
            Self::AccessDenied(_) => "org.freedesktop.DBus.Error.AccessDenied",
            Self::Internal(_) => "cc.nejiman10.Cadrat1.Error.Internal",
        }
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Busy(m)
            | Self::Starting(m)
            | Self::InvalidArgs(m)
            | Self::AccessDenied(m)
            | Self::Internal(m) => m,
        }
    }

    /// The error of a failed call, if it is one of these.
    #[must_use]
    pub fn from_reply(name: &str, message: Option<&str>) -> Option<Self> {
        let message = message.unwrap_or_default().to_owned();
        Some(match name {
            "cc.nejiman10.Cadrat1.Error.Busy" => Self::Busy(message),
            "cc.nejiman10.Cadrat1.Error.Starting" => Self::Starting(message),
            "org.freedesktop.DBus.Error.InvalidArgs" => Self::InvalidArgs(message),
            "org.freedesktop.DBus.Error.AccessDenied" => Self::AccessDenied(message),
            "cc.nejiman10.Cadrat1.Error.Internal" => Self::Internal(message),
            _ => return None,
        })
    }

    /// `cadratctl`'s exit code for this error (spec dbus §5).
    #[must_use]
    pub fn exit(&self) -> Exit {
        match self {
            Self::Busy(_) | Self::Starting(_) => Exit::Busy,
            Self::InvalidArgs(_) => Exit::Usage,
            Self::AccessDenied(_) => Exit::DaemonUnavailable,
            Self::Internal(_) => Exit::Internal,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.error_name(), self.message())
    }
}

impl std::error::Error for Error {}

impl zbus::DBusError for Error {
    fn create_reply(&self, call: &Header<'_>) -> zbus::Result<Message> {
        Message::error(call, self.error_name())?.build(&(self.message(),))
    }

    fn name(&self) -> ErrorName<'_> {
        ErrorName::from_static_str_unchecked(self.error_name())
    }

    fn description(&self) -> Option<&str> {
        Some(self.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(pairs: &[(&str, Value<'static>)]) -> Args {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_owned(),
                    OwnedValue::try_from(v.try_clone().unwrap()).unwrap(),
                )
            })
            .collect()
    }

    fn round_trip(options: &Options, command: &Command) -> Request {
        let encoded: Args = encode(options, command)
            .into_iter()
            .map(|(k, v)| (k.to_owned(), OwnedValue::try_from(v).unwrap()))
            .collect();
        decode(Method::of(command), &encoded).unwrap().unwrap()
    }

    #[test]
    fn requests_survive_the_bus() {
        let options = Options {
            config: Some(PathBuf::from("/home/u/c.toml")),
            mouse: Some("1".to_owned()),
            route: Some(Route::Receiver),
            verbose: true,
            ..Options::default()
        };
        let set = Command::Set {
            assignments: vec!["mouse.dpi=800".to_owned()],
            dry_run: false,
            no_save: true,
        };
        let request = round_trip(&options, &set);
        assert_eq!(request.options.config, options.config);
        assert_eq!(request.options.mouse.as_deref(), Some("1"));
        assert_eq!(request.options.route, Some(Route::Receiver));
        assert!(request.options.verbose_json);
        assert!(
            matches!(request.command, Command::Set { no_save: true, dry_run: false, ref assignments } if assignments == &["mouse.dpi=800"])
        );

        let unpair = Command::ReceiverUnpair {
            receiver: Some("recv:port-1-4".to_owned()),
            slot: Slot::new(3).unwrap(),
            yes: false,
            expected: Some([0x46, 0x59, 1, 2, 3, 4, 5, 6]),
            polling: Polling {
                timeout: Duration::from_secs(15),
                interval: Duration::from_millis(500),
            },
        };
        let request = round_trip(&Options::default(), &unpair);
        let Command::ReceiverUnpair {
            receiver,
            slot,
            yes,
            expected,
            polling,
        } = request.command
        else {
            panic!("{:?}", request.command);
        };
        assert_eq!(receiver.as_deref(), Some("recv:port-1-4"));
        assert_eq!(slot.get(), 3);
        assert!(yes);
        assert_eq!(expected, Some([0x46, 0x59, 1, 2, 3, 4, 5, 6]));
        assert_eq!(polling.interval, Duration::from_millis(500));
    }

    #[test]
    fn keys_and_types_are_checked() {
        let invalid = |method, pairs: &[(&str, Value<'static>)]| {
            matches!(decode(method, &args(pairs)), Err(Error::InvalidArgs(_)))
        };
        assert!(invalid(Method::List, &[("mouse", Value::from("1"))]));
        assert!(invalid(Method::List, &[("verbose", Value::from("yes"))]));
        assert!(invalid(Method::Set, &[]));
        assert!(invalid(
            Method::Check,
            &[("config", Value::from("rel.toml"))]
        ));
        assert!(invalid(
            Method::ReceiverUnpair,
            &[
                ("slot", Value::from(3u32)),
                ("expected", Value::from(vec![0u8; 8]))
            ]
        ));
        assert!(decode_cancel(&args(&[("x", Value::from(true))])).is_err());
    }

    #[test]
    fn wrong_values_are_usage_results() {
        let usage = |method, pairs: &[(&str, Value<'static>)]| {
            matches!(
                decode(method, &args(pairs)),
                Ok(Err(Failure {
                    exit: Exit::Usage,
                    ..
                }))
            )
        };
        assert!(usage(Method::Apply, &[("route", Value::from("bluetooth"))]));
        assert!(usage(
            Method::ReceiverUnpair,
            &[
                ("slot", Value::from(5u8)),
                ("expected", Value::from(vec![0u8; 8]))
            ]
        ));
        assert!(usage(
            Method::ReceiverUnpair,
            &[
                ("slot", Value::from(1u8)),
                ("expected", Value::from(vec![0u8; 7]))
            ]
        ));
        assert!(usage(
            Method::Set,
            &[
                ("assignments", Value::from(vec!["mouse.dpi=800".to_owned()])),
                ("dry_run", Value::from(true)),
                ("no_save", Value::from(true)),
            ]
        ));
        assert!(usage(
            Method::ReceiverPair,
            &[("timeout_ms", Value::from(0u32))]
        ));
    }

    #[test]
    fn errors_map_to_exit_codes() {
        let busy = Error::Busy("receiver pair".to_owned());
        assert_eq!(
            Error::from_reply(busy.error_name(), Some("receiver pair")),
            Some(busy.clone())
        );
        assert_eq!(busy.exit().code(), 22);
        assert_eq!(Error::Starting(String::new()).exit().code(), 22);
        assert_eq!(Error::InvalidArgs(String::new()).exit().code(), 2);
        assert_eq!(Error::AccessDenied(String::new()).exit().code(), 21);
        assert_eq!(Error::Internal(String::new()).exit().code(), 1);
        assert_eq!(
            Error::from_reply("org.freedesktop.DBus.Error.UnknownMethod", None),
            None
        );
    }
}
