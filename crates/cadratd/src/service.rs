//! The D-Bus object and the requests (spec daemon §5, §6, dbus §4–§6).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use cadrat_command::{Command, Env, Failure, Frontend, Interrupt, Options};
use cadrat_dbus::{Args, DeviceUse, Error, Method, Request, decode, decode_cancel, device_use};
use futures_lite::StreamExt;
use serde_json::{Map, Value, json};
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::{Connection, fdo};

use crate::World;
use crate::log::Log;

/// The program named in guidance inside results (spec ctl/cli §3).
const PROGRAM: &str = "cadratctl";

/// The running device-writing request.
struct Writing {
    command: &'static str,
    interrupt: Arc<AtomicBool>,
}

/// State shared by the requests and the main loop.
pub struct Shared {
    pub world: World,
    log: Arc<Log>,
    ready: AtomicBool,
    stopping: AtomicBool,
    devices: Mutex<String>,
    /// Warnings of the last enumeration, logged only when they change.
    last_warnings: Mutex<Vec<Value>>,
    writing: Mutex<Option<Writing>>,
    active: Mutex<usize>,
    idle: Condvar,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    pub fn new(world: World, log: Arc<Log>) -> Self {
        Self {
            world,
            log,
            ready: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            devices: Mutex::new(json!({"mice": [], "receivers": []}).to_string()),
            last_warnings: Mutex::new(Vec::new()),
            writing: Mutex::new(None),
            active: Mutex::new(0),
            idle: Condvar::new(),
        }
    }

    pub fn set_ready(&self) {
        self.ready.store(true, Ordering::SeqCst);
    }

    /// Enumerates again and updates `Devices` (spec daemon §6). Returns
    /// whether `Devices` changed.
    pub fn enumerate(&self) -> bool {
        let result = self.execute_quietly(
            &Options::default(),
            &Command::List {
                nodes: false,
                redact: false,
            },
        );
        let warnings = result["warnings"].as_array().cloned().unwrap_or_default();
        {
            let mut last = lock(&self.last_warnings);
            if *last != warnings {
                for line in cadrat_command::render::warning_lines(&result) {
                    self.log.line(&line);
                }
                *last = warnings;
            }
        }
        if result["ok"] != true {
            self.log.line(&format!(
                "warning: listing devices failed: {}",
                result["error"]["message"].as_str().unwrap_or_default()
            ));
            return false;
        }
        let devices = json!({
            "mice": result["mice"],
            "receivers": result["receivers"],
        })
        .to_string();
        let mut current = lock(&self.devices);
        if *current == devices {
            return false;
        }
        *current = devices;
        true
    }

    fn execute_quietly(&self, options: &Options, command: &Command) -> Value {
        let interrupt = OpInterrupt(Arc::new(AtomicBool::new(false)));
        let env = self.env(&interrupt);
        cadrat_command::execute(&env, &mut Quiet, options, command, PROGRAM)
    }

    fn env<'a>(&'a self, interrupt: &'a dyn Interrupt) -> Env<'a> {
        Env {
            system: &*self.world.system,
            clock: &*self.world.clock,
            interrupt,
            xdg_config_home: self.world.xdg_config_home.clone(),
            home: self.world.home.clone(),
            lock_timeout: self.world.lock_timeout,
            daemon_lock: None,
        }
    }

    /// Runs one request on the calling thread.
    fn execute(
        &self,
        request: &Request,
        interrupt: &Arc<AtomicBool>,
        emitter: SignalEmitter<'static>,
    ) -> Value {
        let interrupt = OpInterrupt(Arc::clone(interrupt));
        let env = self.env(&interrupt);
        let mut frontend = Bus { emitter };
        let result = cadrat_command::execute(
            &env,
            &mut frontend,
            &request.options,
            &request.command,
            PROGRAM,
        );
        self.log_result(&result);
        result
    }

    /// One line per request: command, exit code name, the route sent over
    /// and whether the file was saved; then its warnings (spec daemon §7).
    fn log_result(&self, result: &Value) {
        let mut line = format!(
            "{}: {}",
            result["command"].as_str().unwrap_or("?"),
            result["error"]["code"].as_str().unwrap_or("Success")
        );
        if let Some(route) = result["mouse"]["sent_via"]["route"].as_str()
            && result["sent"] == true
        {
            let _ = write!(line, ", sent via {route}");
        }
        if result["saved"] == true {
            line.push_str(", saved");
        }
        self.log.line(&line);
        for warning in cadrat_command::render::warning_lines(result) {
            self.log.line(&warning);
        }
    }

    /// Marks `cadratd` as stopping, interrupts the running device write
    /// (a pair sends its stop packet) and waits for every request to end.
    pub fn stop_and_wait(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(writing) = lock(&self.writing).as_ref() {
            writing.interrupt.store(true, Ordering::SeqCst);
        }
        let mut active = lock(&self.active);
        while *active > 0 {
            active = self
                .idle
                .wait(active)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Takes the device-write slot, or fails with `Busy` (spec daemon §5).
    fn claim(&self, command: &'static str, interrupt: &Arc<AtomicBool>) -> Result<(), Error> {
        let mut writing = lock(&self.writing);
        if let Some(running) = writing.as_ref() {
            return Err(Error::Busy(format!(
                "cadratd is running `{}`; nothing was done",
                running.command
            )));
        }
        *writing = Some(Writing {
            command,
            interrupt: Arc::clone(interrupt),
        });
        Ok(())
    }

    fn busy(&self) -> String {
        lock(&self.writing)
            .as_ref()
            .map(|w| w.command.to_owned())
            .unwrap_or_default()
    }
}

/// Counts a request as running until dropped.
struct Active<'a>(&'a Shared);

impl<'a> Active<'a> {
    fn new(shared: &'a Shared) -> Self {
        *lock(&shared.active) += 1;
        Self(shared)
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        let mut active = lock(&self.0.active);
        *active -= 1;
        if *active == 0 {
            self.0.idle.notify_all();
        }
    }
}

/// Holds the device-write slot until dropped.
struct WriteSlot<'a>(&'a Shared);

impl Drop for WriteSlot<'_> {
    fn drop(&mut self) {
        *lock(&self.0.writing) = None;
    }
}

/// The interrupt of one request: set by `Cancel`, by the caller leaving the
/// bus, or by stopping `cadratd`.
struct OpInterrupt(Arc<AtomicBool>);

impl Interrupt for OpInterrupt {
    fn arm(&self) {}
    fn disarm(&self) {}
    fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Nothing reaches a user while listing for `Devices`.
struct Quiet;

impl Frontend for Quiet {
    fn verbose(&mut self, _: &str) {}
    fn warning(&mut self, _: &str, _: &str) {}
    fn pairing_started(&mut self, _: &str, _: Duration) {}
    fn can_confirm(&self) -> bool {
        false
    }
    fn unpair_target(&mut self, _: &[String]) {}
    fn confirm_unpair(&mut self, _: cadrat_proto::Slot) -> bool {
        false
    }
}

/// What a request shows its caller while it runs: only `PairingStarted`.
/// Warnings and `-v` lines are in the result; the caller confirms unpair
/// itself (spec daemon §5).
struct Bus {
    emitter: SignalEmitter<'static>,
}

impl Frontend for Bus {
    fn verbose(&mut self, _: &str) {}
    fn warning(&mut self, _: &str, _: &str) {}
    fn pairing_started(&mut self, receiver: &str, _: Duration) {
        let _ = zbus::block_on(Manager::pairing_started(&self.emitter, receiver));
    }
    fn can_confirm(&self) -> bool {
        false
    }
    fn unpair_target(&mut self, _: &[String]) {}
    fn confirm_unpair(&mut self, _: cadrat_proto::Slot) -> bool {
        false
    }
}

/// The object at `/cc/nejiman10/Cadrat1`.
pub struct Manager {
    shared: Arc<Shared>,
}

impl Manager {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Spec dbus §2: only the same user may call.
    async fn check_caller(&self, conn: &Connection, sender: &UniqueName<'_>) -> Result<(), Error> {
        let proxy = fdo::DBusProxy::new(conn)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        let uid = proxy
            .get_connection_unix_user(BusName::from(sender.clone()))
            .await
            .map_err(|e| Error::AccessDenied(format!("cannot identify the caller: {e}")))?;
        if uid == rustix::process::getuid().as_raw() {
            Ok(())
        } else {
            Err(Error::AccessDenied(
                "cadratd only answers its own user".to_owned(),
            ))
        }
    }

    async fn call(
        &self,
        method: Method,
        header: &Header<'_>,
        conn: &Connection,
        emitter: &SignalEmitter<'_>,
        args: Args,
    ) -> Result<String, Error> {
        let sender = header
            .sender()
            .ok_or_else(|| Error::Internal("a method call without a sender".to_owned()))?
            .to_owned();
        self.check_caller(conn, &sender).await?;
        let request = match decode(method, &args)? {
            Ok(request) => request,
            Err(failure) => return Ok(usage_result(method, failure)),
        };
        let shared = &self.shared;
        let uses = device_use(&request.command);
        if uses != DeviceUse::None {
            if shared.stopping.load(Ordering::SeqCst) {
                return Err(Error::Busy(
                    "cadratd is stopping; nothing was done".to_owned(),
                ));
            }
            if !shared.ready.load(Ordering::SeqCst) {
                return Err(Error::Starting(
                    "cadratd is starting: it waits for cadrat-tool to finish writing to a \
                     device; nothing was done"
                        .to_owned(),
                ));
            }
        }
        let interrupt = Arc::new(AtomicBool::new(false));
        let _active = Active::new(shared);
        let slot = if uses == DeviceUse::Write {
            shared.claim(method.command_name(), &interrupt)?;
            let _ = self.busy_changed(emitter).await;
            Some(WriteSlot(shared))
        } else {
            None
        };

        // Spec daemon §5: a pair or an unpair stops waiting when its caller
        // leaves the bus.
        let watcher = matches!(method, Method::ReceiverPair | Method::ReceiverUnpair).then(|| {
            conn.executor().spawn(
                caller_gone(conn.clone(), sender.clone(), Arc::clone(&interrupt)),
                "cadratd caller watch",
            )
        });
        let to_caller = emitter
            .to_owned()
            .set_destination(BusName::from(sender.clone()));
        let runner = Arc::clone(shared);
        let flag = Arc::clone(&interrupt);
        let result = blocking::unblock(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runner.execute(&request, &flag, to_caller)
            }))
        })
        .await;
        drop(watcher);
        if slot.is_some() {
            drop(slot);
            let _ = self.busy_changed(emitter).await;
        }
        if let Ok(json) = result {
            Ok(json.to_string())
        } else {
            let name = method.command_name();
            self.shared
                .log
                .line(&format!("{name}: Internal (panicked)"));
            Err(Error::Internal(format!("{name} failed unexpectedly")))
        }
    }
}

/// The JSON of a request whose values `cadrat-tool` would reject (spec
/// dbus §2).
fn usage_result(method: Method, failure: Failure) -> String {
    cadrat_command::envelope(
        Some(method.command_name()),
        Map::new(),
        Vec::new(),
        &Err(failure),
    )
    .to_string()
}

/// Sets `flag` once `name` has left the bus.
async fn caller_gone(conn: Connection, name: UniqueName<'static>, flag: Arc<AtomicBool>) {
    let Ok(proxy) = fdo::DBusProxy::new(&conn).await else {
        return;
    };
    let Ok(mut changes) = proxy
        .receive_name_owner_changed_with_args(&[(0, name.as_str())])
        .await
    else {
        return;
    };
    // It may have left before the subscription.
    if !proxy
        .name_has_owner(BusName::from(name.clone()))
        .await
        .unwrap_or(true)
    {
        flag.store(true, Ordering::SeqCst);
        return;
    }
    while let Some(change) = changes.next().await {
        if change.args().is_ok_and(|args| args.new_owner().is_none()) {
            flag.store(true, Ordering::SeqCst);
            return;
        }
    }
}

#[zbus::interface(name = "cc.nejiman10.Cadrat1.Manager")]
impl Manager {
    async fn list(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::List, &header, conn, &emitter, args).await
    }

    async fn init(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::Init, &header, conn, &emitter, args).await
    }

    async fn get(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::Get, &header, conn, &emitter, args).await
    }

    async fn check(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::Check, &header, conn, &emitter, args)
            .await
    }

    async fn set(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::Set, &header, conn, &emitter, args).await
    }

    async fn apply(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::Apply, &header, conn, &emitter, args)
            .await
    }

    async fn receiver_slots(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::ReceiverSlots, &header, conn, &emitter, args)
            .await
    }

    async fn receiver_pair(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::ReceiverPair, &header, conn, &emitter, args)
            .await
    }

    async fn receiver_unpair(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        self.call(Method::ReceiverUnpair, &header, conn, &emitter, args)
            .await
    }

    /// Stops a running `ReceiverPair`; its own reply carries the result.
    async fn cancel(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        args: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Result<String, Error> {
        let sender = header
            .sender()
            .ok_or_else(|| Error::Internal("a method call without a sender".to_owned()))?
            .to_owned();
        self.check_caller(conn, &sender).await?;
        decode_cancel(&args)?;
        let cancelled = match lock(&self.shared.writing).as_ref() {
            Some(running) if running.command == Method::ReceiverPair.command_name() => {
                running.interrupt.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        };
        let mut fields = Map::new();
        fields.insert("cancelled".to_owned(), cancelled.into());
        Ok(cadrat_command::envelope(Some("cancel"), fields, Vec::new(), &Ok(())).to_string())
    }

    /// The version of `cadratd`.
    #[zbus(property)]
    #[allow(clippy::unused_self)]
    fn version(&self) -> String {
        crate::VERSION.to_owned()
    }

    /// The last enumeration: `list --json`'s `mice` and `receivers`.
    #[zbus(property)]
    fn devices(&self) -> String {
        lock(&self.shared.devices).clone()
    }

    /// The running device-writing command, or "".
    #[zbus(property)]
    fn busy(&self) -> String {
        self.shared.busy()
    }

    /// Whether requests that touch devices are accepted.
    #[zbus(property)]
    fn ready(&self) -> bool {
        self.shared.ready.load(Ordering::SeqCst)
    }

    /// Pairing mode started on the Receiver with this key. Sent only to the
    /// caller of `ReceiverPair`.
    #[zbus(signal)]
    pub async fn pairing_started(emitter: &SignalEmitter<'_>, receiver: &str) -> zbus::Result<()>;
}
