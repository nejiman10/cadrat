//! The human-readable output is built from the `--json` object alone
//! (spec tool/cli §5, P11): rendering the JSON of a run gives exactly what the
//! same run printed, so `cadratctl` can show `cadrat-tool`'s output.

#![allow(missing_docs)]

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use cadrat_command::render::{self, Human};
use cadrat_command::{Command, Frontend, Options};
use cadrat_config::{Preset, template};
use cadrat_hidraw::fake::{FakeClock, FakeNode, FakeSystem};
use cadrat_hidraw::receiver::Polling;
use cadrat_proto::Slot;
use cadrat_tool::{Env, Interrupt, Io, run};
use serde_json::Value;

// Synthetic device IDs; not taken from any device.
const A: [u8; 6] = [0x0a, 0x1b, 0x2c, 0x3d, 0x4e, 0x5f];
const B: [u8; 6] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

struct NoInterrupt;

impl Interrupt for NoInterrupt {
    fn arm(&self) {}
    fn disarm(&self) {}
    fn is_set(&self) -> bool {
        false
    }
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

/// One run in a fresh fake world, so a human run and a `--json` run see the
/// same devices and file.
fn run_fresh(nodes: Vec<FakeNode>, config: Option<&str>, args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    if let Some(text) = config {
        fs::create_dir_all(dir.path().join("cadrat")).unwrap();
        fs::write(dir.path().join("cadrat/default.toml"), text).unwrap();
    }
    let system = FakeSystem::new(nodes);
    let clock = FakeClock::default();
    let env = Env {
        system: &system,
        clock: &clock,
        interrupt: &NoInterrupt,
        xdg_config_home: Some(dir.path().into()),
        home: None,
        lock_timeout: Duration::from_millis(100),
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut input: &[u8] = b"";
    let io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
        stdin: &mut input,
        stdin_is_terminal: false,
    };
    let args = std::iter::once("cadrat-tool")
        .chain(args.iter().copied())
        .map(Into::into);
    let code = run(args, &env, io);
    let normalize = |bytes: Vec<u8>| {
        String::from_utf8(bytes)
            .unwrap()
            .replace(dir.path().to_str().unwrap(), "<dir>")
    };
    Output {
        code,
        stdout: normalize(stdout),
        stderr: normalize(stderr),
    }
}

fn joined(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    if !lines.is_empty() {
        text.push('\n');
    }
    text
}

/// Runs `args` without and with `--json` and checks that rendering the JSON
/// gives the human run's stdout, and the end of its stderr (warnings and
/// prompts come earlier, while the command runs).
fn same_output(
    nodes: impl Fn() -> Vec<FakeNode>,
    config: Option<&str>,
    args: &[&str],
    options: &Options,
    command: &Command,
) -> Value {
    let human = run_fresh(nodes(), config, args);
    let json_args: Vec<&str> = args.iter().copied().chain(["--json"]).collect();
    let json_run = run_fresh(nodes(), config, &json_args);
    let json: Value = serde_json::from_str(&json_run.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", json_run.stdout));
    assert_eq!(human.code, json_run.code, "{args:?}");
    assert_eq!(json["exit_code"], human.code, "{args:?}");

    let rendered = render::human("cadrat-tool", options, command, &json);
    assert_eq!(joined(&rendered.stdout), human.stdout, "{args:?}");
    assert!(
        human.stderr.ends_with(&joined(&rendered.stderr)),
        "{args:?}\nrendered: {:?}\nprinted: {}",
        rendered.stderr,
        human.stderr
    );
    json
}

fn baseline() -> String {
    template(Preset::ResearchBaseline)
}

fn wired() -> FakeNode {
    FakeNode::wired("hidraw5", "3-2", A)
}

fn management(slots: [&'static [Option<[u8; 6]>]; 5]) -> FakeNode {
    FakeNode::receiver("hidraw6", "1-4", 0, None, true).slots(slots)
}

fn two_routes() -> Vec<FakeNode> {
    vec![
        wired(),
        management([&[None], &[None], &[None], &[Some(A)], &[None]]),
        FakeNode::receiver("hidraw9", "1-4", 3, Some(A), true),
    ]
}

fn receiver_only() -> Vec<FakeNode> {
    let mut nodes = two_routes();
    nodes.remove(0);
    nodes
}

fn quiet() -> Options {
    Options {
        quiet: true,
        ..Options::default()
    }
}

fn polling(timeout: u64, interval_ms: u64) -> Polling {
    Polling {
        timeout: Duration::from_secs(timeout),
        interval: Duration::from_millis(interval_ms),
    }
}

#[test]
fn list() {
    for (nodes, redact) in [(false, false), (true, true)] {
        let mut args = vec!["list"];
        if nodes {
            args.push("--nodes");
        }
        if redact {
            args.push("--redact");
        }
        same_output(
            two_routes,
            None,
            &args,
            &Options::default(),
            &Command::List { nodes, redact },
        );
    }
    same_output(
        Vec::new,
        None,
        &["list"],
        &Options::default(),
        &Command::List {
            nodes: false,
            redact: false,
        },
    );
}

#[test]
fn init_get_check() {
    for preset in [Preset::Empty, Preset::ResearchBaseline] {
        let args: &[&str] = match preset {
            Preset::Empty => &["init"],
            Preset::ResearchBaseline => &["init", "--preset=research-baseline"],
        };
        same_output(
            Vec::new,
            None,
            args,
            &Options::default(),
            &Command::Init {
                preset,
                force: false,
            },
        );
    }
    // Already exists: error with a hint.
    same_output(
        Vec::new,
        Some(&baseline()),
        &["init"],
        &Options::default(),
        &Command::Init {
            preset: Preset::Empty,
            force: false,
        },
    );

    let get = |keys: &[&str], wire, values_only| Command::Get {
        keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        wire,
        values_only,
    };
    let config = baseline();
    same_output(
        Vec::new,
        Some(&config),
        &["get"],
        &Options::default(),
        &get(&[], false, false),
    );
    same_output(
        Vec::new,
        Some(&config),
        &["get", "buttons.back", "mouse.dpi"],
        &Options::default(),
        &get(&["buttons.back", "mouse.dpi"], false, false),
    );
    same_output(
        Vec::new,
        Some(&config),
        &["get", "-n", "mouse.dpi"],
        &Options::default(),
        &get(&["mouse.dpi"], false, true),
    );
    same_output(
        Vec::new,
        Some(&config),
        &["get", "--wire"],
        &Options::default(),
        &get(&[], true, false),
    );
    // Incomplete: the values it could read, then the error.
    same_output(
        Vec::new,
        Some("schema = 1\n[mouse]\ndpi = 800\n"),
        &["get"],
        &Options::default(),
        &get(&[], false, false),
    );

    same_output(
        Vec::new,
        Some(&config),
        &["check"],
        &Options::default(),
        &Command::Check,
    );
    same_output(
        Vec::new,
        None,
        &["check"],
        &Options::default(),
        &Command::Check,
    );
}

#[test]
fn set_and_apply() {
    let config = baseline();
    let set = |assignments: &[&str], dry_run, no_save| Command::Set {
        assignments: assignments.iter().map(|a| (*a).to_owned()).collect(),
        dry_run,
        no_save,
    };
    // Standby note.
    same_output(
        two_routes,
        Some(&config),
        &["set", "mouse.dpi=1000", "buttons.radial=host:1"],
        &Options::default(),
        &set(&["mouse.dpi=1000", "buttons.radial=host:1"], false, false),
    );
    same_output(
        two_routes,
        Some(&config),
        &["set", "-q", "mouse.dpi=1000"],
        &quiet(),
        &set(&["mouse.dpi=1000"], false, false),
    );
    same_output(
        two_routes,
        Some(&config),
        &["set", "--dry-run", "mouse.dpi=1000", "mouse.dpi=1400"],
        &Options::default(),
        &set(&["mouse.dpi=1000", "mouse.dpi=1400"], true, false),
    );
    same_output(
        two_routes,
        Some(&config),
        &["set", "--no-save", "mouse.dpi=1000"],
        &Options::default(),
        &set(&["mouse.dpi=1000"], false, true),
    );
    // Receiver note.
    same_output(
        receiver_only,
        Some(&config),
        &["apply"],
        &Options::default(),
        &Command::Apply { dry_run: false },
    );
    same_output(
        receiver_only,
        Some(&config),
        &["apply", "--dry-run"],
        &Options::default(),
        &Command::Apply { dry_run: true },
    );
    // A send failure after the mouse line.
    same_output(
        || vec![wired().set(&[Err(5)])],
        Some(&config),
        &["apply"],
        &Options::default(),
        &Command::Apply { dry_run: false },
    );
}

#[test]
fn receiver() {
    for redact in [false, true] {
        let args: &[&str] = if redact {
            &["receiver", "slots", "--redact"]
        } else {
            &["receiver", "slots"]
        };
        same_output(
            two_routes,
            None,
            args,
            &Options::default(),
            &Command::ReceiverSlots {
                receiver: None,
                redact,
            },
        );
    }

    let new_slot = || {
        vec![management([
            &[None],
            &[None],
            &[None, None, None, None, Some(B)],
            &[None],
            &[None],
        ])]
    };
    same_output(
        new_slot,
        None,
        &["receiver", "pair"],
        &Options::default(),
        &Command::ReceiverPair {
            receiver: None,
            polling: polling(60, 1000),
        },
    );
    same_output(
        || vec![management([&[None]; 5])],
        None,
        &["receiver", "pair", "--timeout=3"],
        &Options::default(),
        &Command::ReceiverPair {
            receiver: None,
            polling: polling(3, 1000),
        },
    );

    let occupied_slot3 = || {
        vec![management([
            &[None],
            &[None],
            &[None],
            &[Some(A), Some(A), Some(A), None],
            &[None],
        ])]
    };
    same_output(
        occupied_slot3,
        None,
        &["receiver", "unpair", "3", "--yes"],
        &Options::default(),
        &Command::ReceiverUnpair {
            receiver: None,
            slot: Slot::new(3).unwrap(),
            yes: true,
            polling: polling(15, 500),
        },
    );
}

#[test]
fn slots_carry_the_raw_response_unless_redacted() {
    let raw = |args: &[&str]| -> Vec<Value> {
        let out = run_fresh(two_routes(), None, args);
        let json: Value = serde_json::from_str(&out.stdout).unwrap();
        json["slots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["raw_hex"].clone())
            .collect()
    };
    let slots = raw(&["receiver", "slots", "--json"]);
    assert_eq!(slots.len(), 5);
    for slot in &slots {
        // The whole GET 0x44 response as unspaced lowercase hex.
        let hex = slot.as_str().unwrap();
        assert!(hex.len() >= 2 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }
    assert!(slots[3].as_str().unwrap().contains(&hex(&A)));
    assert!(slots[0] != slots[3]);

    let redacted = raw(&["receiver", "slots", "--json", "--redact"]);
    assert!(redacted.iter().all(Value::is_null));
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| [b >> 4, b & 0xf])
        .map(|n| char::from_digit(u32::from(n), 16).unwrap())
        .collect()
}

/// Records what `execute` shows while it runs.
#[derive(Default)]
struct Recorder {
    warnings: Vec<String>,
}

impl Frontend for Recorder {
    fn verbose(&mut self, _: &str) {}
    fn warning(&mut self, code: &str, _: &str) {
        self.warnings.push(code.to_owned());
    }
    fn pairing_started(&mut self, _: Duration) {}
    fn can_confirm(&self) -> bool {
        false
    }
    fn unpair_target(&mut self, _: &[String]) {}
    fn confirm_unpair(&mut self, _: Slot) -> bool {
        false
    }
}

#[test]
fn guidance_names_the_front_end() {
    let dir = tempfile::tempdir().unwrap();
    let system = FakeSystem::new(two_routes());
    let clock = FakeClock::default();
    let env = Env {
        system: &system,
        clock: &clock,
        interrupt: &NoInterrupt,
        xdg_config_home: Some(dir.path().into()),
        home: None,
        lock_timeout: Duration::from_millis(100),
    };
    let options = Options::default();

    // A hint made while the command runs.
    let json = cadrat_command::execute(
        &env,
        &mut Recorder::default(),
        &options,
        &Command::Check,
        "cadratctl",
    );
    assert_eq!(json["exit_code"], 3);
    let hints = json["error"]["hints"].to_string();
    assert!(hints.contains("`cadratctl init`"), "{hints}");
    assert!(!hints.contains("cadrat-tool"), "{hints}");

    // Lines made by rendering.
    let config: PathBuf = dir.path().join("cadrat/default.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, baseline()).unwrap();
    let command = Command::Apply { dry_run: false };
    let mut recorder = Recorder::default();
    let json = cadrat_command::execute(&env, &mut recorder, &options, &command, "cadratctl");
    assert_eq!(json["exit_code"], 0, "{json}");
    let Human { stdout, .. } = render::human("cadratctl", &options, &command, &json);
    let stdout = joined(&stdout);
    assert!(
        stdout.contains("run `cadratctl apply` after switching modes"),
        "{stdout}"
    );
    assert!(!stdout.contains("cadrat-tool"), "{stdout}");
}
