use derive_where::derive_where;
use malachite_common::{Context, Round, SignedProposal, SignedVote, Timeout};

use crate::types::ProposedValue;

/// Messages that can be handled by the consensus process
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Msg<Ctx>
where
    Ctx: Context,
{
    /// Start a new height
    StartHeight(Ctx::Height),

    /// Process a vote
    Vote(SignedVote<Ctx>),

    /// Process a proposal
    Proposal(SignedProposal<Ctx>),

    /// Propose a value
    ProposeValue(Ctx::Height, Round, Ctx::Value),

    /// A timeout has elapsed
    TimeoutElapsed(Timeout),

    /// A block to propose has been received
    ReceivedProposedValue(ProposedValue<Ctx>),
}
