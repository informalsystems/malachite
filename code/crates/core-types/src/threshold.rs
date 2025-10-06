use crate::VotingPower;

/// Represents the different quorum thresholds.
/// FaB: Used with both 2f+1 and 4f+1 thresholds in FaB-a-la-Tendermint-bounded-square
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Threshold<ValueId> {
    /// No quorum has been reached yet
    Unreached,

    /// Quorum of votes but not for the same value
    Any,

    /// Quorum of votes for nil
    Nil,

    /// Quorum of votes for a specific value
    /// FaB: Can represent either 2f+1 (lock) or 4f+1 (certificate) depending on context
    Value(ValueId),
}

/// Represents the different quorum thresholds.
///
/// FaB: Updated for FaB-a-la-Tendermint-bounded-square algorithm
/// There are three thresholds:
/// - The quorum threshold (2f+1): Minimum for detecting locks on values
/// - The certificate quorum (4f+1): Required for decisions and certificates in FaB
/// - The honest threshold (f+1): Minimum number of honest nodes (for round skipping)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ThresholdParams {
    /// Threshold for a quorum (default: 2f+1)
    /// FaB: Used for detecting locks within certificates
    pub quorum: ThresholdParam,

    /// Threshold for certificates (default: 4f+1)
    /// FaB: Required for proposals, decisions, and round transitions
    pub certificate_quorum: ThresholdParam,

    /// Threshold for the minimum number of honest nodes (default: f+1)
    /// FaB: Used for round skipping when receiving f+1 votes from higher round
    pub honest: ThresholdParam,
}

impl Default for ThresholdParams {
    fn default() -> Self {
        Self {
            quorum: ThresholdParam::TWO_F_PLUS_ONE,
            certificate_quorum: ThresholdParam::FOUR_F_PLUS_ONE,
            honest: ThresholdParam::F_PLUS_ONE,
        }
    }
}

/// Represents a single quorum threshold parameter.
/// FaB: Threshold is met when: weight > (numerator/denominator) × total_weight
/// For n=5f+1 validators: f+1=1/5, 2f+1=2/5, 4f+1=4/5
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ThresholdParam {
    /// Numerator of the threshold fraction
    pub numerator: u64,

    /// Denominator of the threshold fraction
    pub denominator: u64,
}

impl ThresholdParam {
    /// 2f+1, ie. more than two fifths of the total weight (n=5f+1)
    /// FaB: Used for detecting locks within 4f+1 certificates
    pub const TWO_F_PLUS_ONE: Self = Self::new(2, 5);

    /// f+1, ie. more than one fifth of the total weight (n=5f+1)
    /// FaB: Used for round skipping when receiving f+1 votes from higher round
    pub const F_PLUS_ONE: Self = Self::new(1, 5);

    /// 4f+1, ie. more than four fifths of the total weight (n=5f+1)
    /// FaB: Required for certificates and decisions in FaB-a-la-Tendermint-bounded-square
    pub const FOUR_F_PLUS_ONE: Self = Self::new(4, 5);

    /// Create a new threshold parameter with the given numerator and denominator.
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Check whether the threshold is met.
    pub fn is_met(&self, weight: VotingPower, total: VotingPower) -> bool {
        let lhs = weight
            .checked_mul(self.denominator)
            .expect("attempt to multiply with overflow");

        let rhs = total
            .checked_mul(self.numerator)
            .expect("attempt to multiply with overflow");

        lhs > rhs
    }

    /// Return the minimum expected weight to meet the threshold when applied to the given total.
    pub fn min_expected(&self, total: VotingPower) -> VotingPower {
        1 + total
            .checked_mul(self.numerator)
            .expect("attempt to multiply with overflow")
            .checked_div(self.denominator)
            .expect("attempt to divide with overflow")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_param_is_met() {
        // FaB: Tests for n=5f+1 thresholds

        // f+1 (1/5): needs > 1/5 of total
        // With total=5: needs > 1, so >= 2
        assert!(!ThresholdParam::F_PLUS_ONE.is_met(1, 5));
        assert!(ThresholdParam::F_PLUS_ONE.is_met(2, 5));

        // 2f+1 (2/5): needs > 2/5 of total
        // With total=5: needs > 2, so >= 3
        assert!(!ThresholdParam::TWO_F_PLUS_ONE.is_met(2, 5));
        assert!(ThresholdParam::TWO_F_PLUS_ONE.is_met(3, 5));

        // 4f+1 (4/5): needs > 4/5 of total
        // With total=5: needs > 4, so >= 5
        assert!(!ThresholdParam::FOUR_F_PLUS_ONE.is_met(4, 5));
        assert!(ThresholdParam::FOUR_F_PLUS_ONE.is_met(5, 5));

        // With total=10 (so f=1.8, but using integer math)
        // f+1: needs > 2, so >= 3
        assert!(!ThresholdParam::F_PLUS_ONE.is_met(2, 10));
        assert!(ThresholdParam::F_PLUS_ONE.is_met(3, 10));

        // 2f+1: needs > 4, so >= 5
        assert!(!ThresholdParam::TWO_F_PLUS_ONE.is_met(4, 10));
        assert!(ThresholdParam::TWO_F_PLUS_ONE.is_met(5, 10));

        // 4f+1: needs > 8, so >= 9
        assert!(!ThresholdParam::FOUR_F_PLUS_ONE.is_met(8, 10));
        assert!(ThresholdParam::FOUR_F_PLUS_ONE.is_met(9, 10));
    }

    #[test]
    #[should_panic(expected = "attempt to multiply with overflow")]
    fn threshold_param_is_met_overflow() {
        assert!(!ThresholdParam::TWO_F_PLUS_ONE.is_met(1, u64::MAX));
    }
}
