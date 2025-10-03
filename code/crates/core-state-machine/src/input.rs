//! Inputs to the round state machine.
//! FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm

use derive_where::derive_where;

use malachitebft_core_types::{Context, Round};

/// A certificate is a set of prevote messages that justify a transition
pub type Certificate<Ctx> = alloc::vec::Vec<<Ctx as Context>::Vote>;

/// Input to the round state machine.
/// FaB: Based on ConsensusInput from Quint spec
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Input<Ctx>
where
    Ctx: Context,
{
    /// No input
    NoInput,

    /// Start a new round, either as proposer or not.
    /// FaB: Maps to StartInput / StartRound
    NewRound(Round),

    /// FaB: Leader (proposer) can propose after receiving 4f+1 prevotes WITH 2f+1 for same value
    /// Maps to: LeaderProposeWithLockInput
    /// Contains: (value, certificate of 4f+1 prevotes, round of prevotes)
    LeaderProposeWithLock {
        value: Ctx::Value,
        certificate: Certificate<Ctx>,
        certificate_round: Round,
    },

    /// FaB: Leader (proposer) can propose after receiving 4f+1 prevotes WITHOUT a 2f+1 lock
    /// Maps to: LeaderProposeWithoutLockInput
    /// Contains: certificate of 4f+1 prevotes
    LeaderProposeWithoutLock {
        certificate: Certificate<Ctx>,
    },

    /// Receive a proposal from the proposer.
    /// FaB: Maps to FollowerReceiveProposalInput
    /// Follower validates SafeProposal and prevotes
    Proposal(Ctx::Proposal),

    /// FaB: Received 4f+1 prevotes for current round (max_round = round_p)
    /// Maps to: EnoughPrevotesForRoundInput
    /// Triggers scheduling of prevote timeout
    EnoughPrevotesForRound,

    /// FaB: Have proposal + 4f+1 matching prevotes for same value → DECIDE!
    /// Maps to: CanDecideInput
    /// Contains: (proposal, certificate of 4f+1 prevotes for that value)
    CanDecide {
        proposal: Ctx::Proposal,
        certificate: Certificate<Ctx>,
    },

    /// FaB: Received a DECISION message from another node
    /// Maps to: ReceiveDecisionInput
    /// Contains: (value decided, certificate)
    ReceiveDecision {
        value: Ctx::Value,
        certificate: Certificate<Ctx>,
    },

    /// FaB: Received f+1 prevotes from a higher round → skip to that round
    /// Maps to: CanSkipRoundInput
    /// Contains: (new round, certificate of f+1 prevotes)
    SkipRound {
        round: Round,
        certificate: Certificate<Ctx>,
    },

    /// Timeout waiting for proposal.
    /// FaB: Maps to TimeoutProposeInput
    TimeoutPropose,

    /// Timeout waiting for prevotes (to move to next round).
    /// FaB: Maps to TimeoutPrevoteInput
    /// Contains: certificate of prevotes seen so far
    TimeoutPrevote {
        certificate: Certificate<Ctx>,
    },
}
