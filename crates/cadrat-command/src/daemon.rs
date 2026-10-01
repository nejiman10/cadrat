//! `cadrat-tool` does not write to devices while `cadratd` runs
//! (spec daemon §4, tool/cli §8, Q9).

use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

use crate::exit::{Exit, Failure};

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
        let dir = home.join("cadrat");
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_failure(&dir, &e)),
        }
        let path = dir.join("cadratd.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| io_failure(&path, &e))?;
        lock(&file, &path, || {
            format!("cadratd is running; use `cadratctl {instead}` instead")
        })?;
        Ok(DaemonGuard(vec![file]))
    }

    /// As root, every user's `cadratd` could be writing: check each existing
    /// lock file, and create none.
    fn hold_all(&self) -> Result<DaemonGuard, Failure> {
        let entries = match std::fs::read_dir(&self.run_user) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DaemonGuard(Vec::new())),
            Err(e) => return Err(io_failure(&self.run_user, &e)),
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("cadrat/cadratd.lock"))
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        let mut files = Vec::new();
        for path in paths {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_failure(&path, &e)),
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
