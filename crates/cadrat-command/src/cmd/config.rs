//! `init`, `get` and `check`, and reading the configuration file.

use std::path::Path;

use cadrat_config::file::{self, Loaded};
use cadrat_config::{Checked, Config, ConfigError, ConfigLock, Document, Key, Preset};
use serde_json::{Map, Value, json};

use crate::ctx::Ctx;
use crate::format;
use crate::{Exit, Failure};

/// Reads and parses the configuration file (spec tool/cli §4 step 3).
pub fn read(path: &Path, program: &str) -> Result<(Loaded, Document), Failure> {
    let loaded = file::load(path).map_err(|e| {
        let missing = matches!(e, cadrat_config::FileError::NotFound(_));
        let failure = Failure::from(e);
        if missing {
            failure.hint(format!("create it with `{program} init`"))
        } else {
            failure
        }
    })?;
    let document = Document::parse(&loaded.text).map_err(|e| {
        Failure::new(
            Exit::ConfigError,
            format!("{} is not valid TOML: {e}", path.display()),
        )
    })?;
    Ok((loaded, document))
}

/// A validation failure listing every problem (spec config §2.1).
pub fn invalid(path: &Path, error: &ConfigError) -> Failure {
    let (kind, summary) = match error {
        ConfigError::Incomplete(_) => ("ConfigIncomplete", "is incomplete"),
        ConfigError::Invalid(_) => ("ConfigInvalid", "is invalid"),
    };
    let problems: Vec<String> = error.problems().iter().map(ToString::to_string).collect();
    let mut failure = Failure::new(Exit::ConfigError, format!("{} {summary}", path.display()))
        .details(json!({ "kind": kind, "problems": problems }));
    for problem in problems {
        failure = failure.hint(problem);
    }
    failure
}

/// Reads and validates the configuration file (steps 3–4).
pub fn load(path: &Path, program: &str) -> Result<(Loaded, Document, Config), Failure> {
    let (loaded, document) = read(path, program)?;
    let config = document.config().map_err(|e| invalid(path, &e))?;
    Ok((loaded, document, config))
}

/// Prints the configuration warnings (spec config §4.4, §5).
pub fn warn_config(ctx: &mut Ctx, config: &Config) {
    for warning in config.warnings() {
        ctx.warn(warning.code(), warning.to_string());
    }
}

pub fn init(ctx: &mut Ctx, preset: Preset, force: bool) -> Result<(), Failure> {
    let path = ctx.config_path()?;
    // Serialize with a concurrent `set` once the directory exists.
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| {
            Failure::new(
                Exit::IoError,
                format!("cannot create {}: {e}", dir.display()),
            )
        })?;
    }
    let _lock = ConfigLock::acquire(&path, ctx.env.lock_timeout)?;
    file::create(&path, &cadrat_config::template(preset), force).map_err(|e| {
        let exists = matches!(e, cadrat_config::FileError::AlreadyExists(_));
        let failure = Failure::from(e);
        if exists {
            failure.hint("use --force to overwrite it")
        } else {
            failure
        }
    })?;
    ctx.set("path", path.display().to_string());
    ctx.set(
        "preset",
        match preset {
            Preset::Empty => Value::Null,
            Preset::ResearchBaseline => "research-baseline".into(),
        },
    );
    Ok(())
}

pub fn get(ctx: &mut Ctx, keys: &[String], wire: bool) -> Result<(), Failure> {
    let keys: Vec<Key> = if keys.is_empty() {
        Key::ALL.to_vec()
    } else {
        keys.iter()
            .map(|k| {
                k.parse::<Key>()
                    .map_err(|e| Failure::new(Exit::Usage, e.to_string()))
            })
            .collect::<Result<_, _>>()?
    };
    let path = ctx.config_path()?;
    let (_, document) = read(&path, ctx.program)?;
    let Checked { values, problems } = document.check();

    let mut shown = Map::new();
    for key in &keys {
        if let Some(value) = values[key.index()] {
            shown.insert(key.to_string(), format::value_json(value));
        }
    }
    ctx.set("values", Value::Object(shown));

    let checked = Checked { values, problems };
    let config = checked.into_config().map_err(|e| invalid(&path, &e))?;
    if wire {
        let report = config.to_report().to_wire();
        ctx.set("wire_hex", format::hex(&report).replace(' ', ""));
        ctx.set("wire_layout", format::wire_layout_json(&config, &report));
    }
    Ok(())
}

pub fn check(ctx: &mut Ctx) -> Result<(), Failure> {
    let path = ctx.config_path()?;
    let (_, _, config) = load(&path, ctx.program)?;
    warn_config(ctx, &config);
    ctx.set("path", path.display().to_string());
    Ok(())
}
