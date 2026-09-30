# Repository instructions

## Scope

These instructions apply to the entire repository. Keep this file timeless: do not record current status, pending work, test results, device-specific session information, local paths or environment details. Unfinished work and its completion criteria belong in `TODO.md`. The next item and brief restart notes belong in `HANDOFF.md`.

## Authority

- The current Git repository state is the source of truth. Chat history, model memory and external summaries are not project facts.
- `docs/spec/` is the specification of record. Implementation follows the specification; when they disagree, fix the specification first (or in the same change) and never let code silently diverge.
- Device and protocol facts are owned by the research repository `nejiman10/3dx-hid-research` (`SPEC.md`). This repository cites them with the research commit it relied on and keeps their evidence labels (`CONFIRMED`, `OBSERVED`, `HYPOTHESIS`, `UNKNOWN`). Do not upgrade the certainty of a research claim here.
- New device behavior discovered while working here is reported to the research repository for investigation. It is not recorded here as a protocol fact.

## Branches

- `main` holds the released state. Each release is tagged on `main`.
- Development happens on `develop`. Work branches start from `develop` and return to it through pull requests.
- `develop` is merged into `main` with a merge commit (not a squash) when a release is ready, after its hardware checks pass.
- A fix for a released version starts from `main`, is merged into `main`, and is then merged into `develop`.
- `TODO.md` and `HANDOFF.md` on `develop` are the current ones.

## Documentation layout

- `docs/spec/` holds documents shared by all executables at its top level, and documents for a single executable in a directory named after it (`tool/`, `hold-open/`, `daemon/`, `ctl/`).
- The specification has one version and one open-question table for the whole project.

## Design rules

- `cadrat-tool` is stateless: the TOML file is its only state. It never uses the research baseline as an implicit default, never guesses between multiple targets, and saves the TOML only after a successful send.
- `cadratctl` is only a D-Bus front end for `cadratd`; it never opens hidraw. The `ctl` suffix is reserved for daemon front ends.
- Shared logic lives in library crates (`cadrat-proto`, `cadrat-hidraw`, `cadrat-config`) so that `cadrat-tool` and `cadratd` behave identically.
- `cadrat-proto` performs no I/O.
- While `cadratd` runs, only `cadratd` writes to devices; `cadrat-tool` refuses its device-writing commands. Reading commands stay available.
- `cadratd`, `cadratctl` and `cadrat-tool` give the same result, exit code and output for the same command.
- Hold-open is done by the `cadrat-hold-open` system service, independent of login and of `cadratd`.

## Safety

- Commands that write device settings or change receiver pairing alter real hardware. Hardware tests require an explicit instruction from the owner, a recorded restore value, and a written procedure.
- Device identifiers (slot identifiers, GET `0x08` bytes 2..7) must not appear in committed files, test fixtures or public logs. Record only equality or inequality when needed.

## Specification changes

- Update the version and date in `docs/spec/README.md` when the specification changes.
- Keep open questions in the table in `docs/spec/implementation.md` with their state and current handling.
- When the referenced research commit changes, review every cited claim and update the commit reference.
