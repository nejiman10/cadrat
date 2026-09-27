//! `init`, `get` and `check`, and reading the configuration file.

use std::path::Path;

use cadrat_config::file::{self, Loaded};
use cadrat_config::{Checked, Config, ConfigError, ConfigLock, Document, Key, Preset};
use serde_json::{Map, Value, json};

use crate::cli;
use crate::ctx::Ctx;
use crate::exit::{Exit, Failure};
use crate::render;

/// Reads and parses the configuration file (spec 03 §4 step 3).
pub fn read(path: &Path) -> Result<(Loaded, Document), Failure> {
    let loaded = file::load(path).map_err(|e| {
        let missing = matches!(e, cadrat_config::FileError::NotFound(_));
        let failure = Failure::from(e);
        if missing {
            failure.hint("create it with `cadrat-tool init`")
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

/// A validation failure listing every problem (spec 01 §2.1).
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
pub fn load(path: &Path) -> Result<(Loaded, Document, Config), Failure> {
    let (loaded, document) = read(path)?;
    let config = document.config().map_err(|e| invalid(path, &e))?;
    Ok((loaded, document, config))
}

/// Prints the configuration warnings (spec 01 §4.4, §5).
pub fn warn_config(ctx: &mut Ctx, config: &Config) {
    for warning in config.warnings() {
        ctx.warn(warning.code(), warning.to_string());
    }
}

pub fn init(ctx: &mut Ctx, preset: Option<cli::Preset>, force: bool) -> Result<(), Failure> {
    let path = ctx.config_path()?;
    let preset = match preset {
        Some(cli::Preset::ResearchBaseline) => Preset::ResearchBaseline,
        None => Preset::Empty,
    };
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
    ctx.info(format!("created {}", path.display()));
    if preset == Preset::Empty {
        ctx.note("every value is commented out; edit the file before sending");
    } else {
        ctx.note("these values are a research baseline, not values read from the mouse");
    }
    Ok(())
}

pub fn get(ctx: &mut Ctx, keys: &[String], wire: bool, values_only: bool) -> Result<(), Failure> {
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
    let (_, document) = read(&path)?;
    let Checked { values, problems } = document.check();

    let mut shown = Map::new();
    for key in &keys {
        if let Some(value) = values[key.index()] {
            shown.insert(key.to_string(), render::value_json(value));
            ctx.out(if values_only {
                value.to_string()
            } else {
                format!("{key}={value}")
            });
        }
    }
    ctx.set("values", Value::Object(shown));

    let checked = Checked { values, problems };
    let config = checked.into_config().map_err(|e| invalid(&path, &e))?;
    if wire {
        let report = config.to_report().to_wire();
        ctx.set("wire_hex", render::hex(&report).replace(' ', ""));
        ctx.set(
            "wire_layout",
            render::wire_layout(&config, &report)
                .into_iter()
                .map(|(offset, bytes, meaning)| json!({"offset": offset, "bytes": bytes, "meaning": meaning}))
                .collect::<Vec<_>>(),
        );
        for line in render::wire_lines(&config, &report) {
            ctx.out(line);
        }
    }
    Ok(())
}

pub fn check(ctx: &mut Ctx) -> Result<(), Failure> {
    let path = ctx.config_path()?;
    let (_, _, config) = load(&path)?;
    warn_config(ctx, &config);
    ctx.set("path", path.display().to_string());
    ctx.info(format!("ok: {}", path.display()));
    Ok(())
}
