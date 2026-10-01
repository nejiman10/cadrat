//! Build helpers.
//!
//! `cargo run -p xtask -- dist` writes the manual pages and shell completions
//! that the `.deb` packages install (spec implementation §7) to `target/dist/`:
//! `man/man1/` and `man/man8/` for the pages, `completions/` for bash, zsh
//! and fish.

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

    // Commands people type: section 1, a page per subcommand, completions.
    for command in [cadrat_tool::command(), cadratctl::command()] {
        write_man_pages(&command, "", "1", &man)?;
        let name = command.get_name().to_owned();
        for (shell, file) in [
            (Shell::Bash, name.clone()),
            (Shell::Zsh, format!("_{name}")),
            (Shell::Fish, format!("{name}.fish")),
        ] {
            let mut buffer = Vec::new();
            clap_complete::generate(shell, &mut command.clone(), &name, &mut buffer);
            fs::write(completions.join(file), buffer)?;
        }
    }
    // The daemon and the system service: section 8, one page each.
    for command in [cadratd::command(), cadrat_hold_open::command()] {
        write_man_pages(&command, "", "8", &man)?;
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// One page for the command and one per subcommand, e.g.
/// `man1/cadrat-tool-receiver-pair.1.gz`, gzip-compressed without a
/// timestamp so that builds are reproducible.
fn write_man_pages(
    command: &clap::Command,
    prefix: &str,
    section: &str,
    man: &Path,
) -> io::Result<()> {
    let name = if prefix.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{prefix}-{}", command.get_name())
    };
    let page = command.clone().name(name.clone());
    let mut roff = Vec::new();
    clap_mangen::Man::new(page)
        .section(section.to_owned())
        .render(&mut roff)?;
    let dir = man.join(format!("man{section}"));
    fs::create_dir_all(&dir)?;
    let file = File::create(dir.join(format!("{name}.{section}.gz")))?;
    let mut gz = GzBuilder::new().mtime(0).write(file, Compression::best());
    gz.write_all(&roff)?;
    gz.finish()?;
    for sub in command.get_subcommands().filter(|s| s.get_name() != "help") {
        write_man_pages(sub, &name, section, man)?;
    }
    Ok(())
}
