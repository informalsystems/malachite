//! For tallying all the votes for a single round

use derive_where::derive_where;

use malachitebft_core_types::{Context, NilOrVal, ValueId, Vote, VoteType};

use crate::count::VoteCount;
use crate::{Threshold, ThresholdParam, Weight};

/// Tracks all the votes for a single round
/// FaB: Only prevotes in FaB-a-la-Tendermint-bounded-square (no precommits)
#[derive_where(Clone, Debug, PartialEq, Eq)]
pub struct RoundVotes<Ctx: Context> {
    /// The prevotes for this round.
    prevotes: VoteCount<Ctx>,
}

impl<Ctx: Context> RoundVotes<Ctx> {
    /// Create a new `RoundVotes` instance.
    pub fn new() -> Self {
        RoundVotes {
            prevotes: VoteCount::new(),
        }
    }

    /// Return the prevotes for this round.
    pub fn prevotes(&self) -> &VoteCount<Ctx> {
        &self.prevotes
    }

    /// Add a vote to the round, of the given type, from the given address,
    /// with the given value and weight.
    pub fn add_vote(&mut self, vote: &Ctx::Vote, weight: Weight) -> Weight {
        // FaB: Only PREVOTE messages in FaB-a-la-Tendermint-bounded-square
        match vote.vote_type() {
            VoteType::Prevote => self.prevotes.add(vote, weight),
        }
    }

    /// Get the weight of the vote of the given type for the given value.
    ///
    /// If there is no vote for that value, return 0.
    pub fn get_weight(&self, vote_type: VoteType, value: &NilOrVal<ValueId<Ctx>>) -> Weight {
        // FaB: Only PREVOTE messages in FaB-a-la-Tendermint-bounded-square
        match vote_type {
            VoteType::Prevote => self.prevotes.get(value),
        }
    }

    /// Get the sum of the weights of the votes of the given type.
    pub fn weight_sum(&self, vote_type: VoteType) -> Weight {
        // FaB: Only PREVOTE messages in FaB-a-la-Tendermint-bounded-square
        match vote_type {
            VoteType::Prevote => self.prevotes.sum(),
        }
    }

    /// Return whether or not the threshold is met, ie. if we have a quorum for that threshold.
    pub fn is_threshold_met(
        &self,
        vote_type: VoteType,
        threshold: Threshold<ValueId<Ctx>>,
        param: ThresholdParam,
        total_weight: Weight,
    ) -> bool {
        // FaB: Only PREVOTE messages in FaB-a-la-Tendermint-bounded-square
        match vote_type {
            VoteType::Prevote => self
                .prevotes
                .is_threshold_met(threshold, param, total_weight),
        }
    }
}

impl<Ctx: Context> Default for RoundVotes<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
