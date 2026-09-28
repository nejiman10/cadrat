//! Receiver management: slot reads, pair and unpair (spec 05).
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

/// Reads one slot (spec 05 §2).
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

/// Counts slot reads that failed with an errno while polling. Right after
/// an unpair request the Receiver has been seen to answer a slot GET with
/// `EPIPE` once; such failures are retried, not treated as the result.
#[derive(Debug, Default)]
struct Retries {
    count: usize,
    last: Option<SlotReadError>,
}

impl Retries {
    /// Whether `error` may be retried; records it if so.
    fn retry(&mut self, error: &SlotReadError) -> bool {
        if matches!(error, SlotReadError::Io { .. }) {
            self.count += 1;
            self.last = Some(error.clone());
            true
        } else {
            false
        }
    }

    fn warning(self) -> Option<Warning> {
        self.last.map(|last| Warning::SlotReadRetried {
            count: self.count,
            last: last.to_string(),
        })
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

/// How pairing ended (spec 05 §3).
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
    /// `PairStopFailed` (13): the stop SET failed. Takes precedence over
    /// every other result; pairing mode may still be on.
    StopFailed(SetResult),
}

/// The full record of a pair attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairOutcome {
    /// Slots before starting.
    pub before: [SlotReport; 5],
    /// Last slots read, if any read happened after starting.
    pub after: Option<[SlotReport; 5]>,
    /// Whether the stop packet was sent (always, once start was attempted).
    pub stop_sent: bool,
    /// The result.
    pub result: PairResult,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Pairs a new device (spec 05 §3 steps 2–7).
///
/// `waiting` is called once pairing has started, to tell the user what to
/// do. `interrupted` is polled between slot reads; the caller sets it from a
/// SIGINT/SIGTERM handler. The stop packet is sent whenever the start packet
/// was attempted, however the wait ends.
///
/// # Errors
///
/// Only the initial slot read; nothing has been sent then.
pub fn pair(
    device: &mut dyn Device,
    clock: &dyn Clock,
    polling: Polling,
    waiting: &mut dyn FnMut(),
    interrupted: &dyn Fn() -> bool,
) -> Result<PairOutcome, SlotReadError> {
    let before = read_slots(device)?;
    let start = set(device, PAIR_START);
    let mut after = None;
    let mut retries = Retries::default();
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
            match read_slots(device) {
                Ok(slots) => {
                    let new: Vec<Slot> =
                        cadrat_proto::receiver::newly_occupied(&before, &slots).collect();
                    after = Some(slots);
                    if !new.is_empty() {
                        break PairResult::Paired(new);
                    }
                }
                Err(e) if retries.retry(&e) => {}
                Err(e) => break PairResult::SlotReadFailed(e),
            }
        }
    } else {
        PairResult::StartFailed(start)
    };
    let stop = set(device, PAIR_STOP);
    if stop != SetResult::Ok {
        result = PairResult::StopFailed(stop);
    }
    let mut warnings: Vec<Warning> = retries.warning().into_iter().collect();
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

/// How unpairing ended (spec 05 §4).
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
    /// `UnpairNotConfirmed` (15): the slot did not empty before the timeout.
    NotConfirmed,
    /// `ReceiverProtocolError` (17): a slot report was malformed after the
    /// request. Failed reads (errno) are retried until the timeout.
    SlotReadFailed(SlotReadError),
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

/// Unpairs one slot (spec 05 §4 steps 2–6).
///
/// `confirm` is shown the slot and returns whether to proceed; with `--yes`
/// the caller returns `true` without asking.
///
/// # Errors
///
/// Only the initial slot read; nothing has been sent then.
pub fn unpair(
    device: &mut dyn Device,
    slot: Slot,
    clock: &dyn Clock,
    polling: Polling,
    confirm: &mut dyn FnMut(&SlotReport) -> bool,
) -> Result<UnpairOutcome, SlotReadError> {
    let target = read_slot(device, slot)?;
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
    if read_slot(device, slot)? != target {
        return Ok(outcome);
    }
    outcome.sent = true;
    match set(device, slot.unpair_packet()) {
        SetResult::Ok => {}
        SetResult::Errno(errno) if errno.is_epipe() => outcome.warnings.push(Warning::UnpairEpipe),
        failed => {
            outcome.result = UnpairResult::CommandFailed(failed);
            return Ok(outcome);
        }
    }
    let deadline = clock.now() + polling.timeout;
    let mut retries = Retries::default();
    outcome.result = loop {
        match read_slot(device, slot) {
            Ok(report) if !report.occupied() => break UnpairResult::Unpaired,
            Ok(_) => {}
            Err(e) if retries.retry(&e) => {}
            Err(e) => break UnpairResult::SlotReadFailed(e),
        }
        if clock.now() >= deadline {
            break UnpairResult::NotConfirmed;
        }
        clock.sleep(polling.interval);
    };
    outcome.warnings.extend(retries.warning());
    if outcome.result == UnpairResult::Unpaired {
        // The slot is already confirmed empty; the snapshot is informational.
        outcome.after = read_slots(device).ok();
    }
    Ok(outcome)
}
