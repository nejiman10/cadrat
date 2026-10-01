//! Receiver management: slot reads, pair and unpair (spec receiver).
//!
//! Pair and unpair are judged only by slot snapshots before and after, never
//! by the return value of the SET (P7).

use std::time::Duration;

use cadrat_proto::receiver::{PAIR_START, PAIR_STOP, PAIRING_REPORT_LEN, SLOT_REPORT_LEN};
use cadrat_proto::{Slot, SlotError, SlotReport};

use crate::model::Warning;
use crate::sys::{Clock, Device, Errno};

/// A slot read failed (`ReceiverProtocolError` for malformed replies).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SlotReadError {
    /// The GET failed.
    #[error("reading slot {slot} failed: {errno}")]
    Io {
        /// The slot.
        slot: Slot,
        /// The error.
        errno: Errno,
    },
    /// The reply had the wrong length or Report ID.
    #[error(transparent)]
    Protocol(#[from] SlotError),
}

/// Reads one slot (spec receiver §2).
///
/// # Errors
///
/// See [`SlotReadError`].
pub fn read_slot(device: &mut dyn Device, slot: Slot) -> Result<SlotReport, SlotReadError> {
    let response = device
        .get_feature(slot.report_id(), SLOT_REPORT_LEN)
        .map_err(|e| SlotReadError::Io {
            slot,
            errno: Errno::of(&e),
        })?;
    Ok(SlotReport::parse(slot, &response)?)
}

/// Reads slots 0..4.
///
/// # Errors
///
/// The first failing slot.
pub fn read_slots(device: &mut dyn Device) -> Result<[SlotReport; 5], SlotReadError> {
    let reports = Slot::ALL
        .iter()
        .map(|&slot| read_slot(device, slot))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(reports.try_into().expect("five slots"))
}

/// Outcome of one SET to the pairing report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetResult {
    /// The ioctl returned the declared length.
    Ok,
    /// The ioctl returned another length.
    Short(usize),
    /// The ioctl failed.
    Errno(Errno),
}

impl std::fmt::Display for SetResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => f.write_str("ok"),
            Self::Short(n) => write!(f, "short-write: {n}"),
            Self::Errno(errno) => write!(f, "errno: {errno}"),
        }
    }
}

fn set(device: &mut dyn Device, packet: [u8; PAIRING_REPORT_LEN]) -> SetResult {
    match device.set_feature(&packet) {
        Ok(n) if n == PAIRING_REPORT_LEN => SetResult::Ok,
        Ok(n) => SetResult::Short(n),
        Err(e) => SetResult::Errno(Errno::of(&e)),
    }
}

/// The management node a pair or unpair runs on.
///
/// The descriptor stays open for the whole procedure, but the Receiver has
/// been seen to drop the node after a slot changes (every GET then fails
/// with `ENODEV`). [`Link::reopen`] finds the same Receiver's management node
/// again so the procedure can continue (spec receiver §3, §4).
pub trait Link {
    /// The open management node.
    fn device(&mut self) -> &mut dyn Device;

    /// Chooses and opens the management node again, by the same rules.
    /// If the old node was locked, the new one is locked again; failing to
    /// lock it does not fail the reopen, so the stop packet can still be
    /// sent (spec device §7.2).
    ///
    /// # Errors
    ///
    /// Why no management node could be opened (yet).
    fn reopen(&mut self) -> Result<(), String>;

    /// Takes the write lock on the management node (spec device §7.2).
    ///
    /// # Errors
    ///
    /// The `flock` error; `EWOULDBLOCK` when another process holds it.
    fn lock(&mut self) -> std::io::Result<()> {
        self.device().lock()
    }
}

/// A link that cannot reopen: for a single node without a system to
/// enumerate.
pub struct Fixed<'a>(pub &'a mut dyn Device);

impl Link for Fixed<'_> {
    fn device(&mut self) -> &mut dyn Device {
        &mut *self.0
    }

    fn reopen(&mut self) -> Result<(), String> {
        Err("this node cannot be reopened".to_owned())
    }
}

/// How long to keep trying to send the stop packet after the management
/// node disappeared.
pub const STOP_RETRY: Duration = Duration::from_secs(10);

/// What happened to reads and the node while waiting.
///
/// Slot reads that fail with an errno are retried until the timeout (right
/// after an unpair request the Receiver has answered a slot GET with `EPIPE`
/// once). When the errno says the node is gone, the node is reopened first.
#[derive(Debug, Default)]
struct Waiting {
    failed_reads: usize,
    last_failure: Option<SlotReadError>,
    reopened: usize,
    reopen_failure: Option<String>,
}

impl Waiting {
    /// Handles a failed read. `Ok(())` means "try again at the next poll";
    /// malformed replies are returned as errors.
    fn failed(&mut self, link: &mut dyn Link, error: SlotReadError) -> Result<(), SlotReadError> {
        let SlotReadError::Io { errno, .. } = &error else {
            return Err(error);
        };
        let gone = errno.is_device_gone();
        self.failed_reads += 1;
        self.last_failure = Some(error);
        if gone {
            self.reopen(link);
        }
        Ok(())
    }

    fn reopen(&mut self, link: &mut dyn Link) -> bool {
        match link.reopen() {
            Ok(()) => {
                self.reopened += 1;
                true
            }
            Err(e) => {
                self.reopen_failure = Some(e);
                false
            }
        }
    }

    fn slot(
        &mut self,
        link: &mut dyn Link,
        slot: Slot,
    ) -> Result<Option<SlotReport>, SlotReadError> {
        match read_slot(link.device(), slot) {
            Ok(report) => Ok(Some(report)),
            Err(e) => self.failed(link, e).map(|()| None),
        }
    }

    fn slots(&mut self, link: &mut dyn Link) -> Result<Option<[SlotReport; 5]>, SlotReadError> {
        match read_slots(link.device()) {
            Ok(slots) => Ok(Some(slots)),
            Err(e) => self.failed(link, e).map(|()| None),
        }
    }

    fn warnings(self) -> Vec<Warning> {
        let mut warnings = Vec::new();
        if let Some(last) = self.last_failure {
            warnings.push(Warning::SlotReadRetried {
                count: self.failed_reads,
                last: last.to_string(),
            });
        }
        if self.reopened > 0 || self.reopen_failure.is_some() {
            warnings.push(Warning::ManagementReopened {
                count: self.reopened,
                last_failure: self.reopen_failure,
            });
        }
        warnings
    }
}

/// Timing of a polling procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Polling {
    /// Give up after this long.
    pub timeout: Duration,
    /// Time between slot reads.
    pub interval: Duration,
}

impl Polling {
    /// Pair defaults: 60 s, every 1 s.
    pub const PAIR: Self = Self {
        timeout: Duration::from_secs(60),
        interval: Duration::from_secs(1),
    };
    /// Unpair defaults: 15 s, every 0.5 s.
    pub const UNPAIR: Self = Self {
        timeout: Duration::from_secs(15),
        interval: Duration::from_millis(500),
    };
}

/// How pairing ended (spec receiver §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairResult {
    /// New slots became occupied and pairing was stopped.
    Paired(Vec<Slot>),
    /// `PairTimeout` (12): no new slot before the timeout.
    Timeout,
    /// `PairTimeout` (12): interrupted by a signal.
    Interrupted,
    /// `ReceiverCommandFailed` (14): the start SET failed.
    StartFailed(SetResult),
    /// `ReceiverProtocolError` (17): a slot report was malformed while
    /// waiting. Failed reads (errno) are retried until the timeout.
    SlotReadFailed(SlotReadError),
    /// `PairStopFailed` (13): the stop SET failed, also after reopening the
    /// node. Takes precedence over every other result; pairing mode may
    /// still be on.
    StopFailed(SetResult),
    /// `Busy` (22): another process holds the management node's write lock
    /// (spec device §7.2). Nothing was sent.
    Busy(Errno),
}

/// The full record of a pair attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairOutcome {
    /// Slots before starting.
    pub before: [SlotReport; 5],
    /// Last slots read, if any read happened after starting.
    pub after: Option<[SlotReport; 5]>,
    /// Whether the stop packet was sent (always, once start was attempted;
    /// never when the lock could not be taken).
    pub stop_sent: bool,
    /// The result.
    pub result: PairResult,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Sends the stop packet; if the node is gone, reopens it and sends again
/// for up to [`STOP_RETRY`].
fn stop(
    link: &mut dyn Link,
    clock: &dyn Clock,
    interval: Duration,
    waiting: &mut Waiting,
) -> SetResult {
    let mut result = set(link.device(), PAIR_STOP);
    let deadline = clock.now() + STOP_RETRY;
    while let SetResult::Errno(errno) = result
        && errno.is_device_gone()
        && clock.now() < deadline
    {
        if waiting.reopen(link) {
            result = set(link.device(), PAIR_STOP);
        } else {
            clock.sleep(interval);
        }
    }
    result
}

/// Pairs a new device (spec receiver §3 steps 2–7).
///
/// The management node is locked before the start packet (spec device
/// §7.2). `waiting` is called once pairing has started, to tell the user what to
/// do. `interrupted` is polled between slot reads; the caller sets it from a
/// SIGINT/SIGTERM handler. The stop packet is sent whenever the start packet
/// was attempted, however the wait ends.
///
/// # Errors
///
/// Only the initial slot read; nothing has been sent then.
pub fn pair(
    link: &mut dyn Link,
    clock: &dyn Clock,
    polling: Polling,
    waiting: &mut dyn FnMut(),
    interrupted: &dyn Fn() -> bool,
) -> Result<PairOutcome, SlotReadError> {
    let before = read_slots(link.device())?;
    if let Err(e) = link.lock() {
        return Ok(PairOutcome {
            before,
            after: None,
            stop_sent: false,
            result: PairResult::Busy(Errno::of(&e)),
            warnings: Vec::new(),
        });
    }
    let start = set(link.device(), PAIR_START);
    let mut after = None;
    let mut state = Waiting::default();
    let mut result = if start == SetResult::Ok {
        waiting();
        let deadline = clock.now() + polling.timeout;
        loop {
            if interrupted() {
                break PairResult::Interrupted;
            }
            if clock.now() >= deadline {
                break PairResult::Timeout;
            }
            clock.sleep(polling.interval);
            if interrupted() {
                break PairResult::Interrupted;
            }
            match state.slots(link) {
                Ok(Some(slots)) => {
                    let new: Vec<Slot> =
                        cadrat_proto::receiver::newly_occupied(&before, &slots).collect();
                    after = Some(slots);
                    if !new.is_empty() {
                        break PairResult::Paired(new);
                    }
                }
                Ok(None) => {}
                Err(e) => break PairResult::SlotReadFailed(e),
            }
        }
    } else {
        PairResult::StartFailed(start)
    };
    let stopped = stop(link, clock, polling.interval, &mut state);
    if stopped != SetResult::Ok {
        result = PairResult::StopFailed(stopped);
    }
    let mut warnings = state.warnings();
    if let PairResult::Paired(slots) = &result
        && slots.len() > 1
    {
        warnings.push(Warning::PairMultiple(slots.clone()));
    }
    Ok(PairOutcome {
        before,
        after,
        stop_sent: true,
        result,
        warnings,
    })
}

/// How unpairing ended (spec receiver §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpairResult {
    /// The slot became empty.
    Unpaired,
    /// `SlotChanged` (16): the slot was empty, or changed between the
    /// confirmation and the request. Nothing was sent.
    SlotChanged,
    /// `Aborted` (18): the confirmation was refused. Nothing was sent.
    Aborted,
    /// `ReceiverCommandFailed` (14): the SET failed other than with `EPIPE`.
    CommandFailed(SetResult),
    /// `UnpairNotConfirmed` (15): the slot was not seen empty before the
    /// timeout.
    NotConfirmed,
    /// `ReceiverProtocolError` (17): a slot report was malformed after the
    /// request. Failed reads (errno) are retried until the timeout.
    SlotReadFailed(SlotReadError),
    /// `Busy` (22): another process holds the management node's write lock
    /// (spec device §7.2). Nothing was sent.
    Busy(Errno),
}

/// The full record of an unpair attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpairOutcome {
    /// The slot as shown for confirmation.
    pub target: SlotReport,
    /// Whether the unpair packet was sent.
    pub sent: bool,
    /// All slots after the slot emptied.
    pub after: Option<[SlotReport; 5]>,
    /// The result.
    pub result: UnpairResult,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Unpairs one slot (spec receiver §4 steps 2–6).
///
/// `confirm` is shown the slot and returns whether to proceed; with `--yes`
/// the caller returns `true` without asking. The management node is locked
/// only after the confirmation, so a prompt left open blocks no one
/// (spec device §7.2).
///
/// # Errors
///
/// Only the slot reads before the request; nothing has been sent then.
pub fn unpair(
    link: &mut dyn Link,
    slot: Slot,
    clock: &dyn Clock,
    polling: Polling,
    confirm: &mut dyn FnMut(&SlotReport) -> bool,
) -> Result<UnpairOutcome, SlotReadError> {
    let target = read_slot(link.device(), slot)?;
    let mut outcome = UnpairOutcome {
        target,
        sent: false,
        after: None,
        result: UnpairResult::SlotChanged,
        warnings: Vec::new(),
    };
    if !target.occupied() {
        return Ok(outcome);
    }
    if !confirm(&target) {
        outcome.result = UnpairResult::Aborted;
        return Ok(outcome);
    }
    if let Err(e) = link.lock() {
        outcome.result = UnpairResult::Busy(Errno::of(&e));
        return Ok(outcome);
    }
    if read_slot(link.device(), slot)? != target {
        return Ok(outcome);
    }
    outcome.sent = true;
    match set(link.device(), slot.unpair_packet()) {
        SetResult::Ok => {}
        SetResult::Errno(errno) if errno.is_epipe() => outcome.warnings.push(Warning::UnpairEpipe),
        failed => {
            outcome.result = UnpairResult::CommandFailed(failed);
            return Ok(outcome);
        }
    }
    let deadline = clock.now() + polling.timeout;
    let mut state = Waiting::default();
    outcome.result = loop {
        match state.slot(link, slot) {
            Ok(Some(report)) if !report.occupied() => break UnpairResult::Unpaired,
            Ok(_) => {}
            Err(e) => break UnpairResult::SlotReadFailed(e),
        }
        if clock.now() >= deadline {
            break UnpairResult::NotConfirmed;
        }
        clock.sleep(polling.interval);
    };
    if outcome.result == UnpairResult::Unpaired {
        // The slot is already confirmed empty; the snapshot is informational.
        outcome.after = state.slots(link).ok().flatten();
    }
    outcome.warnings.extend(state.warnings());
    Ok(outcome)
}
