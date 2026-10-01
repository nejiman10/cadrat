//! The real [`System`]: sysfs and hidraw ioctls.
//!
//! This is the only module with `unsafe` code: the hidraw ioctls. Each call
//! passes a buffer whose size matches the size encoded in the opcode.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::BorrowedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::ioctl::{self, Direction, Ioctl, IoctlOutput, Opcode, opcode};

use crate::sys::{Device, NodeInfo, RawInfo, System, Wait};

/// sysfs and `/dev`, with configurable roots for tests.
#[derive(Debug, Clone)]
pub struct LinuxSystem {
    sysfs: PathBuf,
    dev: PathBuf,
}

impl Default for LinuxSystem {
    fn default() -> Self {
        Self {
            sysfs: PathBuf::from("/sys"),
            dev: PathBuf::from("/dev"),
        }
    }
}

impl LinuxSystem {
    /// A system whose sysfs and `/dev` live under other roots.
    #[must_use]
    pub fn with_roots(sysfs: impl Into<PathBuf>, dev: impl Into<PathBuf>) -> Self {
        Self {
            sysfs: sysfs.into(),
            dev: dev.into(),
        }
    }

    fn class_dir(&self) -> PathBuf {
        self.sysfs.join("class/hidraw")
    }

    fn info(&self, name: &str) -> NodeInfo {
        let mut info = NodeInfo {
            path: self.dev.join(name),
            hid_id: None,
            hid_name: None,
            interface: None,
            usb_port: None,
        };
        // class/hidraw/<name>/device -> the HID device directory, whose parent
        // is the USB interface and grandparent the USB device.
        let Ok(hid_dir) = fs::canonicalize(self.class_dir().join(name).join("device")) else {
            return info;
        };
        if let Ok(uevent) = fs::read_to_string(hid_dir.join("uevent")) {
            for line in uevent.lines() {
                if let Some(id) = line.strip_prefix("HID_ID=") {
                    info.hid_id = parse_hid_id(id);
                } else if let Some(name) = line.strip_prefix("HID_NAME=") {
                    info.hid_name = Some(name.to_owned());
                }
            }
        }
        let Some(interface_dir) = hid_dir.parent() else {
            return info;
        };
        info.interface = fs::read_to_string(interface_dir.join("bInterfaceNumber"))
            .ok()
            .and_then(|s| u8::from_str_radix(s.trim(), 16).ok());
        if info.interface.is_some() {
            info.usb_port = interface_dir
                .parent()
                .filter(|usb| usb.join("busnum").exists())
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned());
        }
        info
    }
}

/// Parses `HID_ID=0003:0000256F:0000C658`.
fn parse_hid_id(id: &str) -> Option<(u32, u16, u16)> {
    let mut parts = id.split(':');
    let bus = u32::from_str_radix(parts.next()?, 16).ok()?;
    let vendor = u32::from_str_radix(parts.next()?, 16).ok()?;
    let product = u32::from_str_radix(parts.next()?, 16).ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((
        bus,
        u16::try_from(vendor).ok()?,
        u16::try_from(product).ok()?,
    ))
}

impl System for LinuxSystem {
    fn nodes(&self) -> io::Result<Vec<NodeInfo>> {
        let mut names: Vec<String> = match fs::read_dir(self.class_dir()) {
            Ok(dir) => dir
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("hidraw"))
                .collect(),
            // No hidraw driver loaded means no nodes.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        names.sort_by_key(|name| name["hidraw".len()..].parse::<u32>().unwrap_or(u32::MAX));
        Ok(names.iter().map(|name| self.info(name)).collect())
    }

    fn node(&self, path: &Path) -> io::Result<NodeInfo> {
        let name = fs::canonicalize(path)?
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| self.class_dir().join(n).exists())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a hidraw node"))?;
        let mut info = self.info(&name);
        path.clone_into(&mut info.path);
        Ok(info)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Device>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed())
            .open(path)?;
        Ok(Box::new(LinuxDevice { file }))
    }
}

/// An open `/dev/hidrawN`.
#[derive(Debug)]
pub struct LinuxDevice {
    file: File,
}

/// `struct hidraw_devinfo`.
#[repr(C)]
#[derive(Clone, Copy)]
struct DevInfo {
    bustype: u32,
    vendor: i16,
    product: i16,
}

const HID_MAX_DESCRIPTOR_SIZE: usize = 4096;

/// `struct hidraw_report_descriptor`.
#[repr(C)]
struct ReportDescriptor {
    size: u32,
    value: [u8; HID_MAX_DESCRIPTOR_SIZE],
}

const HIDIOCGRDESCSIZE: Opcode = opcode::read::<i32>(b'H', 0x01);
const HIDIOCGRDESC: Opcode = opcode::read::<ReportDescriptor>(b'H', 0x02);
const HIDIOCGRAWINFO: Opcode = opcode::read::<DevInfo>(b'H', 0x03);

/// `HIDIOCSFEATURE(len)` / `HIDIOCGFEATURE(len)` on a caller buffer; the
/// output is the ioctl's return value (the byte count).
struct Feature<'a> {
    opcode: Opcode,
    buffer: &'a mut [u8],
}

// SAFETY: the opcode encodes `buffer.len()` as its size and the kernel reads
// and writes at most that many bytes of `buffer`.
unsafe impl Ioctl for Feature<'_> {
    type Output = usize;
    const IS_MUTATING: bool = true;

    fn opcode(&self) -> Opcode {
        self.opcode
    }

    fn as_ptr(&mut self) -> *mut c_void {
        self.buffer.as_mut_ptr().cast()
    }

    unsafe fn output_from_ptr(out: IoctlOutput, _: *mut c_void) -> rustix::io::Result<usize> {
        usize::try_from(out).map_err(|_| rustix::io::Errno::INVAL)
    }
}

fn feature_opcode(number: u8, len: usize) -> Opcode {
    opcode::from_components(Direction::ReadWrite, b'H', number, len)
}

impl Device for LinuxDevice {
    fn raw_info(&mut self) -> io::Result<RawInfo> {
        // SAFETY: HIDIOCGRAWINFO writes one `struct hidraw_devinfo`.
        let info: DevInfo =
            unsafe { ioctl::ioctl(&self.file, ioctl::Getter::<HIDIOCGRAWINFO, DevInfo>::new()) }?;
        Ok(RawInfo {
            bus: info.bustype,
            vendor: info.vendor.cast_unsigned(),
            product: info.product.cast_unsigned(),
        })
    }

    fn descriptor(&mut self) -> io::Result<Vec<u8>> {
        // SAFETY: HIDIOCGRDESCSIZE writes one int.
        let size: i32 =
            unsafe { ioctl::ioctl(&self.file, ioctl::Getter::<HIDIOCGRDESCSIZE, i32>::new()) }?;
        let size = usize::try_from(size)
            .ok()
            .filter(|&size| size <= HID_MAX_DESCRIPTOR_SIZE)
            .ok_or_else(|| io::Error::from(rustix::io::Errno::OVERFLOW))?;
        let mut descriptor = Box::new(ReportDescriptor {
            size: u32::try_from(size).expect("size is at most 4096"),
            value: [0; HID_MAX_DESCRIPTOR_SIZE],
        });
        // SAFETY: HIDIOCGRDESC reads `size` and writes at most `size` bytes
        // of `value` in one `struct hidraw_report_descriptor`.
        unsafe {
            ioctl::ioctl(
                &self.file,
                ioctl::Updater::<HIDIOCGRDESC, ReportDescriptor>::new(&mut descriptor),
            )
        }?;
        Ok(descriptor.value[..size].to_vec())
    }

    fn get_feature(&mut self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let mut buffer = vec![0u8; len.max(1)];
        buffer[0] = report_id;
        let request = Feature {
            opcode: feature_opcode(0x07, buffer.len()),
            buffer: &mut buffer,
        };
        // SAFETY: see `Feature`.
        let count = unsafe { ioctl::ioctl(&self.file, request) }?;
        buffer.truncate(count.min(len));
        Ok(buffer)
    }

    fn set_feature(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut buffer = data.to_vec();
        let request = Feature {
            opcode: feature_opcode(0x06, buffer.len()),
            buffer: &mut buffer,
        };
        // SAFETY: see `Feature`.
        Ok(unsafe { ioctl::ioctl(&self.file, request) }?)
    }

    fn lock(&mut self) -> io::Result<()> {
        rustix::fs::flock(
            &self.file,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )?;
        Ok(())
    }

    fn wait_hangup(&mut self, wake: BorrowedFd<'_>) -> io::Result<Wait> {
        use rustix::event::{PollFd, PollFlags, poll};
        // No events requested on the node: input reports never wake us,
        // only POLLHUP / POLLERR, which poll always reports.
        let mut fds = [
            PollFd::new(&self.file, PollFlags::empty()),
            PollFd::from_borrowed_fd(wake, PollFlags::IN),
        ];
        poll(&mut fds, None)?;
        if fds[0]
            .revents()
            .intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL)
        {
            Ok(Wait::Hangup)
        } else {
            Ok(Wait::Woken)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    // Opcode is u32 or c_ulong depending on the target.
    #[allow(clippy::cast_lossless, clippy::unnecessary_cast)]
    fn opcodes_match_linux_headers() {
        // Values from <linux/hidraw.h> on x86_64 and aarch64.
        assert_eq!(HIDIOCGRDESCSIZE as u64, 0x8004_4801);
        assert_eq!(HIDIOCGRDESC as u64, 0x9004_4802);
        assert_eq!(HIDIOCGRAWINFO as u64, 0x8008_4803);
        assert_eq!(feature_opcode(0x06, 32) as u64, 0xc020_4806);
        assert_eq!(feature_opcode(0x07, 8) as u64, 0xc008_4807);
    }

    #[test]
    fn hid_id() {
        assert_eq!(
            parse_hid_id("0003:0000256F:0000C658"),
            Some((3, 0x256f, 0xc658))
        );
        assert_eq!(
            parse_hid_id("0005:0000046D:0000B023"),
            Some((5, 0x046d, 0xb023))
        );
        assert_eq!(parse_hid_id("0003:0000256F"), None);
        assert_eq!(parse_hid_id("0003:0001256F:0000C658"), None);
    }

    /// Builds the sysfs layout of one hidraw node under `root`.
    fn add_node(root: &Path, name: &str, usb: &str, interface: u8, hid_id: &str) {
        let usb_dir = root.join("devices/pci0000:00/usb3").join(usb);
        let interface_dir = usb_dir.join(format!("{usb}:1.{interface}"));
        let hid_dir = interface_dir.join(format!("{hid_id}.00{interface}"));
        fs::create_dir_all(hid_dir.join("hidraw").join(name)).unwrap();
        fs::write(usb_dir.join("busnum"), "3\n").unwrap();
        fs::write(
            interface_dir.join("bInterfaceNumber"),
            format!("{interface:02x}\n"),
        )
        .unwrap();
        fs::write(
            hid_dir.join("uevent"),
            format!(
                "DRIVER=hid-generic\nHID_ID={hid_id}\nHID_NAME=Synthetic Device\nHID_PHYS=usb\n"
            ),
        )
        .unwrap();
        symlink(&hid_dir, hid_dir.join("hidraw").join(name).join("device")).unwrap();
        let class = root.join("class/hidraw");
        fs::create_dir_all(&class).unwrap();
        symlink(hid_dir.join("hidraw").join(name), class.join(name)).unwrap();
    }

    #[test]
    fn reads_sysfs_layout() {
        let root = tempfile::tempdir().unwrap();
        add_node(root.path(), "hidraw10", "3-2", 1, "0003:0000256F:0000C658");
        add_node(root.path(), "hidraw9", "1-4.1", 3, "0003:0000256F:0000C652");
        let system = LinuxSystem::with_roots(root.path(), "/dev");
        let nodes = system.nodes().unwrap();
        assert_eq!(
            nodes,
            [
                NodeInfo {
                    path: PathBuf::from("/dev/hidraw9"),
                    hid_id: Some((3, 0x256f, 0xc652)),
                    hid_name: Some("Synthetic Device".into()),
                    interface: Some(3),
                    usb_port: Some("1-4.1".into()),
                },
                NodeInfo {
                    path: PathBuf::from("/dev/hidraw10"),
                    hid_id: Some((3, 0x256f, 0xc658)),
                    hid_name: Some("Synthetic Device".into()),
                    interface: Some(1),
                    usb_port: Some("3-2".into()),
                },
            ]
        );
    }

    #[test]
    fn missing_sysfs_parts_leave_fields_empty() {
        let root = tempfile::tempdir().unwrap();
        let class = root.path().join("class/hidraw/hidraw0");
        fs::create_dir_all(&class).unwrap();
        let system = LinuxSystem::with_roots(root.path(), "/dev");
        let nodes = system.nodes().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].hid_id, None);
        assert_eq!(nodes[0].usb_port, None);

        let empty = tempfile::tempdir().unwrap();
        let system = LinuxSystem::with_roots(empty.path(), "/dev");
        assert!(system.nodes().unwrap().is_empty());
    }

    #[test]
    fn node_by_path() {
        let root = tempfile::tempdir().unwrap();
        add_node(root.path(), "hidraw4", "3-2", 1, "0003:0000256F:0000C658");
        let dev = root.path().join("dev");
        fs::create_dir_all(&dev).unwrap();
        fs::write(dev.join("hidraw4"), "").unwrap();
        let system = LinuxSystem::with_roots(root.path(), &dev);
        let info = system.node(&dev.join("hidraw4")).unwrap();
        assert_eq!(info.interface, Some(1));
        assert_eq!(info.path, dev.join("hidraw4"));
        fs::write(dev.join("other"), "").unwrap();
        assert!(system.node(&dev.join("other")).is_err());
    }
}
