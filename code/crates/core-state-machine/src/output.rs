//! Outputs of the round state machine.
//! FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm

use derive_where::derive_where;

use malachitebft_core_types::{Context, NilOrVal, Round, Timeout, TimeoutKind, ValueId};

use crate::input::Certificate;

/// Output of the round state machine.
/// FaB: Based on ConsensusOutput from Quint spec
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub enum Output<Ctx>
where
    Ctx: Context,
{
    /// Move to the new round.
    NewRound(Round),

    /// Broadcast a proposal with its justification certificate.
    /// FaB: Maps to BroadcastProposal
    /// The certificate is None for round 0, Some(certificate) for round > 0
    Proposal {
        proposal: Ctx::Proposal,
        certificate: Option<Certificate<Ctx>>,
    },

    /// Broadcast a prevote.
    /// FaB: Maps to BroadcastPrevote (only prevotes in FaB, no precommits)
    Vote(Ctx::Vote),

    /// Schedule a timeout.
    /// FaB: Maps to ScheduleTimeout
    ScheduleTimeout(Timeout),

    /// Ask for a value at the given height, round and to schedule a timeout.
    /// The timeout tells the proposal builder how long it has to build a value.
    /// FaB: Maps to GetValueBroadcastProposal (proposer getting value when no lock exists)
    GetValueAndScheduleTimeout(Ctx::Height, Round, Timeout),

    /// Decide the value.
    /// FaB: Maps to Decide + ReliableBroadcastDecision
    /// When we have proposal + 4f+1 prevotes, we decide and reliably broadcast
    Decision {
        round: Round,
        proposal: Ctx::Proposal,
        certificate: Certificate<Ctx>,
    },
}

impl<Ctx: Context> Output<Ctx> {
    /// Build a `Proposal` output with no certificate (for testing/convenience).
    /// For proposals with certificates, construct Output::Proposal directly.
    pub fn proposal(
        ctx: &Ctx,
        height: Ctx::Height,
        round: Round,
        value: Ctx::Value,
        pol_round: Round,
        address: Ctx::Address,
    ) -> Self {
        Output::Proposal {
            proposal: ctx.new_proposal(height, round, value, pol_round, address),
            certificate: None,
        }
    }

    /// Build a `Vote` output for a prevote.
    pub fn prevote(
        ctx: &Ctx,
        height: Ctx::Height,
        round: Round,
        value_id: NilOrVal<ValueId<Ctx>>,
        address: Ctx::Address,
    ) -> Self {
        Output::Vote(ctx.new_prevote(height, round, value_id, address))
    }

    // FaB: Removed precommit() helper - no precommits in FaB

    /// Build a `ScheduleTimeout` output.
    pub fn schedule_timeout(round: Round, step: TimeoutKind) -> Self {
        Output::ScheduleTimeout(Timeout { round, kind: step })
    }

    /// Build a `GetValue` output.
    pub fn get_value_and_schedule_timeout(
        height: Ctx::Height,
        round: Round,
        step: TimeoutKind,
    ) -> Self {
        Output::GetValueAndScheduleTimeout(height, round, Timeout { round, kind: step })
    }

    /// Build a `Decision` output.
    /// FaB: Now includes certificate for reliable broadcast
    pub fn decision(round: Round, proposal: Ctx::Proposal, certificate: Certificate<Ctx>) -> Self {
        Output::Decision {
            round,
            proposal,
            certificate,
        }
    }
}
