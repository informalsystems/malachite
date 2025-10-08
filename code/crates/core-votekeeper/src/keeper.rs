//! For tallying votes and emitting messages when certain thresholds are reached.

use derive_where::derive_where;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use malachitebft_core_types::{
    Context, NilOrVal, Round, SignedVote, Validator, ValidatorSet, ValueId, Vote, VoteType,
};

use crate::evidence::EvidenceMap;
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

/// Keeps track of votes and emits messages when thresholds are reached.
/// FaB: Stores ONE prevote per validator (the highest round seen) as per FaB spec line 91
#[derive_where(Clone, Debug)]
pub struct VoteKeeper<Ctx>
where
    Ctx: Context,
{
    /// The validator set for this height.
    validator_set: Ctx::ValidatorSet,

    /// The threshold parameters.
    threshold_params: ThresholdParams,

    /// FaB: Latest prevote from each validator (one per validator, overwrites on higher round)
    /// Maps validator address -> their most recent prevote
    latest_prevotes: BTreeMap<Ctx::Address, SignedVote<Ctx>>,

    /// FaB: Track which outputs have been emitted per round to avoid duplicates
    emitted_outputs_per_round: BTreeMap<Round, BTreeSet<Output<ValueId<Ctx>>>>,

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
            latest_prevotes: BTreeMap::new(),
            emitted_outputs_per_round: BTreeMap::new(),
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

    /// Return how many rounds we have seen votes for so far.
    /// FaB: Compute from latest_prevotes
    pub fn rounds(&self) -> usize {
        self.latest_prevotes
            .values()
            .map(|vote| vote.round())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// Return the highest round we have seen votes for so far.
    /// FaB: Compute from latest_prevotes
    pub fn max_round(&self) -> Round {
        self.latest_prevotes
            .values()
            .map(|vote| vote.round())
            .max()
            .unwrap_or(Round::Nil)
    }

    /// Return the evidence of equivocation.
    pub fn evidence(&self) -> &EvidenceMap<Ctx> {
        &self.evidence
    }

    /// Check if we have already seen a vote.
    pub fn has_vote(&self, vote: &SignedVote<Ctx>) -> bool {
        self.latest_prevotes
            .get(vote.validator_address())
            .map_or(false, |existing| existing == vote)
    }

    /// FaB: Helper to compute total weight of prevotes for a specific round
    fn compute_weight_sum_for_round(&self, round: Round, vote_type: VoteType) -> Weight {
        let mut weight = 0;
        for vote in self.latest_prevotes.values() {
            if vote.round() == round && vote.vote_type() == vote_type {
                if let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) {
                    weight += validator.voting_power();
                }
            }
        }
        weight
    }

    /// FaB: Helper to compute weight for specific value in a round
    fn compute_weight_for_value(
        &self,
        round: Round,
        vote_type: VoteType,
        value: &NilOrVal<ValueId<Ctx>>,
    ) -> Weight {
        let mut weight = 0;
        for vote in self.latest_prevotes.values() {
            if vote.round() == round && vote.vote_type() == vote_type && vote.value() == value {
                if let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) {
                    weight += validator.voting_power();
                }
            }
        }
        weight
    }

    /// FaB: Compute thresholds for a specific round
    fn compute_thresholds_for_round(&self, target_round: Round) -> Option<Output<ValueId<Ctx>>> {
        // Tally prevotes for this round
        let mut weight_sum: Weight = 0;
        let mut weight_per_value: BTreeMap<ValueId<Ctx>, Weight> = BTreeMap::new();

        for vote in self.latest_prevotes.values() {
            if vote.round() == target_round && vote.vote_type() == VoteType::Prevote { // shouldn't this be the case ... that it's only prevotes ...
                if let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) {
                    let weight = validator.voting_power();
                    weight_sum += weight;

                    if let NilOrVal::Val(value_id) = vote.value() {
                        *weight_per_value.entry(value_id.clone()).or_insert(0) += weight;
                    }
                }
            }
        }

        let total_weight = self.total_weight();

        // Check 4f+1 certificate for specific values
        for (value_id, weight) in &weight_per_value {
            if self.threshold_params.certificate_quorum.is_met(*weight, total_weight) {
                return Some(Output::CertificateValue(value_id.clone()));
            }
        }

        // Check 4f+1 certificate for any value (total prevotes)
        if self.threshold_params.certificate_quorum.is_met(weight_sum, total_weight) {
            return Some(Output::CertificateAny);
        }

        None
    }

    /// FaB: Check if we have f+1 prevotes from rounds >= target_round (for skip)
    fn compute_skip_threshold(&self, target_round: Round) -> bool {
        let mut weight: Weight = 0;

        // Count prevotes from rounds >= target_round
        for vote in self.latest_prevotes.values() {
            if vote.round() >= target_round {
                if let Some(validator) = self.validator_set.get_by_address(vote.validator_address()) {
                    weight += validator.voting_power();
                }
            }
        }

        // Check f+1 threshold (honest/weak quorum)
        self.threshold_params.honest.is_met(weight, self.total_weight())
    }

    /// Apply a vote with a given weight, potentially triggering an output.
    /// FaB: Implements "overwrite q highest prevote message" (line 91)
    pub fn apply_vote(
        &mut self,
        vote: SignedVote<Ctx>,
        round: Round,
    ) -> Option<Output<ValueId<Ctx>>> {
        // Step 1: Validate vote is from known validator
        if self.validator_set.get_by_address(vote.validator_address()).is_none() {
            // Vote from unknown validator, discard
            return None;
        }

        // Step 2: Check for existing prevote from this validator
        if let Some(existing) = self.latest_prevotes.get(vote.validator_address()) {
            if existing.round() == vote.round() && existing.value() != vote.value() {
                // EQUIVOCATION: same round, different value
                self.evidence.add(existing.clone(), vote.clone());
                return None;
            }
            if existing.round() > vote.round() {
                // Ignore older vote
                return None;
            }
            // existing.round() < vote.round(): will overwrite below (FaB line 91)
        }

        // Step 3: Store/overwrite the prevote (FaB line 91: "overwrite q highest prevote message")
        self.latest_prevotes
            .insert(vote.validator_address().clone(), vote.clone());

        // Step 4: Check for SkipRound (vote from future round > current round)
        if vote.round() > round {
            if self.compute_skip_threshold(vote.round()) {
                let output = Output::SkipRound(vote.round());
                let emitted = self
                    .emitted_outputs_per_round
                    .entry(vote.round())
                    .or_insert_with(BTreeSet::new);
                if !emitted.contains(&output) {
                    emitted.insert(output.clone());
                    return Some(output);
                }
            }
        }

        // Step 5: Compute thresholds for vote's round
        let outputs = self.compute_thresholds_for_round(vote.round());

        // Step 6: Return output if not yet emitted
        let emitted = self
            .emitted_outputs_per_round
            .entry(vote.round())
            .or_insert_with(BTreeSet::new);

        outputs
            .into_iter()
            .find(|output| !emitted.contains(output))
            .map(|output| {
                emitted.insert(output.clone());
                output
            })
    }

    /// Check if a threshold is met, ie. if we have a quorum for that threshold.
    /// FaB: Compute on-the-fly from latest_prevotes
    pub fn is_threshold_met(
        &self,
        round: &Round,
        vote_type: VoteType,
        threshold: Threshold<ValueId<Ctx>>,
    ) -> bool {
        match threshold {
            Threshold::Unreached => false,
            Threshold::Nil => {
                let weight = self.compute_weight_for_value(*round, vote_type, &NilOrVal::Nil);
                self.threshold_params
                    .quorum
                    .is_met(weight, self.total_weight())
            }
            Threshold::Any => {
                let weight_sum = self.compute_weight_sum_for_round(*round, vote_type);
                self.threshold_params
                    .quorum
                    .is_met(weight_sum, self.total_weight())
            }
            Threshold::Value(value_id) => {
                let weight =
                    self.compute_weight_for_value(*round, vote_type, &NilOrVal::Val(value_id));
                self.threshold_params
                    .quorum
                    .is_met(weight, self.total_weight())
            }
        }
    }

    /// Prunes all stored votes from rounds less than `min_round`.
    /// FaB: Prune both latest_prevotes and emitted_outputs_per_round
    pub fn prune_votes(&mut self, min_round: Round) {
        self.latest_prevotes
            .retain(|_, vote| vote.round() >= min_round);
        self.emitted_outputs_per_round
            .retain(|round, _| *round >= min_round);
    }

    /// Build a certificate (set of votes) for the given round and value.
    /// FaB: Collects all prevotes for the given value in the given round from latest_prevotes.
    /// Returns None if we don't have enough votes to form a certificate.
    pub fn build_certificate(
        &self,
        round: Round,
        value_id: &ValueId<Ctx>,
    ) -> Option<Vec<SignedVote<Ctx>>> {
        // FaB: Collect prevotes for this round and value from latest_prevotes
        let certificate: Vec<SignedVote<Ctx>> = self
            .latest_prevotes
            .values()
            .filter(|vote| {
                vote.vote_type() == VoteType::Prevote
                    && vote.round() == round
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
        // FaB: Collect all prevotes for this round from latest_prevotes
        let certificate: Vec<SignedVote<Ctx>> = self
            .latest_prevotes
            .values()
            .filter(|vote| vote.vote_type() == VoteType::Prevote && vote.round() == round)
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
        // FaB: Collect prevotes from this round from latest_prevotes
        let certificate: Vec<SignedVote<Ctx>> = self
            .latest_prevotes
            .values()
            .filter(|vote| vote.vote_type() == VoteType::Prevote && vote.round() == round)
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
    pub fn build_certificate_from_rounds_gte(
        &self,
        min_round: Round,
    ) -> Option<(Vec<SignedVote<Ctx>>, Round)> {
        // FaB: Collect prevotes from rounds >= min_round from latest_prevotes
        let all_prevotes: Vec<SignedVote<Ctx>> = self
            .latest_prevotes
            .values()
            .filter(|vote| vote.vote_type() == VoteType::Prevote && vote.round() >= min_round)
            .cloned()
            .collect();

        // Find highest round
        let certificate_round = all_prevotes
            .iter()
            .map(|vote| vote.round())
            .max()
            .unwrap_or(min_round);

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

