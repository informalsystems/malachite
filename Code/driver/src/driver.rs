use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::fmt;
use tokio::sync::mpsc::error::SendError;

use tokio::sync::mpsc::{
    unbounded_channel, UnboundedReceiver as Receiver, UnboundedSender as Sender,
};

use malachite_common::{
    Context, Proposal, Round, SignedVote, Timeout, TimeoutStep, Validator, ValidatorSet, Value,
    Vote, VoteType,
};
use malachite_round::input::Input as RoundEvent;
use malachite_round::output::Output as RoundOutput;
use malachite_round::state::{State as RoundState, Step};
use malachite_round::state_machine::Info;
use malachite_vote::keeper::Output as VoteKeeperOutput;
use malachite_vote::keeper::VoteKeeper;
use malachite_vote::Threshold;
use malachite_vote::ThresholdParams;

use crate::input::Input;
use crate::output::Output;
use crate::Error;
use crate::ProposerSelector;
use crate::Validity;

pub struct Handle<Ctx>
where
    Ctx: Context,
{
    tx_input: Sender<Input<Ctx>>,
    rx_output: Receiver<Result<Output<Ctx>, Error<Ctx>>>,
}

impl<Ctx> Handle<Ctx>
where
    Ctx: Context,
{
    pub fn new(
        tx_input: Sender<Input<Ctx>>,
        rx_output: Receiver<Result<Output<Ctx>, Error<Ctx>>>,
    ) -> Self {
        Self {
            tx_input,
            rx_output,
        }
    }

    pub fn send(&self, msg: Input<Ctx>) -> Result<(), SendError<Input<Ctx>>> {
        self.tx_input.send(msg)
    }

    pub async fn recv(&mut self) -> Result<Output<Ctx>, Error<Ctx>> {
        match self.rx_output.recv().await {
            Some(Ok(output)) => Ok(output),
            Some(Err(err)) => Err(err),
            None => Err(Error::RecvError),
        }
    }
}

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

    rx_input: Receiver<Input<Ctx>>,
    // tx_input: Sender<Input<Ctx>>,
    tx_output: Sender<Result<Output<Ctx>, Error<Ctx>>>,
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
    ) -> (Self, Handle<Ctx>) {
        let (tx_output, rx_output) = unbounded_channel();
        let (tx_input, rx_input) = unbounded_channel();

        let handle = Handle::new(tx_input.clone(), rx_output);

        let votes = VoteKeeper::new(
            validator_set.total_voting_power(),
            ThresholdParams::default(), // TODO: Make this configurable
        );

        let driver = Self {
            ctx,
            proposer_selector: Box::new(proposer_selector),
            address,
            validator_set,
            votes,
            round_state: RoundState::default(),
            rx_input,
            // tx_input,
            tx_output,
        };

        (driver, handle)
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

    pub async fn run(mut self) {
        loop {
            let msg = match self.rx_input.recv().await {
                Some(msg) => msg,
                None => break,
            };

            let output = self.process(msg).await;
            self.emit(output);
        }
    }

    pub async fn process(&mut self, msg: Input<Ctx>) -> Result<Option<Output<Ctx>>, Error<Ctx>> {
        let round_output = match self.apply(msg).await? {
            Some(msg) => msg,
            None => return Ok(None),
        };

        let output = self.convert(round_output);
        Ok(Some(output))
    }

    fn emit(&self, output: Result<Option<Output<Ctx>>, Error<Ctx>>) {
        match output {
            Ok(None) => (),
            Ok(Some(output)) => {
                let _ = self.tx_output.send(Ok(output));
            }
            Err(err) => {
                let _ = self.tx_output.send(Err(err));
            }
        }
    }

    fn convert(&self, round_output: RoundOutput<Ctx>) -> Output<Ctx> {
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
        self.apply_event(round, RoundEvent::NewRound)
    }

    async fn apply_propose_value(
        &mut self,
        round: Round,
        value: Ctx::Value,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        self.apply_event(round, RoundEvent::ProposeValue(value))
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
                    return self.apply_event(proposal.round(), RoundEvent::InvalidProposal);
                } else if polka_previous {
                    // L32
                    return self.apply_event(
                        proposal.round(),
                        RoundEvent::InvalidProposalAndPolkaPrevious(proposal.clone()),
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
            return self.apply_event(
                proposal.round(),
                RoundEvent::ProposalAndPrecommitValue(proposal.clone()),
            );
        }

        // If the proposal is for a different round drop the proposal
        // TODO - this check is also done in the round state machine, decide where to do it
        if self.round_state.round != proposal.round() {
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
            return self.apply_event(
                proposal.round(),
                RoundEvent::ProposalAndPolkaCurrent(proposal.clone()),
            );
        }

        // L28
        if polka_previous {
            return self.apply_event(
                proposal.round(),
                RoundEvent::ProposalAndPolkaPrevious(proposal.clone()),
            );
        }

        // TODO - Caller needs to store the proposal (valid or not) as the quorum (polka or commits) may be met later
        self.apply_event(proposal.round(), RoundEvent::Proposal(proposal.clone()))
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

        let round_event = match vote_output {
            VoteKeeperOutput::PolkaAny => RoundEvent::PolkaAny,
            VoteKeeperOutput::PolkaNil => RoundEvent::PolkaNil,
            VoteKeeperOutput::PolkaValue(v) => RoundEvent::PolkaValue(v),
            VoteKeeperOutput::PrecommitAny => RoundEvent::PrecommitAny,
            VoteKeeperOutput::PrecommitValue(v) => RoundEvent::PrecommitValue(v),
            VoteKeeperOutput::SkipRound(r) => RoundEvent::SkipRound(r),
        };

        self.apply_event(vote_round, round_event)
    }

    fn apply_timeout(&mut self, timeout: Timeout) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let event = match timeout.step {
            TimeoutStep::Propose => RoundEvent::TimeoutPropose,
            TimeoutStep::Prevote => RoundEvent::TimeoutPrevote,
            TimeoutStep::Precommit => RoundEvent::TimeoutPrecommit,
        };

        self.apply_event(timeout.round, event)
    }

    /// Apply the event, update the state.
    fn apply_event(
        &mut self,
        event_round: Round,
        event: RoundEvent<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let round_state = core::mem::take(&mut self.round_state);

        // Multiplex the event with the round state.
        let mux_event = match event {
            RoundEvent::PolkaValue(value_id) => match round_state.proposal {
                Some(ref proposal) if proposal.value().id() == value_id => {
                    RoundEvent::ProposalAndPolkaCurrent(proposal.clone())
                }
                _ => RoundEvent::PolkaAny,
            },
            RoundEvent::PrecommitValue(value_id) => match round_state.proposal {
                Some(ref proposal) if proposal.value().id() == value_id => {
                    RoundEvent::ProposalAndPrecommitValue(proposal.clone())
                }
                _ => RoundEvent::PrecommitAny,
            },

            _ => event,
        };

        let proposer = self.get_proposer(round_state.round)?;
        let data = Info::new(event_round, &self.address, proposer.address());

        // Apply the event to the round state machine
        let transition = round_state.apply(&data, mux_event);

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
            .field("round_state", &self.round_state)
            .finish()
    }
}
