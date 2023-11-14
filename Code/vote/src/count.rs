use alloc::collections::BTreeSet;

use crate::value_weights::ValuesWeights;
use crate::{Threshold, ThresholdParam, Weight};

/// VoteCount tallys votes of the same type.
/// Votes are for nil or for some value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VoteCount<Address, Value> {
    /// Weight of votes for the values, including nil
    pub values_weights: ValuesWeights<Option<Value>>,

    /// Addresses of validators who voted for the values
    pub validator_addresses: BTreeSet<Address>,
}

impl<Address, Value> VoteCount<Address, Value> {
    pub fn new() -> Self {
        VoteCount {
            values_weights: ValuesWeights::new(),
            validator_addresses: BTreeSet::new(),
        }
    }

    /// Add vote for a value (or nil) to internal counters, but only if we haven't seen
    /// a vote from that particular validator yet.
    pub fn add(&mut self, address: Address, value: Option<Value>, weight: Weight) -> Weight
    where
        Address: Clone + Ord,
        Value: Clone + Ord,
    {
        let already_voted = !self.validator_addresses.insert(address);

        if !already_voted {
            self.values_weights.add(value, weight)
        } else {
            self.values_weights.get(&value)
        }
    }

    pub fn get(&self, value: &Option<Value>) -> Weight
    where
        Value: Ord,
    {
        self.values_weights.get(value)
    }

    pub fn sum(&self) -> Weight {
        self.values_weights.sum()
    }

    /// Return whether or not the threshold is met, ie. if we have a quorum for that threshold.
    pub fn is_threshold_met(
        &self,
        threshold: Threshold<Value>,
        param: ThresholdParam,
        total_weight: Weight,
    ) -> bool
    where
        Value: Ord,
    {
        match threshold {
            Threshold::Value(value) => {
                let weight = self.values_weights.get(&Some(value));
                param.is_met(weight, total_weight)
            }

            Threshold::Nil => {
                let weight = self.values_weights.get(&None);
                param.is_met(weight, total_weight)
            }

            Threshold::Any => {
                let sum_weight = self.values_weights.sum();
                param.is_met(sum_weight, total_weight)
            }

            Threshold::Skip | Threshold::Unreached => false,
        }
    }
}

// #[cfg(test)]
// #[allow(clippy::bool_assert_comparison)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn vote_count_nil() {
//         let mut vc = VoteCount::new(4, Default::default());
//
//         let addr1 = [1];
//         let addr2 = [2];
//         let addr3 = [3];
//         let addr4 = [4];
//
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr1, None, 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 1);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr2, None, 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 2);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         // addr1 votes again, is ignored
//         assert_eq!(vc.add(addr1, None, 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 2);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr3, None, 1), Threshold::Nil);
//         assert_eq!(vc.get(&None), 3);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr4, Some(1), 1), Threshold::Any);
//         assert_eq!(vc.get(&None), 3);
//         assert_eq!(vc.get(&Some(1)), 1);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//     }
//
//     #[test]
//     fn vote_count_value() {
//         let mut vc = VoteCount::new(4, Default::default());
//
//         let addr1 = [1];
//         let addr2 = [2];
//         let addr3 = [3];
//         let addr4 = [4];
//
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr1, Some(1), 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 1);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr2, Some(1), 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 2);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         // addr1 votes again, for nil this time, is ignored
//         assert_eq!(vc.add(addr1, None, 1), Threshold::Unreached);
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 2);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr3, Some(1), 1), Threshold::Value(1));
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 3);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         // addr2 votes again, for the same value, is ignored
//         assert_eq!(vc.add(addr2, Some(1), 1), Threshold::Value(1));
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 3);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         assert_eq!(vc.add(addr4, Some(2), 1), Threshold::Any);
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 3);
//         assert_eq!(vc.get(&Some(2)), 1);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//
//         // addr4 votes again, for a different value, is ignored
//         assert_eq!(vc.add(addr4, Some(3), 1), Threshold::Any);
//         assert_eq!(vc.get(&None), 0);
//         assert_eq!(vc.get(&Some(1)), 3);
//         assert_eq!(vc.get(&Some(2)), 1);
//         assert_eq!(vc.get(&Some(3)), 0);
//         assert_eq!(vc.is_threshold_met(Threshold::Unreached), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Any), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Nil), false);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(1)), true);
//         assert_eq!(vc.is_threshold_met(Threshold::Value(2)), false);
//     }
// }
