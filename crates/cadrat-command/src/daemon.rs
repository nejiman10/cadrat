//! `cadrat-tool` does not write to devices while `cadratd` runs
//! (spec daemon §4, tool/cli §8, Q9).

use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

use crate::{Exit, Failure};

/// Where `cadratd` keeps its lock: `<run_user>/<uid>/cadrat/cadratd.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonLock {
    /// `/run/user`, created by logind. Never `XDG_RUNTIME_DIR`, which can
    /// differ under ssh, `sudo -E` or `env -i` and silently split the lock.
    pub run_user: PathBuf,
    /// The real user ID (`getuid()`). 0 checks every user's lock.
    pub uid: u32,
}

impl DaemonLock {
    /// `/run/user` and the real user ID of this process.
    #[must_use]
    pub fn system() -> Self {
        Self {
            run_user: PathBuf::from("/run/user"),
            uid: rustix::process::getuid().as_raw(),
        }
    }

    /// Takes the shared lock on `cadratd.lock` without waiting, and keeps
    /// it until the returned guard is dropped, so `cadratd` waits to start
    /// in the meantime.
    ///
    /// `instead` is the same operation as a `cadratctl` command, for the
    /// message.
    pub(crate) fn hold(&self, instead: &str) -> Result<DaemonGuard, Failure> {
        if self.uid == 0 {
            return self.hold_all();
        }
        let home = self.run_user.join(self.uid.to_string());
        if !home.is_dir() {
            // Without the runtime directory, this user's cadratd cannot run.
            return Ok(DaemonGuard(Vec::new()));
        }
        let (path, file) = self.open().map_err(|(path, e)| io_failure(&path, &e))?;
        lock(&file, &path, || {
            format!("cadratd is running; use `cadratctl {instead}` instead")
        })?;
        Ok(DaemonGuard(vec![file]))
    }

    /// Opens `cadratd.lock`, creating `cadrat/` (mode 0700) and the file
    /// (mode 0600) when missing. `cadratd` takes its exclusive lock on it.
    ///
    /// # Errors
    ///
    /// The path and error that failed; `NotFound` when `/run/user/<uid>`
    /// does not exist, which this never creates (spec daemon §4).
    pub fn open(&self) -> Result<(PathBuf, File), (PathBuf, io::Error)> {
        let home = self.run_user.join(self.uid.to_string());
        if !home.is_dir() {
            return Err((home, io::ErrorKind::NotFound.into()));
        }
        let dir = home.join("cadrat");
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err((dir, e)),
        }
        let path = dir.join("cadratd.lock");
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => Ok((path, file)),
            Err(e) => Err((path, e)),
        }
    }

    /// As root, every user's `cadratd` could be writing: check each existing
    /// lock file, and create none.
    fn hold_all(&self) -> Result<DaemonGuard, Failure> {
        let entries = match std::fs::read_dir(&self.run_user) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DaemonGuard(Vec::new())),
            Err(e) => return Err(io_failure(&self.run_user, &e)),
        };
        let mut homes: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        homes.sort();
        let mut files = Vec::new();
        for home in homes {
            let path = home.join("cadrat/cadratd.lock");
            let Some(file) = open_other(&home).map_err(|e| io_failure(&path, &e))? else {
                continue;
            };
            lock(&file, &path, || {
                format!(
                    "cadratd is running ({}); stop it before writing to devices as root",
                    path.display()
                )
            })?;
            files.push(file);
        }
        Ok(DaemonGuard(files))
    }
}

/// Opens another user's `<home>/cadrat/cadratd.lock` for reading, as root.
///
/// The user can write `<home>`, so neither `cadrat/` nor the lock file is
/// followed if it is a symbolic link, the open does not wait (a FIFO swapped
/// in would block it), and only a regular file counts. Anything else is no
/// lock file: `cadratd` creates a regular one. `None` when there is none.
fn open_other(home: &Path) -> io::Result<Option<File>> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open, openat};
    let skip = |e: rustix::io::Errno| match e {
        rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP => Ok(None),
        e => Err(io::Error::from(e)),
    };
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let dir = match open(
        home.join("cadrat"),
        flags | OFlags::DIRECTORY,
        Mode::empty(),
    ) {
        Ok(dir) => dir,
        Err(e) => return skip(e),
    };
    let fd = match openat(
        &dir,
        "cadratd.lock",
        flags | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(e) => return skip(e),
    };
    let stat = fstat(&fd)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Ok(None);
    }
    Ok(Some(File::from(fd)))
}

/// Shared locks on `cadratd.lock`, released when dropped.
#[derive(Debug)]
pub(crate) struct DaemonGuard(#[allow(dead_code)] Vec<File>);

fn lock(file: &File, path: &Path, running: impl FnOnce() -> String) -> Result<(), Failure> {
    match flock(file, FlockOperation::NonBlockingLockShared) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::WOULDBLOCK) => Err(Failure::new(Exit::DaemonRunning, running())),
        Err(e) => Err(io_failure(path, &e.into())),
    }
}

fn io_failure(path: &Path, error: &io::Error) -> Failure {
    Failure::new(
        Exit::IoError,
        format!("cannot check {}: {error}", path.display()),
    )
}
