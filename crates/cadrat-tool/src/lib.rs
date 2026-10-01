//! `cadrat-tool`: the stand-alone configuration tool (spec tool/cli).
//!
//! [`run`] takes the arguments, the outside world ([`Env`]) and the standard
//! streams ([`Io`]), so the CLI tests run the whole program against the fake
//! transport of `cadrat-hidraw` (spec implementation §4). The binary only wires the real
//! implementations in. The commands themselves are in `cadrat-command`,
//! shared with `cadratd`.

// `expect` is used only where clap or this crate already guarantees the value.
#![allow(clippy::missing_panics_doc)]

mod cli;
mod hold;

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::time::Duration;

use cadrat_command::{Failure, Frontend, render};
use clap::Parser;

pub use cadrat_command::{Env, Exit, Interrupt};

use cli::{Cli, Command};

/// Standard streams.
pub struct Io<'a> {
    /// Results.
    pub stdout: &'a mut dyn Write,
    /// Warnings, errors and prompts.
    pub stderr: &'a mut dyn Write,
    /// Answers to the unpair confirmation.
    pub stdin: &'a mut dyn BufRead,
    /// Whether stdin is a terminal.
    pub stdin_is_terminal: bool,
}

/// The command-line definition, for generating the manual page and shell
/// completions.
#[must_use]
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

const PROGRAM: &str = "cadrat-tool";

/// Runs `cadrat-tool` and returns the exit code.
// Output is best effort: a closed stdout or stderr must not turn a
// completed send into a failure.
pub fn run(args: impl IntoIterator<Item = OsString>, env: &Env, io: Io) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            use clap::error::ErrorKind;
            let usage = !matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            );
            let rendered = error.render().to_string();
            let _ = if usage {
                write!(io.stderr, "{rendered}")
            } else {
                write!(io.stdout, "{rendered}")
            };
            if !usage {
                return Exit::Success.code();
            }
            if args.iter().any(|a| a == "--json") {
                let failure = Failure::new(Exit::Usage, error.kind().to_string());
                let json = cadrat_command::envelope(
                    None,
                    serde_json::Map::new(),
                    Vec::new(),
                    &Err(failure),
                );
                let _ = writeln!(io.stdout, "{json}");
            }
            return Exit::Usage.code();
        }
    };
    let Some(request) = cli.command.request() else {
        let Command::HoldOpen { poll_interval } = cli.command else {
            unreachable!("only hold-open has no shared command");
        };
        return hold::run(env, io, &cli.global, poll_interval);
    };
    let options = cli.global.options();
    let mut frontend = Terminal {
        stderr: &mut *io.stderr,
        stdin: &mut *io.stdin,
        interactive: io.stdin_is_terminal,
        quiet: options.quiet,
        json: cli.global.json,
    };
    let result = cadrat_command::execute(env, &mut frontend, &options, &request, PROGRAM);
    let human = render::human(PROGRAM, &options, &request, &result);
    if cli.global.json {
        let _ = writeln!(io.stdout, "{result}");
    } else {
        for line in &human.stdout {
            let _ = writeln!(io.stdout, "{line}");
        }
    }
    for line in &human.stderr {
        let _ = writeln!(io.stderr, "{line}");
    }
    result["exit_code"]
        .as_i64()
        .and_then(|code| i32::try_from(code).ok())
        .unwrap_or(Exit::Internal.code())
}

/// Shows what happens while a command runs on the terminal's standard error.
struct Terminal<'a> {
    stderr: &'a mut dyn Write,
    stdin: &'a mut dyn BufRead,
    interactive: bool,
    quiet: bool,
    json: bool,
}

impl Frontend for Terminal<'_> {
    fn verbose(&mut self, line: &str) {
        let _ = writeln!(self.stderr, "{line}");
    }

    fn warning(&mut self, code: &str, message: &str) {
        let _ = writeln!(self.stderr, "warning: {code}: {message}");
    }

    fn pairing_started(&mut self, timeout: Duration) {
        if !self.quiet {
            let _ = writeln!(
                self.stderr,
                "put the mouse in pairing mode now (waiting up to {} s; Ctrl-C stops pairing)",
                timeout.as_secs_f64()
            );
        }
    }

    fn can_confirm(&self) -> bool {
        !self.json && self.interactive
    }

    fn unpair_target(&mut self, lines: &[String]) {
        for line in lines {
            let _ = writeln!(self.stderr, "{line}");
        }
    }

    fn confirm_unpair(&mut self, slot: cadrat_proto::Slot) -> bool {
        let _ = write!(self.stderr, "Unpair slot {slot}? [y/N] ");
        let _ = self.stderr.flush();
        let mut answer = String::new();
        if self.stdin.read_line(&mut answer).is_err() {
            return false;
        }
        matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
    }
}
