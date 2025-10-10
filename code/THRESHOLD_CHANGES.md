# Threshold Changes for FaB-a-la-Tendermint-bounded-square

## Critical Fix: Threshold Fractions Updated for n=5f+1

### Old (Tendermint - n=3f+1)
```
Total validators: n = 3f+1
Quorum (2f+1): 2/3 of validators
Honest (f+1): 1/3 of validators
```

### New (FaB - n=5f+1)
```
Total validators: n = 5f+1
Certificate quorum (4f+1): 4/5 of validators
Lock quorum (2f+1): 2/5 of validators
Honest (f+1): 1/5 of validators
```

## Changes Made

### 1. ThresholdParam Constants (`core-types/src/threshold.rs`)

**BEFORE (INCORRECT for FaB):**
```rust
pub const TWO_F_PLUS_ONE: Self = Self::new(2, 3);  // ❌ Wrong for n=5f+1
pub const F_PLUS_ONE: Self = Self::new(1, 3);      // ❌ Wrong for n=5f+1
```

**AFTER (CORRECT for FaB):**
```rust
/// 2f+1, ie. more than two fifths of the total weight (n=5f+1)
/// FaB: Used for detecting locks within 4f+1 certificates
pub const TWO_F_PLUS_ONE: Self = Self::new(2, 5);  // ✅ 2/5

/// f+1, ie. more than one fifth of the total weight (n=5f+1)
/// FaB: Used for round skipping when receiving f+1 votes from higher round
pub const F_PLUS_ONE: Self = Self::new(1, 5);      // ✅ 1/5

/// 4f+1, ie. more than four fifths of the total weight (n=5f+1)
/// FaB: Required for certificates and decisions
pub const FOUR_F_PLUS_ONE: Self = Self::new(4, 5); // ✅ 4/5
```

### 2. Example Calculations

For **n=5 validators** (so f=0.8, rounds to f=1):
- **f+1 = 2**: Need > 1/5 of 5 = > 1, so >= 2 validators
- **2f+1 = 3**: Need > 2/5 of 5 = > 2, so >= 3 validators
- **4f+1 = 5**: Need > 4/5 of 5 = > 4, so >= 5 validators (all!)

For **n=10 validators** (so f=1.8, rounds to f=2):
- **f+1 = 3**: Need > 1/5 of 10 = > 2, so >= 3 validators
- **2f+1 = 5**: Need > 2/5 of 10 = > 4, so >= 5 validators
- **4f+1 = 9**: Need > 4/5 of 10 = > 8, so >= 9 validators

### 3. How FaB Uses These Thresholds

#### f+1 (1/5)
- **Purpose**: Detect minority (f+1 honest validators)
- **Use**: Round skipping - if we see f+1 prevotes from a higher round, skip to that round
- **Why**: Guarantees at least 1 honest validator has moved to higher round

#### 2f+1 (2/5)
- **Purpose**: Detect locks on values within certificates
- **Use**: SafeProposal validation - check if a 4f+1 certificate contains 2f+1 for same value
- **Why**: If 2f+1 prevoted for v, at least f+1 honest validators prevoted for v (they won't prevote for v'≠v in higher rounds)

#### 4f+1 (4/5)
- **Purpose**: Build certificates for proposals and decisions
- **Use**:
  - Proposer needs 4f+1 prevotes to propose
  - Decision needs 4f+1 prevotes for same value
  - EnoughPrevotesForRound needs 4f+1 total prevotes
- **Why**: Guarantees at least 3f+1 honest validators participated (majority of honest validators in n=5f+1 system)

## Tests Updated

All tests in `threshold.rs` have been updated to test the correct n=5f+1 fractions with detailed comments explaining the math.
