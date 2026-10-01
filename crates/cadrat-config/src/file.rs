//! The configuration file on disk: location, lock, load and atomic save
//! (spec config §1, §7).

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};
use sha2::{Digest, Sha256};

/// How long [`ConfigLock::acquire`] waits by default.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(50);

/// File-level failures. Each variant names the path involved.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The configuration file does not exist.
    #[error("{} does not exist", .0.display())]
    NotFound(PathBuf),
    /// The file is not UTF-8, so it cannot be TOML.
    #[error("{} is not valid UTF-8", .0.display())]
    NotUtf8(PathBuf),
    /// `ConfigLocked`: another process holds the lock.
    #[error("{} is locked by another process", .0.display())]
    Locked(PathBuf),
    /// The file changed after it was read (spec tool/cli §4 step 9).
    #[error("{} was changed by someone else after it was read", .0.display())]
    Changed(PathBuf),
    /// `init` found an existing file and `--force` was not given.
    #[error("{} already exists; use --force to overwrite it", .0.display())]
    AlreadyExists(PathBuf),
    /// Any other I/O failure.
    #[error("cannot {op} {}: {source}", .path.display())]
    Io {
        /// What was being done.
        op: &'static str,
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
}

fn io_error<'a>(op: &'static str, path: &'a Path) -> impl FnOnce(io::Error) -> FileError + 'a {
    move |source| FileError::Io {
        op,
        path: path.to_owned(),
        source,
    }
}

/// The default configuration path: `$XDG_CONFIG_HOME/cadrat/default.toml`,
/// or `$HOME/.config/cadrat/default.toml` when `XDG_CONFIG_HOME` is unset.
///
/// As the XDG Base Directory specification requires, an empty or relative
/// `XDG_CONFIG_HOME` counts as unset. Returns `None` without a usable home.
#[must_use]
pub fn default_path(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let base = match xdg_config_home.map(Path::new) {
        Some(xdg) if xdg.is_absolute() => xdg.to_owned(),
        _ => {
            let home = Path::new(home?);
            if !home.is_absolute() {
                return None;
            }
            home.join(".config")
        }
    };
    Some(base.join("cadrat").join("default.toml"))
}

/// [`default_path`] from the process environment.
#[must_use]
pub fn default_path_from_env() -> Option<PathBuf> {
    default_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Follows symbolic links so that a linked configuration file is updated in
/// place instead of being replaced by a regular file. Paths that do not
/// exist yet are returned unchanged.
fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The lock file for a configuration file: `<name>.lock` beside it.
#[must_use]
pub fn lock_path(config: &Path) -> PathBuf {
    let config = resolve(config);
    let mut name = config.file_name().unwrap_or_default().to_owned();
    name.push(".lock");
    config.with_file_name(name)
}

/// An exclusive `flock` on the lock file, held until dropped.
#[derive(Debug)]
pub struct ConfigLock {
    _file: File,
}

impl ConfigLock {
    /// Takes the lock for `config`, waiting up to `timeout`.
    ///
    /// # Errors
    ///
    /// [`FileError::Locked`] after the timeout; [`FileError::Io`] if the lock
    /// file cannot be opened.
    pub fn acquire(config: &Path, timeout: Duration) -> Result<Self, FileError> {
        let path = lock_path(config);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(io_error("open lock file", &path))?;
        let deadline = Instant::now() + timeout;
        loop {
            match flock(&file, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(Self { _file: file }),
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    if Instant::now() >= deadline {
                        return Err(FileError::Locked(path));
                    }
                    std::thread::sleep(LOCK_POLL);
                }
                Err(errno) => return Err(io_error("lock", &path)(errno.into())),
            }
        }
    }
}

/// SHA-256 of the file contents.
pub type Hash = [u8; 32];

fn sha256(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

/// A configuration file as read (spec tool/cli §4 step 3).
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The file actually read, with symbolic links resolved.
    pub path: PathBuf,
    /// Its contents.
    pub text: String,
    /// SHA-256 of the contents (H0).
    pub hash: Hash,
}

/// Reads a configuration file.
///
/// # Errors
///
/// [`FileError::NotFound`], [`FileError::NotUtf8`] or [`FileError::Io`].
pub fn load(path: &Path) -> Result<Loaded, FileError> {
    let resolved = resolve(path);
    let bytes = fs::read(&resolved).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => FileError::NotFound(path.to_owned()),
        _ => io_error("read", path)(e),
    })?;
    let hash = sha256(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| FileError::NotUtf8(path.to_owned()))?;
    Ok(Loaded {
        path: resolved,
        text,
        hash,
    })
}

impl Loaded {
    /// Re-reads the file and checks that it is unchanged (step 9).
    ///
    /// # Errors
    ///
    /// [`FileError::Changed`] if the contents differ or the file is gone.
    pub fn verify_unchanged(&self) -> Result<(), FileError> {
        match fs::read(&self.path) {
            Ok(bytes) if sha256(&bytes) == self.hash => Ok(()),
            Ok(_) => Err(FileError::Changed(self.path.clone())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(FileError::Changed(self.path.clone()))
            }
            Err(e) => Err(io_error("re-read", &self.path)(e)),
        }
    }

    /// Replaces the file with `text` if it is unchanged since it was read
    /// (steps 9 and 10). The new file keeps the old file's permissions.
    ///
    /// # Errors
    ///
    /// [`FileError::Changed`] (nothing written) or [`FileError::Io`].
    pub fn save(&self, text: &str) -> Result<(), FileError> {
        self.verify_unchanged()?;
        let permissions = fs::metadata(&self.path)
            .map_err(io_error("stat", &self.path))?
            .permissions();
        write_atomic(&self.path, text, Some(permissions))
    }
}

/// Writes a new configuration file for `init` (spec config §8), creating its
/// directory. An existing file is replaced atomically only with `force`.
///
/// # Errors
///
/// [`FileError::AlreadyExists`] or [`FileError::Io`].
pub fn create(path: &Path, text: &str, force: bool) -> Result<(), FileError> {
    let target = resolve(path);
    if let Some(dir) = target.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(io_error("create directory", dir))?;
    }
    match fs::symlink_metadata(&target) {
        Ok(_) if !force => return Err(FileError::AlreadyExists(path.to_owned())),
        Ok(meta) => return write_atomic(&target, text, Some(meta.permissions())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error("stat", path)(e)),
    }
    // create_new closes the window between the check above and the write.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => FileError::AlreadyExists(path.to_owned()),
            _ => io_error("create", path)(e),
        })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(io_error("write", path))?;
    sync_dir(&target)
}

/// Writes `text` to a temporary file in the same directory, syncs it, and
/// renames it over `target`.
fn write_atomic(
    target: &Path,
    text: &str,
    permissions: Option<fs::Permissions>,
) -> Result<(), FileError> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let (temp, mut file) = (0u32..100)
        .find_map(|n| {
            let temp = dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)
            {
                Ok(file) => Some(Ok((temp, file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(io_error("create temporary file in", dir)(e))),
            }
        })
        .unwrap_or_else(|| {
            Err(io_error("create temporary file in", dir)(
                io::ErrorKind::AlreadyExists.into(),
            ))
        })?;
    let written = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, target)
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(io_error("write", target)(e));
    }
    sync_dir(target)
}

/// Makes a rename or creation in the directory durable.
fn sync_dir(target: &Path) -> Result<(), FileError> {
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error("sync directory", dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn default_paths() {
        let p = |xdg: Option<&str>, home: Option<&str>| {
            default_path(xdg.map(OsStr::new), home.map(OsStr::new))
        };
        assert_eq!(
            p(Some("/x/config"), Some("/home/u")),
            Some(PathBuf::from("/x/config/cadrat/default.toml"))
        );
        assert_eq!(
            p(None, Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/cadrat/default.toml"))
        );
        assert_eq!(
            p(Some(""), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/cadrat/default.toml"))
        );
        assert_eq!(
            p(Some("relative"), Some("/home/u")),
            Some(PathBuf::from("/home/u/.config/cadrat/default.toml"))
        );
        assert_eq!(p(None, None), None);
        assert_eq!(p(None, Some("relative")), None);
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("default.toml");
        assert_eq!(lock_path(&config), dir.path().join("default.toml.lock"));

        let held = ConfigLock::acquire(&config, LOCK_TIMEOUT).unwrap();
        let start = Instant::now();
        let err = ConfigLock::acquire(&config, Duration::from_millis(120)).unwrap_err();
        assert!(matches!(err, FileError::Locked(_)), "{err}");
        assert!(start.elapsed() >= Duration::from_millis(120));
        drop(held);
        ConfigLock::acquire(&config, Duration::ZERO).unwrap();
    }

    #[test]
    fn lock_waits_for_release() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("default.toml");
        let held = ConfigLock::acquire(&config, LOCK_TIMEOUT).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(held);
        });
        ConfigLock::acquire(&config, Duration::from_secs(5)).unwrap();
        releaser.join().unwrap();
    }

    #[test]
    fn save_replaces_contents_and_keeps_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default.toml");
        fs::write(&path, "a = 1\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "a = 1\n");
        loaded.save("a = 2\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 2\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        // No temporary file is left behind.
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["default.toml"]);
    }

    #[test]
    fn save_refuses_concurrent_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default.toml");
        fs::write(&path, "a = 1\n").unwrap();
        let loaded = load(&path).unwrap();
        fs::write(&path, "a = 1 # edited\n").unwrap();
        let err = loaded.save("a = 2\n").unwrap_err();
        assert!(matches!(err, FileError::Changed(_)), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1 # edited\n");

        fs::remove_file(&path).unwrap();
        assert!(matches!(loaded.save("a = 2\n"), Err(FileError::Changed(_))));
        assert!(!path.exists());
    }

    #[test]
    fn save_through_symlink_updates_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        let link = dir.path().join("default.toml");
        fs::write(&real, "a = 1\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        load(&link).unwrap().save("a = 2\n").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "a = 2\n");
        assert_eq!(
            lock_path(&link),
            dir.path().canonicalize().unwrap().join("real.toml.lock")
        );
    }

    #[test]
    fn load_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default.toml");
        assert!(matches!(load(&path), Err(FileError::NotFound(_))));
        fs::write(&path, b"a = \"\xff\"\n").unwrap();
        assert!(matches!(load(&path), Err(FileError::NotUtf8(_))));
    }

    #[test]
    fn create_respects_force() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/cadrat/default.toml");
        create(&path, "a = 1\n", false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\n");
        assert!(matches!(
            create(&path, "a = 2\n", false),
            Err(FileError::AlreadyExists(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 1\n");
        create(&path, "a = 2\n", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a = 2\n");
    }
}
