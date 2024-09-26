use std::marker::PhantomData;

use derive_where::derive_where;

use malachite_common::*;

use crate::types::GossipMsg;

/// An effect which may be yielded by a consensus process.
///
/// Effects are handled by the caller using [`process_sync`][sync] or [`process_async`][async],
/// and the consensus process is then resumed with an appropriate [`Resume`] value, as per
/// the documentation for each effect.
///
/// [sync]: crate::process::process_sync
/// [async]: crate::process::process_async
#[must_use]
#[derive_where(Debug)]
pub enum Effect<Ctx>
where
    Ctx: Context,
{
    /// Reset all timeouts
    /// Resume with: Resume::Continue
    ResetTimeouts,

    /// Cancel all timeouts
    /// Resume with: Resume::Continue
    CancelAllTimeouts,

    /// Cancel a given timeout
    /// Resume with: Resume::Continue
    CancelTimeout(Timeout),

    /// Schedule a timeout
    /// Resume with: Resume::Continue
    ScheduleTimeout(Timeout),

    /// Consensus is starting a new round with the given proposer
    /// Resume with: Resume::Continue
    StartRound(Ctx::Height, Round, Ctx::Address),

    /// Broadcast a message
    /// Resume with: Resume::Continue
    Broadcast(GossipMsg<Ctx>),

    /// Get a value to propose at the given height and round, within the given timeout
    /// Resume with: Resume::Continue
    GetValue(Ctx::Height, Round, Timeout),

    /// Consensus has decided on a value
    /// Resume with: Resume::Continue
    Decide {
        height: Ctx::Height,
        round: Round,
        value: Ctx::Value,
        commits: Vec<SignedMessage<Ctx, Ctx::Vote>>,
    },
}

/// A value with which the consensus process can be resumed after yielding an [`Effect`].
#[must_use]
#[allow(clippy::manual_non_exhaustive)]
#[derive_where(Debug)]
pub enum Resume<Ctx>
where
    Ctx: Context,
{
    /// Internal effect to start processing a [`Msg`][crate::msg::Msg].
    #[doc(hidden)]
    Start(PhantomData<Ctx>),

    /// Resume execution
    Continue,
}
