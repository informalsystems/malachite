use malachite_common::{Context, Round, Timeout};

use derive_where::derive_where;

use crate::Validity;

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
    Vote(Ctx::Vote),

    /// Receive a timeout
    TimeoutElapsed(Timeout),
}
