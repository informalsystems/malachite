//! The consensus state machine.
//! FaB: Rewritten for FaB-a-la-Tendermint-bounded-square algorithm

use malachitebft_core_types::{Context, NilOrVal, Proposal, Round, TimeoutKind, Value};

use crate::input::Input;
use crate::output::Output;
use crate::state::{State, Step};
use crate::transition::Transition;

#[cfg(feature = "debug")]
use crate::traces::*;

/// Immutable information about the input and our node:
/// - Address of our node
/// - Proposer for the round we are at
/// - Round for which the input is for, can be different than the round we are at
pub struct Info<'a, Ctx>
where
    Ctx: Context,
{
    /// The round for which the input is for, can be different than the round we are at
    pub input_round: Round,
    /// Address of our node
    pub address: &'a Ctx::Address,
    /// Proposer for the round we are at
    pub proposer: &'a Ctx::Address,
}

impl<'a, Ctx> Info<'a, Ctx>
where
    Ctx: Context,
{
    /// Create a new `Info` instance.
    pub fn new(input_round: Round, address: &'a Ctx::Address, proposer: &'a Ctx::Address) -> Self {
        Self {
            input_round,
            address,
            proposer,
        }
    }

    /// Create a new `Info` instance where we are the proposer.
    pub fn new_proposer(input_round: Round, address: &'a Ctx::Address) -> Self {
        Self {
            input_round,
            address,
            proposer: address,
        }
    }

    /// Check if we are the proposer for the round we are at.
    pub fn is_proposer(&self) -> bool {
        self.address == self.proposer
    }
}

/// Helper function to start a new round
/// FaB pseudocode lines 29-37
fn start_round<Ctx>(
    mut state: State<Ctx>,
    round: Round,
    is_proposer: bool,
) -> (State<Ctx>, Step)
where
    Ctx: Context,
{
    state.round = round;

    // Determine step based on whether we're proposer and round number
    let step = if is_proposer && round > Round::ZERO {
        Step::Prepropose
    } else {
        Step::Propose
    };

    state.step = step;
    (state, step)
}

/// Apply an input to the current state for FaB algorithm.
///
/// FaB transitions based on pseudocode from important_files/FaB-a-la-Tendermint-bounded-square.md
/// and Quint spec from important_files/tendermint5f_algorithm.qnt
pub fn apply<Ctx>(
    ctx: &Ctx,
    mut state: State<Ctx>,
    info: &Info<Ctx>,
    input: Input<Ctx>,
) -> Transition<Ctx>
where
    Ctx: Context,
{
    let this_round = state.round == info.input_round;

    match (state.step, input) {
        //
        // StartRound - FaB pseudocode line 29-37
        //

        // StartRound - FaB pseudocode lines 29-37
        // Only accept NewRound when in Unstarted step (Malachite pattern)
        (Step::Unstarted, Input::NewRound(round)) => {
            let (new_state, _step) = start_round(state, round, info.is_proposer());
            state = new_state;

            // Schedule propose timeout
            let timeout = Output::schedule_timeout(round, TimeoutKind::Propose);

            // Special case: At round 0, proposer immediately gets value and proposes
            if info.is_proposer() && round == Round::ZERO {
                let get_value = Output::get_value_and_schedule_timeout(
                    state.height,
                    round,
                    TimeoutKind::Propose,
                );

                Transition::to(state)
                    .with_output(timeout)
                    .with_output(get_value)
            } else {
                Transition::to(state).with_output(timeout)
            }
        }

        //
        // PrePropose step - FaB-specific, proposer waits for 4f+1 prevotes
        // FaB pseudocode lines 39-49
        //

        // Leader received 4f+1 prevotes WITH 2f+1 for same value v
        (Step::Prepropose, Input::LeaderProposeWithLock { value, certificate: _, certificate_round })
            if this_round && info.is_proposer() =>
        {
            state.step = Step::Propose;

            // Broadcast PROPOSAL with value v and certificate S
            let proposal = ctx.new_proposal(
                state.height,
                state.round,
                value,
                certificate_round, // pol_round in the proposal
                info.address.clone(),
            );

            Transition::to(state).with_output(Output::Proposal(proposal))
        }

        // Leader received 4f+1 prevotes WITHOUT a 2f+1 lock
        (Step::Prepropose, Input::LeaderProposeWithoutLock { value, certificate })
            if this_round && info.is_proposer() =>
        {
            state.step = Step::Propose;

            // If we already have a value (round 0), broadcast proposal immediately
            // Otherwise, request a new value
            if let Some(v) = value {
                // FaB: Round 0 - we already have the value, broadcast immediately
                let proposal = ctx.new_proposal(
                    state.height,
                    state.round,
                    v,
                    Round::Nil, // No pol_round for round 0
                    info.address.clone(),
                );

                Transition::to(state).with_output(Output::Proposal(proposal))
            } else {
                // FaB: Round > 0 - request a new value
                // When value arrives via ProposeValue, driver will rebuild the certificate
                // and send it along with the value
                let get_value = Output::get_value_and_schedule_timeout(
                    state.height,
                    state.round,
                    TimeoutKind::Propose,
                );

                Transition::to(state).with_output(get_value)
            }
        }

        //
        // Propose step - Proposer at round 0 broadcasts proposal
        // FaB pseudocode lines 33-34
        //

        // Proposer at round 0 receives value from app and broadcasts proposal immediately
        (Step::Propose, Input::LeaderProposeWithoutLock { value: Some(v), certificate: _ })
            if this_round && info.is_proposer() && state.round == Round::ZERO =>
        {
            // FaB: Round 0 - proposer broadcasts proposal immediately
            let proposal = ctx.new_proposal(
                state.height,
                state.round,
                v,
                Round::Nil, // No pol_round for round 0
                info.address.clone(),
            );

            Transition::to(state).with_output(Output::Proposal(proposal))
        }

        // Proposer at round > 0 receives value from app after storing certificate
        // FaB pseudocode lines 45-49: broadcast PROPOSAL with certificate when no 2f+1 lock
        (Step::Propose, Input::LeaderProposeWithoutLock { value: Some(v), certificate })
            if this_round && info.is_proposer() && state.round > Round::ZERO =>
        {
            // FaB: Round > 0 - use the certificate provided by the driver
            // The certificate is built from prevotes in rounds >= previous round
            // TODO: For now, we ignore the certificate in the proposal creation
            // The certificate should be attached during proposal streaming/gossip
            let proposal = ctx.new_proposal(
                state.height,
                state.round,
                v,
                Round::Nil, // pol_round from certificate will be handled separately
                info.address.clone(),
            );

            Transition::to(state).with_output(Output::Proposal(proposal))
        }

        //
        // Propose step - Receiving and validating proposals
        // FaB pseudocode lines 51-59
        //

        // Follower receives PROPOSAL from proposer (SafeProposal validation happens in driver)
        // If SafeProposal is true, driver will provide the proposal
        (Step::Propose, Input::Proposal(proposal)) if this_round => {
            state.step = Step::Prevote;

            // Broadcast PREVOTE for the proposal value (FaB lines 56-59)
            let prevote = ctx.new_prevote(
                state.height,
                state.round,
                NilOrVal::Val(proposal.value().id()),
                info.address.clone(),
            );

            // Update prevoted state and last prevote
            state = state
                .set_prevoted_value(proposal.value().clone())
                .set_prevoted_proposal_msg(proposal.clone())
                .set_last_prevote(prevote.clone());

            Transition::to(state).with_output(Output::Vote(prevote))
        }

        //
        // EnoughPrevotesForRound - Schedule prevote timeout
        // FaB pseudocode lines 69-70
        //

        // max_round = round_p for the first time
        (_, Input::EnoughPrevotesForRound) if this_round => {
            let timeout = Output::schedule_timeout(state.round, TimeoutKind::Prevote);
            Transition::to(state).with_output(timeout)
        }

        //
        // Decision - Have proposal + 4f+1 matching prevotes
        // FaB pseudocode lines 72-74
        //

        // Decide when we have proposal + 4f+1 PREVOTE for same value
        (_, Input::CanDecide { proposal, certificate })
            if state.decision.is_none() =>
        {
            state = state.set_decision(proposal.round(), proposal.value().clone());
            state.step = Step::Commit;

            let decision = Output::decision(proposal.round(), proposal, certificate);

            Transition::to(state).with_output(decision)
        }

        // Receive DECISION message from another node
        (_, Input::ReceiveDecision { value, certificate: _ })
            if state.decision.is_none() =>
        {
            // FaB: Set decision and move to Commit step
            // The round in the decision is the round of the proposal that was decided
            // For now, we use the current round (driver should provide the correct round from certificate)
            let round = state.round;
            state = state.set_decision(round, value);
            state.step = Step::Commit;

            // Note: In full implementation, driver should also output the Decision with certificate
            // for now, we just update state
            Transition::to(state)
        }

        //
        // Skip round - Received f+1 prevotes from higher round
        // FaB pseudocode lines 95-96
        //

        (_, Input::SkipRound { round, certificate: _ }) if round > state.round => {
            // FaB: Follow Malachite pattern - set Unstarted, update round, output NewRound
            // FaB: The NewRound handler will perform the actual round start logic
            state.round = round;
            state.step = Step::Unstarted;

            Transition::to(state).with_output(Output::NewRound(round))
        }

        //
        // Timeouts
        // FaB pseudocode lines 98-106
        //

        // TimeoutPropose - prevote for prevotedValue_p (nil if haven't prevoted)
        // FaB pseudocode lines 98-102
        (Step::Propose, Input::TimeoutPropose) | (Step::Prepropose, Input::TimeoutPropose)
            if this_round =>
        {
            state.step = Step::Prevote;

            let value_id = state.prevoted_value
                .as_ref()
                .map(|v| NilOrVal::Val(v.id()))
                .unwrap_or(NilOrVal::Nil);

            // Create and broadcast prevote (FaB lines 101-102)
            let prevote = ctx.new_prevote(
                state.height,
                state.round,
                value_id,
                info.address.clone(),
            );

            // Update last prevote
            state = state.set_last_prevote(prevote.clone());

            Transition::to(state).with_output(Output::Vote(prevote))
        }

        // TimeoutPrevote - move to next round
        (_, Input::TimeoutPrevote { certificate: _ }) if this_round => {
            let next_round = state.round.increment();

            // FaB: Follow Malachite pattern - set Unstarted, update round, output NewRound
            // FaB: The NewRound handler will perform the actual round start logic
            state.round = next_round;
            state.step = Step::Unstarted;

            Transition::to(state).with_output(Output::NewRound(next_round))
        }

        //
        // No transition - ignore invalid inputs
        //
        _ => Transition::invalid(state),
    }
}
