# Driver Transformation Status - FaB 5f+1

## Compilation Progress

**Starting Point**: 53 compilation errors
**Current Status**: 25 compilation errors
**Progress**: **53% reduction in errors!**

## Completed Work (Step by Step)

### ✅ Step 1: Remove Precommit Fields
- Removed `last_precommit` field
- Added FaB comments explaining absence of precommits

### ✅ Step 2: Remove Old Certificate Types
- Removed imports: `CommitCertificate`, `PolkaCertificate`, `PolkaSignature`
- Removed fields: `commit_certificates`, `polka_certificates`
- Documented that FaB builds certificates on-demand

### ✅ Step 3: Remove Old Methods
- Removed `step_is_precommit()` method
- Replaced `valid_value()` with `prevoted_value()`

### ✅ Step 4: Simplify multiplex_proposal()
**Before**: ~100 lines of complex Tendermint logic
**After**: ~40 lines of simple FaB logic

**New Logic**:
```rust
// 1. Check if valid proposal + 4f+1 prevotes → CanDecide
// 2. Otherwise, valid proposal for current round → Proposal
// 3. Invalid/out-of-round → None
```

### ✅ Step 5: Rewrite multiplex_vote_keeper_output()
**Before**: Handled 6 output types (Polka*/Precommit*)
**After**: Handles 3 FaB outputs with **step restrictions**

**Key Implementation**:
- `CertificateAny` → `EnoughPrevotesForRound` (only at **prevote** step)
- `CertificateValue(v)` → `CanDecide` (**any** step, no restriction)
- `CertificateValue(v)` → `EnoughPrevotesForRound` (only at **prevote** step, when no proposal)
- `SkipRound(r)` → `SkipRound` (**any** step, no restriction)

**Return type change**: Now returns `Option<(Round, RoundInput<Ctx>)>` instead of `(Round, RoundInput<Ctx>)`

### ✅ Step 6: Remove Certificate Storage Methods
Removed:
- `store_and_multiplex_commit_certificate()`
- `store_and_multiplex_polka_certificate()`
- `commit_certificate()` helper
- `polka_certificates()` helper
- `store_polka_certificate()`
- `store_precommit_any_round_certificate()`
- `prune_polka_certificates()`

Simplified:
- `prune_votes_and_certificates()` now only prunes votes

### ✅ Step 7: Update Timeout Handling
**Updated**: `apply_timeout()` method

**Key Changes**:
- `TimeoutPropose` → `RoundInput::TimeoutPropose` (simple)
- `TimeoutPrevote` → `RoundInput::TimeoutPrevote { certificate }` (builds certificate from vote_keeper)
- Removed `TimeoutPrecommit` handling
- Added FaB algorithm line references

### ✅ Step 8: Update Documentation
- Updated mux.rs module doc with FaB multiplexing table
- Added "Step" and "New Step" columns
- Verified step restrictions against FaB algorithm
- Added FaB line references throughout

## Remaining Errors (25)

### Category 1: Type References (2)
- `CommitCertificate` type not found
- `PolkaCertificate` type not found

### Category 2: Method Calls to Removed Methods (4)
- `store_and_multiplex_commit_certificate()` not found
- `store_and_multiplex_polka_certificate()` not found
- `store_polka_certificate()` not found
- `store_precommit_any_round_certificate()` not found

### Category 3: Removed Vote Keeper Outputs (6)
- `PolkaValue` not found (x3)
- `PrecommitAny` not found (x2)
- `PolkaNil` not found
- `PolkaAny` not found

### Category 4: Type Mismatches (8)
- `multiplex_vote_threshold` return type mismatch (changed to Option)
- `RoundOutput::Decision` tuple/struct mismatch
- Various type mismatches from signature changes

### Category 5: Method Calls (4)
- `.id()` method not found on `Value` reference (x4)

### Category 6: Other (1)
- `ProposeValue` input variant not found

## Next Steps

### Remaining Work:
1. Fix calls to removed methods (find and remove/replace)
2. Fix helper functions still referencing old outputs
3. Fix `multiplex_step_change()` method
4. Fix type mismatches from signature changes
5. Replace `.id()` calls with proper method

### Estimated Remaining Work:
- **10-15 more compilation fixes** needed
- Most are straightforward removals/replacements
- Some require understanding call sites

## Code Quality

### Lines Removed: ~200+
### Complexity Reduction: Significant
- Removed entire certificate storage system
- Simplified multiplexing logic
- Removed Tendermint-specific outputs

### Comments Added: ~50+
- Every change explained with FaB comments
- Algorithm line references included
- Clear separation between removed (3f+1) and added (5f+1) code

## Files Modified

```
crates/core-driver/src/driver.rs  (~150 lines modified/removed)
crates/core-driver/src/mux.rs     (~120 lines modified/removed)
```

## Design Decisions

1. **Certificate Storage**: Removed entirely - build on-demand from vote_keeper
2. **Step Restrictions**: Implemented based on FaB algorithm (lines 69-70, 72-74, 95-96)
3. **Timeout Handling**: TimeoutPrevote now includes certificate
4. **Multiplexing**: Drastically simplified - FaB is simpler than Tendermint
5. **Return Types**: Changed to Option where appropriate (defensive programming)

---

**Status**: Making excellent progress - methodically transforming driver from Tendermint to FaB!
