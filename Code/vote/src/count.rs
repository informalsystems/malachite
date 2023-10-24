use alloc::collections::BTreeMap;
use alloc::sync::Arc;

use malachite_common::{ValueId, Vote};

pub type Weight = u64;

/// A value and the weight of votes for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuesWeights {
    value_weights: BTreeMap<Arc<ValueId>, Weight>,
}

impl ValuesWeights {
    pub fn new() -> ValuesWeights {
        ValuesWeights {
            value_weights: BTreeMap::new(),
        }
    }

    /// Add weight to the value and return the new weight.
    pub fn add_weight(&mut self, value: Arc<ValueId>, weight: Weight) -> Weight {
        let entry = self.value_weights.entry(value).or_insert(0);
        *entry += weight;
        *entry
    }

    /// Return the cumulative weight associated with all the values.
    /// Returns None if there are no weights associated with any value.
    pub fn cumulative_weights(&self) -> Option<Weight> {
        self.value_weights
            .iter()
            .fold(None, |mac, (_, w)| mac.map(|acc| acc + w))
    }
}

impl Default for ValuesWeights {
    fn default() -> Self {
        Self::new()
    }
}

/// VoteCount tallys votes of the same type.
/// Votes are for nil or for some value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoteCount {
    // Weight of votes for nil
    pub nil: Weight,
    /// Weight of votes for the values
    pub values_weights: ValuesWeights,
    /// Total weight
    pub total: Weight,
}

impl VoteCount {
    pub fn new(total: Weight) -> VoteCount {
        VoteCount {
            nil: 0,
            total,
            values_weights: ValuesWeights::new(),
        }
    }

    /// Add vote to internal counters and return the highest threshold.
    pub fn add_vote(&mut self, vote: Vote, weight: Weight) -> Threshold {
        // Note: The ordering among these if clauses is important
        // We first check if there is a quorum for a specific value
        if let Some(value) = vote.value {
            let value = Arc::new(value);
            let new_weight = self.values_weights.add_weight(value.clone(), weight);

            // Check if we have a quorum for this value.
            if is_quorum(new_weight, self.total) {
                return Threshold::Value(value);
            }
        } else {
            self.nil += weight;

            // Check if we have a quorum for nil.
            if is_quorum(self.nil, self.total) {
                return Threshold::Nil;
            }
        }

        // Check if we have a quorum for all values and for nil votes,
        if self.is_quorum_any() {
            return Threshold::Any;
        }

        // No quorum was reached
        Threshold::Unreached
    }

    pub fn check_threshold(&self, threshold: Threshold) -> bool {
        match threshold {
            Threshold::Unreached => false,
            Threshold::Any => self.is_quorum_any(),
            Threshold::Nil => self.nil > 0,
            Threshold::Value(value) => self.values_weights.value_weights.contains_key(&value),
        }
    }

    // Checks if the cumulative weight associated to all votes and nil is enough for a quorum
    fn is_quorum_any(&self) -> bool {
        if let Some(cm_weight) = self.values_weights.cumulative_weights() {
            if is_quorum(cm_weight + self.nil, self.total) {
                 return true;
            }
        }
        false
    }
}

//-------------------------------------------------------------------------
// Round votes
//-------------------------------------------------------------------------

// Threshold represents the different quorum thresholds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Threshold {
    /// No quorum was reached
    Unreached,
    /// Quorum reached for a specific value
    Value(Arc<ValueId>),
    /// Quorum reached for a nil value
    /// This happens eg if the proposer missed their time to send proposal
    Nil,
    /// Qorum of votes reached but for multiple different values, including nil
    Any,
}

/// Returns whether or not `value > (2/3)*total`.
pub fn is_quorum(value: Weight, total: Weight) -> bool {
    3 * value > 2 * total
}

#[cfg(test)]
mod tests {
    use malachite_common::{Address, Height, Round};

    use crate::RoundVotes;

    use super::*;

    #[test]
    fn add_votes_nil() {
        let total = 3;

        let mut round_votes = RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote for nil. nothing changes.
        let vote = Vote::new_prevote(Round::new(0), None, Address::new(1));
        let thresh = round_votes.add_vote(vote.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add it again, nothing changes.
        let thresh = round_votes.add_vote(vote.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

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

        let mut round_votes = RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote. nothing changes.
        let vote = Vote::new_prevote(Round::new(0), val, Address::new(1));
        let thresh = round_votes.add_vote(vote.clone(), weight);
        assert_eq!(thresh, Threshold::Unreached);

        // add it again, nothing changes.
        let thresh = round_votes.add_vote(vote.clone(), weight);
        assert_eq!(thresh, Threshold::Unreached);

        // add a vote for nil, get Thresh::Any
        let vote_nil = Vote::new_prevote(Round::new(0), None, Address::new(2));
        let thresh = round_votes.add_vote(vote_nil, weight);
        assert_eq!(thresh, Threshold::Any);

        // add vote for value, get Thresh::Value
        let thresh = round_votes.add_vote(vote, weight);
        assert_eq!(thresh, Threshold::Value(Arc::new(v)));
    }

    #[test]
    fn add_votes_multi_values() {
        let v1 = ValueId::new(1);
        let v2 = ValueId::new(2);
        let val1 = Some(v1);
        let val2 = Some(v2);
        let total = 15;

        let mut round_votes = RoundVotes::new(Height::new(1), Round::new(0), total);

        // add a vote for v1. nothing changes.
        let vote1 = Vote::new_precommit(Round::new(0), val1, Address::new(1));
        let thresh = round_votes.add_vote(vote1.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add a vote for v2. nothing changes.
        let vote2 = Vote::new_precommit(Round::new(0), val2, Address::new(2));
        let thresh = round_votes.add_vote(vote2.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add a vote for nil. nothing changes.
        let vote_nil = Vote::new_precommit(Round::new(0), None, Address::new(3));
        let thresh = round_votes.add_vote(vote_nil.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add a vote for v1. nothing changes
        let thresh = round_votes.add_vote(vote1.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add a vote for v2. nothing changes
        let thresh = round_votes.add_vote(vote2.clone(), 1);
        assert_eq!(thresh, Threshold::Unreached);

        // add a big vote for v2. get Value(v2)
        let thresh = round_votes.add_vote(vote2.clone(), 10);
        assert_eq!(thresh, Threshold::Value(Arc::new(v2)));
    }
}
