//! Build helpers.
//!
//! `cargo run -p xtask -- dist` writes the manual pages and shell completions
//! that the `.deb` package installs (spec implementation §7) to `target/dist/`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap_complete::Shell;
use flate2::{Compression, GzBuilder};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["dist"] {
        eprintln!("usage: cargo run -p xtask -- dist");
        return ExitCode::from(2);
    }
    match dist(&workspace_root().join("target/dist")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_owned()
}

fn dist(out: &Path) -> io::Result<()> {
    let man = out.join("man");
    let completions = out.join("completions");
    for dir in [&man, &completions] {
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        fs::create_dir_all(dir)?;
    }

    let command = cadrat_tool::command();
    write_man_pages(&command, "", &man)?;

    for (shell, file) in [
        (Shell::Bash, "cadrat-tool"),
        (Shell::Zsh, "_cadrat-tool"),
        (Shell::Fish, "cadrat-tool.fish"),
    ] {
        let mut buffer = Vec::new();
        clap_complete::generate(
            shell,
            &mut cadrat_tool::command(),
            "cadrat-tool",
            &mut buffer,
        );
        fs::write(completions.join(file), buffer)?;
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// One page for the command and one per subcommand, e.g.
/// `cadrat-tool-receiver-pair.1.gz`, gzip-compressed without a timestamp so
/// that builds are reproducible.
fn write_man_pages(command: &clap::Command, prefix: &str, dir: &Path) -> io::Result<()> {
    let name = if prefix.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{prefix}-{}", command.get_name())
    };
    let page = command.clone().name(name.clone());
    let mut roff = Vec::new();
    clap_mangen::Man::new(page).render(&mut roff)?;
    let file = File::create(dir.join(format!("{name}.1.gz")))?;
    let mut gz = GzBuilder::new().mtime(0).write(file, Compression::best());
    gz.write_all(&roff)?;
    gz.finish()?;
    for sub in command.get_subcommands().filter(|s| s.get_name() != "help") {
        write_man_pages(sub, &name, dir)?;
    }
    Ok(())
}
