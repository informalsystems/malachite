# FaB-a-la-Tendermint-bounded-square Implementation Guide

## Project Overview
This project implements **FaB-a-la-Tendermint-bounded-square**, a Byzantine Fault Tolerant consensus algorithm, on top of the **Malachite** codebase. Malachite currently implements the standard Tendermint consensus algorithm (n=3f+1, 3 round steps).

## Goal

Transform Malachite from implementing Tendermint to implementing FaB-a-la-Tendermint-bounded-square. This means:
- **It's okay to break existing Tendermint functionality** - we're replacing it, not extending it
- We **not** care about backwards compatibility.
- Do **not** (unless explicitly told to do so) jump to a next step/phase unless you have comleted all the previous steps/phases.
- Focus on correctness over performance initially

## Key Algorithm Differences

### Tendermint (Current - n=3f+1)
- **Round Steps**: Propose → Prevote → Precommit → Decide
- **Message Types**: PROPOSAL, PREVOTE, PRECOMMIT
- **Quorum**: 2f+1 (simple majority)
- **Decision**: Requires 2f+1 PRECOMMIT messages

### FaB-a-la-Tendermint-bounded-square (Target - n=5f+1)
- **Round Steps**: Prepropose → Propose → Prevote → Decide (NO PRECOMMIT!)
- **Message Types**: PROPOSAL, PREVOTE only. Note that PROPOSAL messages include a certificate that can contain up to 4f+1 PREVOTE messages
- **Quorums**: 2f+1 and 4f+1 (different thresholds)
- **Decision**: Requires 4f+1 PREVOTE messages
- **SafeProposal**: More complex validation with 2f+1 or 4f+1 PREVOTE certificates

## Essential Documents

Important documents can be found under `important_files` directory. Specifically,
1. `important_files/FaB-a-la-Tendermint-bounded-square.md` the pseudocode behind the 5f+1 FaB algorithm.
2. `important_files/tendermint5f_algorithm.qnt` a Quint specification of the 5f+1 FaB algorithm.
3. `important_files/latest_gossip_in_consensus.pdf` Original Tendermint paper. Algorithm 1 presented in this paper has line numbers that are used in `/code/crates/core-state-machine/src/state_machine.rs`.

## Malachite Architecture

Malachite separates consensus into three main components:

### 1. State Machine (`code/crates/core-state-machine`)
- Implements the consensus logic for a single round
- Main file: `src/state_machine.rs`
- State file: `src/state.rs`
- **Current Steps**: `Propose`, `Prevote`, `Precommit`, `Commit`
- **FaB Steps**: `Propose`, `Prevote`, `Commit` (remove Precommit)

### 2. Vote Keeper (`code/crates/core-votekeeper`)
- Aggregates votes and tracks quorums
- Main file: `src/keeper.rs`
- **Current Thresholds**: 2f+1 for both prevote and precommit
- **FaB Thresholds**: Need both 2f+1 and 4f+1 for prevotes

### 3. Driver (`code/crates/core-driver`)
- Coordinates between state machine and vote keeper
- Handles message processing
- Main file: `src/driver.rs`

### 4. Core Types (`code/crates/core-types`)
- Defines fundamental types: Vote, VoteType, Timeout, Step, etc.
- Files to modify: `src/vote.rs`, `src/timeout.rs`


## Step-by-Step Implementation Plan

### Phase 1: Core Types Modifications
**Goal**: Remove anything related to the 3f+1 implementation and add everything needed for the 5f+1 implementation based on the 5f+1 pseudocode provided, as well as the 5f+1 Quint spect provided in `important_files`.

1. First devise a plan on what needs to be removed. Then remove.

2. Then devise a plan and what needs to be added. Then add.

**Testing**: Ensure code compiles (will have many errors to fix next)

### Phase 2: State Machine Modifications
**Goal**: Implement 2-step consensus logic


3. **Update State struct** (`code/crates/core-state-machine/src/state.rs`) to take into account the `Initialization` part of `important_files/FaB-a-la-Tendermint-bounded-square.md`. For example, what are the new `Step`s now in `code/crates/core-state-machine/src/state.rs`? Add those steps and remove any that are not needed anymore such as `Precommit`.
4. Remove anything not needed (e.g., things that appear in Algorithm 1 ) but that we do not need here.

For anything you do in the following 2 steps, if you add or modify something, I want you to introduce comments in code that explain what you're doing. Think of good names for inputs, outputs, etc.
5. Introduce relevant inputs in `code/crates/core-state-machine/src/input.rs` (feel free to use the Quint specification `important_files/tendermint5f_algorithm.qnt`` for inspiration.)
6. Introduce relevant outputs in `code/crates/core-state-machine/src/output.rs` (feel free to use the Quint specification `important_files/tendermint5f_algorithm.qnt`` for inspiration.)
7. Remove any Inputs and Outputs that are not needed anymore and that were only related to the previous 3f+1 implementation.
8. **Modify state machine transitions** (`code/crates/core-state-machine/src/state_machine.rs`) to capture the algorithm as described in the Quint specification (`important_files/tendermint5f_algorithm.qnt`), as well as the code in `important_files/FaB-a-la-Tendermint-bounded-square.md`.

**Testing**: State machine unit tests should pass with new logic


### Phase 3
We'll do this at some later point in time.


## Key FaB Algorithm Rules (from md file)

### Initialization
```
h_p = 0
round_p = 0
step_p = nil
decision_p[] = nil
max_rounds[] = -1
prevotedValue_p = nil
prevotedProposalMsg_p = nil
lastPrevote_p = nil
```

### Main Rules

**StartRound(round)**: 
- Set round_p = round
- Set step_p = propose
- Schedule timeoutPropose
- If proposer: move to prepropose step (wait for 4f+1 prevotes)

**Upon receiving 4f+1 PREVOTE (with 2f+1 for same value v)**:
- Proposer moves to propose step
- Broadcast PROPOSAL with value v and certificate S

**Upon receiving valid PROPOSAL**:
- Move to prevote step
- If SafeProposal: prevote for v
- Else: prevote for prevotedValue_p

**SafeProposal checks**:
1. If ∃ 2f+1 PREVOTE for v'' in S: return id(v'') == id(v) AND Valid(v)
2. Else if |S| == 4f+1 AND all from rounds ≥ round_p-1: return Valid(v)
3. Else if S == {} AND r == 0: return Valid(v)
4. Else: return FALSE

**Upon PROPOSAL + 4f+1 PREVOTE for same value v**:
- Decide v
- reliable_broadcast DECISION

## Important Notes

- **Breaking changes are acceptable** - we're replacing Tendermint with FaB
- **Work incrementally** - make sure each phase compiles before moving to next
- **Reference the txt file** - it has the complete algorithm pseudocode
- **Update comments** - note where FaB differs from Tendermint
- **Keep git history clean** - commit after each major step

## File Structure Reference

```
code/crates/
├── core-types/           # Basic types (Vote, Timeout, etc.)
│   └── src/
│       ├── vote.rs       # VoteType enum - REMOVE Precommit
│       └── timeout.rs    # TimeoutKind enum - REMOVE Precommit
│
├── core-state-machine/   # Consensus state machine
│   └── src/
│       ├── state.rs      # State struct and Step enum
│       ├── state_machine.rs  # Main consensus logic
│       └── input.rs      # Input events to state machine
│
├── core-votekeeper/      # Vote aggregation
│   └── src/
│       └── keeper.rs     # Track 2f+1 and 4f+1 quorums
│
└── core-driver/          # Coordinates everything
    └── src/
        ├── driver.rs     # Main driver logic
        └── lib.rs        # ThresholdParams
```

## Compilation Strategy

After each phase, run:
```bash
cd code
cargo check --all
cargo test --all
```

Fix ALL compilation errors before moving to next phase.

## Questions to Consider

1. How to handle the `prepropose` step in FaB (waiting for 4f+1 prevotes before proposing)?
2. How to store and validate PREVOTE certificates in proposals?
3. How to implement reliable_broadcast for DECISION messages?
4. Should we update validator set handling for n=5f+1?

## Success Criteria

- [ ] Code compiles without Precommit references
- [ ] State machine has 2 steps: Propose → Prevote → Decide


## Getting Started

Start with **Phase 1** - modify the core types. This will cause many compilation errors, which will guide you to all the places that need updates. Work through each error systematically.

Good luck! 🚀
