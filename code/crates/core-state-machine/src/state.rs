//! The state maintained by the round state machine

use derive_where::derive_where;

use crate::input::{Certificate, Input};
use crate::state_machine::Info;
use crate::transition::Transition;

#[cfg(feature = "debug")]
use crate::traces::*;

use malachitebft_core_types::{Context, Height, Round};

/// A value and its associated round
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundValue<Value> {
    /// The value
    pub value: Value,
    /// The round
    pub round: Round,
}

impl<Value> RoundValue<Value> {
    /// Create a new `RoundValue` instance.
    pub fn new(value: Value, round: Round) -> Self {
        Self { value, round }
    }
}

/// The step of consensus in this round
/// FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm
/// Steps: PrePropose → Propose → Prevote → Commit (no Precommit)
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    /// The round has not started yet
    Unstarted,

    /// PrePropose step (FaB-specific).
    /// Proposer waits for 4f+1 prevotes before proposing unless it's in the first round.
    Prepropose,

    /// Propose step.
    /// Either we are the proposer or we are waiting for a proposal.
    Propose,

    /// We are at the prevote step.
    Prevote,

    /// We have committed and decided on a value
    Commit,
}

/// The state of the consensus state machine
/// FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm
/// Based on initialization from FaB spec and Quint LocalState
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct State<Ctx>
where
    Ctx: Context,
{
    /// The height of the consensus (h_p in spec)
    pub height: Ctx::Height,

    /// The round we are at within a height (round_p in spec)
    pub round: Round,

    /// The step we are at within a round (step_p in spec)
    pub step: Step,

    /// The value we have decided on, None if no decision has been made yet.
    /// The decision round is the round of the proposal that we decided on.
    /// (decision_p[] in spec)
    pub decision: Option<RoundValue<Ctx::Value>>,

    /// FaB: The value we prevoted for (prevotedValue_p in spec)
    /// This is the value for which we sent a prevote in this or a previous round
    pub prevoted_value: Option<Ctx::Value>,

    /// FaB: The proposal message we prevoted for (prevotedProposalMsg_p in spec)
    /// Stored without justification (empty set of prevotes)
    pub prevoted_proposal_msg: Option<Ctx::Proposal>,

    /// FaB: The last prevote we sent (lastPrevote_p in spec)
    /// Used for rebroadcasting by the driver
    pub last_prevote: Option<Ctx::Vote>,

    /// Buffer with traces of tendermint algorithm lines,
    #[cfg(feature = "debug")]
    #[derive_where(skip)]
    pub traces: alloc::vec::Vec<Trace<Ctx>>,
}

impl<Ctx> State<Ctx>
where
    Ctx: Context,
{
    /// Create a new `State` instance at the given height and round.
    /// FaB: Initialization based on FaB spec
    pub fn new(height: Ctx::Height, round: Round) -> Self {
        Self {
            height,
            round,
            step: Step::Unstarted,
            decision: None,
            prevoted_value: None,           // FaB: prevotedValue_p = nil
            prevoted_proposal_msg: None,    // FaB: prevotedProposalMsg_p = nil
            last_prevote: None,             // FaB: lastPrevote_p = nil
            #[cfg(feature = "debug")]
            traces: alloc::vec::Vec::default(),
        }
    }

    /// Set the round.
    pub fn with_round(self, round: Round) -> Self {
        Self { round, ..self }
    }

    /// Set the step.
    pub fn with_step(self, step: Step) -> Self {
        Self { step, ..self }
    }

    /// FaB: Set the prevoted value (prevotedValue_p in spec)
    pub fn set_prevoted_value(self, value: Ctx::Value) -> Self {
        Self {
            prevoted_value: Some(value),
            ..self
        }
    }

    /// FaB: Set the prevoted proposal message (prevotedProposalMsg_p in spec)
    /// Stored without justification
    pub fn set_prevoted_proposal_msg(self, proposal: Ctx::Proposal) -> Self {
        Self {
            prevoted_proposal_msg: Some(proposal),
            ..self
        }
    }

    /// FaB: Set the last prevote we sent (lastPrevote_p in spec)
    pub fn set_last_prevote(self, vote: Ctx::Vote) -> Self {
        Self {
            last_prevote: Some(vote),
            ..self
        }
    }

    /// Set the value we have decided on.
    pub fn set_decision(self, proposal_round: Round, value: Ctx::Value) -> Self {
        Self {
            decision: Some(RoundValue::new(value, proposal_round)),
            ..self
        }
    }

    /// Apply the given input to the current state, triggering a transition.
    pub fn apply(self, ctx: &Ctx, data: &Info<Ctx>, input: Input<Ctx>) -> Transition<Ctx> {
        crate::state_machine::apply(ctx, self, data, input)
    }

    /// Return the traces logged during execution.
    #[cfg(feature = "debug")]
    pub fn add_trace(&mut self, line: Line) {
        self.traces.push(Trace::new(self.height, self.round, line));
    }

    /// Return the traces logged during execution.
    #[cfg(feature = "debug")]
    pub fn get_traces(&self) -> &[Trace<Ctx>] {
        &self.traces
    }
}

impl<Ctx> Default for State<Ctx>
where
    Ctx: Context,
{
    fn default() -> Self {
        Self::new(Ctx::Height::ZERO, Round::Nil)
    }
}
