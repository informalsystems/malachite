use malachite_common::{Context, Round, SignedVote, Timeout, Validity};

use derive_where::derive_where;

/// Events that can be received by the [`Driver`](crate::Driver).
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Input<Ctx>
where
    Ctx: Context,
{
    /// Start a new round with the given proposer
    NewRound(Ctx::Height, Round, Ctx::Address),

    /// Propose a value for the given round
    ProposeValue(Round, Ctx::Value),

    /// Receive a proposal, of the given validity
    Proposal(Ctx::Proposal, Validity),

    /// Receive a vote
    Vote(SignedVote<Ctx>),

    /// Receive a timeout
    TimeoutElapsed(Timeout),
}
