use alloc::collections::BTreeMap;

use malachite_common::{Consensus, ValueId, Vote};

pub type Weight = u64;

/// A value and the weight of votes for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuesWeights<ValueId> {
    value_weights: BTreeMap<ValueId, Weight>,
}

impl<ValueId> ValuesWeights<ValueId> {
    pub fn new() -> ValuesWeights<ValueId> {
        ValuesWeights {
            value_weights: BTreeMap::new(),
        }
    }

    /// Add weight to the value and return the new weight.
    pub fn add_weight(&mut self, value: ValueId, weight: Weight) -> Weight
    where
        ValueId: Ord,
    {
        let entry = self.value_weights.entry(value).or_insert(0);
        *entry += weight;
        *entry
    }

    /// Return the value with the highest weight and said weight, if any.
    pub fn highest_weighted_value(&self) -> Option<(&ValueId, Weight)> {
        self.value_weights
            .iter()
            .max_by_key(|(_, weight)| *weight)
            .map(|(value, weight)| (value, *weight))
    }
}

impl<ValueId> Default for ValuesWeights<ValueId> {
    fn default() -> Self {
        Self::new()
    }
}

/// VoteCount tallys votes of the same type.
/// Votes are for nil or for some value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoteCount<C>
where
    C: Consensus,
{
    // Weight of votes for nil
    pub nil: Weight,
    /// Weight of votes for the values
    pub values_weights: ValuesWeights<ValueId<C>>,
    /// Total weight
    pub total: Weight,
}

impl<C> VoteCount<C>
where
    C: Consensus,
{
    pub fn new(total: Weight) -> Self {
        VoteCount {
            nil: 0,
            total,
            values_weights: ValuesWeights::new(),
        }
    }

    /// Add vote to internal counters and return the highest threshold.
    pub fn add_vote(&mut self, vote: C::Vote, weight: Weight) -> Threshold<ValueId<C>> {
        if let Some(value) = vote.value() {
            let new_weight = self.values_weights.add_weight(value.clone(), weight);

            // Check if we have a quorum for this value.
            if is_quorum(new_weight, self.total) {
                return Threshold::Value(value.clone());
            }
        } else {
            self.nil += weight;

            // Check if we have a quorum for nil.
            if is_quorum(self.nil, self.total) {
                return Threshold::Nil;
            }
        }

        // Check if we have a quorum for any value, using the highest weighted value, if any.
        if let Some((_max_value, max_weight)) = self.values_weights.highest_weighted_value() {
            if is_quorum(max_weight + self.nil, self.total) {
                return Threshold::Any;
            }
        }

        // No quorum
        Threshold::Init
    }
    pub fn check_threshold(&self, threshold: Threshold<ValueId<C>>) -> bool {
        match threshold {
            Threshold::Init => false,
            Threshold::Any => self.values_weights.highest_weighted_value().is_some(),
            Threshold::Nil => self.nil > 0,
            Threshold::Value(value) => self.values_weights.value_weights.contains_key(&value),
        }
    }
}

//-------------------------------------------------------------------------
// Round votes
//-------------------------------------------------------------------------

// Thresh represents the different quorum thresholds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Threshold<ValueId> {
    /// No quorum
    Init, // no quorum
    /// Qorum of votes but not for the same value
    Any,
    /// Quorum for nil
    Nil,
    /// Quorum for a value
    Value(ValueId),
}

/// Returns whether or note `value > (2/3)*total`.
pub fn is_quorum(value: Weight, total: Weight) -> bool {
    3 * value > 2 * total
}

#[cfg(test)]
mod tests {
    use malachite_common::test::{Address, Height, TestConsensus, ValueId, Vote};
    use malachite_common::Round;

    use crate::RoundVotes;

    use super::*;

    #[test]
    fn add_votes_nil() {
        let total = 3;

        let mut round_votes: RoundVotes<TestConsensus> =
            RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote for nil. nothing changes.
        let vote = Vote::new_prevote(Round::new(0), None, Address::new(1));
        let thresh = round_votes.add_vote(vote.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add it again, nothing changes.
        let thresh = round_votes.add_vote(vote.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add it again, get Nil
        let thresh = round_votes.add_vote(vote.clone(), 1);
        assert_eq!(thresh, Threshold::Nil);
    }

    #[test]
    fn add_votes_single_value() {
        let v = ValueId::new(1);
        let val = Some(v);
        let total = 4;
        let weight = 1;

        let mut round_votes: RoundVotes<TestConsensus> =
            RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote. nothing changes.
        let vote = Vote::new_prevote(Round::new(0), val, Address::new(1));
        let thresh = round_votes.add_vote(vote.clone(), weight);
        assert_eq!(thresh, Threshold::Init);

        // add it again, nothing changes.
        let thresh = round_votes.add_vote(vote.clone(), weight);
        assert_eq!(thresh, Threshold::Init);

        // add a vote for nil, get Thresh::Any
        let vote_nil = Vote::new_prevote(Round::new(0), None, Address::new(2));
        let thresh = round_votes.add_vote(vote_nil, weight);
        assert_eq!(thresh, Threshold::Any);

        // add vote for value, get Thresh::Value
        let thresh = round_votes.add_vote(vote, weight);
        assert_eq!(thresh, Threshold::Value(v));
    }

    #[test]
    fn add_votes_multi_values() {
        let v1 = ValueId::new(1);
        let v2 = ValueId::new(2);
        let val1 = Some(v1);
        let val2 = Some(v2);
        let total = 15;

        let mut round_votes: RoundVotes<TestConsensus> =
            RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote for v1. nothing changes.
        let vote1 = Vote::new_precommit(Round::new(0), val1, Address::new(1));
        let thresh = round_votes.add_vote(vote1.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add a vote for v2. nothing changes.
        let vote2 = Vote::new_precommit(Round::new(0), val2, Address::new(2));
        let thresh = round_votes.add_vote(vote2.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add a vote for nil. nothing changes.
        let vote_nil = Vote::new_precommit(Round::new(0), None, Address::new(3));
        let thresh = round_votes.add_vote(vote_nil.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add a vote for v1. nothing changes
        let thresh = round_votes.add_vote(vote1.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add a vote for v2. nothing changes
        let thresh = round_votes.add_vote(vote2.clone(), 1);
        assert_eq!(thresh, Threshold::Init);

        // add a big vote for v2. get Value(v2)
        let thresh = round_votes.add_vote(vote2.clone(), 10);
        assert_eq!(thresh, Threshold::Value(v2));
    }
}
