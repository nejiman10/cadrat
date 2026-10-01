//! Watching `/dev` for hidraw nodes that appear, disappear or change their
//! permissions (spec daemon §6).

use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};

/// An inotify watch on one directory, normally `/dev`.
#[derive(Debug)]
pub struct DevWatch {
    fd: OwnedFd,
}

impl DevWatch {
    /// Watches `dir` for created, deleted and changed entries.
    ///
    /// # Errors
    ///
    /// The `inotify_init1` or `inotify_add_watch` error.
    pub fn new(dir: &Path) -> io::Result<Self> {
        let fd = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        inotify::add_watch(
            &fd,
            dir,
            WatchFlags::CREATE | WatchFlags::DELETE | WatchFlags::ATTRIB | WatchFlags::ONLYDIR,
        )?;
        Ok(Self { fd })
    }

    /// The descriptor to `poll` for readability.
    #[must_use]
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Reads every pending event. Returns whether one of them concerns a
    /// node named `hidraw…`, or the event queue overflowed.
    ///
    /// # Errors
    ///
    /// The `read` error other than `EAGAIN`.
    pub fn drain(&mut self) -> io::Result<bool> {
        let mut buf = [MaybeUninit::<u8>::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.fd, &mut buf);
        let mut relevant = false;
        loop {
            match reader.next() {
                Ok(event) => {
                    let hidraw = event
                        .file_name()
                        .is_some_and(|name| name.to_bytes().starts_with(b"hidraw"));
                    relevant |= hidraw || event.events().contains(ReadFlags::QUEUE_OVERFLOW);
                }
                Err(rustix::io::Errno::AGAIN) => return Ok(relevant),
                Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_hidraw_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = DevWatch::new(dir.path()).unwrap();
        assert!(!watch.drain().unwrap());
        std::fs::write(dir.path().join("tty5"), "").unwrap();
        assert!(!watch.drain().unwrap());
        std::fs::write(dir.path().join("hidraw3"), "").unwrap();
        assert!(watch.drain().unwrap());
        assert!(!watch.drain().unwrap());
        std::fs::remove_file(dir.path().join("hidraw3")).unwrap();
        assert!(watch.drain().unwrap());
    }
}
