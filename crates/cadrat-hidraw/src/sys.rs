//! The boundary to the operating system: sysfs, hidraw devices and time.
//!
//! Everything above this module talks to these traits only, so the whole
//! discovery, selection, send and Receiver logic runs against fakes in tests
//! (spec 04 §1, §4).

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// USB bus type in `HIDIOCGRAWINFO` and `HID_ID`.
pub const BUS_USB: u32 = 0x03;
/// 3Dconnexion vendor ID.
pub const VENDOR: u16 = 0x256f;
/// C658 mouse (`CadMouse` Compact Wireless), wired.
pub const PRODUCT_C658: u16 = 0xc658;
/// Universal Receiver.
pub const PRODUCT_C652: u16 = 0xc652;

/// What sysfs says about one hidraw node, before opening it (spec 02 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    /// Device path, e.g. `/dev/hidraw5`.
    pub path: PathBuf,
    /// `HID_ID` from uevent: bus, vendor, product.
    pub hid_id: Option<(u32, u16, u16)>,
    /// `HID_NAME` from uevent.
    pub hid_name: Option<String>,
    /// `bInterfaceNumber` of the USB interface.
    pub interface: Option<u8>,
    /// USB port path of the parent USB device, e.g. `3-2` or `1-4.1`.
    pub usb_port: Option<String>,
}

/// `HIDIOCGRAWINFO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawInfo {
    /// Bus type.
    pub bus: u32,
    /// Vendor ID.
    pub vendor: u16,
    /// Product ID.
    pub product: u16,
}

/// Enumerates hidraw nodes and opens them.
pub trait System {
    /// Every hidraw node in sysfs.
    ///
    /// # Errors
    ///
    /// If the hidraw class directory cannot be read.
    fn nodes(&self) -> io::Result<Vec<NodeInfo>>;

    /// The sysfs information for one device path (`--hidraw`).
    ///
    /// # Errors
    ///
    /// If the path is not a hidraw node known to sysfs.
    fn node(&self, path: &std::path::Path) -> io::Result<NodeInfo>;

    /// Opens a node with `O_RDWR | O_CLOEXEC | O_NONBLOCK`.
    ///
    /// # Errors
    ///
    /// The `open` error, e.g. `EACCES` without the udev rule.
    fn open(&self, path: &std::path::Path) -> io::Result<Box<dyn Device>>;
}

/// An open hidraw node.
pub trait Device: fmt::Debug {
    /// `HIDIOCGRAWINFO`.
    ///
    /// # Errors
    ///
    /// The ioctl error.
    fn raw_info(&mut self) -> io::Result<RawInfo>;

    /// `HIDIOCGRDESCSIZE` and `HIDIOCGRDESC`.
    ///
    /// # Errors
    ///
    /// The ioctl error.
    fn descriptor(&mut self) -> io::Result<Vec<u8>>;

    /// `HIDIOCGFEATURE(len)` for `report_id`; returns the bytes the kernel
    /// reported, starting with the Report ID.
    ///
    /// # Errors
    ///
    /// The ioctl error.
    fn get_feature(&mut self, report_id: u8, len: usize) -> io::Result<Vec<u8>>;

    /// `HIDIOCSFEATURE(data.len())`; returns the ioctl's return value.
    ///
    /// # Errors
    ///
    /// The ioctl error.
    fn set_feature(&mut self, data: &[u8]) -> io::Result<usize>;
}

/// Monotonic time and sleeping, replaceable in tests.
pub trait Clock {
    /// Time elapsed since an arbitrary fixed point.
    fn now(&self) -> Duration;
    /// Sleeps for `duration`. May return early when interrupted.
    fn sleep(&self, duration: Duration);
}

/// The real clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    start: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// An `errno` shown by name, e.g. `EPIPE`, as in `probe-error: EPIPE`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Errno(pub i32);

impl Errno {
    /// The errno of an I/O error, or `EIO` if it carries none.
    #[must_use]
    pub fn of(error: &io::Error) -> Self {
        Self(
            error
                .raw_os_error()
                .unwrap_or(rustix::io::Errno::IO.raw_os_error()),
        )
    }

    /// Whether the errno means the device is gone.
    #[must_use]
    pub fn is_device_gone(self) -> bool {
        use rustix::io::Errno as E;
        [E::NODEV, E::NXIO, E::SHUTDOWN, E::NOENT]
            .iter()
            .any(|e| e.raw_os_error() == self.0)
    }

    /// Whether the errno is a permission failure.
    #[must_use]
    pub fn is_permission(self) -> bool {
        use rustix::io::Errno as E;
        [E::ACCESS, E::PERM]
            .iter()
            .any(|e| e.raw_os_error() == self.0)
    }

    /// Whether the errno is `EPIPE`.
    #[must_use]
    pub fn is_epipe(self) -> bool {
        self.0 == rustix::io::Errno::PIPE.raw_os_error()
    }
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use rustix::io::Errno as E;
        let names = [
            (E::PERM, "EPERM"),
            (E::NOENT, "ENOENT"),
            (E::INTR, "EINTR"),
            (E::IO, "EIO"),
            (E::NXIO, "ENXIO"),
            (E::AGAIN, "EAGAIN"),
            (E::ACCESS, "EACCES"),
            (E::BUSY, "EBUSY"),
            (E::NODEV, "ENODEV"),
            (E::INVAL, "EINVAL"),
            (E::PIPE, "EPIPE"),
            (E::NOTTY, "ENOTTY"),
            (E::OVERFLOW, "EOVERFLOW"),
            (E::PROTO, "EPROTO"),
            (E::SHUTDOWN, "ESHUTDOWN"),
            (E::TIMEDOUT, "ETIMEDOUT"),
        ];
        match names.iter().find(|(e, _)| e.raw_os_error() == self.0) {
            Some((_, name)) => f.write_str(name),
            None => write!(f, "errno {}", self.0),
        }
    }
}

impl fmt::Debug for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<rustix::io::Errno> for Errno {
    fn from(errno: rustix::io::Errno) -> Self {
        Self(errno.raw_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_names() {
        assert_eq!(Errno::from(rustix::io::Errno::PIPE).to_string(), "EPIPE");
        assert_eq!(Errno::from(rustix::io::Errno::NODEV).to_string(), "ENODEV");
        assert_eq!(Errno(4095).to_string(), "errno 4095");
        assert!(Errno::from(rustix::io::Errno::NODEV).is_device_gone());
        assert!(Errno::from(rustix::io::Errno::ACCESS).is_permission());
        assert!(!Errno::from(rustix::io::Errno::PIPE).is_device_gone());
        let err = io::Error::from_raw_os_error(rustix::io::Errno::PIPE.raw_os_error());
        assert!(Errno::of(&err).is_epipe());
    }
}
