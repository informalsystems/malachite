use alloc::collections::BTreeSet;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use malachitebft_core_state_machine::input::{Certificate, Input as RoundInput};
use malachitebft_core_state_machine::output::Output as RoundOutput;
use malachitebft_core_state_machine::state::{State as RoundState, Step};
use malachitebft_core_state_machine::state_machine::Info;
// FaB: Removed PolkaCertificate, CommitCertificate, PolkaSignature (3f+1 Tendermint concepts)
// FaB: In FaB-a-la-Tendermint-bounded-square, we use 4f+1 prevote certificates instead
use malachitebft_core_types::{
    Context, EnterRoundCertificate,
    Proposal, Round, RoundCertificateType, SignedProposal, SignedVote, Timeout, TimeoutKind,
    Validator, ValidatorSet, Validity, Value, ValueId, Vote, VoteType,
};
use malachitebft_core_votekeeper::keeper::Output as VKOutput;
use malachitebft_core_votekeeper::keeper::VoteKeeper;

use crate::input::Input;
use crate::output::Output;
use crate::proposal_keeper::ProposalKeeper;
use crate::Error;
use crate::ThresholdParams;

/// Driver for the state machine of the Malachite consensus engine at a given height.
pub struct Driver<Ctx>
where
    Ctx: Context,
{
    /// The context of the consensus engine,
    /// for defining the concrete data types and signature scheme.
    #[allow(dead_code)]
    ctx: Ctx,

    /// The address of the node.
    address: Ctx::Address,

    /// Quorum thresholds
    threshold_params: ThresholdParams,

    /// The validator set at the current height
    validator_set: Ctx::ValidatorSet,

    /// The proposer for the current round, None for round nil.
    proposer: Option<Ctx::Address>,

    /// The proposals to decide on.
    pub(crate) proposal_keeper: ProposalKeeper<Ctx>,

    /// The vote keeper.
    pub(crate) vote_keeper: VoteKeeper<Ctx>,

    // FaB: Removed commit_certificates and polka_certificates (3f+1 Tendermint concepts)
    // FaB: In FaB, certificates are 4f+1 prevotes, built on-demand from vote_keeper
    // FaB: Use vote_keeper.build_certificate() or build_certificate_any() instead

    /// The state of the round state machine.
    pub(crate) round_state: RoundState<Ctx>,

    /// The pending inputs to be processed next, if any.
    /// The first element of the tuple is the round at which that input has been emitted.
    pending_inputs: Vec<(Round, RoundInput<Ctx>)>,

    // FaB: Only prevotes in FaB-a-la-Tendermint-bounded-square (no precommits)
    last_prevote: Option<Ctx::Vote>,

    /// The certificate that justifies moving to the `enter_round` specified in the `EnterRoundCertificate.
    pub round_certificate: Option<EnterRoundCertificate<Ctx>>,
}

impl<Ctx> Driver<Ctx>
where
    Ctx: Context,
{
    /// Create a new `Driver` instance for the given height.
    ///
    /// Called when consensus is started and initialized with the first height.
    /// Re-initialization for subsequent heights is done using `move_to_height()`.
    pub fn new(
        ctx: Ctx,
        height: Ctx::Height,
        validator_set: Ctx::ValidatorSet,
        address: Ctx::Address,
        threshold_params: ThresholdParams,
    ) -> Self {
        let proposal_keeper = ProposalKeeper::new();
        let vote_keeper = VoteKeeper::new(validator_set.clone(), threshold_params);
        let round_state = RoundState::new(height, Round::Nil);

        Self {
            ctx,
            address,
            threshold_params,
            validator_set,
            proposal_keeper,
            vote_keeper,
            round_state,
            proposer: None,
            pending_inputs: vec![],
            // FaB: Removed commit_certificates and polka_certificates initialization
            last_prevote: None,
            // FaB: No precommits in FaB
            round_certificate: None,
        }
    }

    /// Reset votes, round state, pending input
    /// and move to new height with the given validator set.
    pub fn move_to_height(&mut self, height: Ctx::Height, validator_set: Ctx::ValidatorSet) {
        // Reset the proposal keeper
        let proposal_keeper = ProposalKeeper::new();

        // Reset the vote keeper
        let vote_keeper = VoteKeeper::new(validator_set.clone(), self.threshold_params);

        // Reset the round state
        let round_state = RoundState::new(height, Round::Nil);

        self.validator_set = validator_set;
        self.proposal_keeper = proposal_keeper;
        self.vote_keeper = vote_keeper;
        self.round_state = round_state;
        self.pending_inputs = vec![];
        // FaB: Removed commit_certificates and polka_certificates reset
        self.last_prevote = None;
        // FaB: No precommits in FaB
    }

    /// Return the height of the consensus.
    pub fn height(&self) -> Ctx::Height {
        self.round_state.height
    }

    /// Return the current round we are at.
    pub fn round(&self) -> Round {
        self.round_state.round
    }

    /// Return the current step within the round we are at.
    pub fn step(&self) -> Step {
        self.round_state.step
    }

    /// Returns true if the current step is propose.
    pub fn step_is_propose(&self) -> bool {
        self.round_state.step == Step::Propose
    }

    /// Returns true if the current step is prevote.
    pub fn step_is_prevote(&self) -> bool {
        self.round_state.step == Step::Prevote
    }

    // FaB: No precommit step in FaB-a-la-Tendermint-bounded-square

    /// Returns true if the current step is commit.
    pub fn step_is_commit(&self) -> bool {
        self.round_state.step == Step::Commit
    }

    // FaB: Removed valid_value() method - Tendermint concept (polka)
    // FaB: In FaB, use prevoted_value() to get the value we prevoted for
    /// Return the value we prevoted for in the current round, if any.
    pub fn prevoted_value(&self) -> Option<&Ctx::Value> {
        self.round_state.prevoted_value.as_ref()
    }

    /// FaB: Return the proposal message we prevoted for (prevotedProposalMsg_p in spec)
    /// Used for periodic rebroadcast as per FaB line 113
    pub fn prevoted_proposal_msg(&self) -> Option<&Ctx::Proposal> {
        self.round_state.prevoted_proposal_msg.as_ref()
    }

    /// Return a reference to the votekeper
    pub fn votes(&self) -> &VoteKeeper<Ctx> {
        &self.vote_keeper
    }

    /// Return a reference to the proposal keeper
    pub fn proposals(&self) -> &ProposalKeeper<Ctx> {
        &self.proposal_keeper
    }

    /// Return the state for the current round.
    pub fn round_state(&self) -> &RoundState<Ctx> {
        &self.round_state
    }

    /// Return the round and value of the decided proposal
    pub fn decided_value(&self) -> Option<(Round, Ctx::Value)> {
        self.round_state
            .decision
            .as_ref()
            .map(|decision| (decision.round, decision.value.clone()))
    }

    /// Return the address of the node.
    pub fn address(&self) -> &Ctx::Address {
        &self.address
    }

    /// Return the validator set for this height.
    pub fn validator_set(&self) -> &Ctx::ValidatorSet {
        &self.validator_set
    }

    /// Return the proposer address for the current round, if any.
    pub fn proposer_address(&self) -> Option<&Ctx::Address> {
        self.proposer.as_ref()
    }

    /// Return the proposer for the current round.
    pub fn get_proposer(&self) -> Result<&Ctx::Validator, Error<Ctx>> {
        if let Some(proposer) = &self.proposer {
            let proposer = self
                .validator_set
                .get_by_address(proposer)
                .ok_or_else(|| Error::ProposerNotFound(proposer.clone()))?;

            Ok(proposer)
        } else {
            Err(Error::NoProposer(self.height(), self.round()))
        }
    }

    // FaB: Removed commit_certificate() and polka_certificates() methods
    // FaB: No certificate storage in FaB - certificates built on-demand from vote_keeper

    /// Get the round certificate for the current round.
    pub fn round_certificate(&self) -> Option<&EnterRoundCertificate<Ctx>> {
        self.round_certificate.as_ref()
    }

    /// Returns the proposal and its validity for the given round and value_id, if any.
    pub fn proposal_and_validity_for_round_and_value(
        &self,
        round: Round,
        value_id: ValueId<Ctx>,
    ) -> Option<&(SignedProposal<Ctx>, Validity)> {
        self.proposal_keeper
            .get_proposal_and_validity_for_round_and_value(round, value_id)
    }

    /// Returns the proposals and their validities for the given round, if any.
    pub fn proposals_and_validities_for_round(
        &self,
        round: Round,
    ) -> &[(SignedProposal<Ctx>, Validity)] {
        self.proposal_keeper
            .get_proposals_and_validities_for_round(round)
    }

    /// Store the last vote that we have cast
    fn set_last_vote_cast(&mut self, vote: &Ctx::Vote) {
        assert_eq!(vote.height(), self.height());

        // FaB: Only PREVOTE in FaB-a-la-Tendermint-bounded-square
        if vote.round() == self.round() {
            match vote.vote_type() {
                VoteType::Prevote => self.last_prevote = Some(vote.clone()),
            }
        }
    }

    /// Process the given input, returning the outputs to be broadcast to the network.
    pub fn process(&mut self, msg: Input<Ctx>) -> Result<Vec<Output<Ctx>>, Error<Ctx>> {
        let round_output = match self.apply(msg)? {
            Some(msg) => msg,
            None => return Ok(Vec::new()),
        };

        let mut outputs = vec![];

        // Lift the round state machine output to one or more driver outputs
        self.lift_output(round_output, &mut outputs);

        // Apply the pending inputs, if any, and lift their outputs
        while !self.pending_inputs.is_empty() {
            let new_pending = core::mem::take(&mut self.pending_inputs);
            for (round, input) in new_pending {
                if let Some(output) = self.apply_input(round, input)? {
                    self.lift_output(output, &mut outputs)
                }
            }
        }

        Ok(outputs)
    }

    /// Convert the output of the round state machine to the output type of the driver.
    fn lift_output(&mut self, round_output: RoundOutput<Ctx>, outputs: &mut Vec<Output<Ctx>>) {
        match round_output {
            RoundOutput::NewRound(round) => outputs.push(Output::NewRound(self.height(), round)),

            RoundOutput::Proposal(proposal) => outputs.push(Output::Propose(proposal)),

            RoundOutput::Vote(vote) => self.lift_vote_output(vote, outputs),

            RoundOutput::ScheduleTimeout(timeout) => outputs.push(Output::ScheduleTimeout(timeout)),

            RoundOutput::GetValueAndScheduleTimeout(height, round, timeout) => {
                outputs.push(Output::ScheduleTimeout(timeout));
                outputs.push(Output::GetValue(height, round, timeout));
            }

            // FaB: Decision is now a struct with certificate (for reliable broadcast)
            RoundOutput::Decision {
                round,
                proposal,
                certificate,
            } => outputs.push(Output::Decide(round, proposal, certificate)),
        }
    }

    fn lift_vote_output(&mut self, vote: Ctx::Vote, outputs: &mut Vec<Output<Ctx>>) {
        if vote.validator_address() != self.address() {
            return;
        }

        // Only cast a vote if any of the following is true:
        // - We have not voted yet
        // - That vote is for a higher height than our last vote
        // - That vote is for a higher round than our last vote
        // FaB: Only PREVOTE in FaB-a-la-Tendermint-bounded-square
        // - That vote is the same as our last vote
        let can_vote = match vote.vote_type() {
            VoteType::Prevote => self.last_prevote.as_ref().is_none_or(|prev| {
                prev.height() < vote.height() || prev.round() < vote.round() || prev == &vote
            }),
        };

        if can_vote {
            self.set_last_vote_cast(&vote);
            outputs.push(Output::Vote(vote));
        }
    }

    /// FaB: Apply the given input to the state machine, returning the output, if any.
    fn apply(&mut self, input: Input<Ctx>) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        match input {
            // FaB: Removed CommitCertificate and PolkaCertificate - not used in FaB
            Input::NewRound(height, round, proposer) => {
                self.apply_new_round(height, round, proposer)
            }
            Input::ProposeValue(round, value) => self.apply_propose_value(round, value),
            Input::Proposal(proposal, validity) => self.apply_proposal(proposal, validity),
            Input::Vote(vote) => self.apply_vote(vote),
            Input::ReceiveDecision(value, certificate) => {
                self.apply_receive_decision(value, certificate)
            }
            Input::TimeoutElapsed(timeout) => self.apply_timeout(timeout),
        }
    }

    // FaB: Removed apply_commit_certificate() - no commit certificates in FaB
    // FaB: Removed apply_polka_certificate() - no polka certificates in FaB

    fn apply_new_round(
        &mut self,
        height: Ctx::Height,
        round: Round,
        proposer: Ctx::Address,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        if self.height() == height {
            // If it's a new round for same height, just reset the round, keep the valid and locked values
            self.round_state.round = round;
        } else {
            self.round_state = RoundState::new(height, round);
        }

        // Update the proposer for the new round
        self.proposer = Some(proposer);

        self.apply_input(round, RoundInput::NewRound(round))
    }

    // FaB: When proposer receives a value to propose, check if there's a certificate
    /// FaB: Maps to LeaderProposeWithLock or LeaderProposeWithoutLock
    fn apply_propose_value(
        &mut self,
        round: Round,
        value: Ctx::Value,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        // FaB: Try to build a 4f+1 certificate for the previous round
        // FaB: In round 0, there's no certificate needed - propose immediately with empty certificate
        if round == Round::new(0) {
            // FaB: Round 0 - propose without lock, pass value so it broadcasts immediately
            let certificate = Vec::new(); // Empty certificate for round 0
            return self.apply_input(
                round,
                RoundInput::LeaderProposeWithoutLock {
                    value: Some(value),
                    certificate,
                },
            );
        }

        // FaB: Try to build certificate from previous round
        let prev_round = Round::new((round.as_i64() - 1) as u32);

        // FaB: Check if we have a 4f+1 certificate with a 2f+1 lock on this value
        if let Some(certificate) = self.vote_keeper.build_certificate(prev_round, &value.id()) {
            // FaB: We have a certificate for this value → propose with lock
            self.apply_input(
                round,
                RoundInput::LeaderProposeWithLock {
                    value,
                    certificate,
                    certificate_round: prev_round,
                },
            )
        } else if let Some(certificate) = self.vote_keeper.build_certificate_any(prev_round) {
            // FaB: We have 4f+1 prevotes but no lock → propose without lock
            // FaB: Pass the value so state machine can broadcast proposal
            self.apply_input(
                round,
                RoundInput::LeaderProposeWithoutLock {
                    value: Some(value),
                    certificate,
                },
            )
        } else {
            // FaB: No certificate available - shouldn't happen
            // FaB: Proposer should only receive ProposeValue after seeing 4f+1 prevotes
            Ok(None)
        }
    }

    fn apply_proposal(
        &mut self,
        proposal: SignedProposal<Ctx>,
        validity: Validity,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        if self.height() != proposal.height() {
            return Err(Error::InvalidProposalHeight {
                proposal_height: proposal.height(),
                consensus_height: self.height(),
            });
        }

        let round = proposal.round();

        match self.store_and_multiplex_proposal(proposal, validity) {
            Some(round_input) => self.apply_input(round, round_input),
            None => Ok(None),
        }
    }

    fn apply_vote(
        &mut self,
        vote: SignedVote<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        if self.height() != vote.height() {
            return Err(Error::InvalidVoteHeight {
                vote_height: vote.height(),
                consensus_height: self.height(),
            });
        }

        if self
            .validator_set
            .get_by_address(vote.validator_address())
            .is_none()
        {
            return Err(Error::ValidatorNotFound(vote.validator_address().clone()));
        }

        let vote_round = vote.round();
        let this_round = self.round();

        // FaB: Apply vote to vote keeper, get output if threshold reached
        let Some(output) = self.vote_keeper.apply_vote(vote, this_round) else {
            return Ok(None);
        };

        // FaB: Handle certificate storage for skip rounds
        match &output {
            // FaB: Removed PolkaValue - no polka certificate storage
            // FaB: Removed PrecommitAny - no precommit certificates in FaB

            // FaB: Store skip round certificate (lines 95-96)
            VKOutput::MaxRoundPlus(round) => self.store_skip_round_certificate(*round),

            // FaB: CertificateAny and CertificateValue don't need certificate storage
            // FaB: They're built on-demand by multiplex_vote_threshold
            _ => (),
        }

        // FaB: Multiplex the vote keeper output into a state machine input
        let Some((input_round, round_input)) = self.multiplex_vote_threshold(output, vote_round) else {
            return Ok(None);
        };

        self.apply_input(input_round, round_input)
    }

    // FaB: Removed store_polka_certificate() - Tendermint 3f+1 concept
    // FaB: No polka certificate storage in FaB

    /// FaB: Prunes all votes from rounds less than `min_round`.
    pub fn prune_votes_and_certificates(&mut self, min_round: Round) {
        // FaB: No certificates to prune - just prune votes
        self.vote_keeper.prune_votes(min_round);
    }

    // FaB: Removed prune_polka_certificates() - no certificate storage
    // FaB: Removed store_precommit_any_round_certificate() - no precommits in FaB

    fn store_skip_round_certificate(&mut self, vote_round: Round) {
        // FaB: Use build_skip_round_certificate() which collects from latest_prevotes
        let Some(skip_votes) = self.vote_keeper.build_skip_round_certificate(vote_round) else {
            panic!("Missing the SkipRound votes for round {vote_round}");
        };

        self.round_certificate = Some(EnterRoundCertificate::new_from_votes(
            self.height(),
            vote_round,
            vote_round,
            RoundCertificateType::Skip,
            skip_votes,
        ));
    }

    // FaB: Timeout handling for FaB-a-la-Tendermint-bounded-square
    // FaB: Only TimeoutPropose and TimeoutPrevote (no Precommit timeout)
    fn apply_timeout(&mut self, timeout: Timeout) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let input = match timeout.kind {
            // FaB: TimeoutPropose (line 98-102)
            TimeoutKind::Propose => RoundInput::TimeoutPropose,

            // FaB: TimeoutPrevote (line 104-106) - needs certificate of prevotes seen
            TimeoutKind::Prevote => {
                // FaB: Build certificate of all prevotes we've seen for this round
                let certificate = self
                    .vote_keeper
                    .build_certificate_any(timeout.round)
                    .unwrap_or_default();

                RoundInput::TimeoutPrevote { certificate }
            }

            // FaB: Rebroadcast timeout (line 108-113)
            // The driver never receives these events, so we can just ignore them.
            TimeoutKind::Rebroadcast => return Ok(None),
        };

        self.apply_input(timeout.round, input)
    }

    /// FaB: Apply a received decision from the network/sync protocol
    /// FaB: This allows nodes to learn about decisions from other nodes
    fn apply_receive_decision(
        &mut self,
        value: Ctx::Value,
        certificate: Certificate<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        // FaB: Pass the decision to the state machine
        // FaB: The state machine will validate and potentially decide
        self.apply_input(
            self.round(),
            RoundInput::ReceiveDecision {
                value,
                certificate,
            },
        )
    }

    /// Apply the input, update the state.
    fn apply_input(
        &mut self,
        input_round: Round,
        input: RoundInput<Ctx>,
    ) -> Result<Option<RoundOutput<Ctx>>, Error<Ctx>> {
        let round_state = core::mem::take(&mut self.round_state);

        let previous_step = round_state.step;

        // FaB: For SkipRound, we need to update the proposer BEFORE creating Info
        // FaB: because Info needs the proposer for the NEW round, not the current round
        // FaB: IMPORTANT: Use round_state.height, not self.height(), because we've already taken round_state!
        if matches!(input, RoundInput::SkipRound { .. }) {
            let proposer_address = self
                .ctx
                .select_proposer(&self.validator_set, round_state.height, input_round)
                .address()
                .clone();
            self.proposer = Some(proposer_address);
        }

        let proposer = self.get_proposer()?;
        let info = Info::new(input_round, &self.address, proposer.address());

        // Apply the input to the round state machine
        let transition = round_state.apply(&self.ctx, &info, input);

        // Update state
        self.round_state = transition.next_state;

        if previous_step != self.round_state.step && self.round_state.step != Step::Unstarted {
            let pending_inputs = self.multiplex_step_change(input_round);

            self.pending_inputs = pending_inputs;
        }

        // Return output, if any
        Ok(transition.output)
    }

    /// Return the traces logged during execution.
    #[cfg(feature = "debug")]
    pub fn get_traces(&self) -> &[malachitebft_core_state_machine::traces::Trace<Ctx>] {
        self.round_state.get_traces()
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
            .field("proposal", &self.proposal_keeper)
            .field("votes", &self.vote_keeper)
            .field("round_state", &self.round_state)
            .finish()
    }
}
