//! `cadrat-hold-open`: keeps one wired C658 hidraw node open until it
//! disappears or the service is stopped (spec hold-open/cli).
//!
//! udev starts one `cadrat-hold-open@<node>.service` per node. [`run`] takes
//! the arguments, the system and the stop signal, so the tests run it
//! against the fake transport of `cadrat-hidraw`; the binary only wires the
//! real implementations in.

use std::ffi::OsString;
use std::io::Write;
use std::os::fd::BorrowedFd;
use std::path::PathBuf;

use cadrat_hidraw::sys::{BUS_USB, PRODUCT_C658, VENDOR};
use cadrat_hidraw::{Errno, System, Wait};
use clap::Parser;

/// The exit codes this program uses (spec hold-open/cli §3.1). Their numbers
/// and names are those of the shared table (spec tool/cli §6); a test checks
/// them against `cadrat-command`, which this root service does not link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// 0: stopped by a signal, or the node disappeared.
    Success,
    /// 1: unexpected internal error.
    Internal,
    /// 2: invalid arguments.
    Usage,
    /// 4: the node does not exist.
    NoDevice,
    /// 6: no permission to open the node.
    PermissionDenied,
    /// 7: the node is not a wired C658.
    DeviceInvalid,
}

impl Exit {
    /// Every exit code, for the test against the shared table.
    pub const ALL: [Self; 6] = [
        Self::Success,
        Self::Internal,
        Self::Usage,
        Self::NoDevice,
        Self::PermissionDenied,
        Self::DeviceInvalid,
    ];

    /// The process exit code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Internal => 1,
            Self::Usage => 2,
            Self::NoDevice => 4,
            Self::PermissionDenied => 6,
            Self::DeviceInvalid => 7,
        }
    }

    /// The name, as in the shared table.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Internal => "Internal",
            Self::Usage => "Usage",
            Self::NoDevice => "NoDevice",
            Self::PermissionDenied => "PermissionDenied",
            Self::DeviceInvalid => "DeviceInvalid",
        }
    }
}

/// The version shown by `--version`: `CADRAT_VERSION` at build time (set by
/// the packaging scripts), else the crate version.
const VERSION: &str = match option_env!("CADRAT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// Keep one wired C658 hidraw node open until it disappears.
///
/// Started by udev as cadrat-hold-open@<node>.service for each wired C658
/// node. Without it, a wired C658 was seen to stop sending input a few
/// seconds after it was plugged in.
#[derive(Debug, Parser)]
#[command(name = "cadrat-hold-open", version = VERSION, about)]
struct Cli {
    /// The hidraw node, e.g. /dev/hidraw5
    node: PathBuf,
}

/// The command-line definition, for generating the manual page.
#[must_use]
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// Standard error for the journal: every line it writes is an error, so
/// each gets the priority `<3>` (sd-daemon(3), spec hold-open/cli §3).
pub struct ErrorLevel<W> {
    inner: W,
    line_start: bool,
}

impl<W: Write> ErrorLevel<W> {
    /// Wraps `inner`.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            line_start: true,
        }
    }

    /// The wrapped writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for ErrorLevel<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut out = Vec::with_capacity(buf.len() + 3);
        for &byte in buf {
            if self.line_start {
                out.extend_from_slice(b"<3>");
            }
            out.push(byte);
            self.line_start = byte == b'\n';
        }
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// SIGINT and SIGTERM.
pub trait Stop {
    /// Becomes readable when a signal arrives (a self-pipe).
    fn wake(&self) -> BorrowedFd<'_>;
    /// Whether a signal arrived.
    fn is_set(&self) -> bool;
}

/// Runs `cadrat-hold-open` and returns the exit code (spec hold-open/cli
/// §3, §3.1). Output is best effort: the journal may be gone at shutdown.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    system: &dyn System,
    stop: &dyn Stop,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            use clap::error::ErrorKind;
            let rendered = error.render().to_string();
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = write!(stdout, "{rendered}");
                return Exit::Success.code();
            }
            let _ = write!(stderr, "{rendered}");
            return Exit::Usage.code();
        }
    };
    let path = cli.node;
    let fail = |stderr: &mut dyn Write, exit: Exit, message: String| {
        let _ = writeln!(stderr, "error: {message}");
        exit.code()
    };

    // 1. Open.
    let mut device = match system.open(&path) {
        Ok(device) => device,
        Err(e) => {
            let errno = Errno::of(&e);
            let exit = if errno.is_device_gone() {
                Exit::NoDevice
            } else if errno.is_permission() {
                Exit::PermissionDenied
            } else {
                Exit::Internal
            };
            return fail(
                stderr,
                exit,
                format!("cannot open {}: {errno}", path.display()),
            );
        }
    };

    // 2. The same condition as the udev rule, checked on the descriptor.
    match device.raw_info() {
        Ok(info) if (info.bus, info.vendor, info.product) == (BUS_USB, VENDOR, PRODUCT_C658) => {}
        Ok(info) => {
            return fail(
                stderr,
                Exit::DeviceInvalid,
                format!(
                    "{} is not a wired C658 (bus {:#04x}, {:04x}:{:04x})",
                    path.display(),
                    info.bus,
                    info.vendor,
                    info.product
                ),
            );
        }
        Err(e) => {
            let errno = Errno::of(&e);
            let exit = if errno.is_device_gone() {
                Exit::NoDevice
            } else {
                Exit::Internal
            };
            return fail(stderr, exit, format!("{}: {errno}", path.display()));
        }
    }

    // 3. Held.
    let interface = system
        .node(&path)
        .ok()
        .and_then(|info| info.interface)
        .map_or_else(String::new, |n| format!(" (MI_{n:02})"));
    let _ = writeln!(stdout, "held      {}{interface}", path.display());
    let _ = stdout.flush();

    // 4. Wait for the node to disappear or a signal.
    let mut code = Exit::Success.code();
    while !stop.is_set() {
        match device.wait_hangup(stop.wake()) {
            Ok(Wait::Hangup) => break,
            Ok(Wait::Woken) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                let _ = writeln!(
                    stderr,
                    "error: waiting on {}: {}",
                    path.display(),
                    Errno::of(&e)
                );
                code = Exit::Internal.code();
                break;
            }
        }
    }

    // 5. Released.
    drop(device);
    let _ = writeln!(stdout, "released  {}", path.display());
    code
}
