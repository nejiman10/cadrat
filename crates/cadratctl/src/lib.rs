//! `cadratctl`: the command-line front end of `cadratd` (spec ctl/cli.md).
//!
//! It parses the same commands as `cadrat-tool`, sends them to `cadratd`
//! over D-Bus and prints the returned JSON exactly as `cadrat-tool` prints
//! its own result (`cadrat_command::render`). It never opens hidraw.
//! [`run`] takes the arguments, the outside world ([`Env`]) and the standard
//! streams ([`Io`]) so the D-Bus tests run it against a private bus.

// `expect` is used only where clap already guarantees the value.
#![allow(clippy::missing_panics_doc)]

use std::any::Any;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use cadrat_command::cli::{Common, Shared, VERSION};
use cadrat_command::{Command, Exit, Failure, Options, envelope, render};
pub use cadrat_dbus::Bus;
use cadrat_dbus::{BUS_NAME, CALL_MARGIN, CALL_TIMEOUT, INTERFACE, PATH, encode};
use clap::Parser;
use futures_lite::{StreamExt, future};
use serde_json::{Map, Value};
use zbus::proxy::CacheProperties;
use zbus::zvariant::Value as Variant;

const PROGRAM: &str = "cadratctl";
const RESTART: &str = "run `systemctl --user restart cadratd.service`";

/// Configure the C658 mouse and the C652 Receiver through cadratd.
#[derive(Debug, Parser)]
#[command(name = "cadratctl", version = VERSION, about)]
struct Cli {
    #[command(flatten)]
    common: Common,
    #[command(subcommand)]
    command: Shared,
}

/// The command-line definition, for generating the manual page and shell
/// completions.
#[must_use]
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// SIGINT and SIGTERM while `receiver pair` waits (spec ctl/cli §2).
pub trait Signals {
    /// Calls `on_signal`, from any thread, for each SIGINT or SIGTERM until
    /// the returned guard is dropped.
    fn catch(&self, on_signal: Box<dyn Fn() + Send + Sync>) -> Box<dyn Any>;
}

/// The outside world.
pub struct Env<'a> {
    /// Where `cadratd` is.
    pub bus: Bus,
    /// `XDG_CONFIG_HOME`, for the default configuration path.
    pub xdg_config_home: Option<OsString>,
    /// `HOME`.
    pub home: Option<OsString>,
    /// The working directory, to make `--config` absolute.
    pub cwd: PathBuf,
    /// Signals.
    pub signals: &'a dyn Signals,
}

/// Standard streams.
pub struct Io<'a> {
    /// Results.
    pub stdout: &'a mut dyn Write,
    /// Warnings, errors and prompts.
    pub stderr: &'a mut dyn Write,
    /// Answers to the unpair confirmation.
    pub stdin: &'a mut dyn BufRead,
    /// Whether stdin is a terminal.
    pub stdin_is_terminal: bool,
}

/// Runs `cadratctl` and returns the exit code.
// Output is best effort, as in cadrat-tool.
pub fn run(args: impl IntoIterator<Item = OsString>, env: &Env, io: Io) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            use clap::error::ErrorKind;
            let usage = !matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let rendered = error.render().to_string();
            let _ = if usage {
                write!(io.stderr, "{rendered}")
            } else {
                write!(io.stdout, "{rendered}")
            };
            if !usage {
                return Exit::Success.code();
            }
            if args.iter().any(|a| a == "--json") {
                let failure = Failure::new(Exit::Usage, error.kind().to_string());
                let json = envelope(None, Map::new(), Vec::new(), &Err(failure));
                let _ = writeln!(io.stdout, "{json}");
            }
            return Exit::Usage.code();
        }
    };
    let mut ctl = Ctl {
        env,
        options: cli.common.options(),
        command: cli.command.request(),
        json: cli.common.json,
        io,
        shown_warnings: Vec::new(),
        daemon_version: None,
    };
    let result = zbus::block_on(ctl.run());
    ctl.finish(&result)
}

struct Ctl<'e, 'i> {
    env: &'e Env<'e>,
    options: Options,
    command: Command,
    json: bool,
    io: Io<'i>,
    /// Warning lines already printed (unpair prints those of its slot read
    /// before asking).
    shown_warnings: Vec<String>,
    /// `cadratd`'s version when it differs from ours (spec ctl/cli §4.1).
    daemon_version: Option<String>,
}

impl Ctl<'_, '_> {
    async fn run(&mut self) -> Value {
        let bus_options = match self.check_arguments() {
            Ok(options) => options,
            Err(failure) => return self.failed(failure),
        };
        let proxy = match self.connect().await {
            Ok(proxy) => proxy,
            Err(failure) => return self.failed(failure),
        };
        let command = self.command.clone();
        match &command {
            Command::ReceiverPair { polling, .. } => {
                self.pair(&proxy, &bus_options, &command, polling.timeout)
                    .await
            }
            Command::ReceiverUnpair { .. } => self.unpair(&proxy, &bus_options, &command).await,
            _ => self.call(&proxy, &bus_options, &command).await,
        }
    }

    /// What `cadratctl` checks before calling (spec ctl/cli §1): the same
    /// usage errors as `cadrat-tool`, and the configuration path made
    /// absolute. Returns the options to send.
    fn check_arguments(&self) -> Result<Options, Failure> {
        let mut options = self.options.clone();
        match &self.command {
            Command::ReceiverSlots { .. } | Command::ReceiverPair { .. } => {
                cadrat_command::no_mouse(&options)?;
            }
            Command::ReceiverUnpair { yes, .. } => {
                let can_confirm = !self.json && self.io.stdin_is_terminal;
                cadrat_command::unpair_arguments(&options, *yes, can_confirm)?;
            }
            Command::Set { assignments, .. } => {
                cadrat_config::parse_assignments(assignments)
                    .map_err(|e| Failure::new(Exit::Usage, e.to_string()))?;
            }
            _ => {}
        }
        let uses_config = matches!(
            self.command,
            Command::Init { .. }
                | Command::Get { .. }
                | Command::Check
                | Command::Set { .. }
                | Command::Apply { .. }
        );
        if uses_config {
            let path = match &options.config {
                Some(path) => path.clone(),
                None => cadrat_config::file::default_path(
                    self.env.xdg_config_home.as_deref(),
                    self.env.home.as_deref(),
                )
                .ok_or_else(|| {
                    Failure::new(
                        Exit::ConfigError,
                        "cannot find the configuration directory: set HOME or use --config",
                    )
                })?,
            };
            options.config = Some(self.env.cwd.join(path));
        }
        Ok(options)
    }

    fn call_timeout(&self) -> Duration {
        match &self.command {
            Command::ReceiverPair { polling, .. } | Command::ReceiverUnpair { polling, .. } => {
                polling.timeout + CALL_MARGIN
            }
            _ => CALL_TIMEOUT,
        }
    }

    /// Connects, and warns when `cadratd` is another version.
    async fn connect(&mut self) -> Result<zbus::Proxy<'static>, Failure> {
        let unreachable = |e: zbus::Error| {
            Failure::new(
                Exit::DaemonUnavailable,
                format!("cannot reach cadratd: {e}"),
            )
        };
        let builder = match &self.env.bus {
            Bus::Session => zbus::connection::Builder::session(),
            Bus::Address(address) => zbus::connection::Builder::address(address.as_str()),
        }
        .map_err(unreachable)?;
        let conn = builder
            .method_timeout(self.call_timeout())
            .build()
            .await
            .map_err(unreachable)?;
        let proxy = zbus::proxy::Builder::<zbus::Proxy>::new(&conn)
            .destination(BUS_NAME)
            .and_then(|b| b.path(PATH))
            .and_then(|b| b.interface(INTERFACE))
            .map_err(unreachable)?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(unreachable)?;
        let version: String = proxy
            .get_property("Version")
            .await
            .map_err(|e| self.call_failure(&e))?;
        if version != VERSION {
            let _ = writeln!(
                self.io.stderr,
                "warning: cadratd {version} is running, but cadratctl is {VERSION}; {RESTART}"
            );
            self.daemon_version = Some(version);
        }
        Ok(proxy)
    }

    /// Calls the method of `command` and returns its result.
    async fn call(
        &mut self,
        proxy: &zbus::Proxy<'_>,
        options: &Options,
        command: &Command,
    ) -> Value {
        let method = cadrat_dbus::Method::of(command).member();
        let reply = proxy
            .call_method(method, &(encode(options, command),))
            .await;
        self.reply(command, reply)
    }

    fn reply(&self, command: &Command, reply: zbus::Result<zbus::Message>) -> Value {
        let text = reply.and_then(|message| message.body().deserialize::<String>());
        match text {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                failure_result(
                    command,
                    Failure::new(
                        Exit::Internal,
                        format!("cadratd returned malformed JSON: {e}"),
                    ),
                )
            }),
            Err(error) => failure_result(command, self.call_failure(&error)),
        }
    }

    /// Spec dbus §5, ctl/cli §4.
    fn call_failure(&self, error: &zbus::Error) -> Failure {
        let (name, message) = match error {
            zbus::Error::MethodError(name, message, _) => (
                name.as_str().to_owned(),
                message.clone().unwrap_or_default(),
            ),
            zbus::Error::FDO(fdo) => {
                use zbus::DBusError;
                (
                    fdo.name().as_str().to_owned(),
                    fdo.description().unwrap_or_default().to_owned(),
                )
            }
            zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                return Failure::new(
                    Exit::DaemonUnavailable,
                    format!(
                        "cadratd did not answer within {} s; the request may or may not have \
                         been carried out",
                        self.call_timeout().as_secs()
                    ),
                );
            }
            other => {
                return Failure::new(
                    Exit::DaemonUnavailable,
                    format!("cannot reach cadratd: {other}"),
                );
            }
        };
        let known = cadrat_dbus::Error::from_reply(&name, Some(&message));
        let outdated = self.daemon_version.is_some()
            && matches!(
                (&known, name.as_str()),
                (Some(cadrat_dbus::Error::InvalidArgs(_)), _)
                    | (None, "org.freedesktop.DBus.Error.UnknownMethod")
            );
        if outdated {
            return Failure::new(
                Exit::DaemonUnavailable,
                format!(
                    "cadratd {} did not accept this call ({message})",
                    self.daemon_version.as_deref().unwrap_or_default()
                ),
            )
            .hint(RESTART);
        }
        let absent = [
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "org.freedesktop.DBus.Error.NameHasNoOwner",
            "org.freedesktop.DBus.Error.NoReply",
        ];
        if absent.contains(&name.as_str()) || name.starts_with("org.freedesktop.DBus.Error.Spawn") {
            return Failure::new(
                Exit::DaemonUnavailable,
                format!("cannot reach cadratd: {message}"),
            );
        }
        match known {
            Some(error) => Failure::new(error.exit(), error.message()),
            None => Failure::new(
                Exit::DaemonUnavailable,
                format!("cadratd refused the call: {name}: {message}"),
            ),
        }
    }

    fn failed(&self, failure: Failure) -> Value {
        failure_result(&self.command, failure)
    }

    /// Spec ctl/cli §2 pair: the prompt after `PairingStarted`, and
    /// `Cancel` on SIGINT or SIGTERM.
    async fn pair(
        &mut self,
        proxy: &zbus::Proxy<'_>,
        options: &Options,
        command: &Command,
        timeout: Duration,
    ) -> Value {
        let mut started = match proxy.receive_signal("PairingStarted").await {
            Ok(stream) => stream,
            Err(e) => return self.failed(self.call_failure(&e)),
        };
        let (tx, rx) = async_channel::unbounded::<()>();
        let _signals = self.env.signals.catch(Box::new(move || {
            let _ = tx.try_send(());
        }));
        let body = (encode(options, command),);
        let mut call = std::pin::pin!(proxy.call_method("ReceiverPair", &body));
        let mut prompted = false;
        let mut cancelled = false;
        loop {
            // A signal sent before the reply is handled before it.
            let event = future::or(
                async {
                    match started.next().await {
                        Some(_) => Event::Started,
                        None => future::pending().await,
                    }
                },
                future::or(
                    async {
                        match rx.recv().await {
                            Ok(()) => Event::Interrupted,
                            Err(_) => future::pending().await,
                        }
                    },
                    async { Event::Reply(call.as_mut().await) },
                ),
            )
            .await;
            match event {
                Event::Started => {
                    if !prompted && !self.options.quiet {
                        let _ = writeln!(self.io.stderr, "{}", render::pairing_prompt(timeout));
                    }
                    prompted = true;
                }
                Event::Interrupted => {
                    if !cancelled {
                        cancelled = true;
                        let none: HashMap<&str, Variant<'_>> = HashMap::new();
                        let _ = proxy.call_method("Cancel", &(none,)).await;
                    }
                }
                Event::Reply(reply) => return self.reply(command, reply),
            }
        }
    }

    /// Spec ctl/cli §2 unpair: read the slot, ask, then send it with the
    /// value shown.
    async fn unpair(
        &mut self,
        proxy: &zbus::Proxy<'_>,
        options: &Options,
        command: &Command,
    ) -> Value {
        let Command::ReceiverUnpair {
            receiver,
            slot,
            yes,
            polling,
            ..
        } = command
        else {
            unreachable!("unpair is called for unpair");
        };
        let read = Command::ReceiverSlots {
            receiver: receiver.clone(),
            redact: false,
        };
        let mut slots = self.call(proxy, options, &read).await;
        if slots["ok"] != true {
            // As if unpair itself had failed to read the slots.
            slots["command"] = command.name().into();
            if let Some(fields) = slots.as_object_mut() {
                fields.remove("slots");
            }
            return slots;
        }
        if self.options.verbose {
            for line in render::verbose_lines(&slots) {
                let _ = writeln!(self.io.stderr, "{line}");
            }
        }
        for line in render::warning_lines(&slots) {
            let _ = writeln!(self.io.stderr, "{line}");
            self.shown_warnings.push(line);
        }
        let target = slots["slots"][usize::from(slot.get())].clone();
        let expected = target["raw_hex"].as_str().and_then(raw_bytes);
        let Some(expected) = expected else {
            return self.failed(Failure::new(
                Exit::Internal,
                "cadratd did not return the raw slot response",
            ));
        };
        if target["occupied"] == true {
            for line in render::unpair_target_lines(&target) {
                let _ = writeln!(self.io.stderr, "{line}");
            }
            if !yes && !self.ask(*slot) {
                let mut fields = Map::new();
                fields.insert("receiver".to_owned(), slots["receiver"].clone());
                fields.insert("target".to_owned(), target);
                fields.insert("sent".to_owned(), false.into());
                fields.insert("slots_after".to_owned(), Value::Null);
                let warnings = slots["warnings"].as_array().cloned().unwrap_or_default();
                return envelope(
                    Some(command.name()),
                    fields,
                    warnings,
                    &Err(cadrat_command::not_confirmed()),
                );
            }
        }
        // The Receiver as found, not a prefix (spec dbus §6).
        let unpair = Command::ReceiverUnpair {
            receiver: slots["receiver"]["key"].as_str().map(str::to_owned),
            slot: *slot,
            yes: true,
            expected: Some(expected),
            polling: *polling,
        };
        let options = Options {
            verbose: false,
            ..options.clone()
        };
        self.call(proxy, &options, &unpair).await
    }

    fn ask(&mut self, slot: cadrat_proto::Slot) -> bool {
        let _ = write!(self.io.stderr, "{}", render::unpair_question(slot));
        let _ = self.io.stderr.flush();
        let mut answer = String::new();
        if self.io.stdin.read_line(&mut answer).is_err() {
            return false;
        }
        render::confirms(&answer)
    }

    /// Prints the result as `cadrat-tool` would (spec ctl/cli §3) and
    /// returns the exit code.
    fn finish(&mut self, result: &Value) -> i32 {
        if self.options.verbose {
            for line in render::verbose_lines(result) {
                let _ = writeln!(self.io.stderr, "{line}");
            }
        }
        for line in render::warning_lines(result) {
            if let Some(i) = self.shown_warnings.iter().position(|shown| *shown == line) {
                self.shown_warnings.remove(i);
            } else {
                let _ = writeln!(self.io.stderr, "{line}");
            }
        }
        let mut result = result.clone();
        if !matches!(self.command, Command::List { nodes: true, .. })
            && let Some(fields) = result.as_object_mut()
        {
            fields.remove("nodes");
        }
        let human = render::human(PROGRAM, &self.options, &self.command, &result);
        if self.json {
            let _ = writeln!(self.io.stdout, "{result}");
        } else {
            for line in &human.stdout {
                let _ = writeln!(self.io.stdout, "{line}");
            }
        }
        for line in &human.stderr {
            let _ = writeln!(self.io.stderr, "{line}");
        }
        result["exit_code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok())
            .unwrap_or(Exit::Internal.code())
    }
}

fn failure_result(command: &Command, failure: Failure) -> Value {
    envelope(Some(command.name()), Map::new(), Vec::new(), &Err(failure))
}

/// What `receiver pair` waits for.
enum Event {
    Started,
    Interrupted,
    Reply(zbus::Result<zbus::Message>),
}

/// `4659…` (16 hex digits) as bytes.
fn raw_bytes(hex: &str) -> Option<[u8; 8]> {
    if hex.len() != 16 {
        return None;
    }
    let mut bytes = [0; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(bytes)
}
