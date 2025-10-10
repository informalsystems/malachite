# Driver Transformation Progress - FaB 5f+1

## Summary
Systematically transforming the driver from Tendermint (3f+1) to FaB-a-la-Tendermint-bounded-square (5f+1).

## Completed Work

### ✅ Step 1: Removed Precommit Fields
**Files Modified**: `driver.rs`
- Removed `last_precommit` field
- Added FaB comments explaining no precommits in FaB

### ✅ Step 2: Removed Old Certificate Types
**Files Modified**: `driver.rs`
- Removed imports: `CommitCertificate`, `PolkaCertificate`, `PolkaSignature`, `Value`
- Removed fields: `commit_certificates`, `polka_certificates`
- Added comments explaining FaB uses 4f+1 certificates built on-demand from vote_keeper

### ✅ Step 3: Removed Old Methods
**Files Modified**: `driver.rs`
- Removed `step_is_precommit()` method (no precommit step in FaB)
- Replaced `valid_value()` with `prevoted_value()` (FaB terminology)

### ✅ Step 4: Rewrote multiplex_proposal()
**Files Modified**: `mux.rs`
- **Before**: Complex Tendermint logic with Polka/Precommit handling (~100 lines)
- **After**: Simple FaB logic (~40 lines)

**New Logic**:
1. Check if valid proposal + 4f+1 prevotes → `CanDecide`
2. Otherwise, valid proposal for current round → `Proposal`
3. Invalid/out-of-round → None

**Key Changes**:
- Removed all "Polka" logic (PolkaPrevious, PolkaCurrent, PolkaAny)
- Removed all "Precommit" logic (PrecommitValue, PrecommitAny)
- Removed InvalidProposal handling
- Simplified: SafeProposal validation happens in state machine, not driver

### ✅ Step 5: Rewrote multiplex_vote_keeper_output()
**Files Modified**: `mux.rs`
- **Before**: Complex Tendermint logic handling 6 output types (~70 lines)
- **After**: Simple FaB logic handling 3 output types (~60 lines)

**Vote Keeper Outputs Handled**:
1. `CertificateAny` → `EnoughPrevotesForRound` (schedule prevote timeout)
2. `CertificateValue(v)` → `CanDecide` (if we have proposal) or `EnoughPrevotesForRound`
3. `SkipRound(r)` → `SkipRound` (with certificate)

**Removed Outputs**:
- PolkaAny, PolkaNil, PolkaValue (2f+1 thresholds - no longer emitted)
- PrecommitAny, PrecommitValue (no precommits in FaB)

**Method Signature Change**:
- Changed return type from `(Round, RoundInput<Ctx>)` to `Option<(Round, RoundInput<Ctx>)>`
- Allows returning None when certificate can't be built

### ✅ Updated mux.rs Documentation
**Files Modified**: `mux.rs`
- Removed Tendermint multiplexing table
- Added FaB multiplexing table with FaB algorithm line references
- Updated module doc comment to explain FaB logic

## Current Status

**Compilation Errors**: ~20-30 (down from 53)

**Remaining Errors**:
- References to removed certificate storage methods
- Method signature mismatches (multiplex_vote_threshold now returns Option)
- Timeout handling still references precommit
- Helper methods that need removal/updating

## Next Steps

### Step 6: Update Timeout Handling
- Remove `TimeoutKind::Precommit`
- Update timeout handling in driver.rs

### Step 7: Remove Old Certificate Storage Methods
- Remove `store_and_multiplex_commit_certificate()`
- Remove `store_polka_certificate()`
- Remove `commit_certificate()` helper
- Remove `polka_certificates()` helper

### Step 8: Fix Remaining Compilation Errors
- Update all call sites of `multiplex_vote_threshold()`
- Fix `multiplex_step_change()` logic
- Update helper methods

### Step 9: Test Compilation
- Ensure driver compiles successfully
- Run vote keeper tests (blocked until driver compiles)

## Design Decisions Made

### 1. Certificate Storage
**Decision**: Remove all certificate storage from driver
**Rationale**: FaB certificates are built on-demand from vote_keeper when needed

### 2. Proposal Validation
**Decision**: Move SafeProposal validation to state machine
**Rationale**: Clearer separation - driver just routes messages, state machine implements protocol logic

### 3. Multiplexing Simplification
**Decision**: Drastically simplify multiplex logic
**Rationale**: FaB is simpler than Tendermint - no need for complex Polka/Precommit routing

### 4. Method Signatures
**Decision**: Return Option from multiplex_vote_threshold()
**Rationale**: Some cases genuinely can't build certificates (shouldn't happen, but defensive)

## Files Modified So Far

```
crates/core-driver/src/driver.rs
crates/core-driver/src/mux.rs
```

## Lines of Code Removed

Approximately **150+ lines** of Tendermint-specific code removed so far.
