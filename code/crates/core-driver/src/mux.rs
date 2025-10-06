//! FaB: Multiplexer for FaB-a-la-Tendermint-bounded-square
//!
//! The Multiplexer is responsible for multiplexing input data and returning appropriate events to the Round State Machine.
//!
//! Input sources:
//! - Proposals from the Driver
//! - Vote Keeper outputs (CertificateAny, CertificateValue, SkipRound)
//! - Step changes from the Round State Machine
//!
//! FaB Multiplexing Logic:
//!
//! | Step     | Vote Keeper Output     | Proposal Status   | Multiplexed Input to SM  | New Step  | FaB Line  | Notes                              |
//! |--------- |----------------------- | ----------------- | ------------------------ | --------- | --------- | ---------------------------------- |
//! | any      | CertificateValue(v)    | Valid Proposal(v) | CanDecide                | commit    | 72-74     | Decide v (no step restriction)     |
//! | prevote  | CertificateValue(v)    | No proposal       | EnoughPrevotesForRound   | prevote   | 69-70     | Schedule prevote timeout           |
//! | prevote  | CertificateAny         | *                 | EnoughPrevotesForRound   | prevote   | 69-70     | Schedule prevote timeout           |
//! | any      | SkipRound(r)           | *                 | SkipRound                | propose   | 95-96     | Skip to round r (no step restriction)|
//! | propose  | (no threshold)         | Valid Proposal    | Proposal                 | prevote   | 51-59     | Validate SafeProposal, then prevote|

use alloc::vec::Vec;

use malachitebft_core_state_machine::input::Input as RoundInput;
use malachitebft_core_state_machine::state::Step;
// FaB: Removed CommitCertificate and PolkaCertificate imports (3f+1 Tendermint concepts)
use malachitebft_core_types::{SignedProposal};
use malachitebft_core_types::{Context, Proposal, Round, Validity, Value, ValueId};
use malachitebft_core_votekeeper::keeper::Output as VKOutput;

use crate::Driver;

impl<Ctx> Driver<Ctx>
where
    Ctx: Context,
{
    // FaB: Process a received proposal for FaB-a-la-Tendermint-bounded-square
    ///
    /// In FaB, proposal processing is simpler than Tendermint:
    ///
    /// 1. Check if we have a valid proposal + 4f+1 prevotes for it → CanDecide
    /// 2. Otherwise, if it's a valid proposal for current round → Proposal
    /// 3. Invalid or out-of-round proposals → None (ignore)
    ///
    /// SafeProposal validation is done by the state machine when it receives the Proposal input.
    pub(crate) fn multiplex_proposal(
        &mut self,
        proposal: Ctx::Proposal,
        validity: Validity,
    ) -> Option<RoundInput<Ctx>> {
        // FaB: Should only receive proposals for our height
        assert_eq!(self.height(), proposal.height());

        // FaB: Check that there is an ongoing round
        if self.round_state.round == Round::Nil {
            return None;
        }

        // FaB: Ignore invalid proposals
        if !validity.is_valid() {
            return None;
        }

        // FaB: Check if we can decide (FaB lines 72-74)
        // Condition: valid proposal + 4f+1 prevotes for same value → DECIDE
        if self.round_state.decision.is_none() {
            // FaB: Try to build a certificate for this value
            if let Some(certificate) = self
                .vote_keeper
                .build_certificate(proposal.round(), &proposal.value().id())
            {
                // FaB: We have 4f+1 prevotes for this value → CanDecide!
                return Some(RoundInput::CanDecide {
                    proposal,
                    certificate,
                });
            }
        }

        // FaB: If proposal is not for current round, ignore it
        if self.round_state.round != proposal.round() {
            return None;
        }

        // FaB: Valid proposal for current round → send to state machine
        // FaB: State machine will validate SafeProposal (lines 51-67) and prevote
        Some(RoundInput::Proposal(proposal))
    }

    pub(crate) fn store_and_multiplex_proposal(
        &mut self,
        signed_proposal: SignedProposal<Ctx>,
        validity: Validity,
    ) -> Option<RoundInput<Ctx>> {
        // Should only receive proposals for our height.
        assert_eq!(self.height(), signed_proposal.height());

        let proposal = signed_proposal.message.clone();

        // Store the proposal and its validity
        self.proposal_keeper
            .store_proposal(signed_proposal, validity);

        self.multiplex_proposal(proposal, validity)
    }

    // FaB: Removed store_and_multiplex_commit_certificate() - Tendermint 3f+1 concept
    // FaB: Removed store_and_multiplex_polka_certificate() - Tendermint 3f+1 concept
    // FaB: In FaB, certificates are 4f+1 prevotes built on-demand from vote_keeper
    // FaB: No need to store them separately

    // FaB: Process vote keeper outputs for FaB-a-la-Tendermint-bounded-square
    ///
    /// Vote keeper emits 3 types of outputs:
    /// - CertificateAny: 4f+1 prevotes total (need to check for 2f+1 locks)
    /// - CertificateValue(v): 4f+1 prevotes for value v (can decide if we have proposal)
    /// - SkipRound(r): f+1 prevotes from higher round r
    ///
    /// Step restrictions based on FaB algorithm:
    /// - CertificateAny/CertificateValue → EnoughPrevotesForRound: only at prevote step (line 69-70)
    /// - CertificateValue → CanDecide: any step (no restriction, line 72-74)
    /// - SkipRound: any step (no restriction, line 95-96)
    pub(crate) fn multiplex_vote_threshold(
        &mut self,
        new_threshold: VKOutput<ValueId<Ctx>>,
        threshold_round: Round,
    ) -> Option<(Round, RoundInput<Ctx>)> {
        match new_threshold {
            // FaB: 4f+1 prevotes for any values (possibly distributed)
            // FaB: Signal state machine that we have enough prevotes for this round (line 69-70)
            // FaB: Only at prevote step
            VKOutput::CertificateAny => {
                // FaB: Special case - proposer at Prepropose step waiting for 4f+1 prevotes from prev round
                // FaB: Lines 39-49: proposer needs certificate from rounds >= previous round to propose
                let prev_round = Round::new(self.round().as_i64().saturating_sub(1) as u32);
                if self.round_state.step == Step::Prepropose && threshold_round >= prev_round {
                    // FaB: Proposer has 4f+1 prevotes from rounds >= previous round
                    // FaB: Build certificate from rounds >= prev_round (Lines 39, 45: "r >= round_p-1")
                    if let Some((certificate, cert_round)) = self.vote_keeper.build_certificate_from_rounds_gte(prev_round) {
                        let current_round = self.round();

                        // FaB: Check if certificate contains 2f+1 lock on any value
                        if let Some(value_id) = self.vote_keeper.find_lock_in_certificate(&certificate) {
                            // FaB: Has 2f+1 lock on value_id → LeaderProposeWithLock (line 40-43)
                            // FaB: Need to find the locked value from cached proposals
                            // FaB: Search in all rounds from prev_round to cert_round
                            for search_round in prev_round.as_i64()..=cert_round.as_i64() {
                                if let Some((signed_proposal, validity)) =
                                    self.proposal_and_validity_for_round_and_value(Round::new(search_round as u32), value_id.clone())
                                {
                                    if validity.is_valid() {
                                        // FaB: Found the locked value → propose with lock
                                        return Some((
                                            current_round,
                                            RoundInput::LeaderProposeWithLock {
                                                value: signed_proposal.message.value().clone(),
                                                certificate,
                                                certificate_round: Round::new(search_round as u32),
                                            },
                                        ));
                                    }
                                }
                            }

                            // FaB: TODO: If we don't have the locked value cached, we should request it
                            // FaB: For now, fall through to no-lock case (safety violation risk!)
                            // warn!("Proposer has 2f+1 lock on value_id {:?} but doesn't have the value cached!", value_id);
                        }

                        // FaB: No 2f+1 lock (or couldn't find locked value) → LeaderProposeWithoutLock (line 45-49)
                        // FaB: Pass value=None to trigger GetValue request
                        return Some((
                            current_round,
                            RoundInput::LeaderProposeWithoutLock {
                                value: None,
                                certificate,
                            },
                        ));
                    }
                }

                // FaB: Check if we're at prevote step (line 69-70 condition)
                if threshold_round == self.round() && self.round_state.step == Step::Prevote {
                    Some((threshold_round, RoundInput::EnoughPrevotesForRound))
                } else {
                    None
                }
            }

            // FaB: 4f+1 prevotes for specific value v
            // FaB: Check if we have a matching proposal → CanDecide (lines 72-74)
            VKOutput::CertificateValue(ref value_id) => {
                // FaB: Check if we have a valid proposal for this value
                // FaB: CanDecide has no step restriction (line 72-74)
                if let Some((signed_proposal, validity)) =
                    self.proposal_and_validity_for_round_and_value(threshold_round, value_id.clone())
                {
                    if validity.is_valid() {
                        // FaB: Build the certificate for this value
                        if let Some(certificate) =
                            self.vote_keeper.build_certificate(threshold_round, value_id)
                        {
                            // FaB: We can decide! (any step)
                            return Some((
                                threshold_round,
                                RoundInput::CanDecide {
                                    proposal: signed_proposal.message.clone(),
                                    certificate,
                                },
                            ));
                        }
                    }
                }

                // FaB: No valid proposal yet, but we have certificate
                // FaB: Only signal EnoughPrevotesForRound if at prevote step (line 69-70)
                if threshold_round == self.round() && self.round_state.step == Step::Prevote {
                    Some((threshold_round, RoundInput::EnoughPrevotesForRound))
                } else {
                    None
                }
            }

            // FaB: f+1 prevotes from higher round → skip to that round (lines 95-96)
            // FaB: No step restriction
            VKOutput::SkipRound(new_round) => {
                // FaB Lines 92-96: Build f+1 certificate (not 4f+1!) justifying the skip
                // max_round+ requires f+1 voting power, NOT 4f+1
                if let Some(certificate) = self.vote_keeper.build_skip_round_certificate(new_round) {
                    Some((
                        new_round,
                        RoundInput::SkipRound {
                            round: new_round,
                            certificate,
                        },
                    ))
                } else {
                    // FaB: Can't build certificate, shouldn't happen
                    None
                }
            }
        }
    }

    // FaB: After a step change, check for cached proposals AND vote thresholds that need processing
    /// FaB: Simplified for FaB-a-la-Tendermint-bounded-square
    ///
    /// When entering Prepropose step, proactively check if we already have 4f+1 prevotes from previous round
    pub(crate) fn multiplex_step_change(&mut self, round: Round) -> Vec<(Round, RoundInput<Ctx>)> {
        let mut result = Vec::new();

        let step = self.round_state().step;

        // FaB: Special case - proposer entering Prepropose step (round > 0)
        // FaB: Proactively check if we already have 4f+1 prevotes from rounds >= previous round
        if step == Step::Prepropose && round > Round::ZERO {
            let prev_round = Round::new(round.as_i64().saturating_sub(1) as u32);

            // FaB Lines 39, 45: Check if we have 4f+1 prevotes from rounds >= prev_round
            if let Some((certificate, cert_round)) = self.vote_keeper.build_certificate_from_rounds_gte(prev_round) {
                // Check if certificate contains 2f+1 lock on any value
                if let Some(value_id) = self.vote_keeper.find_lock_in_certificate(&certificate) {
                    // FaB: Has 2f+1 lock on value_id → LeaderProposeWithLock (line 40-43)
                    // FaB: Need to find the locked value from cached proposals
                    // FaB: Search in all rounds from prev_round to cert_round
                    for search_round in prev_round.as_i64()..=cert_round.as_i64() {
                        if let Some((signed_proposal, validity)) =
                            self.proposal_and_validity_for_round_and_value(Round::new(search_round as u32), value_id.clone())
                        {
                            if validity.is_valid() {
                                // FaB: Found the locked value → propose with lock
                                result.push((
                                    round,
                                    RoundInput::LeaderProposeWithLock {
                                        value: signed_proposal.message.value().clone(),
                                        certificate,
                                        certificate_round: Round::new(search_round as u32),
                                    },
                                ));
                                return result; // Exit early, we have the input
                            }
                        }
                    }
                }

                // FaB: No 2f+1 lock (or couldn't find locked value) → LeaderProposeWithoutLock
                // FaB: Pass value=None to trigger GetValue request
                result.push((
                    round,
                    RoundInput::LeaderProposeWithoutLock {
                        value: None,
                        certificate,
                    },
                ));
                return result; // Exit early, we have the input
            }
        }

        // FaB: Check if we have cached proposals for this round that need reprocessing
        let proposals = self.proposals_and_validities_for_round(round).to_vec();

        for (signed_proposal, validity) in proposals {
            let proposal = &signed_proposal.message;

            // FaB: At propose step, reprocess proposals (line 51-59)
            if step == Step::Propose {
                if let Some(input) = self.multiplex_proposal(proposal.clone(), validity) {
                    result.push((self.round(), input))
                }
            }

            // FaB: No other step-specific processing needed
        }

        result
    }
}

// FaB: Removed all Tendermint threshold helper functions:
// - find_non_value_threshold() - not needed, vote_keeper emits outputs automatically
// - has_polka_value() - not needed in FaB
// - has_polka_nil() - not needed in FaB
// - has_polka_any() - not needed in FaB
// - has_precommit_any() - no precommits in FaB
//
// FaB: In FaB, the vote_keeper automatically emits CertificateAny, CertificateValue, or SkipRound
// when thresholds are reached, so manual threshold checking is not needed.
