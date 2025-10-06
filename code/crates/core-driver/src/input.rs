// FaB: Removed CommitCertificate and PolkaCertificate imports (3f+1 Tendermint concepts)
use malachitebft_core_state_machine::input::Certificate;
use malachitebft_core_types::{
    Context, Round, SignedProposal, SignedVote, Timeout, Validity,
};

use derive_where::derive_where;

/// Events that can be received by the [`Driver`](crate::Driver).
/// FaB: Updated for FaB-a-la-Tendermint-bounded-square
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Input<Ctx>
where
    Ctx: Context,
{
    /// Start a new round with the given proposer
    NewRound(Ctx::Height, Round, Ctx::Address),

    /// FaB: Propose a value for the given round
    /// FaB: Used by proposer in prepropose step after receiving 4f+1 prevotes
    ProposeValue(Round, Ctx::Value),

    /// Receive a proposal, of the given validity
    Proposal(SignedProposal<Ctx>, Validity),

    /// Receive a vote (only prevotes in FaB)
    Vote(SignedVote<Ctx>),

    // FaB: Removed CommitCertificate - no commit certificates in FaB (3f+1 concept)
    // FaB: Removed PolkaCertificate - no polka certificates in FaB (3f+1 concept)

    /// FaB: Receive a decision from the network/sync protocol
    /// FaB: Contains the decided value and 4f+1 prevote certificate
    /// FaB: Replaces CommitCertificate from Tendermint
    ReceiveDecision(Ctx::Value, Certificate<Ctx>),

    /// Receive a timeout
    TimeoutElapsed(Timeout),
}
