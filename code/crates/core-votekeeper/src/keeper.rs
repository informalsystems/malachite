//! For tallying votes and emitting messages when certain thresholds are reached.

use derive_where::derive_where;
use thiserror::Error;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use malachitebft_core_types::{
    Context, NilOrVal, Round, SignedVote, Validator, ValidatorSet, ValueId, Vote, VoteType,
};

use crate::evidence::EvidenceMap;
use crate::round_votes::RoundVotes;
use crate::round_weights::RoundWeights;
use crate::{Threshold, ThresholdParams, Weight};

/// Messages emitted by the vote keeper
/// FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Output<Value> {
    /// We have 4f+1 prevotes for the current round
    /// FaB: Certificate received (driver should check for 2f+1 locks within it)
    /// Maps to state machine Input::EnoughPrevotesForRound
    CertificateAny,

    /// We have 4f+1 prevotes for a specific value
    /// FaB: Certificate for a specific value (used for decisions)
    /// Maps to state machine Input::CanDecide (if we have matching proposal)
    CertificateValue(Value),

    /// We have f+1 prevotes from a higher round
    /// FaB: Skip to higher round
    /// Maps to state machine Input::SkipRound
    SkipRound(Round),
}

/// Keeps track of votes and emitted outputs for a given round.
#[derive_where(Clone, Debug, PartialEq, Eq, Default)]
pub struct PerRound<Ctx>
where
    Ctx: Context,
{
    /// The votes for this round.
    votes: RoundVotes<Ctx>,

    /// The addresses and their weights for this round.
    addresses_weights: RoundWeights<Ctx::Address>,

    /// All the votes received for this round.
    received_votes: Vec<SignedVote<Ctx>>,

    /// The emitted outputs for this round.
    emitted_outputs: BTreeSet<Output<ValueId<Ctx>>>,
}

/// Errors can that be yielded when recording a vote.
#[derive(Error)]
pub enum RecordVoteError<Ctx>
where
    Ctx: Context,
{
    /// Attempted to record a conflicting vote.
    #[error("Conflicting vote: {existing} vs {conflicting}")]
    ConflictingVote {
        /// The vote already recorded.
        existing: SignedVote<Ctx>,
        /// The conflicting vote.
        conflicting: SignedVote<Ctx>,
    },
}

impl<Ctx> PerRound<Ctx>
where
    Ctx: Context,
{
    /// Create a new `PerRound` instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new `PerRound` instance with pre-allocated capacity for the expected number of votes.
    pub fn with_expected_number_of_votes(num_votes: usize) -> Self {
        Self {
            // pre-allocate capacity to avoid re-allocations during the addition of votes
            received_votes: Vec::with_capacity(num_votes),
            ..Self::default()
        }
    }

    /// Add a vote to the round, checking for conflicts.
    pub fn add(
        &mut self,
        vote: SignedVote<Ctx>,
        weight: Weight,
    ) -> Result<(), RecordVoteError<Ctx>> {
        if let Some(existing) = self.get_vote(vote.vote_type(), vote.validator_address()) {
            if existing.value() != vote.value() {
                // This is an equivocating vote
                return Err(RecordVoteError::ConflictingVote {
                    existing: existing.clone(),
                    conflicting: vote,
                });
            }
        }

        // Tally this vote
        self.votes.add_vote(&vote, weight);

        // Update the weight of the validator
        self.addresses_weights
            .set_once(vote.validator_address(), weight);

        // Add the vote to the received votes
        self.received_votes.push(vote);

        Ok(())
    }

    /// Return the vote of the given type received from the given validator.
    pub fn get_vote<'a>(
        &'a self,
        vote_type: VoteType,
        address: &'a Ctx::Address,
    ) -> Option<&'a SignedVote<Ctx>> {
        self.received_votes
            .iter()
            .find(move |vote| vote.vote_type() == vote_type && vote.validator_address() == address)
    }

    /// Return the votes for this round.
    pub fn votes(&self) -> &RoundVotes<Ctx> {
        &self.votes
    }

    /// Return the votes for this round.
    pub fn received_votes(&self) -> &Vec<SignedVote<Ctx>> {
        &self.received_votes
    }

    /// Return the addresses and their weights for this round.
    pub fn addresses_weights(&self) -> &RoundWeights<Ctx::Address> {
        &self.addresses_weights
    }

    /// Return the emitted outputs for this round.
    pub fn emitted_outputs(&self) -> &BTreeSet<Output<ValueId<Ctx>>> {
        &self.emitted_outputs
    }
}

/// Keeps track of votes and emits messages when thresholds are reached.
#[derive_where(Clone, Debug)]
pub struct VoteKeeper<Ctx>
where
    Ctx: Context,
{
    /// The validator set for this height.
    validator_set: Ctx::ValidatorSet,

    /// The threshold parameters.
    threshold_params: ThresholdParams,

    /// The votes and emitted outputs for each round.
    per_round: BTreeMap<Round, PerRound<Ctx>>,

    /// Evidence of equivocation.
    evidence: EvidenceMap<Ctx>,
}

impl<Ctx> VoteKeeper<Ctx>
where
    Ctx: Context,
{
    /// Create a new `VoteKeeper` instance, for the given
    /// total network weight (ie. voting power) and threshold parameters.
    pub fn new(validator_set: Ctx::ValidatorSet, threshold_params: ThresholdParams) -> Self {
        Self {
            validator_set,
            threshold_params,
            per_round: BTreeMap::new(),
            evidence: EvidenceMap::new(),
        }
    }

    /// Return the current validator set
    pub fn validator_set(&self) -> &Ctx::ValidatorSet {
        &self.validator_set
    }

    /// Return the total weight (ie. voting power) of the network.
    pub fn total_weight(&self) -> Weight {
        self.validator_set.total_voting_power()
    }

    /// Return the votes for the given round.
    pub fn per_round(&self, round: Round) -> Option<&PerRound<Ctx>> {
        self.per_round.get(&round)
    }

    /// Return votes for all rounds we have seen so far.
    pub fn all_rounds(&self) -> &BTreeMap<Round, PerRound<Ctx>> {
        &self.per_round
    }

    /// Return how many rounds we have seen votes for so far.
    pub fn rounds(&self) -> usize {
        self.per_round.len()
    }

    /// Return the highest round we have seen votes for so far.
    pub fn max_round(&self) -> Round {
        self.per_round.keys().max().copied().unwrap_or(Round::Nil)
    }

    /// Return the evidence of equivocation.
    pub fn evidence(&self) -> &EvidenceMap<Ctx> {
        &self.evidence
    }

    /// Check if we have already seen a vote.
    pub fn has_vote(&self, vote: &SignedVote<Ctx>) -> bool {
        self.per_round
            .get(&vote.round())
            .is_some_and(|per_round| per_round.received_votes().contains(vote))
    }

    /// Apply a vote with a given weight, potentially triggering an output.
    pub fn apply_vote(
        &mut self,
        vote: SignedVote<Ctx>,
        round: Round,
    ) -> Option<Output<ValueId<Ctx>>> {
        let total_weight = self.total_weight();
        let per_round =
            self.per_round
                .entry(vote.round())
                .or_insert(PerRound::with_expected_number_of_votes(
                    self.validator_set.count(),
                ));

        let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) else {
            // Vote from unknown validator, let's discard it.
            return None;
        };

        match per_round.add(vote.clone(), validator.voting_power()) {
            Ok(()) => (),
            Err(RecordVoteError::ConflictingVote {
                existing,
                conflicting,
            }) => {
                // This is an equivocating vote
                self.evidence.add(existing.clone(), conflicting);
                //panic!("Equivocating vote {:?}, existing {:?}", &vote, &existing);
                return None;
            }
        }

        if vote.round() > round {
            let combined_weight = per_round.addresses_weights.sum();

            let skip_round = self
                .threshold_params
                .honest
                .is_met(combined_weight, total_weight);

            if skip_round {
                let output = Output::SkipRound(vote.round());
                per_round.emitted_outputs.insert(output.clone());
                return Some(output);
            }
        }

        // FaB: Check both 2f+1 (quorum) and 4f+1 (certificate) thresholds
        let outputs = compute_thresholds(
            vote.vote_type(),
            per_round,
            vote.value(),
            self.threshold_params,
            total_weight,
        );

        // FaB: Return the first new output (highest priority)
        // Priority: Certificate > Quorum
        outputs
            .into_iter()
            .find(|output| !per_round.emitted_outputs.contains(output))
            .map(|output| {
                per_round.emitted_outputs.insert(output.clone());
                output
            })
    }

    /// Check if a threshold is met, ie. if we have a quorum for that threshold.
    pub fn is_threshold_met(
        &self,
        round: &Round,
        vote_type: VoteType,
        threshold: Threshold<ValueId<Ctx>>,
    ) -> bool {
        self.per_round.get(round).is_some_and(|per_round| {
            per_round.votes.is_threshold_met(
                vote_type,
                threshold,
                self.threshold_params.quorum,
                self.total_weight(),
            )
        })
    }

    /// Prunes all stored votes from rounds less than `min_round`.
    pub fn prune_votes(&mut self, min_round: Round) {
        self.per_round.retain(|round, _| *round >= min_round);
    }

    /// Build a certificate (set of votes) for the given round and value.
    /// FaB: Collects all prevotes for the given value in the given round.
    /// Returns None if we don't have enough votes to form a certificate.
    pub fn build_certificate(
        &self,
        round: Round,
        value_id: &ValueId<Ctx>,
    ) -> Option<Vec<SignedVote<Ctx>>> {
        let per_round = self.per_round.get(&round)?;

        // FaB: Collect all prevotes for this value
        let certificate: Vec<SignedVote<Ctx>> = per_round
            .received_votes
            .iter()
            .filter(|vote| {
                vote.vote_type() == VoteType::Prevote
                    && vote.value() == &NilOrVal::Val(value_id.clone())
            })
            .cloned()
            .collect();

        // FaB: Check if we have 4f+1 votes for this value
        let weight: Weight = certificate
            .iter()
            .filter_map(|vote| self.validator_set.get_by_address(vote.validator_address()))
            .map(|v| v.voting_power())
            .sum();

        if self
            .threshold_params
            .certificate_quorum
            .is_met(weight, self.total_weight())
        {
            Some(certificate)
        } else {
            None
        }
    }

    /// Build a certificate for any value in the given round.
    /// FaB: Collects 4f+1 prevotes from the round, regardless of which value they voted for.
    /// Used for proposer certificates when there's no 2f+1 lock.
    pub fn build_certificate_any(&self, round: Round) -> Option<Vec<SignedVote<Ctx>>> {
        let per_round = self.per_round.get(&round)?;

        // FaB: Collect all prevotes from this round
        let certificate: Vec<SignedVote<Ctx>> = per_round
            .received_votes
            .iter()
            .filter(|vote| vote.vote_type() == VoteType::Prevote)
            .cloned()
            .collect();

        // FaB: Check if we have 4f+1 votes total
        let weight: Weight = certificate
            .iter()
            .filter_map(|vote| self.validator_set.get_by_address(vote.validator_address()))
            .map(|v| v.voting_power())
            .sum();

        if self
            .threshold_params
            .certificate_quorum
            .is_met(weight, self.total_weight())
        {
            Some(certificate)
        } else {
            None
        }
    }

    /// Build an f+1 certificate from prevotes in the given round (for SkipRound).
    /// FaB Lines 92-96: "max_round+ = max{r | ∃k. max_rounds[k] = r ∧ |{j | max_rounds[j] ≥ r}| ≥ f + 1}"
    /// Returns the certificate if we have f+1 voting power from the round.
    pub fn build_skip_round_certificate(&self, round: Round) -> Option<Vec<SignedVote<Ctx>>> {
        let per_round = self.per_round.get(&round)?;

        // FaB: Collect all prevotes from this round
        let certificate: Vec<SignedVote<Ctx>> = per_round
            .received_votes
            .iter()
            .filter(|vote| vote.vote_type() == VoteType::Prevote)
            .cloned()
            .collect();

        // FaB: Check if we have f+1 votes total (honest threshold, not 4f+1!)
        let weight: Weight = certificate
            .iter()
            .filter_map(|vote| self.validator_set.get_by_address(vote.validator_address()))
            .map(|v| v.voting_power())
            .sum();

        if self
            .threshold_params
            .honest
            .is_met(weight, self.total_weight())
        {
            Some(certificate)
        } else {
            None
        }
    }

    /// Build a 4f+1 certificate from prevotes in rounds >= min_round.
    /// FaB Lines 39, 45: "4f+1 <PREVOTE, h_p, r, *> while r >= round_p-1"
    /// Returns the certificate and the round it was built from.
    pub fn build_certificate_from_rounds_gte(&self, min_round: Round) -> Option<(Vec<SignedVote<Ctx>>, Round)> {
        // FaB: Collect prevotes from all rounds >= min_round
        let mut all_prevotes: Vec<SignedVote<Ctx>> = Vec::new();
        let mut certificate_round = min_round;

        for (round, per_round) in self.per_round.iter() {
            if *round >= min_round {
                let round_prevotes: Vec<SignedVote<Ctx>> = per_round
                    .received_votes
                    .iter()
                    .filter(|vote| vote.vote_type() == VoteType::Prevote)
                    .cloned()
                    .collect();
                all_prevotes.extend(round_prevotes);

                // Track the highest round we collected from
                if *round > certificate_round {
                    certificate_round = *round;
                }
            }
        }

        // FaB: Check if we have 4f+1 votes total
        let weight: Weight = all_prevotes
            .iter()
            .filter_map(|vote| self.validator_set.get_by_address(vote.validator_address()))
            .map(|v| v.voting_power())
            .sum();

        if self
            .threshold_params
            .certificate_quorum
            .is_met(weight, self.total_weight())
        {
            Some((all_prevotes, certificate_round))
        } else {
            None
        }
    }

    /// Find a 2f+1 lock within a certificate.
    /// FaB: Analyzes a certificate to find if there's a 2f+1 quorum for any specific value.
    /// Returns the locked value if found, None otherwise.
    pub fn find_lock_in_certificate(
        &self,
        certificate: &[SignedVote<Ctx>],
    ) -> Option<ValueId<Ctx>> {
        use alloc::collections::BTreeMap;

        // Count votes per value
        let mut value_weights: BTreeMap<ValueId<Ctx>, Weight> = BTreeMap::new();

        for vote in certificate {
            if let NilOrVal::Val(value_id) = vote.value() {
                if let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) {
                    *value_weights.entry(value_id.clone()).or_insert(0) += validator.voting_power();
                }
            }
        }

        // FaB: Check if any value has 2f+1 weight (lock)
        for (value_id, weight) in value_weights {
            if self.threshold_params.quorum.is_met(weight, self.total_weight()) {
                return Some(value_id);
            }
        }

        None
    }
}

/// Compute whether or not we have reached thresholds (4f+1 certificates) for the given value.
/// FaB: Returns outputs when 4f+1 threshold is reached.
/// The driver will analyze certificates to detect 2f+1 locks within them.
fn compute_thresholds<Ctx>(
    vote_type: VoteType,
    round: &PerRound<Ctx>,
    value: &NilOrVal<ValueId<Ctx>>,
    thresholds: ThresholdParams,
    total_weight: Weight,
) -> Vec<Output<ValueId<Ctx>>>
where
    Ctx: Context,
{
    let mut outputs = Vec::new();

    // FaB: Only PREVOTE messages in FaB-a-la-Tendermint-bounded-square
    if vote_type != VoteType::Prevote {
        return outputs;
    }

    let weight = round.votes.get_weight(vote_type, value);
    let weight_sum = round.votes.weight_sum(vote_type);

    // FaB: Check 4f+1 certificate thresholds
    // Certificate for specific value (4f+1 for same value)
    if let NilOrVal::Val(v) = value {
        if thresholds.certificate_quorum.is_met(weight, total_weight) {
            outputs.push(Output::CertificateValue(v.clone()));
        }
    }

    // Certificate for any value (4f+1 total prevotes, possibly distributed)
    if thresholds.certificate_quorum.is_met(weight_sum, total_weight) {
        outputs.push(Output::CertificateAny);
    }

    outputs
}
