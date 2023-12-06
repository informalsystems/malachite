use alloc::boxed::Box;
use core::fmt;

use malachite_common::{
    Context, Proposal, Round, SignedVote, Timeout, TimeoutStep, Validator, ValidatorSet, Value,
    Vote, VoteType,
};
use malachite_round::input::Input as RoundInput;
use malachite_round::output::Output as RoundOutput;
use malachite_round::state::{State as RoundState, Step};
use malachite_round::state_machine::Info;
use malachite_vote::keeper::Output as VoteKeeperOutput;
use malachite_vote::keeper::VoteKeeper;
use malachite_vote::Threshold;
use malachite_vote::ThresholdParams;

use crate::input::Input;
use crate::mixer;
use crate::output::Output;
use crate::proposals::Proposals;
use crate::Error;
use crate::ProposerSelector;
use crate::Validity;

/// Driver for the state machine of the Malachite consensus engine at a given height.
pub struct Driver<Ctx>
where
    Ctx: Context,
{
    pub ctx: Ctx,
    pub proposer_selector: Box<dyn ProposerSelector<Ctx>>,

    pub address: Ctx::Address,
    pub validator_set: Ctx::ValidatorSet,

    pub votes: VoteKeeper<Ctx>,
    pub round_state: RoundState<Ctx>,
    pub proposals: Proposals<Ctx>,
    pub pending_input: Option<(Round, RoundInput<Ctx>)>,
}

impl<Ctx> Driver<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        ctx: Ctx,
        proposer_selector: impl ProposerSelector<Ctx> + 'static,
        validator_set: Ctx::ValidatorSet,
        address: Ctx::Address,
    ) -> Self {
        let votes = VoteKeeper::new(
            validator_set.total_voting_power(),
            ThresholdParams::default(), // TODO: Make this configurable
        );

        Self {
            ctx,
            proposer_selector: Box::new(proposer_selector),
            address,
            validator_set,
            votes,
            round_state: RoundState::default(),
            proposals: Proposals::new(),
            pending_input: None,
        }
    }

    pub fn height(&self) -> &Ctx::Height {
        &self.round_state.height
    }

    pub fn round(&self) -> Round {
        self.round_state.round
    }

    pub fn get_proposer(&self, round: Round) -> Result<&Ctx::Validator, Error<Ctx>> {
        let address = self
            .proposer_selector
            .select_proposer(round, &self.validator_set);

        let proposer = self
            .validator_set
            .get_by_address(&address)
            .ok_or_else(|| Error::ProposerNotFound(address))?;

        Ok(proposer)
    }

    pub async fn process(&mut self, msg: Input<Ctx>) -> Result<Vec<Output<Ctx>>, Error<Ctx>> {
        let round_output = match self.apply(msg).await? {
            Some(msg) => msg,
            None => return Ok(Vec::new()),
        };

        let output = self.round_output_to_output(round_output);
        let mut outputs = vec![output];

        self.process_pending(&mut outputs)?;

        Ok(outputs)
    }

    fn process_pending(&mut self, outputs: &mut Vec<Output<Ctx>>) -> Result<(), Error<Ctx>> {
        while let Some((round, input)) = self.pending_input.take() {
            if let Some(round_output) = self.apply_input(round, input)? {
                let output = self.round_output_to_output(round_output);
                outputs.push(output);
            };
        }

        Ok(())
    }

    fn round_output_to_output(&mut self, round_output: RoundOutput<Ctx>) -> Output<Ctx> {
        match round_output {
            RoundOutput::NewRound(round) => Output::NewRound(self.height().clone(), round),

            RoundOutput::Proposal(proposal) => {
                // TODO: sign the proposal
                Output::Propose(proposal)
            }

            RoundOutput::Vote(vote) => {
                let signed_vote = self.ctx.sign_vote(vote);
                Output::Vote(signed_vote)
            }

            RoundOutput::ScheduleTimeout(timeout) => Output::ScheduleTimeout(timeout),

            RoundOutput::GetValueAndScheduleTimeout(round, timeout) => {
                Output::GetValueAndScheduleTimeout(round, timeout)
            }

            RoundOutput::Decision(value) => {
                // TODO: update the state
                Output::Decide(value.round, value.value)
            }
        }
    }

    async fn apply(&mut self, input: Input<Ctx>) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        match input {
            Input::NewRound(height, round) => self.apply_new_round(height, round).await,
            Input::ProposeValue(round, value) => self.apply_propose_value(round, value).await,
            Input::Proposal(proposal, validity) => self.apply_proposal(proposal, validity).await,
            Input::Vote(signed_vote) => self.apply_vote(signed_vote),
            Input::TimeoutElapsed(timeout) => self.apply_timeout(timeout),
        }
    }

    async fn apply_new_round(
        &mut self,
        height: Ctx::Height,
        round: Round,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        if self.height() == &height {
            // If it's a new round for same height, just reset the round, keep the valid and locked values
            self.round_state.round = round;
        } else {
            self.round_state = RoundState::new(height, round);
        }
        self.apply_input(round, RoundInput::NewRound)
    }

    async fn apply_propose_value(
        &mut self,
        round: Round,
        value: Ctx::Value,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        self.apply_input(round, RoundInput::ProposeValue(value))
    }

    async fn apply_proposal(
        &mut self,
        proposal: Ctx::Proposal,
        validity: Validity,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        // Check that there is an ongoing round
        if self.round_state.round == Round::Nil {
            return Ok(None);
        }

        // Check that the proposal is for the current height
        if self.round_state.height != proposal.height() {
            return Ok(None);
        }

        self.proposals.insert(proposal.clone());

        let polka_for_pol = self.votes.is_threshold_met(
            &proposal.pol_round(),
            VoteType::Prevote,
            Threshold::Value(proposal.value().id()),
        );

        let polka_previous = proposal.pol_round().is_defined()
            && polka_for_pol
            && proposal.pol_round() < self.round_state.round;

        // Handle invalid proposal
        if !validity.is_valid() {
            if self.round_state.step == Step::Propose {
                if proposal.pol_round().is_nil() {
                    // L26
                    return self.apply_input(proposal.round(), RoundInput::InvalidProposal);
                } else if polka_previous {
                    // L32
                    return self.apply_input(
                        proposal.round(),
                        RoundInput::InvalidProposalAndPolkaPrevious(proposal),
                    );
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }

        // We have a valid proposal.
        // L49
        // TODO - check if not already decided
        if self.votes.is_threshold_met(
            &proposal.round(),
            VoteType::Precommit,
            Threshold::Value(proposal.value().id()),
        ) {
            return self.apply_input(
                proposal.round(),
                RoundInput::ProposalAndPrecommitValue(proposal),
            );
        }

        // If the proposal is for a different round, drop the proposal
        if self.round() != proposal.round() {
            return Ok(None);
        }

        let polka_for_current = self.votes.is_threshold_met(
            &proposal.round(),
            VoteType::Prevote,
            Threshold::Value(proposal.value().id()),
        );

        let polka_current = polka_for_current && self.round_state.step >= Step::Prevote;

        // L36
        if polka_current {
            return self.apply_input(
                proposal.round(),
                RoundInput::ProposalAndPolkaCurrent(proposal),
            );
        }

        // L28
        if self.round_state.step == Step::Propose && polka_previous {
            // TODO: Check proposal vr is equal to threshold vr
            return self.apply_input(
                proposal.round(),
                RoundInput::ProposalAndPolkaPrevious(proposal),
            );
        }

        // TODO - Caller needs to store the proposal (valid or not) as the quorum (polka or commits) may be met later
        self.apply_input(proposal.round(), RoundInput::Proposal(proposal))
    }

    fn apply_vote(
        &mut self,
        signed_vote: SignedVote<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let validator = self
            .validator_set
            .get_by_address(signed_vote.validator_address())
            .ok_or_else(|| Error::ValidatorNotFound(signed_vote.validator_address().clone()))?;

        if !self
            .ctx
            .verify_signed_vote(&signed_vote, validator.public_key())
        {
            return Err(Error::InvalidVoteSignature(
                signed_vote.clone(),
                validator.clone(),
            ));
        }

        let vote_round = signed_vote.vote.round();
        let current_round = self.round();

        let Some(vote_output) =
            self.votes
                .apply_vote(signed_vote.vote, validator.voting_power(), current_round)
        else {
            return Ok(None);
        };

        let round_input = match vote_output {
            VoteKeeperOutput::PolkaAny => RoundInput::PolkaAny,
            VoteKeeperOutput::PolkaNil => RoundInput::PolkaNil,
            VoteKeeperOutput::PolkaValue(v) => RoundInput::PolkaValue(v),
            VoteKeeperOutput::PrecommitAny => RoundInput::PrecommitAny,
            VoteKeeperOutput::PrecommitValue(v) => RoundInput::PrecommitValue(v),
            VoteKeeperOutput::SkipRound(r) => RoundInput::SkipRound(r),
        };

        self.apply_input(vote_round, round_input)
    }

    fn apply_timeout(&mut self, timeout: Timeout) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let input = match timeout.step {
            TimeoutStep::Propose => RoundInput::TimeoutPropose,
            TimeoutStep::Prevote => RoundInput::TimeoutPrevote,
            TimeoutStep::Precommit => RoundInput::TimeoutPrecommit,
        };

        self.apply_input(timeout.round, input)
    }

    /// Apply the input, update the state.
    fn apply_input(
        &mut self,
        input_round: Round,
        input: RoundInput<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let round_state = core::mem::take(&mut self.round_state);
        let current_step = round_state.step;

        let proposer = self.get_proposer(round_state.round)?;
        let info = Info::new(input_round, &self.address, proposer.address());

        // Multiplex the proposal if we have one already for the input round
        let mux_input = mixer::multiplex_proposal(input, input_round, &self.proposals);

        // Apply the input to the round state machine
        let transition = round_state.apply(&info, mux_input);

        let pending_step = transition.next_state.step;

        if current_step != pending_step {
            let pending_input = mixer::multiplex_on_step_change(
                pending_step,
                input_round,
                &self.votes,
                &self.proposals,
            );

            dbg!(&pending_input);

            self.pending_input = pending_input.map(|input| (input_round, input));
        }

        // Update state
        self.round_state = transition.next_state;

        // Return output, if any
        Ok(transition.output)
    }
}

impl<Ctx> fmt::Debug for Driver<Ctx>
where
    Ctx: Context,
{
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Driver")
            .field("address", &self.address)
            .field("validator_set", &self.validator_set)
            .field("votes", &self.votes)
            .field("proposals", &self.proposals.proposals)
            .field("round_state", &self.round_state)
            .finish()
    }
}
