//! The file side of `set` (spec 03 §4 steps 2–5 and 9–10), on a real file.

#![allow(missing_docs)]

use std::fs;

use cadrat_config::file::{self, FileError};
use cadrat_config::{ConfigLock, Document, LOCK_TIMEOUT, Preset, parse_assignments, template};

fn set(
    path: &std::path::Path,
    args: &[&str],
    edit_before_save: Option<&str>,
) -> Result<(), FileError> {
    let _lock = ConfigLock::acquire(path, LOCK_TIMEOUT)?;
    let loaded = file::load(path)?;
    let mut doc = Document::parse(&loaded.text).unwrap();
    let config = doc.config().unwrap();
    let (next, changes) = config.apply(&parse_assignments(args).unwrap()).unwrap();
    let _wire = next.to_report().to_wire();
    // Sending would happen here.
    if let Some(text) = edit_before_save {
        fs::write(path, text).unwrap();
    }
    doc.update(&changes).unwrap();
    loaded.save(&doc.to_string())
}

#[test]
fn set_saves_only_the_changed_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cadrat/default.toml");
    file::create(&path, &template(Preset::ResearchBaseline), false).unwrap();
    let before = fs::read_to_string(&path).unwrap();

    set(&path, &["mouse.dpi=1000", "buttons.radial=host:1"], None).unwrap();

    let after = fs::read_to_string(&path).unwrap();
    let expected = before.replacen("dpi = 1400 ", "dpi = 1000 ", 1).replacen(
        "radial  = \"mouse:middle\"",
        "radial  = \"host:1\"",
        1,
    );
    assert_eq!(after, expected);
    let config = Document::parse(&after).unwrap().config().unwrap();
    assert_eq!(config.dpi.get(), 1000);
}

#[test]
fn concurrent_edit_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default.toml");
    file::create(&path, &template(Preset::ResearchBaseline), false).unwrap();
    let edited = template(Preset::ResearchBaseline) + "# edited in an editor\n";

    let err = set(&path, &["mouse.dpi=1000"], Some(&edited)).unwrap_err();
    assert!(matches!(err, FileError::Changed(_)), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn set_waits_for_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default.toml");
    file::create(&path, &template(Preset::ResearchBaseline), false).unwrap();
    let _held = ConfigLock::acquire(&path, LOCK_TIMEOUT).unwrap();
    let err = ConfigLock::acquire(&path, std::time::Duration::from_millis(50)).unwrap_err();
    assert!(matches!(err, FileError::Locked(_)), "{err}");
}
