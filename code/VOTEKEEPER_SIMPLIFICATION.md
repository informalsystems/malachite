# Vote Keeper Simplification - Removing Redundant Quorum Outputs

## The Problem

Initially, the vote keeper was emitting outputs for BOTH 2f+1 and 4f+1 thresholds:
- `QuorumAny`, `QuorumNil`, `QuorumValue` (2f+1 thresholds)
- `CertificateAny`, `CertificateValue` (4f+1 thresholds)

This was **redundant** because the driver already needs to analyze certificates to validate `SafeProposal`.

## The Solution

The vote keeper now ONLY emits outputs for 4f+1 certificates:
- `CertificateAny` - 4f+1 prevotes total (possibly distributed across values)
- `CertificateValue(v)` - 4f+1 prevotes for specific value v
- `SkipRound(r)` - f+1 prevotes from higher round r

The driver uses the new `find_lock_in_certificate()` method to detect 2f+1 locks within certificates.

## How the Driver Uses This

```rust
// When proposer receives CertificateAny:
let certificate = vote_keeper.build_certificate_any(round)?;

// Check if there's a 2f+1 lock within the certificate
if let Some(locked_value_id) = vote_keeper.find_lock_in_certificate(&certificate) {
    // There's a lock - use LeaderProposeWithLock
    state_machine.apply(Input::LeaderProposeWithLock {
        value: get_value_by_id(locked_value_id),
        certificate,
        certificate_round: round,
    });
} else {
    // No lock - use LeaderProposeWithoutLock
    state_machine.apply(Input::LeaderProposeWithoutLock {
        certificate,
    });
}
```

## Benefits

1. **Simpler API**: Fewer output variants to handle
2. **Clearer separation**: Vote keeper detects thresholds, driver analyzes content
3. **More flexible**: Driver can implement custom lock detection logic if needed
4. **Matches FaB spec**: The FaB algorithm describes checking certificates, not separate 2f+1 events

## Implementation Details

### New Method: `find_lock_in_certificate()`

```rust
pub fn find_lock_in_certificate(
    &self,
    certificate: &[SignedVote<Ctx>],
) -> Option<ValueId<Ctx>>
```

This method:
- Takes a certificate (slice of votes)
- Counts voting power for each value
- Returns the value_id if any value has >= 2f+1 voting power
- Returns None if votes are distributed (no lock)

### Why This is Better

**Before**: Vote keeper emitted 6 different output types, requiring complex state tracking in the driver

**After**: Vote keeper emits 3 output types, driver analyzes certificates when needed

The vote keeper's job is to detect when thresholds are met, not to interpret what those thresholds mean for the protocol logic.
