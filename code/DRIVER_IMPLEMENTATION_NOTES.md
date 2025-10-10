# Driver Implementation Notes for FaB

## Vote Keeper Integration

### Certificate Detection Logic

The vote keeper emits these outputs:

1. **`CertificateAny`**: We have 4f+1 prevotes total (driver should check for locks within it)
2. **`CertificateValue(v)`**: We have 4f+1 prevotes for specific value `v`
3. **`SkipRound(r)`**: We have f+1 prevotes from higher round `r`

Note: The vote keeper does NOT emit separate outputs for 2f+1 thresholds. The driver analyzes certificates using `find_lock_in_certificate()` to detect 2f+1 locks.

### Proposer Decision Logic

When the proposer receives 4f+1 prevotes (in PrePropose step), the driver must determine which state machine Input to send:

#### Case 1: LeaderProposeWithLock (FaB lines 39-43)
**Condition**: 4f+1 prevotes WITH 2f+1 for same value

**Vote Keeper Signals**:
- Received `CertificateAny` OR `CertificateValue(v)`

**Driver Action**:
```rust
// Build certificate from 4f+1 prevotes
let certificate = vote_keeper.build_certificate_any(round)?;

// Check if there's a 2f+1 lock within the certificate
if let Some(locked_value_id) = vote_keeper.find_lock_in_certificate(&certificate) {
    // There's a 2f+1 lock - use LeaderProposeWithLock
    Input::LeaderProposeWithLock {
        value: locked_value,
        certificate,
        certificate_round: round,
    }
} else {
    // No lock - use LeaderProposeWithoutLock
    Input::LeaderProposeWithoutLock {
        certificate,
    }
}
```

#### Case 2: LeaderProposeWithoutLock (FaB lines 45-49)
**Condition**: 4f+1 prevotes WITHOUT any 2f+1 lock

This case is handled in the `else` branch above - when `find_lock_in_certificate()` returns `None`.

### Important Notes

1. **The vote keeper provides the raw data** (which thresholds are met)
2. **The driver implements the logic** (which Input to send based on those thresholds)
3. **SafeProposal validation** happens in the driver before sending `Input::Proposal`
4. The driver must track which outputs have been received to make the right decision

## Other Driver Responsibilities

- SafeProposal validation (FaB lines 61-67)
- Rebroadcast logic (OnTimeoutRebroadcast - FaB lines 108-113)
- `max_rounds` tracking (currently not in state machine or vote keeper)
- Height transitions (create new state machine instances after decisions)
- Message broadcasting and timeout scheduling
